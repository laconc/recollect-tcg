//! Landmark terrain auras computed on read. The occupant half (OccupantHere) is
//! covered in spellbook.rs (High Ground); this pins The Trellis's
//! OccupantAndAdjacentAllies — "allies here AND adjacent +10 Defense" — whose
//! adjacent half is wired in `combat_stats`.
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::effects::Keyword;
use recollect_core::engine::{combat_stats_for_test, keyword_active_for_test};
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

fn engine() -> Engine {
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    Engine::new(7, cat.clone(), deck.clone(), deck).0
}

#[test]
fn trellis_buffs_allies_here_and_adjacent_only() {
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        // On the 5×5 board The Trellis sits on tile 11=(1,2); its neighbors are
        // 6, 10, 12, 16. Tile 24=(4,4) is far away.
        st.board[11].terrain = Some(Terrain {
            card: id_of("The Trellis"),
            owner: Seat::A,
            kind: TerrainKind::Landmark,
            face_down: false,
        });
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // ally occupant ("here")
        put_spirit(st, 16, id_of("Cloudling"), Seat::A); // ally adjacent
        put_spirit(st, 12, id_of("Cloudling"), Seat::B); // ENEMY adjacent
        put_spirit(st, 24, id_of("Cloudling"), Seat::A); // ally far away
    }
    let def_of = |t: u8| combat_stats_for_test(e.state(), &cat, t).defense;
    assert_eq!(def_of(11), 10, "ally occupant: +10 Defense");
    assert_eq!(def_of(16), 10, "adjacent ally: +10 Defense");
    assert_eq!(def_of(12), 0, "adjacent ENEMY: no buff (allies only)");
    assert_eq!(def_of(24), 0, "distant ally: no buff");
}

#[test]
fn crossroads_grants_mobile_to_its_occupant() {
    let cat = canon_catalog();
    let mobile_on = |with_terrain: bool| -> bool {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            if with_terrain {
                st.board[11].terrain = Some(Terrain {
                    card: id_of("Crossroads"),
                    owner: Seat::A,
                    kind: TerrainKind::Landmark,
                    face_down: false,
                });
            }
            put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        }
        keyword_active_for_test(e.state(), &cat, 11, Keyword::Mobile)
    };
    assert!(!mobile_on(false), "bare tile: Cloudling not Mobile");
    assert!(mobile_on(true), "Crossroads grants Mobile to its occupant");
}

#[test]
fn the_threshold_makes_its_occupant_unpushable() {
    // Static/OccupantHere/Restrict(BePushed): the spirit standing on The Threshold
    // cannot be shoved. The striker stands on the Threshold and springs B's push-trap;
    // the shove fails (honored in push_away via occupant_restricted).
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        st.board[11].terrain = Some(Terrain {
            card: id_of("The Threshold"),
            owner: Seat::A,
            kind: TerrainKind::Landmark,
            face_down: false,
        });
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // striker on the Threshold
        st.board[12].terrain = Some(Terrain {
            card: id_of("Bottomless Puddle"),
            owner: Seat::B,
            kind: TerrainKind::Fabrication,
            face_down: true,
        });
    }
    e.apply(Seat::A, Command::StrikeFabrication { from: 11, tile: 12 })
        .unwrap();
    assert!(
        e.state().board[11].spirit.is_some(),
        "The Threshold holds its occupant firm against the push-trap"
    );
}

#[test]
fn the_long_table_buffs_adjacent_allies_once_revealed() {
    // OnReveal/AdjacentAlliesAll/StatDelta{def:10}/WhilePresent — "becomes a Landmark:
    // adjacent allies +10 Defense." combat_stats reads it for the revealed terrain.
    let cat = canon_catalog();
    // Defense of an ally at `tile`, with The Long Table optionally on tile 11.
    let def_at = |tile: u8, present: bool, face_down: bool| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            if present {
                st.board[11].terrain = Some(Terrain {
                    card: id_of("The Long Table"),
                    owner: Seat::A,
                    kind: TerrainKind::Landmark,
                    face_down,
                });
            }
            put_spirit(st, tile, id_of("Cloudling"), Seat::A);
        }
        combat_stats_for_test(e.state(), &cat, tile).defense
    };
    let base = def_at(16, false, false); // no Table
    assert_eq!(
        def_at(16, true, false) - base,
        10,
        "adjacent ally (16) gains +10 Defense"
    );
    assert_eq!(
        def_at(24, true, false),
        base,
        "a far ally (24) is unaffected"
    );
    assert_eq!(
        def_at(16, true, true),
        base,
        "a still-face-down Table grants nothing"
    );
}

#[test]
fn common_ground_makes_bonds_touching_it_free() {
    // Static/OccupantHere/Exception(BondsFreeOnLandmark): a Bond costs 0 when an
    // endpoint stands on the owner's Common Ground (positional, not seat-wide).
    let anima_spent = |on_common_ground: bool| -> u8 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            if on_common_ground {
                st.board[11].terrain = Some(Terrain {
                    card: id_of("Common Ground"),
                    owner: Seat::A,
                    kind: TerrainKind::Landmark,
                    face_down: false,
                });
            }
            put_spirit(st, 11, id_of("Cloudling"), Seat::A); // bond endpoint (on the landmark)
            put_spirit(st, 12, id_of("Cloudling"), Seat::A); // adjacent endpoint
            st.player_a.hand = vec![id_of("Held Hands")];
            st.player_a.anima = 9;
        }
        let a0 = e.state().player(Seat::A).anima;
        e.apply(
            Seat::A,
            Command::AttachBond {
                hand_index: 0,
                tile_a: 11,
                tile_b: 12,
            },
        )
        .unwrap();
        a0 - e.state().player(Seat::A).anima
    };
    assert!(anima_spent(false) > 0, "a Bond normally costs anima");
    assert_eq!(
        anima_spent(true),
        0,
        "Common Ground: a Bond touching its occupant is free"
    );
}

#[test]
fn arbiter_imperishable_makes_your_spirits_unpushable() {
    // Static/AlliesAll/Restrict(BePushed): while Arbiter stands, the owner's spirits cannot
    // be shoved. The striker springs B's push-trap; with Arbiter it holds firm.
    let stayed = |arbiter: bool| -> bool {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Cloudling"), Seat::A); // the striker
            if arbiter {
                put_spirit(st, 0, id_of("Arbiter Imperishable"), Seat::A);
            }
            st.board[12].terrain = Some(Terrain {
                card: id_of("Bottomless Puddle"),
                owner: Seat::B,
                kind: TerrainKind::Fabrication,
                face_down: true,
            });
        }
        e.apply(Seat::A, Command::StrikeFabrication { from: 11, tile: 12 })
            .unwrap();
        e.state().board[11].spirit.is_some()
    };
    assert!(
        !stayed(false),
        "without Arbiter the push-trap shoves the striker off"
    );
    assert!(
        stayed(true),
        "Arbiter holds the owner's spirits firm against the push"
    );
}
