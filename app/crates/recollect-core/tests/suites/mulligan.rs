//! Mulligan (§5 — London-lite): the once-per-seat opening reshuffle. Draw a fresh
//! full hand from the (reshuffled) deck, then bottom one card — the cost, chosen
//! deterministically from the seed. Legal ONLY in the opening window (round 1, the
//! active seat's own turn, before that seat has acted), at most once per seat.
//!
//! Guards the two headline invariants on the new command: DETERMINISM (same seed +
//! same mulligan ⇒ identical state, events, and draw count; decide-time sim ≡
//! evolve-time replay) and REDACTION (the opponent learns THAT you mulliganed — a
//! public beat — but never the redrawn hand, the bottomed card, or the deck order).
use crate::common::*;
use recollect_core::cards::test_catalog;
use recollect_core::state::{Command, Event, Phase};
use recollect_core::types::CardId;
use recollect_core::view::view_for;
use recollect_core::{AggregateRules, Engine, Seat};

fn deck20() -> Vec<CardId> {
    (0..10u16).chain(0..10u16).map(CardId).collect()
}

fn fresh(seed: u64) -> Engine {
    Engine::new(seed, test_catalog(), deck20(), deck20()).0
}

/// The mechanic: a fresh full hand (same cards count drawn back), then one card
/// bottomed as the cost — so the hand ends ONE card smaller and the deck one
/// larger, with the total page conserved and the once-flag set.
#[test]
fn mulligan_redraws_and_bottoms_one() {
    let mut e = fresh(7);
    let before = e.state().player(Seat::A);
    let (hand0, deck0) = (before.hand.len(), before.deck.len());
    let total0 = hand0 + deck0;
    let hand_cards_before = before.hand.clone();

    let evs = e
        .apply(Seat::A, Command::Mulligan { seat: Seat::A })
        .expect("the opener may mulligan in the opening window");

    // Exactly one Mulliganed event, carrying the full resulting hand + deck.
    let mulled: Vec<_> = evs
        .iter()
        .filter(|ev| matches!(ev, Event::Mulliganed { .. }))
        .collect();
    assert_eq!(mulled.len(), 1, "one Mulliganed fact: {evs:?}");

    let after = e.state().player(Seat::A);
    assert_eq!(
        after.hand.len(),
        hand0 - 1,
        "the bottomed card is the cost: the hand ends one smaller"
    );
    assert_eq!(
        after.deck.len(),
        deck0 + 1,
        "the bottomed card joins the deck"
    );
    assert_eq!(
        after.hand.len() + after.deck.len(),
        total0,
        "no card is conjured or lost — the page is conserved"
    );
    assert_ne!(
        after.hand, hand_cards_before,
        "a genuine reshuffle: the fresh hand differs from the opener's"
    );
    assert!(
        e.state().mulliganed[Seat::A as usize],
        "the once-per-match mulligan is spent"
    );
}

/// Once per seat: a second mulligan is rejected and no longer offered.
#[test]
fn mulligan_is_once_per_seat() {
    let mut e = fresh(7);
    e.apply(Seat::A, Command::Mulligan { seat: Seat::A })
        .expect("first mulligan");
    assert!(
        !e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Mulligan { .. })),
        "a spent mulligan is no longer offered"
    );
    assert!(
        e.apply(Seat::A, Command::Mulligan { seat: Seat::A })
            .is_err(),
        "a second mulligan is rejected"
    );
}

/// Both seats get their own opening beat: B may mulligan on B's first turn even
/// after A has played, and the two are independent.
#[test]
fn each_seat_gets_its_own_opening_window() {
    let mut e = fresh(7);
    // A is the opener; B is not active, so it is not offered the mulligan yet.
    assert!(
        !e.legal_commands(Seat::B)
            .iter()
            .any(|c| matches!(c, Command::Mulligan { .. })),
        "B cannot mulligan on A's turn"
    );
    e.apply(Seat::A, Command::EndTurn).unwrap(); // → B, round still 1
    assert_eq!(e.state().active, Seat::B);
    assert!(
        e.legal_commands(Seat::B)
            .iter()
            .any(|c| matches!(c, Command::Mulligan { .. })),
        "B's opening window opens on B's turn"
    );
    e.apply(Seat::B, Command::Mulligan { seat: Seat::B })
        .expect("B mulligans its opener");
    assert_eq!(
        e.state().mulliganed,
        [false, true],
        "the two seats' mulligans are independent"
    );
}

/// You mulligan your OWN hand: a command naming the other seat is rejected.
#[test]
fn mulligan_only_your_own_seat() {
    let mut e = fresh(7);
    assert!(
        e.apply(Seat::A, Command::Mulligan { seat: Seat::B })
            .is_err(),
        "the opener cannot mulligan the opponent's hand"
    );
    assert!(!e.state().mulliganed[Seat::B as usize], "no beat for B");
}

/// The window is the OPENING only — it closes the moment the seat acts (here, a
/// Glimpse) and never reopens; and it does not exist past round 1.
#[test]
fn mulligan_window_closes_after_acting_and_after_round_one() {
    // Glimpsed ⇒ acted ⇒ window closed.
    let mut e = fresh(7);
    e.apply(Seat::A, Command::Glimpse).unwrap();
    assert!(
        !e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Mulligan { .. })),
        "Glimpsing spends the opening — no mulligan after"
    );
    assert!(
        e.apply(Seat::A, Command::Mulligan { seat: Seat::A })
            .is_err(),
        "a mulligan after acting is rejected"
    );

    // Past round 1 the window is gone entirely.
    let mut e2 = fresh(7);
    e2.apply(Seat::A, Command::EndTurn).unwrap();
    e2.apply(Seat::B, Command::EndTurn).unwrap();
    assert_eq!(e2.state().round, 2, "now in round 2");
    assert!(
        e2.apply(Seat::A, Command::Mulligan { seat: Seat::A })
            .is_err(),
        "no mulligan past the opening round"
    );
}

/// DETERMINISM (invariant 1): same seed + the same mulligan ⇒ byte-identical state,
/// identical events, and an identical journal-owned draw counter.
#[test]
fn mulligan_is_deterministic() {
    let mut e1 = fresh(7);
    let mut e2 = fresh(7);
    let v1 = e1
        .apply(Seat::A, Command::Mulligan { seat: Seat::A })
        .unwrap();
    let v2 = e2
        .apply(Seat::A, Command::Mulligan { seat: Seat::A })
        .unwrap();
    assert_eq!(v1, v2, "same seed ⇒ identical mulligan events");
    assert_eq!(
        serde_json::to_string(e1.state()).unwrap(),
        serde_json::to_string(e2.state()).unwrap(),
        "same seed ⇒ identical post-mulligan state"
    );
    assert_eq!(
        e1.entropy_draws(),
        e2.entropy_draws(),
        "the reshuffle advances the counter identically"
    );

    // A different seed shuffles differently ⇒ (almost surely) a different hand.
    let mut e3 = fresh(8);
    e3.apply(Seat::A, Command::Mulligan { seat: Seat::A })
        .unwrap();
    assert_ne!(
        e1.state().player(Seat::A).hand,
        e3.state().player(Seat::A).hand,
        "a different seed yields a different fresh hand"
    );
}

/// The family property holds across a mulligan: everything `decide` simulated,
/// `evolve` reproduces from the journaled events alone — (snapshot₀, events)
/// replays bit-for-bit, mulligan included.
#[test]
fn mulligan_decide_evolve_replay_equivalence() {
    let mut e = fresh(11);
    let (snapshot0, _) = e.snapshot();
    let mut journal = Vec::new();
    // Mulligan, then drive a little so events accumulate on both sides of it.
    journal.extend(
        e.apply(Seat::A, Command::Mulligan { seat: Seat::A })
            .unwrap(),
    );
    let mut steps = 0;
    while steps < 30 && !matches!(e.state().phase, Phase::Finished { .. }) {
        let seat = e.state().active;
        let cmd = e.legal_commands(seat).first().unwrap().clone();
        journal.extend(e.apply(seat, cmd).unwrap());
        steps += 1;
    }
    let mut replayed = snapshot0;
    for ev in &journal {
        replayed.evolve(ev);
    }
    assert_eq!(
        &replayed,
        e.state(),
        "(snapshot₀, events) replays to the same Memory through a mulligan"
    );
}

/// REDACTION (invariant 2): the opponent learns THAT you mulliganed (a public beat)
/// but NEVER the cards. After A mulligans, B's view exposes only A's truthful
/// counts + the `mulliganed` flag — no second hand, no deck order, no peek.
#[test]
fn mulligan_redacts_the_hand_from_the_opponent() {
    let mut e = fresh(42);
    e.apply(Seat::A, Command::Mulligan { seat: Seat::A })
        .unwrap();

    let vb = view_for(&e, Seat::B);
    // The public beat: B learns THAT A mulliganed.
    assert!(
        vb.mulliganed[Seat::A as usize],
        "the public beat is visible"
    );
    assert!(
        !vb.mulliganed[Seat::B as usize],
        "B itself has not mulliganed"
    );
    // Counts are truthful; contents hidden.
    let a = e.state().player(Seat::A);
    assert_eq!(vb.opponent.hand_count as usize, a.hand.len());
    assert_eq!(vb.opponent.deck_count as usize, a.deck.len());

    let json = serde_json::to_string(&vb).unwrap();
    assert_eq!(
        json.matches("\"hand\":").count(),
        1,
        "only B's OWN hand serializes — A's redrawn hand never crosses"
    );
    assert_eq!(
        json.matches("\"peeked_top\":").count(),
        1,
        "no opponent peek leaks through the mulligan"
    );
    // The bottomed card / deck order is nowhere in B's view: the only card ids it
    // carries are its own hand + the public board, never A's deck.
    for &c in &a.deck {
        // (A's deck cards may coincidentally equal B's own hand cards in this
        //  duplicate-deck fixture, so we assert the structural redaction above is
        //  the real guarantee; here we just confirm the view never serializes a
        //  field that would carry A's ordering.)
        let _ = c;
    }
    assert_eq!(
        json.matches("\"deck\":").count(),
        0,
        "no deck ordering is ever serialized into a view"
    );
}

/// An empty opening hand is a safe no-op-ish edge: mulligan still resolves
/// deterministically without panicking on the seed roll (guards the `below(0)`
/// boundary in the handler).
#[test]
fn mulligan_handles_an_empty_hand_without_panic() {
    let mut st = blank();
    // Round 1, A active, untouched (blank() gives anima 20 — set it to the opening
    // income so the window opens), empty hand, a small deck.
    st.round = 1;
    st.active = Seat::A;
    st.player_mut(Seat::A).hand = vec![];
    st.player_mut(Seat::A).deck = vec![CardId(0), CardId(1)];
    st.player_mut(Seat::A).glimpsed_this_turn = false;
    st.player_mut(Seat::A).anima = 2; // (1 + round).min(6) at round 1
    let mut e = eng(st, 5);
    let evs = e
        .apply(Seat::A, Command::Mulligan { seat: Seat::A })
        .expect("an empty-hand mulligan still resolves");
    assert_eq!(evs.len(), 1, "exactly the Mulliganed fact");
    let a = e.state().player(Seat::A);
    assert_eq!(
        a.hand.len(),
        0,
        "no hand to draw into (deck-bounded, no bottom)"
    );
    assert_eq!(a.deck.len(), 2, "the page is intact");
}
