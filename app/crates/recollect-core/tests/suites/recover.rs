//! Recover: a fully-dissolved spirit returns to its owner's hand (The Returning).
//! A spirit that dissolves (leaving an impression) joins its owner's `dissolved` pool;
//! Recover opens a redacted choice over that pool.
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::state::{Command, PendingChoice};
use recollect_core::test_support::put_spirit;
use recollect_core::types::{CardId, Seat, SeatSlot};

fn id_of(name: &str) -> CardId {
    canon_catalog()
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.id)
        .unwrap_or_else(|| panic!("card not found: {name}"))
}

fn engine() -> Engine {
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    Engine::new(7, cat.clone(), deck.clone(), deck).0
}

#[test]
fn a_dissolved_spirit_joins_its_owners_recover_pool() {
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        st.active = Seat::A;
        st.active_slot = SeatSlot::A1;
        put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        st.board[11].spirit.as_mut().unwrap().fading = true; // dissolves at A's turn-END Fade
    }
    // The Fade phase is at turn-END now; force A's Fade to dissolve the fading spirit.
    e.force_fade_step_for_test(Seat::A);
    assert!(
        e.state()
            .dissolved
            .iter()
            .any(|(s, c)| *s == Seat::A && *c == id_of("Cloudling")),
        "the dissolved Cloudling was recorded for Seat A"
    );
}

#[test]
fn the_returning_pulls_a_dissolved_spirit_back_to_hand() {
    let mut e = engine();
    let elk = id_of("Aurora Elk");
    {
        let st = e.state_mut_for_test();
        st.dissolved.push((Seat::A, elk)); // A lost an Aurora Elk earlier
        st.player_a.hand = vec![id_of("The Returning")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("The Returning is castable");
    e.apply(Seat::A, cast).unwrap();
    let Some(PendingChoice::Recover { options, .. }) = e.state().pending_choice.clone() else {
        panic!("a recover choice is pending");
    };
    let idx = options
        .iter()
        .position(|&c| c == elk)
        .expect("the Aurora Elk is recoverable") as u8;
    e.apply(Seat::A, Command::Choose { index: idx }).unwrap();
    assert!(
        e.state().player(Seat::A).hand.contains(&elk),
        "the Aurora Elk returned to hand"
    );
    assert!(
        !e.state().dissolved.iter().any(|(_, c)| *c == elk),
        "and left the Recover pool"
    );
    assert!(
        e.state().pending_choice.is_none(),
        "the choice is fully resolved (no dangling pending_choice)"
    );
}
