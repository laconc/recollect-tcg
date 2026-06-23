//! The standing-Faded window makes Evolution reachable.
//!
//! A base banished in combat **lingers standing-Faded** until the END of its
//! owner's next turn, so the owner gets one Main to Primal-evolve it (a Primal needs
//! a base that is Fading **and** still standing). These tests prove the window is
//! reachable, that an unevolved base dissolves at the owner's turn-END (not start),
//! the round-12 immediate-dissolve exception, the own-turn deadline, and — the crux —
//! that the base survives INTO Main. See `design.md` §0.5 (the standing-Faded window)
//! + §5 (Evolution).
use crate::common::{blank, eng, put};
use recollect_core::Engine;
use recollect_core::state::{Command, Event, Phase};
use recollect_core::types::{CardDef, CardId, CardKind, Reach, Resonance, Seat, SeatSlot};

/// A base low enough that one B strike banishes it, its self-fueled Primal form,
/// and a hard-hitting B attacker. Real costs so the discounted charge is exercised.
fn cat() -> Vec<CardDef> {
    let base = CardDef {
        id: CardId(0),
        name: "Seedling".into(),
        cost: 2,
        attack: 10,
        defense: 0,
        hp: 20, // a single 30-Attack strike banishes it
        reach: Reach::Cross,
        resonance: Resonance::Harmony,
        kind: CardKind::Spirit,
        imprints: vec!["Bloom".into()],
        evolves_to: vec!["Worldtree".into()],
        ..Default::default()
    };
    let primal = CardDef {
        id: CardId(1),
        name: "Worldtree".into(),
        cost: 4,
        attack: 60,
        defense: 20,
        hp: 60,
        reach: Reach::Cross,
        resonance: Resonance::Harmony,
        kind: CardKind::Evolution,
        rarity: "Primal".into(),
        imprints: vec!["Bloom".into()],
        evolves_from: Some("Seedling".into()),
        ..Default::default()
    };
    let raider = CardDef {
        id: CardId(2),
        name: "Raider".into(),
        cost: 2,
        attack: 30, // banishes the 20-HP base outright
        defense: 0,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Fury,
        kind: CardKind::Spirit,
        ..Default::default()
    };
    vec![base, primal, raider]
}

/// Seat a Seedling base for A at `tile`, just **banished by B** (standing-Faded,
/// lingering): `fading`, `banished_by = Some(B)`, and the fade deadline of the
/// owner's next turn-end. It is currently B's turn (the banish happened on B's
/// turn — the common case). A holds the Worldtree form and the Anima to evolve.
fn banished_base_on_bs_turn(seed: u64) -> (Engine, u8) {
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(seed, cat(), deck.clone(), deck);
    let tile = 12u8;
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, tile, CardId(0), Seat::A);
        {
            let sp = st.board[tile as usize].spirit.as_mut().unwrap();
            sp.hp = 20;
            sp.hp_max = 20;
            sp.fading = true;
            sp.banished_by = Some(Seat::B);
            // Banished on B's turn (active B, owner A) ⇒ deadline = round + 1.
            sp.fade_deadline = Some(st.round + 1);
        }
        st.active = Seat::B;
        st.active_slot = SeatSlot::B1;
        st.player_a.anima = 20;
        st.player_a.hand = vec![CardId(1)]; // the Worldtree Primal form
        st.player_a.deck.clear();
        st.player_b.deck.clear();
    }
    (e, tile)
}

#[test]
fn the_window_is_reachable_a_banished_base_survives_into_main_and_primal_evolves() {
    // The whole point of D1: a base banished by the opponent is still standing-Faded
    // when its owner's Main begins, and the owner can Primal-evolve it there.
    let (mut e, tile) = banished_base_on_bs_turn(7);
    // B ends its turn → play passes to A. The Fade step at A's turn-START must NOT
    // dissolve the lingering base — it survives to A's turn-END.
    e.apply(Seat::B, Command::EndTurn).unwrap();
    assert_eq!(e.state().active, Seat::A, "now A's Main");
    let sp = e.state().board[tile as usize].spirit.as_ref().expect(
        "the banished base SURVIVED into A's Main (D1) — it did not dissolve at turn-start",
    );
    assert!(sp.fading, "and it is Fading — a valid Primal base");
    assert_eq!(sp.card, CardId(0), "still the Seedling base");

    // A Primal-evolves it in Main — the form card played onto the Fading base.
    assert!(
        e.legal_commands(Seat::A).iter().any(|c| matches!(
            c,
            Command::Evolve { tile: t, form_hand: 0, fuel: None, .. } if *t == tile
        )),
        "the lingering Fading base is OFFERED its Primal in A's Main"
    );
    let evs = e
        .apply(
            Seat::A,
            Command::Evolve {
                tile,
                form_hand: 0,
                fuel: None,
                engage: None,
            },
        )
        .expect("the Primal evolve resolves");
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritEvolved { to, .. } if *to == CardId(1))),
        "the base became its Primal (Worldtree)"
    );
    let evolved = e.state().board[tile as usize].spirit.as_ref().unwrap();
    assert_eq!(evolved.card, CardId(1));
    assert!(!evolved.fading, "evolution cleared Fading");
    assert_eq!(
        evolved.fade_deadline, None,
        "and cleared the linger deadline"
    );
    assert_eq!(evolved.hp, 60, "arrived at full HP");
}

#[test]
fn an_unevolved_lingering_base_dissolves_at_the_owners_turn_end_not_its_start() {
    let (mut e, tile) = banished_base_on_bs_turn(7);
    // → A's Main: the base stands (verified above).
    e.apply(Seat::B, Command::EndTurn).unwrap();
    assert!(
        e.state().board[tile as usize].spirit.is_some(),
        "stands at A's Main-start"
    );
    // A does NOT evolve it — A simply ends its turn. The dissolve fires HERE, at
    // A's turn-END, laying B's (the banisher's) impression.
    let evs = e.apply(Seat::A, Command::EndTurn).unwrap();
    assert!(
        evs.iter().any(
            |ev| matches!(ev, Event::SpiritDissolved { tile: t, impression: Seat::B } if *t == tile)
        ),
        "the unevolved base dissolves at A's turn-END, in B's color"
    );
    assert!(
        e.state().board[tile as usize].spirit.is_none(),
        "the base is gone after A's turn-end"
    );
    assert_eq!(
        e.state().board[tile as usize].impressions.first().copied(),
        Some(Seat::B),
        "the banisher's impression remains"
    );
}

#[test]
fn the_crux_the_base_is_standing_through_all_of_mains_actions_then_dissolves_at_end() {
    // The timing crux: the Faded base must be present for the ENTIRE Main, not just
    // its first instant. We end B's turn (→ A's Main), then drive several legal A
    // actions and assert the base is still standing before A ends the turn.
    let (mut e, tile) = banished_base_on_bs_turn(7);
    e.apply(Seat::B, Command::EndTurn).unwrap();
    assert_eq!(e.state().active, Seat::A);
    // Take a couple of A actions OTHER than evolving (the base must outlive them).
    // A has Anima and a board; any legal non-evolve, non-endturn command will do.
    for _ in 0..2 {
        let next = e.state().active;
        let cmd = e
            .legal_commands(next)
            .into_iter()
            .find(|c| !matches!(c, Command::EndTurn | Command::Evolve { .. }));
        match cmd {
            Some(c) => {
                e.apply(next, c).unwrap();
                assert!(
                    e.state().board[tile as usize]
                        .spirit
                        .as_ref()
                        .map(|s| s.fading)
                        .unwrap_or(false),
                    "the base stands Faded THROUGH A's Main actions (the crux)"
                );
            }
            None => break,
        }
    }
    // Only now, at A's turn-END, does it dissolve.
    assert!(
        e.state().board[tile as usize].spirit.is_some(),
        "still standing right up to A's turn-end"
    );
    e.apply(Seat::A, Command::EndTurn).unwrap();
    assert!(
        e.state().board[tile as usize].spirit.is_none(),
        "dissolved at A's turn-end"
    );
}

#[test]
fn round_12_a_banished_base_lingers_then_dissolves_before_scoring_banisher_scores() {
    // The final-round REFINEMENT (design §0.5): on round 12 there is no next owner
    // turn to host a Primal-evolve window — but a spirit banished here does NOT
    // vanish on defeat. It lingers standing-Faded through the rest of round 12 and
    // dissolves at the END of the round, in the Nightfall `finish` step BEFORE
    // scoring, laying the banisher's impression so the OPPONENT (the banisher)
    // scores the tile — not the faded spirit, not the owner. Driven through real
    // combat so the banish goes through `banish_or_replace` (the round-12 chokepoint).
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat(), deck.clone(), deck);
    let base = 12u8;
    let raider_tile = 7u8; // Cross-adjacent to 12
    {
        let st = e.state_mut_for_test();
        st.round = st.rules.last_round; // the final round (12)
        recollect_core::test_support::put_spirit(st, base, CardId(0), Seat::A);
        {
            let sp = st.board[base as usize].spirit.as_mut().unwrap();
            sp.hp = 20;
            sp.hp_max = 20;
        }
        // B's Raider already on the board, adjacent, and it is B's turn — B engages
        // the base directly. Clear the rest of the board so the only scoring tile in
        // play is the contested one (a clean assertion on who holds it).
        recollect_core::test_support::put_spirit(st, raider_tile, CardId(2), Seat::B);
        st.board[raider_tile as usize]
            .spirit
            .as_mut()
            .unwrap()
            .attack = 30;
        st.active = Seat::B;
        st.active_slot = SeatSlot::B1;
        st.moved_this_turn.clear();
        st.player_a.hand.clear();
        st.player_b.hand.clear();
        st.player_a.deck.clear();
        st.player_b.deck.clear();
    }
    // B's Raider engages A's base — 30 Attack vs 20 HP ⇒ banished. On round 12 this
    // does NOT dissolve in the same resolution — the base lingers standing-Faded.
    let evs = e.resolve_engage_for_test(raider_tile, base);
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritBecameFading { tile, .. } if *tile == base)),
        "the base was banished in combat"
    );
    assert!(
        !evs.iter()
            .any(|ev| matches!(ev, Event::SpiritDissolved { tile, .. } if *tile == base)),
        "round 12: it does NOT dissolve immediately — it lingers standing-Faded"
    );
    let sp = e.state().board[base as usize]
        .spirit
        .as_ref()
        .expect("the base STANDS Faded through the rest of round 12");
    assert!(sp.fading, "and it is Fading (a body that no longer acts)");
    assert_eq!(
        e.state().board[base as usize].impressions.first().copied(),
        None,
        "no impression yet — it has not dissolved, so the tile is not scored to anyone"
    );

    // B ends its turn → on round 12 the Nightfall `finish` fires. The lingering base
    // dissolves there, BEFORE scoring, laying B's (the banisher's) impression. The
    // match scores with B holding the tile.
    let evs = e.apply(Seat::B, Command::EndTurn).unwrap();
    // Ordering proof: the dissolve precedes the MatchEnded in the event stream.
    let diss = evs.iter().position(
        |ev| matches!(ev, Event::SpiritDissolved { tile, impression: Seat::B } if *tile == base),
    );
    let ended = evs
        .iter()
        .position(|ev| matches!(ev, Event::MatchEnded { .. }));
    assert!(
        diss.is_some() && ended.is_some() && diss < ended,
        "the lingering base dissolves (in B's color) BEFORE the match scores"
    );
    assert!(
        e.state().board[base as usize].spirit.is_none(),
        "the base is gone after Nightfall"
    );
    // The banisher holds the tile: B scores it, not A (the owner) and not a faded body.
    match e.state().phase {
        Phase::Finished {
            result,
            score_a,
            score_b,
            ..
        } => {
            assert_eq!(
                score_a, 0,
                "A does not score the tile its banished base stood on"
            );
            assert!(
                score_b >= 1,
                "B (the banisher) scores the tile via its impression"
            );
            assert_eq!(result, recollect_core::MatchResult::Win(Seat::B));
        }
        _ => panic!("round 12 ended → the match is Finished"),
    }
}

#[test]
fn banished_on_the_owners_own_turn_survives_to_the_next_owner_turn_end() {
    // A base banished on the OWNER's own turn (e.g. an overwriter arriving spent, or
    // a Promise redirect) must skip the current turn's end and dissolve at the
    // owner's NEXT turn-end — it, too, gets a full Main to evolve in. We stamp the
    // own-turn deadline (round + 1) and walk the turns.
    let mut st = blank();
    let tile = 12u8;
    put(&mut st, tile, 0, Seat::A, Some(20));
    {
        let sp = st.board[tile as usize].spirit.as_mut().unwrap();
        sp.fading = true;
        sp.banished_by = Some(Seat::B);
        // Banished on A's OWN turn (active A, owner A) ⇒ deadline = round + 1.
        sp.fade_deadline = Some(st.round + 1);
    }
    st.active = Seat::A;
    st.active_slot = SeatSlot::A1;
    st.player_a.deck.clear();
    st.player_b.deck.clear();
    let start_round = st.round;
    let mut e = eng(st, 7);

    // A ends its OWN turn (round start_round). The base is NOT yet due (deadline is
    // next round) — it survives.
    e.apply(Seat::A, Command::EndTurn).unwrap();
    assert!(
        e.state().board[tile as usize].spirit.is_some(),
        "survives A's own turn-end (banished on A's own turn)"
    );
    // B's turn passes (the base is A's — B's turn-end never dissolves it).
    e.apply(Seat::B, Command::EndTurn).unwrap();
    assert!(
        e.state().board[tile as usize].spirit.is_some(),
        "survives B's turn and into A's NEXT Main"
    );
    assert_eq!(e.state().active, Seat::A, "A's next turn");
    assert!(e.state().round > start_round, "a round has advanced");
    // Now A's NEXT turn-end dissolves it (deadline reached).
    e.apply(Seat::A, Command::EndTurn).unwrap();
    assert!(
        e.state().board[tile as usize].spirit.is_none(),
        "dissolves at the owner's NEXT turn-end"
    );
}

#[test]
fn there_is_no_turn_start_fade_a_banished_base_survives_flow_into_main() {
    // The Fade phase moved to turn-END (the turn is Flow → Main → Fade) and the Dusk is
    // instant, so there is **no turn-START fade**: a base banished on the opponent's turn
    // is still standing-Faded after its owner's Flow (income + draw), present for the
    // whole Main. (The dissolve happens only at this turn's END if unredeemed — covered
    // by the turn-END tests above.) A base banished on B's turn (owner A) carries
    // `fade_deadline = round` deadline = the imminent... no: owner A, active B ⇒ round+1,
    // so it survives A's Flow into A's Main. We give A a card to draw so the Flow runs.
    let mut st = blank();
    let tile = 12u8;
    put(&mut st, tile, 0, Seat::A, Some(20));
    {
        let sp = st.board[tile as usize].spirit.as_mut().unwrap();
        sp.fading = true;
        sp.banished_by = Some(Seat::B);
        sp.fade_deadline = Some(st.round + 1); // banished on B's turn, owner A
    }
    st.active = Seat::B;
    st.active_slot = SeatSlot::B1;
    st.player_a.deck = vec![CardId(0)]; // a card so A's Flow draws (the Flow really runs)
    st.player_b.deck.clear();
    let mut e = eng(st, 7);
    e.apply(Seat::B, Command::EndTurn).unwrap();
    assert_eq!(e.state().active, Seat::A, "now A's turn");
    let sp = e.state().board[tile as usize].spirit.as_ref().expect(
        "the banished base SURVIVED A's Flow (no turn-start Fade) — it stands into A's Main",
    );
    assert!(sp.fading, "still Fading");
    assert!(
        e.state().player_a.hand.contains(&CardId(0)),
        "and A's Flow ran (the drawn card is in hand) — the base outlived the turn opening"
    );
}

// ── Reachability in the large: evolution actually FIRES in real playouts ──

/// A self-contained catalog where Primal evolution is *the* obvious line: a fragile
/// base that the opponent will banish, whose Primal it can then play. Both seats
/// share it. Returns (catalog, deck).
fn playout_kit() -> (Vec<CardDef>, Vec<CardId>) {
    let c = cat();
    // Deck heavy on the base + its Primal form so a greedy policy reaches the line.
    let deck: Vec<CardId> = (0..20)
        .map(|i| if i % 2 == 0 { CardId(0) } else { CardId(1) })
        .collect();
    (c, deck)
}

/// Drive a match with a simple but *playing* policy: each turn, **evolve** if the
/// window is open (the headline line); else **play a spirit engaging an enemy**
/// (manufacturing the banishes that open the window); else play a spirit; else end
/// the turn. This is enough to actually contest the board — unlike a first-legal
/// policy, which would pick the early `EndTurn` and play an empty game. Returns the
/// number of `SpiritEvolved` events seen across the match.
fn count_evolutions(seed: u64, cap: usize) -> usize {
    let (cat, deck) = playout_kit();
    let (mut e, _) = Engine::new(seed, cat, deck.clone(), deck);
    let mut evolutions = 0usize;
    for _ in 0..cap {
        if matches!(e.state().phase, Phase::Finished { .. }) {
            break;
        }
        let seat = e.state().active;
        let legal = e.legal_commands(seat);
        let cmd = legal
            .iter()
            .find(|c| matches!(c, Command::Evolve { .. }))
            .or_else(|| {
                legal.iter().find(|c| {
                    matches!(
                        c,
                        Command::PlaySpirit {
                            engage: Some(_),
                            ..
                        }
                    )
                })
            })
            .or_else(|| {
                legal
                    .iter()
                    .find(|c| matches!(c, Command::PlaySpirit { engage: None, .. }))
            })
            .or_else(|| legal.iter().find(|c| matches!(c, Command::EndTurn)))
            .or_else(|| legal.first())
            .cloned()
            .expect("some command is always legal");
        let evs = e.apply(seat, cmd).expect("legal command applies");
        evolutions += evs
            .iter()
            .filter(|ev| matches!(ev, Event::SpiritEvolved { .. }))
            .count();
    }
    evolutions
}

#[test]
fn evolution_fires_in_real_playouts_not_three_in_one_eighty() {
    // Across a battery of seeded playouts, Primal evolution must actually HAPPEN. We
    // require evolutions in a healthy majority of seeds (a deterministic, seed-stable
    // lower bound) — the standing-Faded window keeps it reachable in real play.
    let seeds = 0..60u64;
    let mut with_evo = 0usize;
    let mut total = 0usize;
    for s in seeds {
        let n = count_evolutions(s, 4000);
        total += n;
        if n > 0 {
            with_evo += 1;
        }
    }
    assert!(
        with_evo >= 30,
        "Primal evolution should fire in a healthy share of playouts (got {with_evo}/60 seeds with ≥1 evolution, {total} total) — the standing-Faded window makes it reachable"
    );
}
