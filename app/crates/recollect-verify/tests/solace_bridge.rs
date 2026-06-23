//! Verification routing: the fast CI slice of the stateright model-check. The full
//! sweep is the `solace-modelcheck` binary; this gate explores a smaller bounded
//! frontier — for the Solace PvE, the 1v1, AND the 2v2 spaces — on every test
//! run, so a regression that breaks an invariant under exhaustive exploration fails
//! CI, not just the nightly bin. Reuses the one model definition (`recollect_verify::model`).
use recollect_core::cards::canon_catalog;
use recollect_core::state::Command;
use recollect_core::types::CardKind;
use recollect_core::{Engine, Seat};
use recollect_verify::model::{EngineModel, Mode};

fn bounded_frontier_states(solace: bool, seed: u64) -> usize {
    let catalog = canon_catalog();
    let model = EngineModel {
        deck_a: EngineModel::cheap_deck(&catalog, 3, false),
        deck_b: EngineModel::cheap_deck(&catalog, 3, solace),
        catalog,
        seed,
        max_round: 2,
        mode: Mode::OneVsOne,
        init_override: None,
    };
    model.run(3_000) // asserts every property on every reachable state
}

#[test]
fn solace_pve_bounded_frontier_holds_every_invariant() {
    assert!(
        bounded_frontier_states(true, 12345) > 100,
        "explored a meaningful Solace PvE frontier"
    );
}

#[test]
fn one_v_one_bounded_frontier_holds_every_invariant() {
    // The seed is a LARGE, distinctive value on purpose: the no-seed-leak property does a
    // SUBSTRING scan for `seed.to_string()` across every state/view/event, so a small seed
    // can false-positive when its digits coincide with a small game integer — e.g. seed 99
    // matched `CardId(399)` (a Stray that surfaces) once the catalog grew. A 7-digit seed
    // disjoint from every card id / HP / tile / round value avoids the coincidence while
    // still exploring a real frontier. (The Solace/2v2/devolution seeds are likewise large.)
    assert!(
        bounded_frontier_states(false, 5_000_111) > 100,
        "explored a meaningful 1v1 frontier"
    );
}

/// The 2v2 four-slot path under formal coverage — the same bounded BFS over the
/// 6×6 telling (init via `new_2v2_with_opener`, actions from the active slot's team,
/// redaction from all four slots via `view_for_slot`). The 6×6 board × four hands
/// branches hard, so the bound is TIGHT (3-card decks, round ≤ 2); even so the frontier
/// is non-trivial, and EVERY property (validity, liveness, determinism, no-seed-leak,
/// redaction, abandonment) is asserted on every reachable 2v2 state, alongside the two
/// 1v1 modes.
#[test]
fn two_v_two_bounded_frontier_holds_every_invariant() {
    let catalog = canon_catalog();
    let model = EngineModel {
        deck_a: EngineModel::cheap_deck(&catalog, 3, false),
        deck_b: EngineModel::cheap_deck(&catalog, 3, false),
        catalog,
        seed: 24680,
        max_round: 2,
        mode: Mode::TwoVsTwo,
        init_override: None,
    };
    // A meaningful frontier (the four-slot rotation + 6×6 placements branch quickly),
    // with every invariant asserted on each reachable state.
    assert!(
        model.run(3_000) > 50,
        "explored a meaningful 2v2 frontier with every invariant holding"
    );
}

/// 2v2-rotation guard: the 2v2 model genuinely drives the four-slot rotation (not a degenerate
/// 1v1-shaped run). The opening state is a real 6×6 telling with `active_slot = A1`,
/// and the acting seat the model keys on is that slot's team — so the BFS explores the
/// A1→B1→A2→B2 turn order. (If a regression collapsed 2v2 init to a 1v1 board, this
/// fails before the always-properties could pass vacuously over a wrong shape.)
#[test]
fn two_v_two_model_opens_a_real_team_board() {
    use recollect_core::types::SeatSlot;
    let catalog = canon_catalog();
    let deck = EngineModel::cheap_deck(&catalog, 3, false);
    let (e, _) = Engine::new_2v2_with_opener(
        24680,
        catalog,
        [deck.clone(), deck.clone(), deck.clone(), deck],
        SeatSlot::A1,
        [recollect_core::types::Faction::Lorekeeper; 2],
    );
    assert!(e.state().is_2v2(), "the 2v2 init is a four-slot telling");
    assert_eq!(e.state().board_w, 6, "on the 6×6 board");
    assert_eq!(e.state().active_slot, SeatSlot::A1, "A1 opens");
    // The active slot's team is the seat the model keys legal_commands/apply on.
    assert_eq!(e.state().active_slot.team(), Seat::A);
    assert!(
        !e.legal_commands(Seat::A).is_empty(),
        "the opener has a real legal menu the BFS expands"
    );
}

/// Mulligan (§5) is genuinely IN the explored opening state space — so the
/// model's "a mulligan reshuffles cleanly and never leaks the hand" property is
/// not vacuously true. The bridge BFS begins at the opening (`init_states`), where
/// the opener's `legal_commands` offers the mulligan; this confirms that frontier
/// edge exists with the same cheap decks the model-check uses. (If a future change
/// stopped offering the mulligan in the opening, this fails — and the always-property
/// would otherwise silently pass over zero mulligan states.)
#[test]
fn mulligan_is_reachable_in_the_opening_frontier() {
    let catalog = canon_catalog();
    let deck = EngineModel::cheap_deck(&catalog, 3, false);
    let (e, _) = Engine::new(99, catalog, deck.clone(), deck);
    assert!(
        e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Mulligan { .. })),
        "the opener's opening window offers the mulligan the model-check then exercises"
    );
}

/// Glimpse (§5) is genuinely IN the explored state space — so the model's "a glimpse
/// burns then keeps-or-bottoms cleanly and never leaks a private card" property is not
/// vacuously true. From the opening, `Glimpse` is offered (a non-empty hand to burn AND a
/// non-empty page to peek), and applying it opens the `PendingChoice::GlimpseBurn` (one
/// option per hand card); paying the burn opens the `PendingChoice::Glimpse` with the
/// two keep/bottom options the model-check then exercises on every reachable
/// Glimpse-pending state. (If a future change stopped offering Glimpse, or skipped the
/// burn, this fails — and the always-property would otherwise silently pass over zero
/// Glimpse states.)
#[test]
fn glimpse_is_reachable_in_the_frontier() {
    let catalog = canon_catalog();
    // A page deep enough that cards remain after the opening 5-card draw (the 3-card
    // model-check decks empty into the opening hand; a Glimpse there has no top to
    // see). The model-check still reaches Glimpse with its small decks post-mulligan
    // (the reshuffle leaves a 1-card page) — this just exercises the same edge directly.
    let deck = EngineModel::cheap_deck(&catalog, 8, false);
    let (mut e, _) = Engine::new(99, catalog.clone(), deck.clone(), deck);
    // The opener is offered Glimpse (`Command::Glimpse`), having both a non-empty hand
    // to burn and a non-empty page to peek.
    assert!(
        !e.state().player(Seat::A).hand.is_empty(),
        "the opener has a hand to burn"
    );
    assert!(
        e.legal_commands(Seat::A).contains(&Command::Glimpse),
        "the opener with a hand and a page is offered Glimpse"
    );
    e.apply(Seat::A, Command::Glimpse).unwrap();
    // Step 1 — the burn choice opens (one option per burnable hand card).
    let burnable = match e.state().pending_choice {
        Some(recollect_core::state::PendingChoice::GlimpseBurn { ref burnable, .. }) => {
            burnable.len()
        }
        _ => panic!("applying Glimpse opens the burn choice the model-check exercises"),
    };
    let choose: Vec<_> = e
        .legal_commands(Seat::A)
        .into_iter()
        .filter(|c| matches!(c, Command::Choose { .. }))
        .collect();
    assert_eq!(choose.len(), burnable, "one Choose per burnable hand card");
    // Pay the burn — the keep-or-bottom choice opens with exactly two options.
    e.apply(Seat::A, Command::Choose { index: 0 }).unwrap();
    assert!(
        matches!(
            e.state().pending_choice,
            Some(recollect_core::state::PendingChoice::Glimpse { .. })
        ),
        "paying the burn opens the keep-or-bottom choice"
    );
    let choose: Vec<_> = e
        .legal_commands(Seat::A)
        .into_iter()
        .filter(|c| matches!(c, Command::Choose { .. }))
        .collect();
    assert_eq!(
        choose,
        vec![Command::Choose { index: 0 }, Command::Choose { index: 1 }],
        "exactly two options — keep (0) and bottom (1)"
    );

    // And prove it with the EXACT 3-card decks the bounded-frontier BFS uses: the
    // opening deals the whole page into the hand (empty deck, no Glimpse), but the
    // opening mulligan reshuffles to a 1-card page — and THERE Glimpse is offered and
    // opens the burn (then the Glimpse) the always-property checks. This nails the
    // property to the model-check's own decks (not the deeper page above), so it
    // isn't vacuous.
    let small = EngineModel::cheap_deck(&catalog, 3, false);
    let (mut e, _) = Engine::new(99, catalog, small.clone(), small);
    assert!(
        e.state().player(Seat::A).deck.is_empty(),
        "the 3-card page empties into the opening hand"
    );
    e.apply(Seat::A, Command::Mulligan { seat: Seat::A })
        .unwrap();
    assert!(
        !e.state().player(Seat::A).deck.is_empty()
            && !e.state().player(Seat::A).hand.is_empty()
            && e.legal_commands(Seat::A).contains(&Command::Glimpse),
        "post-mulligan the 1-card page + hand offers Glimpse — reachable in the BFS frontier"
    );
    e.apply(Seat::A, Command::Glimpse).unwrap();
    assert!(
        matches!(
            e.state().pending_choice,
            Some(recollect_core::state::PendingChoice::GlimpseBurn { .. })
        ),
        "the model-check's own decks reach a Glimpse-pending (burn) state"
    );
    e.apply(Seat::A, Command::Choose { index: 0 }).unwrap();
    assert!(
        matches!(
            e.state().pending_choice,
            Some(recollect_core::state::PendingChoice::Glimpse { .. })
        ),
        "and on through the burn to the keep-or-bottom state"
    );
}

/// Uniform-seat Solace: seat B drafts an actual Solace-faction deck through
/// the same `cheap_deck` path A uses — no flag, no director. This guards the "both
/// modes differ ONLY by B's faction" claim from going vacuous: if a catalog change
/// emptied the cheap Solace bodies, the Solace deck would silently fall back to
/// nothing (or the wrong kind) and the PvE run would degenerate into a second 1v1.
/// The turn mechanics are uniform (verified by the shared `EngineModel` running
/// both modes); what THIS asserts is that the Solace mode is genuinely Solace.
#[test]
fn solace_mode_drafts_a_real_solace_deck() {
    let catalog = canon_catalog();
    let solace = EngineModel::cheap_deck(&catalog, 3, true);
    let lorekeeper = EngineModel::cheap_deck(&catalog, 3, false);
    assert!(
        solace.len() >= 3,
        "the Solace faction must field cheap bodies for the PvE frontier"
    );
    // Every Solace-deck card is an Unwritten/IllIntent body — never a Lorekeeper spirit.
    for id in &solace {
        let def = catalog
            .iter()
            .find(|c| &c.id == id)
            .expect("card in catalog");
        assert!(
            matches!(def.kind, CardKind::Unwritten | CardKind::IllIntent),
            "Solace deck card {} is {:?}, not a Solace faction body",
            def.name,
            def.kind
        );
    }
    // And the two factions field DIFFERENT cards — B is not silently A's deck.
    assert!(
        solace.iter().all(|id| !lorekeeper.contains(id)),
        "the Solace and Lorekeeper cheap decks must be disjoint factions"
    );
}

/// Devolution (§5) is genuinely IN the explored state space — so EVERY model property
/// (state validity, liveness, determinism, no-seed-leak, redaction, abandonment) is
/// asserted across the **Devolve** action (the rescue) and the states it reaches.
///
/// Pure genesis-BFS would take far too long to reach a standing-Faded form with its
/// base in hand (it needs a banish→fade→evolve sequence), so we **seed that
/// configuration directly** as the BFS's initial snapshot (`init_override`): a Primal
/// form for seat A, banished and standing-Faded in A's Main (§0.5 window), with its base
/// in A's hand and the Anima to pay. From that root, `legal_commands` offers the Devolve,
/// the BFS expands it, and the properties hold on every resulting state — closing the gap
/// where the recede had no formal coverage. (RED if the recede leaked the played base
/// pre-reveal, diverged on re-run, or stranded the telling with no legal command.)
#[test]
fn devolution_is_reachable_in_the_frontier_and_holds_every_invariant() {
    use recollect_core::state::Phase;
    use recollect_core::types::{CardId, Seat, SeatSlot};
    let catalog = canon_catalog();
    // A real Lorekeeper line from the canon: Cloudling (base) → Stormswell (Primal).
    let id = |n: &str| catalog.iter().find(|c| c.name == n).map(|c| c.id).unwrap();
    let (base, primal) = (id("Cloudling"), id("Stormswell"));
    // Build an engine, then surgically stage the standing-Faded Primal + base-in-hand.
    let deck: Vec<CardId> = vec![base; 20];
    let (mut e, _) = Engine::new(777, catalog.clone(), deck.clone(), deck);
    let tile = 12u8;
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, tile, primal, Seat::A);
        {
            let sp = st.board[tile as usize].spirit.as_mut().unwrap();
            sp.fading = true;
            sp.banished_by = Some(Seat::B);
            // Banished on B's turn, owner A ⇒ deadline next round (stays in-window now).
            sp.fade_deadline = Some(st.round + 1);
        }
        st.active = Seat::A;
        st.active_slot = SeatSlot::A1;
        st.player_a.anima = 12;
        st.player_a.hand = vec![base]; // the base to recede to
        // Tiny pages so the post-Devolve frontier stays finite + small.
        st.player_a.deck.truncate(2);
        st.player_b.deck.truncate(2);
        st.player_b.hand.clear();
    }
    // The recede IS legal from this seeded root.
    assert!(
        e.legal_commands(Seat::A)
            .iter()
            .any(|c| matches!(c, Command::Devolve { tile: t, .. } if *t == tile)),
        "the seeded standing-Faded Primal offers the Devolve (the model root is well-formed)"
    );
    assert!(!matches!(e.state().phase, Phase::Finished { .. }));

    let snap = serde_json::to_string(&e.snapshot()).expect("snapshot serializes");
    let model = EngineModel {
        deck_a: vec![base],
        deck_b: vec![base],
        catalog,
        seed: 777,
        max_round: 3,
        mode: Mode::OneVsOne,
        init_override: Some(snap),
    };
    // BFS from the seeded root, asserting every property on every reachable state —
    // including the Devolve action and the states it produces.
    assert!(
        model.run(3_000) > 5,
        "explored a Devolve-reachable frontier with every invariant holding"
    );
}
