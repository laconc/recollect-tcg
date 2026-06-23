//! Static, targeting-only Reach auras, widened on read by `targeting_reach`.
//! All test bodies use Cloudling (Cross reach) at tile 12 = (2,2) on the 5×5
//! board, facing for Seat A (forward = +y). Its base targeting reach is the four
//! orthogonal neighbours {7, 11, 13, 17}. "forward +1" adds (2,4)=22; "all
//! directions +1" additionally reaches sideways/backward tiles like (4,2)=14 and
//! (2,0)=2. The reach-aura errata (design doc) makes these TARGETING-only.
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::engine::targeting_reach_for_test;
use recollect_core::state::{Bond, Command, Terrain, TerrainKind};
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

const BASE: [u8; 4] = [7, 11, 13, 17];

fn landmark(name: &str) -> Vec<u8> {
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        st.board[12].terrain = Some(Terrain {
            card: id_of(name),
            owner: Seat::A,
            kind: TerrainKind::Landmark,
            face_down: false,
        });
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
    }
    targeting_reach_for_test(e.state(), &cat, 12, Seat::A)
}

fn bonded(name: &str) -> Vec<u8> {
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
        put_spirit(st, 13, id_of("Cloudling"), Seat::A);
        st.bonds.push(Bond {
            card: id_of(name),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 13,
        });
    }
    targeting_reach_for_test(e.state(), &cat, 12, Seat::A)
}

#[test]
fn lone_cross_spirit_has_unextended_reach() {
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
    }
    let r = targeting_reach_for_test(e.state(), &cat, 12, Seat::A);
    assert_eq!(r.len(), 4, "no aura → base Cross reach");
    for t in BASE {
        assert!(r.contains(&t));
    }
}

#[test]
fn the_overlook_extends_occupant_reach_all_directions() {
    let r = landmark("The Overlook");
    for t in BASE {
        assert!(r.contains(&t), "base reach preserved");
    }
    assert!(r.contains(&14), "all-directions reaches sideways (4,2)");
    assert!(r.contains(&2), "all-directions reaches even backward (2,0)");
}

#[test]
fn skylight_extends_occupant_reach_all_directions() {
    let r = landmark("Skylight");
    assert!(
        r.contains(&14) && r.contains(&2),
        "Skylight widens reach all directions"
    );
}

#[test]
fn shared_horizon_extends_bonded_pair_forward_only() {
    let r = bonded("Shared Horizon");
    assert!(r.contains(&22), "forward +1 reaches (2,4)");
    assert!(
        !r.contains(&2),
        "forward-only does NOT reach backward (2,0)"
    );
}

#[test]
fn stargazing_extends_bonded_pair_all_directions() {
    let r = bonded("Stargazing Together");
    assert!(r.contains(&2), "all-directions reaches backward (2,0)");
    assert!(r.contains(&14), "all-directions reaches sideways (4,2)");
}

#[test]
fn pathfinder_ibex_extends_adjacent_allies_forward() {
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
        put_spirit(st, 13, id_of("Pathfinder Ibex"), Seat::A); // adjacent ally
    }
    let r = targeting_reach_for_test(e.state(), &cat, 12, Seat::A);
    assert!(r.contains(&22), "Ibex grants adjacent allies forward +1");
    assert!(!r.contains(&2), "forward-only");
}

// ── this-round seat-wide reach buffs (temp_reach) ──────────────────────────

#[test]
fn tempestrider_roc_buffs_allied_reach_this_round() {
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // the ally we measure
        put_spirit(st, 7, id_of("Tempestrider Roc"), Seat::A);
    }
    let base = targeting_reach_for_test(e.state(), &cat, 12, Seat::A);
    // The Roc's OnPlay: allied Reach +1 forward this round (targeting only).
    e.fire_arrival_effects_for_test(7, Seat::A);
    let buffed = targeting_reach_for_test(e.state(), &cat, 12, Seat::A);
    assert!(buffed.contains(&22), "ally now reaches forward (2,4)");
    assert!(buffed.len() > base.len());
    // Scoped to this round (until_round == the current round), seat A.
    let round = e.state().round;
    assert!(
        e.state()
            .temp_reach
            .iter()
            .any(|t| t.seat == Seat::A && t.forward == 1 && t.until_round == round),
        "recorded as a this-round, seat-wide buff"
    );
}

#[test]
fn the_skys_whole_weight_buffs_allied_reach_this_round() {
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
        put_spirit(st, 7, id_of("The Sky's Whole Weight"), Seat::A);
    }
    e.fire_arrival_effects_for_test(7, Seat::A);
    assert!(
        targeting_reach_for_test(e.state(), &cat, 12, Seat::A).contains(&22),
        "allied Reach +1 forward this round"
    );
}

#[test]
fn hidden_vista_buffs_allied_reach_when_revealed() {
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        // A's bluff at 12=(2,2); A's ally at 11=(1,2); B's mover at 13=(3,2).
        st.board[12].terrain = Some(Terrain {
            card: id_of("Hidden Vista"),
            owner: Seat::A,
            kind: TerrainKind::Fabrication,
            face_down: true,
        });
        put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        put_spirit(st, 13, id_of("Moth of Small Hours"), Seat::B);
        st.active = Seat::B;
        st.active_slot = SeatSlot::B1;
    }
    let base = targeting_reach_for_test(e.state(), &cat, 11, Seat::A);
    e.apply(
        Seat::B,
        Command::MoveSpirit {
            from: 13,
            to: 12,
            engage: None,
        },
    )
    .unwrap();
    let buffed = targeting_reach_for_test(e.state(), &cat, 11, Seat::A);
    assert!(
        buffed.len() > base.len(),
        "Hidden Vista revealed: A's spirits' Reach +1 this round"
    );
}

#[test]
fn open_sky_full_buff_widens_targeting_and_projection() {
    // AlliesAll/ReachDelta{all_directions, targeting_only:false}: a FULL seat-wide
    // buff — widens engage targeting AND placement (projection), not just targeting.
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
        st.player_a.hand = vec![id_of("Open Sky")];
        st.player_a.anima = 9;
    }
    let reach_before = targeting_reach_for_test(e.state(), &cat, 12, Seat::A).len();
    let proj_before = recollect_core::engine::projection(e.state(), Seat::A, &cat)
        .iter()
        .filter(|&&b| b)
        .count();
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Open Sky is castable");
    e.apply(Seat::A, cast).unwrap();
    assert!(
        targeting_reach_for_test(e.state(), &cat, 12, Seat::A).len() > reach_before,
        "Open Sky widens engage targeting"
    );
    assert!(
        recollect_core::engine::projection(e.state(), Seat::A, &cat)
            .iter()
            .filter(|&&b| b)
            .count()
            > proj_before,
        "Open Sky widens placement (a FULL buff, not targeting-only)"
    );
}

#[test]
fn tailwind_buffs_only_the_chosen_spirit() {
    // TargetAllySpirit/ReachDelta full buff: one spirit's reach grows, not the other.
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // will be chosen
        put_spirit(st, 6, id_of("Cloudling"), Seat::A); // bystander
        st.player_a.hand = vec![id_of("Tailwind")];
        st.player_a.anima = 9;
    }
    let r12 = targeting_reach_for_test(e.state(), &cat, 12, Seat::A).len();
    let r6 = targeting_reach_for_test(e.state(), &cat, 6, Seat::A).len();
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Tailwind is castable");
    e.apply(Seat::A, cast).unwrap();
    // Choose tile 12 (find the Choose whose option is 12).
    let ch = {
        use recollect_core::state::PendingChoice;
        let Some(PendingChoice::Target { options, .. }) = e.state().pending_choice.clone() else {
            panic!("a target choice is pending");
        };
        let idx = options
            .iter()
            .position(|&t| t == 12)
            .expect("12 targetable") as u8;
        Command::Choose { index: idx }
    };
    e.apply(Seat::A, ch).unwrap();
    assert!(
        targeting_reach_for_test(e.state(), &cat, 12, Seat::A).len() > r12,
        "Tailwind widens the chosen spirit's reach"
    );
    assert_eq!(
        targeting_reach_for_test(e.state(), &cat, 6, Seat::A).len(),
        r6,
        "the bystander's reach is unchanged (per-spirit, not seat-wide)"
    );
}

#[test]
fn roc_paramount_grants_all_allies_forward_reach() {
    // Static/AlliesAll/ReachDelta{+1 forward, targeting-only}: a standing Roc widens EVERY
    // ally's targeting reach by 1 forward, board-wide (not just adjacent).
    let cat = canon_catalog();
    let reach = |roc: bool| -> Vec<u8> {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 12, id_of("Cloudling"), Seat::A);
            if roc {
                put_spirit(st, 0, id_of("Roc Paramount, Sky-Entire"), Seat::A); // not adjacent
            }
        }
        targeting_reach_for_test(e.state(), &cat, 12, Seat::A)
    };
    assert!(!reach(false).contains(&22), "no forward +1 without Roc");
    assert!(
        reach(true).contains(&22),
        "Roc grants the ally at 12 its forward +1 (tile 22), board-wide"
    );
}
