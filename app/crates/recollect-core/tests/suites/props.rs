//! Property-based tests (proptest) — the SHRINKING half of the red-team. Where
//! the full-catalog playthrough (`suites/fuzz.rs`, `make fuzz`) hammers the
//! engine with many seeded playouts and prints the seed on failure, these state
//! PROPERTIES over a generated input (a match seed + a bounded stream of indices that
//! pick `legal[idx % len]` each step — the classic model-based shape) and let proptest
//! minimize any failure to the SMALLEST command sequence that breaks the law. The broad
//! random-playout logic has its home in that playthrough harness.
use crate::common::*;
use proptest::prelude::*;
use recollect_core::invariants::check as check_invariants;
use recollect_core::state::Phase;
use recollect_core::view::view_for;
use recollect_core::{Command, Engine, Seat};

/// Per-PR case count; nightly cranks it via PROPTEST_CASES (mirrors the playthrough
/// fuzz's `RT_SEEDS` knob). Found counterexamples are replayed forever via the
/// committed `proptest-regressions/` seed file.
fn config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64);
    ProptestConfig {
        cases,
        ..ProptestConfig::default()
    }
}

/// A generated input: a match seed + a bounded index stream.
fn input() -> impl Strategy<Value = (u64, Vec<u16>)> {
    (any::<u64>(), prop::collection::vec(any::<u16>(), 0..80))
}

/// The command the index `idx` selects from the active seat's legal menu, if any.
fn nth_legal(e: &Engine, idx: u16) -> Option<(Seat, Command)> {
    let seat = e.state().active;
    let legal = e.legal_commands(seat);
    if legal.is_empty() {
        return None;
    }
    Some((seat, legal[(idx as usize) % legal.len()].clone()))
}

proptest! {
    #![proptest_config(config())]

    /// Determinism (invariant #1): the same seed + the same index stream produce
    /// an identical event log, entropy position, and final state. Shrinks to the
    /// minimal diverging step if hidden nondeterminism ever creeps in.
    #[test]
    fn determinism((seed, idxs) in input()) {
        let run = || {
            let mut e = new_match(seed);
            let mut events = Vec::new();
            for &idx in &idxs {
                if matches!(e.state().phase, Phase::Finished { .. }) {
                    break;
                }
                let Some((seat, cmd)) = nth_legal(&e, idx) else { break };
                match e.apply(seat, cmd) {
                    Ok(evs) => events.extend(evs),
                    Err(_) => break,
                }
            }
            (
                serde_json::to_string(&events).unwrap(),
                e.entropy_draws(),
                serde_json::to_string(&e.snapshot().0).unwrap(),
            )
        };
        prop_assert_eq!(run(), run());
    }

    /// Legal-never-rejects: every command the engine OFFERS in `legal_commands`
    /// applies successfully. Shrinks to the minimal offending command.
    #[test]
    fn legal_commands_never_reject((seed, idxs) in input()) {
        let mut e = new_match(seed);
        for &idx in &idxs {
            if matches!(e.state().phase, Phase::Finished { .. }) {
                break;
            }
            let Some((seat, cmd)) = nth_legal(&e, idx) else { break };
            prop_assert!(
                e.apply(seat, cmd.clone()).is_ok(),
                "a legal command was rejected: {:?}",
                cmd
            );
        }
    }

    /// Redaction (invariant #2): through every reachable state, neither seat's
    /// `PlayerView` leaks the opponent's hand — exactly one `"hand":` serializes
    /// (the viewer's own; the opponent is counts-only).
    #[test]
    fn views_never_leak_the_opponents_hand((seed, idxs) in input()) {
        let mut e = new_match(seed);
        let mut step = 0usize;
        loop {
            for seat in [Seat::A, Seat::B] {
                let json = serde_json::to_string(&view_for(&e, seat)).unwrap();
                prop_assert_eq!(
                    json.matches("\"hand\":").count(),
                    1usize,
                    "a seat's view exposed more than its own hand"
                );
            }
            if step >= idxs.len() || matches!(e.state().phase, Phase::Finished { .. }) {
                break;
            }
            let Some((seat, cmd)) = nth_legal(&e, idxs[step]) else { break };
            if e.apply(seat, cmd).is_err() {
                break;
            }
            step += 1;
        }
    }

    /// The shared state-validity suite holds after every command — and this is
    /// where the **score bound** lives (Dominion never exceeds the board's tile
    /// count is one of `invariants::check`'s clauses). Shrinks to the minimal
    /// command sequence that breaks any invariant.
    #[test]
    fn invariants_hold_after_every_command((seed, idxs) in input()) {
        let mut e = new_match(seed);
        prop_assert!(check_invariants(e.state()).is_ok(), "opening state unsound");
        for &idx in &idxs {
            if matches!(e.state().phase, Phase::Finished { .. }) {
                break;
            }
            let Some((seat, cmd)) = nth_legal(&e, idx) else { break };
            if e.apply(seat, cmd.clone()).is_err() {
                break;
            }
            prop_assert!(
                check_invariants(e.state()).is_ok(),
                "{:?} broke an invariant: {}",
                cmd,
                check_invariants(e.state()).unwrap_err()
            );
        }
    }
}
