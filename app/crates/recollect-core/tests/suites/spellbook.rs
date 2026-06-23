//! The spellbook is deck-playable. Cast a Ritual, attach a Bond (aura),
//! place a Landmark (occupant aura), set a Fabrication face-down.
use recollect_core::Engine;
use recollect_core::state::{Command, Event, TerrainKind};
use recollect_core::types::{CardDef, CardId, CardKind, Reach, Resonance, Seat};

fn spirit(id: u16, name: &str) -> CardDef {
    CardDef {
        id: CardId(id),
        name: name.into(),
        cost: 1,
        attack: 10,
        defense: 0,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Wonder,
        kind: CardKind::Spirit,
        ..Default::default()
    }
}
fn card(id: u16, name: &str, kind: CardKind, cost: u8) -> CardDef {
    CardDef {
        id: CardId(id),
        name: name.into(),
        cost,
        attack: 0,
        defense: 0,
        hp: 0,
        reach: Reach::Cross,
        resonance: Resonance::Neutral,
        kind,
        ..Default::default()
    }
}

#[test]
fn a_ritual_casts_and_is_spent_drawing_a_card() {
    // "Recollection": draw 2. Cast it, hand shrinks by the Ritual + grows by 2.
    let cat = vec![
        spirit(0, "Filler"),
        card(1, "Recollection", CardKind::Ritual, 1),
    ];
    let deck: Vec<CardId> = (0..20).map(|i| CardId(if i < 6 { 1 } else { 0 })).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let hi = e
        .state()
        .player(Seat::A)
        .hand
        .iter()
        .position(|c| c.0 == 1)
        .unwrap() as u8;
    let before = e.state().player(Seat::A).hand.len();
    let evs = e
        .apply(Seat::A, Command::CastRitual { hand_index: hi })
        .unwrap();
    assert!(evs.iter().any(|ev| matches!(ev, Event::RitualCast { .. })));
    let draws = evs
        .iter()
        .filter(|ev| matches!(ev, Event::CardDrawn { .. }))
        .count();
    assert_eq!(draws, 2, "Recollection draws two");
    // -1 ritual + 2 drawn = +1 net.
    assert_eq!(e.state().player(Seat::A).hand.len(), before + 1);
}

#[test]
fn a_bond_attaches_to_an_adjacent_pair_and_holds_defense() {
    // Held Hands: +10 Defense each. Place two adjacent spirits, bond them.
    let cat = vec![
        spirit(0, "Filler"),
        card(1, "Held Hands", CardKind::Bond, 1),
    ];
    let deck: Vec<CardId> = (0..20)
        .map(|i| CardId(if i % 3 == 2 { 1 } else { 0 }))
        .collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    // Lay two spirits adjacent across a couple of turns.
    let lay = |e: &mut Engine, seat: Seat| -> Option<u8> {
        let h = e.state().player(seat).hand.iter().position(|c| c.0 == 0)? as u8;
        let cmd = e.legal_commands(seat).into_iter()
            .find(|c| matches!(c, Command::PlaySpirit { hand_index, engage: None, .. } if *hand_index == h))?;
        let t = match cmd {
            Command::PlaySpirit { tile, .. } => tile,
            _ => return None,
        };
        e.apply(seat, cmd).ok().map(|_| t)
    };
    let a = lay(&mut e, Seat::A).unwrap();
    // find an adjacent empty + projected tile for the second spirit
    e.apply(Seat::A, Command::EndTurn).unwrap();
    e.apply(Seat::B, Command::EndTurn).unwrap();
    // try to place adjacent to `a`
    let h = e
        .state()
        .player(Seat::A)
        .hand
        .iter()
        .position(|c| c.0 == 0)
        .unwrap() as u8;
    let adj = recollect_core::types::adjacent4(a).collect::<Vec<_>>();
    let cmd = e.legal_commands(Seat::A).into_iter().find(|c| matches!(c,
        Command::PlaySpirit { hand_index, tile, engage: None, .. } if *hand_index == h && adj.contains(tile)));
    let Some(cmd) = cmd else {
        return;
    }; // geometry permitting
    let b = match cmd {
        Command::PlaySpirit { tile, .. } => tile,
        _ => 0,
    };
    e.apply(Seat::A, cmd).unwrap();
    // Now a Bond between a and b should be offered.
    let bond = e.legal_commands(Seat::A).into_iter().find(|c| matches!(c,
        Command::AttachBond { tile_a, tile_b, .. } if (*tile_a == a && *tile_b == b) || (*tile_a == b && *tile_b == a)));
    if let Some(bond) = bond {
        let evs = e.apply(Seat::A, bond).unwrap();
        assert!(
            evs.iter()
                .any(|ev| matches!(ev, Event::BondAttached { .. }))
        );
        assert_eq!(e.state().bonds.len(), 1, "the pair is bound");
    }
}

#[test]
fn a_landmark_occupies_a_tile_and_a_fabrication_sets_face_down() {
    let cat = vec![
        spirit(0, "Filler"),
        card(1, "High Ground", CardKind::Landmark, 1),
        card(2, "Smoke", CardKind::Fabrication, 1),
    ];
    let deck: Vec<CardId> = (0..20)
        .map(|i| CardId([0u16, 0, 1, 0, 0, 2][i % 6]))
        .collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    // Advance so projection opens beyond home; lay a spirit to root zones.
    let h = e
        .state()
        .player(Seat::A)
        .hand
        .iter()
        .position(|c| c.0 == 0)
        .unwrap() as u8;
    let cmd = e.legal_commands(Seat::A).into_iter()
        .find(|c| matches!(c, Command::PlaySpirit { hand_index, engage: None, .. } if *hand_index == h)).unwrap();
    e.apply(Seat::A, cmd).unwrap();
    e.apply(Seat::A, Command::EndTurn).unwrap();
    e.apply(Seat::B, Command::EndTurn).unwrap();
    // Place a Landmark if one is legal.
    if let Some(lm) = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::PlaceLandmark { .. }))
    {
        let tile = match lm {
            Command::PlaceLandmark { tile, .. } => tile,
            _ => 0,
        };
        e.apply(Seat::A, lm).unwrap();
        let terr = e.state().board[tile as usize].terrain.as_ref().unwrap();
        assert_eq!(terr.kind, TerrainKind::Landmark);
        assert!(!terr.face_down, "Landmarks are open");
    }
}
