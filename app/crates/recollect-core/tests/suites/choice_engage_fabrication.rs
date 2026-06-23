//! Choice-driven engage / fade / fabrication / resource effects (split from target_choice.rs).
//! Free target-choice rituals: a played card opens a `PendingChoice::Target` to
//! pick any spirit board-wide (no adjacency). Pins Hold Fast — "a spirit +20
//! Defense this round" — which routes through the choice seam with source=None.
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::engine::combat_stats_for_test;
use recollect_core::state::{Command, PendingChoice};
use recollect_core::test_support::put_spirit;
use recollect_core::types::{CardId, Seat};

fn id_of(name: &str) -> CardId {
    canon_catalog()
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.id)
        .unwrap_or_else(|| panic!("card not found: {name}"))
}

#[test]
fn the_fog_of_elsewhere_bounces_only_a_cheap_enemy() {
    // TargetEnemySpirit/Bounce gated by CostAtMost{3}: only the Cost-2 enemy is a
    // legal target; the Cost-4 enemy is filtered out. The chosen one returns to hand.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::B); // Cost 2 — eligible
        put_spirit(st, 13, id_of("Aurora Elk"), Seat::B); // Cost ≥4 — filtered out
        st.player_a.hand = vec![id_of("The Fog of Elsewhere")];
        st.player_a.anima = 9;
    }
    let b_hand0 = e.state().player(Seat::B).hand.len();
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("The Fog of Elsewhere is castable");
    e.apply(Seat::A, cast).unwrap();
    // Exactly one target offered (the Cost-2 enemy at 12).
    let choices: Vec<_> = e
        .legal_commands(Seat::A)
        .into_iter()
        .filter(|c| matches!(c, Command::Choose { .. }))
        .collect();
    assert_eq!(choices.len(), 1, "only the Cost-2 enemy is eligible");
    e.apply(Seat::A, choices.into_iter().next().unwrap())
        .unwrap();
    assert!(
        e.state().board[12].spirit.is_none(),
        "Fog: the cheap enemy left the board"
    );
    assert_eq!(
        e.state().player(Seat::B).hand.len(),
        b_hand0 + 1,
        "Fog: it returned to its owner's hand"
    );
    assert!(
        e.state().board[13].spirit.is_some(),
        "Fog: the Cost-4 enemy was never eligible and stays put"
    );
}

#[test]
fn dont_look_marks_a_chosen_enemy_unable_to_engage() {
    // OnPlay/TargetEnemySpirit/Restrict(Engage)/NextRound: the chosen enemy is
    // flagged through the end of next round (round R played ⇒ until R+1).
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::B);
        st.player_a.hand = vec![id_of("Don't Look")];
        st.player_a.anima = 9;
    }
    let r = e.state().round;
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Don't Look is castable");
    e.apply(Seat::A, cast).unwrap();
    let ch = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::Choose { .. }))
        .expect("a target is pending");
    e.apply(Seat::A, ch).unwrap();
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().no_engage_until,
        r + 1,
        "Don't Look marks the enemy through the end of next round"
    );
}

#[test]
fn hold_the_memory_delays_a_fading_spirit_one_step() {
    // OnPlay/TargetFadingSpirit/Replace(DelayFadeOneStep): a chosen Fading base skips its
    // next Fade step. Now that the Dusk is instant and Fade is at turn-END, the only
    // Fading spirits are combat-banished bases inside their standing-Faded window — so the
    // target is one of those (banished_by Some + a fade_deadline DUE at A's turn-end), and
    // Hold the Memory extends its window by one turn. Survival is read off the `fading`
    // flag (a surfaced Stray on the vacated tile isn't fading), so it's immune to stray
    // noise.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let still_fading_after_next_fade = |with_hold: bool| -> bool {
        let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck.clone());
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Cloudling"), Seat::A);
            let sp = st.board[11].spirit.as_mut().unwrap();
            sp.fading = true;
            sp.banished_by = Some(Seat::B); // a combat banish (the only fading state)
            sp.fade_deadline = Some(st.round); // DUE at A's coming turn-END
            st.player_a.hand = vec![id_of("Hold the Memory")];
            st.player_a.anima = 9;
        }
        if with_hold {
            let cast = e
                .legal_commands(Seat::A)
                .into_iter()
                .find(|c| matches!(c, Command::CastRitual { .. }))
                .expect("Hold the Memory is castable");
            e.apply(Seat::A, cast).unwrap();
            let ch = e
                .legal_commands(Seat::A)
                .into_iter()
                .find(|c| matches!(c, Command::Choose { .. }))
                .expect("a fading target is pending");
            e.apply(Seat::A, ch).unwrap();
        }
        // A's turn-END Fade fires here (the base is due); the skip (if cast) spends there.
        e.apply(Seat::A, Command::EndTurn).unwrap();
        e.state().board[11].spirit.as_ref().map(|s| s.fading) == Some(true)
    };
    assert!(
        !still_fading_after_next_fade(false),
        "normally the fading base dissolves at A's turn-END Fade"
    );
    assert!(
        still_fading_after_next_fade(true),
        "Hold the Memory: it skips one Fade step and lingers a turn longer"
    );
}

#[test]
fn patience_grants_one_anima_now_and_one_at_next_flow() {
    // OnPlay/Owner/AnimaDelta (now) + AtFlow/OncePerMatch/Owner/AnimaDelta (scheduled).
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.player_a.hand = vec![id_of("Patience")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Patience is castable");
    let evs_play = e.apply(Seat::A, cast).unwrap();
    // The "now" half is the standard Owner/AnimaDelta (an AnimaGained on play, which
    // its cost may net out); the NEW half is the schedule for the next Flow.
    assert!(
        evs_play.iter().any(|ev| matches!(
            ev,
            recollect_core::state::Event::AnimaGained { seat: Seat::A, .. }
        )),
        "Patience grants Anima now (OnPlay)"
    );
    assert_eq!(
        e.state().pending_flow_anima[Seat::A as usize],
        1,
        "1 Anima is scheduled for A's next Flow"
    );
    // Advance to A's next Flow (A → B → A); the owed anima is paid there.
    e.apply(Seat::A, recollect_core::state::Command::EndTurn)
        .unwrap();
    let evs = e
        .apply(Seat::B, recollect_core::state::Command::EndTurn)
        .unwrap();
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            recollect_core::state::Event::FlowAnimaPaid { seat: Seat::A }
        )),
        "the owed Anima is paid at A's next Flow"
    );
    assert_eq!(
        e.state().pending_flow_anima[Seat::A as usize],
        0,
        "the debt is cleared once paid"
    );
}

#[test]
fn reclaim_refunds_half_cost_and_full_for_last_light_koi() {
    // Fade reclaim (voluntary Act action): cash a standing spirit for ⌊cost/2⌋ Anima,
    // FULL for Last-Light Koi (its PartingReclaimsFullCost).
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let reclaim_gain = |card_name: &str| -> u8 {
        let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck.clone());
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 12, id_of(card_name), Seat::A);
            st.player_a.anima = 0;
        }
        let before = e.state().player(Seat::A).anima;
        e.apply(
            Seat::A,
            recollect_core::state::Command::Reclaim { tile: 12 },
        )
        .unwrap();
        assert!(
            e.state().board[12].spirit.is_none(),
            "the reclaimed spirit left the board (no impression)"
        );
        assert!(
            e.state().board[12].impressions.first().copied().is_none(),
            "...and left no impression"
        );
        e.state().player(Seat::A).anima - before
    };
    // Cloudling (cost 2) → ⌊2/2⌋ = 1.
    assert_eq!(
        reclaim_gain("Cloudling"),
        1,
        "an ordinary reclaim refunds half the cost"
    );
    // Last-Light Koi (cost 3) → FULL 3 (not ⌊3/2⌋ = 1).
    assert_eq!(
        reclaim_gain("Last-Light Koi"),
        3,
        "Last-Light Koi reclaims its full cost"
    );
}

#[test]
fn kindle_buffs_the_next_arriving_spirit() {
    // OnPlay/NextArrivalThisTurn/StatDelta{atk:20}: the seat's next arrival this turn
    // gets +20 Attack this round (applied before it strikes).
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.player_a.first_placement_done = true;
        st.player_a.hand = vec![id_of("Kindle"), id_of("Cloudling")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Kindle is castable");
    e.apply(Seat::A, cast).unwrap();
    assert_eq!(
        e.state().next_arrival_atk[Seat::A as usize],
        20,
        "Kindle queued +20 for the next arrival"
    );
    let play = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::PlaySpirit { engage: None, .. }))
        .expect("a placement is legal");
    let tile = if let Command::PlaySpirit { tile, .. } = &play {
        *tile
    } else {
        unreachable!()
    };
    e.apply(Seat::A, play).unwrap();
    let base = e.card(id_of("Cloudling")).attack;
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, tile).attack,
        base + 20,
        "the arriving spirit got Kindle's +20 Attack"
    );
    assert_eq!(
        e.state().next_arrival_atk[Seat::A as usize],
        0,
        "the buff was consumed by the arrival"
    );
}

#[test]
fn again_lets_the_next_arrival_engage_a_second_target() {
    // OnPlay/NextArrivalThisTurn/SecondTargetEngage{-10}: after the next arrival's
    // first engage, it may engage a SECOND chosen target at −10 (a target choice
    // resolving to a full exchange via the ctx-engage seam).
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.player_a.first_placement_done = true;
        put_spirit(st, 12, id_of("Cloudling"), Seat::B); // enemy in the arrival's reach
        st.board[12].spirit.as_mut().unwrap().defense = 0; // the −10 engage still lands
        st.board[12].spirit.as_mut().unwrap().attack = 0; // no interception/retaliation noise
        st.player_a.hand = vec![id_of("Again!"), id_of("Cloudling")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Again! is castable");
    e.apply(Seat::A, cast).unwrap();
    assert_eq!(
        e.state().next_arrival_2nd_engage[Seat::A as usize],
        Some(-10),
        "Again! armed a second engage for the next arrival"
    );
    // Arrive at home-row tile 7 (adjacent to the enemy at 12), no first engage.
    e.apply(
        Seat::A,
        Command::PlaySpirit {
            hand_index: 0,
            tile: 7,
            engage: None,
            chain_prefs: Vec::new(),
        },
    )
    .unwrap();
    let hp0 = e.state().board[12].spirit.as_ref().unwrap().hp;
    let ch = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::Choose { .. }))
        .expect("a second-engage target choice is pending");
    e.apply(Seat::A, ch).unwrap();
    assert!(
        e.state().board[12]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(0)
            < hp0,
        "the arrival's second engage struck the chosen enemy"
    );
    assert_eq!(
        e.state().next_arrival_2nd_engage[Seat::A as usize],
        None,
        "the second-engage offer was consumed"
    );
}

#[test]
fn reckless_charge_makes_a_chosen_ally_engage_now() {
    // TargetAllySpirit/GrantEngage{immediate} (pick ally → pick target → engage) +
    // TargetAllySpirit/RetaliationDelta{-10}/ThisRound (the charger's retaliation shift).
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // the charger
        put_spirit(st, 13, id_of("Cloudling"), Seat::B); // the target (adjacent)
        st.board[13].spirit.as_mut().unwrap().defense = 0;
        st.board[13].spirit.as_mut().unwrap().attack = 0;
        st.player_a.hand = vec![id_of("Reckless Charge")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Reckless Charge is castable");
    e.apply(Seat::A, cast).unwrap();
    let hp0 = e.state().board[13].spirit.as_ref().unwrap().hp;
    // Three choices: ally (engage), ally (retaliation), then the engage target.
    for _ in 0..3 {
        if let Some(ch) = e
            .legal_commands(Seat::A)
            .into_iter()
            .find(|c| matches!(c, Command::Choose { .. }))
        {
            e.apply(Seat::A, ch).unwrap();
        }
    }
    assert!(
        e.state().board[13]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(0)
            < hp0,
        "Reckless Charge: the chosen ally engaged the chosen target"
    );
    assert!(
        !e.state().temp_retaliation.is_empty(),
        "the charger's retaliation was shifted this round"
    );
}

#[test]
fn ragewoken_bison_pays_hp_for_an_extra_engage() {
    // OnPlay/PayForm{20}/SelfSpirit/ExtraEngage: on arrival, may pay 20 HP to engage a
    // chosen extra target (choosing itself declines).
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.player_a.first_placement_done = true;
        // Target in Lance reach from 7. A Fury body (Cinderling) so it does NOT edge the Fury
        // Bison: with the combat re-stat dropping the Bison's defense, a Wonder target's +10
        // resonance edge would otherwise leak 5 damage through on both the arrival interception
        // and the engage retaliation, masking the clean 20-HP PayForm this test measures.
        put_spirit(st, 12, id_of("Cinderling"), Seat::B);
        st.board[12].spirit.as_mut().unwrap().defense = 0;
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        st.player_a.hand = vec![id_of("Ragewoken Bison")];
        st.player_a.anima = 9;
    }
    e.apply(
        Seat::A,
        Command::PlaySpirit {
            hand_index: 0,
            tile: 7,
            engage: None,
            chain_prefs: Vec::new(),
        },
    )
    .unwrap();
    let Some(PendingChoice::Target { options, .. }) = e.state().pending_choice.clone() else {
        panic!("a pay-engage choice is pending");
    };
    let idx = options
        .iter()
        .position(|&t| t == 12)
        .expect("the enemy at 12 is an option") as u8;
    let bison_hp0 = e.state().board[7].spirit.as_ref().unwrap().hp;
    e.apply(Seat::A, Command::Choose { index: idx }).unwrap();
    assert_eq!(
        e.state().board[7].spirit.as_ref().unwrap().hp,
        bison_hp0 - 20,
        "Ragewoken paid 20 HP for the extra engage"
    );
    assert!(
        e.state().board[12]
            .spirit
            .as_ref()
            .map(|s| s.fading)
            .unwrap_or(true),
        "the extra engage struck the chosen target"
    );
}

#[test]
fn vertigo_pushes_the_engage_survivor() {
    // OnEngageResolved/Survivor/Displace(Push 1): after Vertigo's engage, the still-
    // standing party is shoved one tile away from Vertigo.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.player_a.first_placement_done = true;
        put_spirit(st, 12, id_of("Cloudling"), Seat::B); // in Vertigo's Lance reach from 7
        st.board[12].spirit.as_mut().unwrap().hp = 500; // survives the engage
        st.board[12].spirit.as_mut().unwrap().attack = 0; // Vertigo survives
        st.player_a.hand = vec![id_of("Vertigo, Who Loves the Long Fall")];
        st.player_a.anima = 9;
    }
    e.apply(
        Seat::A,
        Command::PlaySpirit {
            hand_index: 0,
            tile: 7,
            engage: Some(12),
            chain_prefs: Vec::new(),
        },
    )
    .unwrap();
    assert!(
        e.state().board[12].spirit.is_none(),
        "the survivor was shoved off tile 12"
    );
    assert!(
        e.state().board[17].spirit.is_some(),
        "the survivor landed on tile 17 (pushed away from Vertigo)"
    );
}

#[test]
fn second_farewell_refires_an_allys_parting() {
    // OnPlay/Owner/ReTriggerParting: pick one of your standing Parting-bearers and fire
    // its Parting again (the spirit stays). Drizzle Sprite's Parting restores a wounded ally.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Drizzle Sprite"), Seat::A); // has a Parting
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // wounded restore target
        st.board[12].spirit.as_mut().unwrap().hp = 10;
        st.board[12].spirit.as_mut().unwrap().hp_max = 50;
        st.player_a.hand = vec![id_of("Second Farewell")];
        st.player_a.anima = 9;
    }
    let hp0 = e.state().board[12].spirit.as_ref().unwrap().hp;
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Second Farewell is castable");
    e.apply(Seat::A, cast).unwrap();
    let ch = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::Choose { .. }))
        .expect("a Parting-bearer choice is pending");
    e.apply(Seat::A, ch).unwrap();
    assert!(
        e.state().board[12].spirit.as_ref().unwrap().hp > hp0,
        "the chosen ally's Parting re-fired (wounded ally restored)"
    );
    assert!(
        e.state().board[11]
            .spirit
            .as_ref()
            .map(|s| !s.fading)
            .unwrap_or(false),
        "the Parting-bearer still stands (it didn't fade)"
    );
}

#[test]
fn the_long_shadow_discounts_adjacent_fabrications() {
    // The Long Shadow (Static/Owner/CostDelta{-1}, a Landmark): a Fabrication the owner
    // places ADJACENT to it costs 1 less (positional; cost_aura ignores terrain).
    use recollect_core::state::{Terrain, TerrainKind};
    let cat = canon_catalog();
    let fab = cat
        .iter()
        .find(|c| matches!(c.kind, recollect_core::types::CardKind::Fabrication) && c.cost >= 2)
        .expect("a Fabrication exists");
    let base_cost = fab.cost;
    let spent_at = |tile: u8| -> u8 {
        let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
        let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
        {
            let st = e.state_mut_for_test();
            st.player_a.first_placement_done = true;
            st.board[6].terrain = Some(Terrain {
                card: fab_long_shadow(&cat),
                owner: Seat::A,
                kind: TerrainKind::Landmark,
                face_down: false,
            });
            st.player_a.hand = vec![fab.id];
            st.player_a.anima = 20;
        }
        let before = e.state().player(Seat::A).anima;
        e.apply(
            Seat::A,
            recollect_core::state::Command::SetFabrication {
                hand_index: 0,
                tile,
            },
        )
        .unwrap();
        before - e.state().player(Seat::A).anima
    };
    assert_eq!(
        spent_at(7),
        base_cost - 1,
        "adjacent to the Long Shadow: 1 less"
    );
    assert_eq!(spent_at(9), base_cost, "not adjacent: full cost");
}

fn fab_long_shadow(cat: &[recollect_core::types::CardDef]) -> CardId {
    cat.iter().find(|c| c.name == "The Long Shadow").unwrap().id
}

#[test]
fn curio_fox_privately_peeks_an_enemy_fabrication() {
    // OnPlay/Owner/RevealFabrication: Curio Fox lets its owner LOOK at one enemy face-down
    // Fabrication — un-redacted in the owner's view only; the lie stays face-down in state.
    use recollect_core::state::{Terrain, TerrainKind};
    use recollect_core::view::view_for;
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    let fab = id_of("Hidden Vista");
    {
        let st = e.state_mut_for_test();
        st.player_a.first_placement_done = true;
        st.board[12].terrain = Some(Terrain {
            card: fab,
            owner: Seat::B,
            kind: TerrainKind::Fabrication,
            face_down: true,
        });
        st.player_a.hand = vec![id_of("Curio Fox")];
        st.player_a.anima = 9;
    }
    let before = view_for(&e, Seat::A);
    assert_eq!(
        before.tiles[12].terrain.as_ref().unwrap().card,
        CardId(u16::MAX),
        "A cannot see B's face-down Fabrication before the peek"
    );
    e.apply(
        Seat::A,
        Command::PlaySpirit {
            hand_index: 0,
            tile: 7,
            engage: None,
            chain_prefs: Vec::new(),
        },
    )
    .unwrap();
    let ch = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::Choose { .. }))
        .expect("a peek choice is pending");
    e.apply(Seat::A, ch).unwrap();
    assert_eq!(
        view_for(&e, Seat::A).tiles[12]
            .terrain
            .as_ref()
            .unwrap()
            .card,
        fab,
        "A now sees the peeked Fabrication's identity"
    );
    assert!(
        e.state().board[12].spirit.is_none()
            && e.state().board[12].terrain.as_ref().unwrap().face_down,
        "the lie stays face-down in state (a private peek, not a public reveal)"
    );
}

#[test]
fn beacon_reveals_adjacent_fabrications_to_its_owner() {
    // Beacon (Static/Owner/RevealFabrication, a Landmark): an enemy face-down Fabrication
    // adjacent to the owner's face-up Beacon is un-redacted in the owner's view.
    use recollect_core::state::{Terrain, TerrainKind};
    use recollect_core::view::view_for;
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let fab = id_of("Hidden Vista");
    let card_at_12 = |beacon: bool| -> CardId {
        let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck.clone());
        {
            let st = e.state_mut_for_test();
            st.board[12].terrain = Some(Terrain {
                card: fab,
                owner: Seat::B,
                kind: TerrainKind::Fabrication,
                face_down: true,
            });
            if beacon {
                st.board[11].terrain = Some(Terrain {
                    card: id_of("Beacon"),
                    owner: Seat::A,
                    kind: TerrainKind::Landmark,
                    face_down: false,
                });
            }
        }
        view_for(&e, Seat::A).tiles[12]
            .terrain
            .as_ref()
            .unwrap()
            .card
    };
    assert_eq!(
        card_at_12(false),
        CardId(u16::MAX),
        "without a Beacon, the enemy lie is hidden"
    );
    assert_eq!(
        card_at_12(true),
        fab,
        "the Beacon reveals the adjacent enemy Fabrication to A"
    );
}

#[test]
fn otterling_magus_makes_rituals_hit_an_extra_target() {
    // Static/Owner/Exception(RitualsExtraTarget): a ritual's target choice also hits one more
    // eligible target. Cast Mend (heal) with two wounded allies; Otterling heals both.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let healed = |otterling: bool| -> usize {
        let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck.clone());
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 0, id_of("Cloudling"), Seat::A);
            put_spirit(st, 1, id_of("Cloudling"), Seat::A);
            for t in [0usize, 1] {
                let s = st.board[t].spirit.as_mut().unwrap();
                s.hp = 10;
                s.hp_max = 50;
            }
            if otterling {
                // far tile (24) so the extra target is the 2nd wounded ally, not Otterling
                put_spirit(
                    st,
                    24,
                    id_of("Otterling Magus, Keeper of Five Lights"),
                    Seat::A,
                );
            }
            st.player_a.hand = vec![id_of("Mend")];
            st.player_a.anima = 9;
            st.player_a.first_placement_done = true;
        }
        let cast = e
            .legal_commands(Seat::A)
            .into_iter()
            .find(|c| matches!(c, Command::CastRitual { .. }))
            .expect("Mend is castable");
        e.apply(Seat::A, cast).unwrap();
        let ch = e
            .legal_commands(Seat::A)
            .into_iter()
            .find(|c| matches!(c, Command::Choose { .. }))
            .expect("a target choice is pending");
        e.apply(Seat::A, ch).unwrap();
        [0u8, 1]
            .iter()
            .filter(|&&t| {
                e.state().board[t as usize]
                    .spirit
                    .as_ref()
                    .map(|s| s.hp > 10)
                    .unwrap_or(false)
            })
            .count()
    };
    assert_eq!(healed(false), 1, "without Otterling, Mend heals one ally");
    assert_eq!(healed(true), 2, "Otterling: Mend heals an extra ally too");
}
