//! Web client: a **wgpu canvas** over the same deterministic core the server runs
//! — combat previews are computed locally and are exact, because client and server
//! share one rules crate. The canvas draws the board ([`scene`]/`render`) and the
//! **whole in-canvas game shell** ([`shell`]): the HUD, the opponent strip, the
//! hand tray, the floating buttons, **the interactive affordances + the floating inspect
//! panel**, the paced replay, the Dusk/Nightfall set-pieces, and the result screen,
//! composed around the board — the canvas-native client the design-of-record describes
//! (`docs/decisions/web_client_ux.md`), shared with the future native shells. The JS
//! bridge maps canvas pointer events onto the shell's hit-test regions
//! ([`shell::shell_regions`]) and renders the **virtual ARIA tree**
//! ([`shell::build_a11y_tree`]) — every affordance an actionable button firing the
//! same legal command the canvas does (invariant 7) — so the HTML move buttons
//! are **not used** for the shell.
//!
//! The shell drives **three transports**: a LOCAL engine (vs-AI 1v1 — [`LocalGame`]),
//! and the launch-critical **online PvP + 2v2** ([`OnlineShell`] over the [`online`]
//! builders), which compose the SAME `ShellModel` from the server's **redacted**
//! `PlayerView` / `TeamView` + its legal list — no engine on the client (the server is
//! authoritative; redaction holds — the opponent is counts/backs only). The only
//! board-only path left is the local-2v2 WATCH (the AI drives all four slots).
//!
//! Build: `trunk serve` in this crate (wasm32-unknown-unknown target).
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(unsafe_code)] // forbid would break wasm-bindgen macro expansion
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
pub mod atlas;
pub mod font;
pub mod online;
pub mod render;
pub mod scene;
pub mod shell;

#[wasm_bindgen]
pub fn protocol_version() -> u16 {
    recollect_protocol::PROTOCOL_VERSION
}

// --- Networked play: pure Command builders -----------------------------------
// The online client has no local engine (the server is authoritative): it renders
// the received `PlayerView` via `WebRenderer::draw_view` and sends commands as
// JSON. These build the `Command` serde shapes in Rust so the JS side never
// hardcodes the encoding. Basic interaction only (play / glimpse / end / release);
// the rich legal-move menu needs the engine and stays on the local client.

/// `"EndTurn"`.
#[wasm_bindgen]
pub fn cmd_end_turn_json() -> String {
    serde_json::to_string(&recollect_core::state::Command::EndTurn).unwrap()
}

/// `"Glimpse"`.
#[wasm_bindgen]
pub fn cmd_study_json() -> String {
    serde_json::to_string(&recollect_core::state::Command::Glimpse).unwrap()
}

/// Play hand card `hand_index` onto `tile` (no arrival engage — the simple path).
#[wasm_bindgen]
pub fn cmd_play_spirit_json(hand_index: u8, tile: u8) -> String {
    serde_json::to_string(&recollect_core::state::Command::PlaySpirit {
        hand_index,
        tile,
        engage: None,
        chain_prefs: Vec::new(),
    })
    .unwrap()
}

/// Release hand card `hand_index` (the hand-cap resolution at Flow).
#[wasm_bindgen]
pub fn cmd_release_json(hand_index: u8) -> String {
    serde_json::to_string(&recollect_core::state::Command::Release { hand_index }).unwrap()
}

/// A card's name by id — the online client has no engine, so it looks names up
/// here (the catalog is id-ordered) to label hand chips.
#[wasm_bindgen]
pub fn card_name(id: u16) -> String {
    recollect_core::cards::canon_catalog()
        .get(id as usize)
        .map(|c| c.name.clone())
        .unwrap_or_default()
}

/// One card as the frontend-facing JSON object — shared by `card_detail_json`
/// (one card, in a running game) and [`catalog_json`] (all of them, no game), so
/// the inspect panel and the card gallery render the exact same shape.
fn card_json(c: &recollect_core::types::CardDef) -> serde_json::Value {
    let mut kw: Vec<&str> = Vec::new();
    if c.arcane {
        kw.push("Arcane");
    }
    if c.warded {
        kw.push("Warded");
    }
    if c.mobile {
        kw.push("Mobile");
    }
    if c.steadfast {
        kw.push("Steadfast");
    }
    if c.relentless {
        kw.push("Relentless");
    }
    if c.lurk {
        kw.push("Lurk");
    }
    serde_json::json!({
        "id": c.id.0,
        "name": c.name,
        "kind": format!("{:?}", c.kind),
        "resonance": format!("{:?}", c.resonance),
        "cost": c.cost,
        "attack": c.attack,
        "defense": c.defense,
        "hp": c.hp,
        "keywords": kw,
        "rules": c.rules,
        "reach": format!("{:?}", c.reach),
    })
}

/// The whole card catalog as a JSON array (every card keyed by `id`, the same
/// shape `card_detail_json` returns). A frontend loads this once for a complete
/// client-side card database — usable in local play, online play (where there's
/// no engine to query), and the card gallery / web page. Pure: just the canon
/// catalog, serialized; no running game required.
#[wasm_bindgen]
pub fn catalog_json() -> String {
    let cards: Vec<serde_json::Value> = recollect_core::cards::canon_catalog()
        .iter()
        .map(card_json)
        .collect();
    serde_json::to_string(&cards).unwrap()
}

/// The board tiles a card threatens from `tile` on a `board_w`×`board_w` board,
/// facing up (seat A's vantage) — a reach grid for ANY card with no running game
/// (online inspect, the gallery). `card_id` out of range ⇒ `[]`. JSON: `[tile…]`.
#[wasm_bindgen]
pub fn reach_tiles_json(card_id: u16, tile: u8, board_w: u8) -> String {
    let cat = recollect_core::cards::canon_catalog();
    let Some(c) = cat.get(card_id as usize) else {
        return "[]".to_string();
    };
    let tiles =
        recollect_core::engine::reach_tiles(c.reach, tile, recollect_core::Seat::A, board_w.max(1));
    serde_json::to_string(&tiles).unwrap()
}

/// Full card detail for the inspect panel from the canon catalog (id-ordered) — the
/// **engine-free** twin of [`LocalGame::card_detail_json`], for the ONLINE / 2v2 client
/// (which has no engine). Same JSON shape as [`catalog_json`]'s per-card entry. An
/// out-of-range id yields `{}` (the inspect panel then draws nothing).
#[wasm_bindgen]
pub fn card_detail_by_id_json(id: u16) -> String {
    match recollect_core::cards::canon_catalog().get(id as usize) {
        Some(c) => card_json(c).to_string(),
        None => "{}".to_string(),
    }
}

/// A card's reach as a board-sized grid for the inspect preview — the **engine-free**
/// twin of [`LocalGame::reach_grid_json`] for the online / 2v2 client. `board_w` is the
/// grid side (5 for 1v1, 6 for 2v2); the reach is computed from board centre facing up.
/// `{ "w": board_w, "center": c, "reach": [tiles…] }`; an out-of-range id ⇒ empty reach.
#[wasm_bindgen]
pub fn reach_grid_by_id_json(id: u16, board_w: u8) -> String {
    let w = board_w.max(1);
    // The central tile (row w/2, col w/2) — `w*w/2` lands on an EDGE for an even width
    // (e.g. 18 = row 3, col 0 on a 6×6), so compute the row+col centre explicitly.
    let center = (w / 2) * w + (w / 2);
    let reach = match recollect_core::cards::canon_catalog().get(id as usize) {
        Some(c) => recollect_core::engine::reach_tiles(c.reach, center, recollect_core::Seat::A, w),
        None => Vec::new(),
    };
    serde_json::json!({ "w": w, "center": center, "reach": reach }).to_string()
}

/// One `LegalMove` wire object (`{ label, cmd, forecast }`) for `seat`'s `cmd`, exactly
/// as the server ships it — so the sample welcomes below carry the real shape the
/// online client consumes.
fn legal_move_json(
    engine: &recollect_core::Engine,
    seat: recollect_core::Seat,
    cmd: recollect_core::state::Command,
) -> serde_json::Value {
    serde_json::json!({
        "label": recollect_protocol::label(engine, seat, &cmd),
        "forecast": recollect_protocol::forecast(engine, seat, &cmd),
        "cmd": cmd,
    })
}

/// **Test-support**: a sample online 1v1 `welcome` message — the **redacted**
/// `PlayerView` for `seat` (`"A"`/`"B"`) + the server's legal list + session names —
/// exactly the wire shape `recollect-server` sends. The headless UI suite injects this
/// (with no real websocket) to drive the online shell's rendering / a11y / redaction.
/// A couple of opening plays populate the board so the card treatment shows. Never used
/// in normal play; it produces only the redacted view (the opponent stays counts-only).
#[wasm_bindgen]
pub fn sample_online_welcome_json(seed: u64, seat: &str, moves: u8) -> String {
    let cat = recollect_core::cards::canon_catalog();
    let deck: Vec<recollect_core::CardId> = cat
        .iter()
        .filter(|c| matches!(c.kind, recollect_core::types::CardKind::Spirit))
        .take(20)
        .map(|c| c.id)
        .collect();
    let (mut engine, _) = recollect_core::Engine::new(seed, cat, deck.clone(), deck);
    // Populate the board a little (seat A opens) so the shell shows placed cards.
    for _ in 0..moves {
        let a = engine.state().active;
        let play = engine.legal_commands(a).into_iter().find(|c| {
            matches!(
                c,
                recollect_core::state::Command::PlaySpirit { engage: None, .. }
            )
        });
        match play {
            Some(cmd) => {
                let _ = engine.apply(a, cmd);
            }
            None => {
                let _ = engine.apply(a, recollect_core::state::Command::EndTurn);
            }
        }
    }
    let who = if seat.eq_ignore_ascii_case("B") {
        recollect_core::Seat::B
    } else {
        recollect_core::Seat::A
    };
    let view = recollect_core::view::view_for(&engine, who);
    // The recipient's legal moves (empty unless it's their turn) — the real wire rule.
    let legal: Vec<serde_json::Value> = if engine.state().active == who {
        engine
            .legal_commands(who)
            .into_iter()
            .map(|cmd| legal_move_json(&engine, who, cmd))
            .collect()
    } else {
        Vec::new()
    };
    serde_json::json!({
        "t": "welcome",
        "v": recollect_protocol::PROTOCOL_VERSION,
        "seat": who,
        "view": view,
        "legal": legal,
        "seat_names": ["Warden Ames", "Corin Ashe"],
    })
    .to_string()
}

/// **Test-support**: a sample online 1v1 `welcome` whose board has a spirit
/// standing ON a face-up Landmark (a co-occupied tile) — so the headless UI suite can drive
/// the canvas-native shell to a state where BOTH a spirit and a landmark share a tile, and
/// verify the landmark is independently inspectable (its own a11y node + a toggle-tap). The
/// recipient is seat A, on their turn. Never used in normal play; produces only the redacted
/// view, exactly the wire shape the server sends.
#[wasm_bindgen]
pub fn sample_landmark_welcome_json(seed: u64) -> String {
    use recollect_core::state::{Terrain, TerrainKind};
    use recollect_core::test_support::put_spirit;
    let cat = recollect_core::cards::canon_catalog();
    let cloud = cat
        .iter()
        .find(|c| c.name == "Cloudling")
        .map(|c| c.id)
        .unwrap_or(recollect_core::CardId(0));
    let deck: Vec<recollect_core::CardId> = (0..20).map(|_| cloud).collect();
    let (mut engine, _) = recollect_core::Engine::new(seed, cat, deck.clone(), deck);
    {
        let st = engine.state_mut_for_test();
        // A face-up Landmark (seat A) on tile 12, with a seat-A spirit standing on it.
        st.board[12].terrain = Some(Terrain {
            card: cloud,
            owner: recollect_core::Seat::A,
            kind: TerrainKind::Landmark,
            face_down: false,
        });
        put_spirit(st, 12, cloud, recollect_core::Seat::A);
    }
    let who = recollect_core::Seat::A;
    let view = recollect_core::view::view_for(&engine, who);
    let legal: Vec<serde_json::Value> = if engine.state().active == who {
        engine
            .legal_commands(who)
            .into_iter()
            .map(|cmd| legal_move_json(&engine, who, cmd))
            .collect()
    } else {
        Vec::new()
    };
    serde_json::json!({
        "t": "welcome",
        "v": recollect_protocol::PROTOCOL_VERSION,
        "seat": who,
        "view": view,
        "legal": legal,
        "seat_names": ["Warden Ames", "Corin Ashe"],
    })
    .to_string()
}

/// **Test-support**: a sample online 1v1 `welcome` whose board has a **standing-Faded**
/// form (a Primal "Stormswell" banished in combat, in its §0.5 window) on tile 12 for seat A,
/// with the matching **base** ("Cloudling") in A's hand and ample Anima — so the server's
/// legal list carries a `Devolve`, driving the headless UI suite to a state where the
/// **devolve (recede) affordance** is live: the form renders standing-Faded (rescuable), the
/// tile + base card carry the recede chevron + a11y node, and a tap-then-target recede
/// resolves. The recipient is seat A, on their turn. Mirrors the engine's standing-Faded
/// staging (`tests/devolution.rs`'s `faded_form`). Never used in normal play; produces only
/// the redacted view, exactly the wire shape the server sends.
#[wasm_bindgen]
pub fn sample_devolve_welcome_json(seed: u64) -> String {
    use recollect_core::test_support::put_spirit;
    let cat = recollect_core::cards::canon_catalog();
    let id_of = |name: &str| {
        cat.iter()
            .find(|c| c.name == name)
            .map(|c| c.id)
            .unwrap_or(recollect_core::CardId(0))
    };
    let base = id_of("Cloudling"); // a Lorekeeper Wonder base…
    let primal = id_of("Stormswell"); // …and its Primal form (Cloudling → Stormswell)
    let deck: Vec<recollect_core::CardId> = (0..20).map(|_| base).collect();
    let (mut engine, _) = recollect_core::Engine::new(seed, cat, deck.clone(), deck);
    {
        let st = engine.state_mut_for_test();
        let tile = 12u8;
        // A standing-Faded Primal for seat A: banished in combat (by B), wounded, in its
        // §0.5 window (`fading` + `fade_deadline` Some) — so it is rescuable THIS turn.
        put_spirit(st, tile, primal, recollect_core::Seat::A);
        if let Some(sp) = st.board[tile as usize].spirit.as_mut() {
            sp.hp = 5; // wounded — the rescued base will arrive at FULL HP, a real change
            sp.fading = true;
            sp.banished_by = Some(recollect_core::Seat::B);
            sp.fade_deadline = Some(st.round + 1); // banished on B's turn, owner A ⇒ round+1
        }
        st.active = recollect_core::Seat::A;
        st.active_slot = recollect_core::types::SeatSlot::A1;
        st.player_a.anima = 20;
        st.player_a.hand = vec![base]; // the base card that recedes the form
        st.player_a.deck.clear();
        st.player_b.deck.clear();
        st.moved_this_turn.clear();
    }
    let who = recollect_core::Seat::A;
    let view = recollect_core::view::view_for(&engine, who);
    let legal: Vec<serde_json::Value> = if engine.state().active == who {
        engine
            .legal_commands(who)
            .into_iter()
            .map(|cmd| legal_move_json(&engine, who, cmd))
            .collect()
    } else {
        Vec::new()
    };
    serde_json::json!({
        "t": "welcome",
        "v": recollect_protocol::PROTOCOL_VERSION,
        "seat": who,
        "view": view,
        "legal": legal,
        "seat_names": ["Warden Ames", "Corin Ashe"],
    })
    .to_string()
}

/// **Test-support**: a sample 2v2 `team_welcome` message — the **redacted**
/// `TeamView` for the opening slot (A1) + its legal list — exactly the wire shape the
/// server sends. The headless UI suite injects it to drive the 2v2 shell (the 6×6 board,
/// the opposing team as combined counts — redaction holds). Never used in normal play.
#[wasm_bindgen]
pub fn sample_team_welcome_json(seed: u64) -> String {
    use recollect_core::types::SeatSlot;
    let cat = recollect_core::cards::canon_catalog();
    let deck: Vec<recollect_core::CardId> = cat
        .iter()
        .filter(|c| matches!(c.kind, recollect_core::types::CardKind::Spirit))
        .take(20)
        .map(|c| c.id)
        .collect();
    let decks = [deck.clone(), deck.clone(), deck.clone(), deck];
    let (engine, _) = recollect_core::Engine::new_2v2(seed, cat, decks);
    let slot = engine.state().active_slot; // A1 opens
    let view = recollect_core::view::view_for_slot(&engine, slot);
    let team = slot.team();
    let legal: Vec<serde_json::Value> = engine
        .legal_commands(team)
        .into_iter()
        .map(|cmd| legal_move_json(&engine, team, cmd))
        .collect();
    let _ = SeatSlot::A1;
    serde_json::json!({
        "t": "team_welcome",
        "v": recollect_protocol::PROTOCOL_VERSION,
        "slot": slot,
        "view": view,
        "legal": legal,
    })
    .to_string()
}

/// Local hotseat engine for offline playtesting in the browser — proves the
/// core runs under wasm before any networking exists.
#[wasm_bindgen]
pub struct LocalGame {
    engine: recollect_core::Engine,
    /// Deterministic AI policy for "vs AI" quick play, seeded from the match so
    /// it's replayable. A trained policy net (see bot_and_ml_plan.md) sits behind
    /// `recollect_bot::choose`, swapping the chooser only — not this shape.
    auto_rng: recollect_core::rng::Rng,
    /// The opponent's difficulty (defaults to Normal).
    difficulty: recollect_bot::Difficulty,
    /// The NAMED character each seat fields this match (e.g. "Corin Ashe"),
    /// for the HUD (your name) + the opponent strip (their name), per
    /// `web_client_ux.md` §Opponent strip. Empty ⇒ fall back to the faction word
    /// ("the Solace" / "Lorekeepers"). Indexed by seat (`[A, B]`); the human is seat A.
    char_names: [String; 2],
}

#[wasm_bindgen]
impl LocalGame {
    #[wasm_bindgen(constructor)]
    pub fn new(seed: u64) -> LocalGame {
        let deck: Vec<recollect_core::CardId> = [
            0u16, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10,
        ]
        .iter()
        .map(|i| recollect_core::CardId(*i))
        .collect();
        let (engine, _) = recollect_core::Engine::new(
            seed,
            recollect_core::cards::canon_catalog(),
            deck.clone(),
            deck,
        );
        LocalGame {
            engine,
            auto_rng: recollect_core::rng::Rng::from_seed(seed ^ 0xA1),
            difficulty: recollect_bot::Difficulty::default(),
            char_names: [String::new(), String::new()],
        }
    }

    /// A local 2v2 match on a 6×6 board (four seeded decks, slots
    /// A1→B1→A2→B2). Drive it with `auto_play_turn` (AI watch) and render with
    /// `WebRenderer::draw_team(team_view_json())`.
    pub fn new_2v2(seed: u64) -> LocalGame {
        let cat = recollect_core::cards::canon_catalog();
        let offers = recollect_core::quickplay::offer(seed);
        let decks: [Vec<recollect_core::CardId>; 4] = [
            recollect_core::quickplay::generate_deck(offers[0].id, seed, &cat),
            recollect_core::quickplay::generate_deck(offers[1].id, seed.wrapping_add(1), &cat),
            recollect_core::quickplay::generate_deck(offers[2].id, seed.wrapping_add(2), &cat),
            recollect_core::quickplay::generate_deck(offers[0].id, seed.wrapping_add(3), &cat),
        ];
        let (engine, _) = recollect_core::Engine::new_2v2(seed, cat, decks);
        LocalGame {
            engine,
            auto_rng: recollect_core::rng::Rng::from_seed(seed ^ 0xA1),
            difficulty: recollect_bot::Difficulty::default(),
            char_names: [String::new(), String::new()],
        }
    }

    /// True for a 2v2 match (the shell renders a 6×6 TeamView instead of 5×5).
    pub fn is_2v2(&self) -> bool {
        self.engine.state().is_2v2()
    }

    /// The active SLOT's four-seat `TeamView` as JSON (your hand, teammate +
    /// both opponents as counts, the 6×6 board, `board_w`). Redaction identical to
    /// 1v1: only your own cards are shown.
    pub fn team_view_json(&self) -> String {
        let slot = self.engine.state().active_slot;
        serde_json::to_string(&recollect_core::view::view_for_slot(&self.engine, slot)).unwrap()
    }

    /// The Quick Play offer: three seeded style choices for the picker UI. Each
    /// carries BOTH the subjective `blurb` (the voice) and the objective
    /// `selection` — the four labelled [`recollect_core::quickplay::SelectionInfo`]
    /// facets (resonance lean · tempo · aggression · body-mix) plus a one-line
    /// `summary` — so the picker can show the shape, and a screen reader can read it
    /// as words. Serialized through serde (no hand-rolled JSON: a blurb is free to
    /// hold any character).
    pub fn quick_offer_json(seed: u64) -> String {
        let offers = recollect_core::quickplay::offer(seed);
        let items: Vec<OfferStyle> = offers.iter().map(OfferStyle::from).collect();
        serde_json::to_string(&items).unwrap()
    }

    /// Picker preview: the deck a style would derive at this seed, as JSON
    /// (`quickplay::DeckPreview` — card list, cost curve, spirit/spell split). The
    /// picker shows this for each offered style so the choice is informed, not blind.
    pub fn deck_preview_json(seed: u64, style: u8) -> String {
        let cat = recollect_core::cards::canon_catalog();
        serde_json::to_string(&recollect_core::quickplay::preview(style, seed, &cat)).unwrap()
    }

    /// Start a quick match: chosen style for the player, a seeded style for
    /// the opponent (AI or hotseat human). Decks are derived, never sent.
    pub fn new_quick(seed: u64, your_style: u8, opponent_style: u8, difficulty: u8) -> LocalGame {
        let cat = recollect_core::cards::canon_catalog();
        let da = recollect_core::quickplay::generate_deck(your_style, seed, &cat);
        let db =
            recollect_core::quickplay::generate_deck(opponent_style, seed.wrapping_add(1), &cat);
        let (engine, _) = recollect_core::Engine::new(seed, cat, da, db);
        let difficulty = recollect_bot::Difficulty::ALL
            .get(difficulty as usize)
            .copied()
            .unwrap_or_default();
        // Name the two storytellers. Each seat fields a named character drawn
        // from the roster matching its faction, keyed by the chosen style + the seed (so
        // the same match always names the same pair, and different matches vary them).
        let st = engine.state();
        let you_name = pick_character(st.rules.factions[0], your_style, seed);
        let opp_name = pick_character(st.rules.factions[1], opponent_style, seed ^ 0x5151);
        LocalGame {
            engine,
            auto_rng: recollect_core::rng::Rng::from_seed(seed ^ 0xA1),
            difficulty,
            char_names: [you_name, opp_name],
        }
    }

    /// The four difficulty options as JSON (index, name) for the picker UI,
    /// with Normal (index 1) the default.
    pub fn difficulties_json() -> String {
        let items: Vec<String> = recollect_bot::Difficulty::ALL
            .iter()
            .enumerate()
            .map(|(i, d)| {
                format!(
                    "{{\"index\":{},\"name\":\"{}\",\"default\":{}}}",
                    i,
                    d.name(),
                    *d == recollect_bot::Difficulty::default()
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    }

    /// Play one full AI turn for the active seat (vs-AI mode). Deterministic
    /// given the match seed; returns the events as JSON.
    pub fn auto_play_turn(&mut self) -> String {
        let mut all = Vec::new();
        let me = self.engine.state().active;
        loop {
            let st = self.engine.state();
            if st.active != me || matches!(st.phase, recollect_core::Phase::Finished { .. }) {
                break;
            }
            let cmd = recollect_bot::choose(&self.engine, me, self.difficulty, &mut self.auto_rng);
            all.extend(self.engine.apply(me, cmd).expect("legal"));
        }
        serde_json::to_string(&all).unwrap()
    }

    /// Play one full AI turn, but as a **paced, watchable replay**: the
    /// bot's turn is driven command-by-command and each discrete action is distilled
    /// into a [`ReplayBeat`](crate::shell::ReplayBeat), captured alongside the **human's
    /// redacted shell snapshot** *after* that action. The JS shell paces through the
    /// beats on a timer (the "watched + paced" decision in `web_client_ux.md`),
    /// animating to each snapshot, drawing the caption, and pushing the announcement
    /// into the `#status` live region (invariant 7).
    ///
    /// Returns a JSON envelope `{ "beats": [ { "caption", "announce", "kind", "tiles",
    /// "erasures", "model" } … ], "round_note": String? }`, where `model` is a complete
    /// [`ShellModel`](crate::shell::ShellModel) for that beat (the human's `PlayerView`
    /// snapshot + the running score/erasures + the replay caption overlay), so the JS
    /// pacer is fully data-driven and never re-derives state. `round_note` is the
    /// Dusk/Nightfall announcement if the bot's turn crossed that boundary.
    ///
    /// Determinism + redaction are untouched: the engine has already applied each
    /// command (same seed ⇒ same turn), and EVERY snapshot is `view_for(human_seat)` —
    /// the opponent's hand is never revealed, only their public board actions +
    /// face-down draws. A command that produces no player-visible action is folded into
    /// the next beat (no empty frames).
    pub fn auto_play_turn_paced(&mut self) -> String {
        use recollect_core::view::view_for;
        let me = self.engine.state().active; // the bot
        let human = me.other(); // the watcher (seat A in local 1v1)
        let round_before = self.engine.state().round;

        let mut beats: Vec<serde_json::Value> = Vec::new();
        let opp = faction_word(self.engine.state().rules.factions[me as usize]);
        loop {
            let st = self.engine.state();
            if st.active != me || matches!(st.phase, recollect_core::Phase::Finished { .. }) {
                break;
            }
            let cmd = recollect_bot::choose(&self.engine, me, self.difficulty, &mut self.auto_rng);
            let events = self.engine.apply(me, cmd).expect("legal");
            let erasures_after = self.engine.state().solace_erasures;
            // Distill this command's events into one paced beat. A no-op (no visible
            // action) yields None — fold it forward by simply not emitting a frame; the
            // next real action's snapshot already reflects the applied state.
            if let Some(beat) = crate::shell::beat_for_command(&events, &opp, erasures_after) {
                // The human's redacted snapshot AFTER this command, wrapped in the shell
                // model with the replay caption overlay (your controls inert mid-replay).
                let view = view_for(&self.engine, human);
                let caption = crate::shell::ReplayCaption {
                    text: beat.caption.clone(),
                    kind: beat.kind.clone(),
                    tiles: beat.tiles.clone(),
                };
                let model = self.shell_model_for_view(human, view, Some(caption));
                beats.push(serde_json::json!({
                    "caption": beat.caption,
                    "announce": beat.announce,
                    "kind": beat.kind,
                    "tiles": beat.tiles,
                    "erasures": beat.erasures,
                    "model": model,
                }));
            }
        }

        // A Dusk/Nightfall set-piece announcement if the bot's turn crossed the round
        // boundary into one of the binding beats (the live region surfaces these).
        let st = self.engine.state();
        let round_note = if st.round != round_before {
            crate::shell::round_announcement(
                st.round,
                st.rules.contraction_after,
                recollect_core::engine::LAST_ROUND,
            )
        } else {
            None
        };

        serde_json::json!({ "beats": beats, "round_note": round_note }).to_string()
    }

    /// The match-start announcement: **who opens** the match, in the
    /// game's register, always naming the first player (design §5, "Who opens the
    /// match"). Local 1v1 opens the human (seat A). Pushed into the `#status` live
    /// region when the match begins, so the screen-reader narration and the on-canvas
    /// flourish are one source.
    pub fn opener_announcement(&self) -> String {
        let st = self.engine.state();
        // The opener is whoever acts on round 1's first turn — the active seat at start.
        // Local 1v1 starts with seat A (the human) to act; this stays correct if that
        // ever changes (it reads the live active seat at match start).
        let first = st.active;
        let you = recollect_core::Seat::A; // the human's seat in local play
        // Name the opener by their CHARACTER when one is fielded, else faction.
        let opp_faction = faction_word(st.rules.factions[first.other() as usize]);
        let opp_name = self.seat_label(first.other(), &opp_faction);
        crate::shell::opener_announcement(you, first, &opp_name)
    }

    /// Active seat's view as JSON for the JS renderer.
    pub fn view_json(&self) -> String {
        let seat = self.engine.state().active;
        serde_json::to_string(&recollect_core::view::view_for(&self.engine, seat)).unwrap()
    }

    pub fn legal_json(&self) -> String {
        serde_json::to_string(&self.engine.legal_commands(self.engine.state().active)).unwrap()
    }

    /// Movement cues for the live frame, as a `MoveCues` JSON
    /// (`{ "movable": [tile…], "sick": [tile…] }`). `movable` is every tile the
    /// engine currently offers a Move from (the active seat's Mobile spirits that
    /// can still take their one step); `sick` is the active seat's Mobile,
    /// non-fading spirits whose tile sits in `moved_this_turn` — they have spent
    /// their move or just arrived (summoning-sick) and cannot step again this turn.
    /// The `PlayerView` carries none of this, so the renderer reads it from here.
    pub fn move_cues_json(&self) -> String {
        use recollect_core::state::Command;
        let st = self.engine.state();
        let seat = st.active;
        let movable: Vec<u8> = self
            .engine
            .legal_commands(seat)
            .into_iter()
            .filter_map(|c| match c {
                Command::MoveSpirit { from, .. } => Some(from),
                _ => None,
            })
            .collect();
        let sick: Vec<u8> = (0..st.board.len() as u8)
            .filter(|&t| {
                st.spirit_at(t).is_some_and(|sp| {
                    sp.owner == seat
                        && !sp.fading
                        && self.engine.card(sp.card).mobile
                        && st.moved_this_turn.contains(&t)
                })
            })
            .collect();
        serde_json::json!({ "movable": movable, "sick": sick }).to_string()
    }

    /// Running score readout for the HUD: each seat's board points (one per tile
    /// held, by the last mark on it) plus the Solace's off-board **erasure tally**
    /// (`erasures` — every banish or Unwriting it lands; the Unwritten leave no board
    /// mark, so this is the only place forgetting shows). `b_total = b_board +
    /// erasures` is what Seat B scores if Nightfall struck now, exactly as `flow`
    /// folds it at Nightfall. JSON: `{ "a": A, "b_board": B, "erasures": E,
    /// "b_total": B+E }`.
    pub fn score_readout_json(&self) -> String {
        let st = self.engine.state();
        let mut a = 0u8;
        let mut b_board = 0u8;
        for t in st.board.iter() {
            let owner = if let Some(sp) = &t.spirit {
                Some(if sp.fading {
                    sp.banished_by.unwrap_or(sp.owner)
                } else {
                    sp.owner
                })
            } else {
                t.impressions.first().copied()
            };
            match owner {
                Some(recollect_core::Seat::A) => a = a.saturating_add(1),
                Some(recollect_core::Seat::B) => b_board = b_board.saturating_add(1),
                None => {}
            }
        }
        let erasures = st.solace_erasures;
        serde_json::json!({
            "a": a,
            "b_board": b_board,
            "erasures": erasures,
            "b_total": b_board.saturating_add(erasures),
        })
        .to_string()
    }

    /// The whole in-canvas game shell as one JSON
    /// [`ShellModel`](crate::shell::ShellModel): the move-cued, interaction-overlaid
    /// board scene plus the HUD (your score · your Anima · the round/clock), the
    /// opponent strip
    /// (name · score *including* the off-board erasure tally · their hand size as
    /// face-down backs), and your hand as placeholder cards (cost · name · the
    /// A/D/HP stat block). The board half folds in the movement cues and the
    /// interaction overlays exactly as `WebRenderer::draw_view_interactive`
    /// does; the chrome reads the running score (board points + the Solace's erasure
    /// tally — neither in the `PlayerView`) from the engine, like the score readout.
    ///
    /// **Phase B** also folds in the affordances (the action dots / evolve glyphs)
    /// — computed HERE from the engine's legal moves (the source of truth), so the
    /// canvas and the a11y tree agree — and the transient UI state JS owns
    /// (`ui_json`: the lifted hand card, drag in progress + position, the inspect
    /// panel). 1v1 only (the local engine's vantage); the renderer draws it with
    /// `WebRenderer::draw_shell`.
    pub fn shell_model_json(
        &self,
        cues_json: &str,
        interaction_json: &str,
        ui_json: &str,
    ) -> String {
        use recollect_core::types::Faction;
        let st = self.engine.state();
        let you = st.active;
        let opp = you.other();
        let view = recollect_core::view::view_for(&self.engine, you);

        // The board (the hero) is built renderer-side from the view + these cues +
        // overlays — the same score/movement/interaction inputs the standalone board draw uses. The
        // names index the catalog (id-ordered) for spirit labels on the board.
        let cues: crate::scene::MoveCues = serde_json::from_str(cues_json).unwrap_or_default();
        let inter: crate::scene::Interaction =
            serde_json::from_str(interaction_json).unwrap_or_default();
        let board_names: Vec<String> = recollect_core::cards::canon_catalog()
            .iter()
            .map(|c| crate::scene::short_board_name(&c.name))
            .collect();

        // The running score (board points per seat) + the Solace's off-board tally.
        let (mut a_board, mut b_board) = (0u8, 0u8);
        for t in st.board.iter() {
            let owner = if let Some(sp) = &t.spirit {
                Some(if sp.fading {
                    sp.banished_by.unwrap_or(sp.owner)
                } else {
                    sp.owner
                })
            } else {
                t.impressions.first().copied()
            };
            match owner {
                Some(recollect_core::Seat::A) => a_board = a_board.saturating_add(1),
                Some(recollect_core::Seat::B) => b_board = b_board.saturating_add(1),
                None => {}
            }
        }
        let erasures = st.solace_erasures;
        // The erasure tally folds into whichever seat is the Solace (it is the only
        // side that erases off-board); a Lorekeeper opponent shows board points only.
        let seat_total = |seat: recollect_core::Seat, board: u8| -> (u8, u8) {
            if st.rules.factions[seat as usize] == Faction::Solace {
                (board.saturating_add(erasures), erasures)
            } else {
                (board, 0)
            }
        };
        let you_board = if you == recollect_core::Seat::A {
            a_board
        } else {
            b_board
        };
        let opp_board = if opp == recollect_core::Seat::A {
            a_board
        } else {
            b_board
        };
        let (you_score, _) = seat_total(you, you_board);
        let (opp_score, opp_erasures) = seat_total(opp, opp_board);

        // Your hand as placeholder cards (the catalog's full stat block).
        let hand: Vec<crate::shell::HandCard> = view
            .you
            .hand
            .iter()
            .map(|id| {
                let c = self.engine.card(*id);
                crate::shell::HandCard {
                    name: c.name.clone(),
                    cost: c.cost,
                    attack: c.attack,
                    defense: c.defense,
                    hp: c.hp,
                    kind: format!("{:?}", c.kind),
                    resonance: format!("{:?}", c.resonance),
                }
            })
            .collect();

        // The opponent strip names the CHARACTER when one is fielded, with the
        // faction word as a sub-label; the HUD names you the same way.
        let opp_faction = faction_word(st.rules.factions[opp as usize]);
        let opp_name = self.seat_label(opp, &opp_faction);
        let you_faction = faction_word(st.rules.factions[you as usize]);
        let you_name = self.seat_label(you, &you_faction);

        // Read the scalars the HUD/opponent strip need before the view is moved into
        // the model (the model owns it so the board builds renderer-side).
        let you_anima = view.you.anima;
        let opp_hand_count = view.opponent.hand_count;
        let round = view.round;
        let your_turn = view.active == view.seat;
        let last_round = recollect_core::engine::LAST_ROUND;
        let dusk_after = st.rules.contraction_after;

        // Phase B affordances — derived from the engine's legal moves (the source of
        // truth): which board tiles can act, which Fading bases can evolve, which
        // hand cards have a legal play, and which of those are evolution forms.
        let aff = self.affordances(you);
        // The transient UI state JS owns (the lifted card / drag / inspect panel).
        let ui: ShellUi = serde_json::from_str(ui_json).unwrap_or_default();

        // The in-canvas Glimpse + Mulligan choice prompt. The GLIMPSE is an engine
        // pending choice, surfaced to its owner ONLY via the redacted view (`view.you.pending`)
        // — so the opponent's glimpse can never build a prompt here (redaction, invariant 2);
        // `build_choice_prompt` maps the burn / keep-or-bottom step to the modal. The MULLIGAN
        // is a *legal command* the JS opening offer requests (`ui.mulligan_offer`), honoured
        // only while the engine's `mulligan_window` is still open (a stale flag draws nothing).
        // A pending choice takes precedence (you can't open the mulligan mid-glimpse anyway).
        let card_of = |id: recollect_core::types::CardId| {
            let c = self.engine.card(id);
            crate::shell::HandCard {
                name: c.name.clone(),
                cost: c.cost,
                attack: c.attack,
                defense: c.defense,
                hp: c.hp,
                kind: format!("{:?}", c.kind),
                resonance: format!("{:?}", c.resonance),
            }
        };
        let choice = if let Some(pending) = view.you.pending.as_ref() {
            crate::shell::build_choice_prompt(pending, &card_of)
        } else if ui.mulligan_offer && your_turn && self.mulligan_available(you) {
            Some(crate::shell::build_mulligan_prompt())
        } else {
            None
        };

        let model = crate::shell::ShellModel {
            you_seat: you,
            you_name,
            you_faction,
            you_score,
            you_anima,
            hand,
            opp_name,
            opp_faction,
            opp_score,
            opp_erasures,
            opp_hand_count,
            round,
            last_round,
            dusk_after,
            your_turn,
            view,
            names: board_names,
            cues,
            interaction: inter,
            actionable_tiles: aff.actionable_tiles,
            evolvable_tiles: aff.evolvable_tiles,
            devolvable_tiles: aff.devolvable_tiles,
            actionable_hand: aff.actionable_hand,
            evolve_forms: aff.evolve_forms,
            devolve_bases: aff.devolve_bases,
            lifted_hand: ui.lifted_hand,
            hand_scroll: ui.hand_scroll,
            dragging: ui.dragging,
            drag_xy: ui.drag_xy,
            inspect: ui.inspect,
            replay: None,
            // Phase D: the Dusk/Nightfall set-piece + the result screen are JS-driven
            // overlays — their CONTENT is engine-computed (`result_screen_json` / the
            // round pacer's `dusk_set_piece_json`), but the live animation `progress` is
            // JS-owned, so they ride in through `ui_json` like the inspect panel does.
            dusk: ui.dusk,
            result: ui.result,
            choice,
        };
        serde_json::to_string(&model).unwrap()
    }

    /// The shell's interactive **hit-test regions** for a `vw`×`vh`
    /// (CSS-px / backing-px) viewport, as JSON ([`ShellRegions`](crate::shell::ShellRegions)):
    /// the board rect + grid side, one rect per hand card, and the two FAB rects. The
    /// JS pointer bridge maps a canvas tap/drag to a board tile, a hand-card index, or
    /// a FAB through these — the same layout the draw uses, so they never disagree.
    pub fn shell_regions_json(
        &self,
        cues_json: &str,
        interaction_json: &str,
        ui_json: &str,
        vw: f32,
        vh: f32,
    ) -> String {
        // Regions only need the hand SIZE + the board (the affordance lists don't
        // affect geometry), but build the same model so the layout is byte-identical
        // to the draw. (Cheap — one PlayerView + a hand map.)
        let model_json = self.shell_model_json(cues_json, interaction_json, ui_json);
        let model: crate::shell::ShellModel = match serde_json::from_str(&model_json) {
            Ok(m) => m,
            Err(_) => return "{}".into(),
        };
        serde_json::to_string(&crate::shell::shell_regions(&model, vw, vh)).unwrap()
    }

    /// Invariant 7 — the **virtual a11y tree** for the shell, as JSON
    /// (a list of [`A11yNode`](crate::shell::A11yNode)): the board's actionable /
    /// occupied tiles, your hand cards, the opponent strip, and the End-Turn + Glimpse
    /// buttons — each an actionable accessible element that fires the SAME legal
    /// command its canvas affordance does. The JS bridge renders these as off-screen
    /// ARIA buttons (focus-stable by `id`), so a keyboard / screen-reader user reaches
    /// every action at parity with the canvas. Per-tile labels reuse the board mirror's
    /// wording (one source for the reading).
    pub fn shell_a11y_json(
        &self,
        cues_json: &str,
        interaction_json: &str,
        ui_json: &str,
    ) -> String {
        let model_json = self.shell_model_json(cues_json, interaction_json, ui_json);
        let model: crate::shell::ShellModel = match serde_json::from_str(&model_json) {
            Ok(m) => m,
            Err(_) => return "[]".into(),
        };
        // While a Glimpse / Mulligan modal is up, the accessible tree IS the choice's
        // (its options as actionable buttons), not the live game tree: the board/hand plays
        // are blocked mid-choice, so offering them would be a lie. One modal, one tree.
        if let Some(choice) = &model.choice {
            return serde_json::to_string(&crate::shell::choice_a11y_tree(choice)).unwrap();
        }
        // Per-tile readings (the same phrasing the #board-sr mirror uses), indexed by
        // tile, so the a11y button labels and the text mirror narrate identically.
        let board_names: Vec<String> = recollect_core::cards::canon_catalog()
            .iter()
            .map(|c| crate::scene::short_board_name(&c.name))
            .collect();
        let labels = crate::scene::tile_readings(&model.view, &board_names);
        serde_json::to_string(&crate::shell::build_a11y_tree(&model, &labels)).unwrap()
    }

    /// The active **choice prompt** (Glimpse / Mulligan) as JSON
    /// ([`ChoicePrompt`](crate::shell::ChoicePrompt)), or `null` when none is in flight.
    /// The JS bridge reads it to know a modal is up (and what it is) without re-deriving
    /// the pending choice. Same `ui_json` the draw uses (so the mulligan offer agrees).
    pub fn choice_prompt_json(
        &self,
        cues_json: &str,
        interaction_json: &str,
        ui_json: &str,
    ) -> String {
        let model_json = self.shell_model_json(cues_json, interaction_json, ui_json);
        let model: crate::shell::ShellModel = match serde_json::from_str(&model_json) {
            Ok(m) => m,
            Err(_) => return "null".into(),
        };
        match &model.choice {
            Some(c) => serde_json::to_string(c).unwrap(),
            None => "null".into(),
        }
    }

    /// The active choice prompt's option hit-test rects for `(vw, vh)`, as JSON
    /// (`[{ "verb", "index", "x", "y", "w", "h" } …]`), or `[]` when no modal is up — so
    /// the JS bridge maps a canvas tap onto the right option (one source with the draw).
    pub fn choice_regions_json(
        &self,
        cues_json: &str,
        interaction_json: &str,
        ui_json: &str,
        vw: f32,
        vh: f32,
    ) -> String {
        let model_json = self.shell_model_json(cues_json, interaction_json, ui_json);
        let model: crate::shell::ShellModel = match serde_json::from_str(&model_json) {
            Ok(m) => m,
            Err(_) => return "[]".into(),
        };
        let Some(choice) = &model.choice else {
            return "[]".into();
        };
        let rects: Vec<serde_json::Value> = crate::shell::choice_regions(choice, vw, vh)
            .into_iter()
            .map(|(verb, index, r)| {
                serde_json::json!({ "verb": verb, "index": index, "x": r.x, "y": r.y, "w": r.w, "h": r.h })
            })
            .collect();
        serde_json::to_string(&rects).unwrap()
    }

    /// Apply a command (JSON) for the active seat; returns events or a reject.
    pub fn apply_json(&mut self, command_json: &str) -> String {
        match serde_json::from_str::<recollect_core::state::Command>(command_json) {
            Ok(cmd) => self.apply_cmd(cmd),
            Err(_) => "{\"reject\":\"Malformed\"}".to_string(),
        }
    }

    /// Interaction primitive — tap-to-place: play hand card `hand_index` onto
    /// `tile` (no engage; the shell's simple path). Events or a reject as JSON.
    pub fn play_at(&mut self, hand_index: u8, tile: u8) -> String {
        self.apply_cmd(recollect_core::state::Command::PlaySpirit {
            hand_index,
            tile,
            engage: None,
            chain_prefs: Vec::new(),
        })
    }

    /// Interaction primitive: end the active seat's turn.
    pub fn end_turn(&mut self) -> String {
        self.apply_cmd(recollect_core::state::Command::EndTurn)
    }

    /// Interaction primitive: Glimpse (+1 Anima, peek the top card; once per turn).
    pub fn study(&mut self) -> String {
        self.apply_cmd(recollect_core::state::Command::Glimpse)
    }

    /// Interaction primitive: resolve the pending choice by option `index`. For the
    /// §5 Glimpse this is the BURN card index (step 1), then `0` = keep the peeked card
    /// on top, `1` = bottom it for +1 Anima (step 2); for effect-driven choices
    /// (Peek/Target/Recover) it's the option index. The engine stays authoritative — an
    /// out-of-range or no-pending-choice call is rejected cleanly, never panics. The
    /// in-canvas Glimpse modal ([`crate::shell::ChoicePrompt`]) drives this; headless
    /// callers use it directly.
    pub fn choose(&mut self, index: u8) -> String {
        self.apply_cmd(recollect_core::state::Command::Choose { index })
    }

    /// Interaction primitive: take the opening **Mulligan** for the active seat
    /// (the London-lite redraw — draw a fresh full hand, bottom one card, the seed-chosen
    /// cost). Legal ONLY in the opening window (round 1, before you've acted, once);
    /// outside it the engine rejects cleanly. The in-canvas mulligan modal fires this.
    pub fn mulligan(&mut self) -> String {
        let seat = self.engine.state().active;
        self.apply_cmd(recollect_core::state::Command::Mulligan { seat })
    }

    /// Whether the active seat may open the Mulligan offer right now, as a bare bool
    /// for the JS bridge (it shows the opening modal at game start only while this holds).
    /// Sourced from `legal_commands` (the single truth — it offers `Command::Mulligan`
    /// exactly in the opening window), so the offer never draws past the opening or twice.
    pub fn can_mulligan(&self) -> bool {
        let seat = self.engine.state().active;
        self.mulligan_available(seat)
    }

    /// Whose turn it is (`"A"`/`"B"`) — so the shell knows when to hand to the AI.
    pub fn active_seat(&self) -> String {
        format!("{:?}", self.engine.state().active)
    }

    /// True once the match has ended.
    pub fn is_over(&self) -> bool {
        matches!(
            self.engine.state().phase,
            recollect_core::Phase::Finished { .. }
        )
    }

    /// The active seat's hand as card names (read-only context beside the moves).
    pub fn hand_names_json(&self) -> String {
        let seat = self.engine.state().active;
        let names: Vec<String> = self
            .engine
            .state()
            .player(seat)
            .hand
            .iter()
            .map(|c| self.engine.card(*c).name.clone())
            .collect();
        serde_json::to_string(&names).unwrap()
    }

    /// The result, for the end screen: `{"over":true,"result":"Win(A)","score_a":N,
    /// "score_b":M}` once finished, else `{"over":false}`.
    pub fn result_json(&self) -> String {
        match &self.engine.state().phase {
            recollect_core::Phase::Finished {
                result,
                score_a,
                score_b,
            } => format!(
                "{{\"over\":true,\"result\":\"{result:?}\",\"score_a\":{score_a},\"score_b\":{score_b}}}"
            ),
            _ => "{\"over\":false}".into(),
        }
    }

    /// The in-canvas **result screen** content as a
    /// [`ResultScreen`](crate::shell::ResultScreen) JSON, once the match has ended (else
    /// `null`). The verdict speaks in the game's voice — *the Memory keeps the winner* when a
    /// Lorekeeper holds the page, *the Memory is forgotten* when the Solace's erasure carried
    /// it, *both are kept* on a draw — tinted to the winner's ink. The breakdown lists each
    /// seat's board points and folds the Solace's off-board **erasure tally** into its row
    /// exactly as the HUD shows it. The actions adapt to the `mode` (`"bot"` / `"2v2"` /
    /// `"pvp"`): a bot match offers **Rematch / New opponent / Back to site**; PvP offers
    /// **Offer a rematch / New opponent / Back to site** (the rematch is an invite, not an
    /// instant reseed); 2v2 keeps the same shape from the active seat's vantage. The JS
    /// bridge folds this into `ui_json` (with a JS-owned reveal `progress`) so `draw_shell`
    /// renders it, and mirrors the verdict + actions into the virtual a11y tree (invariant 7).
    pub fn result_screen_json(&self, mode: &str) -> String {
        let st = self.engine.state();
        let (result, score_a, score_b) = match &st.phase {
            recollect_core::Phase::Finished {
                result,
                score_a,
                score_b,
            } => (*result, *score_a, *score_b),
            _ => return "null".into(),
        };
        // Board points per seat (one per held tile) + the Solace's off-board erasure tally
        // — the same split the HUD reads, so the breakdown and the running score agree.
        let (mut a_board, mut b_board) = (0u8, 0u8);
        for t in st.board.iter() {
            let owner = if let Some(sp) = &t.spirit {
                Some(if sp.fading {
                    sp.banished_by.unwrap_or(sp.owner)
                } else {
                    sp.owner
                })
            } else {
                t.impressions.first().copied()
            };
            match owner {
                Some(recollect_core::Seat::A) => a_board = a_board.saturating_add(1),
                Some(recollect_core::Seat::B) => b_board = b_board.saturating_add(1),
                None => {}
            }
        }
        let erasures = st.solace_erasures;
        let faction = |seat: recollect_core::Seat| st.rules.factions[seat as usize];
        // Name each seat by their CHARACTER when one is fielded ("the Memory
        // keeps Corin Ashe"), else the faction word.
        let word = |seat: recollect_core::Seat| self.seat_label(seat, &faction_word(faction(seat)));
        // In local 1v1 the human is seat A; the verdict reads from their vantage. (For
        // 2v2/PvP the same builder phrases by faction; the host wires the human seat.)
        let human = recollect_core::Seat::A;

        let screen = crate::shell::build_result_screen(
            result,
            score_a,
            score_b,
            a_board,
            b_board,
            erasures,
            faction(recollect_core::Seat::A),
            faction(recollect_core::Seat::B),
            &word(recollect_core::Seat::A),
            &word(recollect_core::Seat::B),
            human,
            mode,
        );
        serde_json::to_string(&screen).unwrap()
    }

    /// Invariant 7 — the **virtual a11y tree for the result screen**, as
    /// JSON (a list of [`A11yNode`](crate::shell::A11yNode)): the verdict + the score
    /// breakdown as a readout, then the Rematch / New opponent / Back to site actions as
    /// actionable buttons firing the same verbs the canvas buttons do. The JS bridge
    /// renders these into the off-screen `#shell-a11y` group so a keyboard / screen-reader
    /// user reaches the result at parity with the canvas. `null` (→ `[]`) mid-match.
    pub fn result_a11y_json(&self, mode: &str) -> String {
        let screen_json = self.result_screen_json(mode);
        let screen: crate::shell::ResultScreen = match serde_json::from_str(&screen_json) {
            Ok(s) => s,
            Err(_) => return "[]".into(),
        };
        serde_json::to_string(&crate::shell::result_a11y_tree(&screen)).unwrap()
    }

    /// The **Dusk / Nightfall set-piece** content for the current round, as a
    /// [`DuskSetPiece`](crate::shell::DuskSetPiece) JSON, or `null` if this round is not one
    /// of the binding beats (Dusk = the round after the contraction, `dusk_after + 1`;
    /// Nightfall = the last round). `progress` starts at 0 (the JS pacer ramps it to 1 and
    /// honours `prefers-reduced-motion`). Pairs with the `#status` live-region announcement
    /// Phase C already wired (`round_announcement`) — one source for the seal + the narration.
    pub fn dusk_set_piece_json(&self) -> String {
        let st = self.engine.state();
        let round = st.round;
        let dusk_after = st.rules.contraction_after;
        let last = recollect_core::engine::LAST_ROUND;
        let (kind, title, subtitle) = if round == last {
            ("nightfall", "Nightfall", "the Memory will keep what stands")
        } else if round == dusk_after + 1 {
            ("dusk", "Dusk falls", "the page begins to fail at its edges")
        } else {
            return "null".into();
        };
        serde_json::to_string(&crate::shell::DuskSetPiece {
            kind: kind.into(),
            title: title.into(),
            subtitle: subtitle.into(),
            progress: 0.0,
        })
        .unwrap()
    }

    /// Full card detail for the inspect panel (the TUI's `inspect_card`): name,
    /// kind, resonance, cost, A/D/H, keywords, rules text, reach — the same shape
    /// [`catalog_json`] emits per card.
    pub fn card_detail_json(&self, id: u16) -> String {
        card_json(self.engine.card(recollect_core::CardId(id))).to_string()
    }

    /// The card's reach as a board-sized grid for the inspect preview (the TUI's
    /// mini reach-grid): the threatened tiles for the card at board centre, facing
    /// up (the way seat A attacks). `{ "w": 5, "center": 12, "reach": [tiles…] }`.
    pub fn reach_grid_json(&self, id: u16) -> String {
        let c = self.engine.card(recollect_core::CardId(id));
        let w = self.engine.state().board_w as u8;
        let center = w * w / 2;
        let reach =
            recollect_core::engine::reach_tiles(c.reach, center, recollect_core::Seat::A, w);
        serde_json::json!({ "w": w, "center": center, "reach": reach }).to_string()
    }

    /// Every legal move for the active seat, each with a human-readable label and
    /// the command to apply — the shell renders these as buttons (the same
    /// legal-command-menu model that makes the TUI a complete client). The label is
    /// spoken text (it lands in the `#status` live region when a move resolves), so it
    /// is **all words, never glyphs** — a screen reader reads it as a sentence. JSON:
    /// `[{ "label": "Play Cinderling to c3, attacking d3", "cmd": <Command> }]`.
    pub fn legal_labeled_json(&self) -> String {
        let seat = self.engine.state().active;
        let w = self.engine.state().board_w;
        let labeled: Vec<serde_json::Value> = self
            .engine
            .legal_commands(seat)
            .into_iter()
            .map(|cmd| {
                // `forecast` mirrors the online wire shape (LegalMove.forecast), so
                // the frontend renders the local and networked menus identically.
                serde_json::json!({
                    "label": self.label(seat, &cmd, w),
                    "forecast": recollect_protocol::forecast(&self.engine, seat, &cmd),
                    "cmd": cmd,
                })
            })
            .collect();
        serde_json::to_string(&labeled).unwrap()
    }
}

/// The **online / 2v2 in-canvas shell** seam: the same whole-game canvas shell
/// the local 1v1 match draws ([`LocalGame`]), but driven by the **server's redacted
/// `PlayerView` / `TeamView` + its legal-move list** instead of a local engine (the
/// server is authoritative online). Online PvP is launch-critical, and 2v2 rides the
/// same shell.
///
/// It is **stateless** — every method takes the freshest `view_json` (a `PlayerView`
/// for 1v1, a `TeamView` for 2v2) + `legal_json` (the `[Command]` the server shipped)
/// + the JS-owned transient UI (`interaction_json` / `ui_json`, exactly as
/// [`LocalGame::shell_model_json`] takes them) + the seat display names, and rebuilds
/// the [`ShellModel`](crate::shell::ShellModel) purely (see [`crate::online`]). The JS
/// bridge holds the latest view/legal (from `onServerMsg`) and the transient UI, and
/// calls these to draw + mirror the a11y tree — the SAME `draw_shell` / `shell_regions`
/// / `build_a11y_tree` path the local shell uses.
///
/// **Redaction holds by construction:** the client only ever has the redacted view, so
/// the model can never carry an opponent's hand/deck — the opponent is counts/backs
/// only (1v1) or the opposing team's combined counts (2v2).
#[wasm_bindgen]
pub struct OnlineShell;

#[wasm_bindgen]
impl OnlineShell {
    #[wasm_bindgen(constructor)]
    pub fn new() -> OnlineShell {
        OnlineShell
    }

    /// Build the [`ShellModel`](crate::shell::ShellModel) JSON for the current online /
    /// 2v2 frame. `team` selects the 2v2 `TeamView` build (a 6×6 board, the opposing team
    /// as combined counts) over the 1v1 `PlayerView` build (the opponent as counts/backs).
    /// `interaction_json` is the board overlay (selection / legal-target glows / focus
    /// ring) and `ui_json` the JS-owned transient state (the lifted hand card, an
    /// in-progress drag, the floating inspect panel) — the SAME shapes
    /// [`LocalGame::shell_model_json`] consumes, so the online shell and the local shell
    /// draw through one path. A malformed view yields `{}` (the caller falls back).
    pub fn shell_model_json(
        &self,
        view_json: &str,
        legal_json: &str,
        team: bool,
        interaction_json: &str,
        ui_json: &str,
        you_name: &str,
        opp_name: &str,
    ) -> String {
        let Some(mut model) = self.base_model(view_json, legal_json, team, you_name, opp_name)
        else {
            return "{}".into();
        };
        // Overlay the JS-owned transient state the same way the local shell does: the
        // board interaction (selection / legal-target glows / focus) and the lifted hand
        // card / drag / inspect panel. The board affordances + score are already in `model`.
        model.interaction = serde_json::from_str(interaction_json).unwrap_or_default();
        let ui: ShellUi = serde_json::from_str(ui_json).unwrap_or_default();
        model.lifted_hand = ui.lifted_hand;
        model.hand_scroll = ui.hand_scroll;
        model.dragging = ui.dragging;
        model.drag_xy = ui.drag_xy;
        model.inspect = ui.inspect;
        serde_json::to_string(&model).unwrap()
    }

    /// The shell's interactive **hit-test regions** for a `vw`×`vh` viewport (the board
    /// rect + grid side, one rect per hand card, the FAB rects) — see
    /// [`LocalGame::shell_regions_json`]. The JS pointer bridge maps a canvas tap/drag
    /// through these (one source of truth with the draw).
    pub fn shell_regions_json(
        &self,
        view_json: &str,
        legal_json: &str,
        team: bool,
        interaction_json: &str,
        ui_json: &str,
        you_name: &str,
        opp_name: &str,
        vw: f32,
        vh: f32,
    ) -> String {
        let model_json = self.shell_model_json(
            view_json,
            legal_json,
            team,
            interaction_json,
            ui_json,
            you_name,
            opp_name,
        );
        let model: crate::shell::ShellModel = match serde_json::from_str(&model_json) {
            Ok(m) => m,
            Err(_) => return "{}".into(),
        };
        serde_json::to_string(&crate::shell::shell_regions(&model, vw, vh)).unwrap()
    }

    /// The **virtual a11y tree** (invariant 7) for the online / 2v2 shell — the board's
    /// actionable / occupied tiles, your hand, the opponent strip (counts only — never
    /// their cards), and the End-Turn + Glimpse buttons, each firing the same legal command
    /// its canvas affordance does. Identical to [`LocalGame::shell_a11y_json`], over the
    /// view-sourced model. While a Glimpse / Mulligan modal is up, the tree IS the
    /// choice's (its options as buttons).
    pub fn shell_a11y_json(
        &self,
        view_json: &str,
        legal_json: &str,
        team: bool,
        interaction_json: &str,
        ui_json: &str,
        you_name: &str,
        opp_name: &str,
    ) -> String {
        let model_json = self.shell_model_json(
            view_json,
            legal_json,
            team,
            interaction_json,
            ui_json,
            you_name,
            opp_name,
        );
        let model: crate::shell::ShellModel = match serde_json::from_str(&model_json) {
            Ok(m) => m,
            Err(_) => return "[]".into(),
        };
        // A Glimpse / Mulligan modal owns the tree while it's up (the board/hand plays are
        // blocked mid-choice) — one modal, one tree, exactly as the local shell does.
        if let Some(choice) = &model.choice {
            return serde_json::to_string(&crate::shell::choice_a11y_tree(choice)).unwrap();
        }
        let names: Vec<String> = recollect_core::cards::canon_catalog()
            .iter()
            .map(|c| crate::scene::short_board_name(&c.name))
            .collect();
        let labels = crate::scene::tile_readings(&model.view, &names);
        serde_json::to_string(&crate::shell::build_a11y_tree(&model, &labels)).unwrap()
    }

    /// The active **choice prompt** (Glimpse / Mulligan) JSON for the online frame, or
    /// `null` — so the JS bridge knows a modal is up (and what it is). Online surfaces the
    /// §5 Glimpse (the engine pending choice in YOUR redacted view); the opponent's never
    /// surfaces (redaction). The Mulligan offer rides `ui_json`'s `mulligan_offer` flag,
    /// honoured while the view shows the opening window (you haven't acted, round 1).
    pub fn choice_prompt_json(
        &self,
        view_json: &str,
        legal_json: &str,
        team: bool,
        ui_json: &str,
    ) -> String {
        let Some(mut model) = self.base_model(view_json, legal_json, team, "", "") else {
            return "null".into();
        };
        // The Mulligan is a legal command offered at the opening; mirror the local shell —
        // honour the JS offer flag only while the view still lists a Mulligan as legal.
        let ui: ShellUi = serde_json::from_str(ui_json).unwrap_or_default();
        if model.choice.is_none()
            && ui.mulligan_offer
            && model.your_turn
            && self.mulligan_legal(legal_json)
        {
            model.choice = Some(crate::shell::build_mulligan_prompt());
        }
        match &model.choice {
            Some(c) => serde_json::to_string(c).unwrap(),
            None => "null".into(),
        }
    }

    /// The active choice prompt's option hit-test rects for `(vw, vh)` (verb · index ·
    /// rect), or `[]` — the same shape [`LocalGame::choice_regions_json`] returns.
    pub fn choice_regions_json(
        &self,
        view_json: &str,
        legal_json: &str,
        team: bool,
        ui_json: &str,
        vw: f32,
        vh: f32,
    ) -> String {
        let prompt_json = self.choice_prompt_json(view_json, legal_json, team, ui_json);
        let prompt: crate::shell::ChoicePrompt = match serde_json::from_str(&prompt_json) {
            Ok(p) => p,
            Err(_) => return "[]".into(),
        };
        let rects: Vec<serde_json::Value> = crate::shell::choice_regions(&prompt, vw, vh)
            .into_iter()
            .map(|(verb, index, r)| {
                serde_json::json!({ "verb": verb, "index": index, "x": r.x, "y": r.y, "w": r.w, "h": r.h })
            })
            .collect();
        serde_json::to_string(&rects).unwrap()
    }

    /// Whether the opening **Mulligan** is offered right now (the view's legal list carries
    /// a `Mulligan` command) — so the JS bridge shows the opening modal only in the window.
    pub fn can_mulligan(&self, legal_json: &str) -> bool {
        self.mulligan_legal(legal_json)
    }

    /// The in-canvas **result screen** content once the match has ended (else `null`),
    /// as a [`ResultScreen`](crate::shell::ResultScreen) — the verdict (game's voice), the
    /// board+erasure breakdown, and the mode's actions. `mode` is `"pvp"` (online 1v1) or
    /// `"2v2"`: PvP relabels Rematch as an invite. Built purely from the view's `Finished`
    /// phase + the redacted tiles (board points) + the public erasure tally.
    pub fn result_screen_json(&self, view_json: &str, team: bool, mode: &str) -> String {
        crate::online::result_screen_json(view_json, team, mode)
    }

    /// The **virtual a11y tree for the result screen** (invariant 7) — the verdict +
    /// breakdown as a readout, each action a button. `[]` mid-match.
    pub fn result_a11y_json(&self, view_json: &str, team: bool, mode: &str) -> String {
        let screen_json = self.result_screen_json(view_json, team, mode);
        let screen: crate::shell::ResultScreen = match serde_json::from_str(&screen_json) {
            Ok(s) => s,
            Err(_) => return "[]".into(),
        };
        serde_json::to_string(&crate::shell::result_a11y_tree(&screen)).unwrap()
    }
}

impl Default for OnlineShell {
    fn default() -> Self {
        OnlineShell
    }
}

impl OnlineShell {
    /// Parse the view + legal JSON and build the base [`ShellModel`](crate::shell::ShellModel)
    /// (board · HUD · hand · affordances · score · cues), before the JS-owned transient
    /// overlay. `team` selects the 2v2 `TeamView` build. `None` if the view is malformed.
    fn base_model(
        &self,
        view_json: &str,
        legal_json: &str,
        team: bool,
        you_name: &str,
        opp_name: &str,
    ) -> Option<crate::shell::ShellModel> {
        let legal: Vec<recollect_core::state::Command> =
            serde_json::from_str(legal_json).unwrap_or_default();
        if team {
            let tv: recollect_core::view::TeamView = serde_json::from_str(view_json).ok()?;
            Some(crate::online::shell_model_for_team_view(
                &tv, &legal, you_name, opp_name,
            ))
        } else {
            let pv: recollect_core::view::PlayerView = serde_json::from_str(view_json).ok()?;
            Some(crate::online::shell_model_for_player_view(
                pv, &legal, you_name, opp_name,
            ))
        }
    }

    /// Whether the supplied legal list offers a `Mulligan` (the opening window is open).
    fn mulligan_legal(&self, legal_json: &str) -> bool {
        let legal: Vec<recollect_core::state::Command> =
            serde_json::from_str(legal_json).unwrap_or_default();
        legal
            .iter()
            .any(|c| matches!(c, recollect_core::state::Command::Mulligan { .. }))
    }
}

/// Tile coordinate like the board's column letters + 1-based row (a1, c3, …).
fn tile_name(t: u8, w: i8) -> String {
    let w = w.max(1) as u8;
    format!("{}{}", (b'a' + (t % w)) as char, t / w + 1)
}

/// A short, player-facing faction label for the opponent strip — "the
/// Solace" or "Lorekeepers", matching the design's register.
fn faction_word(f: recollect_core::types::Faction) -> String {
    match f {
        recollect_core::types::Faction::Solace => "the Solace".into(),
        recollect_core::types::Faction::Lorekeeper => "Lorekeepers".into(),
    }
}

/// The NAMED character a seat fields, drawn from the roster matching its
/// `faction` ([`recollect_core::quickplay::LOREKEEPER_CHARACTERS`] /
/// `SOLACE_CHARACTERS`). The Quick Play `style`/disposition (0–4) selects the
/// four-strong group so the name fits the deck the seat actually pilots; the `seed`
/// picks which of the four (so the same match always names the same character, and
/// different matches vary the face). Pure + re-derivable — and the within-group pick
/// uses the same wrapping the CLI/server roster indexing does, so the web names the
/// kind of character those surfaces would for an equivalent match.
fn pick_character(faction: recollect_core::types::Faction, style: u8, seed: u64) -> String {
    use recollect_core::quickplay::{LOREKEEPER_CHARACTERS, SOLACE_CHARACTERS};
    // Each disposition/style is four consecutive roster entries; the seed picks within.
    let within = ((seed >> 8) % 4) as usize;
    let group = (style as usize) % 5;
    let idx = group * 4 + within;
    match faction {
        recollect_core::types::Faction::Solace => SOLACE_CHARACTERS[idx % SOLACE_CHARACTERS.len()]
            .name
            .to_string(),
        recollect_core::types::Faction::Lorekeeper => LOREKEEPER_CHARACTERS
            [idx % LOREKEEPER_CHARACTERS.len()]
        .name
        .to_string(),
    }
}

/// One offered Quick Play style, serialized for the picker: the subjective `blurb`
/// (the voice) AND the objective `selection` — the four labelled
/// [`recollect_core::quickplay::SelectionInfo`] facets plus a one-line `summary`.
/// The web view; the labels' wording is authored in core (`quickplay`), so this is
/// purely the JSON shape the picker JS reads.
#[derive(serde::Serialize)]
struct OfferStyle {
    id: u8,
    name: &'static str,
    blurb: &'static str,
    selection: OfferSelection,
}

/// The objective selection-info for one style, as the picker renders it: the four
/// `facets` (each a dimension · value · detail, shown as a chip and read as text)
/// and the `summary` line (the chips' accessible roll-up).
#[derive(serde::Serialize)]
struct OfferSelection {
    facets: Vec<OfferFacet>,
    summary: String,
}

/// One labelled dimension of the selection-info, JSON-shaped for the picker chip
/// (and its accessible text): the dimension heading, the value word, and the gloss.
#[derive(serde::Serialize)]
struct OfferFacet {
    dimension: &'static str,
    value: &'static str,
    detail: &'static str,
}

impl From<&recollect_core::quickplay::DeckStyle> for OfferStyle {
    fn from(s: &recollect_core::quickplay::DeckStyle) -> Self {
        OfferStyle {
            id: s.id,
            name: s.name,
            blurb: s.blurb,
            selection: OfferSelection {
                facets: s
                    .selection
                    .facets()
                    .iter()
                    .map(|f| OfferFacet {
                        dimension: f.dimension,
                        value: f.value,
                        detail: f.detail,
                    })
                    .collect(),
                summary: s.selection.summary(),
            },
        }
    }
}

/// The transient, JS-owned UI state folded into the shell each frame:
/// the lifted hand card, an in-progress drag (+ the pointer position), the floating
/// inspect panel, and (Phase D) the JS-driven Dusk/Nightfall set-piece + result-screen
/// overlays (content engine-computed via `result_screen_json` / the round pacer; the
/// animation `progress` JS owns). The engine owns the *game* state; this is purely how
/// the player is currently touching it. Deserialized from `shell_model_json`'s `ui_json`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
struct ShellUi {
    #[serde(default)]
    lifted_hand: Option<u8>,
    #[serde(default)]
    hand_scroll: f32,
    #[serde(default)]
    dragging: bool,
    #[serde(default)]
    drag_xy: Option<(f32, f32)>,
    #[serde(default)]
    inspect: Option<crate::shell::Inspect>,
    #[serde(default)]
    dusk: Option<crate::shell::DuskSetPiece>,
    #[serde(default)]
    result: Option<crate::shell::ResultScreen>,
    /// Whether the JS bridge is showing the opening **Mulligan** offer. Unlike the
    /// Glimpse (an engine pending choice, derived from the view), the mulligan is a *legal
    /// command* offered at the opening, so the bridge owns the "offer / dismissed" state:
    /// it sets this at game open and clears it on either pick (Mulligan / Keep). The
    /// engine's `mulligan_window` still gates whether the offer is honoured (so a stale
    /// flag past the opening never draws the modal).
    #[serde(default)]
    mulligan_offer: bool,
}

/// The shell's affordances, derived from the engine's legal moves:
/// which board tiles can act, which Fading bases can evolve, which standing-Faded forms
/// can devolve (recede), which hand cards have a legal play, and which of those are
/// evolution form cards / recede bases (their target is a base / a faded form).
#[derive(Debug, Clone, Default)]
struct Affordances {
    actionable_tiles: Vec<u8>,
    evolvable_tiles: Vec<u8>,
    devolvable_tiles: Vec<u8>,
    actionable_hand: Vec<u8>,
    evolve_forms: Vec<u8>,
    devolve_bases: Vec<u8>,
}

impl LocalGame {
    /// Build a self-contained [`ShellModel`](crate::shell::ShellModel)
    /// from a **redacted `PlayerView` snapshot** (the watcher `you`'s view at some point
    /// in the opponent's replay) plus an optional replay caption overlay. Unlike
    /// [`Self::shell_model_json`] (which reads the LIVE engine), this builds the model
    /// purely from the snapshot view — so a paced beat carries its own complete frame
    /// and the JS pacer never re-derives state. The running score is computed from the
    /// snapshot's tiles (board points per seat) + the view's `solace_erasures` folded
    /// into whichever seat is the Solace; affordances are empty and `your_turn = false`
    /// (it's the opponent's turn — the player's controls are inert). Redaction holds:
    /// the snapshot is already `view_for(you)`.
    fn shell_model_for_view(
        &self,
        you: recollect_core::types::Seat,
        view: recollect_core::view::PlayerView,
        replay: Option<crate::shell::ReplayCaption>,
    ) -> crate::shell::ShellModel {
        use recollect_core::types::Faction;
        let opp = you.other();
        let st = self.engine.state();

        // Board points per seat, read from the redacted snapshot tiles (a held tile
        // scores for its standing spirit's owner, or — if empty — its impression). A
        // fading spirit still stands, so it scores for whoever the view shows as owner.
        let (mut a_board, mut b_board) = (0u8, 0u8);
        for t in &view.tiles {
            let owner = match &t.spirit {
                Some(sp) => Some(sp.owner),
                None => t.impression,
            };
            match owner {
                Some(recollect_core::Seat::A) => a_board = a_board.saturating_add(1),
                Some(recollect_core::Seat::B) => b_board = b_board.saturating_add(1),
                None => {}
            }
        }
        let erasures = view.solace_erasures;
        let seat_total = |seat: recollect_core::Seat, board: u8| -> (u8, u8) {
            if st.rules.factions[seat as usize] == Faction::Solace {
                (board.saturating_add(erasures), erasures)
            } else {
                (board, 0)
            }
        };
        let you_board = if you == recollect_core::Seat::A {
            a_board
        } else {
            b_board
        };
        let opp_board = if opp == recollect_core::Seat::A {
            a_board
        } else {
            b_board
        };
        let (you_score, _) = seat_total(you, you_board);
        let (opp_score, opp_erasures) = seat_total(opp, opp_board);

        // Your hand as placeholder cards (the catalog's full stat block), from the snapshot.
        let hand: Vec<crate::shell::HandCard> = view
            .you
            .hand
            .iter()
            .map(|id| {
                let c = self.engine.card(*id);
                crate::shell::HandCard {
                    name: c.name.clone(),
                    cost: c.cost,
                    attack: c.attack,
                    defense: c.defense,
                    hp: c.hp,
                    kind: format!("{:?}", c.kind),
                    resonance: format!("{:?}", c.resonance),
                }
            })
            .collect();

        let board_names: Vec<String> = recollect_core::cards::canon_catalog()
            .iter()
            .map(|c| crate::scene::short_board_name(&c.name))
            .collect();
        let opp_faction = faction_word(st.rules.factions[opp as usize]);
        let opp_name = self.seat_label(opp, &opp_faction);
        let you_faction = faction_word(st.rules.factions[you as usize]);
        let you_name = self.seat_label(you, &you_faction);
        let you_anima = view.you.anima;
        let opp_hand_count = view.opponent.hand_count;
        let round = view.round;

        crate::shell::ShellModel {
            you_seat: you,
            you_name,
            you_faction,
            you_score,
            you_anima,
            hand,
            opp_name,
            opp_faction,
            opp_score,
            opp_erasures,
            opp_hand_count,
            round,
            last_round: recollect_core::engine::LAST_ROUND,
            dusk_after: st.rules.contraction_after,
            your_turn: false, // the opponent is telling — your affordances are inert
            view,
            names: board_names,
            cues: crate::scene::MoveCues::default(),
            interaction: crate::scene::Interaction::default(),
            actionable_tiles: Vec::new(),
            evolvable_tiles: Vec::new(),
            devolvable_tiles: Vec::new(),
            actionable_hand: Vec::new(),
            evolve_forms: Vec::new(),
            devolve_bases: Vec::new(),
            lifted_hand: None,
            hand_scroll: 0.0,
            dragging: false,
            drag_xy: None,
            inspect: None,
            replay,
            dusk: None,
            result: None,
            // A replay snapshot is the OPPONENT's turn — your Glimpse/Mulligan is never in
            // flight then, and redaction keeps theirs off your view (invariant 2).
            choice: None,
        }
    }

    /// The player-facing label for `seat`: its named character (e.g. "Corin
    /// Ashe") when one is fielded this match, else the `faction` word fallback. Used by
    /// the HUD (your name) and the opponent strip (their name).
    fn seat_label(&self, seat: recollect_core::types::Seat, faction: &str) -> String {
        let name = &self.char_names[seat as usize];
        if name.is_empty() {
            faction.to_string()
        } else {
            name.clone()
        }
    }

    /// Whether `seat` may take the opening Mulligan right now (the engine's opening
    /// window is open). Sourced from `legal_commands` (the single truth — it offers
    /// `Command::Mulligan` exactly in the window), so the in-canvas offer never draws the
    /// modal past the opening, and never twice. (Not wasm-exported — `Seat` doesn't cross
    /// the ABI; the wasm seam is [`Self::can_mulligan`], which reads the active seat.)
    fn mulligan_available(&self, seat: recollect_core::types::Seat) -> bool {
        self.engine
            .legal_commands(seat)
            .iter()
            .any(|c| matches!(c, recollect_core::state::Command::Mulligan { .. }))
    }

    /// Compute the [`Affordances`] for `seat` from the engine's legal commands — the
    /// single source of truth, so the canvas dots/glyphs and the a11y tree agree with
    /// what the engine will actually accept. A board tile is "actionable" if it's the
    /// origin of a Move / the subject of an Evolve / Reclaim / Reveal /
    /// StrikeFabrication; a Fading base that an Evolve targets gets the evolve glyph
    /// (the matching form is in hand). A hand index is "actionable" if any legal
    /// command plays it; an Evolve's `form_hand` additionally marks it an evolve form.
    fn affordances(&self, seat: recollect_core::types::Seat) -> Affordances {
        use recollect_core::state::Command as C;
        use std::collections::BTreeSet;
        let st = self.engine.state();
        let mut tiles: BTreeSet<u8> = BTreeSet::new();
        let mut evolvable: BTreeSet<u8> = BTreeSet::new();
        let mut devolvable: BTreeSet<u8> = BTreeSet::new();
        let mut hand: BTreeSet<u8> = BTreeSet::new();
        let mut forms: BTreeSet<u8> = BTreeSet::new();
        let mut bases: BTreeSet<u8> = BTreeSet::new();
        for cmd in self.engine.legal_commands(seat) {
            match cmd {
                C::MoveSpirit { from, .. } => {
                    tiles.insert(from);
                }
                C::Reclaim { tile } | C::Reveal { tile, .. } => {
                    tiles.insert(tile);
                }
                C::StrikeFabrication { from, .. } => {
                    tiles.insert(from);
                }
                C::SetOrders { tile, .. } => {
                    tiles.insert(tile);
                }
                C::Evolve {
                    tile, form_hand, ..
                } => {
                    // The base can act (it can become); a Fading base earns the evolve
                    // glyph (a Primal rescue), and the form card in hand is an evolve form.
                    tiles.insert(tile);
                    if st.spirit_at(tile).is_some_and(|sp| sp.fading) {
                        evolvable.insert(tile);
                    }
                    hand.insert(form_hand);
                    forms.insert(form_hand);
                }
                // Devolution (§5): the standing-Faded form can recede a tier down. It earns
                // the DEVOLVE glyph (a downward chevron — it *recedes*, where evolve's
                // upward chevron *becomes*), and the base card in hand is a recede base.
                // The faded form is always `combat_faded` (the engine only offers Devolve in
                // the §0.5 window), so the scene already renders it as rescuable.
                C::Devolve { tile, base_hand } => {
                    tiles.insert(tile);
                    devolvable.insert(tile);
                    hand.insert(base_hand);
                    bases.insert(base_hand);
                }
                // Hand-card plays — the card has a legal play (an action dot on it).
                C::PlaySpirit { hand_index, .. }
                | C::Overwrite { hand_index, .. }
                | C::CastRitual { hand_index }
                | C::PlaceLandmark { hand_index, .. }
                | C::SetFabrication { hand_index, .. }
                | C::TellUnwriting { hand_index }
                | C::Release { hand_index } => {
                    hand.insert(hand_index);
                }
                C::AttachBond {
                    hand_index,
                    tile_a,
                    tile_b,
                } => {
                    hand.insert(hand_index);
                    // A Bond also acts on its two anchor pieces (a faint dot reads
                    // "these can be bonded"); keep them in the board affordances.
                    tiles.insert(tile_a);
                    tiles.insert(tile_b);
                }
                // §5 Mulligan: a whole-hand opening action, not anchored to a tile
                // or a single hand card — no board/hand affordance dot. The rich
                // canvas opening prompt is a later lane; the move still rides the
                // legal menu + ARIA action list.
                C::Glimpse
                | C::EndTurn
                | C::Choose { .. }
                | C::BanishStray
                | C::Mulligan { .. }
                | C::MatchAbandoned { .. } => {}
            }
        }
        Affordances {
            actionable_tiles: tiles.into_iter().collect(),
            evolvable_tiles: evolvable.into_iter().collect(),
            devolvable_tiles: devolvable.into_iter().collect(),
            actionable_hand: hand.into_iter().collect(),
            evolve_forms: forms.into_iter().collect(),
            devolve_bases: bases.into_iter().collect(),
        }
    }

    /// A compact, readable label for one command (card names + tile coords).
    ///
    /// This is **spoken text** — the JS bridge drops it into the `#status` live region
    /// when a move resolves — so it is written in plain words, never glyphs: a screen
    /// reader voices it as a natural phrase ("Move c3 to d3, attacking e3"), not a row
    /// of symbols a synthesiser reads as "crossed swords" or skips. The visible canvas
    /// draws from the scene primitives, not from this string, so the worded form is the
    /// accessible mirror only — the canvas glyph language is untouched.
    fn label(
        &self,
        seat: recollect_core::types::Seat,
        cmd: &recollect_core::state::Command,
        w: i8,
    ) -> String {
        use recollect_core::state::Command as C;
        let st = self.engine.state();
        let hand_name = |i: u8| {
            st.player(seat)
                .hand
                .get(i as usize)
                .map(|c| self.engine.card(*c).name.clone())
                .unwrap_or_else(|| "?".into())
        };
        let tn = |t: u8| tile_name(t, w);
        match cmd {
            C::MatchAbandoned { seat: who } => format!("{who:?} abandons the match (forfeit)"),
            C::Mulligan { .. } => "Mulligan (redraw, bottom one — opening only)".into(),
            C::Glimpse => "Glimpse (burn a hand card, then peek your top card)".into(),
            C::EndTurn => "End turn".into(),
            C::TellUnwriting { .. } => "Tell an Unwriting".into(),
            // The 1v1 canvas surfaces the §5 Glimpse as a rich in-canvas modal
            // (`shell::ChoicePrompt`, the burn / keep-or-bottom chips). This generic label is
            // the fallback for the labeled-menu (online / 2v2) path, which resolves a choice
            // by bare index.
            C::Choose { index } => format!("Choose option {index}"),
            C::SetOrders { tile, hold } => {
                format!(
                    "Orders {}: {}",
                    tn(*tile),
                    if *hold { "Hold" } else { "Watch" }
                )
            }
            C::Reveal { tile, engage } => {
                let mut s = format!("Reveal lurker {}", tn(*tile));
                if let Some(t) = engage {
                    s += &format!(", attacking {}", tn(*t));
                }
                s
            }
            C::CastRitual { hand_index } => format!("Cast {}", hand_name(*hand_index)),
            C::AttachBond {
                hand_index,
                tile_a,
                tile_b,
            } => {
                format!(
                    "Bond {}+{}: {}",
                    tn(*tile_a),
                    tn(*tile_b),
                    hand_name(*hand_index)
                )
            }
            C::PlaceLandmark { hand_index, tile } => {
                format!("Place Landmark {} at {}", hand_name(*hand_index), tn(*tile))
            }
            C::SetFabrication { hand_index, tile } => {
                format!("Set {} face-down at {}", hand_name(*hand_index), tn(*tile))
            }
            C::Evolve {
                tile,
                form_hand,
                fuel,
                engage,
            } => {
                let mut s = format!("Evolve {} with {}", tn(*tile), hand_name(*form_hand));
                match fuel {
                    Some(d) => s += &format!(", fuelled by {}", tn(*d)),
                    None => s += ", primal",
                }
                if let Some(t) = engage {
                    s += &format!(", attacking {}", tn(*t));
                }
                s
            }
            C::Devolve { tile, base_hand } => {
                // Vocabulary (law): the Lorekeeper REVERTS, the Solace RECEDES.
                let verb = match st.rules.factions[seat as usize] {
                    recollect_core::types::Faction::Solace => "Recede",
                    recollect_core::types::Faction::Lorekeeper => "Revert",
                };
                format!(
                    "{verb} {} with {} — the rescue",
                    tn(*tile),
                    hand_name(*base_hand)
                )
            }
            C::BanishStray => "Banish the surfaced Stray".into(),
            C::Reclaim { tile } => format!("Reclaim {} (cash back for Anima)", tn(*tile)),
            C::StrikeFabrication { from, tile } => {
                format!("Strike the lie {} from {}", tn(*tile), tn(*from))
            }
            C::Release { hand_index } => format!("Release {}", hand_name(*hand_index)),
            C::PlaySpirit {
                hand_index,
                tile,
                engage: None,
                ..
            } => {
                format!("Play {} to {}", hand_name(*hand_index), tn(*tile))
            }
            C::PlaySpirit {
                hand_index,
                tile,
                engage: Some(t),
                ..
            } => {
                format!(
                    "Play {} to {}, attacking {}{}",
                    hand_name(*hand_index),
                    tn(*tile),
                    tn(*t),
                    self.forecast_hand(seat, *hand_index, *t)
                )
            }
            C::Overwrite { hand_index, tile } => {
                format!(
                    "Overwrite {} with {}{}",
                    tn(*tile),
                    hand_name(*hand_index),
                    self.forecast_hand(seat, *hand_index, *tile)
                )
            }
            C::MoveSpirit { from, to, engage } => {
                let mut s = format!("Move {} to {}", tn(*from), tn(*to));
                if let Some(t) = engage {
                    s += &format!(", attacking {}", tn(*t));
                }
                s
            }
        }
    }

    /// A combat forecast in **spoken words** for an arrival that engages tile `t` with
    /// the hand card at `hand_index`, via the shared `forecast_exchange`. It is appended
    /// to the move's spoken label (the `#status` live region), so it reads as a clause a
    /// screen reader can voice — "you would deal 3 and banish it, taking 2 — you would
    /// fall; their Echo answers" — never the terse bracketed `[deal 3 BANISH · take 2]`
    /// form (the middot and the shout would read badly). Empty if there's no defender at `t`.
    fn forecast_hand(&self, seat: recollect_core::types::Seat, hand_index: u8, t: u8) -> String {
        let st = self.engine.state();
        let Some(&cid) = st.player(seat).hand.get(hand_index as usize) else {
            return String::new();
        };
        let ac = self.engine.card(cid);
        let Some(dfn) = st.spirit_at(t) else {
            return String::new();
        };
        let dc = self.engine.card(dfn.card);
        let f = recollect_core::engine::forecast_exchange(
            ac,
            ac.attack,
            ac.defense,
            ac.hp,
            ac.hp,
            dfn,
            dc,
            0,
            self.engine.warded_at(t),
        );
        let mut s = format!("; you would deal {}", f.to_defender);
        if f.banishes_defender {
            s += " and banish it";
        }
        s += &format!(", taking {}", f.to_attacker);
        if f.banishes_attacker {
            s += " — you would fall";
        }
        if f.defender_echo_live {
            s += "; their Echo answers";
        }
        s
    }

    fn apply_cmd(&mut self, cmd: recollect_core::state::Command) -> String {
        let seat = self.engine.state().active;
        match self.engine.apply(seat, cmd) {
            Ok(events) => serde_json::to_string(&events).unwrap(),
            Err(r) => format!("{{\"reject\":\"{r:?}\"}}"),
        }
    }
}

#[cfg(test)]
mod interaction {
    //! The tap-to-command primitives the wgpu shell drives apply or reject
    //! cleanly (never panic) — the engine stays authoritative.
    use super::LocalGame;

    #[test]
    fn tap_primitives_apply_or_reject_cleanly() {
        let mut g = LocalGame::new(7);
        // A blatantly out-of-range target is rejected, not panicked.
        assert!(g.play_at(0, 250).contains("reject"), "bad tile rejected");
        // Glimpse (§5): Glimpse is legal at turn start and opens the BURN choice; a
        // second Glimpse is barred while any Glimpse choice is pending.
        assert!(!g.study().contains("reject"), "Glimpse legal at turn start");
        assert!(
            g.study().contains("reject"),
            "Glimpse barred while the Glimpse burn choice is pending"
        );
        // EndTurn is blocked until the choices resolve — the engine stays authoritative.
        assert!(
            g.end_turn().contains("reject"),
            "EndTurn blocked while a choice is pending"
        );
        // Resolve step 1 (burn a hand card); the keep-or-bottom choice opens.
        assert!(!g.choose(0).contains("reject"), "burn resolves cleanly");
        assert!(
            g.study().contains("reject"),
            "Glimpse still barred while the keep-or-bottom choice is pending"
        );
        // Resolve step 2 (keep the card); now the once-per-turn flag is spent.
        assert!(!g.choose(0).contains("reject"), "keep resolves cleanly");
        assert!(
            g.study().contains("reject"),
            "Glimpse only once per turn (flag spent)"
        );
        // Ending the turn is now legal and passes to the other seat.
        assert!(!g.end_turn().contains("reject"), "EndTurn legal");
        assert_eq!(g.active_seat(), "B", "turn passed to B");
        assert!(!g.is_over(), "the match continues");
    }

    // ── the in-canvas Glimpse + Mulligan choice surfaces in the live shell model ──
    /// Parse the live shell model's `choice` field (the in-canvas prompt), or `None`.
    fn live_choice(g: &LocalGame) -> Option<crate::shell::ChoicePrompt> {
        let model: crate::shell::ShellModel =
            serde_json::from_str(&g.shell_model_json("{}", "{}", "{}")).unwrap();
        model.choice
    }

    #[test]
    fn the_glimpse_choice_surfaces_in_the_canvas_model_at_both_steps() {
        let mut g = LocalGame::new(7);
        // No choice at rest.
        assert!(live_choice(&g).is_none(), "no modal at turn start");
        // Open the Glimpse (Glimpse): the BURN step surfaces in the canvas model.
        assert!(!g.study().contains("reject"));
        let burn = live_choice(&g).expect("the burn step is a canvas modal");
        assert_eq!(burn.kind, "glimpse_burn");
        assert!(!burn.options.is_empty(), "one chip per burnable hand card");
        assert!(burn.options.iter().all(|o| o.verb == "choose" && o.cost));
        // The a11y tree mirrors the SAME options (invariant 7) — actionable buttons.
        let tree: Vec<crate::shell::A11yNode> =
            serde_json::from_str(&g.shell_a11y_json("{}", "{}", "{}")).unwrap();
        assert!(
            tree.iter().any(|n| n.id == "section-choice"),
            "the choice is the active a11y section"
        );
        let buttons = tree.iter().filter(|n| n.role == "button").count();
        assert_eq!(buttons, burn.options.len(), "every chip is an a11y button");
        // Resolve the burn → the KEEP/BOTTOM step surfaces, floating the peeked top card.
        assert!(!g.choose(0).contains("reject"));
        let keep = live_choice(&g).expect("the keep step is a canvas modal");
        assert_eq!(keep.kind, "glimpse_keep");
        assert!(keep.peeked.is_some(), "the peeked top card is floated");
        assert_eq!(keep.options.len(), 2, "Keep / Bottom");
        // Resolve keep → the modal clears.
        assert!(!g.choose(0).contains("reject"));
        assert!(live_choice(&g).is_none(), "the modal clears once resolved");
    }

    #[test]
    fn the_canvas_modal_mirrors_only_the_owner_pending_choice_redaction() {
        // Invariant 2 redaction: the canvas modal is sourced from `view.you.pending`,
        // which `view_for` shows to the choice's OWNER only — so a choice never surfaces on
        // a view that doesn't own it (the opponent sees only THAT a choice is in flight, via
        // `phase`, never the burnable hand / the peek / the outcome). With A's Glimpse in
        // flight (active = A): the model built from A's view DOES carry the modal, and the
        // model's `choice.is_some()` tracks A's `view.you.pending.is_some()` exactly (never
        // a leak from any other seat).
        let mut g = LocalGame::new(7);
        let _ = g.study(); // A's Glimpse burn is pending (active = A)
        let model: crate::shell::ShellModel =
            serde_json::from_str(&g.shell_model_json("{}", "{}", "{}")).unwrap();
        assert!(
            model.choice.is_some(),
            "A owns the pending choice — A sees the modal"
        );
        assert_eq!(
            model.choice.is_some(),
            model.view.you.pending.is_some(),
            "the modal mirrors ONLY this view's owner pending choice"
        );
        // The pending choice's burnable list (A's own hand) rides under `burnable`, by design
        // — NEVER a `"hand"` key on the pending choice — so the protocol's opponent-leak probe
        // can't false-positive on it. The peeked top, when present, is also owner-only.
        let pending_json = serde_json::to_string(&model.view.you.pending).unwrap();
        assert!(
            !pending_json.contains("\"hand\""),
            "the pending choice never carries a `hand` key (the leak-probe contract)"
        );
    }

    #[test]
    fn the_opening_mulligan_offer_surfaces_and_clears() {
        let mut g = LocalGame::new(7);
        // The opening window is open at the very start.
        assert!(g.can_mulligan(), "mulligan offered at the opening");
        // With the JS offer flag, the canvas model carries the mulligan modal.
        let ui = r#"{"mulligan_offer":true}"#;
        let model: crate::shell::ShellModel =
            serde_json::from_str(&g.shell_model_json("{}", "{}", ui)).unwrap();
        let prompt = model.choice.expect("the mulligan modal");
        assert_eq!(prompt.kind, "mulligan");
        assert_eq!(prompt.options.len(), 2, "Mulligan / Keep");
        assert_eq!(prompt.options[0].verb, "mulligan");
        assert_eq!(prompt.options[1].verb, "keep");
        // Taking the mulligan resolves it; the window closes (once per opening).
        assert!(!g.mulligan().contains("reject"), "mulligan applies");
        assert!(!g.can_mulligan(), "the window is spent after one mulligan");
        // Even with the (stale) offer flag set, no modal draws past the window.
        let after: crate::shell::ShellModel =
            serde_json::from_str(&g.shell_model_json("{}", "{}", ui)).unwrap();
        assert!(
            after.choice.is_none(),
            "a stale offer flag never draws past the window"
        );
    }

    #[test]
    fn a_mulligan_offer_never_overrides_a_pending_glimpse() {
        // A pending Glimpse takes precedence over the mulligan offer (you can't open the
        // opening mulligan mid-glimpse) — the modal shows the glimpse, not the mulligan.
        let mut g = LocalGame::new(7);
        let _ = g.study(); // a Glimpse burn is now pending
        let ui = r#"{"mulligan_offer":true}"#;
        let model: crate::shell::ShellModel =
            serde_json::from_str(&g.shell_model_json("{}", "{}", ui)).unwrap();
        assert_eq!(
            model.choice.map(|c| c.kind),
            Some("glimpse_burn".to_string()),
            "the pending glimpse wins over the mulligan offer"
        );
    }

    #[test]
    fn auto_play_turn_paced_yields_an_announced_beat_stream_for_the_watcher() {
        // The bot's turn comes back as an ORDERED stream of paced beats,
        // each carrying a caption, an a11y announcement, the affected tiles, and a
        // self-contained shell model snapshot — so the JS pacer is fully data-driven.
        let mut g = LocalGame::new(7);
        // The match-start opener always names the first player (the human, seat A).
        assert!(
            g.opener_announcement().contains("open"),
            "the opener announcement names who opens: {}",
            g.opener_announcement()
        );
        // Hand the turn to the bot, then replay it.
        assert!(!g.end_turn().contains("reject"));
        assert_eq!(g.active_seat(), "B");
        let env: serde_json::Value = serde_json::from_str(&g.auto_play_turn_paced()).unwrap();
        let beats = env["beats"].as_array().expect("a beats array");
        assert!(
            !beats.is_empty(),
            "the bot took at least one watchable action"
        );
        for b in beats {
            // Every beat carries a non-trivial caption + the matching a11y announcement.
            let cap = b["caption"].as_str().unwrap();
            assert!(cap.len() > 4, "the caption names the action: {cap}");
            assert_eq!(
                b["announce"].as_str().unwrap(),
                cap,
                "announce == caption (one source)"
            );
            assert!(b["tiles"].is_array(), "the affected tiles ride along");
            // The self-contained snapshot model is a valid ShellModel with the replay
            // overlay set and the player's controls inert.
            let model = &b["model"];
            assert_eq!(
                model["your_turn"].as_bool(),
                Some(false),
                "your controls are inert"
            );
            assert!(
                model["replay"]["text"].as_str().is_some(),
                "the model carries the caption"
            );
            assert!(
                model["actionable_hand"].as_array().unwrap().is_empty(),
                "no affordances mid-replay"
            );
        }
        // The replay left the turn back with the human (or finished) — the engine, not
        // the pacer, owns the state; paced replay never changes who is to act.
        assert!(g.active_seat() == "A" || g.is_over());
    }

    #[test]
    fn the_paced_replay_never_reveals_the_opponents_hand() {
        // Redaction holds across the replay: every per-beat snapshot is the HUMAN's
        // view — the opponent's hand is a COUNT, never enumerated cards.
        let mut g = LocalGame::new(3);
        g.end_turn();
        let env: serde_json::Value = serde_json::from_str(&g.auto_play_turn_paced()).unwrap();
        for b in env["beats"].as_array().unwrap() {
            let model = &b["model"];
            // `you.hand` is the WATCHER's own hand (seat A); the opponent strip carries
            // only a count. The snapshot's view.seat must be the human (A), never the bot.
            assert_eq!(
                model["view"]["seat"].as_str(),
                Some("A"),
                "snapshots are the human's view"
            );
            assert!(
                model["opp_hand_count"].as_u64().is_some(),
                "the opponent's hand is a count, never named cards"
            );
            // The redacted view never lists the opponent's cards anywhere.
            assert!(
                model["view"]["opponent"].get("hand").is_none(),
                "no opponent hand array leaks"
            );
        }
    }

    #[test]
    fn move_cues_and_score_readout_expose_the_phase3_state() {
        // The shell reads the running score (board + the Solace's erasure
        // tally) and the movement cues from the engine — neither is in the view.
        let g = LocalGame::new(7);
        let cues: serde_json::Value = serde_json::from_str(&g.move_cues_json()).unwrap();
        assert!(cues["movable"].is_array(), "movable tiles listed");
        assert!(cues["sick"].is_array(), "summoning-sick tiles listed");
        let sc: serde_json::Value = serde_json::from_str(&g.score_readout_json()).unwrap();
        // The readout always carries the split, and b_total folds in the erasures.
        let b_board = sc["b_board"].as_u64().unwrap();
        let erasures = sc["erasures"].as_u64().unwrap();
        assert_eq!(
            sc["b_total"].as_u64().unwrap(),
            b_board + erasures,
            "B's total = board points + the off-board erasure tally"
        );
        assert!(sc["a"].as_u64().is_some(), "Seat A's board score present");
    }

    #[test]
    fn legal_moves_are_labeled_and_applicable() {
        let g = LocalGame::new(7);
        let labeled: serde_json::Value = serde_json::from_str(&g.legal_labeled_json()).unwrap();
        let moves = labeled.as_array().unwrap();
        assert!(!moves.is_empty(), "the opening offers legal moves");
        assert!(
            moves
                .iter()
                .all(|m| m["label"].as_str().is_some_and(|s| !s.is_empty())),
            "every move carries a readable label"
        );
        // A play move names a card and a tile in WORDS ("Play <card> to <tile>") — the
        // label is spoken in the live region, so it reads as a phrase, not "Play X → c3".
        assert!(
            moves
                .iter()
                .any(|m| m["label"].as_str().unwrap().contains(" to ")),
            "play/place moves name a card and a tile in words"
        );
        // The spoken labels are GLYPH-FREE (invariant 7 / the a11y bar): no arrow / sword /
        // markers that a screen reader would read as "rightwards arrow" / "crossed swords"
        // or silently drop. The canvas keeps its glyphs; only this spoken mirror is worded.
        for m in moves {
            let label = m["label"].as_str().unwrap();
            for glyph in ['→', '←', '⚔', '·', '°', '⌂', '▒', '░', '★', '●'] {
                assert!(
                    !label.contains(glyph),
                    "spoken label {label:?} must be all words, not the glyph {glyph:?}"
                );
            }
        }
        // Each `cmd` round-trips to a real Command (the apply path the shell uses).
        assert!(moves.iter().all(|m| {
            serde_json::from_value::<recollect_core::state::Command>(m["cmd"].clone()).is_ok()
        }));
        // Hand names are exposed for the context strip (5 dealt + the Flow draw).
        let hand: Vec<String> = serde_json::from_str(&g.hand_names_json()).unwrap();
        assert!(hand.len() >= 5, "the opening hand is named");
    }

    #[test]
    fn card_detail_exposes_stats_keywords_and_text() {
        let g = LocalGame::new(7);
        let d: serde_json::Value = serde_json::from_str(&g.card_detail_json(0)).unwrap();
        assert!(d["name"].as_str().is_some_and(|s| !s.is_empty()), "named");
        assert!(d["reach"].as_str().is_some(), "reach shown");
        assert!(d["attack"].as_i64().is_some(), "A/D/H shown");
        assert!(d["keywords"].is_array(), "keywords listed");
    }

    #[test]
    fn reach_grid_exposes_the_threatened_tiles() {
        let g = LocalGame::new(7);
        let rg: serde_json::Value = serde_json::from_str(&g.reach_grid_json(0)).unwrap();
        assert_eq!(rg["w"].as_u64().unwrap(), 5, "1v1 board width");
        assert_eq!(rg["center"].as_u64().unwrap(), 12, "centre tile of a 5×5");
        assert!(
            rg["reach"].as_array().is_some_and(|a| !a.is_empty()),
            "the card threatens some tiles"
        );
    }

    #[test]
    fn reach_grid_by_id_centres_on_a_central_tile_for_both_board_widths() {
        // The engine-free online/2v2 inspect grid: the centre must be a CENTRAL tile, not an
        // edge — `w*w/2` lands on the left edge for an even width (18 = row 3, col 0 on 6×6),
        // so the centre is computed by row+col. 5×5 ⇒ 12 (true centre); 6×6 ⇒ 21 (row 3, col 3).
        let rg5: serde_json::Value =
            serde_json::from_str(&super::reach_grid_by_id_json(0, 5)).unwrap();
        assert_eq!(rg5["center"].as_u64().unwrap(), 12, "5×5 centre tile");
        let rg6: serde_json::Value =
            serde_json::from_str(&super::reach_grid_by_id_json(0, 6)).unwrap();
        let c = rg6["center"].as_u64().unwrap();
        assert_eq!(
            c, 21,
            "6×6 centre tile (row 3, col 3 — not the edge tile 18)"
        );
        // A central tile is not on the left or right column (col 0 or col w-1).
        assert_ne!(c % 6, 0, "the centre is not on the left edge");
        assert_ne!(c % 6, 5, "the centre is not on the right edge");
    }

    #[test]
    fn a_2v2_game_auto_plays_without_panic() {
        // The 6×6 watch-AI mode: the bot drives all four slots; each
        // `auto_play_turn` advances the active slot's turn. Proves the bot and the
        // slot rotation hold together (the shell renders the TeamView each step).
        let mut g = LocalGame::new_2v2(11);
        assert!(g.is_2v2());
        assert!(
            !g.team_view_json().is_empty(),
            "the active slot has a TeamView"
        );
        for _ in 0..24 {
            if g.is_over() {
                break;
            }
            g.auto_play_turn();
        }
    }

    #[test]
    fn networked_command_builders_emit_real_commands() {
        use recollect_core::state::Command;
        // The online client builds these in Rust and ships them over the wire; the
        // server parses the same `Command`. Round-trip each shape.
        let parse = |s: String| serde_json::from_str::<Command>(&s).unwrap();
        assert_eq!(parse(super::cmd_end_turn_json()), Command::EndTurn);
        assert_eq!(parse(super::cmd_study_json()), Command::Glimpse);
        assert_eq!(
            parse(super::cmd_play_spirit_json(2, 13)),
            Command::PlaySpirit {
                hand_index: 2,
                tile: 13,
                engage: None,
                chain_prefs: vec![]
            }
        );
        assert_eq!(
            parse(super::cmd_release_json(1)),
            Command::Release { hand_index: 1 }
        );
    }

    #[test]
    fn catalog_json_is_a_complete_card_database_for_the_frontend() {
        let cat: Vec<serde_json::Value> = serde_json::from_str(&super::catalog_json()).unwrap();
        assert!(
            cat.len() > 100,
            "the whole catalog is exported: {}",
            cat.len()
        );
        // Every entry carries the fields a frontend renders, keyed by id.
        for (i, c) in cat.iter().enumerate() {
            assert_eq!(c["id"].as_u64(), Some(i as u64), "id-ordered, keyed by id");
            assert!(c["name"].as_str().is_some_and(|s| !s.is_empty()), "named");
            assert!(c["reach"].as_str().is_some(), "reach present");
            assert!(c["keywords"].is_array(), "keywords listed");
            assert!(c["attack"].as_i64().is_some(), "A/D/H present");
        }
        // The in-game detail panel and the catalog agree (shared shape).
        let g = super::LocalGame::new(1);
        let detail: serde_json::Value = serde_json::from_str(&g.card_detail_json(0)).unwrap();
        assert_eq!(detail["name"], cat[0]["name"]);

        // reach_tiles works with no running game; some card threatens tiles from
        // centre, and an out-of-range id is empty (not a panic).
        let any_reach = (0..cat.len() as u16).take(40).any(|id| {
            !serde_json::from_str::<Vec<u8>>(&super::reach_tiles_json(id, 12, 5))
                .unwrap()
                .is_empty()
        });
        assert!(any_reach, "at least one card has a reach grid");
        assert_eq!(super::reach_tiles_json(60000, 12, 5), "[]");
    }

    // ── affordances + the regions / a11y JSON exports ────────────

    #[test]
    fn shell_affordances_match_the_engines_legal_moves() {
        // The action dots / evolve glyphs are derived from the engine's legal moves,
        // so the canvas + a11y tree never offer something the engine would reject.
        let g = LocalGame::new(7);
        let seat = g.engine.state().active;
        let aff = g.affordances(seat);
        let legal = g.engine.legal_commands(seat);
        use recollect_core::state::Command as C;
        // Every actionable hand index is the hand_index of SOME legal play.
        for &i in &aff.actionable_hand {
            assert!(
                legal.iter().any(|c| matches!(c,
                    C::PlaySpirit { hand_index, .. }
                    | C::Overwrite { hand_index, .. }
                    | C::CastRitual { hand_index }
                    | C::PlaceLandmark { hand_index, .. }
                    | C::SetFabrication { hand_index, .. }
                    | C::TellUnwriting { hand_index }
                    | C::AttachBond { hand_index, .. }
                    | C::Release { hand_index }
                    | C::Evolve { form_hand: hand_index, .. } if *hand_index == i)),
                "actionable hand {i} corresponds to a legal play"
            );
        }
        // The opening has at least one legal play (Glimpse aside), so some affordance
        // shows — either a hand card or a board piece can act.
        assert!(
            !aff.actionable_hand.is_empty() || !aff.actionable_tiles.is_empty(),
            "the opening surfaces at least one affordance"
        );
    }

    #[test]
    fn shell_model_regions_and_a11y_json_are_well_formed() {
        let g = LocalGame::new(7);
        let ui = "{}";
        // The shell model JSON now carries the Phase-B affordance fields.
        let model: serde_json::Value =
            serde_json::from_str(&g.shell_model_json("{}", "{}", ui)).unwrap();
        for f in [
            "actionable_tiles",
            "evolvable_tiles",
            "actionable_hand",
            "evolve_forms",
        ] {
            assert!(model[f].is_array(), "shell model carries {f}");
        }
        // Regions: a board rect + grid side, a hand rect per card, two FAB rects.
        let reg: serde_json::Value =
            serde_json::from_str(&g.shell_regions_json("{}", "{}", ui, 400.0, 800.0)).unwrap();
        assert_eq!(reg["board_w"].as_u64().unwrap(), 5);
        assert!(reg["board"]["w"].as_f64().unwrap() > 0.0);
        assert!(
            reg["hand"].as_array().unwrap().len() >= 5,
            "a rect per hand card"
        );
        assert!(reg["end_turn"]["w"].as_f64().unwrap() > 0.0);
        assert!(reg["study"]["w"].as_f64().unwrap() > 0.0);
        // The a11y tree: a flat list with the structural sections + actionable nodes.
        let tree: Vec<serde_json::Value> =
            serde_json::from_str(&g.shell_a11y_json("{}", "{}", ui)).unwrap();
        let ids: Vec<&str> = tree.iter().filter_map(|n| n["id"].as_str()).collect();
        assert!(ids.contains(&"section-board"));
        assert!(ids.contains(&"section-hand"));
        assert!(ids.contains(&"fab-end"));
        assert!(ids.contains(&"fab-study"));
        // Every FAB node names a target with the matching verb (parity with canvas).
        let fab_end = tree.iter().find(|n| n["id"] == "fab-end").unwrap();
        assert_eq!(fab_end["role"], "button");
        assert_eq!(fab_end["target"]["Action"], "EndTurn");
        // Each hand card surfaces an actionable Hand-target button.
        let hand_buttons = tree
            .iter()
            .filter(|n| n["id"].as_str().is_some_and(|s| s.starts_with("hand-")))
            .count();
        assert!(hand_buttons >= 5, "a button per hand card");
    }

    #[test]
    fn shell_ui_state_flows_into_the_model() {
        // The JS-owned transient UI state (lifted card, drag, inspect) round-trips
        // through ui_json into the model the renderer draws.
        let g = LocalGame::new(7);
        let ui = serde_json::json!({
            "lifted_hand": 1,
            "dragging": true,
            "drag_xy": [120.0, 300.0],
            "inspect": {
                "name": "Cinderling", "kind": "Spirit", "resonance": "Ember",
                "cost": 2, "attack": 3, "defense": 1, "hp": 2, "reach": "Cross",
                "keywords": ["Mobile"], "rules": "drifts on the wind",
                "reach_w": 5, "reach_center": 12, "reach_tiles": [7, 13],
                "anchor": [200.0, 600.0]
            }
        })
        .to_string();
        let model: serde_json::Value =
            serde_json::from_str(&g.shell_model_json("{}", "{}", &ui)).unwrap();
        assert_eq!(model["lifted_hand"].as_u64().unwrap(), 1);
        assert!(model["dragging"].as_bool().unwrap());
        assert_eq!(model["inspect"]["name"], "Cinderling");
        // A malformed ui_json degrades gracefully (no lift / drag / inspect).
        let bare: serde_json::Value =
            serde_json::from_str(&g.shell_model_json("{}", "{}", "not json")).unwrap();
        assert!(bare["lifted_hand"].is_null());
        assert!(!bare["dragging"].as_bool().unwrap());
    }

    // ── the result screen + the Dusk/Nightfall set-piece JSON ──────────

    #[test]
    fn result_screen_json_is_null_until_the_match_ends_then_carries_a_verdict() {
        use recollect_core::state::Command;
        let mut g = LocalGame::new(7);
        // Mid-match: no result screen.
        assert_eq!(g.result_screen_json("bot"), "null");
        assert_eq!(g.result_a11y_json("bot"), "[]");
        // Drive the match to its end by abandoning (a system forfeit ⇒ Finished). This
        // is the cleanest deterministic way to reach a verdict in a unit test.
        let seat = g.engine.state().active;
        g.engine
            .apply(seat, Command::MatchAbandoned { seat })
            .expect("abandon ends the match");
        assert!(g.is_over());
        // Now the result screen carries a verdict (game's voice), a breakdown, and the
        // three actions — Rematch / New opponent / Back to site.
        let res: serde_json::Value = serde_json::from_str(&g.result_screen_json("bot")).unwrap();
        assert!(
            res["verdict"].as_str().is_some_and(|v| v.len() > 8),
            "the verdict speaks: {:?}",
            res["verdict"]
        );
        let verbs: Vec<&str> = res["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["verb"].as_str().unwrap())
            .collect();
        assert_eq!(verbs, vec!["rematch", "new", "site"]);
        assert!(
            res["breakdown"].as_array().is_some_and(|b| !b.is_empty()),
            "the score breakdown is listed"
        );
        // The result a11y tree mirrors the verdict + the actions as actionable buttons.
        let tree: Vec<serde_json::Value> =
            serde_json::from_str(&g.result_a11y_json("bot")).unwrap();
        let ids: Vec<&str> = tree.iter().filter_map(|n| n["id"].as_str()).collect();
        assert!(ids.contains(&"section-result"));
        assert!(ids.contains(&"result-rematch"));
        let rematch = tree.iter().find(|n| n["id"] == "result-rematch").unwrap();
        assert_eq!(rematch["role"], "button");
        assert_eq!(rematch["target"]["Action"], "rematch");

        // The action hit-test rects (via the renderer) cover each verb on-screen. (We
        // can't construct a WebRenderer off-wasm, so assert the pure shell export here.)
        let screen: crate::shell::ResultScreen =
            serde_json::from_str(&g.result_screen_json("bot")).unwrap();
        let rects = crate::shell::result_action_rects(&screen, 400.0, 800.0);
        assert_eq!(rects.len(), 3, "a rect per action");
        assert!(rects.iter().all(|(_, r)| r.w > 0.0 && r.h > 0.0));
    }

    #[test]
    fn dusk_set_piece_json_only_fires_on_the_binding_beats() {
        // Round 1 (the opening): not a binding beat ⇒ null.
        let g = LocalGame::new(7);
        assert_eq!(g.dusk_set_piece_json(), "null", "no set-piece on round 1");
        // Force the round to the Dusk beat (the round AFTER the contraction) and Nightfall;
        // the set-piece content fires with the right kind + seal.
        let mut g2 = LocalGame::new(7);
        let dusk_after = g2.engine.state().rules.contraction_after;
        {
            let st = g2.engine.state_mut_for_test();
            st.round = dusk_after + 1;
        }
        let dusk: serde_json::Value = serde_json::from_str(&g2.dusk_set_piece_json()).unwrap();
        assert_eq!(dusk["kind"], "dusk");
        assert!(dusk["title"].as_str().unwrap().contains("Dusk"));
        {
            let st = g2.engine.state_mut_for_test();
            st.round = recollect_core::engine::LAST_ROUND;
        }
        let night: serde_json::Value = serde_json::from_str(&g2.dusk_set_piece_json()).unwrap();
        assert_eq!(night["kind"], "nightfall");
        assert!(night["title"].as_str().unwrap().contains("Nightfall"));
    }
}

#[cfg(test)]
mod shell_contract {
    //! The web shell renders the fields below. This pins the JSON
    //! contract so a view refactor that drops a field fails here, not in the
    //! browser.
    use recollect_core::Engine;

    #[test]
    fn view_json_carries_every_field_the_shell_renders() {
        let cat = recollect_core::cards::canon_catalog();
        let deck: Vec<_> = cat
            .iter()
            .filter(|c| matches!(c.kind, recollect_core::types::CardKind::Spirit))
            .take(20)
            .map(|c| c.id)
            .collect();
        let (mut e, _) = Engine::new(1, cat, deck.clone(), deck);
        // Place a spirit so the tile JSON exercises the spirit fields.
        let cmd = e
            .legal_commands(recollect_core::Seat::A)
            .into_iter()
            .find(|c| matches!(c, recollect_core::Command::PlaySpirit { engage: None, .. }))
            .unwrap();
        e.apply(recollect_core::Seat::A, cmd).unwrap();
        let json =
            serde_json::to_string(&recollect_core::view::view_for(&e, recollect_core::Seat::A))
                .unwrap();
        for field in [
            "\"round\"",
            "\"active\"",
            "\"tiles\"",
            "\"spirit\"",
            "\"attack\"",
            "\"defense\"",
            "\"hp\"",
            "\"hp_max\"",
            "\"echo\"",
            "\"face_down\"",
            "\"impression\"",
            "\"faded\"",
            "\"in_your_projection\"",
            "\"terrain\"",
            "\"evolutions\"",
            "\"hand\"",
            "\"anima\"",
            "\"deck_count\"",
        ] {
            assert!(
                json.contains(field),
                "shell contract: PlayerView JSON missing {field}"
            );
        }
    }
}
