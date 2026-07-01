//! Choice shapes — pending peeks, target picks, Parting doctrine,
//! Standing Orders.
use recollect_core::Engine;
use recollect_core::state::{Command, PendingChoice, Phase};
use recollect_core::types::{CardDef, CardId, CardKind, Reach, Resonance, Seat};
use recollect_core::view::view_for;

fn named(id: u16, name: &str, a: i16, d: i16, h: i16) -> CardDef {
    CardDef {
        id: CardId(id),
        name: name.into(),
        cost: 1,
        attack: a,
        defense: d,
        hp: h,
        reach: Reach::Cross,
        resonance: Resonance::Wonder,
        kind: CardKind::Spirit,
        ..Default::default()
    }
}

#[test]
fn glimpse_opens_a_pending_peek_redacted_from_the_opponent() {
    let cat = vec![
        named(0, "Plain Vanilla", 10, 0, 40),
        named(1, "Cloudling", 10, 0, 30),
    ];
    let deck: Vec<CardId> = (0..20)
        .map(|i| CardId(if i < 10 { 1 } else { 0 }))
        .collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let hi = e
        .state()
        .player(Seat::A)
        .hand
        .iter()
        .position(|c| c.0 == 1)
        .unwrap() as u8;
    let cmd = e.legal_commands(Seat::A).into_iter()
        .find(|c| matches!(c, Command::PlaySpirit { hand_index, engage: None, .. } if *hand_index == hi))
        .unwrap();
    e.apply(Seat::A, cmd).unwrap();
    assert!(matches!(
        e.state().phase,
        Phase::PendingChoice { seat: Seat::A, .. }
    ));
    let Some(PendingChoice::Peek { looked, .. }) = e.state().pending_choice.clone() else {
        panic!("a peek is pending");
    };
    assert_eq!(looked.len(), 2, "Cloudling glimpses two");
    // Redaction: the chooser sees it; the opponent never does.
    assert!(view_for(&e, Seat::A).you.pending.is_some());
    assert!(view_for(&e, Seat::B).you.pending.is_none());
    // Mid-choice, only Choose is legal — and EndTurn is refused.
    assert!(
        e.legal_commands(Seat::A)
            .iter()
            .all(|c| matches!(c, Command::Choose { .. }))
    );
    assert!(e.apply(Seat::A, Command::EndTurn).is_err());
    let hand_before = e.state().player(Seat::A).hand.len();
    let deck_before = e.state().player(Seat::A).deck.len();
    e.apply(Seat::A, Command::Choose { index: 0 }).unwrap();
    assert_eq!(
        e.state().player(Seat::A).hand.len(),
        hand_before + 1,
        "took one to hand"
    );
    assert_eq!(
        e.state().player(Seat::A).deck.len(),
        deck_before - 1,
        "the other bottomed"
    );
    assert!(
        matches!(e.state().phase, Phase::Acting),
        "the match resumes"
    );
}

#[test]
fn choice_opening_plays_cost_an_action() {
    // Regression: a Play that opens a choice (Cloudling's Glimpse) flips the
    // phase to PendingChoice before the action is charged — the action must still
    // be spent (two-actions-per-turn economy), and
    // exhausting the last action via a choice play must end the turn.
    let cat = recollect_core::cards::canon_catalog();
    let cloud = cat.iter().find(|c| c.name == "Cloudling").unwrap().id;
    let deck: Vec<CardId> = (0..20).map(|_| cloud).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    e.state_mut_for_test().player_a.anima = 9; // afford two Cloudling plays (cost 2)

    let play_and_resolve = |e: &mut Engine| {
        let cmd = e
            .legal_commands(Seat::A)
            .into_iter()
            .find(|c| matches!(c, Command::PlaySpirit { engage: None, .. }))
            .expect("a placement is legal");
        e.apply(Seat::A, cmd).unwrap();
        assert!(
            matches!(e.state().phase, Phase::PendingChoice { seat: Seat::A, .. }),
            "Cloudling opens a Glimpse"
        );
        e.apply(Seat::A, Command::Choose { index: 0 }).unwrap();
    };

    play_and_resolve(&mut e);
    assert!(
        matches!(e.state().phase, Phase::Acting),
        "the choice resolved and the turn continues — no action cap, got {:?}",
        e.state().phase
    );
    assert_eq!(
        e.state().active,
        Seat::A,
        "still A's turn — plays don't end it"
    );

    // A second choice-play also resolves cleanly; the turn ends only on an explicit EndTurn.
    play_and_resolve(&mut e);
    assert_eq!(
        e.state().active,
        Seat::A,
        "still A's turn after a second play"
    );
    e.apply(Seat::A, Command::EndTurn).unwrap();
    assert_eq!(e.state().active, Seat::B, "EndTurn passes to B");
}

#[test]
fn standing_orders_hold_a_spirit_out_of_interception_for_free() {
    let cat = vec![named(0, "Plain Vanilla", 10, 0, 40)];
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let cmd = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::PlaySpirit { engage: None, .. }))
        .unwrap();
    let tile = match cmd {
        Command::PlaySpirit { tile, .. } => tile,
        _ => 0,
    };
    e.apply(Seat::A, cmd).unwrap();
    e.apply(Seat::A, Command::SetOrders { tile, hold: true })
        .unwrap();
    assert!(
        e.state().spirit_at(tile).unwrap().holding,
        "Standing Orders holds the spirit"
    );
    assert_eq!(e.state().active, Seat::A, "and it never ends the turn");
}
