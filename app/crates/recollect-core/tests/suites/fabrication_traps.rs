//! Fabrication trap triggers. A Fabrication is a face-down lie
//! on its own tile; an enemy that STEPS INTO it springs the trap — the
//! Fabrication reveals, its OnReveal clause fires on the engager (Traps) or
//! the owner (Bluffs), and the spent lie is consumed.
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::effects::Keyword;
use recollect_core::engine::{combat_stats_for_test, keyword_active_for_test};
use recollect_core::state::{Command, Event, TerrainKind};
use recollect_core::types::{CardId, CardKind, Seat};

/// Put seat A's face-down Fabrication `fab` at `tile`, and a Mobile seat-B
/// spirit adjacent at `from`, then return the engine ready to spring.
fn trap_setup(fab_name: &str, tile: u8, from: u8) -> (Engine, CardId) {
    let cat = canon_catalog();
    let fab = cat
        .iter()
        .find(|c| c.name == fab_name && c.kind == CardKind::Fabrication)
        .expect("fabrication exists")
        .id;
    // A Mobile enemy body (Moth of Small Hours is Mobile in canon).
    let mover = cat
        .iter()
        .find(|c| c.name == "Moth of Small Hours")
        .expect("mobile spirit")
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let st = e.state_mut_for_test();
    // A's lie at `tile`.
    st.board[tile as usize].terrain = Some(recollect_core::state::Terrain {
        card: fab,
        owner: Seat::A,
        kind: TerrainKind::Fabrication,
        face_down: true,
    });
    // B's mover at `from` (adjacent), and it's B's turn.
    recollect_core::test_support::put_spirit(st, from, mover, Seat::B);
    st.board[from as usize].spirit.as_mut().unwrap().hp = 40;
    st.active = Seat::B;
    st.active_slot = recollect_core::types::SeatSlot::B1;
    (e, mover)
}

#[test]
fn a_damage_trap_punishes_the_spirit_that_steps_in() {
    // Buried Ember: "Trap — engager takes 20 damage". Tiles 12 (lie) & 11 (mover).
    let (mut e, _mover) = trap_setup("Buried Ember", 12, 11);
    let hp_before = e.state().board[11].spirit.as_ref().unwrap().hp;
    let evs = e
        .apply(
            Seat::B,
            Command::MoveSpirit {
                from: 11,
                to: 12,
                engage: None,
            },
        )
        .unwrap();
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::FabricationRevealed { tile: 12 })),
        "the lie is shown"
    );
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::FabricationSpent { tile: 12 })),
        "the lie is spent"
    );
    // The mover never took tile 12; it's punished where it stood.
    assert!(
        e.state().board[12].spirit.is_none(),
        "the engager did NOT arrive on the lie"
    );
    let after = e.state().board[11].spirit.as_ref().map(|s| s.hp);
    assert!(
        after.map(|h| h < hp_before).unwrap_or(true),
        "the engager took trap damage ({hp_before} -> {after:?})"
    );
    assert!(
        e.state().board[12].terrain.is_none(),
        "the sprung Fabrication is consumed"
    );
}

#[test]
fn a_push_trap_shoves_the_engager_back() {
    // Bottomless Puddle: "Trap — push engager back 1". Lie at 12=(2,2), mover at
    // 11=(1,2); the engager is shoved directly away from the lie → 10=(0,2).
    let (mut e, _mover) = trap_setup("Bottomless Puddle", 12, 11);
    let evs = e
        .apply(
            Seat::B,
            Command::MoveSpirit {
                from: 11,
                to: 12,
                engage: None,
            },
        )
        .unwrap();
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::FabricationRevealed { tile: 12 })),
        "the lie is shown"
    );
    assert!(
        e.state().board[11].spirit.is_none(),
        "the engager was pushed off tile 11"
    );
    assert!(
        e.state().board[10].spirit.is_some(),
        "the engager was shoved back to 10, away from the lie"
    );
    assert!(
        e.state().board[12].spirit.is_none() && e.state().board[12].terrain.is_none(),
        "the engager never arrived on the lie, and the sprung lie is consumed"
    );
}

#[test]
fn a_bounce_trap_returns_the_engager_to_its_owners_hand() {
    // The Unasked Question: "Trap — engager returns to owner's hand."
    let (mut e, mover) = trap_setup("The Unasked Question", 12, 11);
    let b_hand_before = e.state().player(Seat::B).hand.len();
    e.apply(
        Seat::B,
        Command::MoveSpirit {
            from: 11,
            to: 12,
            engage: None,
        },
    )
    .unwrap();
    assert!(
        e.state().board[11].spirit.is_none(),
        "the engager left the board"
    );
    assert!(
        e.state().board[12].spirit.is_none() && e.state().board[12].terrain.is_none(),
        "it never arrived on the lie, and the sprung lie is consumed"
    );
    assert_eq!(
        e.state().player(Seat::B).hand.len(),
        b_hand_before + 1,
        "the engager was returned to B's hand"
    );
    assert!(
        e.state().player(Seat::B).hand.contains(&mover),
        "specifically, the engaging spirit's own card"
    );
}

#[test]
fn a_debuff_trap_weakens_the_engager_this_round_and_next() {
    // Your Name, Misheard: "Trap — engager −20 Attack this round and next."
    let cat = canon_catalog();
    let (mut e, _mover) = trap_setup("Your Name, Misheard", 12, 11);
    let before = combat_stats_for_test(e.state(), &cat, 11).attack;
    let round = e.state().round;
    e.apply(
        Seat::B,
        Command::MoveSpirit {
            from: 11,
            to: 12,
            engage: None,
        },
    )
    .unwrap();
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, 11).attack,
        before - 20,
        "the engager is weakened by 20 Attack"
    );
    // A temp debuff that rides through next round (until_round = round + 1) — not
    // a permanent stat change.
    assert!(
        e.state()
            .temp_mods
            .iter()
            .any(|m| m.tile == 11 && m.attack == -20 && m.until_round == round + 1),
        "the debuff lasts this round AND next"
    );
}

#[test]
fn a_take_control_trap_steals_its_engager() {
    // The Perfect Lie: "Trap — take control of the engaging spirit." B's mover springs
    // A's lie and is taken under A's control (it stayed on its tile, like a damage trap).
    let (mut e, _mover) = trap_setup("The Perfect Lie", 12, 11);
    assert_eq!(e.state().board[11].spirit.as_ref().unwrap().owner, Seat::B);
    e.apply(
        Seat::B,
        Command::MoveSpirit {
            from: 11,
            to: 12,
            engage: None,
        },
    )
    .unwrap();
    assert_eq!(
        e.state().board[11].spirit.as_ref().unwrap().owner,
        Seat::A,
        "the engager is now controlled by the trap's owner"
    );
}

#[test]
fn a_trait_strip_trap_silences_its_engager() {
    // The Blank Page: "Trap — the engager loses all printed Traits and Keywords."
    // The Moth (Mobile) springs it and loses its printed Mobile.
    let cat = canon_catalog();
    let (mut e, _mover) = trap_setup("The Blank Page", 12, 11);
    assert!(
        keyword_active_for_test(e.state(), &cat, 11, Keyword::Mobile),
        "the Moth is Mobile before springing the lie"
    );
    e.apply(
        Seat::B,
        Command::MoveSpirit {
            from: 11,
            to: 12,
            engage: None,
        },
    )
    .unwrap();
    assert!(
        !keyword_active_for_test(e.state(), &cat, 11, Keyword::Mobile),
        "its printed Mobile was stripped"
    );
}

#[test]
fn a_bluff_rewards_its_owner_when_sprung() {
    // Spare Memory: "Bluff — revealed: gain 2 Anima" (owner = A).
    let (mut e, _mover) = trap_setup("Spare Memory", 12, 11);
    let a_anima_before = e.state().player(Seat::A).anima;
    let evs = e
        .apply(
            Seat::B,
            Command::MoveSpirit {
                from: 11,
                to: 12,
                engage: None,
            },
        )
        .unwrap();
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::FabricationRevealed { tile: 12 }))
    );
    // The owner (A) gained anima even though B sprang it.
    assert!(
        e.state().player(Seat::A).anima > a_anima_before,
        "the bluff rewarded its owner"
    );
}

#[test]
fn you_cannot_step_onto_your_own_terrain() {
    // A's own Fabrication blocks A's own mover (no self-springing).
    let cat = canon_catalog();
    let fab = cat
        .iter()
        .find(|c| c.kind == CardKind::Fabrication)
        .unwrap()
        .id;
    let mover = cat
        .iter()
        .find(|c| c.name == "Moth of Small Hours")
        .unwrap()
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let st = e.state_mut_for_test();
    st.board[12].terrain = Some(recollect_core::state::Terrain {
        card: fab,
        owner: Seat::A,
        kind: TerrainKind::Fabrication,
        face_down: true,
    });
    recollect_core::test_support::put_spirit(st, 11, mover, Seat::A);
    let r = e.apply(
        Seat::A,
        Command::MoveSpirit {
            from: 11,
            to: 12,
            engage: None,
        },
    );
    assert!(r.is_err(), "a teller does not step onto its own lie");
}

#[test]
fn a_spirit_can_spring_an_enemy_lie_from_reach_without_stepping_in() {
    // Counterplay: a standing enemy spirit strikes the lie from an adjacent
    // tile (in reach), springing it from range and staying put.
    let cat = canon_catalog();
    let fab = cat
        .iter()
        .find(|c| c.name == "Buried Ember" && c.kind == CardKind::Fabrication)
        .unwrap()
        .id;
    let striker = cat
        .iter()
        .find(|c| c.kind == CardKind::Spirit && !c.lurk)
        .unwrap()
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let st = e.state_mut_for_test();
    st.board[12].terrain = Some(recollect_core::state::Terrain {
        card: fab,
        owner: Seat::A,
        kind: TerrainKind::Fabrication,
        face_down: true,
    });
    recollect_core::test_support::put_spirit(st, 11, striker, Seat::B); // adjacent (Cross reach)
    st.active = Seat::B;
    st.active_slot = recollect_core::types::SeatSlot::B1;
    // The strike is offered, and springs the lie.
    let legal = e.legal_commands(Seat::B);
    assert!(
        legal
            .iter()
            .any(|c| matches!(c, Command::StrikeFabrication { tile: 12, .. })),
        "reach-engage against the lie is offered"
    );
    let evs = e
        .apply(Seat::B, Command::StrikeFabrication { from: 11, tile: 12 })
        .unwrap();
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::FabricationRevealed { tile: 12 }))
    );
    assert!(
        e.state().board[12].terrain.is_none(),
        "the lie is cleared from range"
    );
    // The striker stayed at 11 (it struck from a distance, did not move).
    assert!(
        e.state().board[11].spirit.is_some(),
        "the striker stays put"
    );
}

#[test]
fn sudden_clearing_reveals_a_chosen_facedown_fabrication() {
    // OnPlay/TargetSpirit/RevealFabrication: pick a face-down Fabrication; flip it up.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    let sudden = cat.iter().find(|c| c.name == "Sudden Clearing").unwrap().id;
    let buried = cat
        .iter()
        .find(|c| c.name == "Buried Ember" && c.kind == CardKind::Fabrication)
        .unwrap()
        .id;
    {
        let st = e.state_mut_for_test();
        st.board[12].terrain = Some(recollect_core::state::Terrain {
            card: buried,
            owner: Seat::B,
            kind: TerrainKind::Fabrication,
            face_down: true,
        });
        st.player_a.hand = vec![sudden];
        st.player_a.anima = 9;
    }
    assert!(
        e.state().board[12].terrain.as_ref().unwrap().face_down,
        "precondition: the lie is face-down"
    );
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Sudden Clearing is castable");
    e.apply(Seat::A, cast).unwrap();
    let ch = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::Choose { .. }))
        .expect("a reveal target is pending");
    e.apply(Seat::A, ch).unwrap();
    assert!(
        !e.state().board[12].terrain.as_ref().unwrap().face_down,
        "Sudden Clearing flipped the chosen Fabrication face-up"
    );
}

#[test]
fn nothing_really_is_a_harmless_bluff() {
    // OnReveal/Owner/NoEffect: springing it reveals the lie but does nothing to the
    // engager — the no-op IS the card.
    let (mut e, _mover) = trap_setup("Nothing, Really", 12, 11);
    let hp0 = e.state().board[11].spirit.as_ref().unwrap().hp;
    let evs = e
        .apply(
            Seat::B,
            Command::MoveSpirit {
                from: 11,
                to: 12,
                engage: None,
            },
        )
        .unwrap();
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::FabricationRevealed { tile: 12 })),
        "the bluff reveals when sprung"
    );
    assert_eq!(
        e.state().board[11].spirit.as_ref().map(|s| s.hp),
        Some(hp0),
        "the engager is unharmed"
    );
    assert!(
        e.state().board[11].spirit.is_some(),
        "and still stands (not bounced or taken)"
    );
}

#[test]
fn the_toll_taxes_the_springer_two_anima() {
    // OnReveal/Engager/AnimaDelta{-2}: the spirit that springs it pays its owner 2 Anima.
    let (mut e, _) = trap_setup("The Toll", 12, 11);
    e.state_mut_for_test().player_b.anima = 5;
    let before = e.state().player(Seat::B).anima;
    e.apply(
        Seat::B,
        Command::MoveSpirit {
            from: 11,
            to: 12,
            engage: None,
        },
    )
    .unwrap();
    assert_eq!(
        e.state().player(Seat::B).anima,
        before - 2,
        "The Toll drained 2 Anima from the springer"
    );
}

#[test]
fn paper_sentinel_is_a_harmless_bluff() {
    // OnReveal/Engager/NoEffect: a bluff — the springer's wasted strike IS the effect.
    let (mut e, _) = trap_setup("Paper Sentinel", 12, 11);
    let hp0 = e.state().board[11].spirit.as_ref().unwrap().hp;
    let evs = e
        .apply(
            Seat::B,
            Command::MoveSpirit {
                from: 11,
                to: 12,
                engage: None,
            },
        )
        .unwrap();
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::FabricationRevealed { tile: 12 })),
        "the bluff reveals when sprung"
    );
    assert_eq!(
        e.state().board[11].spirit.as_ref().map(|s| s.hp),
        Some(hp0),
        "the engager is unharmed (a harmless bluff)"
    );
}

#[test]
fn the_provocation_forces_the_springer_to_engage_again() {
    // OnReveal/Engager/ExtraEngage: the spirit that springs it is forced into one more
    // engage (−10) against the trap owner's strongest spirit in its reach.
    let (mut e, _) = trap_setup("The Provocation", 12, 11);
    let cloud = canon_catalog()
        .iter()
        .find(|c| c.name == "Cloudling")
        .unwrap()
        .id;
    recollect_core::test_support::put_spirit(e.state_mut_for_test(), 6, cloud, Seat::A);
    let evs = e
        .apply(
            Seat::B,
            Command::MoveSpirit {
                from: 11,
                to: 12,
                engage: None,
            },
        )
        .unwrap();
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::Struck {
                from_tile: 11,
                to_tile: 6,
                ..
            }
        )),
        "the sprung Provocation forced the engager at 11 to engage tile 6"
    );
}

#[test]
fn magistrate_of_masks_grows_when_a_fabrication_is_revealed() {
    // OnFabricationRevealed/SelfSpirit/StatDelta{+10/+10}: any Fabrication revealing anywhere
    // buffs every standing Magistrate.
    let cat = canon_catalog();
    let id = |n: &str| cat.iter().find(|c| c.name == n).unwrap().id;
    let deck: Vec<CardId> = (0..20).map(|_| id("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 0, id("Magistrate of Masks"), Seat::A);
        recollect_core::test_support::put_spirit(st, 11, id("Cloudling"), Seat::A);
        st.board[12].terrain = Some(recollect_core::state::Terrain {
            card: id("Paper Sentinel"),
            owner: Seat::B,
            kind: TerrainKind::Fabrication,
            face_down: true,
        });
    }
    let base = e.state().board[0].spirit.as_ref().unwrap().attack;
    e.apply(Seat::A, Command::StrikeFabrication { from: 11, tile: 12 })
        .unwrap();
    assert_eq!(
        e.state().board[0].spirit.as_ref().unwrap().attack,
        base + 10,
        "Magistrate grew when a Fabrication was revealed"
    );
}
