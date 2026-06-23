//! Solace-set effect contracts. The new Unwritten/ill-intent cards added in the
//! Solace expansion get direct tests of their authored ON-ARRIVAL effects —
//! proving the spec does the thing, not just that it fires. Parting/OnDefeat
//! effects are covered for firing by card_effects_fire.rs.
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::state::{Command, Event, Terrain, TerrainKind};
use recollect_core::types::{CardId, Faction, Seat, SeatSlot};

fn id_of(name: &str) -> CardId {
    canon_catalog()
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.id)
        .unwrap_or_else(|| panic!("card not found: {name}"))
}

/// PvE engine with the named card at `tile` (seat B), a seat-A enemy at tile-1.
fn pve_with(card: &str, tile: u8) -> (Engine, u8) {
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, tile, id_of(card), Seat::B);
        recollect_core::test_support::put_spirit(st, tile - 1, id_of("Cloudling"), Seat::A);
    }
    (e, tile)
}

#[test]
fn the_vanishing_point_pushes_the_engaged_enemy() {
    let (mut e, tile) = pve_with("The Vanishing Point", 12);
    let evs = e.fire_arrival_effects_for_test(tile, Seat::B);
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritPushed { .. })),
        "The Vanishing Point should push the engaged enemy (events: {evs:?})"
    );
}

#[test]
fn the_new_solace_cards_fire_without_panic() {
    // Every authored on-arrival Solace effect reaches its resolver cleanly.
    for name in [
        "The Vanishing Point",
        "Negative Space",
        "Smear",
        "The Last Warm Page",
        "The Devouring Margin",
        "What Wants You Gone",
        "And So It Ends",
    ] {
        let (mut e, tile) = pve_with(name, 12);
        let _ = e.fire_arrival_effects_for_test(tile, Seat::B); // must not panic
    }
}

#[test]
fn the_mercy_itself_releases_a_fading_neighbor_leaving_no_impression() {
    // The merciful release: a fading adjacent spirit is removed with NO impression.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    let tile = 12u8;
    let enemy_tile = tile - 1;
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, tile, id_of("The Mercy Itself"), Seat::B);
        recollect_core::test_support::put_spirit(st, enemy_tile, id_of("Cloudling"), Seat::A);
        // Make the neighbour fading (a dying spirit — the release target).
        if let Some(sp) = st.board[enemy_tile as usize].spirit.as_mut() {
            sp.fading = true;
            sp.hp = 5;
        }
    }
    let evs = e.fire_arrival_effects_for_test(tile, Seat::B);
    let released = evs
        .iter()
        .any(|ev| matches!(ev, Event::SpiritReleased { .. }));
    assert!(
        released,
        "The Mercy Itself should release the fading neighbour (events: {evs:?})"
    );
    // The tile is now empty AND bears no impression (gentle erasure, not banishment).
    assert!(
        e.state().spirit_at(enemy_tile).is_none(),
        "released spirit is gone"
    );
    assert!(
        e.state().board[enemy_tile as usize]
            .impressions
            .first()
            .copied()
            .is_none(),
        "release leaves NO impression — that is the whole mercy"
    );
}

#[test]
fn the_kind_erasure_releases_one_fading_adjacent_spirit() {
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    let tile = 12u8;
    let enemy_tile = tile - 1;
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, tile, id_of("The Kind Erasure"), Seat::B);
        recollect_core::test_support::put_spirit(st, enemy_tile, id_of("Cloudling"), Seat::A);
        if let Some(sp) = st.board[enemy_tile as usize].spirit.as_mut() {
            sp.fading = true;
            sp.hp = 5;
        }
    }
    let evs = e.fire_arrival_effects_for_test(tile, Seat::B);
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritReleased { .. })),
        "The Kind Erasure should release a fading adjacent spirit (events: {evs:?})"
    );
}

#[test]
fn negative_space_aura_lowers_adjacent_enemy_defense() {
    // Static aura: −10 Defense to adjacent enemies. Compare derived defense with
    // Negative Space adjacent vs. a plain Unwritten adjacent.
    use recollect_core::engine::combat_stats_for_test;
    let cat = canon_catalog();
    let enemy_tile = 11u8;
    let measure = |aura_card: &str| -> i16 {
        let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
        let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 12, id_of(aura_card), Seat::B);
        recollect_core::test_support::put_spirit(st, enemy_tile, id_of("Cloudling"), Seat::A);
        combat_stats_for_test(e.state(), &cat, enemy_tile).defense
    };
    let with = measure("Negative Space");
    let without = measure("Static"); // a plain Unwritten, no defense aura
    assert_eq!(
        without - with,
        10,
        "Negative Space should lower adjacent enemy Defense by 10 (with={with}, without={without})"
    );
}

#[test]
fn what_you_set_down_returns_an_adjacent_enemy_to_hand() {
    // OnPlay/AdjacentEnemyChoose/Bounce: on arrival, an adjacent enemy returns to its owner's hand.
    let (mut e, tile) = pve_with("What You Set Down", 12);
    let evs = e.fire_arrival_effects_for_test(tile, Seat::B);
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritBounced { .. })),
        "What You Set Down bounces an adjacent enemy (events: {evs:?})"
    );
}

#[test]
fn footnote_mills_its_controllers_top_on_unwrite() {
    // OnUnwrite/SelfSpirit/MillTopDeck{opponent:false}: when Footnote Unwrites, its controller
    // (the Solace, seat B) mills its top card.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 12, id_of("Footnote"), Seat::B);
        st.board[12].spirit.as_mut().unwrap().fading = true;
    }
    let evs = e.force_fade_step_for_test(Seat::B);
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::DeckMilled { seat: Seat::B })),
        "Footnote mills its controller's top card on Unwrite (events: {evs:?})"
    );
}

#[test]
fn sentence_fragment_mills_the_opponents_top_on_unwrite() {
    // OnUnwrite/SelfSpirit/MillTopDeck{opponent:true}: "steals a word" — the opponent (seat A)
    // mills its top card when Sentence Fragment Unwrites.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 12, id_of("Sentence Fragment"), Seat::B);
        st.board[12].spirit.as_mut().unwrap().fading = true;
    }
    let evs = e.force_fade_step_for_test(Seat::B);
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::DeckMilled { seat: Seat::A })),
        "Sentence Fragment mills the opponent's top card on Unwrite (events: {evs:?})"
    );
}

/// The Solace plays an Unwriting event from hand — fire its effect through the normal
/// play path (the test seam), no director `lower()`. Returns the events it produced.
fn tell(e: &mut Engine, name: &str) -> Vec<Event> {
    e.fire_unwriting_for_test(name, Seat::B)
}

#[test]
fn the_page_turns_shifts_every_unwritten_inward() {
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 0, id_of("Forgotten Name"), Seat::B);
    }
    let evs = tell(&mut e, "The Page Turns");
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::UnwrittenShifted { from: 0, .. })),
        "The Page Turns shifts the corner Unwritten inward ({evs:?})"
    );
    for ev in evs {
        e.apply_event_for_test(ev);
    }
    assert!(
        e.state().board[0].spirit.is_none(),
        "the Unwritten left tile 0"
    );
}

#[test]
fn the_page_turns_steps_each_unwritten_to_its_exact_inward_tile() {
    // Mutation-killer (effects_exec.rs `exec_clause_mode`, ShiftUnwrittenInward: the
    // center `c = (w-1)/2`, the manhattan-to-center, and the axis-choice
    // `(x-c).abs() >= (y-c).abs()`). The inward step is EXACT, and the two axes must be
    // pinned independently. On the 5×5 (center (2,2)):
    //   corner 0=(0,0)  steps along X → tile 1=(1,0)   [the |Δx|>=|Δy| branch]
    //   top-mid 2=(2,0) steps along Y → tile 7=(2,1)   [the else-if `sy != 0` branch,
    //                                                   reached because |Δx|=0 < |Δy|=2]
    // A wrong center, or the axis-guard flipped, sends one of these to the wrong tile
    // (or nowhere). Processed farthest-first: dist(0)=4 before dist(2)=2, no collision.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 0, id_of("Forgotten Name"), Seat::B);
        recollect_core::test_support::put_spirit(st, 2, id_of("Forgotten Name"), Seat::B);
    }
    let evs = tell(&mut e, "The Page Turns");
    let shifted: Vec<(u8, u8)> = evs
        .iter()
        .filter_map(|ev| match ev {
            Event::UnwrittenShifted { from, to, .. } => Some((*from, *to)),
            _ => None,
        })
        .collect();
    assert!(
        shifted.contains(&(0, 1)),
        "corner Unwritten steps 0→1 along X ({shifted:?})"
    );
    assert!(
        shifted.contains(&(2, 7)),
        "top-mid Unwritten steps 2→7 along Y ({shifted:?})"
    );
    // `tell` already applied the events (see the sibling Devouring Margin test) —
    // assert the resulting state directly rather than re-applying (which would double-move).
    assert!(
        e.state().board[1].spirit.is_some() && e.state().board[7].spirit.is_some(),
        "both Unwritten arrived on their exact inward tiles"
    );
    assert!(
        e.state().board[0].spirit.is_none() && e.state().board[2].spirit.is_none(),
        "and left their starting tiles"
    );
}

#[test]
fn the_devouring_margin_forgets_the_mark_it_lands_on() {
    // An eater (The Devouring Margin) shifting inward via The Page Turns onto seat A's impression
    // EATS it: the mark is GONE (the Unwritten leave nothing) and the Solace's erasure tally goes +1
    // — that is how forgetting scores. (The shift once hardcoded `eats_impression: false`, silently
    // killing the mechanic; this guards it.)
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 0, id_of("The Devouring Margin"), Seat::B);
        // Seat A's mark sits on the inward landing tile (the corner shifts 0 → 1).
        st.board[1].impressions = vec![Seat::A];
    }
    let evs = tell(&mut e, "The Page Turns");
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::UnwrittenShifted {
                from: 0,
                to: 1,
                eats_impression: true
            }
        )),
        "the eater's inward shift onto the player's mark eats it ({evs:?})"
    );
    // `tell` already applied the events; assert the resulting state directly. (Re-applying them
    // would double-count non-idempotent effects like the eat tally.)
    assert!(
        e.state().board[1].impressions.is_empty(),
        "the eaten mark is gone — the Unwritten leave nothing"
    );
    assert_eq!(
        e.state().solace_erasures,
        1,
        "forgetting scores: the Solace's erasure tally goes +1"
    );
}

#[test]
fn the_solace_banish_tallies_off_board_and_leaves_no_mark() {
    // The combat half of the erasure model: when the Solace banishes a player spirit, the mark it
    // would lay is instead an off-board tally (the Unwritten leave nothing). `lay_mark` keys this on
    // the banisher's faction, so a prior player mark on the landing tile is erased too.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(1, cat, deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Lorekeeper, Faction::Solace];
        st.board[5].impressions = vec![Seat::A];
    }
    e.apply_event_for_test(Event::SpiritDissolved {
        tile: 5,
        impression: Seat::B,
    });
    assert!(
        e.state().board[5].impressions.is_empty(),
        "the Solace leaves no impression where it banishes"
    );
    assert_eq!(
        e.state().solace_erasures,
        1,
        "the erasure is banked off-board"
    );
}

#[test]
fn a_lorekeeper_banish_lays_its_mark_and_tallies_nothing() {
    // The same banish for a Lorekeeper (the PvP default) stamps a scoring impression and never
    // touches the tally — the asymmetry is purely the Solace's.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(1, cat, deck.clone(), deck);
    e.apply_event_for_test(Event::SpiritDissolved {
        tile: 5,
        impression: Seat::B,
    });
    assert_eq!(
        e.state().board[5].impressions,
        vec![Seat::B],
        "a Lorekeeper banish leaves its mark"
    );
    assert_eq!(
        e.state().solace_erasures,
        0,
        "no off-board tally for a Lorekeeper"
    );
}

#[test]
fn the_solace_plays_an_unwriting_event_from_hand() {
    // The production play path: the Solace plays an Unwriting event via the TellUnwriting
    // command — its effect fires (OnPlay) and the one-shot is discarded from hand.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    e.apply(Seat::A, Command::EndTurn)
        .expect("A ends → the Solace acts");
    {
        let st = e.state_mut_for_test();
        st.player_b.hand = vec![id_of("The Page Turns")];
        st.player_b.anima = 9;
        recollect_core::test_support::put_spirit(st, 0, id_of("Forgotten Name"), Seat::B);
        st.board[0].spirit.as_mut().unwrap().is_token = true;
    }
    let evs = e
        .apply(Seat::B, Command::TellUnwriting { hand_index: 0 })
        .expect("the Solace plays the Unwriting event from hand");
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::UnwrittenShifted { from: 0, .. })),
        "playing The Page Turns shifts the rim Unwritten inward ({evs:?})"
    );
    assert!(
        e.state().player_b.hand.is_empty(),
        "the one-shot Unwriting is discarded after it's played"
    );
}

#[test]
fn a_mercy_for_the_rim_releases_held_rim_spirits() {
    let (mut e, _) = pve_with("Forgotten Name", 13); // off-rim, won't be released
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 0, id_of("Cloudling"), Seat::A);
        st.board[0].spirit.as_mut().unwrap().holding = true; // held, on the rim
    }
    let evs = tell(&mut e, "A Mercy for the Rim");
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritReleased { tile: 0 })),
        "A Mercy for the Rim releases the held rim spirit ({evs:?})"
    );
}

#[test]
fn lost_paragraph_manifests_an_unwritten_on_the_rim() {
    let (mut e, _) = pve_with("Forgotten Name", 13);
    let evs = tell(&mut e, "Lost Paragraph");
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::UnwrittenManifested { tile, .. }
            if [0u8,1,2,3,4,5,9,10,14,15,19,20,21,22,23,24].contains(tile))),
        "Lost Paragraph manifests an Unwritten on a rim tile ({evs:?})"
    );
}

#[test]
fn let_it_lie_stops_impressions_from_scoring() {
    let (mut e, _) = pve_with("Forgotten Name", 13);
    let evs = tell(&mut e, "Let It Lie");
    for ev in evs {
        e.apply_event_for_test(ev);
    }
    assert!(
        e.state().impressions_dormant_round.is_some(),
        "Let It Lie marks impressions dormant"
    );
}

#[test]
fn silence_spreads_silences_a_landmark() {
    let (mut e, _) = pve_with("Forgotten Name", 13);
    {
        let st = e.state_mut_for_test();
        st.board[12].terrain = Some(recollect_core::state::Terrain {
            card: id_of("Rainpool"),
            owner: Seat::A,
            kind: recollect_core::state::TerrainKind::Landmark,
            face_down: false,
        });
    }
    let evs = tell(&mut e, "Silence Spreads");
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::LandmarkSilenced { tile: 12, .. })),
        "Silence Spreads silences the Landmark ({evs:?})"
    );
}

#[test]
fn the_quiet_spreads_makes_inner_tiles_uncallable() {
    let (mut e, _) = pve_with("Forgotten Name", 13);
    let evs = tell(&mut e, "The Quiet Spreads");
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::TilesGoneCalm { .. })),
        "The Quiet Spreads schedules calm tiles ({evs:?})"
    );
    for ev in evs {
        e.apply_event_for_test(ev);
    }
    assert!(!e.state().calm_tiles.is_empty(), "calm tiles are recorded");
}

#[test]
fn the_half_remembered_copies_the_last_played_spirit() {
    // OnReveal/SelfSpirit/CopyLastPlayed: revealing The Half-Remembered copies the owner's
    // last face-up spirit's stats.
    let cat = canon_catalog();
    let target = cat
        .iter()
        .find(|c| format!("{:?}", c.kind) == "Spirit" && c.attack >= 20)
        .expect("a spirit with attack >= 20 exists");
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.last_played_spirit[Seat::B as usize] = Some(target.id);
        recollect_core::test_support::put_spirit(st, 12, id_of("The Half-Remembered"), Seat::B);
        st.board[12].spirit.as_mut().unwrap().face_down = true;
    }
    e.fire_arrival_effects_for_test(12, Seat::B);
    let sp = e.state().board[12].spirit.as_ref().unwrap().clone();
    assert_eq!(
        (sp.attack, sp.defense, sp.hp_max),
        (target.attack, target.defense, target.hp),
        "The Half-Remembered copies the last-played spirit's stats"
    );
}

#[test]
fn you_were_never_really_here_banishes_one_adjacent_enemy_without_impression() {
    // OnPlay/AdjacentEnemyChoose/Banish — the aggressive erasure the set lacked: banish a
    // HEALTHY adjacent enemy leaving NO impression, so the keeper loses the spirit AND the
    // score. Banish (not the mercy Release) is what takes the LIVING — Release spares a
    // non-fading spirit; this card must not.
    let (mut e, tile) = pve_with("You Were Never Really Here", 12);
    // The neighbour is HEALTHY (not fading) — the whole point is taking the living.
    assert!(
        !e.state().board[(tile - 1) as usize]
            .spirit
            .as_ref()
            .unwrap()
            .fading
    );
    let evs = e.fire_arrival_effects_for_test(tile, Seat::B);
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritReleased { .. })),
        "banishes an adjacent HEALTHY enemy via Banish (events: {evs:?})"
    );
    assert!(e.state().spirit_at(tile - 1).is_none(), "the enemy is gone");
    assert!(
        e.state().board[(tile - 1) as usize]
            .impressions
            .first()
            .copied()
            .is_none(),
        "banished as if it never was — no impression, the whole point"
    );
}

#[test]
fn i_too_can_create_desolation_banishes_every_adjacent_enemy_without_impression() {
    // OnPlay/AdjacentEnemiesAll/Banish — the boss desolation: every adjacent (HEALTHY) enemy
    // erased at once, none leaving an impression (distinct from The Mercy Itself, which only
    // releases the FADING and also frees allies). Banish takes the living; Release would not.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    let tile = 12u8;
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(
            st,
            tile,
            id_of("I Too Can Create Desolation"),
            Seat::B,
        );
        recollect_core::test_support::put_spirit(st, tile - 1, id_of("Cloudling"), Seat::A);
        recollect_core::test_support::put_spirit(st, tile + 1, id_of("Cloudling"), Seat::A);
    }
    let evs = e.fire_arrival_effects_for_test(tile, Seat::B);
    let released = evs
        .iter()
        .filter(|ev| matches!(ev, Event::SpiritReleased { .. }))
        .count();
    assert!(
        released >= 2,
        "desolation banishes every adjacent enemy (events: {evs:?})"
    );
    assert!(
        e.state().spirit_at(tile - 1).is_none() && e.state().spirit_at(tile + 1).is_none(),
        "both adjacent enemies are gone"
    );
    assert!(
        e.state().board[(tile - 1) as usize]
            .impressions
            .first()
            .copied()
            .is_none()
            && e.state().board[(tile + 1) as usize]
                .impressions
                .first()
                .copied()
                .is_none(),
        "no impressions — desolation leaves nothing"
    );
}

// ── Red-team: effects that were authored + ratcheted but FIRED NOTHING (dead effects) ──
// Each card below has an authored EffectSpec but its clause shape reached no executor, so
// the effect was a silent no-op (the class the ratchet can't catch for non-deck cards).
// These outcome-asserting tests pin the now-live behavior so the dead state can't recur.

/// PvE engine, seat B = the Solace, decks emptied so EndTurn → B's Flow fires AtFlow.
fn pve_flow() -> Engine {
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Lorekeeper, Faction::Solace];
        st.active = Seat::A;
        st.active_slot = SeatSlot::A1;
        st.player_a.deck.clear();
        st.player_b.deck.clear();
    }
    e
}

#[test]
fn quiet_tide_heals_itself_at_its_flow() {
    // AtFlow/SelfSpirit/RestoreForm{50}: "it heals fully at end of round." This was DEAD —
    // fire_at_flow handled no SelfSpirit/RestoreForm shape, so a wounded Quiet Tide stayed
    // wounded forever. Now its own Flow restores it.
    let mut e = pve_flow();
    let tide = 12u8;
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, tide, id_of("Quiet Tide"), Seat::B);
        st.board[tide as usize].spirit.as_mut().unwrap().hp = 5;
    }
    let before = e.state().board[tide as usize].spirit.as_ref().unwrap().hp;
    e.apply(Seat::A, Command::EndTurn).unwrap(); // → B's Flow
    let after = e.state().board[tide as usize].spirit.as_ref().unwrap().hp;
    assert!(
        after > before,
        "Quiet Tide heals at its Flow (hp {before} → {after}) — was dead, healed nothing"
    );
    assert_eq!(
        after,
        e.state().board[tide as usize]
            .spirit
            .as_ref()
            .unwrap()
            .hp_max,
        "and the heal is FULL (RestoreForm{{50}} caps at hp_max)"
    );
}

#[test]
fn the_last_warm_page_erodes_one_adjacent_impression_at_its_flow() {
    // AtFlow/Owner/ImpressionRemoveTarget: "one ADJACENT impression fades (gentle erosion)."
    // This was DEAD — fire_at_flow had no Owner/ImpressionRemoveTarget shape, so the page
    // eroded nothing. Now it erases one adjacent enemy mark; the Solace forgetting an
    // existing mark SCORES (the erasure tally +1).
    let mut e = pve_flow();
    let page = 12u8;
    let adj = 7u8; // (2,1) adjacent to 12 (2,2)
    let far = 0u8; // a non-adjacent mark must SURVIVE
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, page, id_of("The Last Warm Page"), Seat::B);
        st.board[adj as usize].impressions = vec![Seat::A];
        st.board[far as usize].impressions = vec![Seat::A];
    }
    let tally0 = e.state().solace_erasures;
    e.apply(Seat::A, Command::EndTurn).unwrap(); // → B's Flow
    assert!(
        e.state().board[adj as usize].impressions.is_empty(),
        "the ADJACENT mark eroded at the page's Flow — was dead, eroded nothing"
    );
    assert_eq!(
        e.state().board[far as usize].impressions,
        vec![Seat::A],
        "a NON-adjacent mark is untouched (erosion is adjacent-only)"
    );
    assert_eq!(
        e.state().solace_erasures,
        tally0 + 1,
        "the Solace banks the forgetting (erasure tally +1)"
    );
}

#[test]
fn faultline_damages_all_adjacent_spirits_when_one_dissolves_on_it() {
    // OnAnyBanish/Damage{10}: "when a spirit fully dissolves HERE: 10 damage to all adjacent
    // spirits." This was DEAD — a Landmark is not a standing OnAnyBanish witness, so the
    // terrain's clause never fired. Now a dissolve on the Faultline tile damages every
    // adjacent spirit (both owners — the card says "all adjacent spirits", a double edge).
    let mut e = pve_flow();
    let fault = 12u8;
    let enemy = 11u8; // (1,2) adjacent — seat B (enemy of the A-owned Faultline)
    let ally = 13u8; // (3,2) adjacent — seat A (the Faultline owner's own side)
    {
        let st = e.state_mut_for_test();
        st.board[fault as usize].terrain = Some(Terrain {
            kind: TerrainKind::Landmark,
            card: id_of("Faultline"),
            owner: Seat::A,
            face_down: false,
        });
        // A seat-A spirit standing ON the Faultline, about to dissolve there.
        recollect_core::test_support::put_spirit(st, fault, id_of("Cloudling"), Seat::A);
        recollect_core::test_support::put_spirit(st, enemy, id_of("Cloudling"), Seat::B);
        recollect_core::test_support::put_spirit(st, ally, id_of("Cloudling"), Seat::A);
        // Stage the standing-on-faultline spirit as a combat fade (dissolves at A's turn-end).
        let sp = st.board[fault as usize].spirit.as_mut().unwrap();
        sp.fading = true;
        sp.banished_by = Some(Seat::B);
        sp.fade_deadline = Some(st.round);
    }
    let e_before = e.state().board[enemy as usize].spirit.as_ref().unwrap().hp;
    let a_before = e.state().board[ally as usize].spirit.as_ref().unwrap().hp;
    let evs = e.force_fade_step_for_test(Seat::A);
    let e_after = e.state().board[enemy as usize].spirit.as_ref().unwrap().hp;
    let a_after = e.state().board[ally as usize].spirit.as_ref().unwrap().hp;
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::EffectDamaged { amount: 10, .. })),
        "Faultline deals effect-damage when a spirit dissolves on it — was dead ({evs:?})"
    );
    assert_eq!(e_before - e_after, 10, "the adjacent ENEMY took 10");
    assert_eq!(
        a_before - a_after,
        10,
        "the adjacent ALLY took 10 too — 'all adjacent spirits', not enemies-only"
    );
}

#[test]
fn the_unsaid_cruelty_burns_an_adjacent_enemy_when_it_defeats_a_spirit() {
    // OnDefeat/AdjacentEnemyChoose/Damage{10}: "when it defeats a spirit, an adjacent enemy
    // takes 10." This was DEAD — OnDefeat fires in Open mode, and AdjacentEnemyChoose/Damage
    // hit the choose-handler's `_ => return` (no Damage arm), so the spite never spread. Now
    // it resolves by doctrine to an adjacent enemy.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    e.state_mut_for_test().rules.factions = [Faction::Lorekeeper, Faction::Solace];
    let cruelty = 12u8;
    let victim = 11u8; // the spirit it defeats
    let bystander = 13u8; // another adjacent enemy — should take the 10 spite
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, cruelty, id_of("The Unsaid Cruelty"), Seat::B);
        {
            let c = st.board[cruelty as usize].spirit.as_mut().unwrap();
            c.attack = 200; // overkill the victim and survive
            c.hp = 100;
            c.hp_max = 100;
        }
        recollect_core::test_support::put_spirit(st, victim, id_of("Cloudling"), Seat::A);
        {
            let v = st.board[victim as usize].spirit.as_mut().unwrap();
            v.hp = 10;
            v.attack = 0; // no retaliation
        }
        recollect_core::test_support::put_spirit(st, bystander, id_of("Cloudling"), Seat::A);
        st.board[bystander as usize].spirit.as_mut().unwrap().hp = 40;
    }
    let by_before = e.state().board[bystander as usize]
        .spirit
        .as_ref()
        .unwrap()
        .hp;
    let evs = e.resolve_engage_for_test(cruelty, victim);
    // A combat banish leaves the victim standing-Faded (the D1 window), not gone — what
    // matters is OnDefeat fired (the victim is defeated/fading).
    assert!(
        e.state()
            .spirit_at(victim)
            .map(|s| s.fading)
            .unwrap_or(true),
        "the victim is defeated (precondition for OnDefeat)"
    );
    let by_after = e.state().board[bystander as usize]
        .spirit
        .as_ref()
        .unwrap()
        .hp;
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::EffectDamaged { amount: 10, .. })),
        "The Unsaid Cruelty's spite spread on defeat — was dead ({evs:?})"
    );
    assert_eq!(
        by_before - by_after,
        10,
        "the adjacent enemy took the 10 spite"
    );
}
