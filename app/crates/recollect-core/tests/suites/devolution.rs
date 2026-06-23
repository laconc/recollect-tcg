//! Devolution (design §5) — **recede a banished form to a base**, the rescue. A
//! Primal/Fabled form banished in combat enters its standing-Faded window (§0.5); its
//! owner may play a **base card from hand** onto it during Main, before the turn-end
//! dissolve. The base arrives at FULL HP, fade cleared (rescued one tier down), is
//! **summoning-sick** until the owner's next turn, and costs **half the banished form's
//! Anima rounded down**. Devolution **is an arrival, symmetric with evolution** (the
//! maintainer's ruling): it fires the same arrival triggers a form's evolution fires —
//! `check_throughline` (a base receding into a standing 3-line re-completes on the spot)
//! and a queued next-arrival buff — but **engages no one** (no strike target) and fires
//! **no OnPlay**, and the base stays summoning-sick (no free Mobile step that turn). A
//! spirit may cycle evolve↔devolve without limit. These tests pin every
//! constraint, the full evolve↔devolve cycle, redaction, and determinism. The
//! Lorekeeper *reverts*; the Solace *recedes* (one engine action, the faction's verb).
use crate::common::{blank, eng};
use recollect_core::Engine;
use recollect_core::state::{Command, Event};
use recollect_core::types::{CardDef, CardId, CardKind, Reach, Resonance, Seat, SeatSlot};

/// A line: base "Cub" (cost 4), Primal "Direwolf" (cost 6 ⇒ devolve = 3), Fabled
/// "Mythwolf" (cost 8 ⇒ devolve = 4), plus a donor (for the Fabled leap) and a prey
/// (an arrival strike target — to prove devolution does NOT strike). Real costs so the
/// ½-cost charge is exercised.
fn cat() -> Vec<CardDef> {
    let base = CardDef {
        id: CardId(0),
        name: "Cub".into(),
        cost: 4,
        attack: 10,
        defense: 0,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Fury,
        kind: CardKind::Spirit,
        imprints: vec!["Beast".into()],
        evolves_to: vec!["Direwolf".into(), "Mythwolf".into()],
        ..Default::default()
    };
    let primal = CardDef {
        id: CardId(1),
        name: "Direwolf".into(),
        cost: 6,
        attack: 60,
        defense: 10,
        hp: 60,
        reach: Reach::Cross,
        resonance: Resonance::Fury,
        kind: CardKind::Evolution,
        rarity: "Primal".into(),
        imprints: vec!["Beast".into()],
        evolves_from: Some("Cub".into()),
        ..Default::default()
    };
    let fabled = CardDef {
        id: CardId(2),
        name: "Mythwolf".into(),
        cost: 8,
        attack: 80,
        defense: 30,
        hp: 80,
        reach: Reach::Cross,
        resonance: Resonance::Fury,
        kind: CardKind::Evolution,
        rarity: "Fabled".into(),
        imprints: vec!["Beast".into()],
        evolves_from: Some("Cub".into()),
        ..Default::default()
    };
    let donor = CardDef {
        id: CardId(3),
        name: "Packmate".into(),
        cost: 1,
        attack: 20,
        defense: 10,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Fury,
        kind: CardKind::Spirit,
        imprints: vec!["Beast".into()],
        ..Default::default()
    };
    let prey = CardDef {
        id: CardId(4),
        name: "Prey".into(),
        cost: 1,
        attack: 5,
        defense: 0,
        hp: 20,
        reach: Reach::Cross,
        resonance: Resonance::Wonder,
        kind: CardKind::Spirit,
        ..Default::default()
    };
    vec![base, primal, fabled, donor, prey]
}

/// Seat a STANDING-FADED form (CardId `form`) for A at `tile`: banished by B, in its
/// §0.5 window (`fading`, `fade_deadline` Some). A holds the base (CardId 0) and ample
/// Anima. It is A's Main (so devolution is reachable). The form is wounded-then-faded so
/// "full HP on the base" is a real observation.
fn faded_form(form: u16, seed: u64) -> (Engine, u8) {
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(seed, cat(), deck.clone(), deck);
    let tile = 12u8;
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, tile, CardId(form), Seat::A);
        {
            let sp = st.board[tile as usize].spirit.as_mut().unwrap();
            sp.hp = 5; // wounded — proving the base arrives at FULL HP, not this value
            sp.fading = true;
            sp.banished_by = Some(Seat::B);
            sp.fade_deadline = Some(st.round + 1); // banished on B's turn, owner A ⇒ round+1
        }
        st.active = Seat::A;
        st.active_slot = SeatSlot::A1;
        st.player_a.anima = 20;
        st.player_a.hand = vec![CardId(0)]; // the Cub base
        st.player_a.deck.clear();
        st.player_b.deck.clear();
        st.moved_this_turn.clear();
    }
    (e, tile)
}

#[test]
fn devolve_recedes_a_faded_primal_to_its_base_full_hp_fade_cleared_summoning_sick() {
    let (mut e, tile) = faded_form(1, 7); // a faded Direwolf (Primal)
    // The Devolve is OFFERED on the standing-Faded form (the base card is in hand).
    assert!(
        e.legal_commands(Seat::A).iter().any(|c| matches!(
            c,
            Command::Devolve { tile: t, base_hand: 0 } if *t == tile
        )),
        "a standing-Faded Primal is offered the recede (Devolve) onto its base"
    );
    let anima0 = e.state().player_a.anima;
    let evs = e
        .apply(Seat::A, Command::Devolve { tile, base_hand: 0 })
        .expect("the recede resolves");
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritDevolved { from, to, .. } if *from == CardId(1) && *to == CardId(0))),
        "the Primal receded to its Cub base (a distinct SpiritDevolved event)"
    );
    let sp = e.state().board[tile as usize].spirit.as_ref().unwrap();
    assert_eq!(sp.card, CardId(0), "now the Cub base");
    assert_eq!(sp.hp, 40, "FULL HP — rescued (not the 5 it faded at)");
    assert_eq!(sp.hp_max, 40);
    assert!(!sp.fading, "the fade is cleared");
    assert_eq!(sp.fade_deadline, None, "out of the standing-Faded window");
    // Summoning-sick: the rescued base cannot move or evolve until next turn.
    assert!(
        e.state().moved_this_turn.contains(&tile),
        "the rescued base is summoning-sick (moved_this_turn)"
    );
    // ½-cost rounded down: Direwolf cost 6 ⇒ 3.
    assert_eq!(anima0 - e.state().player_a.anima, 3, "cost = ⌊6/2⌋ = 3");
    // The base card left the hand (it was the played card); no impression on the tile.
    assert!(
        e.state().player_a.hand.is_empty(),
        "the base card was consumed"
    );
    assert!(
        e.state().board[tile as usize].impressions.is_empty(),
        "devolution lays no impression — the form BECAME a base"
    );
}

#[test]
fn devolve_cost_is_half_the_banished_form_rounded_down_fabled_path() {
    // The Fabled "Mythwolf" cost 8 ⇒ devolve = ⌊8/2⌋ = 4 (not the base's cost).
    let (mut e, tile) = faded_form(2, 7); // a faded Mythwolf (Fabled)
    let anima0 = e.state().player_a.anima;
    e.apply(Seat::A, Command::Devolve { tile, base_hand: 0 })
        .expect("the Fabled recede resolves");
    assert_eq!(anima0 - e.state().player_a.anima, 4, "cost = ⌊8/2⌋ = 4");
    assert_eq!(
        e.state().board[tile as usize].spirit.as_ref().unwrap().card,
        CardId(0),
        "a faded Fabled also recedes straight to its base (the line is 2-stage)"
    );
}

#[test]
fn devolve_engages_no_one_no_onplay_no_free_mobile() {
    // Devolution is an arrival (symmetric with evolution), but it carries NO strike target —
    // so it engages no one and fires no OnPlay. We seat a prey in the base's reach; after the
    // recede the prey is untouched. (The faded form here stands alone — no flanking line — so
    // no Throughline completes either; the line-completion arrival trigger is pinned in
    // redteam_rules_change.rs.)
    let (mut e, tile) = faded_form(1, 7);
    recollect_core::test_support::put_spirit(e.state_mut_for_test(), 7, CardId(4), Seat::B);
    let prey_hp0 = e.state().board[7].spirit.as_ref().unwrap().hp;
    let evs = e
        .apply(Seat::A, Command::Devolve { tile, base_hand: 0 })
        .unwrap();
    // No combat events at all (Struck / interception / momentum) and no OnPlay effects — the
    // recede never strikes (no engage target).
    assert!(
        !evs.iter().any(|ev| matches!(
            ev,
            Event::Struck { .. } | Event::EffectDamaged { .. } | Event::SpiritBecameFading { .. }
        )),
        "devolution fires no combat — it engages no one ({evs:?})"
    );
    assert_eq!(
        e.state().board[7].spirit.as_ref().unwrap().hp,
        prey_hp0,
        "the prey in reach is untouched — the recede never strikes"
    );
    // The base is summoning-sick (no free Mobile step on arrival) — proven by the
    // moved_this_turn mark, which blocks any move/evolve this turn.
    assert!(
        e.state().moved_this_turn.contains(&tile),
        "the rescued base took no free Mobile step (it is summoning-sick)"
    );
}

#[test]
fn devolve_only_a_standing_faded_form_window_only() {
    // A HEALTHY form (not fading) cannot be receded — devolution is the rescue of a
    // BANISHED form in its window, not a free downgrade.
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat(), deck.clone(), deck);
    let tile = 12u8;
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, tile, CardId(1), Seat::A); // a healthy Direwolf
        st.active = Seat::A;
        st.active_slot = SeatSlot::A1;
        st.player_a.anima = 20;
        st.player_a.hand = vec![CardId(0)];
    }
    assert!(
        matches!(
            e.apply(Seat::A, Command::Devolve { tile, base_hand: 0 }),
            Err(recollect_core::Reject::DevolveConditionUnmet)
        ),
        "a healthy form cannot be receded — devolution is window-only"
    );
    assert!(
        !e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Devolve { .. })),
        "and it is never offered for a healthy form"
    );
}

#[test]
fn devolve_rejects_an_uncontested_fade_no_deadline() {
    // A fading form with NO `fade_deadline` (an uncontested Dusk fade, not a combat
    // banish) is NOT in the standing-Faded window — it cannot be receded.
    let mut st = blank();
    let tile = 12u8;
    recollect_core::test_support::put_spirit(&mut st, tile, CardId(1), Seat::A);
    {
        let sp = st.board[tile as usize].spirit.as_mut().unwrap();
        sp.fading = true;
        sp.banished_by = None;
        sp.fade_deadline = None; // uncontested — not a combat fade
    }
    st.active = Seat::A;
    st.active_slot = SeatSlot::A1;
    st.player_a.anima = 20;
    st.player_a.hand = vec![CardId(0)];
    let mut e = eng(st, 7);
    assert!(
        matches!(
            e.apply(Seat::A, Command::Devolve { tile, base_hand: 0 }),
            Err(recollect_core::Reject::DevolveConditionUnmet)
        ),
        "an uncontested fade (no deadline) is outside the window"
    );
}

#[test]
fn devolve_the_played_card_must_be_a_base_in_the_forms_line() {
    // The hand card must be the form's base. A wrong card (the prey, or even the Fabled)
    // is rejected. We give A a hand of [Prey, Mythwolf] and try each against a faded Primal.
    let (mut e, tile) = faded_form(1, 7);
    e.state_mut_for_test().player_a.hand = vec![CardId(4), CardId(2)]; // [Prey, Fabled]
    // Index 0 = Prey (not the base).
    assert!(
        matches!(
            e.apply(Seat::A, Command::Devolve { tile, base_hand: 0 }),
            Err(recollect_core::Reject::DevolveConditionUnmet)
        ),
        "an unrelated card cannot be the recede base"
    );
    // Index 1 = the Fabled form (a form, not the base) — also rejected.
    assert!(
        matches!(
            e.apply(Seat::A, Command::Devolve { tile, base_hand: 1 }),
            Err(recollect_core::Reject::DevolveConditionUnmet)
        ),
        "a form is not its own line's base"
    );
    // And neither is offered — only a true base in hand yields a Devolve.
    assert!(
        !e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Devolve { .. })),
        "no base in hand ⇒ no recede offered"
    );
}

#[test]
fn devolve_requires_the_half_cost_anima() {
    // Without ⌊form.cost/2⌋ Anima, the recede is rejected and never offered.
    let (mut e, tile) = faded_form(1, 7); // Direwolf ⇒ cost 3
    e.state_mut_for_test().player_a.anima = 2; // one short
    assert!(
        matches!(
            e.apply(Seat::A, Command::Devolve { tile, base_hand: 0 }),
            Err(recollect_core::Reject::NotEnoughAnima)
        ),
        "the recede needs ⌊cost/2⌋ = 3 Anima"
    );
    assert!(
        !e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Devolve { .. })),
        "an unaffordable recede is not offered"
    );
}

#[test]
fn devolve_only_your_own_form() {
    // You cannot recede the opponent's faded form.
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat(), deck.clone(), deck);
    let tile = 12u8;
    {
        let st = e.state_mut_for_test();
        // B's faded Direwolf; it is A's turn.
        recollect_core::test_support::put_spirit(st, tile, CardId(1), Seat::B);
        {
            let sp = st.board[tile as usize].spirit.as_mut().unwrap();
            sp.fading = true;
            sp.banished_by = Some(Seat::A);
            sp.fade_deadline = Some(st.round);
        }
        st.active = Seat::A;
        st.active_slot = SeatSlot::A1;
        st.player_a.anima = 20;
        st.player_a.hand = vec![CardId(0)];
    }
    assert!(
        matches!(
            e.apply(Seat::A, Command::Devolve { tile, base_hand: 0 }),
            Err(recollect_core::Reject::NotYourSpirit)
        ),
        "you cannot recede the opponent's form"
    );
}

#[test]
fn the_summoning_sick_rescued_base_cannot_move_or_evolve_until_next_turn() {
    // After devolution, the base is summoning-sick: no move, no evolve this turn —
    // even though it is a healthy base with a form card available. It re-evolves NEXT turn.
    let (mut e, tile) = faded_form(1, 7);
    // Give A the Fabled form too, so a Fabled evolve would be tempting THIS turn.
    e.state_mut_for_test().player_a.hand = vec![CardId(0), CardId(2)]; // [Cub base, Mythwolf]
    e.apply(Seat::A, Command::Devolve { tile, base_hand: 0 })
        .unwrap();
    // This turn: the base stands healthy, but is summoning-sick — no Evolve offered on it.
    assert!(
        !e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Evolve { tile: t, .. } if *t == tile)),
        "a just-rescued base cannot evolve the same turn (summoning sick)"
    );
    // Also no Move (it has no Mobile, but the summoning-sick gate is the point — a Mobile
    // base would still be blocked). Pass the turn around to A again.
    e.apply(Seat::A, Command::EndTurn).unwrap();
    e.apply(Seat::B, Command::EndTurn).unwrap();
    assert_eq!(e.state().active, Seat::A, "A's next turn");
    // Now (a fresh turn, summoning sickness cleared, base healthy + not just-arrived) the
    // base may Fabled-evolve — give it a donor in play first.
    recollect_core::test_support::put_spirit(e.state_mut_for_test(), 7, CardId(3), Seat::A);
    e.state_mut_for_test().player_a.anima = 20;
    assert!(
        e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Evolve { tile: t, .. } if *t == tile)),
        "next turn the rescued base may evolve again (the cycle continues)"
    );
}

#[test]
fn the_full_evolve_devolve_cycle_base_primal_devolve_fabled_devolve() {
    // The headline cycle (§5): base → Primal → (banished/faded) → recede to base →
    // [summoning-sick a turn] → Fabled → (banished/faded) → recede to base again.
    // A spirit may cycle evolve↔devolve without limit, bounded only by the forms/bases
    // in hand. Driven surgically through the standing-Faded window each lap.
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat(), deck.clone(), deck);
    let tile = 12u8;

    // ── Lap 1: a faded base Primal-evolves. (Set up a faded Cub base.)
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, tile, CardId(0), Seat::A);
        {
            let sp = st.board[tile as usize].spirit.as_mut().unwrap();
            sp.fading = true;
            sp.banished_by = Some(Seat::B);
            sp.fade_deadline = Some(st.round + 1);
        }
        st.active = Seat::A;
        st.active_slot = SeatSlot::A1;
        st.player_a.anima = 30;
        st.player_a.hand = vec![CardId(1)]; // the Primal form
        st.player_a.deck.clear();
        st.player_b.deck.clear();
        st.moved_this_turn.clear();
    }
    e.apply(
        Seat::A,
        Command::Evolve {
            tile,
            form_hand: 0,
            fuel: None,
            engage: None,
        },
    )
    .expect("base → Primal");
    assert_eq!(
        e.state().board[tile as usize].spirit.as_ref().unwrap().card,
        CardId(1)
    );

    // ── The Primal is banished in combat (standing-Faded). Recede it to the base.
    {
        let st = e.state_mut_for_test();
        let sp = st.board[tile as usize].spirit.as_mut().unwrap();
        sp.fading = true;
        sp.banished_by = Some(Seat::B);
        sp.fade_deadline = Some(st.round + 1);
        st.player_a.hand = vec![CardId(0)]; // the base card to recede to
        st.player_a.anima = 30;
        st.moved_this_turn.clear();
    }
    e.apply(Seat::A, Command::Devolve { tile, base_hand: 0 })
        .expect("Primal → base (recede)");
    let sp = e.state().board[tile as usize].spirit.as_ref().unwrap();
    assert_eq!(sp.card, CardId(0), "back to the base");
    assert!(!sp.fading && sp.hp == 40, "rescued, full HP");
    assert!(
        e.state().moved_this_turn.contains(&tile),
        "and summoning-sick this lap"
    );

    // ── A turn passes (summoning sickness clears). Now the base Fabled-evolves (donor).
    e.apply(Seat::A, Command::EndTurn).unwrap();
    e.apply(Seat::B, Command::EndTurn).unwrap();
    {
        let st = e.state_mut_for_test();
        // The base survived the round-trip (it is healthy, not banished). Give a donor + form.
        recollect_core::test_support::put_spirit(st, 7, CardId(3), Seat::A);
        st.player_a.hand = vec![CardId(2)]; // the Fabled form
        st.player_a.anima = 30;
        st.moved_this_turn.clear();
    }
    e.apply(
        Seat::A,
        Command::Evolve {
            tile,
            form_hand: 0,
            fuel: Some(7),
            engage: None,
        },
    )
    .expect("base → Fabled (donor-fueled)");
    assert_eq!(
        e.state().board[tile as usize].spirit.as_ref().unwrap().card,
        CardId(2),
        "now the Fabled"
    );

    // ── The Fabled is banished (standing-Faded). Recede it to the base AGAIN.
    {
        let st = e.state_mut_for_test();
        let sp = st.board[tile as usize].spirit.as_mut().unwrap();
        sp.fading = true;
        sp.banished_by = Some(Seat::B);
        sp.fade_deadline = Some(st.round + 1);
        st.player_a.hand = vec![CardId(0)];
        st.player_a.anima = 30;
        st.moved_this_turn.clear();
    }
    e.apply(Seat::A, Command::Devolve { tile, base_hand: 0 })
        .expect("Fabled → base (recede again — the cycle has no limit)");
    assert_eq!(
        e.state().board[tile as usize].spirit.as_ref().unwrap().card,
        CardId(0),
        "the cycle returns to the base: base → Primal → base → Fabled → base"
    );
}

#[test]
fn devolution_is_deterministic_same_seed_same_state_identical_events() {
    // Determinism (invariant 1): the same staged state + the same Devolve produces
    // byte-identical events and resulting state on two independent engines.
    let run = || -> (Vec<Event>, String) {
        let (mut e, tile) = faded_form(1, 42);
        let evs = e
            .apply(Seat::A, Command::Devolve { tile, base_hand: 0 })
            .unwrap();
        (evs.clone(), serde_json::to_string(e.state()).unwrap())
    };
    let (ev_a, st_a) = run();
    let (ev_b, st_b) = run();
    assert_eq!(
        ev_a, ev_b,
        "Devolve emits identical events for the same input"
    );
    assert_eq!(st_a, st_b, "and reaches a byte-identical state");
}

#[test]
fn devolve_redaction_the_opponent_never_sees_the_played_base_pre_reveal() {
    // Redaction (invariant 2): while the base card sits in A's hand (pre-Devolve), B's
    // view shows it only as a count — never the card. After the Devolve, B sees the
    // RESULTING base on the board (public), but still never the rest of A's hand.
    use recollect_core::view::view_for;
    let (mut e, tile) = faded_form(1, 7);
    // Add a second, DIFFERENT hidden card so the hand is non-trivial.
    e.state_mut_for_test().player_a.hand = vec![CardId(0), CardId(2)]; // [base, Fabled]
    // Pre-Devolve: B's view of A is counts only — neither card id leaks.
    let vb = view_for(&e, Seat::B);
    let json = serde_json::to_string(&vb).unwrap();
    assert_eq!(vb.opponent.hand_count, 2, "B sees A's hand SIZE");
    assert_eq!(
        json.matches("\"hand\":").count(),
        1,
        "exactly one hand in the view — B's own; A's hand is counts-only"
    );
    // Resolve the Devolve (plays the base at index 0).
    e.apply(Seat::A, Command::Devolve { tile, base_hand: 0 })
        .unwrap();
    // Post: the base is on the board (B can see it there — public), A's hand is now [Fabled],
    // and B's view still never enumerates A's remaining hidden card.
    let vb2 = view_for(&e, Seat::B);
    assert_eq!(
        vb2.opponent.hand_count, 1,
        "A's hand shrank by the played base"
    );
    assert_eq!(
        serde_json::to_string(&vb2)
            .unwrap()
            .matches("\"hand\":")
            .count(),
        1,
        "still exactly one hand (B's own) — the remaining card stays hidden"
    );
    // The resulting base IS visible on the board to B (it became public when played).
    assert_eq!(
        e.state().board[tile as usize].spirit.as_ref().unwrap().card,
        CardId(0)
    );
}

#[test]
fn devolve_on_round_12_rescues_the_base_which_then_survives_nightfall_scoring() {
    // The round-12 edge of the cycle (§0.5 window vs the Nightfall dissolve): a form
    // banished on round 12 lingers standing-Faded and would be dissolved by `finish`
    // BEFORE scoring (the banisher takes the tile). But if its owner DEVOLVES it that
    // round, the rescue clears the fade — so at Nightfall the receded base is NOT in
    // `finish`'s dissolve pass; it STANDS and scores for its owner, not the banisher.
    let (mut e, tile) = faded_form(1, 7); // a faded Direwolf (Primal), A's turn, A holds the base
    {
        let st = e.state_mut_for_test();
        st.round = st.rules.last_round; // round 12 (Nightfall is this turn's end)
        let sp = st.board[tile as usize].spirit.as_mut().unwrap();
        sp.fade_deadline = Some(st.round + 1); // banished on B's turn, owner A ⇒ round+1
        st.player_a.deck.clear();
        st.player_b.deck.clear();
    }
    // A devolves the standing-Faded Primal on round 12 — the rescue is legal here too.
    e.apply(Seat::A, Command::Devolve { tile, base_hand: 0 })
        .expect("devolve is legal on round 12 (it rescues the base from the Nightfall dissolve)");
    let sp = e.state().board[tile as usize].spirit.as_ref().unwrap();
    assert_eq!(sp.card, CardId(0), "receded to the Cub base");
    assert!(
        !sp.fading,
        "the fade is cleared — it is no longer due to dissolve"
    );
    // A ends its turn, then B's turn-end fires the Nightfall `finish`.
    e.apply(Seat::A, Command::EndTurn).unwrap();
    let evs = e.apply(Seat::B, Command::EndTurn).unwrap();
    assert!(
        !evs.iter()
            .any(|ev| matches!(ev, Event::SpiritDissolved { tile: t, .. } if *t == tile)),
        "the rescued base is NOT dissolved at Nightfall (it was saved before scoring)"
    );
    assert_eq!(
        e.state().board[tile as usize]
            .spirit
            .as_ref()
            .map(|s| s.owner),
        Some(Seat::A),
        "the base stands at Nightfall and scores its tile for A (not the banisher B)"
    );
    match e.state().phase {
        recollect_core::state::Phase::Finished { score_a, .. } => {
            assert!(score_a >= 1, "A scores the rescued base's tile");
        }
        _ => panic!("round 12 ended → the match is Finished"),
    }
}

/// §5.4 Throughline-completion lifecycle: **devolution resets** `throughline_done`. The
/// form had completed its Throughline (flag set) before it was banished; receding it to a
/// fresh base arrives `throughline_done = false` — "a fresh base earns its Throughline
/// anew." (Devolution only happens while the form is Fading, which already broke the flag;
/// the reset is made explicit on the base for consistency — this pins it directly.)
#[test]
fn devolution_resets_the_throughline_done_flag() {
    let (mut e, tile) = faded_form(1, 7); // a faded Direwolf (Primal)
    // The form had completed a Throughline (and was then banished into its window).
    e.state_mut_for_test().board[tile as usize]
        .spirit
        .as_mut()
        .unwrap()
        .throughline_done = true;
    e.apply(Seat::A, Command::Devolve { tile, base_hand: 0 })
        .expect("the recede resolves");
    assert!(
        !e.state().board[tile as usize]
            .spirit
            .as_ref()
            .unwrap()
            .throughline_done,
        "devolution recedes to a fresh base — throughline_done is reset, re-completable"
    );
}
