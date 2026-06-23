//! Cost-and-momentum aura cluster: cost auras (placement + spend) and momentum auras.
use recollect_core::Engine;
use recollect_core::state::{Command, Event};
use recollect_core::types::{CardDef, CardId, CardKind, Reach, Resonance, Seat};

fn sp(id: u16, name: &str, cost: u8) -> CardDef {
    CardDef {
        id: CardId(id),
        name: name.into(),
        cost,
        attack: 20,
        defense: 0,
        hp: 30,
        reach: Reach::Cross,
        resonance: Resonance::Wonder,
        kind: CardKind::Spirit,
        ..Default::default()
    }
}

#[test]
fn the_lurking_courts_cost_aura_cheapens_your_spirits() {
    // The Lurking Court: while in play, your spirits cost 1 less (a Lurk
    // card; its CostDelta aura is the one under test). Pair with a cost-2 filler.
    let court = CardDef {
        lurk: true,
        ..sp(0, "The Lurking Court", 2)
    };
    let filler = sp(1, "Filler", 2);
    let cat = vec![court, filler];
    let deck: Vec<CardId> = (0..20)
        .map(|i| CardId(if i < 10 { 0 } else { 1 }))
        .collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    // Lay the Court (face-down), reveal it so its aura is live.
    let hi = e
        .state()
        .player(Seat::A)
        .hand
        .iter()
        .position(|c| c.0 == 0)
        .unwrap() as u8;
    let cmd = e.legal_commands(Seat::A).into_iter()
        .find(|c| matches!(c, Command::PlaySpirit { hand_index, engage: None, .. } if *hand_index == hi)).unwrap();
    let court_tile = match cmd {
        Command::PlaySpirit { tile, .. } => tile,
        _ => 0,
    };
    e.apply(Seat::A, cmd).unwrap(); // lay (action 1)
    let _ = e.apply(Seat::A, Command::EndTurn); // pass
    e.apply(Seat::B, Command::EndTurn).unwrap();
    // A reveals (action 1), leaving an action for the discounted placement.
    e.apply(
        Seat::A,
        Command::Reveal {
            tile: court_tile,
            engage: None,
        },
    )
    .unwrap();
    let anima_before = e.state().player(Seat::A).anima;
    // A cost-2 filler now costs 1 under the aura.
    let fhi = e
        .state()
        .player(Seat::A)
        .hand
        .iter()
        .position(|c| c.0 == 1)
        .unwrap() as u8;
    let place = e.legal_commands(Seat::A).into_iter()
        .find(|c| matches!(c, Command::PlaySpirit { hand_index, engage: None, .. } if *hand_index == fhi)).unwrap();
    let evs = e.apply(Seat::A, place).unwrap();
    let spent = evs
        .iter()
        .find_map(|ev| match ev {
            Event::AnimaSpent { amount, .. } => Some(*amount),
            _ => None,
        })
        .unwrap();
    assert_eq!(spent, 1, "cost 2 − aura 1 = 1");
    assert_eq!(e.state().player(Seat::A).anima, anima_before - 1);
}
