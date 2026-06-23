//! Input-validation **reject paths** for the `decide_*` command handlers and
//! `Engine::legal_commands`.
//!
//! The mutation sweep found a cluster of GUARD survivors here: tests exercise the
//! HAPPY path of each command but rarely assert the REJECTIONS, so flipping a guard
//! (`==`→`!=`, `&&`→`||`, deleting a `!`, an off-by-one on a `<`) often left no test
//! red. Each test below drives a command that is INVALID for a specific reason and
//! asserts `decide` returns the EXACT right [`Reject`] — and, where the same guard
//! also feeds `legal_commands`, that the command is correspondingly never offered.
//!
//! Setups are surgical: a [`blank`] board (empty hands/decks, rich anima) with
//! exactly the spirits/hand a case needs, plus a small purpose-built catalog with a
//! real 2-stage evolution line (base → Primal/Fabled), a donor, a lurker, and a
//! vanilla. Costs are real so the affordability paths are exercised.
use crate::common::blank;
use recollect_core::Reject;
use recollect_core::state::{Command, Event, Phase};
use recollect_core::types::{CardDef, CardId, CardKind, Reach, Resonance, Seat, SeatSlot};

// Card ids in the local catalog.
const BASE: u16 = 0; // "Cub" — cost 4, Cross reach
const PRIMAL: u16 = 1; // "Direwolf" — Primal form of Cub, cost 6 ⇒ evolve eff cost & devolve 3
const FABLED: u16 = 2; // "Mythwolf" — Fabled form of Cub, cost 8
const DONOR: u16 = 3; // "Packmate" — a plain non-token ally (Fabled fuel)
const LURKER: u16 = 4; // "Pale Knife" — face-down, Cross reach
const VANILLA: u16 = 5; // "Sheep" — plain, no line
const OTHER_BASE: u16 = 6; // "Kit" — an unrelated base (its own form is FOXFORM)
const FOXFORM: u16 = 7; // a Primal of OTHER_BASE (a real form that is NOT Cub's)
const SEED: u16 = 8; // "Seed" — a cost-1 base
const SPROUT: u16 = 9; // "Sprout" — a cost-1 Primal of SEED ⇒ devolve costs ⌊1/2⌋ = 0

fn cat() -> Vec<CardDef> {
    let base = CardDef {
        id: CardId(BASE),
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
        id: CardId(PRIMAL),
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
        id: CardId(FABLED),
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
        id: CardId(DONOR),
        name: "Packmate".into(),
        cost: 2,
        attack: 20,
        defense: 10,
        hp: 30,
        reach: Reach::Cross,
        resonance: Resonance::Fury,
        kind: CardKind::Spirit,
        ..Default::default()
    };
    let lurker = CardDef {
        id: CardId(LURKER),
        name: "Pale Knife".into(),
        cost: 3,
        attack: 40,
        defense: 0,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Fear,
        kind: CardKind::Spirit,
        lurk: true,
        ..Default::default()
    };
    let vanilla = CardDef {
        id: CardId(VANILLA),
        name: "Sheep".into(),
        cost: 1,
        attack: 10,
        defense: 0,
        hp: 30,
        reach: Reach::Cross,
        resonance: Resonance::Harmony,
        kind: CardKind::Spirit,
        ..Default::default()
    };
    let other_base = CardDef {
        id: CardId(OTHER_BASE),
        name: "Kit".into(),
        cost: 2,
        attack: 10,
        defense: 0,
        hp: 30,
        reach: Reach::Cross,
        resonance: Resonance::Wonder,
        kind: CardKind::Spirit,
        evolves_to: vec!["Foxfire".into()],
        ..Default::default()
    };
    let foxform = CardDef {
        id: CardId(FOXFORM),
        name: "Foxfire".into(),
        cost: 5,
        attack: 50,
        defense: 10,
        hp: 50,
        reach: Reach::Cross,
        resonance: Resonance::Wonder,
        kind: CardKind::Evolution,
        rarity: "Primal".into(),
        evolves_from: Some("Kit".into()),
        ..Default::default()
    };
    // A base→cheap-form line for the ZERO-cost arrival cases: base "Seed" (cost 5) →
    // Primal "Sprout" (cost 1). The evolve charge is max(0, 1 − ⌊5/2⌋) = max(0, −1) = 0
    // (a free evolve — pins the evolve AnimaSpent gate), and a devolve back costs
    // ⌊form.cost/2⌋ = ⌊1/2⌋ = 0 (a free recede — pins the devolve AnimaSpent gate).
    let seed = CardDef {
        id: CardId(SEED),
        name: "Seed".into(),
        cost: 5,
        attack: 10,
        defense: 0,
        hp: 20,
        reach: Reach::Cross,
        resonance: Resonance::Harmony,
        kind: CardKind::Spirit,
        evolves_to: vec!["Sprout".into()],
        ..Default::default()
    };
    let sprout = CardDef {
        id: CardId(SPROUT),
        name: "Sprout".into(),
        cost: 1,
        attack: 20,
        defense: 0,
        hp: 30,
        reach: Reach::Cross,
        resonance: Resonance::Harmony,
        kind: CardKind::Evolution,
        rarity: "Primal".into(),
        evolves_from: Some("Seed".into()),
        ..Default::default()
    };
    vec![
        base, primal, fabled, donor, lurker, vanilla, other_base, foxform, seed, sprout,
    ]
}

/// `blank` + this local catalog, A to act from slot A1 with rich anima.
fn board() -> recollect_core::state::GameState {
    let mut st = blank();
    st.active = Seat::A;
    st.active_slot = SeatSlot::A1;
    st.player_a.anima = 20;
    st
}

/// Drop a spirit of `card` for `owner` at `tile` (full printed stats from the
/// local catalog).
fn put(st: &mut recollect_core::state::GameState, tile: u8, card: u16, owner: Seat) {
    let d = cat().into_iter().find(|c| c.id == CardId(card)).unwrap();
    use recollect_core::state::Spirit;
    st.board[tile as usize].spirit = Some(Spirit {
        replacement_used: false,
        holding: false,
        face_down: false,
        is_token: false,
        placed_by: None,
        card: CardId(card),
        owner,
        attack: d.attack,
        defense: d.defense,
        hp: d.hp,
        hp_max: d.hp,
        fading: false,
        banished_by: None,
        intercepted_this_round: false,
        traits_stripped: false,
        traits_stripped_until: None,
        kw_grants: Vec::new(),
        no_engage_until: 0,
        throughline_done: false,
        copied_reach: None,
        fade_deadline: None,
    });
}

/// `eng` (from common) constructs with `test_catalog`; we need OUR catalog, so build
/// the engine directly from the state with the local catalog.
fn engine(st: recollect_core::state::GameState) -> recollect_core::Engine {
    recollect_core::Engine::from_state(st, 7, recollect_core::DrawPos(0), cat())
}

// ----------------------------------------------------------------------------
// decide_reclaim — the `sp.owner == actor && !sp.fading` guard (L351) and the
// `amount > 0` Anima gate (L400).
// ----------------------------------------------------------------------------

#[test]
fn reclaim_rejects_an_enemy_spirit() {
    // The tile holds B's spirit; A may not reclaim it (NotYourSpirit). Kills the
    // `sp.owner == actor` half of the L351 guard (`&&`→`||` would admit it).
    let mut st = board();
    put(&mut st, 12, VANILLA, Seat::B);
    let mut e = engine(st);
    assert_eq!(
        e.apply(Seat::A, Command::Reclaim { tile: 12 }),
        Err(Reject::NotYourSpirit)
    );
    assert!(
        !e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Reclaim { tile } if *tile == 12)),
        "an enemy spirit is never offered for reclaim"
    );
}

#[test]
fn reclaim_rejects_a_fading_spirit() {
    // A's own spirit, but already fading: reclaim is for STANDING spirits. Kills the
    // `!sp.fading` half of the L351 guard. (A fading spirit falls to the catch-all
    // `Some(_) => NotYourSpirit` arm.)
    let mut st = board();
    put(&mut st, 12, VANILLA, Seat::A);
    st.board[12].spirit.as_mut().unwrap().fading = true;
    let mut e = engine(st);
    assert_eq!(
        e.apply(Seat::A, Command::Reclaim { tile: 12 }),
        Err(Reject::NotYourSpirit)
    );
}

#[test]
fn reclaim_rejects_an_empty_tile() {
    let mut e = engine(board());
    assert_eq!(
        e.apply(Seat::A, Command::Reclaim { tile: 12 }),
        Err(Reject::TileEmpty)
    );
}

#[test]
fn reclaim_a_zero_cost_spirit_grants_no_anima_event() {
    // ⌊cost/2⌋ of a cost-0 spirit is 0 — the `if amount > 0` gate (L400) suppresses the
    // AnimaGained event. A cost-1 spirit (⌊1/2⌋ = 0) is the natural zero case. Kills the
    // boundary by asserting a reclaim with amount 0 emits NO AnimaGained, while a
    // higher-cost one does.
    let mut st = board();
    put(&mut st, 12, VANILLA, Seat::A); // Sheep, cost 1 ⇒ reclaim 0
    let mut e = engine(st);
    let evs = e.apply(Seat::A, Command::Reclaim { tile: 12 }).unwrap();
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritReclaimed { .. })),
        "the spirit is still reclaimed"
    );
    assert!(
        !evs.iter().any(|ev| matches!(ev, Event::AnimaGained { .. })),
        "a cost-1 spirit reclaims ⌊1/2⌋ = 0 anima — no AnimaGained event"
    );

    // Contrast: a cost-4 base reclaims 2 ⇒ an AnimaGained DOES fire.
    let mut st = board();
    put(&mut st, 12, BASE, Seat::A); // Cub, cost 4 ⇒ reclaim 2
    let mut e = engine(st);
    let evs = e.apply(Seat::A, Command::Reclaim { tile: 12 }).unwrap();
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::AnimaGained { amount: 2, .. })),
        "a cost-4 spirit reclaims 2 anima"
    );
}

// ----------------------------------------------------------------------------
// decide_set_orders — the `sp.owner == actor && !sp.fading` guard (L864).
// ----------------------------------------------------------------------------

#[test]
fn set_orders_rejects_an_enemy_spirit() {
    // Standing Orders is a free action over YOUR OWN standing spirit. An enemy
    // spirit (or empty tile) falls to TileEmpty. Kills the `sp.owner == actor` half.
    let mut st = board();
    put(&mut st, 12, VANILLA, Seat::B);
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::SetOrders {
                tile: 12,
                hold: true
            }
        ),
        Err(Reject::TileEmpty)
    );
    assert!(
        !e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::SetOrders { tile, .. } if *tile == 12)),
        "an enemy spirit is never offered Standing Orders"
    );
}

#[test]
fn set_orders_rejects_a_fading_spirit() {
    // A's own spirit, but fading — no orders for a spirit on its way out. Kills the
    // `!sp.fading` half.
    let mut st = board();
    put(&mut st, 12, VANILLA, Seat::A);
    st.board[12].spirit.as_mut().unwrap().fading = true;
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::SetOrders {
                tile: 12,
                hold: true
            }
        ),
        Err(Reject::TileEmpty)
    );
}

#[test]
fn set_orders_rejects_an_empty_tile() {
    let mut e = engine(board());
    assert_eq!(
        e.apply(
            Seat::A,
            Command::SetOrders {
                tile: 12,
                hold: true
            }
        ),
        Err(Reject::TileEmpty)
    );
}

// ----------------------------------------------------------------------------
// decide_reveal — the owner/face_down/fading guard (L795), the engage-tile
// bounds check (L790), the `no_engage_until >= round` restriction (L806), the
// reach gate (L810), and the engage-target guard (L814).
// ----------------------------------------------------------------------------

/// A's face-down lurker at `tile`, ready to reveal.
fn with_lurker(tile: u8) -> recollect_core::state::GameState {
    let mut st = board();
    put(&mut st, tile, LURKER, Seat::A);
    st.board[tile as usize].spirit.as_mut().unwrap().face_down = true;
    st
}

#[test]
fn reveal_rejects_an_enemy_spirit() {
    // The target must be YOUR OWN face-down spirit; an enemy's tile is TargetNotEnemy.
    // Kills the `sp.owner == actor` clause of the L795 guard.
    let mut st = board();
    put(&mut st, 12, LURKER, Seat::B);
    st.board[12].spirit.as_mut().unwrap().face_down = true;
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Reveal {
                tile: 12,
                engage: None
            }
        ),
        Err(Reject::TargetNotEnemy)
    );
}

#[test]
fn reveal_rejects_an_already_face_up_spirit() {
    // A standing, FACE-UP own spirit has nothing to reveal — `face_down` is false, so
    // it falls to the TargetNotEnemy catch-all. Kills the `sp.face_down` clause.
    let mut st = board();
    put(&mut st, 12, VANILLA, Seat::A); // face_up by construction
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Reveal {
                tile: 12,
                engage: None
            }
        ),
        Err(Reject::TargetNotEnemy)
    );
    assert!(
        !e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Reveal { tile, .. } if *tile == 12)),
        "a face-up spirit is never offered Reveal"
    );
}

#[test]
fn reveal_rejects_a_fading_lurker() {
    // A's own face-down spirit, but fading: the `!sp.fading` clause rejects it.
    let mut st = with_lurker(12);
    st.board[12].spirit.as_mut().unwrap().fading = true;
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Reveal {
                tile: 12,
                engage: None
            }
        ),
        Err(Reject::TargetNotEnemy)
    );
}

#[test]
fn reveal_rejects_an_out_of_bounds_engage_tile() {
    // The engage tile is bounds-checked (L790): an index past the board is BadTile.
    // Kills the `>=` boundary (a `<` would let the OOB index through to a panic/UB path).
    let mut e = engine(with_lurker(12));
    let n = e.state().board.len() as u8;
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Reveal {
                tile: 12,
                engage: Some(n)
            }
        ),
        Err(Reject::BadTile)
    );
}

#[test]
fn reveal_engage_rejects_a_restricted_lurker() {
    // "Don't Look": a lurker with `no_engage_until >= round` may step into the light but
    // NOT strike on the reveal — EngageRestricted. There is a real enemy in reach, so the
    // ONLY reason to reject is the restriction. Kills the L806 `>=` boundary.
    let mut st = with_lurker(12);
    put(&mut st, 13, VANILLA, Seat::B); // an enemy adjacent (Cross reach hits it)
    st.round = 2;
    st.board[12].spirit.as_mut().unwrap().no_engage_until = 2; // restricted THIS round
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Reveal {
                tile: 12,
                engage: Some(13)
            }
        ),
        Err(Reject::EngageRestricted)
    );
    // legal_commands agrees (the L1017 `no_engage_until < round` gate): the bare reveal
    // is offered, the engaging reveal is NOT. (Assert on the menu BEFORE applying the
    // bare reveal, which would flip the lurker face-up and drop the Reveal commands.)
    let menu = e.legal_commands(Seat::A);
    assert!(
        menu.contains(&Command::Reveal {
            tile: 12,
            engage: None
        }),
        "the bare reveal is in the menu"
    );
    assert!(
        !menu.contains(&Command::Reveal {
            tile: 12,
            engage: Some(13)
        }),
        "a restricted lurker's engaging reveal is NOT offered (L1017)"
    );
    // And the bare reveal applies — the restriction blocks only the strike, not stepping
    // into the light.
    assert!(
        e.apply(
            Seat::A,
            Command::Reveal {
                tile: 12,
                engage: None
            }
        )
        .is_ok(),
        "the restriction blocks only the strike, not stepping into the light"
    );

    // Contrast: an UNrestricted lurker (no_engage_until < round) IS offered the engage —
    // proving the gate, not a blanket suppression.
    let mut st2 = with_lurker(12);
    put(&mut st2, 13, VANILLA, Seat::B);
    st2.round = 2;
    st2.board[12].spirit.as_mut().unwrap().no_engage_until = 0; // free to strike
    let e2 = engine(st2);
    assert!(
        e2.legal_commands(Seat::A).contains(&Command::Reveal {
            tile: 12,
            engage: Some(13)
        }),
        "an unrestricted lurker's engaging reveal IS offered"
    );
}

#[test]
fn reveal_engage_rejects_a_target_out_of_reach() {
    // A real enemy, but OUTSIDE the lurker's Cross reach: TargetNotInReach. Kills the
    // `!reach.contains(target)` L810 guard (deleting the `!` would wrongly accept it).
    let mut st = with_lurker(12);
    // 12 = (2,2). A Cross reach hits the 4 orthogonal neighbours; (0,0)=tile 0 is far.
    put(&mut st, 0, VANILLA, Seat::B);
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Reveal {
                tile: 12,
                engage: Some(0)
            }
        ),
        Err(Reject::TargetNotInReach)
    );
}

#[test]
fn reveal_engage_rejects_an_empty_target_tile() {
    // In reach, but no spirit there: TileEmpty. Kills the engage-target None arm (L814
    // catch-all) — distinct from striking your own (next test).
    let mut e = engine(with_lurker(12));
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Reveal {
                tile: 12,
                engage: Some(13) // adjacent, but empty
            }
        ),
        Err(Reject::TileEmpty)
    );
}

#[test]
fn reveal_engage_rejects_striking_your_own_spirit() {
    // In reach, but the target is YOUR OWN spirit: TargetNotEnemy. Kills the
    // `e.owner != actor` clause of the L814 engage-target guard (`!=`→`==` or `&&`→`||`
    // would wrongly admit a friendly target).
    let mut st = with_lurker(12);
    put(&mut st, 13, VANILLA, Seat::A); // your own ally, adjacent
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Reveal {
                tile: 12,
                engage: Some(13)
            }
        ),
        Err(Reject::TargetNotEnemy)
    );
}

#[test]
fn reveal_engage_rejects_a_fading_enemy_target() {
    // In reach and an enemy, but already FADING: a fading spirit is not a legal strike
    // target (TargetNotEnemy). Kills the `!e.fading` clause of the L814 guard.
    let mut st = with_lurker(12);
    put(&mut st, 13, VANILLA, Seat::B);
    st.board[13].spirit.as_mut().unwrap().fading = true;
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Reveal {
                tile: 12,
                engage: Some(13)
            }
        ),
        Err(Reject::TargetNotEnemy)
    );
}

#[test]
fn reveal_engaging_a_valid_enemy_in_reach_strikes() {
    // The positive case the reject tests imply: an unrestricted lurker revealing into a
    // real, standing enemy in reach STRIKES. This pins the `e.owner != actor && !e.fading`
    // engage-target guard against the `→ false` mutation (which would make the arm never
    // match and WRONGLY reject every legal reveal-engage). A Struck event must fire.
    let mut st = with_lurker(12);
    put(&mut st, 13, VANILLA, Seat::B); // a standing enemy in Cross reach
    let mut e = engine(st);
    let evs = e
        .apply(
            Seat::A,
            Command::Reveal {
                tile: 12,
                engage: Some(13),
            },
        )
        .expect("revealing into a valid enemy is legal");
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritRevealed { .. })),
        "the lurker steps into the light"
    );
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::Struck {
                from_tile: 12,
                to_tile: 13,
                ..
            }
        )),
        "and it strikes the enemy on the reveal ({evs:?})"
    );
}

// ----------------------------------------------------------------------------
// decide_evolve — owner guards on the base (L452) and donor (L518), the
// engage-target guard (L564), and the Primal/Fabled fuel pairing.
// ----------------------------------------------------------------------------

/// A's FADING Cub at `tile` (ready for a Primal evolve), holding the Primal form.
fn fading_cub(tile: u8) -> recollect_core::state::GameState {
    let mut st = board();
    put(&mut st, tile, BASE, Seat::A);
    st.board[tile as usize].spirit.as_mut().unwrap().fading = true;
    st.player_a.hand = vec![CardId(PRIMAL)];
    st
}

/// A's HEALTHY Cub at `tile` (ready for a Fabled leap), holding the Fabled form,
/// with a donor ally present.
fn healthy_cub_with_donor(base_tile: u8, donor_tile: u8) -> recollect_core::state::GameState {
    let mut st = board();
    put(&mut st, base_tile, BASE, Seat::A); // healthy, did not arrive this turn
    put(&mut st, donor_tile, DONOR, Seat::A);
    st.player_a.hand = vec![CardId(FABLED)];
    st
}

#[test]
fn evolve_rejects_an_enemy_base() {
    // The base must be YOURS — an enemy spirit at the tile is TargetNotEnemy. Kills the
    // `sp.owner == actor` L452 guard.
    let mut st = board();
    put(&mut st, 12, BASE, Seat::B);
    st.board[12].spirit.as_mut().unwrap().fading = true;
    st.player_a.hand = vec![CardId(PRIMAL)];
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Evolve {
                tile: 12,
                form_hand: 0,
                fuel: None,
                engage: None
            }
        ),
        Err(Reject::TargetNotEnemy)
    );
}

#[test]
fn evolve_primal_rejects_a_healthy_base() {
    // A Primal form requires a FADING base (its self-fueled last becoming). A healthy
    // Cub cannot evolve into the Primal — EvolveConditionUnmet. Kills the Primal-path
    // `!base.fading` gate.
    let mut st = board();
    put(&mut st, 12, BASE, Seat::A); // healthy
    st.player_a.hand = vec![CardId(PRIMAL)];
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Evolve {
                tile: 12,
                form_hand: 0,
                fuel: None,
                engage: None
            }
        ),
        Err(Reject::EvolveConditionUnmet)
    );
}

#[test]
fn evolve_rejects_a_form_that_is_not_this_bases_becoming() {
    // Holding FOXFORM (Kit's form) over a Cub: not this base's becoming.
    let mut st = fading_cub(12);
    st.player_a.hand = vec![CardId(FOXFORM)];
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Evolve {
                tile: 12,
                form_hand: 0,
                fuel: None,
                engage: None
            }
        ),
        Err(Reject::EvolveConditionUnmet)
    );
}

#[test]
fn evolve_fabled_rejects_an_enemy_donor() {
    // The Fabled donor must be YOUR OWN non-token ally. Pointing `fuel` at an ENEMY
    // spirit is TargetNotEnemy. Kills the `sp.owner == actor` clause of the L518 donor
    // guard.
    let mut st = healthy_cub_with_donor(12, 7);
    // Replace the donor at 7 with an enemy one.
    put(&mut st, 7, DONOR, Seat::B);
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Evolve {
                tile: 12,
                form_hand: 0,
                fuel: Some(7),
                engage: None
            }
        ),
        Err(Reject::TargetNotEnemy)
    );
}

#[test]
fn evolve_fabled_rejects_a_token_donor() {
    // A TOKEN may not be spent as Fabled fuel (`!sp.is_token`). Kills the is_token clause
    // of the L518 guard (`&&`→`||` would admit it). A token donor ⇒ TargetNotEnemy.
    let mut st = healthy_cub_with_donor(12, 7);
    st.board[7].spirit.as_mut().unwrap().is_token = true;
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Evolve {
                tile: 12,
                form_hand: 0,
                fuel: Some(7),
                engage: None
            }
        ),
        Err(Reject::TargetNotEnemy)
    );
}

#[test]
fn evolve_primal_rejects_a_donor() {
    // A Primal takes NO donor — supplying `fuel` is BadTile. Pins the `fuel.is_some()`
    // Primal branch (L543).
    let mut st = fading_cub(12);
    put(&mut st, 7, DONOR, Seat::A);
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Evolve {
                tile: 12,
                form_hand: 0,
                fuel: Some(7),
                engage: None
            }
        ),
        Err(Reject::BadTile)
    );
}

#[test]
fn evolve_engage_rejects_a_friendly_target() {
    // The arrival strike target must be an ENEMY. A Primal evolve that engages YOUR OWN
    // adjacent ally is TargetNotEnemy. Kills the `sp.owner != actor` clause of the L564
    // engage-target guard.
    let mut st = fading_cub(12);
    put(&mut st, 13, VANILLA, Seat::A); // your own ally, in Cross reach
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Evolve {
                tile: 12,
                form_hand: 0,
                fuel: None,
                engage: Some(13)
            }
        ),
        Err(Reject::TargetNotEnemy)
    );
}

#[test]
fn evolve_engage_rejects_a_fading_enemy_target() {
    // A fading enemy is not a legal arrival-strike target (`!sp.fading`, L564) ⇒
    // TargetNotEnemy.
    let mut st = fading_cub(12);
    put(&mut st, 13, VANILLA, Seat::B);
    st.board[13].spirit.as_mut().unwrap().fading = true;
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Evolve {
                tile: 12,
                form_hand: 0,
                fuel: None,
                engage: Some(13)
            }
        ),
        Err(Reject::TargetNotEnemy)
    );
}

#[test]
fn evolve_engage_rejects_a_target_out_of_reach() {
    // An enemy outside the form's reach ⇒ TargetNotInReach (the L560 guard, shared with
    // reveal but exercised here on the evolve path).
    let mut st = fading_cub(12);
    put(&mut st, 0, VANILLA, Seat::B); // far corner, outside Cross reach of (2,2)
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Evolve {
                tile: 12,
                form_hand: 0,
                fuel: None,
                engage: Some(0)
            }
        ),
        Err(Reject::TargetNotInReach)
    );
}

#[test]
fn evolve_charges_form_cost_minus_half_the_base_cost() {
    // The evolution charge is `form.cost − ⌊base.cost/2⌋` (cost-aura adjusted): a Primal
    // Direwolf (cost 6) off a fading Cub (cost 4) costs 6 − ⌊4/2⌋ = 4. Pins the half-credit
    // subtraction — a dropped `− base.cost/2` would charge 6, and the `/2` becoming a `*`
    // would over-credit. We assert the exact AnimaSpent amount.
    let mut e = engine(fading_cub(12));
    let evs = e
        .apply(
            Seat::A,
            Command::Evolve {
                tile: 12,
                form_hand: 0,
                fuel: None,
                engage: None,
            },
        )
        .expect("a fading base evolves to its Primal");
    let spent: i64 = evs
        .iter()
        .filter_map(|ev| match ev {
            Event::AnimaSpent { amount, .. } => Some(*amount as i64),
            _ => None,
        })
        .sum();
    assert_eq!(
        spent, 4,
        "Direwolf(6) off Cub(4) charges 6 − ⌊4/2⌋ = 4 anima ({evs:?})"
    );
}

#[test]
fn evolve_at_zero_eff_cost_spends_no_anima() {
    // When `form.cost − ⌊base.cost/2⌋` floors at 0, the evolve is free: a Primal Sprout
    // (cost 1) off a fading Seed (cost 5) charges max(0, 1 − ⌊5/2⌋) = max(0, −1) = 0, so
    // NO AnimaSpent fires. Pins the `if eff_cost > 0` gate on the evolve path (a `>`→`>=`
    // would still suppress it, but a deleted gate would emit a spurious AnimaSpent{0}).
    let mut st = board();
    put(&mut st, 12, SEED, Seat::A);
    st.board[12].spirit.as_mut().unwrap().fading = true; // a fading base ⇒ Primal path
    st.player_a.hand = vec![CardId(SPROUT)];
    let mut e = engine(st);
    let evs = e
        .apply(
            Seat::A,
            Command::Evolve {
                tile: 12,
                form_hand: 0,
                fuel: None,
                engage: None,
            },
        )
        .expect("a fading Seed evolves to Sprout for free");
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritEvolved { .. })),
        "the base still becomes the form"
    );
    assert!(
        !evs.iter().any(|ev| matches!(ev, Event::AnimaSpent { .. })),
        "a zero-eff-cost evolve spends no anima ({evs:?})"
    );
}

// ----------------------------------------------------------------------------
// decide_devolve — the standing-Faded window guard (L706) and the half-cost
// anima boundary (L729).
// ----------------------------------------------------------------------------

/// A's standing-Faded Direwolf (Primal) at `tile`, inside its combat-fade window,
/// holding the Cub base for a recede.
fn faded_form(tile: u8) -> recollect_core::state::GameState {
    let mut st = board();
    put(&mut st, tile, PRIMAL, Seat::A);
    {
        let sp = st.board[tile as usize].spirit.as_mut().unwrap();
        sp.fading = true;
        sp.banished_by = Some(Seat::B);
        sp.fade_deadline = Some(st.round); // a combat fade — inside the window
    }
    st.player_a.hand = vec![CardId(BASE)];
    st
}

#[test]
fn devolve_rejects_a_healthy_form() {
    // Devolution is the rescue of a BANISHED form in its window. A healthy (non-fading)
    // form has nothing to recede from. Kills the L706 `!form.fading` clause (the
    // `||`→`&&` mutation would require BOTH to admit, changing the reject for a healthy
    // form with a deadline).
    let mut st = board();
    put(&mut st, 12, PRIMAL, Seat::A); // healthy form
    st.player_a.hand = vec![CardId(BASE)];
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Devolve {
                tile: 12,
                base_hand: 0
            }
        ),
        Err(Reject::DevolveConditionUnmet)
    );
}

#[test]
fn devolve_rejects_an_uncontested_fade_with_no_deadline() {
    // A fading form with NO `fade_deadline` (an uncontested Dusk fade, not a combat
    // banish) is outside the standing-Faded window. Kills the `form.fade_deadline.is_none()`
    // clause of the L706 guard.
    let mut st = faded_form(12);
    st.board[12].spirit.as_mut().unwrap().fade_deadline = None;
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Devolve {
                tile: 12,
                base_hand: 0
            }
        ),
        Err(Reject::DevolveConditionUnmet)
    );
}

#[test]
fn devolve_requires_exactly_the_half_cost_anima() {
    // ⌊form.cost/2⌋ = ⌊6/2⌋ = 3. With 2 anima the recede is rejected; with exactly 3 it
    // is allowed. Pins the L729 `anima < eff_cost` boundary (`<`→`<=` would reject the
    // exact-3 case that must succeed).
    let mut st = faded_form(12);
    st.player_a.anima = 2; // one short
    let mut e = engine(st);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Devolve {
                tile: 12,
                base_hand: 0
            }
        ),
        Err(Reject::NotEnoughAnima)
    );

    // Exactly the cost succeeds — the boundary is inclusive-affordable.
    let mut st = faded_form(12);
    st.player_a.anima = 3;
    let mut e = engine(st);
    assert!(
        e.apply(
            Seat::A,
            Command::Devolve {
                tile: 12,
                base_hand: 0
            }
        )
        .is_ok(),
        "exactly ⌊cost/2⌋ = 3 anima funds the recede"
    );
}

#[test]
fn devolve_a_zero_cost_form_spends_no_anima() {
    // ⌊form.cost/2⌋ for a cost-1 Sprout is 0 — the `if eff_cost > 0` gate (L732) suppresses
    // the AnimaSpent event. Pins that boundary (mirrors the reclaim zero-amount case).
    let mut st = board();
    put(&mut st, 12, SPROUT, Seat::A);
    {
        let sp = st.board[12].spirit.as_mut().unwrap();
        sp.fading = true;
        sp.banished_by = Some(Seat::B);
        sp.fade_deadline = Some(st.round);
    }
    st.player_a.hand = vec![CardId(SEED)];
    let mut e = engine(st);
    let evs = e
        .apply(
            Seat::A,
            Command::Devolve {
                tile: 12,
                base_hand: 0,
            },
        )
        .expect("a cost-1 form recedes for free");
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritDevolved { .. })),
        "the form still recedes"
    );
    assert!(
        !evs.iter().any(|ev| matches!(ev, Event::AnimaSpent { .. })),
        "⌊1/2⌋ = 0 anima — no AnimaSpent event ({evs:?})"
    );
}

// ----------------------------------------------------------------------------
// mulligan_window — the opening-window guards (L283 round/turn/spent, L300/L304
// the untouched-income checks).
// ----------------------------------------------------------------------------

/// A pristine round-1 opening for A (untouched: opening income, no footprint, not
/// yet mulliganed).
fn opening() -> recollect_core::state::GameState {
    let mut st = board();
    st.round = 1;
    st.active = Seat::A;
    st.active_slot = SeatSlot::A1;
    // The opening income is (1 + round).min(6) = 2 at round 1.
    st.player_a.anima = 2;
    st.player_a.glimpsed_this_turn = false;
    st.mulliganed = [false, false];
    st
}

#[test]
fn mulligan_offered_in_a_pristine_opening() {
    // Sanity anchor for the rejection tests: the pristine window DOES offer a mulligan.
    let mut e = engine(opening());
    assert!(
        e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Mulligan { .. })),
        "the opening window offers a mulligan"
    );
    assert!(
        e.apply(Seat::A, Command::Mulligan { seat: Seat::A })
            .is_ok()
    );
}

#[test]
fn mulligan_rejected_after_round_one() {
    // Round 2: outside the window. Kills the `state.round != 1` clause of L283.
    let mut st = opening();
    st.round = 2;
    st.player_a.anima = (1 + st.round).min(6); // keep income otherwise "untouched"
    let mut e = engine(st);
    assert_eq!(
        e.apply(Seat::A, Command::Mulligan { seat: Seat::A }),
        Err(Reject::MulliganUnavailable)
    );
    assert!(
        !e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Mulligan { .. })),
        "no mulligan offered after round 1"
    );
}

#[test]
fn mulligan_rejected_once_already_spent() {
    // The once-per-match mulligan is spent. Kills the `state.mulliganed[seat]` clause.
    let mut st = opening();
    st.mulliganed = [true, false];
    let mut e = engine(st);
    assert_eq!(
        e.apply(Seat::A, Command::Mulligan { seat: Seat::A }),
        Err(Reject::MulliganUnavailable)
    );
}

#[test]
fn mulligan_rejected_after_glimpsing() {
    // A Glimpse this turn touches the seat — the window closes. Kills the
    // `glimpsed_this_turn` early-return.
    let mut st = opening();
    st.player_a.glimpsed_this_turn = true;
    let mut e = engine(st);
    assert_eq!(
        e.apply(Seat::A, Command::Mulligan { seat: Seat::A }),
        Err(Reject::MulliganUnavailable)
    );
}

#[test]
fn mulligan_rejected_once_anima_was_spent() {
    // Anima below the opening income means the seat acted (a ritual/unwriting without a
    // board footprint). Kills the `p.anima != opening_income` clause (L300/L304: `==`→`!=`
    // flips the accept/reject).
    let mut st = opening();
    st.player_a.anima = 1; // below the opening income of 2
    let mut e = engine(st);
    assert_eq!(
        e.apply(Seat::A, Command::Mulligan { seat: Seat::A }),
        Err(Reject::MulliganUnavailable)
    );
}

#[test]
fn mulligan_rejected_with_a_spirit_footprint() {
    // A spirit of the seat's own on the board ⇒ the seat has acted; the window closes.
    // Exercises the `has_footprint` scan (the spirit-owner branch).
    let mut st = opening();
    put(&mut st, 12, VANILLA, Seat::A);
    let mut e = engine(st);
    assert_eq!(
        e.apply(Seat::A, Command::Mulligan { seat: Seat::A }),
        Err(Reject::MulliganUnavailable)
    );
}

#[test]
fn mulligan_rejected_with_a_terrain_footprint() {
    // A TERRAIN of the seat's own (a Landmark/Fabrication it placed) is equally a
    // footprint — the window closes. Kills the terrain-owner branch of the has_footprint
    // scan (`tr.owner == seat`, L304: `==`→`!=` would miss an own-terrain footprint). An
    // ENEMY terrain, by contrast, does NOT close A's window.
    use recollect_core::state::{Terrain, TerrainKind};
    let mut st = opening();
    st.board[12].terrain = Some(Terrain {
        card: CardId(VANILLA), // any id; only owner/presence matters here
        owner: Seat::A,
        kind: TerrainKind::Landmark,
        face_down: false,
    });
    let mut e = engine(st);
    assert_eq!(
        e.apply(Seat::A, Command::Mulligan { seat: Seat::A }),
        Err(Reject::MulliganUnavailable),
        "the seat's own terrain is a footprint"
    );

    // An ENEMY terrain is not A's footprint — the window stays open.
    let mut st = opening();
    st.board[12].terrain = Some(Terrain {
        card: CardId(VANILLA),
        owner: Seat::B,
        kind: TerrainKind::Landmark,
        face_down: false,
    });
    let mut e = engine(st);
    assert!(
        e.apply(Seat::A, Command::Mulligan { seat: Seat::A })
            .is_ok(),
        "an enemy's terrain does not close A's opening window"
    );
}

#[test]
fn mulligan_not_offered_on_the_other_seats_turn() {
    // It is B's turn. The DIRECT apply is gated by turn-ownership first (NotYourTurn,
    // which precedes the window check in `decide`). The `state.active != seat` clause of
    // `mulligan_window` is the one that keeps A's mulligan OUT of B's menu — assert that
    // (and that A's own menu, when it is not A's turn, never offers it).
    let mut st = opening();
    st.active = Seat::B;
    st.active_slot = SeatSlot::B1;
    st.player_b.anima = 2;
    let mut e = engine(st);
    assert_eq!(
        e.apply(Seat::A, Command::Mulligan { seat: Seat::A }),
        Err(Reject::NotYourTurn),
        "the turn-ownership check fires before the window"
    );
    assert!(
        !e.legal_commands(Seat::B)
            .iter()
            .any(|c| matches!(c, Command::Mulligan { seat } if *seat == Seat::A)),
        "A's mulligan is not in B's menu (the active != seat window guard)"
    );
}

// ----------------------------------------------------------------------------
// legal_commands — a couple of guards that decide which commands are admitted:
// the Fabrication-spring match guard (L995) and the lurker no-engage gate (L1017).
// ----------------------------------------------------------------------------

#[test]
fn legal_commands_offers_no_devolve_without_the_base_in_hand() {
    // The standing-Faded form is present and affordable, but the Cub base is NOT in hand
    // — so no Devolve is offered (the L1156 name match). Holding the base flips it on.
    let mut st = faded_form(12);
    st.player_a.hand = vec![CardId(VANILLA)]; // wrong card
    let e = engine(st);
    assert!(
        !e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Devolve { .. })),
        "no base in hand ⇒ no recede offered"
    );

    let mut st = faded_form(12);
    st.player_a.hand = vec![CardId(BASE)];
    let e = engine(st);
    assert!(
        e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Devolve { tile: 12, .. })),
        "the base in hand ⇒ the recede is offered"
    );
}

#[test]
fn finished_match_offers_no_commands() {
    // A finished match yields an EMPTY menu regardless of seat — the early return at the
    // top of legal_commands.
    let mut st = board();
    st.phase = Phase::Finished {
        result: recollect_core::MatchResult::Win(Seat::A),
        score_a: 0,
        score_b: 0,
    };
    let e = engine(st);
    assert!(
        e.legal_commands(Seat::A).is_empty() && e.legal_commands(Seat::B).is_empty(),
        "a finished match offers no commands"
    );
}
