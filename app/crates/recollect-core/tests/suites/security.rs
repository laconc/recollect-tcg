//! Hostile-input invariants: `decide` is total over arbitrary commands.
//! A malicious client may send ANY Command; the engine must reject without
//! panicking, without mutating, and without consuming a single draw.
use crate::common::*;
use recollect_core::rng::Rng;
use recollect_core::{Command, Seat};

fn garbage(rng: &mut Rng) -> Command {
    let b = |r: &mut Rng| (r.next_u64() % 64) as u8; // deliberately out of range often
    match rng.next_u64() % 6 {
        0 => Command::PlaySpirit {
            hand_index: b(rng),
            tile: b(rng),
            engage: if rng.chance(1, 2) { Some(b(rng)) } else { None },
            chain_prefs: Vec::new(),
        },
        1 => Command::Overwrite {
            hand_index: b(rng),
            tile: b(rng),
        },
        2 => Command::MoveSpirit {
            from: b(rng),
            to: b(rng),
            engage: if rng.chance(1, 2) { Some(b(rng)) } else { None },
        },
        3 => Command::Glimpse,
        4 => Command::Release { hand_index: b(rng) },
        _ => Command::EndTurn,
    }
}

#[test]
fn hostile_commands_never_panic_never_mutate_never_draw() {
    for seed in 0..20u64 {
        let mut e = new_match(seed);
        let mut fuzz = Rng::from_seed(seed ^ 0xBAD);
        let mut applied = 0;
        for _ in 0..400 {
            let seat = if fuzz.chance(1, 2) { Seat::A } else { Seat::B };
            let cmd = garbage(&mut fuzz);
            let state_before = e.state().clone();
            let draws_before = e.entropy_draws();
            match e.apply(seat, cmd) {
                Ok(_) => applied += 1, // garbage occasionally lands legal; fine
                Err(_) => {
                    assert_eq!(e.state(), &state_before, "rejection mutated state");
                    assert_eq!(
                        e.entropy_draws(),
                        draws_before,
                        "rejection consumed entropy"
                    );
                }
            }
        }
        assert!(applied < 400, "not everything can be legal");
    }
}

#[test]
fn out_of_range_tiles_reject_cleanly_never_panic() {
    // Red-team regression (found by the gameplay fuzz, suites/fuzz.rs): these handlers indexed the
    // board before bounds-checking and PANICKED on a malformed tile. They must
    // return a Reject instead — a bad client must never crash the engine.
    use recollect_core::Seat;
    use recollect_core::state::Command::*;
    let cat = recollect_core::cards::canon_catalog();
    let deck: Vec<_> = cat
        .iter()
        .filter(|c| matches!(c.kind, recollect_core::types::CardKind::Spirit))
        .take(20)
        .map(|c| c.id)
        .collect();
    let (mut e, _) = recollect_core::Engine::new(1, cat, deck.clone(), deck);
    for cmd in [
        Evolve {
            tile: 99,
            form_hand: 9,
            fuel: Some(99),
            engage: Some(99),
        },
        Reveal {
            tile: 200,
            engage: Some(200),
        },
        StrikeFabrication { from: 99, tile: 99 },
    ] {
        let r = e.apply(Seat::A, cmd.clone());
        assert!(r.is_err(), "{cmd:?} should reject, not apply");
        // (The harness already proved it doesn't panic by reaching this line.)
    }
}
