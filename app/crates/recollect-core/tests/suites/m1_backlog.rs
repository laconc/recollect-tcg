//! The headline design-law assertions — the standing proof that the systems
//! specified in design.md and the card source (cards.toml / cards_design.md)
//! hold in the engine. We KEEP these (never delete — invariant #5): each is a
//! top-level contract over a whole mechanic. Detailed, per-mechanic coverage
//! lives in the feature suites cited on each test (strays.rs, lurk.rs, summon.rs,
//! fabrication_traps.rs, evolve.rs, solace.rs, rules.rs, effects_engine.rs).

use crate::common::{blank, drive_first_legal, eng, hand, new_match, put, strikes, t};
use recollect_core::Engine;
// There is no `solace` director module — the Solace's card effects live in
// effects.json + are tested in solace_effects.rs.
use recollect_core::state::{
    Command, Event, MatchResult, Phase, Stray, StrikeKind, Temperament, Terrain, TerrainKind,
};
use recollect_core::types::{CardDef, CardId, CardKind, Reach, Resonance, Seat, adjacent4};
use recollect_core::view::view_for;

#[test]
fn interactive_interception_choice() {
    // Standing Orders: the defender governs which of its spirits
    // intercept — a per-spirit Hold, a free action set on the defender's OWN turn,
    // applied during the attacker's. A Held zone stands down; a watching zone
    // bites the arrival. (Interception suite: rules.rs.)
    let intercept_strikes = |hold: bool| -> usize {
        let mut st = blank();
        // A Pale Stalker (Slant) for Seat B covers the arrival tile (2,2).
        put(&mut st, t(1, 1), 3, Seat::B, None);
        // An A impression neighbour makes (2,2) a legal placement.
        st.board[t(2, 1) as usize].impressions = vec![Seat::A];
        hand(&mut st, Seat::A, &[8]);
        let mut e = eng(st, 1);
        // The defender sets the standing order on its own turn (free action).
        e.apply(Seat::A, Command::EndTurn).unwrap(); // → B
        e.apply(
            Seat::B,
            Command::SetOrders {
                tile: t(1, 1),
                hold,
            },
        )
        .unwrap();
        e.apply(Seat::B, Command::EndTurn).unwrap(); // → A
        let evs = e
            .apply(
                Seat::A,
                Command::PlaySpirit {
                    hand_index: 0,
                    tile: t(2, 2),
                    engage: None,
                    chain_prefs: Vec::new(),
                },
            )
            .unwrap();
        strikes(&evs)
            .iter()
            .filter(|s| s.4 == StrikeKind::Interception)
            .count()
    };
    assert!(
        intercept_strikes(false) >= 1,
        "a watching zone answers the arrival"
    );
    assert_eq!(
        intercept_strikes(true),
        0,
        "a Held spirit stands down — the defender chose which zones bite"
    );
}

#[test]
fn interactive_chain_targeting() {
    // each Momentum link is the teller's choice. An ordered
    // chain-preference list rides the arrival command and is consumed
    // first-legal-each-link, OVERRIDING the engine's banishing-first heuristic —
    // deterministic, journal-clean. (Base coverage: effects_engine.rs.)
    fn cat() -> Vec<CardDef> {
        let arriver = CardDef {
            id: CardId(0),
            name: "Momentum Caller".into(),
            cost: 1,
            attack: 50,
            defense: 0,
            hp: 90,
            reach: Reach::Burst,
            resonance: Resonance::Wonder,
            kind: CardKind::Spirit,
            ..Default::default()
        };
        let body = CardDef {
            id: CardId(1),
            name: "Reed".into(),
            cost: 1,
            attack: 0,
            defense: 0,
            hp: 30,
            reach: Reach::Cross,
            resonance: Resonance::Wonder,
            kind: CardKind::Spirit,
            ..Default::default()
        };
        vec![arriver, body]
    }
    // The first Momentum link's target, for a given preference list.
    let first_chain_target = |prefs: Vec<u8>| -> Option<u8> {
        let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
        let (mut e, _) = Engine::new(7, cat(), deck.clone(), deck);
        let st = e.state_mut_for_test();
        // Engage 11; two chain candidates in Burst reach of 12: tile 13 (low-HP,
        // the heuristic's banishing-first pick) and tile 7 (a wall the chain can't fell).
        recollect_core::test_support::put_spirit(st, 11, CardId(1), Seat::B);
        recollect_core::test_support::put_spirit(st, 13, CardId(1), Seat::B);
        recollect_core::test_support::put_spirit(st, 7, CardId(1), Seat::B);
        st.board[11].spirit.as_mut().unwrap().hp = 10; // engage banishes → chain fires
        st.board[13].spirit.as_mut().unwrap().hp = 10; // banishable by the 50-atk chain
        st.board[7].spirit.as_mut().unwrap().hp = 400; // a wall: the chain cannot fell it
        st.board[7].spirit.as_mut().unwrap().hp_max = 400;
        st.board[17].impressions = vec![Seat::A]; // makes 12 a legal placement
        st.player_a.first_placement_done = true;
        let hi = e
            .state()
            .player(Seat::A)
            .hand
            .iter()
            .position(|c| c.0 == 0)
            .unwrap() as u8;
        let evs = e
            .apply(
                Seat::A,
                Command::PlaySpirit {
                    hand_index: hi,
                    tile: 12,
                    engage: Some(11),
                    chain_prefs: prefs,
                },
            )
            .expect("the Caller arrives engaging 11");
        evs.iter().find_map(|ev| match ev {
            Event::Struck {
                to_tile,
                kind: StrikeKind::Chain(_),
                ..
            } => Some(*to_tile),
            _ => None,
        })
    };
    // The heuristic alone takes the banishable low-HP target.
    assert_eq!(
        first_chain_target(Vec::new()),
        Some(13),
        "default: the banishing-first heuristic picks 13"
    );
    // A preference list steers the same link to the wall instead — the teller wins.
    assert_eq!(
        first_chain_target(vec![7]),
        Some(7),
        "the chain preference overrides the heuristic"
    );
}

#[test]
fn strays_surface_and_befriend() {
    // a Stray is seeded 1-in-7 from the match seed (telegraphed, not
    // an arrival); a surfaced Gentle Stray with an adjacent shared-Imprint ally is
    // courted and befriended. (Full law — Wary veil, Feral-at-Echo, Midnight, the
    // denial counterplay — in strays.rs.)
    let bloom = CardDef {
        id: CardId(0),
        name: "Bloom Ally".into(),
        cost: 1,
        attack: 10,
        defense: 0,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Harmony,
        kind: CardKind::Spirit,
        imprints: vec!["Bloom".into()],
        ..Default::default()
    };
    let lamb = CardDef {
        id: CardId(1),
        name: "Lost Lamb".into(),
        cost: 0,
        attack: 10,
        defense: 0,
        hp: 30,
        reach: Reach::Cross,
        resonance: Resonance::Neutral,
        kind: CardKind::Foundling,
        rarity: "G".into(),
        imprints: vec!["Bloom".into()],
        rules: "Gentle. follows the kind".into(),
        ..Default::default()
    };
    let cat = vec![bloom, lamb];
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();

    // 1-in-7 seeding, each stray-match telegraphed at construction.
    let mut stray_matches = 0;
    for seed in 0..700u64 {
        let (e, _) = Engine::new(seed, cat.clone(), deck.clone(), deck.clone());
        if e.state().stray_match {
            stray_matches += 1;
            assert!(
                e.state().stray_telegraph.is_some(),
                "a stray-match telegraphs its surfacing"
            );
        }
    }
    assert!(
        (60..160).contains(&stray_matches),
        "≈1-in-7 matches host a Stray: {stray_matches}/700"
    );

    // A surfaced Gentle Stray, courted by an adjacent Bloom ally, is befriended.
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck.clone());
    let st = e.state_mut_for_test();
    st.stray = Some(Stray {
        card: CardId(1),
        tile: 12,
        temperament: Temperament::Gentle,
        veiled: false,
        courtship: 0,
        courted_by: None,
        hp: 30,
        hp_max: 30,
    });
    recollect_core::test_support::put_spirit(st, 7, CardId(0), Seat::A); // adjacent Bloom
    e.apply(Seat::A, Command::EndTurn).unwrap(); // → B
    e.apply(Seat::B, Command::EndTurn).unwrap(); // → A turn-start: courtship runs
    assert!(e.state().stray.is_none(), "befriended off the wild slot");
    let sp = e.state().board[12]
        .spirit
        .as_ref()
        .expect("now an owned spirit");
    assert_eq!(
        (sp.owner, sp.card),
        (Seat::A, CardId(1)),
        "the kind keeps it"
    );
}

#[test]
fn lurk_reveal_is_arrival() {
    // a face-down Lurker is a Fabrication for all rules — hidden,
    // redacted from the opponent, projecting/intercepting nothing; Reveal steps it
    // into the light as its own arrival. (Full law in lurk.rs.)
    let vanilla = CardDef {
        id: CardId(0),
        name: "Vanilla".into(),
        cost: 1,
        attack: 20,
        defense: 0,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Wonder,
        kind: CardKind::Spirit,
        ..Default::default()
    };
    let stalker = CardDef {
        id: CardId(1),
        name: "Pale Stalker".into(),
        lurk: true,
        ..vanilla.clone()
    };
    let da: Vec<CardId> = (0..20)
        .map(|i| CardId(if i < 10 { 1 } else { 0 }))
        .collect();
    let db: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, vec![vanilla, stalker], da, db);
    let hi = e
        .state()
        .player(Seat::A)
        .hand
        .iter()
        .position(|c| c.0 == 1)
        .unwrap() as u8;
    let cmd = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::PlaySpirit { hand_index, engage: None, .. } if *hand_index == hi))
        .expect("a placement");
    let tile = match cmd {
        Command::PlaySpirit { tile, .. } => tile,
        _ => unreachable!(),
    };
    let evs = e.apply(Seat::A, cmd).unwrap();
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::SpiritPlayed {
                face_down: true,
                ..
            }
        )),
        "the Lurker enters face-down"
    );
    // Redacted from the opponent: name and numbers are withheld.
    let oview = view_for(&e, Seat::B);
    let theirs = oview.tiles[tile as usize]
        .spirit
        .as_ref()
        .expect("a lurker stands here");
    assert!(
        theirs.face_down && theirs.card == CardId(u16::MAX),
        "the unspoken keeps its name"
    );
    // Reveal steps it into the light.
    let evs = e
        .apply(Seat::A, Command::Reveal { tile, engage: None })
        .unwrap();
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritRevealed { tile: r } if *r == tile)),
        "reveal is its own arrival"
    );
    assert!(
        !e.state().board[tile as usize]
            .spirit
            .as_ref()
            .unwrap()
            .face_down,
        "now seen — it roots a zone again"
    );
}

#[test]
fn kindred_call_is_arrival() {
    // a Caller's Kindred is an arrival — it manifests on an adjacent
    // tile, one at a time, as a token that leaves NO impression. (Full law in
    // summon.rs.)
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
    let deck: Vec<CardId> = (0..20)
        .map(|i| CardId(if i < 10 { 0 } else { 2 }))
        .collect();
    let (mut e, _) = Engine::new(7, vec![caller, token, filler], deck.clone(), deck);
    let hi = e
        .state()
        .player(Seat::A)
        .hand
        .iter()
        .position(|c| c.0 == 0)
        .unwrap() as u8;
    let cmd = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::PlaySpirit { hand_index, engage: None, .. } if *hand_index == hi))
        .expect("a placement");
    let caller_tile = match cmd {
        Command::PlaySpirit { tile, .. } => tile,
        _ => unreachable!(),
    };
    let evs = e.apply(Seat::A, cmd).unwrap();
    let manifested: Vec<_> = evs
        .iter()
        .filter_map(|ev| match ev {
            Event::SpiritManifested { tile, card, .. } => Some((*tile, *card)),
            _ => None,
        })
        .collect();
    assert_eq!(manifested.len(), 1, "exactly one Kindred manifests");
    let (tok_tile, tok_card) = manifested[0];
    assert_eq!(tok_card, CardId(1), "the named Kindred (Hum)");
    assert!(
        adjacent4(caller_tile).any(|a| a == tok_tile),
        "on an adjacent tile (an arrival)"
    );
    assert!(
        e.state().board[tok_tile as usize]
            .spirit
            .as_ref()
            .unwrap()
            .is_token,
        "a token (it leaves no impression)"
    );
}

#[test]
fn fabrications_bluff_and_trap() {
    // a lie holds a little ground. A Fabrication is a face-down lie on
    // its own tile; an enemy that STEPS IN springs it — revealed, its clause
    // fires, the lie consumed — and never arrives on the tile. (Full trap/bluff
    // law in fabrication_traps.rs.)
    use recollect_core::cards::canon_catalog;
    let cat = canon_catalog();
    let ember = cat
        .iter()
        .find(|c| c.name == "Buried Ember" && c.kind == CardKind::Fabrication)
        .expect("a damage trap exists")
        .id;
    let mover = cat
        .iter()
        .find(|c| c.name == "Moth of Small Hours")
        .expect("a Mobile spirit")
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let st = e.state_mut_for_test();
    st.board[12].terrain = Some(Terrain {
        card: ember,
        owner: Seat::A,
        kind: TerrainKind::Fabrication,
        face_down: true,
    });
    recollect_core::test_support::put_spirit(st, 11, mover, Seat::B);
    st.board[11].spirit.as_mut().unwrap().hp = 40;
    st.active = Seat::B;
    st.active_slot = recollect_core::types::SeatSlot::B1;
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
        "and spent"
    );
    assert!(
        e.state().board[12].spirit.is_none(),
        "the engager did NOT arrive on the lie"
    );
    assert!(
        e.state().board[12].terrain.is_none(),
        "the sprung Fabrication is consumed"
    );
    assert!(
        e.state().board[11]
            .spirit
            .as_ref()
            .map(|s| s.hp < hp_before)
            .unwrap_or(true),
        "the engager took the trap where it stood"
    );
}

#[test]
fn evolution_rescues_fading() {
    // the last-full-round save. A Fading line-base evolves into its
    // form — arriving at full HP with Fading cleared. (Primal self-fuel, Fabled
    // donor, the shared-Imprint rule: evolve.rs.)
    let base = CardDef {
        id: CardId(0),
        name: "Cubling".into(),
        cost: 1,
        attack: 10,
        defense: 0,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Fury,
        kind: CardKind::Spirit,
        imprints: vec!["Beast".into()],
        evolves_to: vec!["Direclaw".into()],
        ..Default::default()
    };
    let primal = CardDef {
        id: CardId(1),
        name: "Direclaw".into(),
        cost: 0,
        attack: 60,
        defense: 10,
        hp: 50,
        reach: Reach::Cross,
        resonance: Resonance::Fury,
        kind: CardKind::Evolution,
        rarity: "Primal".into(),
        imprints: vec!["Beast".into()],
        evolves_from: Some("Cubling".into()),
        ..Default::default()
    };
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, vec![base, primal], deck.clone(), deck);
    let st = e.state_mut_for_test();
    recollect_core::test_support::put_spirit(st, 12, CardId(0), Seat::A);
    st.board[12].spirit.as_mut().unwrap().fading = true; // the base is on its last round
    st.player_a.hand = vec![CardId(1)]; // the Primal form, held in hand to play
    let evs = e
        .apply(
            Seat::A,
            Command::Evolve {
                tile: 12,
                form_hand: 0, // the Primal — self-fueled by its own Fading
                fuel: None,
                engage: None,
            },
        )
        .unwrap();
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritEvolved { to, .. } if *to == CardId(1))),
        "the base became its Primal form"
    );
    let sp = e.state().board[12]
        .spirit
        .as_ref()
        .expect("the form stands");
    assert_eq!(sp.card, CardId(1));
    assert!(!sp.fading, "Fading cleared — the save");
    assert_eq!(sp.hp, sp.hp_max, "a non-engaging form arrives at full HP");
}

// The Unwritten-advances-inward contract is carried by the played Unwriting card
// The Page Turns (Effect::ShiftUnwrittenInward), guarded by
// `the_page_turns_shifts_every_unwritten_inward` in solace_effects.rs.

#[test]
// `MatchAbandoned { seat }` is a
// system-issued forfeit — resolvable on either seat's turn (it precedes the
// turn-ownership check in `decide`), journaled as a distinct event (not a scored
// `MatchEnded`), and the present player wins. The transport layer gates WHO may
// issue it (`#[only_accepts(kind = "system")]`); the engine just resolves it.
fn match_abandonment_resolves_by_journaled_system_command() {
    let mut e = new_match(7);
    drive_first_legal(&mut e, 5); // mid-telling, possibly the other seat's turn
    let abandoner = Seat::A;
    let evs = e
        .apply(abandoner, Command::MatchAbandoned { seat: abandoner })
        .expect("the system forfeit resolves like any command");
    // Journaled as a distinct, replayable event (so a spectator sees HOW it ended).
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::MatchAbandoned { seat, .. } if *seat == abandoner)),
        "forfeit recorded as a MatchAbandoned event, not a scored MatchEnded: {evs:?}"
    );
    // The abandoner forfeits; the present player wins — by forfeit, not by score.
    assert!(
        matches!(
            e.state().phase,
            Phase::Finished {
                result: MatchResult::Win(Seat::B),
                ..
            }
        ),
        "abandoner loses, present player wins: {:?}",
        e.state().phase
    );
    // The telling is over: any further command is rejected.
    assert!(
        e.apply(Seat::B, Command::EndTurn).is_err(),
        "no command applies after a forfeit"
    );
}
