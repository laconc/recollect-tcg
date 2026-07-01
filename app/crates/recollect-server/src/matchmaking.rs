//! Match creation/recovery + bot-difficulty helpers.
//! A sibling of `server/lib.rs`; `use super::*` shares AppState/Match + imports.
use super::*;
use crate::match_settings::{MatchSettings, Mode, SeatFill};

pub(crate) async fn create_match(
    State(app): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    axum::extract::RawQuery(raw_query): axum::extract::RawQuery,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // Behind the Cloudflare Tunnel the socket peer is one container IP; key the
    // bucket on the true client IP (CF-Connecting-IP) so clients don't share one.
    if !rate_ok(&app, client_ip(&headers, addr), 30) {
        return (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"error": "rate limited"})),
        )
            .into_response();
    }
    // Host's match settings (mode · per-seat human|bot+difficulty · faction), parsed
    // from the query — generalizes the legacy `mode=2v2`/`opponent=bot`/`difficulty=`.
    let settings = match MatchSettings::from_query(raw_query.as_deref().unwrap_or("")) {
        Ok(s) => s,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    // The match seed comes from the OS CSPRNG — never `SystemTime`, which a
    // watcher can predict to the nanosecond and grind. `?seed=N` overrides it for
    // reproducible matches — debugging, sharing, and deterministic tests — threaded via
    // MatchSettings (an explicit param, not global env / unsafe state). The seed
    // appears in NO event and NO view; only its *generation* is hardened.
    let seed = settings.seed.unwrap_or_else(crate::crypto::fresh_seed);
    // Absence-forfeit grace: the host's `?abandon_grace_secs=N` override, else the
    // default (120s). `0` disables it. Threaded into the actor (1v1 + 2v2) so a human
    // seat/slot that disconnects and doesn't reconnect within it forfeits.
    let abandon_grace = settings
        .abandon_grace
        .unwrap_or(crate::match_settings::DEFAULT_ABANDON_GRACE);
    // Commit–reveal: commit to the seed now (publish `sha256(seed‖salt)` in the
    // response), reveal `{seed, salt}` at match end — a provably-fair shuffle no side
    // can rig. Cheap and seed-source-agnostic, so a `?seed=`-pinned match is provable too.
    let seed_commit = SeedCommitment::new(seed);
    // Canon Quick Play decks: derived, never dealt (seeded styles).
    let cat = recollect_core::cards::canon_catalog();
    // `?mode=2v2` opens a 6×6 lobby with four slot tokens. Postgres-
    // authoritative when a journal connection is present (the slot applies are
    // appended-before-ack on a `journal_events` stream); in-memory degrade otherwise.
    if settings.mode == Mode::TwoVsTwo {
        let o = recollect_core::quickplay::offer(seed);
        // Each seat fields a deck — a bot seat fields its faction's character (Solace by
        // disposition, Lorekeeper by style), a human seat a Quick Play style; the bot characters'
        // initiative weights the 4-way opener toss toward their own seats.
        let slots = SeatSlot::all_2v2();
        let mut decks: [Vec<recollect_core::CardId>; 4] = Default::default();
        let mut weights = [100u32; 4];
        for (i, (slot, fill)) in slots.iter().zip(settings.seats.iter()).enumerate() {
            let salt = seed.wrapping_add(i as u64);
            if matches!(fill, SeatFill::Bot(_)) {
                let faction = if slot.team() == Seat::A {
                    recollect_bot::Faction::Lorekeeper
                } else {
                    settings.opponent_faction
                };
                if matches!(faction, recollect_bot::Faction::Solace) {
                    let c =
                        (salt % recollect_core::quickplay::SOLACE_CHARACTERS.len() as u64) as usize;
                    decks[i] = recollect_core::quickplay::solace_character_deck(c, salt, &cat);
                    weights[i] +=
                        recollect_core::quickplay::SOLACE_CHARACTERS[c].initiative() as u32;
                } else {
                    let c = (salt % recollect_core::quickplay::LOREKEEPER_CHARACTERS.len() as u64)
                        as usize;
                    decks[i] = recollect_core::quickplay::lorekeeper_character_deck(c, salt, &cat);
                    weights[i] +=
                        recollect_core::quickplay::LOREKEEPER_CHARACTERS[c].initiative() as u32;
                }
            } else {
                decks[i] = recollect_core::quickplay::generate_deck(o[i % 3].id, salt, &cat);
            }
        }
        let first_slot = recollect_core::quickplay::decide_opener_2v2(seed, weights);
        // Mint one token per slot — plaintext handed out ONCE (response +
        // registry), only the hash kept on the lobby. `plain[i]` is slot `i`'s
        // one-time credential; `tokens[i]` its stored hash.
        let slot_order = [SeatSlot::A1, SeatSlot::B1, SeatSlot::A2, SeatSlot::B2];
        let minted: [(SeatToken, String); 4] = std::array::from_fn(|_| SeatToken::mint());
        let plain: [String; 4] = std::array::from_fn(|i| minted[i].1.clone());
        let tokens: [(SeatSlot, SeatToken); 4] =
            std::array::from_fn(|i| (slot_order[i], minted[i].0.clone()));
        let resp = Json(serde_json::json!({
            "match_id": id,
            "mode": "2v2",
            "protocol": PROTOCOL_VERSION,
            "slot_tokens": {
                "A1": plain[0], "B1": plain[1],
                "A2": plain[2], "B2": plain[3],
            },
            // Commit–reveal: the published seed commitment (verify against the
            // reveal pushed when the match ends).
            "seed_commit": seed_commit.commit_hex(),
        }));
        let db_id = uuid::Uuid::new_v4().to_string();
        // 2v2 today is a four-human-slot lobby (all Lorekeeper); a bot/Solace team would set B here.
        let session = Session::new_2v2_with_opener(
            seed,
            decks,
            first_slot,
            [recollect_core::types::Faction::Lorekeeper; 2],
        );
        // Record the match row (seed/result) and open the authoritative stream at
        // the post-opening entropy position. A stream-open failure drops this
        // lobby to in-memory rather than refusing play — same degrade as 1v1.
        if let Some(j) = app.journal.clone() {
            let (mid, s) = (db_id.clone(), seed as i64);
            tokio::spawn(async move {
                if let Err(e) = j.lock().await.create_match(&mid, s, None, None).await {
                    tracing::warn!(error = %e, "2v2 match row insert failed (record-only)");
                }
            });
        }
        let event_client = if let Some(client) = app.event_client.clone() {
            let (genesis, pos) = session.snapshot();
            match AsyncStore::open(client.as_ref(), db_id.clone(), &genesis, pos).await {
                Ok(_) => Some(client),
                Err(e) => {
                    tracing::error!(error = %e, "2v2 journal stream open failed; lobby runs in-memory");
                    None
                }
            }
        } else {
            None
        };
        // A journaled lobby is restart-recoverable — record its four slot
        // token HASHES (A1,B1,A2,B2) + the commit–reveal salt so `recover_match`
        // can rebuild the lobby and honour the original commitment after a restart.
        // The registry persists the DIGESTS, never the plaintext (the
        // running lobby and the DB both hold only the hash); recovery rebuilds each
        // `SeatToken` from its digest. The salt is captured here (Copy) so it can
        // ride into the spawn while `seed_commit` itself moves into the actor below.
        if event_client.is_some()
            && let Some(j) = app.journal.clone()
        {
            let reg_db = db_id.clone();
            let slot_hashes: [[u8; 32]; 4] = std::array::from_fn(|i| tokens[i].1.digest());
            let seed_salt = seed_commit.salt();
            tokio::spawn(async move {
                let refs: [&[u8]; 4] = [
                    &slot_hashes[0],
                    &slot_hashes[1],
                    &slot_hashes[2],
                    &slot_hashes[3],
                ];
                if let Err(e) = j
                    .lock()
                    .await
                    .register_match_2v2(id as i64, &reg_db, seed as i64, &refs, &seed_salt)
                    .await
                {
                    tracing::warn!(error = %e, "2v2 match registry insert failed");
                }
            });
        }
        // Map the seat settings to server-driven bots: team A (A1,A2) pilots the host's
        // faction (Lorekeeper), team B (B1,B2) the chosen opponent faction.
        let bots: Vec<(SeatSlot, SlotBot)> = SeatSlot::all_2v2()
            .into_iter()
            .zip(settings.seats.iter())
            .filter_map(|(slot, fill)| match fill {
                SeatFill::Bot(d) => {
                    let (faction, opp_faction) = if slot.team() == Seat::A {
                        (
                            recollect_bot::Faction::Lorekeeper,
                            settings.opponent_faction,
                        )
                    } else {
                        (
                            settings.opponent_faction,
                            recollect_bot::Faction::Lorekeeper,
                        )
                    };
                    Some((
                        slot,
                        SlotBot {
                            difficulty: *d,
                            faction,
                            opp_faction,
                            rng: std::sync::Mutex::new(recollect_core::rng::Rng::from_seed(
                                seed ^ (slot as u64).wrapping_mul(0xB07),
                            )),
                        },
                    ))
                }
                SeatFill::Human => None,
            })
            .collect();
        // Spawn the owning actor — it takes the session by value (no lock) and
        // fans per-slot frames out per-seat (no broadcast). The map entry keeps only
        // the routing tokens + lifecycle flag + the join-analytics ids.
        let handle = actor::spawn(
            session,
            ActorConfig {
                mode: ActorMode::TwoVsTwo { bots },
                db_id: db_id.clone(),
                journal: app.journal.clone(),
                event_client,
                seed_commit,
                abandon_grace,
            },
        );
        let lobby = Match {
            handle,
            kind: MatchKind::TwoVsTwo { tokens },
            evict: std::sync::atomic::AtomicBool::new(false),
            db_id,
            journal: app.journal.clone(),
        };
        lock_matches(&app.matches).insert(id, Arc::new(lobby));
        let has_bot = settings.seats.iter().any(|f| matches!(f, SeatFill::Bot(_)));
        crate::metrics::match_created(
            "2v2",
            has_bot,
            if has_bot { "mixed" } else { "none" },
            if matches!(settings.opponent_faction, recollect_bot::Faction::Solace) {
                "solace"
            } else {
                "lorekeeper"
            },
        );
        tracing::info!(match_id = id, mode = "2v2", "lobby created");
        return resp.into_response();
    }
    // 1v1: seat B (settings.seats[1]) is either a human (code-join) or a server-driven
    // bot. When the bot opponent is the Solace, seat B is the Solace faction — it plays its
    // Solace deck normally (the bot picks B's commands like any seat).
    let (vs_bot, bot_difficulty) = match settings.seats[1] {
        SeatFill::Bot(d) => (true, d),
        SeatFill::Human => (false, recollect_bot::Difficulty::default()),
    };
    let offers = recollect_core::quickplay::offer(seed);
    let deck_a = recollect_core::quickplay::generate_deck(offers[0].id, seed, &cat);
    let deck_b = recollect_core::quickplay::generate_deck(offers[1].id, seed.wrapping_add(1), &cat);
    let db_id = uuid::Uuid::new_v4().to_string();
    // Optional account binding: Authorization: Bearer <token> claims seat A.
    let mut account_a: Option<String> = None;
    if let (Some(j), Some(auth)) = (
        app.journal.as_ref(),
        headers.get(axum::http::header::AUTHORIZATION),
    ) && let Some(tok) = auth.to_str().ok().and_then(|v| v.strip_prefix("Bearer "))
    {
        account_a = j.lock().await.verify_token(tok).await.ok().flatten();
    }
    if let Some(j) = app.journal.clone() {
        let (mid, s, acct) = (db_id.clone(), seed as i64, account_a.clone());
        tokio::spawn(async move {
            if let Err(e) = j
                .lock()
                .await
                .create_match(&mid, s, acct.as_deref(), None)
                .await
            {
                tracing::warn!(error = %e, "match row insert failed (record-only in v0)");
            }
        });
    }
    // Vs a bot, seat B fields a seeded faction character — one of the ~20 Solace
    // (by disposition) or ~20 Lorekeeper (by style) — a real faction-pure deck the bot pilots through
    // normal play, at the chosen difficulty. No special match rules: each faction is just a faction.
    let solace_bot = vs_bot && matches!(settings.opponent_faction, recollect_bot::Faction::Solace);
    let lorekeeper_bot = vs_bot
        && matches!(
            settings.opponent_faction,
            recollect_bot::Faction::Lorekeeper
        );
    // First-player: a seeded coin-flip decides who opens; a bot character's initiative biases the
    // toss toward its seat (B) — an edge, not a guarantee. A human seat B keeps its deck + a fair toss.
    // Deterministic from the match seed → replay-verified like the rest of the genesis.
    let (deck_b, bias) = if solace_bot {
        let c = (seed % recollect_core::quickplay::SOLACE_CHARACTERS.len() as u64) as usize;
        let deck = recollect_core::quickplay::solace_character_deck(c, seed.wrapping_add(1), &cat);
        (
            deck,
            -recollect_core::quickplay::SOLACE_CHARACTERS[c].initiative(),
        )
    } else if lorekeeper_bot {
        let c = (seed % recollect_core::quickplay::LOREKEEPER_CHARACTERS.len() as u64) as usize;
        let deck =
            recollect_core::quickplay::lorekeeper_character_deck(c, seed.wrapping_add(1), &cat);
        (
            deck,
            -recollect_core::quickplay::LOREKEEPER_CHARACTERS[c].initiative(),
        )
    } else {
        (deck_b, 0)
    };
    let opener = recollect_core::quickplay::decide_opener(seed, bias);
    // Seat A is the human (Lorekeeper); seat B is the opponent. Passing B's faction lets the engine
    // tally the Solace's off-board erasures (its removals leave no impression).
    let session = Session::new_with_opener(
        seed,
        deck_a,
        deck_b,
        opener,
        [
            recollect_core::types::Faction::Lorekeeper,
            settings.opponent_faction,
        ],
    );
    // Open the authoritative journal stream: write the genesis snapshot at the
    // post-opening entropy position (shuffles/seeding have already drawn). A
    // failure here drops this match to in-memory rather than refusing play.
    let event_client = if let Some(client) = app.event_client.clone() {
        let (genesis, pos) = session.snapshot();
        match AsyncStore::open(client.as_ref(), db_id.clone(), &genesis, pos).await {
            Ok(_) => Some(client),
            Err(e) => {
                tracing::error!(error = %e, "journal stream open failed; match runs in-memory");
                None
            }
        }
    } else {
        None
    };
    // Mint per-seat tokens — plaintext handed out ONCE (response + registry),
    // only the hash kept on the match entry.
    let (token_a, token_a_plain) = SeatToken::mint();
    let (token_b, token_b_plain) = SeatToken::mint();
    let bot = vs_bot.then(|| Bot {
        seat: Seat::B,
        difficulty: bot_difficulty,
        faction: settings.opponent_faction,
        rng: std::sync::Mutex::new(recollect_core::rng::Rng::from_seed(seed ^ 0xB07)),
    });
    // Only a journaled match (its events are durable) can be rebuilt after a
    // restart, so register only those. A vs-bot match records its bot difficulty
    // so recovery re-attaches the Seat-B chooser (Seat B is always the bot).
    let recoverable = event_client.is_some();
    let bot_diff_str: Option<&'static str> = vs_bot.then(|| difficulty_str(bot_difficulty));
    let reg_db_id = db_id.clone();
    // Capture the published commitment, the seat-token DIGESTS, and the commitment
    // SALT before `seed_commit`/`token_a`/`token_b` move into the actor + map entry.
    // The registry stores the hashes (not the plaintext) and the salt, so a
    // recovered match authorises against the same digest and re-commits identically.
    let seed_commit_hex = seed_commit.commit_hex();
    let seed_salt = seed_commit.salt();
    let (token_a_hash, token_b_hash) = (token_a.digest(), token_b.digest());
    // Spawn the owning actor. It takes the session by value (no `Mutex<Session>`)
    // and fans the opponent's Update out over a per-seat sender (no `broadcast`). The
    // bot, journal-on-finish, seed reveal, and display names all move behind the actor;
    // the map entry keeps only the routing tokens + lifecycle flag + join-analytics ids.
    let handle = actor::spawn(
        session,
        ActorConfig {
            mode: ActorMode::OneVsOne {
                bot,
                names: [None, None],
            },
            db_id: db_id.clone(),
            journal: app.journal.clone(),
            event_client,
            seed_commit,
            abandon_grace,
        },
    );
    let entry = Match {
        handle,
        kind: MatchKind::OneVsOne { token_a, token_b },
        evict: std::sync::atomic::AtomicBool::new(false),
        db_id,
        journal: app.journal.clone(),
    };
    lock_matches(&app.matches).insert(id, Arc::new(entry));
    crate::metrics::match_created(
        "1v1",
        vs_bot,
        bot_diff_str.unwrap_or("none"),
        if matches!(settings.opponent_faction, recollect_bot::Faction::Solace) {
            "solace"
        } else {
            "lorekeeper"
        },
    );
    if recoverable && let Some(j) = app.journal.clone() {
        // The registry persists the seat-token HASHES + the commitment salt
        // (its schema is owned by the journal crate). Neither the running entry nor
        // the DB holds a clear-text credential; recovery rebuilds each `SeatToken`
        // from its digest and re-commits under the persisted salt.
        tokio::spawn(async move {
            if let Err(e) = j
                .lock()
                .await
                .register_match(
                    id as i64,
                    &reg_db_id,
                    seed as i64,
                    &token_a_hash,
                    &token_b_hash,
                    &seed_salt,
                    bot_diff_str,
                )
                .await
            {
                tracing::warn!(error = %e, "match registry insert failed");
            }
        });
    }
    tracing::info!(
        match_id = id,
        opponent = if vs_bot { "bot" } else { "human" },
        recoverable,
        "1v1 match created"
    );
    Json(serde_json::json!({
        "match_id": id,
        "seat_a_token": token_a_plain,
        // For a vs-bot match Seat B is the server's AI; its token is never handed out.
        "seat_b_token": if vs_bot { serde_json::Value::Null } else { token_b_plain.into() },
        "opponent": if vs_bot { "bot" } else { "human" },
        "protocol": PROTOCOL_VERSION,
        "account_a": account_a,
        // Commit–reveal: the published seed commitment (verify against the
        // reveal pushed when the match ends).
        "seed_commit": seed_commit_hex,
    }))
    .into_response()
}

/// Parse a `?difficulty=` query value into a bot tier (defaults to Normal).
pub(crate) fn parse_difficulty(s: &str) -> recollect_bot::Difficulty {
    use recollect_bot::Difficulty::*;
    match s {
        "easy" => Easy,
        "hard" => Hard,
        "expert" => Expert,
        _ => Normal,
    }
}

/// The registry string for a bot tier — the inverse of [`parse_difficulty`], so a
/// recovered vs-bot match re-attaches the same difficulty.
pub(crate) fn difficulty_str(d: recollect_bot::Difficulty) -> &'static str {
    use recollect_bot::Difficulty::*;
    match d {
        Easy => "easy",
        Normal => "normal",
        Hard => "hard",
        Expert => "expert",
    }
}

#[allow(dead_code)] // test deck helper (the AI seat uses real generated decks)
pub(crate) fn recollect_bot_deck() -> Vec<recollect_core::CardId> {
    [
        0u16, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10,
    ]
    .iter()
    .map(|i| recollect_core::CardId(*i))
    .collect()
}

/// Rebuild an in-memory [`SeatToken`] from the SHA-256 digest the `match_registry`
/// persisted. `None` if the stored bytes aren't a 32-byte digest — `recover_match`
/// then declines the row and the caller
/// 404s rather than recovering a match with an unauthorisable seat.
fn seat_token_from_hash(stored: &[u8]) -> Option<SeatToken> {
    let hash: [u8; 32] = stored.try_into().ok()?;
    Some(SeatToken::from_hash(hash))
}

/// Rebuild a journaled match the server no longer holds in memory (a restart,
/// or a swept entry) by replaying its journal stream — both 1v1 and 2v2. Returns
/// `None` if there's no database, no registry row, the replay fails, or a stored
/// hash/salt is malformed — the caller 404s. The game state (board, slot rotation,
/// entropy position) comes from the journal; the registry supplies the seed, the
/// seat-token HASHES, and the commit–reveal salt.
#[tracing::instrument(skip(app), fields(match_id = id))]
pub(crate) async fn recover_match(app: &AppState, id: u64) -> Option<Arc<Match>> {
    let journal = app.journal.as_ref()?;
    let client = app.event_client.clone()?;
    let reg = journal.lock().await.lookup_match(id as i64).await.ok()??;

    let store = AsyncStore::attach(client.as_ref(), reg.db_id.clone());
    let (agg, pos) = resume_async::<recollect_core::GameState>(&store)
        .await
        .ok()?;
    let engine = recollect_core::Engine::from_state(
        agg.state().clone(),
        reg.seed as u64,
        pos,
        recollect_core::cards::canon_catalog(),
    );
    let session = Session::from_engine(engine);
    // The registry persisted the seat-token HASHES and the commitment SALT.
    // Rebuild each in-memory `SeatToken` from its digest — the running server never
    // reconstructs a clear-text credential, and a reconnect authorises against the
    // identical hash (`ws.rs` hashes the presented token and compares). Re-commit
    // under the PERSISTED salt so the published commitment is bit-identical to the
    // one announced at creation — the commit–reveal stays provably-fair across a
    // restart, not just within one process. The seed/salt still reach a client only
    // at the end-of-match reveal (never a view/event) — redaction is untouched.
    let salt: [u8; 16] = reg.seed_salt.as_slice().try_into().ok()?;
    let seed_commit = SeedCommitment::from_parts(reg.seed as u64, salt);
    let db_id = reg.db_id;
    let (kind, mode) = if reg.mode == "2v2" {
        let tokens = [
            (SeatSlot::A1, seat_token_from_hash(&reg.token_a_hash)?),
            (SeatSlot::B1, seat_token_from_hash(&reg.token_b_hash)?),
            (SeatSlot::A2, seat_token_from_hash(&reg.token_a2_hash?)?),
            (SeatSlot::B2, seat_token_from_hash(&reg.token_b2_hash?)?),
        ];
        // The registry persists slot-token hashes, not the bot config; a recovered
        // 2v2 lobby comes back all-human (bot seats aren't auto-driven after a restart).
        (
            MatchKind::TwoVsTwo { tokens },
            ActorMode::TwoVsTwo { bots: Vec::new() },
        )
    } else {
        // Re-attach the Seat-B bot for a recovered vs-bot match. Its rng restarts
        // from the seed (the journal replays past moves exactly; future bot moves
        // need only be legal, not identical to a no-crash timeline).
        let bot = reg.bot_difficulty.map(|d| Bot {
            seat: Seat::B,
            difficulty: parse_difficulty(&d),
            // The registry records difficulty but not faction yet; recover as Lorekeeper.
            faction: recollect_bot::Faction::default(),
            rng: std::sync::Mutex::new(recollect_core::rng::Rng::from_seed(
                reg.seed as u64 ^ 0xB07,
            )),
        });
        (
            MatchKind::OneVsOne {
                token_a: seat_token_from_hash(&reg.token_a_hash)?,
                token_b: seat_token_from_hash(&reg.token_b_hash)?,
            },
            ActorMode::OneVsOne {
                bot,
                names: [None, None],
            },
        )
    };
    // Spawn the owning actor over the resumed session.
    let handle = actor::spawn(
        session,
        ActorConfig {
            mode,
            db_id: db_id.clone(),
            journal: Some(journal.clone()),
            event_client: Some(client),
            seed_commit,
            // The registry doesn't persist the per-match grace; a recovered match
            // re-arms the absence forfeit at the default so a disconnect still ends it.
            abandon_grace: crate::match_settings::DEFAULT_ABANDON_GRACE,
        },
    );
    let rebuilt = Match {
        handle,
        kind,
        evict: std::sync::atomic::AtomicBool::new(false),
        db_id,
        journal: Some(journal.clone()),
    };
    // Re-insert under the lock; if another connection recovered it first, use theirs.
    // (A redundantly-spawned actor's handle drops with the discarded `Arc`, ending
    // its task — no leak.)
    let arc = Arc::new(rebuilt);
    let mut map = lock_matches(&app.matches);
    Some(map.entry(id).or_insert(arc).clone())
}
