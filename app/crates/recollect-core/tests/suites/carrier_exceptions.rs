//! Routed RuleException carriers — behavioral proof at each rule's chokepoint.
//! (GlimpseLooksOneMore is proven in evolution_arrivals.rs alongside Zenith.)
use recollect_core::cards::canon_catalog;
use recollect_core::state::Command;
use recollect_core::test_support::put_spirit;
use recollect_core::types::{CardId, Seat};
use recollect_core::{Engine, Event, Reject};

fn id_of(name: &str) -> CardId {
    canon_catalog()
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.id)
        .unwrap_or_else(|| panic!("card not found: {name}"))
}

/// Rondel, the Joining — "Your Bonds cost 0" (BondsCostZero). A player with zero
/// anima can attach a Bond only when Rondel is on the board.
#[test]
fn rondel_makes_bonds_cost_zero() {
    fn attach(with_rondel: bool) -> Result<Vec<Event>, Reject> {
        let cat = canon_catalog();
        let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
        let (mut e, _) = Engine::new(1, cat, deck.clone(), deck);
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Cloudling"), Seat::A); // two adjacent own
            put_spirit(st, 12, id_of("Cloudling"), Seat::A); // standing spirits
            st.player_mut(Seat::A).hand = vec![id_of("Fellow Travelers")]; // Bond, cost 1
            st.player_mut(Seat::A).anima = 0; // no anima
            st.player_a.first_placement_done = true;
            if with_rondel {
                put_spirit(st, 7, id_of("Rondel, the Joining"), Seat::A);
            }
        }
        e.apply(
            Seat::A,
            Command::AttachBond {
                hand_index: 0,
                tile_a: 11,
                tile_b: 12,
            },
        )
    }

    assert!(
        matches!(attach(false), Err(Reject::NotEnoughAnima)),
        "without Rondel, 0 anima can't afford the Bond"
    );
    let evs = attach(true).expect("with Rondel the Bond costs 0 and attaches at 0 anima");
    assert!(
        !evs.iter().any(|ev| matches!(ev, Event::AnimaSpent { .. })),
        "a zero-cost Bond spends no anima (events: {evs:?})"
    );
}
