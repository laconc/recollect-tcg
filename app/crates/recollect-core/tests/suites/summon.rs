//! Kindred — callers manifest tokens on an adjacent empty tile, one at a
//! time, dissolving to NO impression, and fading when their caller leaves play.
// Shared test scaffolding; not every helper is exercised by every test file.
#![allow(dead_code)]
use recollect_core::Engine;
use recollect_core::state::{Command, Event};
use recollect_core::types::{CardDef, CardId, CardKind, Reach, Resonance, Seat, adjacent4};

fn spirit(id: u16, name: &str, summons: bool) -> CardDef {
    CardDef {
        id: CardId(id),
        name: name.into(),
        cost: 1,
        attack: 20,
        defense: 0,
        hp: 30,
        reach: Reach::Cross,
        resonance: Resonance::Wonder,
        kind: CardKind::Spirit,
        ..Default::default()
    }
    .tap(|_c| {
        let _ = summons;
    })
}
trait Tap {
    fn tap(self, f: impl FnOnce(&Self)) -> Self
    where
        Self: Sized,
    {
        f(&self);
        self
    }
}
impl Tap for CardDef {}

#[test]
fn a_caller_manifests_one_kindred_on_an_adjacent_tile() {
    // The caller's spec references "Hum" by name; the token must be in the catalog.
    let caller = CardDef {
        id: CardId(0),
        name: "Choirmother Lark".into(),
        cost: 1,
        attack: 20,
        defense: 0,
        hp: 30,
        reach: Reach::Cross,
        resonance: Resonance::Harmony,
        kind: CardKind::Spirit,
        ..Default::default()
    };
    let token = CardDef {
        id: CardId(1),
        name: "Hum".into(),
        cost: 0,
        attack: 10,
        defense: 0,
        hp: 20,
        reach: Reach::Cross,
        resonance: Resonance::Harmony,
        kind: CardKind::Kindred,
        ..Default::default()
    };
    let filler = CardDef {
        id: CardId(2),
        name: "Filler".into(),
        cost: 1,
        attack: 10,
        defense: 0,
        hp: 30,
        reach: Reach::Cross,
        resonance: Resonance::Wonder,
        kind: CardKind::Spirit,
        ..Default::default()
    };
    let cat = vec![caller, token, filler];
    let deck: Vec<CardId> = (0..20)
        .map(|i| CardId(if i < 10 { 0 } else { 2 }))
        .collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let hi = e
        .state()
        .player(Seat::A)
        .hand
        .iter()
        .position(|c| c.0 == 0)
        .unwrap() as u8;
    let cmd = e.legal_commands(Seat::A).into_iter()
        .find(|c| matches!(c, Command::PlaySpirit { hand_index, engage: None, .. } if *hand_index == hi))
        .unwrap();
    let caller_tile = match cmd {
        Command::PlaySpirit { tile, .. } => tile,
        _ => 0,
    };
    let evs = e.apply(Seat::A, cmd).unwrap();
    let manifested: Vec<_> = evs
        .iter()
        .filter_map(|ev| match ev {
            Event::SpiritManifested { tile, card, .. } => Some((*tile, *card)),
            _ => None,
        })
        .collect();
    assert_eq!(manifested.len(), 1, "exactly one Kindred");
    let (tok_tile, tok_card) = manifested[0];
    assert_eq!(tok_card, CardId(1), "it manifested Hum");
    assert!(
        adjacent4(caller_tile).any(|a| a == tok_tile),
        "on an adjacent tile"
    );
    assert!(
        e.state().board[tok_tile as usize]
            .spirit
            .as_ref()
            .unwrap()
            .is_token
    );

    // Playing the caller again does NOT mint a second while the first lives.
    e.apply(Seat::A, Command::EndTurn).unwrap();
    e.apply(Seat::B, Command::EndTurn).unwrap();
    if let Some(hi2) = e.state().player(Seat::A).hand.iter().position(|c| c.0 == 0) {
        let cmd2 = e.legal_commands(Seat::A).into_iter()
            .find(|c| matches!(c, Command::PlaySpirit { hand_index, engage: None, .. } if *hand_index == hi2 as u8));
        if let Some(cmd2) = cmd2 {
            let evs2 = e.apply(Seat::A, cmd2).unwrap();
            assert!(
                !evs2
                    .iter()
                    .any(|ev| matches!(ev, Event::SpiritManifested { .. })),
                "one Kindred at a time"
            );
        }
    }
}
