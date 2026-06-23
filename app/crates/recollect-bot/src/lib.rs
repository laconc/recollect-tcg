//! AI adversaries and probe policies. Everything here plays through
//! `Engine::legal_commands` + `apply` like any client — bots get no private
//! state, no extra information, and no rule exemptions. The probe fleet
//! (bin/probes.rs) monitors law health nightly: win rates by seat, board
//! texture, camper viability. See docs/testing.md for the tripwires.
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]
pub mod agent;
pub mod evidence;
pub use agent::{Difficulty, Faction, choose, choose_as, choose_params};
use recollect_core::cards::test_catalog;
use recollect_core::effects::{Effect, Trigger, canon_effects};
use recollect_core::engine::forecast_exchange;
use recollect_core::rng::Rng;
use recollect_core::state::{Event, MatchResult, Phase};
use recollect_core::types::CardId;
use recollect_core::types::is_rim;
use recollect_core::{Command, Engine, Seat};

pub fn standard_deck() -> Vec<CardId> {
    [
        0u16, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10,
    ]
    .iter()
    .map(|i| CardId(*i))
    .collect()
}

/// The single match loop, shared by the in-process sims, the diagnostic probes, and the CLI's
/// headless autoplay (which wraps it for NDJSON) — ONE code path, so a sim can never drift from real
/// play. `pick` chooses each command (always `choose`, at the caller's difficulty); `on_step` then
/// observes the post-apply engine + that step's events (a no-op for plain sims, the NDJSON emitter
/// for the CLI, an event/state tally for the probes). Returns the final [`MatchResult`].
pub fn drive_match<F, G>(engine: &mut Engine, mut pick: F, mut on_step: G) -> MatchResult
where
    F: FnMut(&Engine, Seat) -> Command,
    G: FnMut(&Engine, &[Event]),
{
    let mut steps = 0u32;
    loop {
        if let Phase::Finished { result, .. } = engine.state().phase {
            return result;
        }
        let seat = engine.state().active;
        let cmd = pick(engine, seat);
        let events = engine
            .apply(seat, cmd)
            .expect("drive_match: `pick` must return a legal command");
        on_step(engine, &events);
        steps += 1;
        assert!(steps < 100_000, "drive_match: the match did not terminate");
    }
}

/// The difficulty the balance sims pilot BOTH seats at — strong, lookahead play, so fairness
/// reflects skilled decisions, not myopic blunders (which read 0–0). The difficulty-ladder
/// sim varies this deliberately.
pub const SIM_DIFFICULTY: Difficulty = Difficulty::Expert;

/// One full self-play match — both seats `choose` at [`SIM_DIFFICULTY`]. Returns (result, rounds, steps).
pub fn selfplay(seed: u64, bot_seed: u64) -> (MatchResult, u8, u32) {
    let (mut engine, _) = Engine::new(seed, test_catalog(), standard_deck(), standard_deck());
    let mut bot = Rng::from_seed(bot_seed);
    let mut steps = 0u32;
    let result = drive_match(
        &mut engine,
        |e, seat| choose(e, seat, SIM_DIFFICULTY, &mut bot),
        |_, _| steps += 1,
    );
    (result, engine.state().round, steps)
}

/// Play one full greedy 2v2 self-play match on the 6×6 board (four standard
/// decks). The acting team rotates A1→B1→A2→B2; `state().active` is the team to
/// move. Returns (result, steps). The fleet (`bin/fleet.rs`) uses the same shape
/// for its fairness cells; this reusable form backs the 2v2 regression guard.
pub fn selfplay_2v2(seed: u64) -> (MatchResult, u32) {
    let decks = [
        standard_deck(),
        standard_deck(),
        standard_deck(),
        standard_deck(),
    ];
    let (mut e, _) = Engine::new_2v2(seed, test_catalog(), decks);
    let mut bot = Rng::from_seed(seed ^ 0x2002);
    let mut steps = 0u32;
    let result = drive_match(
        &mut e,
        |e, seat| choose(e, seat, SIM_DIFFICULTY, &mut bot),
        |_, _| steps += 1,
    );
    (result, steps)
}

/// The greedy heuristic value of one command for `seat`, piloting as a
/// Lorekeeper (the historical default). Thin alias over [`greedy_score_as`].
pub fn greedy_score(e: &Engine, seat: Seat, c: &Command) -> i64 {
    greedy_score_as(e, seat, c, Faction::Lorekeeper)
}

/// Held-ground value of ONE standing spirit at `tile` for the bot's own seat — the worth of
/// keeping a body planted there. Scoring is one-point-per-tile to whatever stands on it at
/// Nightfall (see `engine/flow.rs` `finish`), so a standing spirit IS the win condition; this
/// term teaches the bot to build and hold a board instead of cashing it out (reclaim-churn).
///
/// Weighted two ways:
///   - **inner ≫ rim**: rim tiles fade after the contraction (round 8), so a body there is
///     transient; an inner body holds to Nightfall. (`is_rim` — types.rs.)
///   - **late-round ramp**: presence matters most as the clock runs out — a spirit standing in
///     the last rounds is about to bank its tile, so it is worth progressively more.
///
/// A fading or face-down spirit holds nothing (it will dissolve / isn't yet a memory). Both
/// factions weight presence the same here — the Solace's faction-correct play (it scores by
/// off-board erasure, not standing) comes from its banish/self-loss economics in `greedy_score_as`,
/// not from discounting its bodies (a standing Unwritten still contests + denies a tile).
fn tile_hold_value(e: &Engine, tile: u8) -> i64 {
    let st = e.state();
    let Some(sp) = st.spirit_at(tile) else {
        return 0;
    };
    if sp.fading || sp.face_down {
        return 0;
    }
    // A late-round ramp in [1.0, ~2.4]: presence at Nightfall is the win condition, so a body is
    // worth more the closer the Memory is to closing. Integer-scaled (×10) to stay deterministic.
    let last = recollect_core::engine::LAST_ROUND as i64;
    let ramp10 = 10 + (12 * st.round.min(recollect_core::engine::LAST_ROUND) as i64) / last; // 10..=22
    // The seat of presence — a planted tile is worth a base + a little of its body (a tougher
    // body is harder to evict, so it holds the tile more reliably).
    let body = 6 + (sp.hp as i64) / 20 + (sp.attack as i64) / 20;
    let inner_mult = if is_rim(tile) { 1 } else { 3 };
    body * inner_mult * ramp10 / 10
}

/// Total held-ground value for `seat`: the sum of [`tile_hold_value`] over every tile holding one
/// of the seat's standing spirits. The positional term `base_score` folds into the forked eval (net
/// of the opponent's) so the bot prefers lines that BUILD and KEEP a board — counterbalancing the
/// depth-2 exposure penalty, which on its own reads board presence as risk and churns it away.
pub(crate) fn held_ground(e: &Engine, seat: Seat) -> i64 {
    let st = e.state();
    (0..st.board.len() as u8)
        .filter(|&t| st.spirit_at(t).map(|s| s.owner == seat).unwrap_or(false))
        .map(|t| tile_hold_value(e, t))
        .sum()
}

/// Points-per-erasure: the bot's positional weight on ONE banked erasure (a player spirit banished
/// or an impression unwritten). The off-board tally is the Solace's true win condition — each is +1,
/// **permanent**, at Nightfall — yet the held-ground term alone is blind to it (it scores only
/// standing bodies). Scaled to ~2× a held inner late-game tile so the depth-2 Solace prizes the
/// erasure it banks well above a single kept body; that is what makes a smarter (deeper) Solace
/// optimize ITS objective (erase + deny) rather than mirror the Lorekeeper's board-building. The
/// weight is part of what pulls the Expert Solace into its target band (see the `char_sweep` notes).
const ERASURE_VALUE: i64 = 120;

/// The Solace's board-**presence** multiplier on the held-ground term, as a numerator over 2 (so the
/// Lorekeeper's implicit ×1 is `2/2`). The Solace prizes a standing Unwritten far above a Lorekeeper
/// spirit: it DENIES the player a tile (no impression can be scored under a contested body) AND lives
/// to bank more erasures, so its plan is to keep a wall up turn after turn. This is the single most
/// important PvE knob — it makes the Solace actually PLANT and HOLD bodies, and because the
/// depth-2 lookahead builds on this greedy score, a deeper (Expert) Solace compounds the wall and
/// finally overcomes the structurally board-favoured Lorekeeper. Tuned (×10/2 = ×5) so the Expert
/// PvE tier lands in its target band in BOTH 1v1 and 2v2; see the `char_sweep` notes. Higher
/// pulls Expert player-win down further but also lifts Hard (the greedy tier over-commits to bodies),
/// so this sits at the balance point between the two tiers' bands.
const SOLACE_PRESENCE_NUM: i64 = 10;

/// The faction-aware **positional objective** for `seat`: the standing value of its win condition in
/// the current position. This is the term the depth-2 lookahead nets against the opponent's, so each
/// side is valued by what IT is trying to do — the fix for "a smarter Solace optimized the
/// Lorekeeper's objective" (it used [`held_ground`] for both seats and so was blind to the erasure
/// tally that the Solace actually wins on).
///
///   - **Lorekeeper** banks Score from standing spirits + impressions, so its objective is
///     [`held_ground`] — build and hold a board to Nightfall.
///   - **Solace** scores OFF-board: its banked **erasure tally** (`solace_erasures` — each player
///     spirit it banishes and each impression it unwrites, permanent) PLUS its standing Unwritten
///     (which still contest + deny tiles, [`held_ground`]). The tally dominates: it is the score the
///     Lorekeeper can never claw back (the Dusk sweeps standing Unwritten, never the banked tally).
pub(crate) fn positional_objective(e: &Engine, seat: Seat, faction: Faction) -> i64 {
    let standing = held_ground(e, seat);
    match faction {
        Faction::Lorekeeper => standing,
        // The erasure tally is a whole-match running total banked by seat B (the Solace). Reading it
        // after a forked move captures the banishes/unwrites that move produced — the signal the
        // depth-2 Solace was missing. The standing term is up-weighted (×3/2): a standing Unwritten
        // both DENIES the player a tile (it can't be scored under) AND lives to bank more erasures, so
        // presence is doubly valuable to the Solace — the measured gap is concentrated in the passive
        // dispositions whose bodies don't contest the board, so the eval pushes ALL of them to hold.
        Faction::Solace => standing * 3 / 2 + e.state().solace_erasures as i64 * ERASURE_VALUE,
    }
}

/// Value of an engage's **defender outcome** for the attacker's `faction`. Banishing the defender is
/// always worth a lot, but the two factions prize it differently:
///   - **Lorekeeper** banishes to trade and lay an impression — a flat, high reward.
///   - **Solace** banishes a **player** spirit to ERASE it: +1 to the permanent tally, its win
///     condition — so it values banishing the player even *higher* than the Lorekeeper does (each
///     player erasure is +1 banked). Banishing a non-player body (a Stray, or
///     in 2v2 an ally) banks no erasure, so it falls back to the trade value.
///
/// `defender_is_enemy` is whether the defender belongs to the attacker's opponent.
fn banish_defender_value(faction: Faction, defender_is_enemy: bool) -> i64 {
    match faction {
        Faction::Lorekeeper => 120,
        Faction::Solace if defender_is_enemy => 150, // erasure: its win condition (clearly above LK)
        Faction::Solace => 60,                       // a non-scoring banish (Stray/ally)
    }
}

/// Value of an engage's **attacker self-loss** for the attacker's `faction` (a positive number, then
/// subtracted). Losing your own body always hurts — but the **Solace must not over-fear it**: trading
/// an Unwritten to banish a player spirit IS its win condition (the body dies, but a +1 erasure is
/// banked permanently). An earlier tune set this to 120 (ABOVE the Lorekeeper's 70) on the reasoning
/// "an Unwritten that falls leaves nothing"; that made the Solace *passive* — it refused the very
/// trades it wins on, so a deeper lookahead would steer AWAY from contesting and play WORSE than
/// greedy Hard. To prevent that the Solace's self-loss is held BELOW the Lorekeeper's (55 < 70):
/// the Solace is the *attrition* faction — it
/// WANTS the exchange, because the body it loses is cheap relative to the permanent erasure it banks
/// (`banish_defender_value(Solace, enemy)` = 150 dwarfs this), and its board plan is to keep replacing
/// walls. Lowering self-fear is a key part of making the deeper Solace press the board instead of
/// hoarding bodies, which is what brings the Expert tier into its target band.
fn self_loss_value(faction: Faction) -> i64 {
    match faction {
        Faction::Lorekeeper => 70,
        Faction::Solace => 55,
    }
}

/// The greedy heuristic value of one command for `seat`, by `faction` —
/// the shared scoring function the difficulty-tiered agent runs through
/// [`greedy_score`] (the Lorekeeper alias) and the agent's `base_score` (which
/// adds the depth-2 lookahead). Higher is better. Uses only public information (board,
/// own hand, forecast math). The **Solace** branch reflects its no-impression
/// economics: a banish (its
/// or the enemy's) earns it no scoring impression, so it values killing the enemy
/// LESS, losing its own MORE, and stainless removal (Release) HIGHER — presence + denial.
pub fn greedy_score_as(e: &Engine, seat: Seat, c: &Command, faction: Faction) -> i64 {
    let st = e.state();
    // `seat` is part of the public scoring contract; the per-command terms read the ACTIVE slot
    // (the mover) and tile geometry, while the positional held-ground term lives in `base_score`.
    let _ = seat;
    let solace = matches!(faction, Faction::Solace);
    match c {
        // System-only forfeit — never offered in `legal_commands`, so a policy never
        // picks it; score it impossibly low for defensiveness.
        Command::MatchAbandoned { .. } => i64::MIN,
        // Mulligan (§5): the opening reshuffle. The greedy heuristic can't judge
        // hand quality, and a fresh hand costs a card, so the fleet holds its
        // opener by default — a small negative keeps any real opening play (a
        // positive-scored Play/Cast) ahead of it, while staying above nothing
        // (it's once-gated, so it can never loop). Judging hand quality is the
        // policy net's job, not the greedy tier's.
        Command::Mulligan { .. } => -7,
        // Release removes a fading spirit with NO impression — a denial tool the Solace
        // prizes (clear the enemy's about-to-score impression, or its own dying).
        Command::Release { .. } => {
            if solace {
                750
            } else {
                500
            }
        }
        // The Solace plays an Unwriting event — value its effect (like a one-shot), less its
        // cost. Per-event tuning lives in `effect_value`; this is the play-vs-pass signal.
        Command::TellUnwriting { hand_index } => {
            let ac = e.card(st.player_slot(st.active_slot).hand[*hand_index as usize]);
            10 + effect_value(&ac.key, faction) - ac.cost as i64
        }
        Command::PlaySpirit {
            hand_index,
            tile,
            engage,
            ..
        } => {
            let ac = e.card(st.player_slot(st.active_slot).hand[*hand_index as usize]);
            let mut s = 12 + if is_rim(*tile) { 0 } else { 8 } - ac.cost as i64;
            s += effect_value(&ac.key, faction);
            // Held ground: planting a body (esp. inner, esp. late) builds presence — the
            // Lorekeeper banks Nightfall score, the Solace contests/denies tiles and fields
            // interceptors. Estimate the tile's hold-worth from the card's stats (the spirit
            // isn't down yet).
            let last = recollect_core::engine::LAST_ROUND as i64;
            let ramp10 = 10 + (12 * st.round.min(recollect_core::engine::LAST_ROUND) as i64) / last;
            let body = 6 + (ac.hp as i64) / 20 + (ac.attack as i64) / 20;
            let inner_mult = if is_rim(*tile) { 1 } else { 3 };
            // The Solace prizes presence far more (see [`SOLACE_PRESENCE_NUM`]): a standing Unwritten
            // denies the player a tile AND lives to bank erasures. This is what pushes the passive
            // dispositions — the measured pushovers — to plant and hold a wall rather than pass, and
            // (because depth-2 builds on this) is the lever that brings the Expert tier into band.
            let hold = body * inner_mult * ramp10 / 10;
            s += if solace {
                hold * SOLACE_PRESENCE_NUM / 2
            } else {
                hold
            };
            if let Some(t) = engage {
                let d = st.spirit_at(*t).unwrap();
                let defender_is_enemy = d.owner == st.active.other();
                let f = forecast_exchange(
                    ac,
                    ac.attack,
                    ac.defense,
                    ac.hp,
                    ac.hp,
                    d,
                    e.card(d.card),
                    0,
                    e.warded_at(*t),
                );
                s += if f.banishes_defender {
                    banish_defender_value(faction, defender_is_enemy)
                } else {
                    f.to_defender as i64
                };
                s -= if f.banishes_attacker {
                    self_loss_value(faction)
                } else {
                    f.to_attacker as i64 / 2
                };
            }
            s
        }
        Command::Overwrite { hand_index, tile } => {
            let ac = e.card(st.player_slot(st.active_slot).hand[*hand_index as usize]);
            // An Overwrite usually targets a board spirit you don't control — but it also reaches a
            // STRAY (§2: the wild stands on its tile, in `st.stray`, not `board.spirit`). When the
            // target is a Stray, score grabbing that inner tile: a revealed Stray the overwriter
            // would fell (or a hidden one, denied entry) cedes the tile; a revealed Stray that
            // survives is a wasted body. (No board spirit to unwrap here.)
            let Some(d) = st.spirit_at(*tile) else {
                let grab = match &st.stray {
                    Some(s) if s.tile == *tile => {
                        let sc = e.card(s.card);
                        // Hidden → denied entry, the overwriter takes the tile uncontested.
                        // Revealed → it lands iff its strike fells the Stray's living HP.
                        let fells = s.veiled || (ac.attack - sc.defense).max(0) >= s.hp;
                        if fells { 10 - ac.cost as i64 * 5 } else { -25 }
                    }
                    _ => 0,
                };
                return effect_value(&ac.key, faction) + grab;
            };
            let f = forecast_exchange(
                ac,
                ac.attack,
                ac.defense,
                ac.hp,
                ac.hp,
                d,
                e.card(d.card),
                0,
                e.warded_at(*tile),
            );
            // Overwrite targets a tile held by a spirit you don't control — for the Solace that is a
            // player body, so banishing it banks erasure (its win condition). A successful overwrite
            // also plants the overwriter on the tile (the overwriter dissolves on a failed trade).
            let defender_is_enemy = d.owner == st.active.other();
            effect_value(&ac.key, faction)
                + if f.banishes_defender {
                    (banish_defender_value(faction, defender_is_enemy) + 10) - ac.cost as i64 * 5
                } else {
                    f.to_defender as i64 - 25
                }
        }
        Command::MoveSpirit { from, to, engage } => {
            let sp = st.spirit_at(*from).unwrap();
            let ac = e.card(sp.card);
            let mut s = if is_rim(*from) && !is_rim(*to) { 14 } else { 2 };
            // Held ground: relocating a body rim → inner converts transient presence into a tile
            // that holds to Nightfall. The body's hold value scales ×3 inner vs ×1 rim, so credit
            // (or debit) the delta from the inner-mult change — a rim→inner late move is prized,
            // inner→rim is penalized.
            let hold = tile_hold_value(e, *from);
            let mult_to = if is_rim(*to) { 1 } else { 3 };
            let mult_from = if is_rim(*from) { 1 } else { 3 };
            s += hold * (mult_to - mult_from) / mult_from;
            if let Some(t) = engage {
                let d = st.spirit_at(*t).unwrap();
                let defender_is_enemy = d.owner == st.active.other();
                let f = forecast_exchange(
                    ac,
                    sp.attack,
                    sp.defense,
                    sp.hp,
                    sp.hp_max,
                    d,
                    e.card(d.card),
                    0,
                    e.warded_at(*t),
                );
                s += if f.banishes_defender {
                    // A move-strike is slightly cheaper than a fresh play (no card spent), so nudge
                    // the banish value down a touch from the play baseline.
                    banish_defender_value(faction, defender_is_enemy) - 10
                } else {
                    f.to_defender as i64
                };
                s -= if f.banishes_attacker {
                    self_loss_value(faction) + 10
                } else {
                    f.to_attacker as i64 / 2
                };
            }
            s
        }
        // Glimpse (§5): no longer free — to activate it you BURN a hand card (the
        // bot spends its worst, `glimpse_burn_choice`), THEN peek + keep-or-bottom
        // (`glimpse_keep_or_bottom`). Score it as the foresight/tempo it buys minus
        // the card it costs: a small flat worth for the peek-and-maybe-Anima, less
        // the draw-value of the cheapest hand card you'd spend. With a junky card to
        // pitch this stays a positive, modest play (below a real development, above
        // passing); when your worst card is itself valuable, the burn makes Glimpse
        // net-negative and the bot rightly skips it. Floored just under EndTurn's −8
        // so a forced ugly glimpse never out-ranks simply holding.
        Command::Glimpse => {
            let hand = &st.player_slot(st.active_slot).hand;
            let worst_burn = hand
                .iter()
                .map(|&id| card_draw_value(e.card(id), faction))
                .min()
                .unwrap_or(0);
            // `saturating_sub`: a form-only hand makes `worst_burn` == i64::MAX
            // (forms are never-burn, B2), and a plain `5 - MAX` would overflow.
            5i64.saturating_sub(worst_burn).max(-7)
        }
        Command::EndTurn => -8,
        Command::Choose { .. } => 1_000_000, // a pending choice outranks all
        Command::SetOrders { .. } => -1_000_000, // free action — never toggle (would loop)
        Command::Reveal { engage, .. } => {
            if engage.is_some() {
                6
            } else {
                2
            }
        } // step into light, ideally striking
        Command::CastRitual { hand_index } => {
            // A ritual is pure effect — value what it does, not a flat constant.
            let ac = e.card(st.player_slot(st.active_slot).hand[*hand_index as usize]);
            4 + effect_value(&ac.key, faction)
        }
        Command::AttachBond { .. } => 3,
        Command::PlaceLandmark { .. } => 3,
        Command::SetFabrication { .. } => 2,
        Command::Evolve {
            form_hand,
            fuel,
            engage,
            ..
        } => {
            // The form is a hand card played onto the base. Score by the FORM's
            // own strength (its stat budget) so the fleet prefers landing a stronger
            // becoming, plus the donor/strike riders.
            let hand = &st.player_slot(st.active_slot).hand;
            let form_budget = hand
                .get(*form_hand as usize)
                .map(|&id| e.card(id))
                .map(|f| (f.attack + f.defense + f.hp) as i64 / 30)
                .unwrap_or(0);
            let mut s = 10 + form_budget; // a stronger form is worth more
            if fuel.is_some() {
                s += 6;
            } // Fabled: ongoing payoff
            if engage.is_some() {
                s += 14;
            } // strike on arrival
            s
        }
        Command::Devolve { tile, .. } => {
            // Devolution (§5): recede a standing-Faded form to a base — the rescue. The
            // form would otherwise dissolve at turn-end (laying the foe's impression), so
            // saving a body and keeping the tile is real value; the rescued base is
            // summoning-sick and can re-evolve next turn (the cycle). Worth the held value
            // of the tile kept, plus a small base for the recovered body, minus the body
            // already being downgraded a tier. The Solace values keeping its presence on
            // the margin too. A positive line, but below a strong fresh play.
            8 + tile_hold_value(e, *tile)
        }
        Command::BanishStray => -9,             // leave the wild be
        Command::StrikeFabrication { .. } => 3, // clearing an enemy lie is worth a strike
        Command::Reclaim { tile } => {
            // Reclaim-churn: a spirit PLACED (or moved) this turn is never worth cashing the
            // same turn — it just spent a card + Anima to stand, and reclaiming hands the tile
            // straight back for ⌊cost/2⌋ Anima. Score that out of contention (i64::MIN) so the bot
            // can't "play a spirit then reclaim it" in one turn — pure churn. The exception: a card
            // with an OnPlay payoff (already banked on arrival) or a
            // Parting payoff (which fires ON the Reclaim) — there cashing the body is a real line.
            if st.moved_this_turn.contains(tile)
                && let Some(sp) = st.spirit_at(*tile)
                && !has_onplay_or_parting_payoff(&e.card(sp.card).key)
            {
                return i64::MIN;
            }
            // Cashing a standing spirit cedes its tile (the win condition) for ⌊cost/2⌋ Anima.
            // The penalty SCALES by the held value of the tile being cashed: an inner, late-game
            // body is far costlier to give up than a transient rim one early — a distinction a
            // flat penalty could not draw. A small flat floor keeps even a worthless cash
            // net-negative, below EndTurn's −8 (no action cap, so Reclaim must never out-rank holding).
            -10 - tile_hold_value(e, *tile)
        }
    }
}

/// The worth of a card held as a *draw* — the shared yardstick the Glimpse (§5)
/// decisions weigh against each other and against +1 Anima. A body's stat budget
/// plus its authored-effect value, minus a fraction of its cost (a card you can
/// barely afford is a weaker draw). Floored at 0 — a draw is never negative.
/// Deterministic.
///
/// B2 fix — finishers are not fodder. Two corrections to the old `÷30` budget:
///   - An **evolution form** is NEVER a burn target. A Primal/Fabled form is a
///     scarce, deck-paired payoff card (its base is useless without it); pitching
///     one to a Glimpse is a strict misplay. It scores `i64::MAX` so it can never
///     be the `min` the burn picks (and is always kept on a keep/bottom peek).
///   - **Big bodies are weighted super-linearly.** The flaw wasn't the low-end
///     slope — it was the *cap*: the old `÷30` valued a 205-budget wall at only +6,
///     below a 1-cost trickster's +14 Draw, so the bot burned its biggest finisher
///     to peek. We keep the gentle `÷30` floor (so a weak body still ranks low and
///     is correctly bottomed/burned) and add a **heavy-body kicker** — `(budget −
///     100) × 3⁄10` above 100 — so a game-ender's worth climbs fast: a ~205 wall
///     scores ~+37, towering over any lone effect, while a ~50-budget runt is
///     unchanged from before. The threshold leaves ordinary 1–3-drops untouched.
fn card_draw_value(c: &recollect_core::types::CardDef, faction: Faction) -> i64 {
    use recollect_core::types::CardKind;
    // A form card is irreplaceable fodder — never burn it (and always keep it).
    if matches!(c.kind, CardKind::Evolution) {
        return i64::MAX;
    }
    let budget = (c.attack + c.defense + c.hp).max(0) as i64;
    // Gentle floor (unchanged ÷30) PLUS a super-linear kicker for the big bodies —
    // a wall/finisher's worth grows far faster than a utility card's, so it never
    // ranks below one, while small bodies keep their old (low) draw value.
    let stat_value = budget / 30 + (budget - 100).max(0) * 3 / 10;
    (4 + stat_value + effect_value(&c.key, faction) - c.cost as i64 / 2).max(0)
}

/// Glimpse (§5) step 1 — the BURN cost, decided honestly. Returns the
/// `Choose { index }` that spends the **least valuable hand card** (lowest
/// `card_draw_value`): the rational cost is your worst card. The bot pilots this
/// seat, so it legitimately sees its own hand via the pending `GlimpseBurn { hand }`
/// — no opponent info. Deterministic (ties break to the lowest index), so self-play
/// stays re-derivable.
pub fn glimpse_burn_choice(e: &Engine, faction: Faction) -> Command {
    let Some(recollect_core::state::PendingChoice::GlimpseBurn { burnable, .. }) =
        &e.state().pending_choice
    else {
        // Not actually a burn (shouldn't happen) — spend the first card.
        return Command::Choose { index: 0 };
    };
    // The cheapest card to give up is the one worth least as a draw. `min_by_key`
    // keeps the FIRST on ties — deterministic.
    let idx = burnable
        .iter()
        .enumerate()
        .min_by_key(|(_, id)| card_draw_value(e.card(**id), faction))
        .map(|(i, _)| i)
        .unwrap_or(0);
    Command::Choose { index: idx as u8 }
}

/// Glimpse (§5) step 2 — keep-or-bottom, decided honestly (the fix for "the old
/// free Glimpse was a flat +6 — now the choice is a real card-vs-Anima tradeoff").
/// Returns the `Choose { index }` to take: **0 = KEEP** the peeked card as next
/// turn's draw, **1 = BOTTOM** it for +1 Anima. The bot pilots this seat, so it
/// legitimately sees its own top card via the pending `Glimpse { top }` — no
/// opponent info.
///
/// The weigh: the card's worth as a *draw* (`card_draw_value`) against the worth
/// of **+1 Anima now**. A cheap, useful body is kept; a weak or expensive-and-
/// unaffordable top is bottomed for the tempo. Deterministic — no rng — so
/// self-play stays re-derivable.
pub fn glimpse_keep_or_bottom(e: &Engine, faction: Faction) -> Command {
    let st = e.state();
    let Some(recollect_core::state::PendingChoice::Glimpse { top, .. }) = &st.pending_choice else {
        // Not actually a glimpse (shouldn't happen) — keep is the safe, lossless pick.
        return Command::Choose { index: 0 };
    };
    let keep = card_draw_value(e.card(*top), faction);
    // +1 Anima's worth: a small, steady tempo gain. Calibrated so a worthwhile
    // body (a cheap spirit clears ~6–8 here) is kept, while a marginal or pricey
    // top falls below it and is banked for the Anima instead.
    let bottom = 6;
    if keep >= bottom {
        Command::Choose { index: 0 } // KEEP the draw
    } else {
        Command::Choose { index: 1 } // BOTTOM for +1 Anima
    }
}

/// Heuristic value of a card's authored effect(s): the agent should prefer cards
/// that DO something, not just bodies with stats. Sums the impactful effect kinds
/// across the card's specs; situational/aura effects get a small nudge. Reads the
/// same public effect IR the engine executes (`canon_effects`), keyed by card key.
fn effect_value(key: &str, faction: Faction) -> i64 {
    let solace = matches!(faction, Faction::Solace);
    let Some(specs) = canon_effects().specs.get(key) else {
        return 0;
    };
    let mut v = 0;
    for spec in specs {
        for cl in &spec.clauses {
            v += match &cl.effect {
                Effect::Damage { amount } => *amount as i64 * 3,
                Effect::AnimaDelta { amount } => *amount as i64 * 6,
                Effect::Draw { count } => *count as i64 * 14,
                Effect::PeekDeck { take, .. } => *take as i64 * 6,
                Effect::RestoreForm { amount } => *amount as i64 * 2,
                Effect::StatDelta {
                    attack, defense, ..
                } => (*attack as i64 + *defense as i64) * 2,
                Effect::Bounce => {
                    if solace {
                        70
                    } else {
                        55
                    }
                }
                Effect::Release => {
                    if solace {
                        80
                    } else {
                        45
                    }
                }
                // Banish (the IllIntent erasure) takes a LIVING enemy outright, no
                // impression — at least as valuable as the mercy Release, and a strong
                // tempo/denial swing for the Solace.
                Effect::Banish => {
                    if solace {
                        85
                    } else {
                        50
                    }
                }
                Effect::Recover => 30,
                Effect::GrantKeyword { .. } => 18,
                Effect::GrantEngage { .. } | Effect::ExtraEngage => 25,
                Effect::Displace(_) => 20,
                Effect::CostDelta { delta } => -(*delta as i64) * 4,
                Effect::RetaliationDelta { amount } => *amount as i64,
                Effect::NoEffect => 0,
                // High-impact effects that were falling to the situational floor:
                Effect::TakeControl => 90, // steal the engaging enemy — game-swinging
                Effect::Summon { .. } => 35, // a free body on the board
                Effect::MomentumMod { per_link_bonus, .. } => 14 + *per_link_bonus as i64 * 2,
                Effect::ThroughlineGrant { attack, defense } => *attack as i64 + *defense as i64,
                Effect::AnimaPerBanishedAlly { max }
                | Effect::AnimaPerAdjacentAlliedPair { max }
                | Effect::DrawPerBanishedThisTurn { max } => *max as i64 * 5,
                Effect::StatShareHigher { .. } => 16,
                Effect::GrantSharedResonance => 14,
                Effect::ReTriggerParting => 20,
                _ => 4, // remaining auras / situational / ill-intent specials: a small nudge
            };
        }
    }
    v
}

/// Does this card key carry an **OnPlay** or **Parting** payoff — an effect that
/// makes playing-then-reclaiming the same spirit a real line rather than pure
/// churn? Used by the [`Command::Reclaim`] scorer: a freshly-placed body is
/// normally never worth cashing the turn it arrived (it ceded a card and Anima to
/// stand, then gives the tile straight back), so the bot scores that Reclaim out
/// of contention — UNLESS the card has a payoff tied to entering or leaving:
///   - **OnPlay** already fired when the spirit was played, so the body did its
///     job on arrival and reclaiming it banks the leftover Anima (e.g. an arrival
///     that drew a card / dealt damage, then is no longer needed on the board).
///   - **Parting** fires *on the Reclaim itself* (a Reclaim is a departure), so
///     cashing the body is how you cash the effect.
///
/// A `NoEffect` clause doesn't count (it's a placeholder, not a payoff).
fn has_onplay_or_parting_payoff(key: &str) -> bool {
    let Some(specs) = canon_effects().specs.get(key) else {
        return false;
    };
    specs.iter().any(|spec| {
        matches!(spec.trigger, Trigger::OnPlay | Trigger::Parting)
            && spec
                .clauses
                .iter()
                .any(|cl| !matches!(cl.effect, Effect::NoEffect))
    })
}

#[cfg(test)]
mod effect_value_tests {
    use super::*;

    #[test]
    fn effect_value_engages_with_the_effect_ir() {
        // Reading the real IR: many catalog cards carry a valued effect, and at least
        // one scores well above the situational floor (a Draw/Damage/multi-clause card).
        let valued = canon_effects()
            .specs
            .keys()
            .filter(|k| effect_value(k.as_str(), Faction::Lorekeeper) > 0)
            .count();
        assert!(valued >= 30, "only {valued} cards have a valued effect");
        let max = canon_effects()
            .specs
            .keys()
            .map(|k| effect_value(k.as_str(), Faction::Lorekeeper))
            .max()
            .unwrap_or(0);
        assert!(
            max > 30,
            "no card scores above the situational floor (max {max})"
        );
    }

    /// Glimpse (§5) step 2: the bot weighs keep-vs-bottom honestly. A strong body is a
    /// draw worth keeping; a genuinely low-value card (minimal stat budget, no valued
    /// effect) is better banked for the +1 Anima. Driven by seating each kind on the deck
    /// top, paying the burn, and resolving through the same `glimpse_keep_or_bottom` the
    /// agent calls. (The honest tradeoff keeps impactful cards regardless of cost — a
    /// powerful card is a good draw — and only bottoms weak ones; this pins that.)
    #[test]
    fn glimpse_keeps_a_strong_body_and_bottoms_a_low_value_card() {
        use recollect_core::types::CardKind;
        let cat = test_catalog();
        // A strong Spirit — a good draw worth keeping (highest stat budget).
        let keepable = cat
            .iter()
            .filter(|c| matches!(c.kind, CardKind::Spirit))
            .max_by_key(|c| (c.attack + c.defense + c.hp) as i64)
            .expect("a spirit exists")
            .id;
        // A genuinely low-value card: the smallest stat budget AND no valued effect —
        // a weak draw, better banked for the +1 Anima than held.
        let bottomable = cat
            .iter()
            .filter(|c| effect_value(&c.key, Faction::Lorekeeper) <= 0)
            .min_by_key(|c| (c.attack + c.defense + c.hp) as i64)
            .expect("a low-budget card exists")
            .id;

        let decide_top = |top: recollect_core::types::CardId| {
            let (mut e, _) = Engine::new(1, cat.clone(), standard_deck(), standard_deck());
            {
                let st = e.state_mut_for_test();
                st.active = Seat::A; // ensure A is the one to glimpse, whoever opened
                st.player_a.deck.insert(0, top); // seat the chosen card on top of A's page
            }
            e.apply(Seat::A, Command::Glimpse).unwrap();
            // Pay the burn (the bot's own choice) so the keep-or-bottom choice opens.
            let burn = glimpse_burn_choice(&e, Faction::Lorekeeper);
            e.apply(Seat::A, burn).unwrap();
            glimpse_keep_or_bottom(&e, Faction::Lorekeeper)
        };
        assert_eq!(
            decide_top(keepable),
            Command::Choose { index: 0 },
            "a strong body is kept as a draw"
        );
        assert_eq!(
            decide_top(bottomable),
            Command::Choose { index: 1 },
            "a low-value card is bottomed for the +1 Anima"
        );
    }

    /// Glimpse (§5) step 1: the bot burns its LEAST valuable hand card. Seat a hand of
    /// a strong body and a weak one; the burn choice must spend the weak one (the
    /// rational cost is your worst card). Pins `glimpse_burn_choice` against burning a
    /// keeper.
    #[test]
    fn glimpse_burns_the_least_valuable_hand_card() {
        use recollect_core::types::CardKind;
        let cat = test_catalog();
        let strong = cat
            .iter()
            .filter(|c| matches!(c.kind, CardKind::Spirit))
            .max_by_key(|c| (c.attack + c.defense + c.hp) as i64)
            .expect("a spirit exists")
            .id;
        let weak = cat
            .iter()
            .filter(|c| effect_value(&c.key, Faction::Lorekeeper) <= 0)
            .min_by_key(|c| (c.attack + c.defense + c.hp) as i64)
            .expect("a low-budget card exists")
            .id;

        // Hand = [strong, weak]; the burn should pick index 1 (the weak card). Put the
        // weak card SECOND so a naive "burn index 0" would wrongly spend the keeper.
        let (mut e, _) = Engine::new(2, cat.clone(), standard_deck(), standard_deck());
        {
            let st = e.state_mut_for_test();
            st.active = Seat::A;
            st.player_a.hand = vec![strong, weak];
            st.player_a.deck.insert(0, strong); // a non-empty page to peek
        }
        e.apply(Seat::A, Command::Glimpse).unwrap();
        assert_eq!(
            glimpse_burn_choice(&e, Faction::Lorekeeper),
            Command::Choose { index: 1 },
            "the bot burns the weaker card, not the keeper"
        );
    }

    /// B1 (reclaim-churn): a spirit PLAYED this turn whose card has NO
    /// OnPlay/Parting payoff is never worth cashing the same turn — the Reclaim of
    /// it scores `i64::MIN`, out of contention, so the bot can't "play a body then
    /// reclaim it" in one breath. Pins the just-placed Reclaim demotion. (Uses the
    /// canon catalog — `test_catalog` carries no Evolution forms / authored-effect
    /// keys — and a home-row tile so the opening placement is legal.)
    #[test]
    fn reclaim_of_a_freshly_placed_vanilla_body_is_out_of_contention() {
        use recollect_core::cards::canon_catalog;
        use recollect_core::types::CardKind;
        let cat = canon_catalog();
        // A plain body with no authored effect at all (no OnPlay, no Parting).
        let vanilla = cat
            .iter()
            .find(|c| {
                matches!(c.kind, CardKind::Spirit)
                    && canon_effects()
                        .specs
                        .get(&c.key)
                        .is_none_or(|s| s.is_empty())
            })
            .expect("a vanilla spirit exists")
            .id;
        let (mut e, _) = Engine::new(5, cat.clone(), standard_deck(), standard_deck());
        let seat = {
            let st = e.state_mut_for_test();
            st.active = Seat::A;
            // Hand the vanilla body and enough Anima to play it; seat it in hand.
            st.player_a.hand = vec![vanilla];
            st.player_a.anima = 9;
            Seat::A
        };
        // Play onto a home-row tile (the legal opening placement for seat A), then
        // assert the Reclaim of that tile is out of contention.
        let tile = 2u8; // row 0 — within seat A's home rows on the 5×5
        e.apply(
            seat,
            Command::PlaySpirit {
                hand_index: 0,
                tile,
                engage: None,
                chain_prefs: vec![],
            },
        )
        .expect("the vanilla body plays");
        assert!(
            e.state().moved_this_turn.contains(&tile),
            "a fresh arrival is recorded in moved_this_turn"
        );
        let rc = Command::Reclaim { tile };
        assert_eq!(
            greedy_score_as(&e, seat, &rc, Faction::Lorekeeper),
            i64::MIN,
            "reclaiming a just-placed payoff-less body is scored out of contention"
        );
        // And EndTurn (-8) trivially outranks it, so the bot holds instead of churning.
        assert!(
            greedy_score_as(&e, seat, &Command::EndTurn, Faction::Lorekeeper)
                > greedy_score_as(&e, seat, &rc, Faction::Lorekeeper),
            "holding beats the churn-reclaim"
        );
    }

    /// B1 exception: a freshly-placed body WHOSE card has an OnPlay/Parting payoff
    /// is still a real Reclaim line (the effect already fired on arrival, or its
    /// Parting fires on the Reclaim itself) — so it is NOT demoted to i64::MIN.
    #[test]
    fn reclaim_of_a_freshly_placed_payoff_body_is_allowed() {
        use recollect_core::cards::canon_catalog;
        use recollect_core::effects::Trigger;
        use recollect_core::types::CardKind;
        let cat = canon_catalog();
        // A plain (non-evolution) spirit with an OnPlay or Parting payoff clause,
        // cheap enough to play in the opening.
        let payoff = cat
            .iter()
            .find(|c| {
                matches!(c.kind, CardKind::Spirit)
                    && c.cost <= 4
                    && has_onplay_or_parting_payoff(&c.key)
            })
            .expect("a payoff spirit exists");
        assert!(
            canon_effects()
                .specs
                .get(&payoff.key)
                .unwrap()
                .iter()
                .any(|s| matches!(s.trigger, Trigger::OnPlay | Trigger::Parting)),
            "fixture really has an OnPlay/Parting trigger"
        );
        let (mut e, _) = Engine::new(6, cat.clone(), standard_deck(), standard_deck());
        let seat = {
            let st = e.state_mut_for_test();
            st.active = Seat::A;
            st.player_a.hand = vec![payoff.id];
            st.player_a.anima = 9;
            Seat::A
        };
        let tile = 2u8; // home row
        e.apply(
            seat,
            Command::PlaySpirit {
                hand_index: 0,
                tile,
                engage: None,
                chain_prefs: vec![],
            },
        )
        .expect("the payoff body plays");
        assert_ne!(
            greedy_score_as(&e, seat, &Command::Reclaim { tile }, Faction::Lorekeeper),
            i64::MIN,
            "a payoff body's same-turn Reclaim is a real line, not demoted"
        );
    }

    /// B2: the bot NEVER burns an evolution FORM (a scarce, deck-paired finisher)
    /// to a Glimpse — `card_draw_value` returns `i64::MAX` for a form, so the burn's
    /// `min` can never land on it, and it always outvalues an ordinary card.
    #[test]
    fn glimpse_never_burns_an_evolution_form() {
        use recollect_core::cards::canon_catalog;
        use recollect_core::types::CardKind;
        let cat = canon_catalog();
        let form = cat
            .iter()
            .find(|c| matches!(c.kind, CardKind::Evolution))
            .expect("an evolution form exists");
        // The form is a never-burn card, and outvalues even the strongest ordinary body.
        assert_eq!(
            card_draw_value(form, Faction::Lorekeeper),
            i64::MAX,
            "a form is never-burn fodder"
        );
        let strongest_ordinary = cat
            .iter()
            .filter(|c| !matches!(c.kind, CardKind::Evolution))
            .map(|c| card_draw_value(c, Faction::Lorekeeper))
            .max()
            .unwrap();
        assert!(
            card_draw_value(form, Faction::Lorekeeper) > strongest_ordinary,
            "a form outvalues every ordinary card as a draw"
        );
        // End-to-end: hand [form, weak]; the burn picks the weak card, never the form.
        let weak = cat
            .iter()
            .filter(|c| {
                !matches!(c.kind, CardKind::Evolution)
                    && effect_value(&c.key, Faction::Lorekeeper) <= 0
            })
            .min_by_key(|c| (c.attack + c.defense + c.hp) as i64)
            .expect("a low-budget non-form card exists")
            .id;
        let (mut e, _) = Engine::new(7, cat.clone(), standard_deck(), standard_deck());
        {
            let st = e.state_mut_for_test();
            st.active = Seat::A;
            st.player_a.hand = vec![form.id, weak];
            st.player_a.deck.insert(0, weak); // a non-empty page to peek
        }
        e.apply(Seat::A, Command::Glimpse).unwrap();
        assert_eq!(
            glimpse_burn_choice(&e, Faction::Lorekeeper),
            Command::Choose { index: 1 },
            "the bot burns the weak card, never the evolution form"
        );
    }

    /// B2: a big-budget body (a wall / finisher) ranks as a BETTER draw than a
    /// cheap utility card carrying a lone Draw effect — the old `÷30` capped the
    /// wall below the trickster (a +14 Draw), so the bot burned its finisher. Pins
    /// the re-weighting: the biggest body out-values a cheap Draw card.
    #[test]
    fn a_big_body_outvalues_a_cheap_utility_draw_card() {
        use recollect_core::cards::canon_catalog;
        use recollect_core::types::CardKind;
        let cat = canon_catalog();
        // The biggest non-form body.
        let big = cat
            .iter()
            .filter(|c| matches!(c.kind, CardKind::Spirit))
            .max_by_key(|c| (c.attack + c.defense + c.hp) as i64)
            .expect("a spirit exists");
        // A cheap card whose effect carries a Draw (the kind that out-ranked walls).
        let drawish = cat.iter().find(|c| {
            c.cost <= 2
                && (c.attack + c.defense + c.hp) < 90
                && canon_effects().specs.get(&c.key).is_some_and(|specs| {
                    specs.iter().any(|s| {
                        s.clauses
                            .iter()
                            .any(|cl| matches!(cl.effect, Effect::Draw { .. }))
                    })
                })
        });
        if let Some(d) = drawish {
            assert!(
                card_draw_value(big, Faction::Lorekeeper) > card_draw_value(d, Faction::Lorekeeper),
                "the big body ({}) must out-value the cheap draw card ({}): {} vs {}",
                big.name,
                d.name,
                card_draw_value(big, Faction::Lorekeeper),
                card_draw_value(d, Faction::Lorekeeper),
            );
        }
    }
}
