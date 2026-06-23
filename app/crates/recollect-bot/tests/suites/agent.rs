//! The difficulty agent: every tier returns a legal move, and a fixed seed is
//! deterministic (so a replayed match reproduces). Strength ordering is
//! verified separately by the calibration fleet (bin/calibrate.rs), not here —
//! a unit test can't establish "Expert beats Easy" without many games.
use recollect_bot::{Difficulty, Faction, choose, choose_as, greedy_score_as};
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::generate_deck;
use recollect_core::rng::Rng;
use recollect_core::state::{Command, Phase};
use recollect_core::{Engine, Seat};

fn fresh() -> Engine {
    let cat = canon_catalog();
    let da = generate_deck(0, 7, &cat);
    let db = generate_deck(1, 11, &cat);
    Engine::new(1, cat, da, db).0
}

#[test]
fn every_difficulty_returns_a_legal_move() {
    for diff in [
        Difficulty::Easy,
        Difficulty::Normal,
        Difficulty::Hard,
        Difficulty::Expert,
    ] {
        let e = fresh();
        let mut rng = Rng::from_seed(42);
        let seat = e.state().active;
        let cmd = choose(&e, seat, diff, &mut rng);
        let legal = e.legal_commands(seat);
        assert!(
            legal.contains(&cmd),
            "{}: chose an illegal move {cmd:?}",
            diff.name()
        );
    }
}

#[test]
fn the_agent_can_play_a_whole_match_at_every_difficulty() {
    // No panics, the match terminates, and a real result is produced.
    for diff in [
        Difficulty::Easy,
        Difficulty::Normal,
        Difficulty::Hard,
        Difficulty::Expert,
    ] {
        let mut e = fresh();
        let mut rng = Rng::from_seed(99);
        let mut steps = 0;
        loop {
            if matches!(e.state().phase, Phase::Finished { .. }) {
                break;
            }
            assert!(steps < 2000, "{}: match did not terminate", diff.name());
            let seat = e.state().active;
            let cmd = choose(&e, seat, diff, &mut rng);
            e.apply(seat, cmd.clone())
                .unwrap_or_else(|r| panic!("{}: {cmd:?} rejected {r:?}", diff.name()));
            steps += 1;
        }
    }
}

#[test]
fn choices_are_deterministic_for_a_fixed_seed() {
    let run = || {
        let e = fresh();
        let mut rng = Rng::from_seed(7);
        let seat = e.state().active;
        choose(&e, seat, Difficulty::Hard, &mut rng)
    };
    assert_eq!(run(), run(), "same seed → same choice");
}

#[test]
fn the_difficulty_ladder_is_monotonic() {
    // The ladder must be MONOTONE *and adjacent-separated*: each stronger tier beats the
    // tier immediately below it well above 50% in the Lorekeeper mirror — Normal > Easy,
    // Hard > Normal, Expert > Hard — not merely the headline Expert > Easy. This is the
    // regression guard against the Bal1 overshoot, where Normal↔Hard had compressed to
    // ~53% (Hard barely cleared Normal) and Hard↔Expert to ~52%; the re-derived knobs
    // (Easy 400/1, Normal 90/1, Hard 35/2, Expert 8/2) restore clear gaps (~69/63/59 at
    // N=200 in `bin/calibrate`). The fine spacing lives in `calibrate`; this pins the
    // ORDERING so a future knob change can't silently flatten or invert a rung. Small
    // sample — enough to catch a flattened ladder, fast enough for the unit suite.
    use recollect_core::cards::canon_catalog;
    use recollect_core::quickplay::generate_deck;
    use recollect_core::state::MatchResult;
    let cat = canon_catalog();
    let play = |seed: u64, a: Difficulty, b: Difficulty| -> Option<Seat> {
        let da = generate_deck((seed % 6) as u8, seed, &cat);
        let db = generate_deck(((seed + 3) % 6) as u8, seed ^ 0x5EED, &cat);
        let (mut e, _) = Engine::new(seed, cat.clone(), da, db);
        let mut ra = Rng::from_seed(seed ^ 0xA);
        let mut rb = Rng::from_seed(seed ^ 0xB);
        let mut steps = 0;
        loop {
            if let Phase::Finished { .. } = e.state().phase {
                if let Phase::Finished { result, .. } = e.state().phase {
                    return match result {
                        MatchResult::Win(w) => Some(w),
                        MatchResult::Draw => None,
                    };
                }
                return None;
            }
            if steps > 5000 {
                return None;
            }
            let seat = e.state().active;
            let (d, rng) = if seat == Seat::A {
                (a, &mut ra)
            } else {
                (b, &mut rb)
            };
            let cmd = choose(&e, seat, d, rng);
            e.apply(seat, cmd).unwrap();
            steps += 1;
        }
    };
    // A's win rate over `n` mirror seeds (each played once; the stronger tier on seat A).
    let winrate = |a: Difficulty, b: Difficulty, n: u64| -> f64 {
        let wins = (0..n)
            .filter(|&seed| play(seed, a, b) == Some(Seat::A))
            .count();
        wins as f64 / n as f64
    };
    // Each adjacent step: the stronger tier (seat A) must clear a real majority. Threshold
    // 0.55 sits comfortably below the measured ~59/63/69 yet far enough above 0.50 to be a
    // genuine ladder, not noise. n=150 keeps the 95% interval tight (~±8pp) so the tightest
    // rung (Expert>Hard ~59%) stays safely above the bar.
    let n = 150u64;
    for (stronger, weaker) in [
        (Difficulty::Normal, Difficulty::Easy),
        (Difficulty::Hard, Difficulty::Normal),
        (Difficulty::Expert, Difficulty::Hard),
    ] {
        let rate = winrate(stronger, weaker, n);
        assert!(
            rate > 0.55,
            "ladder rung flattened: {} beats {} only {:.0}% (want >55% — adjacent tiers must stay separated)",
            stronger.name(),
            weaker.name(),
            rate * 100.0
        );
    }
    // The headline: Expert must dominate Easy outright (the ends of the ladder are far apart).
    let rate = winrate(Difficulty::Expert, Difficulty::Easy, 60);
    assert!(
        rate > 0.62,
        "Expert should dominate Easy; got {:.0}%",
        rate * 100.0
    );
}

#[test]
fn expert_lookahead_does_not_corrupt_determinism() {
    // Expert forks the engine state for its 1-ply lookahead every move. If that
    // fork leaked back into the live engine or desynced entropy, a replay would
    // diverge. Play a full Expert-vs-Expert match twice; require identical
    // results and identical entropy draw counts.
    let run = || {
        let cat = canon_catalog();
        let da = generate_deck(2, 5, &cat);
        let db = generate_deck(3, 9, &cat);
        let (mut e, _) = Engine::new(3, cat, da, db);
        let mut ra = Rng::from_seed(0xE1);
        let mut rb = Rng::from_seed(0xE2);
        let mut steps = 0;
        while !matches!(e.state().phase, Phase::Finished { .. }) && steps < 3000 {
            let seat = e.state().active;
            let rng = if seat == Seat::A { &mut ra } else { &mut rb };
            let cmd = choose(&e, seat, Difficulty::Expert, rng);
            e.apply(seat, cmd).unwrap();
            steps += 1;
        }
        (format!("{:?}", e.state().phase), e.entropy_draws())
    };
    assert_eq!(
        run(),
        run(),
        "Expert lookahead must not break match determinism"
    );
}

// --- faction-aware agent ------------------------------------------------------

#[test]
fn the_solace_values_stainless_removal_over_a_lorekeeper() {
    // Release removes a fading spirit with no impression — denial the Solace prizes
    // (no impression fallback), and a Lorekeeper values less (it banks impressions by
    // trading). The same scorer, branched by faction.
    let e = fresh();
    let seat = e.state().active;
    let rel = Command::Release { hand_index: 0 };
    let solace = greedy_score_as(&e, seat, &rel, Faction::Solace);
    let keeper = greedy_score_as(&e, seat, &rel, Faction::Lorekeeper);
    assert!(
        solace > keeper,
        "the Solace up-weights stainless removal: {solace} vs {keeper}"
    );
    // The Lorekeeper alias is exactly the faction-agnostic `greedy_score`.
    assert_eq!(
        keeper,
        recollect_bot::greedy_score(&e, seat, &rel),
        "greedy_score is the Lorekeeper alias"
    );
}

#[test]
fn a_faction_matchup_plays_to_a_result() {
    // Seat A pilots the Solace, Seat B a Lorekeeper, each modelling the other's
    // faction in its lookahead — the faction path is wired at every depth.
    for diff in [Difficulty::Normal, Difficulty::Expert] {
        let mut e = fresh();
        let mut rng = Rng::from_seed(123);
        let mut steps = 0;
        while !matches!(e.state().phase, Phase::Finished { .. }) {
            assert!(steps < 2000, "{}: match did not terminate", diff.name());
            let seat = e.state().active;
            let cmd = if seat == Seat::A {
                choose_as(
                    &e,
                    seat,
                    diff,
                    Faction::Solace,
                    Faction::Lorekeeper,
                    &mut rng,
                )
            } else {
                choose_as(
                    &e,
                    seat,
                    diff,
                    Faction::Lorekeeper,
                    Faction::Solace,
                    &mut rng,
                )
            };
            let legal = e.legal_commands(seat);
            assert!(legal.contains(&cmd), "{}: illegal {cmd:?}", diff.name());
            e.apply(seat, cmd.clone())
                .unwrap_or_else(|r| panic!("{}: {cmd:?} rejected {r:?}", diff.name()));
            steps += 1;
        }
    }
}
