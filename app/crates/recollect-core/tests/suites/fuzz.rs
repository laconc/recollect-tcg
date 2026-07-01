//! The canonical gameplay fuzz / red-team (`make fuzz` / `make soak`) — a FULL-CATALOG
//! playthrough. It builds decks from the **canon catalog** (all 419 cards: every evolution
//! line, Bond, Landmark, Ritual, Unwriting, and the Solace's Unwritten) for 1v1 Lorekeeper,
//! 1v1 vs the Solace, and 2v2 — so the FULL card set is actually *played* through a game, not
//! the small ~10-card test-catalog slice the earlier fuzzer used. It drives a board-shaping
//! random legal playthrough to the finish, and after EVERY command asserts
//!   * the state-validity invariants (`invariants::check`),
//!   * snapshot→restore parity (same state + same legal set — a server restart is safe),
//!   * a structural redaction probe (no opponent hand/peek/pending leaks), and
//!   * two semantic guards no structural invariant catches: a fading spirit never lingers
//!     past its standing-Faded window, and a stray telegraph never goes stale — both tells
//!     of a *lost mutation* (a direct `sim` write in `decide` that never rode an event onto
//!     the committed board).
//!
//! Three further arms — the canon-catalog successors to the retired small-catalog fuzzer,
//! run over the same canon decks and all match modes — close the gaps it left:
//!   * `canon_replays_are_bit_identical` (DETERMINISM): the same seed + the same random
//!     policy yields the EXACT same event stream and entropy-draw count on a re-run. A
//!     divergence is hidden nondeterminism (a float, a HashMap iteration, an unseeded draw) —
//!     the bug class that corrupts replay and offline rewards.
//!   * `canon_rejected_commands_leave_no_trace` (REJECTION): a spray of likely-illegal
//!     commands fired at each step leaves the snapshot byte-identical and the entropy
//!     unmoved — a rejected command leaves NOTHING observable.
//!   * the SOAK mode: every `play` arm honours `FUZZ_SECONDS` (a wall-clock budget) in place
//!     of the seed count, so `make soak SECONDS=1800` runs a fixed interval on the full catalog.
//!
//! Deterministic (seeded), so any failure reproduces exactly. `RT_SEEDS=N` overrides the
//! per-run seed count for a deeper local/nightly sweep; `FUZZ_SECONDS=N` caps wall-clock time
//! instead (the soak knob); `RT_SEED_BASE=M` shifts the seed window so back-to-back runs cover
//! disjoint ranges (base 0 then base N ⇒ 2N unique games) or a nightly job can shard.
use recollect_core::cards::canon_catalog;
use recollect_core::invariants::check as check_invariants;
use recollect_core::quickplay::{generate_deck_for, solace_character_deck};
use recollect_core::rng::Rng;
use recollect_core::state::{Command, Phase};
use recollect_core::types::Faction;
use recollect_core::view::view_for;
use recollect_core::{Engine, Seat};

/// Parse an env var as a `u64` (the shared `RT_SEEDS` / `RT_SEED_BASE` / `FUZZ_SECONDS` knobs).
fn env_u64(k: &str) -> Option<u64> {
    std::env::var(k).ok().and_then(|v| v.parse::<u64>().ok())
}

/// Pick a board-shaping command from `legal` with the same weighting `play` uses, advancing
/// `pol` deterministically. The three fuzz arms share this so their policies match.
fn weighted_pick(legal: &[Command], pol: &mut Rng) -> Command {
    let total: u64 = legal.iter().map(weight).sum();
    let mut pick = pol.next_u64() % total;
    legal
        .iter()
        .find(|c| {
            let w = weight(c);
            (pick < w) || {
                pick -= w;
                false
            }
        })
        .unwrap_or(&legal[0])
        .clone()
}

/// A canon-deck 1v1: A is a seeded Lorekeeper style; B is either another Lorekeeper
/// style or a Solace character (PvE), which flips seat B's faction to Solace.
fn canon_1v1(seed: u64, b_solace: bool) -> Engine {
    let cat = canon_catalog();
    let deck_a = generate_deck_for(Faction::Lorekeeper, (seed % 5) as u8, seed, &cat);
    let mut rules = recollect_core::state::MatchRules::default();
    let deck_b = if b_solace {
        rules.factions = [Faction::Lorekeeper, Faction::Solace];
        solace_character_deck((seed % 20) as usize, seed ^ 0x5011, &cat)
    } else {
        generate_deck_for(
            Faction::Lorekeeper,
            ((seed / 5) % 5) as u8,
            seed ^ 0x99,
            &cat,
        )
    };
    Engine::new_with_rules(seed, cat, deck_a, deck_b, rules, Seat::A).0
}

fn canon_2v2(seed: u64) -> Engine {
    let cat = canon_catalog();
    let d = |s: u64| generate_deck_for(Faction::Lorekeeper, (s % 5) as u8, s, &cat);
    let decks = [d(seed), d(seed ^ 1), d(seed ^ 2), d(seed ^ 3)];
    Engine::new_2v2(seed, cat, decks).0
}

/// Bias toward board-shaping moves so playouts reach the contested late game and the
/// Dusk — the deep states where rare interactions surface.
fn weight(cmd: &Command) -> u64 {
    use Command::*;
    match cmd {
        PlaySpirit {
            engage: Some(_), ..
        } => 9,
        Evolve { .. } | Devolve { .. } => 7,
        PlaySpirit { .. } | Choose { .. } => 6,
        Overwrite { .. }
        | MoveSpirit {
            engage: Some(_), ..
        } => 5,
        Reveal {
            engage: Some(_), ..
        }
        | CastRitual { .. }
        | AttachBond { .. }
        | PlaceLandmark { .. }
        | SetFabrication { .. }
        | TellUnwriting { .. } => 4,
        StrikeFabrication { .. } | Reveal { .. } => 3,
        MoveSpirit { .. } => 2,
        Glimpse | EndTurn => 1,
        _ => 2,
    }
}

/// Each seat's serialized view carries at most its own private keys — no opponent
/// hand/peek/pending ever leaks (the structural half of the redaction contract).
fn redaction_probe(e: &Engine, label: &str, seed: u64, step: usize) {
    let cap = if e.state().is_2v2() { 2 } else { 1 };
    for seat in [Seat::A, Seat::B] {
        let json = serde_json::to_string(&view_for(e, seat)).unwrap_or_else(|err| {
            panic!("{label} seed {seed} step {step}: view {seat:?} ser: {err}")
        });
        for key in ["\"hand\":", "\"peeked_top\":", "\"pending\":"] {
            assert!(
                json.matches(key).count() <= cap,
                "{label} seed {seed} step {step}: view {seat:?} leaks `{key}` (cap {cap})"
            );
        }
    }
}

fn play(label: &str, n: u64, mk: impl Fn(u64) -> Engine) {
    let n = env_u64("RT_SEEDS").unwrap_or(n);
    // `RT_SEED_BASE` shifts the seed window so a deeper sweep can cover a *disjoint*
    // range (run base 0 and base N back-to-back for 2N unique games), or a nightly job
    // can fan disjoint shards across machines. Each seed is still fully deterministic.
    let base = env_u64("RT_SEED_BASE").unwrap_or(0);
    // SOAK alternative: `FUZZ_SECONDS` caps wall-clock time instead of the seed count, so
    // `make soak SECONDS=1800` runs a fixed interval (nightly / manual long runs) over the
    // full catalog. When set, we keep drawing fresh seeds past `base + n` until the budget
    // expires; unset, the seed window is `base..base + n` as before.
    let budget = env_u64("FUZZ_SECONDS").map(std::time::Duration::from_secs);
    let start = std::time::Instant::now();
    let cat = canon_catalog();
    let mut seed = base;
    loop {
        match budget {
            Some(b) if start.elapsed() >= b => break,
            None if seed >= base + n => break,
            _ => {}
        }
        let mut e = mk(seed);
        let mut pol = Rng::from_seed(seed ^ 0xB1A5_ED_C0DE);
        check_invariants(e.state())
            .unwrap_or_else(|m| panic!("{label} seed {seed}: opening invariant: {m}"));
        // How many rounds each tile has held a *fading* spirit. The standing-Faded window
        // closes by its owner's next turn-end (+1 for one Hold-the-Memory skip), so a body
        // fading for many rounds means a dissolve was lost on the committed board.
        let mut fading_age = [0u32; 36];
        let mut prev_round = e.state().round;
        let mut steps = 0usize;
        loop {
            if matches!(e.state().phase, Phase::Finished { .. }) {
                for (i, t) in e.state().board.iter().enumerate() {
                    assert!(
                        t.spirit.as_ref().map(|s| !s.fading).unwrap_or(true),
                        "{label} seed {seed}: a spirit still fades at match end (tile {i}) — lost dissolve"
                    );
                }
                break;
            }
            if e.state().round != prev_round {
                prev_round = e.state().round;
                for (i, t) in e.state().board.iter().enumerate() {
                    if t.spirit.as_ref().map(|s| s.fading).unwrap_or(false) {
                        fading_age[i] += 1;
                        assert!(
                            fading_age[i] <= 4,
                            "{label} seed {seed}: tile {i} has held a fading spirit {} rounds — its \
                             standing-Faded window should have closed (a lost dissolve?)",
                            fading_age[i]
                        );
                    } else {
                        fading_age[i] = 0;
                    }
                }
            }
            assert!(
                steps < 4000,
                "{label} seed {seed}: the match must end (step {steps})"
            );
            let seat = e.state().active;
            let legal = e.legal_commands(seat);
            assert!(
                !legal.is_empty(),
                "{label} seed {seed} step {steps}: no legal command"
            );
            let cmd = weighted_pick(&legal, &mut pol);
            // Snapshot→restore parity: a persisted-then-restored engine is identical and
            // offers the same moves (a server restart can't corrupt a live match).
            let (snap, pos) = e.snapshot();
            let restored = Engine::from_state(snap.clone(), 0, pos, cat.clone());
            assert_eq!(
                restored.state(),
                &snap,
                "{label} seed {seed} step {steps}: restore differs"
            );
            assert_eq!(
                restored.legal_commands(seat),
                legal,
                "{label} seed {seed} step {steps}: restored legal set differs"
            );
            e.apply(seat, cmd.clone()).unwrap_or_else(|r| {
                panic!("{label} seed {seed} step {steps}: legal {cmd:?} rejected {r:?}")
            });
            check_invariants(e.state())
                .unwrap_or_else(|m| panic!("{label} seed {seed} step {steps} after {cmd:?}: {m}"));
            redaction_probe(&e, label, seed, steps);
            if let Some(tele) = &e.state().stray_telegraph {
                assert!(
                    tele.surface_round >= e.state().round,
                    "{label} seed {seed} step {steps}: stray telegraph points at past round {} (now {}) — stale shimmer",
                    tele.surface_round,
                    e.state().round
                );
            }
            steps += 1;
        }
        seed += 1;
    }
}

#[test]
fn canon_1v1_lorekeeper_playthroughs_hold_every_invariant() {
    play("canon-1v1", 40, |s| canon_1v1(s, false));
}

#[test]
fn canon_1v1_solace_playthroughs_hold_every_invariant() {
    play("canon-solace", 40, |s| canon_1v1(s, true));
}

#[test]
fn canon_2v2_playthroughs_hold_every_invariant() {
    play("canon-2v2", 30, canon_2v2);
}

// ---------------------------------------------------------------------------
// The three arms ported from the retired small-catalog fuzzer, now over the
// FULL canon catalog and every match mode.
// ---------------------------------------------------------------------------

/// Drive one seeded canon playout with the same board-shaping policy `play` uses, recording
/// the event stream (one stringified `Vec<Event>` per command) and the final entropy-draw
/// count. The `(seed, policy)` pair fully determines the run, so two calls must agree.
fn replay_log(mk: &impl Fn(u64) -> Engine, seed: u64) -> (Vec<String>, u64) {
    let mut e = mk(seed);
    let mut pol = Rng::from_seed(seed ^ 0xB1A5_ED_C0DE);
    let mut log = Vec::new();
    let mut steps = 0usize;
    while !matches!(e.state().phase, Phase::Finished { .. }) && steps < 4000 {
        let seat = e.state().active;
        let legal = e.legal_commands(seat);
        if legal.is_empty() {
            break;
        }
        let cmd = weighted_pick(&legal, &mut pol);
        let evs = e.apply(seat, cmd).unwrap();
        log.push(format!("{evs:?}"));
        steps += 1;
    }
    (log, e.entropy_draws())
}

/// DETERMINISM red-team over the canon catalog: the same seed + the same policy must produce
/// the EXACT same event stream and entropy-draw count on a re-run, across every match mode.
/// A divergence means hidden nondeterminism (a float, a HashMap iteration, an unseeded draw) —
/// the bug class that corrupts replay and offline rewards. (The retired small-catalog fuzzer
/// proved this only over ~10 cards; this proves it over all 419 + Solace PvE + 2v2.)
#[test]
fn canon_replays_are_bit_identical() {
    let n = env_u64("RT_SEEDS").unwrap_or(20);
    let base = env_u64("RT_SEED_BASE").unwrap_or(0);
    let budget = env_u64("FUZZ_SECONDS").map(std::time::Duration::from_secs);
    let start = std::time::Instant::now();
    let modes: [(&str, &dyn Fn(u64) -> Engine); 3] = [
        ("canon-1v1", &|s| canon_1v1(s, false)),
        ("canon-solace", &|s| canon_1v1(s, true)),
        ("canon-2v2", &canon_2v2),
    ];
    let mut seed = base;
    loop {
        match budget {
            Some(b) if start.elapsed() >= b => break,
            None if seed >= base + n => break,
            _ => {}
        }
        for (label, mk) in &modes {
            let (a_log, a_draws) = replay_log(mk, seed);
            let (b_log, b_draws) = replay_log(mk, seed);
            assert_eq!(
                a_log, b_log,
                "{label} seed {seed}: event streams diverged on replay"
            );
            assert_eq!(
                a_draws, b_draws,
                "{label} seed {seed}: entropy draw count diverged"
            );
        }
        seed += 1;
    }
}

/// REJECTION red-team over the canon catalog: a rejected command must leave NOTHING
/// observable — no state change, no entropy movement. At each step we fire a spray of
/// likely-illegal commands (out-of-range hand/tile indices across the whole vocabulary) and
/// assert the snapshot is byte-identical and the entropy unmoved before/after each rejection,
/// then advance with a real board-shaping move. Run on every match mode. (Where `security.rs`
/// fires garbage at the OPENING state, this fires it at every reachable mid-game state of a
/// full-catalog playout.)
#[test]
fn canon_rejected_commands_leave_no_trace() {
    use Command::*;
    let n = env_u64("RT_SEEDS").unwrap_or(20);
    let base = env_u64("RT_SEED_BASE").unwrap_or(0);
    let budget = env_u64("FUZZ_SECONDS").map(std::time::Duration::from_secs);
    let start = std::time::Instant::now();
    let modes: [(&str, &dyn Fn(u64) -> Engine); 3] = [
        ("canon-1v1", &|s| canon_1v1(s, false)),
        ("canon-solace", &|s| canon_1v1(s, true)),
        ("canon-2v2", &canon_2v2),
    ];
    let mut seed = base;
    loop {
        match budget {
            Some(b) if start.elapsed() >= b => break,
            None if seed >= base + n => break,
            _ => {}
        }
        for (label, mk) in &modes {
            let mut e = mk(seed);
            let mut pol = Rng::from_seed(seed ^ 0xDEAD_BEEF_C0DE);
            let mut steps = 0usize;
            while !matches!(e.state().phase, Phase::Finished { .. }) && steps < 600 {
                let seat = e.state().active;
                // Plausible-but-out-of-range commands spanning the vocabulary; each should be
                // rejected, and a rejection must move neither the state nor the entropy.
                let probes = [
                    PlaySpirit {
                        hand_index: 99,
                        tile: 99,
                        engage: Some(99),
                        chain_prefs: vec![],
                    },
                    Overwrite {
                        hand_index: 250,
                        tile: 250,
                    },
                    MoveSpirit {
                        from: 200,
                        to: 201,
                        engage: None,
                    },
                    Evolve {
                        tile: 99,
                        form_hand: 9,
                        fuel: None,
                        engage: None,
                    },
                    Devolve {
                        tile: 99,
                        base_hand: 99,
                    },
                    StrikeFabrication { from: 99, tile: 99 },
                    Choose { index: 200 },
                    Reveal {
                        tile: 200,
                        engage: None,
                    },
                ];
                for p in probes {
                    let before = e.snapshot();
                    let before_draws = e.entropy_draws();
                    if e.apply(seat, p.clone()).is_err() {
                        assert_eq!(
                            before,
                            e.snapshot(),
                            "{label} seed {seed} step {steps}: rejected {p:?} mutated state"
                        );
                        assert_eq!(
                            before_draws,
                            e.entropy_draws(),
                            "{label} seed {seed} step {steps}: rejected {p:?} moved entropy"
                        );
                    }
                }
                // Advance with a real board-shaping move.
                let legal = e.legal_commands(seat);
                let cmd = weighted_pick(&legal, &mut pol);
                e.apply(seat, cmd).unwrap();
                steps += 1;
            }
        }
        seed += 1;
    }
}
