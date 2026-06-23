//! Keyword behaviour suite. The combat keywords each have a precise contract
//! in the design doc; this asserts each one DIRECTLY (not incidentally via a
//! card test), so a refactor that breaks a keyword fails here with a clear
//! name. Companion to the full-catalog playthrough fuzz
//! (`suites/fuzz.rs`, random play) — this is the targeted half,
//! constructing the exact triggering condition each keyword needs.
use recollect_core::Engine;
use recollect_core::engine::forecast_exchange;
use recollect_core::state::{Command, Event, Spirit, StrikeKind};
use recollect_core::types::{CardDef, CardId, CardKind, Reach, Resonance, Seat};

fn def(id: u16, name: &str) -> CardDef {
    CardDef {
        id: CardId(id),
        name: name.into(),
        cost: 2,
        attack: 30,
        defense: 20,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Neutral,
        kind: CardKind::Spirit,
        ..Default::default()
    }
}

fn spirit(c: &CardDef, owner: Seat) -> Spirit {
    Spirit {
        card: c.id,
        owner,
        attack: c.attack,
        defense: c.defense,
        hp: c.hp,
        hp_max: c.hp,
        fading: false,
        banished_by: None,
        intercepted_this_round: false,
        traits_stripped: false,
        traits_stripped_until: None,
        replacement_used: false,
        holding: false,
        face_down: false,
        is_token: false,
        placed_by: None,
        kw_grants: Vec::new(),
        no_engage_until: 0,
        throughline_done: false,
        copied_reach: None,
        fade_deadline: None,
    }
}

#[test]
fn arcane_ignores_twenty_defense() {
    let mut arc = def(0, "Piercer");
    arc.arcane = true;
    let plain = def(1, "Plain");
    let target = def(2, "Wall");
    let dfn = spirit(&target, Seat::B);
    let f_arc = forecast_exchange(
        &arc,
        arc.attack,
        arc.defense,
        arc.hp,
        arc.hp,
        &dfn,
        &target,
        0,
        target.warded,
    );
    let f_plain = forecast_exchange(
        &plain,
        plain.attack,
        plain.defense,
        plain.hp,
        plain.hp,
        &dfn,
        &target,
        0,
        target.warded,
    );
    assert_eq!(
        f_arc.to_defender - f_plain.to_defender,
        20,
        "Arcane ignores exactly 20 defense"
    );
}

#[test]
fn warded_negates_arcane_piercing() {
    // Warded's contract: Arcane does NOT pierce a Warded defender — it keeps
    // its full defense even against Arcane. So a Warded defender takes the SAME
    // damage from an Arcane attacker as from an identical plain one (whereas a
    // non-Warded defender would take 20 more from the Arcane).
    let mut warded = def(0, "Bulwark");
    warded.warded = true;
    warded.defense = 20;
    let mut unwarded = def(3, "OpenWall");
    unwarded.defense = 20;
    let plain = def(1, "Striker");
    let mut arc = def(2, "Arc");
    arc.arcane = true;
    let dfn_w = spirit(&warded, Seat::B);
    let dfn_u = spirit(&unwarded, Seat::B);
    let w_vs_plain = forecast_exchange(
        &plain,
        plain.attack,
        plain.defense,
        plain.hp,
        plain.hp,
        &dfn_w,
        &warded,
        0,
        warded.warded,
    );
    let w_vs_arc = forecast_exchange(
        &arc,
        arc.attack,
        arc.defense,
        arc.hp,
        arc.hp,
        &dfn_w,
        &warded,
        0,
        warded.warded,
    );
    assert_eq!(
        w_vs_arc.to_defender, w_vs_plain.to_defender,
        "Warded negates Arcane piercing — same damage from Arcane as from plain"
    );
    // And the un-Warded control DOES take more from Arcane (so the test bites).
    let u_vs_arc = forecast_exchange(
        &arc,
        arc.attack,
        arc.defense,
        arc.hp,
        arc.hp,
        &dfn_u,
        &unwarded,
        0,
        unwarded.warded,
    );
    assert_eq!(
        u_vs_arc.to_defender - w_vs_arc.to_defender,
        20,
        "an un-Warded defender takes 20 more from Arcane than a Warded one"
    );
}

#[test]
fn resonance_edge_adds_ten_attack() {
    let mut fury = def(0, "Fury");
    fury.resonance = Resonance::Fury;
    let mut calm = def(1, "Sorrow");
    calm.resonance = Resonance::Sorrow;
    let neutral_atk = def(2, "Neutral");
    let dfn = spirit(&calm, Seat::B);
    let with_edge = forecast_exchange(
        &fury,
        fury.attack,
        fury.defense,
        fury.hp,
        fury.hp,
        &dfn,
        &calm,
        0,
        calm.warded,
    );
    let no_edge = forecast_exchange(
        &neutral_atk,
        neutral_atk.attack,
        neutral_atk.defense,
        neutral_atk.hp,
        neutral_atk.hp,
        &dfn,
        &calm,
        0,
        calm.warded,
    );
    if fury.resonance.edge_over(calm.resonance) {
        assert_eq!(
            with_edge.to_defender - no_edge.to_defender,
            10,
            "a resonance edge is +10 attack"
        );
    }
}

#[test]
fn echo_eligibility_toggles_at_half_hp() {
    let c = def(0, "Echoer");
    let mut s = spirit(&c, Seat::A);
    assert!(!s.echo_eligible(), "not echo-eligible above half HP");
    s.hp = 20;
    assert!(s.echo_eligible(), "echo-eligible at or below half HP");
}

#[test]
fn steadfast_cannot_be_pushed() {
    let mut anchor = def(0, "Anchor");
    anchor.steadfast = true;
    let pusher = def(1, "Shover");
    let cat = vec![anchor, pusher];
    let deck: Vec<CardId> = (0..20).map(|_| CardId(1)).collect();
    let (mut e, _) = Engine::new(1, cat, deck.clone(), deck);
    recollect_core::test_support::put_spirit(e.state_mut_for_test(), 12, CardId(0), Seat::A);
    let moved = e.try_push_for_test(11, 12, Seat::B);
    assert!(!moved, "a Steadfast spirit resists the push");
    assert!(
        e.state().board[12].spirit.is_some(),
        "and stays on its tile"
    );
}

#[test]
fn a_normal_spirit_is_pushed() {
    let cat = vec![def(0, "Drifter"), def(1, "Shover")];
    let deck: Vec<CardId> = (0..20).map(|_| CardId(1)).collect();
    let (mut e, _) = Engine::new(1, cat, deck.clone(), deck);
    recollect_core::test_support::put_spirit(e.state_mut_for_test(), 12, CardId(0), Seat::A);
    let moved = e.try_push_for_test(11, 12, Seat::B);
    assert!(moved, "a normal spirit is pushed");
    assert!(e.state().board[12].spirit.is_none(), "it left tile 12");
}

#[test]
fn relentless_chains_while_defeating() {
    let mut chainer = def(0, "Relentless One");
    chainer.relentless = true;
    chainer.attack = 90;
    chainer.reach = Reach::Wide;
    chainer.hp = 60;
    let mut weak = def(1, "Weakling");
    weak.attack = 0;
    weak.defense = 0;
    weak.hp = 10;
    let cat = vec![chainer, weak];
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(1, cat, deck.clone(), deck);
    let st = e.state_mut_for_test();
    recollect_core::test_support::put_spirit(st, 11, CardId(1), Seat::B);
    recollect_core::test_support::put_spirit(st, 13, CardId(1), Seat::B);
    st.board[11].spirit.as_mut().unwrap().hp = 10;
    st.board[13].spirit.as_mut().unwrap().hp = 10;
    st.board[7].impressions = vec![Seat::A];
    st.player_a.first_placement_done = true;
    let hi = e
        .state()
        .player(Seat::A)
        .hand
        .iter()
        .position(|c| c.0 == 0)
        .unwrap_or(0) as u8;
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: hi,
                tile: 12,
                engage: Some(11),
                chain_prefs: vec![13],
            },
        )
        .unwrap();
    let chains = evs
        .iter()
        .filter(|ev| {
            matches!(
                ev,
                Event::Struck {
                    kind: StrikeKind::Chain(_),
                    ..
                }
            )
        })
        .count();
    assert!(
        chains >= 1,
        "Relentless continued the chain into a second target"
    );
}

#[test]
fn mourner_heals_all_allies_when_a_spirit_dissolves() {
    // Saudade (Mourner): OnAnyBanish → restore 10 HP to all your spirits. We
    // wound an ally, then dissolve a spirit, and check the ally healed.
    let cat = recollect_core::cards::canon_catalog();
    let saudade = cat
        .iter()
        .find(|c| c.name.starts_with("Saudade"))
        .expect("Saudade exists")
        .id;
    let filler = cat
        .iter()
        .find(|c| c.kind == CardKind::Spirit && c.id != saudade && !c.lurk)
        .unwrap()
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(1, cat, deck.clone(), deck);
    let st = e.state_mut_for_test();
    recollect_core::test_support::put_spirit(st, 12, saudade, Seat::A);
    recollect_core::test_support::put_spirit(st, 13, filler, Seat::A); // wounded ally
    st.board[13].spirit.as_mut().unwrap().hp = 20;
    st.board[13].spirit.as_mut().unwrap().hp_max = 40;
    // A spirit to dissolve (enemy) at 7.
    recollect_core::test_support::put_spirit(st, 7, filler, Seat::B);
    let before = e.state().board[13].spirit.as_ref().unwrap().hp;
    // Fire OnAnyBanish via the dissolution path: force-fade the enemy.
    e.state_mut_for_test().board[7]
        .spirit
        .as_mut()
        .unwrap()
        .fading = true;
    e.force_fade_step_for_test(Seat::B);
    let after = e.state().board[13].spirit.as_ref().unwrap().hp;
    assert!(
        after > before,
        "Mourner healed the wounded ally when a spirit dissolved ({before}→{after})"
    );
}

#[test]
fn attune_grants_shared_resonance_when_adjacent_allies_align() {
    // Latchling (Attune): Static, while adjacent to 2+ allies sharing a
    // Resonance, it counts as that Resonance. We surround it with two Fury
    // allies and check its combat resonance becomes Fury (an edge it lacked).
    use recollect_core::engine::combat_stats_for_test;
    let cat = recollect_core::cards::canon_catalog();
    let latch = cat
        .iter()
        .find(|c| c.name == "Latchling")
        .expect("Latchling exists")
        .id;
    // Two allies of the same Resonance adjacent to the Latchling.
    let fury_ally = cat
        .iter()
        .find(|c| c.resonance == Resonance::Fury && c.kind == CardKind::Spirit)
        .map(|c| c.id);
    let Some(fury_ally) = fury_ally else {
        return;
    };
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(1, cat.clone(), deck.clone(), deck);
    let st = e.state_mut_for_test();
    recollect_core::test_support::put_spirit(st, 12, latch, Seat::A);
    recollect_core::test_support::put_spirit(st, 11, fury_ally, Seat::A);
    recollect_core::test_support::put_spirit(st, 13, fury_ally, Seat::A);
    let cs = combat_stats_for_test(e.state(), &cat, 12);
    assert_eq!(
        cs.resonance,
        Some(Resonance::Fury),
        "Attune granted the shared adjacent Resonance"
    );
}

#[test]
fn throughline_completes_a_line_of_three_shared_imprints() {
    // Throughline — the one keyword RULES_COVERAGE flagged as fuzzed but never
    // asserted directly. An arrival that forms a straight line of 3+ allied spirits
    // sharing one Imprint completes the run: the completer gains +10/+10 and a full
    // restore, once.
    let kin = |id: u16, name: &str| {
        let mut c = def(id, name);
        c.imprints = vec!["Kin".into()];
        c
    };
    let cat = vec![kin(0, "Kin Alpha"), kin(1, "Kin Beta"), kin(2, "Kin Gamma")];
    let deck: Vec<CardId> = (0..20).map(|_| CardId(2)).collect(); // the hand holds Gamma
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let st = e.state_mut_for_test();
    // Two allied Kin already standing in a row (11, 12); the third closes it at 13.
    recollect_core::test_support::put_spirit(st, 11, CardId(0), Seat::A);
    recollect_core::test_support::put_spirit(st, 12, CardId(1), Seat::A);
    st.player_a.first_placement_done = true;
    let hi = e
        .state()
        .player(Seat::A)
        .hand
        .iter()
        .position(|c| c.0 == 2)
        .unwrap() as u8;
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: hi,
                tile: 13,
                engage: None,
                chain_prefs: Vec::new(),
            },
        )
        .expect("Gamma arrives at 13, closing the line of Kin");
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::ThroughlineCompleted {
                tile: 13,
                attack: 10,
                defense: 10
            }
        )),
        "a straight line of three shared Imprints completed: {evs:?}"
    );
    let sp = e.state().board[13].spirit.as_ref().expect("Gamma stands");
    assert_eq!(
        (sp.attack, sp.defense),
        (40, 30),
        "+10/+10 over Gamma's printed 30/20"
    );
    assert_eq!(sp.hp, sp.hp_max, "a full restore rides the completion");
    assert!(sp.throughline_done, "the reward is once-only");
}

// ── Forecast exact-output precision (mutation killers) ──────────────────────
// `forecast_exchange` is the PURE preview math every client shows before a commit; it
// must match `full_exchange`. The keyword tests above assert DIFFERENCES (arcane − plain,
// edge − none), which leaves the raw arithmetic and the echo/banish comparisons unpinned.
// These call it with fully-known inputs and assert EVERY field of the Forecast, so a
// mutation of any operator in L754-773 changes a value an assertion reads.

#[test]
fn forecast_pins_every_field_for_a_known_exchange() {
    // Attacker Fury 40/10/40 (full) with +10 bonus vs defender Sorrow 20/20/40.
    // Fury edges Sorrow (+10); Sorrow does not edge Fury (0). No arcane, no warded.
    //   to_defender = (40 + 10 edge + 10 bonus − 20 def) = 40  (exactly fells the 40-HP defender)
    //   to_attacker = (20 + 0 edge − 10 def)             = 10
    //   banishes_defender = (40 − 40 = 0) <= 0           = true
    //   banishes_attacker = (40 − 10 = 30) <= 0          = false
    //   attacker_echo_live = (40 * 2 = 80) <= 40         = false
    //   defender_echo_live = full-HP defender            = false
    let mut att = def(0, "Forecaster");
    att.attack = 40;
    att.defense = 10;
    att.resonance = Resonance::Fury;
    let mut dc = def(1, "Mark");
    dc.attack = 20;
    dc.defense = 20;
    dc.resonance = Resonance::Sorrow;
    let dfn = spirit(&dc, Seat::B);
    let f = forecast_exchange(
        &att,
        att.attack,
        att.defense,
        att.hp,
        att.hp,
        &dfn,
        &dc,
        10,
        false,
    );
    assert_eq!(
        f.to_defender, 40,
        "40 attack + 10 edge + 10 bonus − 20 defense"
    );
    assert_eq!(f.to_attacker, 10, "20 attack + 0 edge − 10 defense");
    assert!(f.banishes_defender, "40 − 40 = 0 ≤ 0: exactly lethal");
    assert!(!f.banishes_attacker, "40 − 10 = 30: the attacker stands");
    assert!(!f.attacker_echo_live, "a full-HP attacker is not at Echo");
    assert!(!f.defender_echo_live, "a full-HP defender is not at Echo");

    // Swap the resonances so the DEFENDER edges the attacker (Fury defender over Sorrow
    // attacker ⇒ d_edge = 10), pinning the `dfn.attack + d_edge` term (L766): to_attacker
    // = 20 + 10 edge − 10 def = 20. A `+ d_edge`→`- d_edge` flip would give 0.
    att.resonance = Resonance::Sorrow;
    dc.resonance = Resonance::Fury;
    let dfn2 = spirit(&dc, Seat::B);
    let g = forecast_exchange(
        &att,
        att.attack,
        att.defense,
        att.hp,
        att.hp,
        &dfn2,
        &dc,
        0,
        false,
    );
    assert_eq!(
        g.to_attacker, 20,
        "defender edge: 20 attack + 10 d_edge − 10 defense = 20"
    );
    assert_eq!(
        g.to_defender, 20,
        "no attacker edge now (Sorrow over Fury is not on the wheel): 40 attack − 20 defense = 20"
    );
}

#[test]
fn forecast_echo_live_flips_exactly_at_half_hp() {
    // attacker_echo_live is `att_hp * 2 <= att_hp_max` (L772). Pin both sides of the
    // boundary, which also pins the multiply:
    //   att_hp = 20, max = 40 ⇒ 40 <= 40 ⇒ TRUE  (kills `<=`→`>`: 40 > 40 is false)
    //   att_hp = 30, max = 40 ⇒ 60 <= 40 ⇒ FALSE (kills `*`→`+`: 32 ≤ 40 would be true;
    //                                              and `*`→`/`: 15 ≤ 40 would be true)
    let att = def(0, "Echoer");
    let dc = def(1, "Mark");
    let dfn = spirit(&dc, Seat::B);
    let at_half = forecast_exchange(&att, att.attack, att.defense, 20, 40, &dfn, &dc, 0, false);
    assert!(
        at_half.attacker_echo_live,
        "att_hp 20 of 40: exactly at half ⇒ Echo live"
    );
    let above_half = forecast_exchange(&att, att.attack, att.defense, 30, 40, &dfn, &dc, 0, false);
    assert!(
        !above_half.attacker_echo_live,
        "att_hp 30 of 40: above half ⇒ Echo not live"
    );
}

#[test]
fn forecast_banish_flags_track_the_lethal_boundary() {
    // banishes_defender `dfn.hp - to_defender <= 0` (L770) and banishes_attacker
    // `att_hp - to_attacker <= 0` (L771). Tune so each sits at exactly 0 (lethal) and each
    // at +10 (survives), pinning both the subtraction and the `<=`.
    let mut att = def(0, "Striker");
    att.attack = 40;
    att.defense = 20;
    let mut dc = def(1, "Mark");
    dc.attack = 40;
    dc.defense = 20;
    // Defender at 20 HP: to_defender = 40 − 20 = 20 ⇒ 20 − 20 = 0 ≤ 0 ⇒ banished.
    let mut dfn = spirit(&dc, Seat::B);
    dfn.hp = 20;
    // Attacker at 30 HP: to_attacker = 40 − 20 = 20 ⇒ 30 − 20 = 10 > 0 ⇒ survives.
    let f = forecast_exchange(&att, att.attack, att.defense, 30, 40, &dfn, &dc, 0, false);
    assert_eq!(
        (f.to_defender, f.to_attacker),
        (20, 20),
        "both blows are 40 − 20 = 20"
    );
    assert!(
        f.banishes_defender,
        "20 − 20 = 0 ≤ 0: the defender is felled"
    );
    assert!(
        !f.banishes_attacker,
        "30 − 20 = 10 > 0: the attacker survives (kills the `<=`→`>` and `-`→`+` on L771)"
    );
    // Now make the attacker exactly lethal too (att_hp 20): 20 − 20 = 0 ≤ 0 ⇒ banished.
    let g = forecast_exchange(&att, att.attack, att.defense, 20, 40, &dfn, &dc, 0, false);
    assert!(
        g.banishes_attacker,
        "20 − 20 = 0 ≤ 0: the attacker is felled at the boundary"
    );
}

#[test]
fn forecast_forgiven_debt_caps_an_echo_attackers_lethal_blow() {
    // The Forgiven Debt branch (L757-765): when the attacker is at Echo (att_hp*2 <= max)
    // AND the defender's name carries UnbanishableByEcho, a lethal blow is capped to leave
    // the defender at 1 HP — `to_defender.min((dfn.hp - 1).max(0))` (L762).
    let mut att = def(0, "Echo Striker");
    att.attack = 60; // a lethal blow before the cap
    att.defense = 20;
    let mut dc = def(1, "The Forgiven Debt"); // the NAME carries the exception
    dc.defense = 0;
    let mut dfn = spirit(&dc, Seat::B);
    dfn.hp = 40;
    // Echo-live attacker (20 of 40): the cap applies ⇒ to_defender = min(60, 40 − 1) = 39,
    // leaving the defender at 1; not banished. Kills L757:37 `<=`→`>` and L762 `(hp - 1)`.
    let capped = forecast_exchange(&att, att.attack, att.defense, 20, 40, &dfn, &dc, 0, false);
    assert_eq!(
        capped.to_defender, 39,
        "Echo blow capped to leave the Forgiven Debt at 1 HP"
    );
    assert!(
        !capped.banishes_defender,
        "the cap saves it: 40 − 39 = 1 > 0"
    );
    // A HEALTHY attacker (30 of 40, above half) is NOT at Echo, so the cap does NOT apply —
    // the full 60 lands and banishes. Kills the L757:33 `att_hp * 2` mutants (`+`/`/` would
    // wrongly read this attacker as echo-live and cap the blow).
    let full = forecast_exchange(&att, att.attack, att.defense, 30, 40, &dfn, &dc, 0, false);
    assert_eq!(
        full.to_defender, 60,
        "a healthy attacker's blow is uncapped"
    );
    assert!(full.banishes_defender, "60 fells the 40-HP defender");
}
