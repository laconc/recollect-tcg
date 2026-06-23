//! The `decide_journaled`/`Decided` seam — the in-memory half of the
//! postgres-authoritative server loop. It must reproduce `Engine::apply` exactly
//! (commit), leave nothing observable on `abort`, and rewind the entropy stream
//! on a rule rejection. The durable append that the server wedges between
//! `decide_journaled` and `commit` is proven separately against Postgres
//! (`recollect-journal-postgres` contract + async tests).
use crate::common::new_match;

use recollect_core::Command;
use recollect_core::state::Phase;

/// Driving the same first-legal sequence through `apply` and through
/// `decide_journaled` + `commit` keeps two engines bit-identical: same events,
/// same state, same entropy position. This is what lets the server move the
/// durability boundary without changing the game.
#[test]
fn decide_journaled_commit_matches_apply() {
    let mut by_apply = new_match(7);
    let mut by_seam = new_match(7);
    let mut steps = 0;
    for _ in 0..60 {
        if matches!(by_apply.state().phase, Phase::Finished { .. }) {
            break;
        }
        let seat = by_apply.state().active;
        let cmd = by_apply
            .legal_commands(seat)
            .first()
            .cloned()
            .expect("a command is always legal");

        let events_apply = by_apply.apply(seat, cmd.clone()).expect("apply ok");
        let decided = by_seam.decide_journaled(seat, &cmd).expect("decide ok");
        let draws = decided.draws();
        let events_seam = decided.commit();

        assert_eq!(events_apply, events_seam, "same events");
        assert_eq!(
            by_apply.snapshot(),
            by_seam.snapshot(),
            "same state + draws"
        );
        assert_eq!(
            draws.0,
            by_seam.entropy_draws(),
            "draws() is the post-commit position, what gets journaled"
        );
        steps += 1;
    }
    assert!(steps > 5, "the run actually exercised several commands");
    assert!(by_apply.entropy_draws() > 0, "the run drew entropy");
}

/// `abort` (the append-failed path) leaves the engine exactly as it was — no
/// state change, no entropy advanced.
#[test]
fn decide_journaled_abort_leaves_nothing() {
    let mut e = new_match(11);
    for _ in 0..3 {
        let seat = e.state().active;
        let cmd = e.legal_commands(seat).first().cloned().unwrap();
        e.apply(seat, cmd).unwrap();
    }
    let before = e.snapshot();

    let seat = e.state().active;
    let cmd = e.legal_commands(seat).first().cloned().unwrap();
    e.decide_journaled(seat, &cmd).expect("decide ok").abort();

    assert_eq!(e.snapshot(), before, "abort rewound state + entropy");
}

/// A rule rejection rewinds the stream before returning — a refused command
/// leaves nothing observable, the same contract `apply` honors.
#[test]
fn decide_journaled_rejection_rewinds() {
    let mut e = new_match(5);
    let before = e.snapshot();
    // The non-active seat acting is a rule rejection.
    let wrong_seat = e.state().active.other();
    let rejected = e.decide_journaled(wrong_seat, &Command::EndTurn).is_err();
    assert!(rejected, "out-of-turn command rejected");
    assert_eq!(
        e.snapshot(),
        before,
        "a rejected command left the stream untouched"
    );
}
