//! Same seed + same commands ⇒ same state + same draw count, forever.
//! And the family property: decide-time simulation ≡ evolve-time replay.
use crate::common::*;
use recollect_core::AggregateRules;
use recollect_core::state::Phase;
use recollect_core::{Engine, Seat};

#[test]
fn same_seed_same_commands_same_everything() {
    let mut e1 = new_match(7);
    let mut e2 = new_match(7);
    drive_first_legal(&mut e1, 300);
    drive_first_legal(&mut e2, 300);
    assert_eq!(e1.state(), e2.state());
    assert_eq!(
        e1.entropy_draws(),
        e2.entropy_draws(),
        "the journal-owned counter agrees"
    );
    assert_eq!(
        serde_json::to_string(e1.state()).unwrap(),
        serde_json::to_string(e2.state()).unwrap()
    );
}

#[test]
fn glimpse_is_deterministic_per_branch() {
    // Glimpse (§5): same seed + same burn + same keep-or-bottom choice ⇒ byte-
    // identical state, and the two keep/bottom branches diverge (keep leaves the
    // page; bottom rotates it + grants Anima). This pins the determinism invariant
    // across the two choices the rules-change introduces. Driven on a fixed hand +
    // deck so the outcome is inspectable.
    let setup = |idx: u8| {
        let mut st = blank();
        st.player_mut(Seat::A).hand = vec![recollect_core::types::CardId(3)];
        st.player_mut(Seat::A).deck = vec![
            recollect_core::types::CardId(9),
            recollect_core::types::CardId(0),
            recollect_core::types::CardId(5),
        ];
        let mut e = eng(st, 7);
        e.apply(Seat::A, recollect_core::Command::Glimpse).unwrap();
        // Step 1: burn the only hand card (a fixed, deterministic choice).
        e.apply(Seat::A, recollect_core::Command::Choose { index: 0 })
            .unwrap();
        // Step 2: keep (idx 0) or bottom (idx 1) — the branch under test.
        e.apply(Seat::A, recollect_core::Command::Choose { index: idx })
            .unwrap();
        e
    };
    // Same branch, twice ⇒ identical (state + entropy counter).
    for idx in [0u8, 1u8] {
        let (a, b) = (setup(idx), setup(idx));
        assert_eq!(
            serde_json::to_string(a.state()).unwrap(),
            serde_json::to_string(b.state()).unwrap(),
            "Glimpse branch {idx} is deterministic"
        );
        assert_eq!(a.entropy_draws(), b.entropy_draws());
    }
    // The branches differ — keep ≠ bottom in deck order AND Anima.
    let (keep, bottom) = (setup(0), setup(1));
    assert_ne!(
        keep.state().player(Seat::A).deck,
        bottom.state().player(Seat::A).deck,
        "keep leaves the top; bottom rotates it under"
    );
    assert_eq!(
        bottom.state().player(Seat::A).anima,
        keep.state().player(Seat::A).anima + 1,
        "only bottoming buys the +1 Anima"
    );
}

#[test]
fn different_seeds_diverge() {
    let mut e1 = new_match(7);
    let mut e2 = new_match(8);
    drive_first_legal(&mut e1, 60);
    drive_first_legal(&mut e2, 60);
    assert_ne!(e1.state(), e2.state(), "shuffles differ; tellings differ");
}

#[test]
fn decide_evolve_replay_equivalence() {
    // Everything decide simulated, evolve must reproduce from events alone.
    let mut e = new_match(11);
    let (snapshot0, _) = e.snapshot();
    let mut journal = Vec::new();
    let mut steps = 0;
    while steps < 250 && !matches!(e.state().phase, Phase::Finished { .. }) {
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
        "(snapshot₀, events) replays to the same Memory"
    );
}

#[test]
fn snapshot_resume_matches_uninterrupted_run() {
    let mut whole = new_match(23);
    let mut first_half = new_match(23);
    drive_first_legal(&mut first_half, 40);
    let (state, pos) = first_half.snapshot();
    let mut resumed = Engine::from_state(state, 23, pos, recollect_core::cards::test_catalog());
    drive_first_legal(&mut whole, 40);
    // continue both with the same policy
    loop {
        let done_w = matches!(whole.state().phase, Phase::Finished { .. });
        let done_r = matches!(resumed.state().phase, Phase::Finished { .. });
        assert_eq!(done_w, done_r);
        if done_w {
            break;
        }
        for e in [&mut whole, &mut resumed] {
            let seat = e.state().active;
            let cmd = e.legal_commands(seat).first().unwrap().clone();
            e.apply(seat, cmd).unwrap();
        }
    }
    assert_eq!(whole.state(), resumed.state());
    assert_eq!(whole.entropy_draws(), resumed.entropy_draws());
    let _ = Seat::A;
}

#[test]
fn from_state_shared_resumes_identically_and_shares_the_catalog() {
    // H3: the bot's depth-2 lookahead forks the engine once per legal move via
    // `from_state_shared` + `catalog_arc` to avoid deep-cloning all 407 cards.
    // The shared-catalog resume must be byte-for-byte identical to the owned-Vec
    // `from_state` resume, AND the catalog must be the SAME allocation (a refcount
    // bump, not a clone) — that's the whole point of the optimization.
    let mut by_value = new_match(31);
    let mut by_shared = new_match(31);
    drive_first_legal(&mut by_value, 20);
    drive_first_legal(&mut by_shared, 20);

    // The owned-Vec path (existing API).
    let (state_v, pos_v) = by_value.snapshot();
    let resumed_v = Engine::from_state(state_v, 31, pos_v, by_value.catalog_ref().to_vec());

    // The shared-Arc path (H3): hand the parent's catalog Arc straight over.
    let shared = by_shared.catalog_arc();
    let before = std::sync::Arc::strong_count(&shared);
    let (state_s, pos_s) = by_shared.snapshot();
    let resumed_s = Engine::from_state_shared(state_s, 31, pos_s, std::sync::Arc::clone(&shared));
    // The fork holds its own handle to the SAME allocation — strong count grew.
    assert!(
        std::sync::Arc::strong_count(&shared) > before,
        "from_state_shared shares the catalog (refcount bump), not a deep clone"
    );

    // Both reconstructions are identical engines.
    assert_eq!(resumed_v.state(), resumed_s.state());
    assert_eq!(resumed_v.entropy_draws(), resumed_s.entropy_draws());
    // And the shared engine sees the same catalog the parent did.
    assert_eq!(resumed_s.catalog_ref(), by_shared.catalog_ref());
}

#[test]
fn entropy_is_counter_addressable() {
    // at(seed, pos) is an exact O(1) seek.
    use recollect_core::rng::Rng;
    let mut walked = Rng::from_seed(99);
    for _ in 0..57 {
        walked.next_u64();
    }
    let mut seeked = Rng::at(99, 57);
    assert_eq!(walked, seeked);
    assert_eq!(walked.next_u64(), seeked.next_u64());
}

#[test]
fn failed_command_leaves_no_position_change() {
    // a rejected command leaves NOTHING observable.
    use recollect_core::Command;
    let mut e = new_match(31);
    drive_first_legal(&mut e, 9);
    let pos_before = e.entropy_draws();
    let state_before = e.state().clone();
    let seat = e.state().active;
    // Illegal: glimpsing twice, or acting out of turn — try both shapes.
    let _ = e.apply(seat.other(), Command::Glimpse).unwrap_err();
    let _ = e
        .apply(
            seat,
            Command::MoveSpirit {
                from: 0,
                to: 1,
                engage: None,
            },
        )
        .unwrap_err();
    assert_eq!(e.entropy_draws(), pos_before, "the stream did not move");
    assert_eq!(e.state(), &state_before, "the Memory did not move");
}

#[test]
fn seed_appears_in_no_state_and_no_event() {
    // the secret is not there. A client holding every
    // event and every view must not be able to precompute an Echo.
    let seed = 987_654_321u64;
    let mut e = recollect_core::Engine::new(
        seed,
        recollect_core::cards::test_catalog(),
        common_deck(),
        common_deck(),
    );
    let mut journal = e.1.clone();
    let needle = seed.to_string();
    let mut steps = 0;
    while steps < 120 && !matches!(e.0.state().phase, Phase::Finished { .. }) {
        let seat = e.0.state().active;
        let cmd = e.0.legal_commands(seat).first().unwrap().clone();
        journal.extend(e.0.apply(seat, cmd).unwrap());
        steps += 1;
    }
    assert!(
        !serde_json::to_string(e.0.state())
            .unwrap()
            .contains(&needle)
    );
    assert!(!serde_json::to_string(&journal).unwrap().contains(&needle));
}

fn common_deck() -> Vec<recollect_core::CardId> {
    deck20()
}
