//! Action economy: no fixed action count (Plays/Calls are Anima-gated; the turn ends only on
//! EndTurn), and each Mobile spirit moves once per turn — never the turn it arrives (summoning
//! sickness). These guard the turn loop against any fixed 2-action / auto-end assumption.
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::state::{Command, Phase};
use recollect_core::test_support::put_spirit;
use recollect_core::types::{CardId, Seat};

fn id_of(name: &str) -> CardId {
    canon_catalog()
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.id)
        .unwrap_or_else(|| panic!("card not found: {name}"))
}

fn fresh() -> Engine {
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    Engine::new(7, cat, deck.clone(), deck).0
}

fn can_move(e: &Engine, from: u8) -> bool {
    e.legal_commands(Seat::A)
        .iter()
        .any(|c| matches!(c, Command::MoveSpirit { from: f, .. } if *f == from))
}

#[test]
fn the_turn_ends_only_on_end_turn() {
    // No fixed action count: Glimpse (like a Play) does not end the turn — only EndTurn
    // does. Glimpse now raises two choices (burn, then keep-or-bottom); resolving them
    // keeps the turn going. (A fresh match deals a 5-card hand and 15-card page, so both
    // the burn cost and the peek are payable.)
    let mut e = fresh();
    e.state_mut_for_test().player_a.anima = 20;
    e.apply(Seat::A, Command::Glimpse).unwrap();
    assert_eq!(e.state().active, Seat::A, "Glimpse does not end the turn");
    assert!(
        matches!(e.state().phase, Phase::PendingChoice { .. }),
        "Glimpse raised the burn choice"
    );
    e.apply(Seat::A, Command::Choose { index: 0 }).unwrap(); // BURN a hand card
    assert!(
        matches!(e.state().phase, Phase::PendingChoice { .. }),
        "the keep-or-bottom choice follows the burn"
    );
    e.apply(Seat::A, Command::Choose { index: 0 }).unwrap(); // KEEP
    assert!(
        matches!(e.state().phase, Phase::Acting),
        "back to Acting after the choices — no action cap to hit"
    );
    assert_eq!(e.state().active, Seat::A, "still A's turn");
    e.apply(Seat::A, Command::EndTurn).unwrap();
    assert_eq!(e.state().active, Seat::B, "only EndTurn passes the turn");
}

#[test]
fn one_move_per_mobile_spirit_per_turn() {
    // A Mobile spirit moves once; a second Move leaves the menu AND is rejected by decide (no
    // legal/decide split). A different spirit still moves.
    let mut e = fresh();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 6, id_of("Moth of Small Hours"), Seat::A); // Mobile
        put_spirit(st, 8, id_of("Moth of Small Hours"), Seat::A); // Mobile
        st.moved_this_turn.clear(); // both have stood since before this turn — free to move
    }
    assert!(
        can_move(&e, 6) && can_move(&e, 8),
        "both may move at turn start"
    );
    e.apply(
        Seat::A,
        Command::MoveSpirit {
            from: 6,
            to: 1,
            engage: None,
        },
    )
    .expect("6 steps to adjacent empty tile 1");
    assert!(
        !can_move(&e, 1),
        "having moved, it gets no second Move this turn"
    );
    assert!(can_move(&e, 8), "but a different spirit still may");
    assert!(
        e.apply(
            Seat::A,
            Command::MoveSpirit {
                from: 1,
                to: 6,
                engage: None,
            },
        )
        .is_err(),
        "a second move is rejected, not merely hidden from the menu"
    );
}

#[test]
fn a_just_placed_mobile_spirit_is_summoning_sick() {
    // A spirit cannot Move the turn it arrives.
    let mut e = fresh();
    {
        let st = e.state_mut_for_test();
        st.player_a.hand = vec![id_of("Moth of Small Hours")];
        st.player_a.anima = 9;
        st.player_a.first_placement_done = true;
    }
    let place = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::PlaySpirit { engage: None, .. }))
        .expect("the Moth is placeable");
    let tile = match place {
        Command::PlaySpirit { tile, .. } => tile,
        _ => unreachable!(),
    };
    e.apply(Seat::A, place).unwrap();
    assert!(
        !can_move(&e, tile),
        "a spirit just played onto the board is summoning-sick — no Move this turn"
    );
}

#[test]
fn summoning_sickness_clears_on_the_next_turn() {
    // The sickness is for the ARRIVAL turn only — once a full turn-cycle returns the seat to
    // play, the spirit moves freely. (If `moved_this_turn` failed to clear, spirits would be
    // permanently stranded where placed — they'd never reach the board's surviving centre.)
    let mut e = fresh();
    {
        let st = e.state_mut_for_test();
        st.player_a.hand = vec![id_of("Moth of Small Hours")];
        st.player_a.anima = 20;
        st.player_a.first_placement_done = true;
    }
    let place = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::PlaySpirit { engage: None, .. }))
        .expect("placeable");
    let tile = match place {
        Command::PlaySpirit { tile, .. } => tile,
        _ => unreachable!(),
    };
    e.apply(Seat::A, place).unwrap();
    assert!(!can_move(&e, tile), "sick on the arrival turn");
    e.apply(Seat::A, Command::EndTurn).unwrap();
    e.apply(Seat::B, Command::EndTurn).unwrap();
    assert_eq!(e.state().active, Seat::A, "cycle returned to A");
    assert!(
        can_move(&e, tile),
        "the spirit must move the turn AFTER it arrived — summoning sickness clears"
    );
}
