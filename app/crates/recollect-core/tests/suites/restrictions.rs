//! This-round, seat-wide movement/push restrictions (Stand Ground): "your
//! spirits can't be pushed or moved this round." Recorded as `temp_restrict`,
//! honored at the move-legality and push call sites.
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::state::{Command, Terrain, TerrainKind};
use recollect_core::test_support::put_spirit;
use recollect_core::types::{CardId, Seat};

fn id_of(name: &str) -> CardId {
    canon_catalog()
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.id)
        .unwrap_or_else(|| panic!("card not found: {name}"))
}

fn engine_with_stand_ground() -> Engine {
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    Engine::new(7, cat.clone(), deck.clone(), deck).0
}

#[test]
fn stand_ground_forbids_your_spirits_moving_this_round() {
    let mut e = engine_with_stand_ground();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Moth of Small Hours"), Seat::A); // Mobile
        st.player_a.hand = vec![id_of("Stand Ground")];
        st.player_a.anima = 9;
    }
    let can_move = |e: &Engine| {
        e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::MoveSpirit { from: 12, .. }))
    };
    assert!(
        can_move(&e),
        "the Mobile spirit can move before Stand Ground"
    );
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Stand Ground is castable");
    e.apply(Seat::A, cast).unwrap();
    assert!(!can_move(&e), "no move is offered while Stand Ground holds");
    assert!(
        e.apply(
            Seat::A,
            Command::MoveSpirit {
                from: 12,
                to: 7,
                engage: None,
            },
        )
        .is_err(),
        "an explicit move is rejected"
    );
    let round = e.state().round;
    assert!(
        e.state()
            .temp_restrict
            .iter()
            .filter(|t| t.seat == Seat::A && t.until_round == round)
            .count()
            >= 2,
        "both BePushed and Move are recorded for this round"
    );
}

#[test]
fn stand_ground_makes_your_spirits_unpushable() {
    let mut e = engine_with_stand_ground();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // striker at (1,2)
        // B's push-trap at (2,2), in the striker's Cross reach.
        st.board[12].terrain = Some(Terrain {
            card: id_of("Bottomless Puddle"),
            owner: Seat::B,
            kind: TerrainKind::Fabrication,
            face_down: true,
        });
        st.player_a.hand = vec![id_of("Stand Ground")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .unwrap();
    e.apply(Seat::A, cast).unwrap();
    // Strike the lie from range; the sprung trap tries to shove the engager
    // (A's spirit at 11) — Stand Ground's BePushed restriction blocks it.
    e.apply(Seat::A, Command::StrikeFabrication { from: 11, tile: 12 })
        .unwrap();
    assert!(
        e.state().board[11].spirit.is_some(),
        "the striker was NOT pushed off tile 11 (Stand Ground holds it firm)"
    );
}
