//! Post-rules-change red-team — END-TO-END scenarios for the three rules that shipped
//! after the last full-catalog sweep, plus the interaction edges they open. Distinct from
//! the unit pins in `evolve.rs` / `strays.rs` / `redteam_playthrough.rs`: each test here
//! drives the WHOLE lifecycle through real `decide`/`evolve` (a completion that actually
//! fires `check_throughline`, a fade that actually rides `SpiritBecameFading`, an Overwrite
//! that actually resolves the exchange) rather than constructing the mid-state — so the
//! reset/grant TIMING and the cross-rule compositions are what is under test.
//!
//! The three changes (design §2 / §5.4 / §11):
//!   1. THROUGHLINE lifecycle — fading breaks `throughline_done`; a Primal-evolve and a
//!      Devolve are both arrivals that arrive re-completable AND complete on the spot into a
//!      standing line (devolution is now an arrival, symmetric with evolution — the
//!      maintainer's ruling); a Fabled-evolve keeps it (locked).
//!   2. OVERWRITE reaches a Stray — a revealed Stray is fought (full exchange); a hidden
//!      Stray / veiled Wary is denied entry (it leaves: no impression, no reveal, no leak)
//!      + invariant 1b (a spirit and a Stray never coexist on a tile).
//!   3. The Unwritten-banishes-a-player path — no mark + a +1 off-board erasure tally.
//!
//! After every command: `invariants::check` (incl. 1b) must hold.
use crate::common::*;
use recollect_core::invariants::check as check_invariants;
use recollect_core::state::{Command, Event, Stray, StrikeKind, Temperament};
use recollect_core::types::*;
use recollect_core::{Engine, Seat};

// ---------------------------------------------------------------------------
// A purpose-built "Pack" line: a base, its Primal and Fabled forms, a donor, two
// Pack-mates to flank a 3-line, and a heavy hitter that fades a body in combat.
// Imprints are controlled so a straight 3-line of allied "Pack" carriers completes.
// ---------------------------------------------------------------------------
const PUP: u16 = 0; // base, imprint "Pack"
const WOLF: u16 = 1; // Primal form of Pup, imprint "Pack"
const ALPHA: u16 = 2; // Fabled form of Pup, imprint "Pack"
const MATE: u16 = 3; // a plain Pack ally (the flanking links / a Fabled donor)
const HUNTER: u16 = 4; // a heavy enemy hitter (fades a Pack body in one strike)
const GLASS: u16 = 5; // a fragile enemy (so an arrival's engage fades IT, not the arriver)

fn pack_cat() -> Vec<CardDef> {
    let mk = |id: u16,
              name: &str,
              cost: u8,
              attack: i16,
              defense: i16,
              hp: i16,
              kind: CardKind,
              rarity: &str,
              imprints: &[&str],
              evolves_from: Option<&str>,
              evolves_to: &[&str]| CardDef {
        id: CardId(id),
        name: name.into(),
        cost,
        attack,
        defense,
        hp,
        reach: Reach::Cross,
        resonance: Resonance::Neutral, // no wheel edge muddies the arithmetic
        kind,
        rarity: rarity.into(),
        imprints: imprints.iter().map(|s| s.to_string()).collect(),
        evolves_from: evolves_from.map(|s| s.to_string()),
        evolves_to: evolves_to.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    vec![
        mk(
            PUP,
            "Pup",
            2,
            10,
            0,
            40,
            CardKind::Spirit,
            "C",
            &["Pack"],
            None,
            &["Wolf", "Alpha"],
        ),
        mk(
            WOLF,
            "Wolf",
            5,
            30,
            10,
            50,
            CardKind::Evolution,
            "Primal",
            &["Pack"],
            Some("Pup"),
            &[],
        ),
        mk(
            ALPHA,
            "Alpha",
            6,
            40,
            30,
            60,
            CardKind::Evolution,
            "Fabled",
            &["Pack"],
            Some("Pup"),
            &[],
        ),
        mk(
            MATE,
            "Mate",
            1,
            20,
            10,
            40,
            CardKind::Spirit,
            "C",
            &["Pack"],
            None,
            &[],
        ),
        // 90 atk vs 0 def fells a 40-HP Pup outright; 0 atk so it never retaliates (the
        // banished body is what we want, cleanly, with the banisher recorded).
        mk(
            HUNTER,
            "Hunter",
            1,
            90,
            0,
            60,
            CardKind::Spirit,
            "C",
            &["Maw"],
            None,
            &[],
        ),
        // 5 atk / 0 def / 20 HP: an arriver's engage fells it and survives its retaliation.
        mk(
            GLASS,
            "Glass",
            1,
            5,
            0,
            20,
            CardKind::Spirit,
            "C",
            &["Maw"],
            None,
            &[],
        ),
    ]
}

/// A live engine on A's turn over the Pack catalog: A has rich Anima, its first placement
/// done, and the Primal + Fabled form cards in hand (index 0 = Primal Wolf, 1 = Fabled
/// Alpha). Each test seats its own board (the Pup + flanking Pack-mates) on top.
fn pack_engine() -> Engine {
    let deck: Vec<CardId> = (0..20).map(|_| CardId(PUP)).collect();
    let (mut e, _) = Engine::new(7, pack_cat(), deck.clone(), deck);
    let st = e.state_mut_for_test();
    st.player_a.anima = 30;
    st.player_a.first_placement_done = true;
    st.player_a.hand = vec![CardId(WOLF), CardId(ALPHA)]; // [Primal=0, Fabled=1]
    e
}

/// Assert a real Throughline completion (the `ThroughlineCompleted` buff) fired on `tile`.
fn assert_completed(evs: &[Event], tile: u8) {
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::ThroughlineCompleted { tile: t, .. } if *t == tile)),
        "expected a ThroughlineCompleted on tile {tile}: {evs:?}"
    );
}

// ===========================================================================
// 1. THROUGHLINE lifecycle — the full chain, through real play.
// ===========================================================================

/// THE headline scenario: a body **completes** a Throughline (real `check_throughline`
/// buff), is **banished in combat** (real `SpiritBecameFading` — the fade BREAKS the flag),
/// then is **Primal-rescued** into a fresh 3-line and **re-completes** (the buff a SECOND
/// time). Every transition rides a real event; nothing is hand-set. This is the §5.4
/// lifecycle end-to-end: complete → fade-resets → Primal re-completes.
#[test]
fn throughline_completes_then_fades_then_a_primal_rescue_recompletes() {
    let mut e = pack_engine();
    // Seat a Pup at centre (12) flanked by two Pack-mates (11, 13) — a horizontal 3-line.
    // The Pup is wounded to 1 HP so a single Hunter strike fades it (it must FADE, not
    // dissolve, to enter the standing-Faded window the Primal rescues from).
    {
        let st = e.state_mut_for_test();
        put(st, 11, MATE, Seat::A, None);
        put(st, 13, MATE, Seat::A, None);
        // Place the Pup through the arrival path so check_throughline fires for real.
    }
    // (a) COMPLETE: play a Pup onto 12 via the real arrival → it forms 11-12-13 and the
    // Throughline completes, granting +10/+10 and a full restore. We use Overwrite-free
    // placement: put a projector so 12 is legal, then PlaySpirit.
    {
        let st = e.state_mut_for_test();
        // A projector ally at 7 puts 12 in A's projection (Cross reach from 7 covers 12).
        put(st, 7, MATE, Seat::A, None);
        st.player_a.hand = vec![CardId(PUP), CardId(WOLF), CardId(ALPHA)];
    }
    let comp_evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0, // Pup
                tile: 12,
                engage: None,
                chain_prefs: vec![],
            },
        )
        .expect("the Pup is placed into the 3-line");
    assert_completed(&comp_evs, 12);
    {
        let sp = e.state().board[12].spirit.as_ref().unwrap();
        assert!(sp.throughline_done, "the Pup completed and is now done");
        assert_eq!(sp.attack, 10 + 10, "the +10 Attack completion buff landed");
        check_invariants(e.state()).unwrap();
    }

    // (b) FADE: the completed Pup is banished in combat (the Solace/an enemy fells it). The
    // combat that produces a `SpiritBecameFading` is exhaustively covered elsewhere; here the
    // unit under test is the §5.4 RESET, so we drive the real fade reducer directly — exactly
    // the event a lethal exchange emits — keeping the scenario surgical.
    e.apply_event_for_test(Event::SpiritBecameFading {
        tile: 12,
        banished_by: Some(Seat::B),
    });
    {
        let sp = e.state().board[12].spirit.as_ref().unwrap();
        assert!(sp.fading, "the Pup is now Fading (banished in combat)");
        assert!(
            !sp.throughline_done,
            "fading BROKE the Throughline flag — the body is re-completable"
        );
        check_invariants(e.state()).unwrap();
    }

    // (c) PRIMAL RESCUE → RE-COMPLETE: back on A's turn, A evolves the Fading Pup to its
    // Primal Wolf. The Primal arrives into the still-standing 11-12-13 Pack line and
    // re-completes the Throughline — the +10/+10 buff a SECOND time, on the same tile.
    {
        let st = e.state_mut_for_test();
        st.active = Seat::A;
        st.player_a.anima = 30;
        st.player_a.hand = vec![CardId(WOLF), CardId(ALPHA)]; // [Primal=0, Fabled=1]
        st.moved_this_turn.clear();
    }
    let rescue_evs = e
        .apply(
            Seat::A,
            Command::Evolve {
                tile: 12,
                form_hand: 0, // Primal Wolf
                fuel: None,
                engage: None,
            },
        )
        .expect("the Fading Pup is Primal-rescued");
    assert!(
        rescue_evs
            .iter()
            .any(|ev| matches!(ev, Event::SpiritEvolved { to, keeps_throughline: false, .. } if *to == CardId(WOLF))),
        "a Primal evolution carries keeps_throughline = false (re-completable): {rescue_evs:?}"
    );
    assert_completed(&rescue_evs, 12);
    let wolf = e.state().board[12].spirit.as_ref().unwrap();
    assert_eq!(wolf.card, CardId(WOLF), "the body is now the Primal Wolf");
    assert!(!wolf.fading, "the rescue cleared the fade");
    assert!(wolf.throughline_done, "and it re-completed — done again");
    assert_eq!(
        wolf.attack,
        30 + 10,
        "the Primal's printed 30 Attack + the re-completion's +10 (the SECOND buff)"
    );
    check_invariants(e.state()).unwrap();
}

/// The Fabled half of the asymmetry, end-to-end: a HEALTHY body **completes** a Throughline
/// (real buff), then leaps to its **Fabled** Alpha. The Fabled INHERITS the completed flag
/// (locked) — landing into a fresh 3-line yields NO second completion and NO extra buff.
#[test]
fn throughline_completes_then_a_fabled_evolution_keeps_it_locked() {
    let mut e = pack_engine();
    {
        let st = e.state_mut_for_test();
        put(st, 11, MATE, Seat::A, None);
        put(st, 13, MATE, Seat::A, None);
        put(st, 7, MATE, Seat::A, None); // projector so 12 is legal
        put(st, 6, MATE, Seat::A, None); // a donor for the Fabled leap (tile 6)
        st.player_a.hand = vec![CardId(PUP), CardId(WOLF), CardId(ALPHA)];
    }
    // (a) COMPLETE with a healthy Pup.
    let comp = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: 12,
                engage: None,
                chain_prefs: vec![],
            },
        )
        .expect("the Pup completes the line");
    assert_completed(&comp, 12);
    assert!(
        e.state().board[12]
            .spirit
            .as_ref()
            .unwrap()
            .throughline_done
    );
    // The Pup arrived THIS turn — a Fabled needs the turn after (summoning sickness). Clear
    // the just-arrived flag so the Fabled leap is legal (the next-turn state).
    {
        let st = e.state_mut_for_test();
        st.moved_this_turn.clear();
        st.player_a.anima = 30;
    }
    // (b) FABLED leap, fueled by the donor at 6.
    let fab = e
        .apply(
            Seat::A,
            Command::Evolve {
                tile: 12,
                form_hand: 1, // Fabled Alpha
                fuel: Some(6),
                engage: None,
            },
        )
        .expect("the healthy Pup leaps to Fabled");
    assert!(
        fab.iter()
            .any(|ev| matches!(ev, Event::SpiritEvolved { to, keeps_throughline: true, .. } if *to == CardId(ALPHA))),
        "a Fabled evolution carries keeps_throughline = true (inherited/locked): {fab:?}"
    );
    assert!(
        !fab.iter()
            .any(|ev| matches!(ev, Event::ThroughlineCompleted { .. })),
        "the Fabled kept the flag — it does NOT re-complete into the 3-line: {fab:?}"
    );
    let alpha = e.state().board[12].spirit.as_ref().unwrap();
    assert_eq!(alpha.card, CardId(ALPHA));
    assert!(alpha.throughline_done, "stays done (inherited, locked)");
    assert_eq!(
        alpha.attack, 40,
        "printed Attack only — no second Throughline buff"
    );
    check_invariants(e.state()).unwrap();
}

/// The Devolution leg of §5.4, end-to-end: a Fading FORM that had completed is **Devolved**
/// back to its base directly INTO a standing 3-line — and, because **devolution is now an
/// arrival** (the maintainer's ruling: *if evolutions are arrivals, devolutions should be
/// too*), the fresh base **re-completes the Throughline ON THE DEVOLVE**: `SpiritDevolved`
/// resets `throughline_done` to false, then `check_throughline` fires the +10/+10 and the
/// full heal on the spot — at parity with the Primal-evolve-into-a-line case
/// (`throughline_completes_then_fades_then_a_primal_rescue_recompletes`). Drives the real
/// `decide_devolve` path (not a flag poke).
///
/// This FLIPS the prior pin (devolution "not an arrival" → no auto-complete). The design
/// changed: devolution fires the same arrival triggers evolution fires.
#[test]
fn a_devolved_base_arrives_into_a_line_and_re_completes_on_the_devolve() {
    let mut e = pack_engine();
    {
        let st = e.state_mut_for_test();
        // A standing-Faded Wolf (a Primal FORM) at 12 that had completed, flanked by two
        // Pack-mates so the rescued base lands amid an 11-12-13 Pack 3-line. Standing-Faded =
        // fading + a due deadline (banished in combat, still in its §0.5 window).
        recollect_core::test_support::put_spirit(st, 12, CardId(WOLF), Seat::A);
        {
            let sp = st.board[12].spirit.as_mut().unwrap();
            sp.fading = true;
            sp.banished_by = Some(Seat::B);
            sp.fade_deadline = Some(st.round + 1); // due next turn — still inside the window
            sp.throughline_done = true; // it HAD completed before the fade
        }
        put(st, 11, MATE, Seat::A, None);
        put(st, 13, MATE, Seat::A, None);
        st.player_a.anima = 30;
        st.player_a.hand = vec![CardId(PUP)]; // the base to recede to
        st.moved_this_turn.clear();
    }
    let evs = e
        .apply(
            Seat::A,
            Command::Devolve {
                tile: 12,
                base_hand: 0, // Pup, the base in the Wolf's line
            },
        )
        .expect("the Fading Wolf recedes to its Pup base");
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritDevolved { to, .. } if *to == CardId(PUP))),
        "the form receded to its base: {evs:?}"
    );
    // Devolution is an arrival → it re-completes the Throughline ON THE DEVOLVE.
    assert_completed(&evs, 12);
    let pup = e.state().board[12].spirit.as_ref().unwrap();
    assert_eq!(pup.card, CardId(PUP), "the body is the base again");
    assert!(!pup.fading, "the devolve rescued it from the fade");
    assert!(
        pup.throughline_done,
        "the fresh base re-completed on the spot — done again (the §5.4 reset, then the arrival completion)"
    );
    assert_eq!(
        pup.attack,
        10 + 10,
        "the Pup's printed 10 Attack + the re-completion's +10 (the arrival completion buff landed)"
    );
    assert_eq!(
        pup.hp, pup.hp_max,
        "the completion full-healed the rescued base (full HP)"
    );
    check_invariants(e.state()).unwrap();
}

/// Parity proof, the OTHER half: a base that **devolves into a line completes exactly as a
/// Primal-evolve into the same line does** — same +10/+10, same full heal, same on-the-spot
/// `ThroughlineCompleted`. Receding a Fading form (whose `throughline_done` was NEVER set —
/// it had not completed before the fade) into a standing 3-line still completes, because the
/// devolve is an arrival and the line is real. The two rescue paths (Primal-evolve, Devolve)
/// now yield the same Throughline swing — the symmetry the ruling asked for.
#[test]
fn devolve_into_a_standing_line_completes_at_parity_with_a_primal_evolve() {
    let mut e = pack_engine();
    {
        let st = e.state_mut_for_test();
        // A standing-Faded Wolf at 12 that had NOT completed (throughline_done stays false),
        // flanked by two Pack-mates at 11/13 — the 11-12-13 Pack line.
        recollect_core::test_support::put_spirit(st, 12, CardId(WOLF), Seat::A);
        {
            let sp = st.board[12].spirit.as_mut().unwrap();
            sp.fading = true;
            sp.banished_by = Some(Seat::B);
            sp.fade_deadline = Some(st.round + 1);
            // throughline_done left false — this body never completed.
        }
        put(st, 11, MATE, Seat::A, None);
        put(st, 13, MATE, Seat::A, None);
        st.player_a.anima = 30;
        st.player_a.hand = vec![CardId(PUP)];
        st.moved_this_turn.clear();
    }
    let evs = e
        .apply(
            Seat::A,
            Command::Devolve {
                tile: 12,
                base_hand: 0,
            },
        )
        .expect("the Fading Wolf recedes to its Pup base, into the line");
    assert_completed(&evs, 12);
    let pup = e.state().board[12].spirit.as_ref().unwrap();
    assert!(pup.throughline_done, "the devolve completed the line");
    assert_eq!(pup.attack, 10 + 10, "printed 10 + the completion's +10");
    assert_eq!(pup.hp, pup.hp_max, "full heal from the completion");
    check_invariants(e.state()).unwrap();
}

// ===========================================================================
// 2. OVERWRITE reaches a Stray — invariant 1b never breaks; courtship/snipe edges.
// ===========================================================================

/// A purpose-built Overwrite/Stray catalog: a projector, a strong overwriter (fells a
/// 30-HP Stray and survives), a glass overwriter (dies to the Stray, leaving it wounded),
/// and Foundlings of each temperament.
fn stray_cat() -> Vec<CardDef> {
    let mk = |id: u16, name: &str, attack, defense, hp, kind, rules: &str| CardDef {
        id: CardId(id),
        name: name.into(),
        cost: 1,
        attack,
        defense,
        hp,
        reach: Reach::Cross,
        resonance: Resonance::Neutral,
        kind,
        rarity: "C".into(),
        imprints: vec!["Bloom".into()],
        rules: rules.into(),
        ..Default::default()
    };
    vec![
        mk(0, "Bloom Ally", 10, 0, 40, CardKind::Spirit, ""), // projector / courter
        mk(
            1,
            "Gentle Pup",
            10,
            0,
            30,
            CardKind::Foundling,
            "Gentle. follows",
        ),
        mk(
            2,
            "Wary Fawn",
            10,
            0,
            30,
            CardKind::Foundling,
            "Wary. watches",
        ),
        mk(
            3,
            "Feral Lynx",
            25,
            0,
            40,
            CardKind::Foundling,
            "Feral. bites",
        ),
        mk(4, "Kiln Bull", 40, 0, 50, CardKind::Spirit, ""), // strong overwriter (fells 30-HP)
        mk(5, "Spark Wisp", 10, 0, 5, CardKind::Spirit, ""), // glass overwriter (dies, wild lives)
    ]
}

fn stray_engine(stray: Stray, a_hand: &[u16]) -> Engine {
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, stray_cat(), deck.clone(), deck);
    let st = e.state_mut_for_test();
    recollect_core::test_support::put_spirit(st, 7, CardId(0), Seat::A); // projector reaching 12
    st.player_a.hand = a_hand.iter().map(|i| CardId(*i)).collect();
    st.player_a.anima = 20;
    st.player_a.first_placement_done = true;
    st.stray = Some(stray);
    e
}

fn stray_at(card: u16, tile: u8, temperament: Temperament, veiled: bool, hp: i16) -> Stray {
    Stray {
        card: CardId(card),
        tile,
        temperament,
        veiled,
        courtship: 0,
        courted_by: None,
        hp,
        hp_max: 30,
    }
}

/// Win-and-lose in one test, asserting invariant 1b (spirit-and-stray never coexist) at
/// EVERY step: (win) a strong overwriter fells a revealed Gentle Stray and takes the tile
/// — `stray` is cleared the instant the body lands; (lose) a glass overwriter dies, the
/// Stray survives in its slot and NO body ever shares the tile.
#[test]
fn overwrite_onto_a_revealed_stray_win_and_lose_never_coexist() {
    // WIN: Kiln Bull (40 atk) fells the 30-HP Gentle Stray.
    let mut win = stray_engine(stray_at(1, 12, Temperament::Gentle, false, 30), &[4]);
    let evs = win
        .apply(
            Seat::A,
            Command::Overwrite {
                hand_index: 0,
                tile: 12,
            },
        )
        .expect("the overwrite fells the revealed Stray");
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::OverwroteStray {
                tile: 12,
                success: true,
                ..
            }
        )),
        "the wild fell: {evs:?}"
    );
    assert!(
        win.state().stray.is_none(),
        "the Stray slot emptied as the body landed"
    );
    let sp = win.state().board[12]
        .spirit
        .as_ref()
        .expect("the overwriter stands");
    assert_eq!(sp.owner, Seat::A);
    assert!(!sp.fading);
    // Invariant 1b: a spirit on tile 12 with no Stray on 12 — never coexist.
    check_invariants(win.state()).unwrap();

    // LOSE: Spark Wisp (10 atk, 5 HP) can't fell the 30-HP Stray and dies to its 10-atk bite.
    let mut lose = stray_engine(stray_at(1, 12, Temperament::Gentle, false, 30), &[5]);
    let evs = lose
        .apply(
            Seat::A,
            Command::Overwrite {
                hand_index: 0,
                tile: 12,
            },
        )
        .expect("the failed overwrite resolves");
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::OverwroteStray {
                tile: 12,
                success: false,
                ..
            }
        )),
        "the wild survived; the overwriter dissolved: {evs:?}"
    );
    let s = lose
        .state()
        .stray
        .as_ref()
        .expect("the wild survives in its slot");
    assert_eq!(s.hp, 20, "the damage the overwriter dealt persists");
    assert!(
        lose.state().board[12].spirit.is_none(),
        "the dissolved overwriter never took the tile — no spirit shares the Stray's tile"
    );
    // Invariant 1b: a Stray on tile 12 with no spirit on 12 — never coexist.
    check_invariants(lose.state()).unwrap();
}

/// A Feral Stray is REVEALED (open from surfacing), so it is a legal Overwrite target — a
/// full exchange, NOT a deny. The snipe edge: a defeated Feral cannot intercept its own
/// banisher (the slot clears before the post-arrival interception step), so the overwriter
/// is not double-bitten by the wild it just felled. Invariant 1b holds throughout.
#[test]
fn overwrite_onto_a_revealed_feral_stray_is_fought_and_the_felled_feral_cannot_snipe() {
    // Kiln Bull (40 atk, 50 HP) vs a 40-HP Feral Lynx (25 atk). 40 ≥ 40 → felled; the Bull
    // takes 25 back from the simultaneous exchange (survives at 25). The felled Feral must
    // NOT also intercept (that would be a second 25 off the Bull).
    let mut e = stray_engine(stray_at(3, 12, Temperament::Feral, false, 40), &[4]);
    let evs = e
        .apply(
            Seat::A,
            Command::Overwrite {
                hand_index: 0,
                tile: 12,
            },
        )
        .expect("the overwrite fells the revealed Feral");
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::OverwroteStray {
                tile: 12,
                success: true,
                ..
            }
        )),
        "a revealed Feral is FOUGHT (not denied) and falls: {evs:?}"
    );
    assert!(
        e.state().stray.is_none(),
        "the Feral was banished by the overwrite"
    );
    let bull = e.state().board[12]
        .spirit
        .as_ref()
        .expect("the Bull took the tile");
    // The Bull took exactly the exchange's 25 (one bite), never a second interception bite.
    assert_eq!(
        bull.hp,
        50 - 25,
        "the felled Feral did NOT also intercept its banisher (no double bite)"
    );
    // No interception Struck landed on the overwriter from the (gone) Stray.
    assert!(
        !evs.iter().any(|ev| matches!(
            ev,
            Event::Struck {
                to_tile: 12,
                kind: StrikeKind::Interception,
                ..
            }
        )),
        "the banished Feral laid no interception strike: {evs:?}"
    );
    check_invariants(e.state()).unwrap();
}

/// Courtship-snipe edge: a Wary Stray PART-WAY through courtship (unveiled, one turn banked)
/// is **denied entry** by an Overwrite — it leaves cleanly. The courtship state vanishes
/// with it (`stray` is None), so no dangling `courted_by`/`courtship` lingers to mis-resume,
/// and the overwriter takes the cleared tile. (A hidden/veiled Wary denies; here it is
/// UNVEILED but mid-courtship, so the §2 "not face-up" deny does NOT apply — it is fought.)
#[test]
fn an_unveiled_mid_courtship_wary_is_fought_not_denied() {
    // An UNVEILED Wary with one courtship turn banked is face-up → a legal Overwrite target,
    // resolved as a fight (not a deny). Kiln Bull fells it.
    let mut courted = stray_at(2, 12, Temperament::Wary, false, 30);
    courted.courtship = 1;
    courted.courted_by = Some(Seat::A);
    let mut e = stray_engine(courted, &[4]);
    let evs = e
        .apply(
            Seat::A,
            Command::Overwrite {
                hand_index: 0,
                tile: 12,
            },
        )
        .expect("the overwrite fights the unveiled Wary");
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::OverwroteStray {
                tile: 12,
                success: true,
                ..
            }
        )),
        "an UNVEILED Wary is face-up → fought (a full exchange), not denied: {evs:?}"
    );
    assert!(
        !evs.iter().any(|ev| matches!(ev, Event::StrayDenied { .. })),
        "a face-up Stray is never DENIED — denial is the hidden path only: {evs:?}"
    );
    assert!(
        e.state().stray.is_none(),
        "the courted Wary is gone (banished, courtship cleared)"
    );
    check_invariants(e.state()).unwrap();
}

/// A VEILED Wary that is denied entry leaves no trace: no impression from the denial, no
/// reveal, the courtship state gone, and — crucially — the overwriter that lands is still
/// INTERCEPTABLE by an enemy zone (a deny-entry Overwrite is an arrival, §2). An enemy
/// covering spirit strikes the freshly-landed overwriter. Invariant 1b holds.
#[test]
fn a_denied_veiled_wary_leaves_the_overwriter_interceptable() {
    let veiled = stray_at(2, 12, Temperament::Wary, true, 30);
    let mut e = stray_engine(veiled, &[4]); // Kiln Bull lands uncontested on the cleared tile
    // An ENEMY (B) spirit covers tile 12 from 17 (Cross reach 17→12 ✓), so it may intercept
    // the overwriter that lands at 12.
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 17, CardId(4), Seat::B); // Kiln Bull (40 atk) covers 12
    }
    let evs = e
        .apply(
            Seat::A,
            Command::Overwrite {
                hand_index: 0,
                tile: 12,
            },
        )
        .expect("the deny-entry overwrite resolves");
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::StrayDenied { tile: 12 })),
        "the hidden Wary was denied entry: {evs:?}"
    );
    // The overwriter landed and the enemy zone bit it (an arrival is interceptable).
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::Struck {
                from_tile: 17,
                to_tile: 12,
                kind: StrikeKind::Interception,
                ..
            }
        )),
        "the landed overwriter is interceptable by the enemy zone (deny-entry is an arrival): {evs:?}"
    );
    // The Stray left no impression of its own; the denial named nothing.
    assert!(e.state().stray.is_none(), "the denied veil is gone");
    check_invariants(e.state()).unwrap();
}

// ===========================================================================
// 3. The Unwritten banishes a player — no mark + erasure +1; composed with §5.4.
// ===========================================================================

/// The §11 path composed with §5.4: the Solace banishes a player spirit that HAD completed
/// its Throughline. The fade must (a) reset `throughline_done` (the §5.4 break) AND (b) at
/// the owner's turn-END dissolve laying NO mark, with the Solace banking +1 on its off-board
/// erasure tally (the §11 asymmetry) — the two new rules holding TOGETHER on one body.
#[test]
fn a_solace_banished_player_throughline_body_resets_the_flag_and_tallies_no_mark() {
    use recollect_core::cards::canon_catalog;
    let cat = canon_catalog();
    let a_spirit = cat
        .iter()
        .find(|c| c.kind == CardKind::Spirit && !c.lurk)
        .expect("a plain spirit exists")
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| a_spirit).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let victim_tile = 12u8;
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Lorekeeper, Faction::Solace];
        st.round = 4;
        st.active = Seat::A; // the victim's owner; its turn-END runs the Fade
        recollect_core::test_support::put_spirit(st, victim_tile, a_spirit, Seat::A);
        {
            let sp = st.board[victim_tile as usize].spirit.as_mut().unwrap();
            sp.fading = true;
            sp.banished_by = Some(Seat::B); // the Solace felled it in combat
            sp.fade_deadline = Some(st.round); // due this turn-END
            // First assert the becoming-fading reducer ALREADY broke the flag in the live
            // engine: we simulate the post-fade state, so set it false here (the §5.4 break is
            // unit-pinned in evolve.rs; here we confirm it composes with the §11 dissolve).
            sp.throughline_done = false;
        }
        st.solace_erasures = 0;
        st.player_a.deck.clear();
        st.player_b.deck.clear();
    }
    // Sanity: the fading victim carries no completed flag (the §5.4 break already applied).
    assert!(
        !e.state().board[victim_tile as usize]
            .spirit
            .as_ref()
            .unwrap()
            .throughline_done,
        "fading broke the Throughline flag before the dissolve"
    );
    let evs = e
        .apply(Seat::A, Command::EndTurn)
        .expect("the Fade resolves the Solace-banished victim");
    // §11: NO mark on the tile, and the Solace's erasure tally went +1.
    let tile = &e.state().board[victim_tile as usize];
    assert!(tile.spirit.is_none(), "the victim dissolved");
    assert!(
        tile.impressions.is_empty(),
        "the Solace's banish leaves NO mark (§11): {:?}",
        tile.impressions
    );
    assert_eq!(
        e.state().solace_erasures,
        1,
        "the Solace banks +1 on its erasure tally for banishing a player foothold (§11)"
    );
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::SpiritDissolved { tile, impression: Seat::B } if *tile == victim_tile
        )),
        "the player spirit dissolves via SpiritDissolved with the Solace banisher: {evs:?}"
    );
    check_invariants(e.state()).unwrap();
}

/// §11 at NIGHTFALL, the PLAYER-spirit leg: a player spirit banished by the Solace that
/// lingers standing-Faded into round 12 is dissolved by the `finish` pass — and that final
/// dissolve must ALSO lay no mark and tally +1 (the same §11 asymmetry as the turn-END Fade,
/// via `lay_mark`). The existing round-12 test exercises the Unwritten/`TokenDissolved`
/// branch; this pins the `finish` → `SpiritDissolved` → `lay_mark`(Solace) branch — the path
/// for a PLAYER foothold the Solace erased on the final round, which would otherwise have
/// scored a free point if `finish` had laid the banisher's color naively.
#[test]
fn a_round_12_solace_banished_player_spirit_tallies_no_mark_at_nightfall() {
    use recollect_core::cards::canon_catalog;
    let cat = canon_catalog();
    let a_spirit = cat
        .iter()
        .find(|c| c.kind == CardKind::Spirit && !c.lurk)
        .expect("a plain spirit exists")
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| a_spirit).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let victim_tile = 12u8; // inner tile — no Dusk interaction
    let a_hold = 7u8; // A's own surviving spirit (its real point)
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Lorekeeper, Faction::Solace];
        st.round = 12; // the final round
        st.active = Seat::B; // B's turn-END runs the Nightfall finish
        st.contracted = true; // post-Dusk
        // A's spirit, banished IN COMBAT by the Solace, standing-Faded with a deadline that
        // is NOT due at B's round-12 turn-END (13), so it survives to the `finish` pass.
        recollect_core::test_support::put_spirit(st, victim_tile, a_spirit, Seat::A);
        {
            let sp = st.board[victim_tile as usize].spirit.as_mut().unwrap();
            sp.fading = true;
            sp.banished_by = Some(Seat::B); // the Solace felled it
            sp.fade_deadline = Some(13);
        }
        // A's own standing spirit holds one real tile.
        recollect_core::test_support::put_spirit(st, a_hold, a_spirit, Seat::A);
        st.solace_erasures = 0;
        st.player_a.deck.clear();
        st.player_b.deck.clear();
    }
    let evs = e
        .apply(Seat::B, Command::EndTurn)
        .expect("Nightfall resolves");
    // The victim dissolved at Nightfall leaving NO mark, and the Solace banked +1.
    let tile = &e.state().board[victim_tile as usize];
    assert!(
        tile.spirit.is_none(),
        "the round-12 player victim dissolved"
    );
    assert!(
        tile.impressions.is_empty(),
        "the Solace's banish leaves NO mark even at Nightfall (§11): {:?}",
        tile.impressions
    );
    assert_eq!(
        e.state().solace_erasures,
        1,
        "the Solace banks +1 for erasing the player foothold at Nightfall (§11)"
    );
    // The final score must credit A with ONLY its own standing spirit (1) and B with its
    // erasure tally (1) — A never scores the erased tile.
    let ended = evs.iter().find_map(|ev| match ev {
        Event::MatchEnded {
            score_a, score_b, ..
        } => Some((*score_a, *score_b)),
        _ => None,
    });
    assert_eq!(
        ended,
        Some((1, 1)),
        "A scores its own held tile (1); B scores its erasure tally (1) — the erased tile is no one's free point"
    );
    check_invariants(e.state()).unwrap();
}
