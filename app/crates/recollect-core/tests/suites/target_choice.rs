//! Free target-choice rituals: a played card opens a `PendingChoice::Target` to
//! pick any spirit board-wide (no adjacency). Pins Hold Fast — "a spirit +20
//! Defense this round" — which routes through the choice seam with source=None.
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::effects::Keyword;
use recollect_core::engine::{combat_stats_for_test, keyword_active_for_test};
use recollect_core::state::{Bond, Command, PendingChoice, Phase};
use recollect_core::test_support::put_spirit;
use recollect_core::types::{CardId, Faction, Seat};

fn id_of(name: &str) -> CardId {
    canon_catalog()
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.id)
        .unwrap_or_else(|| panic!("card not found: {name}"))
}

#[test]
fn hold_fast_buffs_a_chosen_spirit_this_round() {
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        st.player_a.hand = vec![id_of("Hold Fast")];
        st.player_a.anima = 9;
    }
    // Cast Hold Fast → opens a target choice over every spirit on the board.
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Hold Fast is castable");
    e.apply(Seat::A, cast).unwrap();
    assert!(
        matches!(e.state().phase, Phase::PendingChoice { seat: Seat::A, .. }),
        "casting opens a target choice"
    );
    let Some(PendingChoice::Target { options, .. }) = e.state().pending_choice.clone() else {
        panic!("a target choice is pending");
    };
    let idx = options
        .iter()
        .position(|&t| t == 11)
        .expect("the spirit at 11 is a legal target") as u8;
    e.apply(Seat::A, Command::Choose { index: idx }).unwrap();
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, 11).defense,
        20,
        "Hold Fast grants the chosen spirit +20 Defense this round"
    );
}

#[test]
fn mend_restores_hp_to_a_chosen_spirit() {
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        st.board[11].spirit.as_mut().unwrap().hp = 5; // wounded (hp_max 40)
        st.player_a.hand = vec![id_of("Mend")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Mend is castable");
    e.apply(Seat::A, cast).unwrap();
    let Some(PendingChoice::Target { options, .. }) = e.state().pending_choice.clone() else {
        panic!("a target choice is pending");
    };
    let idx = options.iter().position(|&t| t == 11).unwrap() as u8;
    e.apply(Seat::A, Command::Choose { index: idx }).unwrap();
    assert_eq!(
        e.state().board[11].spirit.as_ref().unwrap().hp,
        35,
        "Mend restores 30 HP (5 → 35) to the chosen spirit"
    );
}

#[test]
fn night_falls_early_debuffs_every_enemy_this_round() {
    // EnemiesAll/StatDelta with no choice: "All enemies −10 Attack this round."
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 16, id_of("Cloudling"), Seat::B); // an enemy of the caster
        st.player_a.hand = vec![id_of("Night Falls Early")];
        st.player_a.anima = 9;
    }
    let before = combat_stats_for_test(e.state(), &cat, 16).attack;
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Night Falls Early is castable");
    e.apply(Seat::A, cast).unwrap();
    assert!(
        e.state().pending_choice.is_none(),
        "a board-wide debuff opens no choice"
    );
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, 16).attack,
        before - 10,
        "every enemy is weakened by 10 Attack this round"
    );
}

#[test]
fn star_strewn_otter_discounts_your_next_ritual() {
    let cat = canon_catalog();
    // Anima spent casting Night Falls Early, with the Otter's discount granted or not.
    let spent = |with_otter: bool| -> u8 {
        let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
        let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
        {
            let st = e.state_mut_for_test();
            if with_otter {
                put_spirit(st, 11, id_of("Star-Strewn Otter"), Seat::A);
            }
            st.player_a.hand = vec![id_of("Night Falls Early")];
            st.player_a.anima = 20;
        }
        if with_otter {
            e.fire_arrival_effects_for_test(11, Seat::A); // Otter's OnPlay grants the discount
        }
        let before = e.state().player(Seat::A).anima;
        let cast = e
            .legal_commands(Seat::A)
            .into_iter()
            .find(|c| matches!(c, Command::CastRitual { .. }))
            .expect("a ritual is castable");
        e.apply(Seat::A, cast).unwrap();
        before - e.state().player(Seat::A).anima
    };
    assert_eq!(
        spent(false) as i16 - spent(true) as i16,
        1,
        "the Otter's one-shot discount saved 1 Anima on the next ritual"
    );
}

#[test]
fn scrub_the_margin_erases_a_chosen_enemy_impression() {
    // Owner/ImpressionRemoveTarget: open a choice over enemy impressions, erase the chosen one.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.board[12].impressions = vec![Seat::B]; // an enemy impression (B's point)
        st.player_a.hand = vec![id_of("Scrub the Margin")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Scrub the Margin is castable");
    e.apply(Seat::A, cast).unwrap();
    let Some(PendingChoice::Target { options, .. }) = e.state().pending_choice.clone() else {
        panic!("an impression-removal choice is pending");
    };
    let idx = options
        .iter()
        .position(|&t| t == 12)
        .expect("the enemy impression is a target") as u8;
    e.apply(Seat::A, Command::Choose { index: idx }).unwrap();
    assert!(
        e.state().board[12].impressions.first().copied().is_none(),
        "the chosen enemy impression was erased"
    );
}

#[test]
fn scrub_the_margin_tallies_when_the_solace_erases() {
    // The same erase, but the caster is the Solace: removing an EXISTING mark is an erasure, so it
    // SCORES (+1 to the off-board tally) — unwriting unified with forgetting.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Solace, Faction::Lorekeeper]; // seat A pilots the Solace here
        st.board[12].impressions = vec![Seat::B];
        st.player_a.hand = vec![id_of("Scrub the Margin")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Scrub the Margin is castable");
    e.apply(Seat::A, cast).unwrap();
    let Some(PendingChoice::Target { options, .. }) = e.state().pending_choice.clone() else {
        panic!("an impression-removal choice is pending");
    };
    let idx = options
        .iter()
        .position(|&t| t == 12)
        .expect("the enemy impression is a target") as u8;
    e.apply(Seat::A, Command::Choose { index: idx }).unwrap();
    assert!(
        e.state().board[12].impressions.first().copied().is_none(),
        "the chosen mark was erased"
    );
    assert_eq!(
        e.state().solace_erasures,
        1,
        "the Solace erasing an existing mark scores"
    );
}

#[test]
fn eruption_burns_enemies_next_to_your_fury_spirits() {
    // EnemiesAdjacentToAlliesOf{Fury}/Damage: 20 to every enemy adjacent to your
    // Fury spirits. Cinderling (Fury) at 11=(1,2); enemy at 12 (adjacent), enemy far.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cinderling"), Seat::A); // your Fury spirit
        put_spirit(st, 12, id_of("Cloudling"), Seat::B); // enemy adjacent to it
        put_spirit(st, 24, id_of("Cloudling"), Seat::B); // enemy far away (4,4)
        st.player_a.hand = vec![id_of("Eruption")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Eruption is castable");
    e.apply(Seat::A, cast).unwrap();
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        20,
        "the adjacent enemy took 20 (40 → 20)"
    );
    assert_eq!(
        e.state().board[24].spirit.as_ref().unwrap().hp,
        40,
        "the distant enemy is untouched"
    );
}

#[test]
fn harvest_together_grants_anima_per_adjacent_allied_pair() {
    let cat = canon_catalog();
    // Anima after casting Harvest Together with `n` allies in a row (n-1 pairs).
    // Cast cost cancels in the difference, leaving the per-pair grant.
    let after = |n: u8| -> u8 {
        let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
        let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
        {
            let st = e.state_mut_for_test();
            for k in 0..n {
                put_spirit(st, 11 + k, id_of("Cloudling"), Seat::A); // 11,12,13 are a row
            }
            st.player_a.hand = vec![id_of("Harvest Together")];
            st.player_a.anima = 9;
        }
        let cast = e
            .legal_commands(Seat::A)
            .into_iter()
            .find(|c| matches!(c, Command::CastRitual { .. }))
            .expect("Harvest Together is castable");
        e.apply(Seat::A, cast).unwrap();
        e.state().player(Seat::A).anima
    };
    assert_eq!(
        after(3) as i16 - after(1) as i16,
        2,
        "three allies in a row are two adjacent pairs → +2 Anima"
    );
}

#[test]
fn what_remains_grants_anima_per_lost_spirit() {
    let cat = canon_catalog();
    let after = |lost: usize| -> u8 {
        let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
        let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
        {
            let st = e.state_mut_for_test();
            for _ in 0..lost {
                st.dissolved.push((Seat::A, id_of("Cloudling")));
            }
            st.player_a.hand = vec![id_of("What Remains")];
            st.player_a.anima = 9;
        }
        let cast = e
            .legal_commands(Seat::A)
            .into_iter()
            .find(|c| matches!(c, Command::CastRitual { .. }))
            .expect("What Remains is castable");
        e.apply(Seat::A, cast).unwrap();
        e.state().player(Seat::A).anima
    };
    assert_eq!(
        after(2) as i16 - after(0) as i16,
        2,
        "two lost spirits → +2 Anima (under the cap of 3)"
    );
}

#[test]
fn war_roar_buffs_only_your_beast_spirits() {
    // AlliesWithImprint{Beast}/StatDelta: "All your Beast spirits +10 Attack this round."
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Aurora Elk"), Seat::A); // Beast
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // not Beast
        st.player_a.hand = vec![id_of("War Roar")];
        st.player_a.anima = 9;
    }
    let beast_before = combat_stats_for_test(e.state(), &cat, 11).attack;
    let plain_before = combat_stats_for_test(e.state(), &cat, 12).attack;
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("War Roar is castable");
    e.apply(Seat::A, cast).unwrap();
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, 11).attack,
        beast_before + 10,
        "your Beast spirit gains +10 Attack"
    );
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, 12).attack,
        plain_before,
        "your non-Beast spirit is unaffected"
    );
}

#[test]
fn stoke_damages_a_chosen_ally_for_anima() {
    // TargetAllySpirit/Damage via the choice seam: "Deal 10 damage to your spirit;
    // gain 2 Anima." (The Owner/AnimaDelta half fires at cast; the damage is chosen.)
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // your ally, full HP (40)
        st.player_a.hand = vec![id_of("Stoke")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Stoke is castable");
    e.apply(Seat::A, cast).unwrap();
    let Some(PendingChoice::Target { options, .. }) = e.state().pending_choice.clone() else {
        panic!("a damage target is pending");
    };
    let idx = options
        .iter()
        .position(|&t| t == 11)
        .expect("your ally is a valid target") as u8;
    e.apply(Seat::A, Command::Choose { index: idx }).unwrap();
    assert_eq!(
        e.state().board[11].spirit.as_ref().unwrap().hp,
        30,
        "the chosen ally took 10 damage (40 → 30)"
    );
}

#[test]
fn trade_winds_draws_for_both_narrators() {
    // BothNarrators/Draw: the OPPONENT (who did nothing) also draws a card.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.player_a.hand = vec![id_of("Trade Winds")];
        st.player_a.anima = 9;
    }
    let b_hand = e.state().player(Seat::B).hand.len();
    let b_deck = e.state().player(Seat::B).deck.len();
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Trade Winds is castable");
    e.apply(Seat::A, cast).unwrap();
    assert_eq!(
        e.state().player(Seat::B).hand.len(),
        b_hand + 1,
        "the opponent drew a card"
    );
    assert_eq!(
        e.state().player(Seat::B).deck.len(),
        b_deck - 1,
        "from their own deck"
    );
}

#[test]
fn gather_in_draws_one_or_two_with_a_bond() {
    // Two specs: Always/Owner/Draw 1, plus YouControlABond/Owner/Draw 1. With a
    // controlled Bond the conditional clause also fires, so the owner draws 2.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let draws = |with_bond: bool| -> usize {
        let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck.clone());
        {
            let st = e.state_mut_for_test();
            st.player_a.hand = vec![id_of("Gather In")];
            st.player_a.anima = 9;
            if with_bond {
                st.bonds.push(Bond {
                    card: id_of("Promise"),
                    owner: Seat::A,
                    tile_a: 11,
                    tile_b: 12,
                });
            }
        }
        let deck0 = e.state().player(Seat::A).deck.len();
        let cast = e
            .legal_commands(Seat::A)
            .into_iter()
            .find(|c| matches!(c, Command::CastRitual { .. }))
            .expect("Gather In is castable");
        e.apply(Seat::A, cast).unwrap();
        deck0 - e.state().player(Seat::A).deck.len()
    };
    assert_eq!(draws(false), 1, "no Bond: draw 1");
    assert_eq!(draws(true), 2, "controlling a Bond: draw 2");
}

#[test]
fn dig_in_grants_steadfast_and_defense_to_a_chosen_spirit() {
    // Two target clauses fired by one play (GrantKeyword{Steadfast} this round,
    // then StatDelta +10 Defense this round). The second queues behind the first;
    // both resolve onto the chosen ally via the choice queue.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
        st.player_a.hand = vec![id_of("Dig In")];
        st.player_a.anima = 9;
    }
    let def0 = combat_stats_for_test(e.state(), &cat, 12).defense;
    assert!(
        !keyword_active_for_test(e.state(), &cat, 12, Keyword::Steadfast),
        "precondition: the ally is not yet Steadfast"
    );
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Dig In is castable");
    e.apply(Seat::A, cast).unwrap();
    // Resolve both queued target choices (only tile 12 is eligible for each).
    for _ in 0..2 {
        let ch = e
            .legal_commands(Seat::A)
            .into_iter()
            .find(|c| matches!(c, Command::Choose { .. }))
            .expect("a target choice is pending");
        e.apply(Seat::A, ch).unwrap();
    }
    assert!(
        keyword_active_for_test(e.state(), &cat, 12, Keyword::Steadfast),
        "Dig In: the chosen spirit gained Steadfast"
    );
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, 12).defense,
        def0 + 10,
        "Dig In: the chosen spirit gained +10 Defense this round"
    );
    assert!(
        e.state().pending_choice.is_none() && e.state().choice_queue.is_empty(),
        "both choices resolved; the queue is drained"
    );
}

#[test]
fn misstep_slides_an_enemy_to_a_chosen_empty_tile() {
    // Two-step displacement: step 1 picks the enemy (TargetEnemySpirit), step 2
    // picks the destination. Tile 12's neighbours 7/11/13 are blocked by allies,
    // leaving only 17 empty, so the slide is deterministic.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::B); // the enemy to displace
        put_spirit(st, 7, id_of("Cloudling"), Seat::A); // blockers (own, not targetable)
        put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        put_spirit(st, 13, id_of("Cloudling"), Seat::A);
        st.player_a.hand = vec![id_of("Misstep")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Misstep is castable");
    e.apply(Seat::A, cast).unwrap();
    // Step 1: pick the only enemy (tile 12). Step 2: pick the only empty neighbour (17).
    for _ in 0..2 {
        let ch = e
            .legal_commands(Seat::A)
            .into_iter()
            .find(|c| matches!(c, Command::Choose { .. }))
            .expect("a displacement choice is pending");
        e.apply(Seat::A, ch).unwrap();
    }
    assert!(
        e.state().board[12].spirit.is_none(),
        "Misstep: the enemy left tile 12"
    );
    assert_eq!(
        e.state().board[17].spirit.as_ref().map(|s| s.owner),
        Some(Seat::B),
        "Misstep: the enemy slid to the chosen empty tile 17"
    );
}

#[test]
fn round_buffs_two_adjacent_allies() {
    // TargetTwoAdjacentAllies: step 1 picks the first ally, step 2 its adjacent
    // ally; both get +10 Attack this round. Two adjacent allies (12,13) make the
    // picks deterministic.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
        put_spirit(st, 13, id_of("Cloudling"), Seat::A);
        st.player_a.hand = vec![id_of("Round")];
        st.player_a.anima = 9;
    }
    let a12 = combat_stats_for_test(e.state(), &cat, 12).attack;
    let a13 = combat_stats_for_test(e.state(), &cat, 13).attack;
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Round is castable");
    e.apply(Seat::A, cast).unwrap();
    for _ in 0..2 {
        let ch = e
            .legal_commands(Seat::A)
            .into_iter()
            .find(|c| matches!(c, Command::Choose { .. }))
            .expect("a pair-buff choice is pending");
        e.apply(Seat::A, ch).unwrap();
    }
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, 12).attack,
        a12 + 10,
        "Round: first ally +10 Attack"
    );
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, 13).attack,
        a13 + 10,
        "Round: paired ally +10 Attack"
    );
}

#[test]
fn behind_you_swaps_two_adjacent_enemies() {
    // Displace(Swap): pick one enemy, then an adjacent enemy; exchange them. The two
    // enemies carry different HP so the swap is observable.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::B);
        st.board[12].spirit.as_mut().unwrap().hp = 15; // marker
        put_spirit(st, 13, id_of("Cloudling"), Seat::B);
        st.board[13].spirit.as_mut().unwrap().hp = 40;
        st.player_a.hand = vec![id_of("Behind You")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Behind You is castable");
    e.apply(Seat::A, cast).unwrap();
    for _ in 0..2 {
        let ch = e
            .legal_commands(Seat::A)
            .into_iter()
            .find(|c| matches!(c, Command::Choose { .. }))
            .expect("a swap choice is pending");
        e.apply(Seat::A, ch).unwrap();
    }
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        40,
        "Behind You: tile 12 now holds the other enemy"
    );
    assert_eq!(
        e.state().board[13].spirit.as_ref().unwrap().hp,
        15,
        "Behind You: tile 13 now holds the first enemy"
    );
}

#[test]
fn hold_the_note_heals_both_ends_of_a_chosen_bond() {
    // TargetBondedPair/RestoreForm: pick one of your Bonds; both endpoints heal 20.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
        put_spirit(st, 13, id_of("Cloudling"), Seat::A);
        st.board[12].spirit.as_mut().unwrap().hp = 10;
        st.board[13].spirit.as_mut().unwrap().hp = 10;
        st.bonds.push(Bond {
            card: id_of("Promise"),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 13,
        });
        st.player_a.hand = vec![id_of("Hold the Note")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Hold the Note is castable");
    e.apply(Seat::A, cast).unwrap();
    let ch = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::Choose { .. }))
        .expect("a bond choice is pending");
    e.apply(Seat::A, ch).unwrap();
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        30,
        "Hold the Note: endpoint 12 healed 10→30"
    );
    assert_eq!(
        e.state().board[13].spirit.as_ref().unwrap().hp,
        30,
        "Hold the Note: endpoint 13 healed 10→30"
    );
}
