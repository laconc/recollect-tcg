//! Action-choice distribution probe — *what does the bot actually DO?*
//!
//! `calibrate`/`char_sweep`/`tier_sweep` answer **who wins**; none of them answer **how the bot
//! spends its turns**. This binary tallies, across many seeded sim matches, the **percentage of the
//! time the bot chooses each activity** — Play / Call / Evolve / Devolve / Glimpse / Move / EndTurn /
//! Mulligan (plus the rest of the command vocabulary, so the percentages sum honestly) — broken out
//! **per difficulty tier, per faction, and per game phase** (opening / mid / post-Dusk). The
//! maintainer reads it to judge whether the proportions are *human-sensible*: does the bot Evolve,
//! Devolve, Glimpse, Call, and Move at rates a thoughtful player would, or is something off (never
//! devolving, over-glimpsing, ignoring Calls, end-turn churn)?
//!
//! It is a **DATA probe only** — no engine / card / balance change. Every match is driven through the
//! exact public seam a real client uses (`Engine::new_with_rules` / `new_2v2_with_opener` +
//! `legal_commands`/`apply`), each seat piloted in its own faction by `choose_as` at the chosen tier,
//! so the mix it measures is the mix a real game would see. Each command is classified **before** it
//! is applied (so a `PlaySpirit` is read against the live hand — a **Caller** card is a *Call*, any
//! other body is a *Play*), then bucketed by `(tier, faction, phase)`.
//!
//! ## What an "activity" is
//! The engine's [`Command`] has 21 variants; a player thinks in fewer **activities**. The headline
//! eight the maintainer asked for are Play / Call / Evolve / Devolve / Glimpse / Move / EndTurn /
//! Mulligan; the remaining commands the bot can issue (Overwrite, Reveal, Release, Reclaim, a Ritual,
//! an Unwriting, a Bond / Landmark / Fabrication, a Stray banish, a Fabrication strike) are kept as
//! their own buckets so nothing is hidden and the column sums to ~100%. The two `Choose` follow-ups
//! that resolve a Glimpse (burn, then keep-or-bottom) and any target-pick are counted under `Choose`
//! — they are sub-steps of an activity already counted (the initiating `Glimpse` / play), reported
//! apart so they don't inflate the headline mix.
//!
//! ## Phases
//! Bucketed by round against the Dusk (the round-8 contraction, the one real gameplay boundary):
//! **opening** = rounds 1–3 (build), **mid** = rounds 4..=`contraction_after` (the contest, through
//! the Dusk), **post-Dusk** = rounds > `contraction_after` (the inner-board endgame, rounds 9–12).
//!
//! ## Reproducibility
//! Fixed seeds (`0..N`) with seeded RNGs, exactly like the rest of the fleet — the tables
//! **re-derive bit-identically on a re-run**. The counts are exact for the given `N`, not
//! Monte-Carlo estimates that wobble.
//!
//!   cargo run -p recollect-bot --bin action_mix --release
//!   cargo run -p recollect-bot --bin action_mix --release -- 1v1   # skip the (slower) 2v2 pass
use recollect_bot::{Difficulty, Faction, choose_as};
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::{generate_deck, solace_character_deck};
use recollect_core::rng::Rng;
use recollect_core::state::{Command, MatchRules, Phase};
use recollect_core::types::{CardDef, CardId, CardKind, SeatSlot};
use recollect_core::{Engine, Seat};

/// Matches per (tier × faction) cell. 200 seeds is plenty to stabilise a *distribution* (we tally
/// every decision in every match — tens of thousands of decisions per cell), and keeps the whole
/// 1v1 sweep (4 tiers × 2 factions × 200) well under a minute in release.
const N: u64 = 200;

/// The activities the bot can choose, in report order: the eight headline ones the maintainer named
/// first, then the rest of the command vocabulary (so the mix sums to ~100% with nothing hidden),
/// then `Choose` (the Glimpse/target sub-steps, reported apart). Every [`Command`] maps to exactly
/// one of these via [`classify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Activity {
    // --- the eight headline activities ---
    Play,
    Call,
    Evolve,
    Devolve,
    Glimpse,
    Move,
    EndTurn,
    Mulligan,
    // --- the rest of the vocabulary (kept visible so columns sum honestly) ---
    Overwrite,
    Reveal,
    Release,
    Reclaim,
    Ritual,
    Unwriting,
    Bond,
    Landmark,
    Fabrication,
    StrikeFab,
    BanishStray,
    SetOrders,
    /// Glimpse burn / keep-or-bottom + any target pick — sub-steps of an activity already counted.
    Choose,
}

impl Activity {
    /// All activities, in the order they print.
    const ALL: [Activity; 21] = [
        Activity::Play,
        Activity::Call,
        Activity::Evolve,
        Activity::Devolve,
        Activity::Glimpse,
        Activity::Move,
        Activity::EndTurn,
        Activity::Mulligan,
        Activity::Overwrite,
        Activity::Reveal,
        Activity::Release,
        Activity::Reclaim,
        Activity::Ritual,
        Activity::Unwriting,
        Activity::Bond,
        Activity::Landmark,
        Activity::Fabrication,
        Activity::StrikeFab,
        Activity::BanishStray,
        Activity::SetOrders,
        Activity::Choose,
    ];

    /// The eight headline activities, the ones the human-sensibility read is about.
    const HEADLINE: [Activity; 8] = [
        Activity::Play,
        Activity::Call,
        Activity::Evolve,
        Activity::Devolve,
        Activity::Glimpse,
        Activity::Move,
        Activity::EndTurn,
        Activity::Mulligan,
    ];

    fn label(self) -> &'static str {
        match self {
            Activity::Play => "Play",
            Activity::Call => "Call",
            Activity::Evolve => "Evolve",
            Activity::Devolve => "Devolve",
            Activity::Glimpse => "Glimpse",
            Activity::Move => "Move",
            Activity::EndTurn => "EndTurn",
            Activity::Mulligan => "Mulligan",
            Activity::Overwrite => "Overwrite",
            Activity::Reveal => "Reveal",
            Activity::Release => "Release",
            Activity::Reclaim => "Reclaim",
            Activity::Ritual => "Ritual",
            Activity::Unwriting => "Unwriting",
            Activity::Bond => "Bond",
            Activity::Landmark => "Landmark",
            Activity::Fabrication => "Fabric.",
            Activity::StrikeFab => "StrikeFab",
            Activity::BanishStray => "BanishStr",
            Activity::SetOrders => "SetOrders",
            Activity::Choose => "Choose",
        }
    }
}

/// Classify the command the bot is *about* to play into an [`Activity`]. Read against the engine
/// state **before** apply — a `PlaySpirit` whose card is a **Caller** is a *Call* (summoning a
/// Kindred), any other body is a *Play*. The engine has no `Call` command; a Call IS a `PlaySpirit`
/// of a Caller-kind card, so the kind of the played card is the only way to tell them apart, exactly
/// as a player would ("am I dropping a body or calling a companion?").
fn classify(e: &Engine, c: &Command) -> Activity {
    let st = e.state();
    match c {
        Command::PlaySpirit { hand_index, .. } => {
            // Read the played card's kind off the ACTIVE slot's hand (the mover). A Caller summons a
            // Kindred — that is a Call; anything else is a Play.
            let hand = &st.player_slot(st.active_slot).hand;
            let kind = hand
                .get(*hand_index as usize)
                .map(|&id| e.card(id).kind)
                .unwrap_or(CardKind::Spirit);
            if matches!(kind, CardKind::Caller) {
                Activity::Call
            } else {
                Activity::Play
            }
        }
        Command::MoveSpirit { .. } => Activity::Move,
        Command::Evolve { .. } => Activity::Evolve,
        Command::Devolve { .. } => Activity::Devolve,
        Command::Glimpse => Activity::Glimpse,
        Command::EndTurn => Activity::EndTurn,
        Command::Mulligan { .. } => Activity::Mulligan,
        Command::Overwrite { .. } => Activity::Overwrite,
        Command::Reveal { .. } => Activity::Reveal,
        Command::Release { .. } => Activity::Release,
        Command::Reclaim { .. } => Activity::Reclaim,
        Command::CastRitual { .. } => Activity::Ritual,
        Command::TellUnwriting { .. } => Activity::Unwriting,
        Command::AttachBond { .. } => Activity::Bond,
        Command::PlaceLandmark { .. } => Activity::Landmark,
        Command::SetFabrication { .. } => Activity::Fabrication,
        Command::StrikeFabrication { .. } => Activity::StrikeFab,
        Command::BanishStray => Activity::BanishStray,
        Command::SetOrders { .. } => Activity::SetOrders,
        Command::Choose { .. } => Activity::Choose,
        // System-only forfeit — never offered to a policy; fold into Choose's catch-all so the match
        // arm is exhaustive (it is unreachable in these sims).
        Command::MatchAbandoned { .. } => Activity::Choose,
    }
}

/// The three game phases, bucketed by round against the Dusk (the round-8 contraction).
#[derive(Clone, Copy, PartialEq, Eq)]
enum GamePhase {
    Opening,
    Mid,
    PostDusk,
}

impl GamePhase {
    const ALL: [GamePhase; 3] = [GamePhase::Opening, GamePhase::Mid, GamePhase::PostDusk];
    fn label(self) -> &'static str {
        match self {
            GamePhase::Opening => "opening (r1-3)",
            GamePhase::Mid => "mid (r4-Dusk)",
            GamePhase::PostDusk => "post-Dusk",
        }
    }
    /// Which phase a round sits in. `opening` = rounds 1–3; `mid` = 4..=`contraction_after` (the
    /// contest through the Dusk); `post-Dusk` = beyond the contraction (the inner endgame).
    fn of(round: u8, contraction_after: u8) -> GamePhase {
        if round <= 3 {
            GamePhase::Opening
        } else if round <= contraction_after {
            GamePhase::Mid
        } else {
            GamePhase::PostDusk
        }
    }
}

/// A running tally of activity counts — one per cell, plus a per-phase split.
#[derive(Default, Clone)]
struct Tally {
    /// Count per activity (indexed by `Activity::ALL` position).
    total: [u64; 21],
    /// Count per activity per phase, indexed `by_phase[phase][activity]`.
    by_phase: [[u64; 21]; 3],
    /// Total decisions tallied (the denominator).
    decisions: u64,
    /// Total decisions per phase.
    phase_decisions: [u64; 3],
    /// Matches contributing to this tally (for the per-match-rate notes).
    matches: u64,
}

impl Tally {
    fn record(&mut self, a: Activity, phase: GamePhase) {
        let ai = Activity::ALL.iter().position(|x| *x == a).unwrap();
        let pi = phase as usize;
        self.total[ai] += 1;
        self.by_phase[pi][ai] += 1;
        self.decisions += 1;
        self.phase_decisions[pi] += 1;
    }
    /// Percentage of all decisions that were activity `a`.
    fn pct(&self, a: Activity) -> f64 {
        if self.decisions == 0 {
            return 0.0;
        }
        let ai = Activity::ALL.iter().position(|x| *x == a).unwrap();
        100.0 * self.total[ai] as f64 / self.decisions as f64
    }
    /// Percentage within a phase.
    fn pct_phase(&self, a: Activity, phase: GamePhase) -> f64 {
        let pi = phase as usize;
        if self.phase_decisions[pi] == 0 {
            return 0.0;
        }
        let ai = Activity::ALL.iter().position(|x| *x == a).unwrap();
        100.0 * self.by_phase[pi][ai] as f64 / self.phase_decisions[pi] as f64
    }
    /// Mean count of activity `a` per match (e.g. "~7 Glimpses per match").
    fn per_match(&self, a: Activity) -> f64 {
        if self.matches == 0 {
            return 0.0;
        }
        let ai = Activity::ALL.iter().position(|x| *x == a).unwrap();
        self.total[ai] as f64 / self.matches as f64
    }
}

/// Decorrelated 1v1 decks for a seed: seat A a rotating Quick-Play style, seat B either a rotating
/// style (mirror) or a rotating Solace disposition (Solace). The Solace deck path is the SAME one
/// `char_sweep` fields, so this probe walks a representative slice of the catalog.
fn decks_1v1(seed: u64, opp: Faction, cat: &[CardDef]) -> (Vec<CardId>, Vec<CardId>) {
    let a = generate_deck((seed % 6) as u8, seed, cat);
    let b = match opp {
        Faction::Lorekeeper => generate_deck(((seed + 2) % 6) as u8, seed + 1, cat),
        Faction::Solace => solace_character_deck((seed % 20) as usize, seed ^ 0x5EED, cat),
    };
    (a, b)
}

/// Drive one 1v1 match, tallying each seat's decisions into its faction's tally. `a_faction` /
/// `b_faction` are the seats' factions; both seats play at `tier`. The loop is the char_sweep
/// `play()` shape — `state().active` to move, `choose_as` to pick, `apply` to advance — with the
/// classification interposed before each apply. Deterministic given the seed.
fn run_1v1(
    seed: u64,
    tier: Difficulty,
    a_faction: Faction,
    b_faction: Faction,
    cat: &[CardDef],
    tally_a: &mut Tally,
    tally_b: &mut Tally,
) {
    let (da, db) = decks_1v1(seed, b_faction, cat);
    let mut rules = MatchRules::default();
    rules.factions = [a_faction, b_faction];
    let (mut e, _) = Engine::new_with_rules(seed, cat.to_vec(), da, db, rules, Seat::A);
    let mut rng = Rng::from_seed(seed ^ 0xAC10);
    let mut steps = 0u32;
    loop {
        if matches!(e.state().phase, Phase::Finished { .. }) {
            return;
        }
        if steps > 5000 {
            return; // a stuck match never happens in practice; bound it like the sims do
        }
        let seat = e.state().active;
        let (faction, opp_faction, tally) = if seat == Seat::A {
            (a_faction, b_faction, &mut *tally_a)
        } else {
            (b_faction, a_faction, &mut *tally_b)
        };
        let cmd = choose_as(&e, seat, tier, faction, opp_faction, &mut rng);
        let phase = GamePhase::of(e.state().round, e.state().rules.contraction_after);
        let act = classify(&e, &cmd);
        tally.record(act, phase);
        e.apply(seat, cmd).expect("legal");
        steps += 1;
    }
}

/// Drive one 2v2 match (team A two Lorekeepers, team B two Solace), tallying team A's decisions into
/// `tally_lk` and team B's into `tally_solace`. Same public 2v2 seam `char_sweep_2v2` uses.
fn run_2v2(
    seed: u64,
    tier: Difficulty,
    cat: &[CardDef],
    tally_lk: &mut Tally,
    tally_solace: &mut Tally,
) {
    let a1 = generate_deck((seed % 6) as u8, seed, cat);
    let a2 = generate_deck(((seed + 3) % 6) as u8, seed ^ 0x1, cat);
    let b1 = solace_character_deck((seed % 20) as usize, seed ^ 0x5EED, cat);
    let b2 = solace_character_deck(((seed + 7) % 20) as usize, seed ^ 0x5EED ^ 0x2, cat);
    let (mut e, _) = Engine::new_2v2_with_opener(
        seed,
        cat.to_vec(),
        [a1, b1, a2, b2],
        SeatSlot::A1,
        [Faction::Lorekeeper, Faction::Solace],
    );
    let mut rng = Rng::from_seed(seed ^ 0x2C10);
    let mut steps = 0u32;
    loop {
        if matches!(e.state().phase, Phase::Finished { .. }) {
            return;
        }
        if steps > 20_000 {
            return;
        }
        let seat = e.state().active;
        let (faction, opp_faction, tally) = if seat == Seat::A {
            (Faction::Lorekeeper, Faction::Solace, &mut *tally_lk)
        } else {
            (Faction::Solace, Faction::Lorekeeper, &mut *tally_solace)
        };
        let cmd = choose_as(&e, seat, tier, faction, opp_faction, &mut rng);
        let phase = GamePhase::of(e.state().round, e.state().rules.contraction_after);
        let act = classify(&e, &cmd);
        tally.record(act, phase);
        e.apply(seat, cmd).expect("legal");
        steps += 1;
    }
}

/// Print one cell's headline-activity table: the eight named activities' share of all decisions,
/// their per-match count, and (for the activities that vary by phase) the opening / mid / post-Dusk
/// split.
fn print_cell(title: &str, t: &Tally) {
    println!("\n=== {title} ===");
    println!(
        "  {} decisions over {} matches ({:.0} decisions/match)\n",
        t.decisions,
        t.matches,
        t.decisions as f64 / t.matches.max(1) as f64
    );
    println!(
        "  {:>9}  {:>7}  {:>9}  |  {:>9}  {:>9}  {:>9}",
        "activity", "% mix", "per-match", "opening%", "mid%", "postDusk%"
    );
    for a in Activity::HEADLINE {
        println!(
            "  {:>9}  {:>6.2}%  {:>9.2}  |  {:>8.2}%  {:>8.2}%  {:>8.2}%",
            a.label(),
            t.pct(a),
            t.per_match(a),
            t.pct_phase(a, GamePhase::Opening),
            t.pct_phase(a, GamePhase::Mid),
            t.pct_phase(a, GamePhase::PostDusk),
        );
    }
    // The rest of the vocabulary, condensed to one line each (mix% + per-match), so the column sums
    // honestly and an oddity (e.g. Reclaim-churn) is visible.
    println!("\n  other commands (mix% · per-match):");
    let rest: Vec<Activity> = Activity::ALL
        .into_iter()
        .filter(|a| !Activity::HEADLINE.contains(a))
        .collect();
    let mut line = String::new();
    for a in &rest {
        let p = t.pct(*a);
        if p < 0.005 && t.per_match(*a) < 0.005 {
            continue; // never issued — omit to keep the line readable
        }
        line.push_str(&format!("{} {:.2}%/{:.2}  ", a.label(), p, t.per_match(*a)));
    }
    if line.is_empty() {
        line.push_str("(none issued)");
    }
    println!("    {line}");
    // How the decisions distribute across the phases (context for the per-phase columns above).
    let phase_share: Vec<String> = GamePhase::ALL
        .into_iter()
        .map(|p| {
            let pi = p as usize;
            let share = 100.0 * t.phase_decisions[pi] as f64 / t.decisions.max(1) as f64;
            format!("{} {:.0}%", p.label(), share)
        })
        .collect();
    println!("    decisions by phase: {}", phase_share.join(" · "));
    // Sanity: the full mix sums to ~100%.
    let sum: f64 = Activity::ALL.into_iter().map(|a| t.pct(a)).sum();
    println!("    [mix sum {sum:.1}%]");
}

/// Run the 1v1 sweep for one tier across both factions and print the two cells.
fn sweep_1v1_tier(tier: Difficulty, cat: &[CardDef]) {
    // Mirror: both seats Lorekeeper — tally BOTH seats into one Lorekeeper mirror cell.
    let mut lk_mirror = Tally::default();
    for seed in 0..N {
        let mut a = Tally::default();
        let mut b = Tally::default();
        run_1v1(
            seed,
            tier,
            Faction::Lorekeeper,
            Faction::Lorekeeper,
            cat,
            &mut a,
            &mut b,
        );
        merge(&mut lk_mirror, &a);
        merge(&mut lk_mirror, &b);
    }
    lk_mirror.matches = 2 * N; // both seats contributed a "player-match" of decisions

    // PvE: seat A Lorekeeper, seat B Solace — tally each seat into its faction's cell.
    let mut pve_lk = Tally::default();
    let mut pve_solace = Tally::default();
    for seed in 0..N {
        run_1v1(
            seed,
            tier,
            Faction::Lorekeeper,
            Faction::Solace,
            cat,
            &mut pve_lk,
            &mut pve_solace,
        );
    }
    pve_lk.matches = N;
    pve_solace.matches = N;

    println!("\n##########  1v1 · {} tier  ##########", tier.name());
    print_cell(
        &format!("{} · Lorekeeper (mirror, both seats)", tier.name()),
        &lk_mirror,
    );
    print_cell(
        &format!("{} · Lorekeeper (vs Solace, seat A)", tier.name()),
        &pve_lk,
    );
    print_cell(
        &format!("{} · Solace (vs Lorekeeper, seat B)", tier.name()),
        &pve_solace,
    );
}

/// Run the 2v2 sweep for one tier and print the two team cells.
fn sweep_2v2_tier(tier: Difficulty, cat: &[CardDef]) {
    let mut lk = Tally::default();
    let mut solace = Tally::default();
    for seed in 0..N {
        run_2v2(seed, tier, cat, &mut lk, &mut solace);
    }
    lk.matches = N;
    solace.matches = N;
    println!("\n##########  2v2 · {} tier  ##########", tier.name());
    print_cell(&format!("{} · Lorekeeper team A (2v2)", tier.name()), &lk);
    print_cell(&format!("{} · Solace team B (2v2)", tier.name()), &solace);
}

/// Add `src`'s counts into `dst` (per-activity, per-phase, and decision totals; NOT `matches`, which
/// the caller sets to the right denominator).
fn merge(dst: &mut Tally, src: &Tally) {
    for i in 0..21 {
        dst.total[i] += src.total[i];
        for p in 0..3 {
            dst.by_phase[p][i] += src.by_phase[p][i];
        }
    }
    for p in 0..3 {
        dst.phase_decisions[p] += src.phase_decisions[p];
    }
    dst.decisions += src.decisions;
}

fn main() {
    let cat = canon_catalog();
    let arg = std::env::args().nth(1).unwrap_or_default();
    let do_2v2 = arg != "1v1";

    println!(
        "Bot action-choice distribution — what % of decisions is each activity, per tier/faction/phase.\n\
         N={N} matches/cell, seeded (re-derives bit-identically). Each decision is classified BEFORE it is\n\
         applied, through the same legal_commands/apply seam a real client uses. A `PlaySpirit` of a Caller\n\
         card is a Call; any other body is a Play. The Glimpse burn / keep-or-bottom + target picks are\n\
         counted under `Choose` (sub-steps of an activity already counted), so the headline mix isn't\n\
         inflated by them. Phases split on the Dusk: opening r1-3, mid r4..=contraction, post-Dusk beyond.\n\
         Decks are Quick-Play GENERATED decks (curve-tuned, NOT evolution-density-tuned) — so a low Evolve\n\
         rate is EXPECTED, not a bug (see docs/decisions/bot_action_mix.md)."
    );

    for tier in Difficulty::ALL {
        sweep_1v1_tier(tier, &cat);
    }
    if do_2v2 {
        for tier in Difficulty::ALL {
            sweep_2v2_tier(tier, &cat);
        }
    } else {
        println!("\n(2v2 pass skipped — pass no arg, or any arg other than `1v1`, to include it.)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recollect_core::cards::canon_catalog;

    /// `classify` reads the played card's KIND to tell a Call (a Caller summoning a Kindred) from a
    /// Play (any other body) — the one piece of the bucketing that isn't a 1:1 command→activity map.
    /// Seat a Caller and a plain Spirit in hand and assert each `PlaySpirit` classifies correctly.
    #[test]
    fn classify_splits_call_from_play_by_card_kind() {
        let cat = canon_catalog();
        let caller = cat
            .iter()
            .find(|c| matches!(c.kind, CardKind::Caller))
            .expect("the catalog has a Caller");
        let spirit = cat
            .iter()
            .find(|c| matches!(c.kind, CardKind::Spirit))
            .expect("the catalog has a Spirit");
        let (mut e, _) = Engine::new(
            1,
            cat.clone(),
            recollect_bot::standard_deck(),
            recollect_bot::standard_deck(),
        );
        {
            let st = e.state_mut_for_test();
            st.active = Seat::A;
            // Hand = [Caller, Spirit]; index 0 is the Call, index 1 the Play.
            st.player_a.hand = vec![caller.id, spirit.id];
        }
        let call = Command::PlaySpirit {
            hand_index: 0,
            tile: 2,
            engage: None,
            chain_prefs: vec![],
        };
        let play = Command::PlaySpirit {
            hand_index: 1,
            tile: 2,
            engage: None,
            chain_prefs: vec![],
        };
        assert_eq!(
            classify(&e, &call),
            Activity::Call,
            "a Caller card is a Call"
        );
        assert_eq!(
            classify(&e, &play),
            Activity::Play,
            "a plain Spirit is a Play"
        );
    }

    /// The remaining commands map 1:1 to their activity (spot-check the headline + a few others), and
    /// the engine state is irrelevant to those arms.
    #[test]
    fn classify_maps_the_simple_commands() {
        let cat = canon_catalog();
        let (e, _) = Engine::new(
            2,
            cat,
            recollect_bot::standard_deck(),
            recollect_bot::standard_deck(),
        );
        assert_eq!(classify(&e, &Command::Glimpse), Activity::Glimpse);
        assert_eq!(classify(&e, &Command::EndTurn), Activity::EndTurn);
        assert_eq!(
            classify(&e, &Command::Mulligan { seat: Seat::A }),
            Activity::Mulligan
        );
        assert_eq!(
            classify(
                &e,
                &Command::MoveSpirit {
                    from: 0,
                    to: 1,
                    engage: None
                }
            ),
            Activity::Move
        );
        assert_eq!(
            classify(
                &e,
                &Command::Evolve {
                    tile: 0,
                    form_hand: 0,
                    fuel: None,
                    engage: None
                }
            ),
            Activity::Evolve
        );
        assert_eq!(
            classify(
                &e,
                &Command::Devolve {
                    tile: 0,
                    base_hand: 0
                }
            ),
            Activity::Devolve
        );
        assert_eq!(
            classify(&e, &Command::Reclaim { tile: 0 }),
            Activity::Reclaim
        );
        assert_eq!(
            classify(&e, &Command::Choose { index: 0 }),
            Activity::Choose
        );
        assert_eq!(classify(&e, &Command::BanishStray), Activity::BanishStray);
    }

    /// Phase bucketing splits on the Dusk (the round-8 contraction): opening 1–3, mid 4..=contraction,
    /// post-Dusk beyond.
    #[test]
    fn phase_buckets_split_on_the_dusk() {
        let c = 8; // contraction_after in 1v1
        assert!(matches!(GamePhase::of(1, c), GamePhase::Opening));
        assert!(matches!(GamePhase::of(3, c), GamePhase::Opening));
        assert!(matches!(GamePhase::of(4, c), GamePhase::Mid));
        assert!(matches!(GamePhase::of(8, c), GamePhase::Mid));
        assert!(matches!(GamePhase::of(9, c), GamePhase::PostDusk));
        assert!(matches!(GamePhase::of(12, c), GamePhase::PostDusk));
    }

    /// The headline activities are a subset of the full activity list, and the full list has no
    /// duplicates — so a column of per-activity percentages over `ALL` sums without double-counting.
    #[test]
    fn activity_lists_are_consistent() {
        for h in Activity::HEADLINE {
            assert!(
                Activity::ALL.contains(&h),
                "{} is missing from ALL",
                h.label()
            );
        }
        // No duplicate in ALL (each activity appears exactly once → the % mix sums cleanly).
        for (i, a) in Activity::ALL.iter().enumerate() {
            assert_eq!(
                Activity::ALL.iter().position(|x| x == a),
                Some(i),
                "{} appears twice in ALL",
                a.label()
            );
        }
    }

    /// A tiny end-to-end smoke: a handful of seeded matches tally SOME decisions, the mix sums to
    /// ~100%, and the EndTurn count is exactly one per round (a structural invariant — every turn ends
    /// with exactly one EndTurn, so per-match EndTurn == the round count). Guards the harness wiring
    /// without running the full (slow) sweep.
    #[test]
    fn smoke_tally_is_well_formed() {
        let cat = canon_catalog();
        let mut a = Tally::default();
        let mut b = Tally::default();
        let n = 8u64;
        for seed in 0..n {
            run_1v1(
                seed,
                Difficulty::Normal,
                Faction::Lorekeeper,
                Faction::Lorekeeper,
                &cat,
                &mut a,
                &mut b,
            );
        }
        a.matches = n;
        assert!(a.decisions > 0, "the tally recorded no decisions");
        let sum: f64 = Activity::ALL.into_iter().map(|x| a.pct(x)).sum();
        assert!(
            (sum - 100.0).abs() < 0.01,
            "the mix must sum to 100%, got {sum}"
        );
        // Every match ends each of its rounds with exactly one EndTurn, so per-match EndTurn equals
        // the (1v1) round count of 12 — a clean structural check the loop is driving full matches.
        assert_eq!(
            a.per_match(Activity::EndTurn),
            12.0,
            "per-match EndTurn should equal the 12-round 1v1 clock"
        );
    }
}
