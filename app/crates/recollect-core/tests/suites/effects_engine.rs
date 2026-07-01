//! Instants: effects fire from real play, through real events.

use recollect_core::Engine;
use recollect_core::state::{Command, Event};
use recollect_core::types::{CardDef, CardId, CardKind, Reach, Resonance, Seat};

fn named(id: u16, name: &str, a: i16, d: i16, h: i16) -> CardDef {
    named_r(id, name, a, d, h, Reach::Cross)
}

fn named_r(id: u16, name: &str, a: i16, d: i16, h: i16, reach: Reach) -> CardDef {
    CardDef {
        id: CardId(id),
        name: name.into(),
        cost: 1,
        attack: a,
        defense: d,
        hp: h,
        reach,
        resonance: Resonance::Wonder,
        kind: CardKind::Spirit,
        ..Default::default()
    }
}

fn mini() -> (Vec<CardDef>, Vec<CardId>, Vec<CardId>) {
    let cat = vec![
        named(0, "Plain Vanilla", 10, 0, 40),
        named(1, "Tidebound Colossus", 10, 10, 50),
        named(2, "Bullroarer Immense", 30, 0, 40),
        named(3, "Wake-Walker", 10, 10, 40),
        named(4, "Frenzy Kit", 10, 0, 40),
        named(5, "Wisp of Doubt", 10, 0, 40),
        named(6, "Anchorite Ox", 10, 0, 20),
        named_r(7, "Plain Burst", 10, 0, 40, Reach::Burst),
    ];
    let deck = |ids: [u16; 5]| ids.iter().cycle().take(20).map(|i| CardId(*i)).collect();
    (cat, deck([0, 1, 2, 4, 6]), deck([7, 7, 7, 7, 7]))
}

/// Play `want` somewhere legal (engaging `hit` if given, at `at` if possible).
fn play(
    e: &mut Engine,
    seat: Seat,
    want: u16,
    hit: Option<u8>,
    at: Option<u8>,
) -> (u8, Vec<Event>) {
    let hi = e
        .state()
        .player(seat)
        .hand
        .iter()
        .position(|c| c.0 == want)
        .expect("card in hand") as u8;
    let fits = |c: &Command| {
        matches!(c,
        Command::PlaySpirit { hand_index, engage, .. } if *hand_index == hi && *engage == hit)
    };
    let all = e.legal_commands(seat);
    let cmd = all
        .iter()
        .find(|c| fits(c) && matches!(c, Command::PlaySpirit { tile, .. } if Some(*tile) == at))
        .or_else(|| all.iter().find(|c| fits(c)))
        .unwrap_or_else(|| {
            let occ: Vec<(usize, bool)> = e
                .state()
                .board
                .iter()
                .enumerate()
                .filter_map(|(i, t)| t.spirit.as_ref().map(|s| (i, s.fading)))
                .collect();
            panic!(
                "no legal placement: seat={seat:?} want={want} hit={hit:?} occ={occ:?} sample={:?}",
                all.iter().take(4).collect::<Vec<_>>()
            )
        })
        .clone();
    let tile = match cmd {
        Command::PlaySpirit { tile, .. } => tile,
        _ => unreachable!(),
    };
    (tile, e.apply(seat, cmd).expect("plays"))
}

/// Both seats' first placements are home-row bound; this prelude marches B
/// into reach and wounds A's vanilla at tile 7. Returns the ally tile.
fn wound_prelude(e: &mut Engine) -> u8 {
    let (ally, _) = play(e, Seat::A, 0, None, Some(7));
    e.apply(Seat::A, Command::EndTurn).unwrap();
    play(e, Seat::B, 7, None, Some(17));
    e.apply(Seat::B, Command::EndTurn).unwrap();
    e.apply(Seat::A, Command::EndTurn).unwrap();
    play(e, Seat::B, 7, Some(ally), None);
    e.apply(Seat::B, Command::EndTurn).unwrap();
    ally
}

#[test]
fn tidebound_restores_a_wounded_ally_through_real_events() {
    let (cat, da, db) = mini();
    let (mut e, _) = Engine::new(7, cat, da, db);
    let ally = wound_prelude(&mut e);
    let hp_before = e.state().spirit_at(ally).unwrap().hp;
    assert!(hp_before < 40, "ally is wounded");
    let (_, evs) = play(&mut e, Seat::A, 1, None, None); // Tidebound arrives
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::EffectRestored { tile, amount: 20 } if *tile == ally))
    );
    assert_eq!(
        e.state().spirit_at(ally).unwrap().hp,
        (hp_before + 20).min(40)
    );
}

#[test]
fn bullroarer_damages_all_others_and_lethal_effect_damage_banishes() {
    let (cat, da, db) = mini();
    let (mut e, _) = Engine::new(7, cat, da, db);
    let _ally = wound_prelude(&mut e); // both sides wounded
    let (_, evs) = play(&mut e, Seat::A, 2, None, None); // Bullroarer: 10 to ALL others
    let hits = evs
        .iter()
        .filter(|ev| matches!(ev, Event::EffectDamaged { amount: 10, .. }))
        .count();
    assert_eq!(hits, 3, "every other spirit struck by the roar");
    // determinism canary: effects draw no entropy
    let (cat2, da2, db2) = mini();
    let (mut e2, _) = Engine::new(7, cat2, da2, db2);
    wound_prelude(&mut e2);
    let (_, evs2) = play(&mut e2, Seat::A, 2, None, None);
    assert_eq!(format!("{evs:?}"), format!("{evs2:?}"));
}

#[test]
fn events_roundtrip_on_the_wire_and_schema_is_prerelease_v1() {
    // Pre-release (owner decision): the schema moves freely at version 1;
    // the Amendment 1 whole-enum ceremony begins at first release.
    let ev = Event::EffectTempStat {
        tile: 3,
        attack: 10,
        defense: 0,
        until_round: 4,
    };
    let bytes = postcard::to_allocvec(&ev).unwrap();
    assert_eq!(postcard::from_bytes::<Event>(&bytes).unwrap(), ev);
    assert_eq!(recollect_core::effects::EVENTS_SCHEMA_VERSION, 1);
}

/// Deck whose opening hand is guaranteed to hold `lead`.
fn lead_deck(lead: u16) -> Vec<CardId> {
    (0..20)
        .map(|i| CardId(if i < 10 { lead } else { 0 }))
        .collect()
}

/// End the active seat's turn, releasing first if the hand cap demands it.
fn pass(e: &mut Engine) {
    let seat = e.state().active;
    if matches!(
        e.state().phase,
        recollect_core::state::Phase::PendingRelease { .. }
    ) {
        e.apply(seat, Command::Release { hand_index: 0 }).unwrap();
    }
    e.apply(seat, Command::EndTurn).unwrap();
}

/// March B into reach of A's spirit at tile 7 and strike it once.
fn march_and_strike(e: &mut Engine, a_card: u16) -> (u8, Vec<Event>) {
    let (target, _) = play(e, Seat::A, a_card, None, Some(7));
    e.apply(Seat::A, Command::EndTurn).unwrap();
    play(e, Seat::B, 7, None, Some(17));
    e.apply(Seat::B, Command::EndTurn).unwrap();
    e.apply(Seat::A, Command::EndTurn).unwrap();
    let (_, evs) = play(e, Seat::B, 7, Some(target), None);
    (target, evs)
}

#[test]
fn frenzy_aura_sharpens_retaliation_while_damaged() {
    let (cat, _, db) = mini();
    let (mut e, _) = Engine::new(7, cat, lead_deck(4), db);
    let (kit, _first) = march_and_strike(&mut e, 4); // Frenzy Kit, def 0
    // First strike: Kit undamaged when retaliating? Retaliation resolves
    // after the wound lands in this engine — find both retaliations.
    // An exchange computes both strikes from the pre-exchange
    // snapshot (simultaneous memory) — Frenzy sharpens the NEXT exchange.
    e.apply(Seat::B, Command::EndTurn).unwrap();
    e.apply(Seat::A, Command::EndTurn).unwrap();
    assert!(
        e.state().spirit_at(kit).unwrap().hp < 40,
        "kit carries its wound"
    );
    let (_, second) = play(&mut e, Seat::B, 7, Some(kit), None);
    let d2 = second
        .iter()
        .find_map(|ev| match ev {
            Event::Struck {
                from_tile,
                damage,
                kind,
                ..
            } if *from_tile == kit
                && matches!(kind, recollect_core::state::StrikeKind::Retaliation) =>
            {
                Some(*damage)
            }
            _ => None,
        })
        .expect("kit retaliates");
    assert_eq!(d2, 20, "damaged Kit: 10 printed + 10 Frenzy");
}

#[test]
fn wisp_parting_hushes_adjacent_enemies_for_one_round_only() {
    let (cat, _, db) = mini();
    let (mut e, _) = Engine::new(7, cat, lead_deck(5), db);
    let (wisp, _) = march_and_strike(&mut e, 5); // B wounds the Wisp (10/40)
    // Finish it: B strikes again next B turn.
    e.apply(Seat::B, Command::EndTurn).unwrap();
    e.apply(Seat::A, Command::EndTurn).unwrap();
    let mut hush: Option<Vec<Event>> = None;
    for _ in 0..6 {
        play(&mut e, Seat::B, 7, Some(wisp), None);
        let fading = e.state().spirit_at(wisp).map(|s| s.fading).unwrap_or(false);
        pass(&mut e); // end B's turn
        if fading {
            // The standing-Faded window: the banished Wisp (A's spirit) does
            // NOT dissolve at the start of A's turn. It lingers standing-Faded into
            // A's Main and dissolves at A's turn-END, where its Parting fires and
            // hushes the adjacent (B) enemy. Capture A's EndTurn events.
            assert!(
                e.state().spirit_at(wisp).map(|s| s.fading).unwrap_or(false),
                "the banished Wisp lingers standing-Faded into A's Main (D1)"
            );
            let a = e.state().active; // A's turn now
            if matches!(
                e.state().phase,
                recollect_core::state::Phase::PendingRelease { .. }
            ) {
                e.apply(a, Command::Release { hand_index: 0 }).unwrap();
            }
            hush = Some(e.apply(a, Command::EndTurn).unwrap());
            break;
        }
        pass(&mut e); // A passes — march on
    }
    let hush = hush.expect("the wisp dissolved");
    assert!(
        hush.iter()
            .any(|ev| matches!(ev, Event::EffectTempStat { attack: -10, .. })),
        "Parting hushed an adjacent enemy for the round"
    );
    assert!(
        !e.state().temp_mods.is_empty(),
        "the hush is held this round"
    );
    let until = e.state().temp_mods[0].until_round;
    // March the clock past the round: the hush lifts.
    while e.state().round <= until {
        pass(&mut e);
    }
    assert!(
        e.state().temp_mods.is_empty(),
        "ThisRound expired with the round"
    );
}

#[test]
fn unyielding_survives_once_and_the_banishment_is_never_journaled() {
    let (cat, _, db) = mini();
    let (mut e, _) = Engine::new(7, cat, lead_deck(6), db);
    let (ox, evs) = march_and_strike(&mut e, 6); // 10 dmg → 10 HP, undramatic
    assert!(
        !evs.iter()
            .any(|ev| matches!(ev, Event::ReplacementSurvived { .. }))
    );
    e.apply(Seat::B, Command::EndTurn).unwrap();
    e.apply(Seat::A, Command::EndTurn).unwrap();
    let (_, evs2) = play(&mut e, Seat::B, 7, Some(ox), None); // lethal: 10 dmg at 10 HP
    assert!(
        evs2.iter()
            .any(|ev| matches!(ev, Event::ReplacementSurvived { tile, form: 10 } if *tile == ox)),
        "the Ox stands"
    );
    assert!(
        !evs2
            .iter()
            .any(|ev| matches!(ev, Event::SpiritBecameFading { tile, .. } if *tile == ox)),
        "R-D1-3: the replaced banishment was never journaled"
    );
    assert_eq!(e.state().spirit_at(ox).unwrap().hp, 10);
    e.apply(Seat::B, Command::EndTurn).unwrap();
    e.apply(Seat::A, Command::EndTurn).unwrap();
    let (_, evs3) = play(&mut e, Seat::B, 7, Some(ox), None); // once means once
    assert!(
        evs3.iter()
            .any(|ev| matches!(ev, Event::SpiritBecameFading { tile, .. } if *tile == ox)),
        "the second match holds"
    );
}

#[test]
fn d13_chain_preference_steers_the_momentum_link() {
    use recollect_core::Engine;
    use recollect_core::state::{Command, Event};
    use recollect_core::types::{CardDef, CardId, CardKind, Reach, Resonance, Seat};
    // A high-attack arrival that defeats its first target, then chains. Two
    // chainable enemies are in reach; the preference list names the SECOND one
    // first, so the chain should go there even though the heuristic might pick
    // the other (both equally banishable).
    let attacker = CardDef {
        id: CardId(0),
        name: "Chainer".into(),
        cost: 1,
        attack: 90,
        defense: 0,
        hp: 60,
        reach: Reach::Wide,
        resonance: Resonance::Fury,
        kind: CardKind::Spirit,
        relentless: true,
        ..Default::default()
    };
    let weak = CardDef {
        id: CardId(1),
        name: "Weakling".into(),
        cost: 1,
        attack: 0,
        defense: 0,
        hp: 10,
        reach: Reach::Cross,
        resonance: Resonance::Wonder,
        kind: CardKind::Spirit,
        ..Default::default()
    };
    let cat = vec![attacker, weak];
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let st = e.state_mut_for_test();
    // Put two enemy weaklings in the attacker's Wide reach from tile 12.
    recollect_core::test_support::put_spirit(st, 11, CardId(1), Seat::B);
    recollect_core::test_support::put_spirit(st, 13, CardId(1), Seat::B);
    st.board[11].spirit.as_mut().unwrap().hp = 10;
    st.board[13].spirit.as_mut().unwrap().hp = 10;
    // Give A a Chainer in hand at a known index.
    let hi = e
        .state()
        .player(Seat::A)
        .hand
        .iter()
        .position(|c| c.0 == 0)
        .unwrap_or(0) as u8;
    // Arrive at 12 engaging 11; chain preference says 13 next.
    // Ensure tile 12 is in A's projection (place a friendly impression neighbor) and
    // that 11 is a legal engage at placement.
    e.state_mut_for_test().board[7].impressions = vec![Seat::A]; // 7 is adjacent to 12
    e.state_mut_for_test().player_a.first_placement_done = true;
    let r = e.apply(
        Seat::A,
        Command::PlaySpirit {
            hand_index: hi,
            tile: 12,
            engage: Some(11),
            chain_prefs: vec![13],
        },
    );
    let evs = r.expect("Chainer arrives engaging 11");
    // The chain struck tile 13 (the preferred target).
    let chained_13 = evs.iter().any(|ev| {
        matches!(
            ev,
            Event::Struck {
                to_tile: 13,
                kind: recollect_core::state::StrikeKind::Chain(_),
                ..
            }
        )
    });
    assert!(
        chained_13,
        "the momentum chain followed the preference to tile 13"
    );
}

#[test]
fn adjacent_allies_all_instant_heal_actually_lands() {
    // Regression (found by tests/card_effects_fire.rs): AdjacentAlliesAll was
    // not a supported instant selector, so on-arrival heals using it (Rillsong
    // Tadpole, Picnic Blanket) silently no-op'd. A restore must reach a fading
    // adjacent ally and heal it.
    use recollect_core::Engine;
    use recollect_core::types::{CardId, CardKind, Seat};
    let cat = recollect_core::cards::canon_catalog();
    let tad = cat
        .iter()
        .find(|c| c.name == "Rillsong Tadpole")
        .expect("Tadpole exists")
        .id;
    let filler = cat
        .iter()
        .find(|c| c.kind == CardKind::Spirit && c.id != tad)
        .unwrap()
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(1, cat, deck.clone(), deck);
    let st = e.state_mut_for_test();
    recollect_core::test_support::put_spirit(st, 12, tad, Seat::A);
    recollect_core::test_support::put_spirit(st, 13, filler, Seat::A);
    if let Some(sp) = st.board[13].spirit.as_mut() {
        sp.fading = true;
        sp.hp = 10;
        sp.hp_max = 40;
    }
    let evs = e.fire_arrival_effects_for_test(12, Seat::A);
    assert!(
        !evs.is_empty(),
        "the on-arrival heal must fire (not silently no-op)"
    );
    assert_eq!(
        e.state().board[13].spirit.as_ref().unwrap().hp,
        20,
        "the fading adjacent ally was healed by 10"
    );
}

#[test]
fn adjacent_enemies_all_hits_exactly_the_orthogonal_enemies() {
    // Mutation-killer (effects_exec.rs `effect_targets`, the AdjacentEnemiesAll arm:
    // the `owner != owner && |Δx|+|Δy| == 1` predicate). The Patient Knife's OnReveal
    // damages every ORTHOGONALLY adjacent enemy by 10 — and nothing else. On the 5×5
    // board the Knife sits at 12=(2,2); enemies ring it at the four orthogonal tiles
    // {7,11,13,17} and at one DIAGONAL tile 6=(1,1); a friendly sits at orthogonal 8?…
    // no — 8 is not adjacent to 12. We place a friendly at orthogonal 17's mirror by
    // putting an ALLY at 7 instead, to also pin the owner predicate. Layout:
    //   12 = Patient Knife (Seat A)
    //   enemies (Seat B): 11, 13, 17   — orthogonal → must take 10
    //   enemy   (Seat B): 6            — DIAGONAL  → must take 0 (==1 gate)
    //   ally    (Seat A): 7            — orthogonal but FRIENDLY → must take 0 (owner gate)
    // A `+`→`-`/`*`/`/` flip in the manhattan sum, a `==1`→`!=1`, or an owner `!=`→`==`
    // each changes this exact set, so any of them flips a 10↔0 and fails the asserts.
    use recollect_core::types::CardId;
    let cat = recollect_core::cards::canon_catalog();
    let knife = cat
        .iter()
        .find(|c| c.name == "The Patient Knife")
        .unwrap()
        .id;
    let filler = cat
        .iter()
        .find(|c| c.kind == CardKind::Spirit && c.id != knife)
        .unwrap()
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(1, cat, deck.clone(), deck);
    let st = e.state_mut_for_test();
    recollect_core::test_support::put_spirit(st, 12, knife, Seat::A);
    for t in [11u8, 13, 17, 6] {
        recollect_core::test_support::put_spirit(st, t, filler, Seat::B);
    }
    recollect_core::test_support::put_spirit(st, 7, filler, Seat::A); // friendly, orthogonal
    let hp_before: Vec<i16> = [6u8, 7, 11, 13, 17]
        .iter()
        .map(|&t| st.board[t as usize].spirit.as_ref().unwrap().hp)
        .collect();
    let _ = e.fire_arrival_effects_for_test(12, Seat::A);
    let dmg = |t: u8, before: i16| before - e.state().board[t as usize].spirit.as_ref().unwrap().hp;
    assert_eq!(dmg(11, hp_before[2]), 10, "orthogonal enemy @11 took 10");
    assert_eq!(dmg(13, hp_before[3]), 10, "orthogonal enemy @13 took 10");
    assert_eq!(dmg(17, hp_before[4]), 10, "orthogonal enemy @17 took 10");
    assert_eq!(
        dmg(6, hp_before[0]),
        0,
        "DIAGONAL enemy @6 untouched (==1 gate)"
    );
    assert_eq!(
        dmg(7, hp_before[1]),
        0,
        "friendly @7 untouched (owner gate)"
    );
}

#[test]
fn adjacent_allies_all_heals_exactly_the_orthogonal_allies() {
    // Mutation-killer (effects_exec.rs `effect_targets`, the AdjacentAlliesAll arm:
    // `owner == owner && Some(i) != source && |Δx|+|Δy| == 1`). Rillsong Tadpole's
    // OnPlay restores 10 to every ORTHOGONALLY adjacent ally — not itself, not a
    // diagonal ally, not an adjacent enemy. Tadpole@12; fading allies at orthogonal
    // {11,13} (must heal), a fading ally at DIAGONAL 8?… 8 isn't adjacent — use 16
    // (diagonal, must NOT heal), and a fading ENEMY at orthogonal 17 (must NOT heal).
    use recollect_core::types::CardId;
    let cat = recollect_core::cards::canon_catalog();
    let tad = cat
        .iter()
        .find(|c| c.name == "Rillsong Tadpole")
        .unwrap()
        .id;
    let filler = cat
        .iter()
        .find(|c| c.kind == CardKind::Spirit && c.id != tad)
        .unwrap()
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(1, cat, deck.clone(), deck);
    let st = e.state_mut_for_test();
    recollect_core::test_support::put_spirit(st, 12, tad, Seat::A);
    // Allies (heal candidates) and the exclusions, all fading at hp 10 / max 40.
    let wound = |st: &mut recollect_core::state::GameState, t: u8| {
        let sp = st.board[t as usize].spirit.as_mut().unwrap();
        sp.fading = true;
        sp.hp = 10;
        sp.hp_max = 40;
    };
    recollect_core::test_support::put_spirit(st, 11, filler, Seat::A); // orthogonal ally → heal
    recollect_core::test_support::put_spirit(st, 13, filler, Seat::A); // orthogonal ally → heal
    recollect_core::test_support::put_spirit(st, 16, filler, Seat::A); // DIAGONAL ally → no heal
    recollect_core::test_support::put_spirit(st, 17, filler, Seat::B); // orthogonal ENEMY → no heal
    for t in [11u8, 13, 16, 17] {
        wound(st, t);
    }
    let _ = e.fire_arrival_effects_for_test(12, Seat::A);
    let hp = |t: u8| e.state().board[t as usize].spirit.as_ref().unwrap().hp;
    assert_eq!(hp(11), 20, "orthogonal ally @11 healed by 10");
    assert_eq!(hp(13), 20, "orthogonal ally @13 healed by 10");
    assert_eq!(hp(16), 10, "DIAGONAL ally @16 NOT healed (==1 gate)");
    assert_eq!(hp(17), 10, "orthogonal ENEMY @17 NOT healed (owner gate)");
}
