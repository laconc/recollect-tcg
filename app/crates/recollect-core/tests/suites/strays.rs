//! Strays — surfacing law (inner-only, denial counterplay), courtship,
//! befriending, banishing.
use recollect_core::Engine;
use recollect_core::state::{Command, Event, Stray, StrayTelegraph, Temperament};
use recollect_core::types::{CardDef, CardId, CardKind, Reach, Resonance, Seat};

fn cat() -> Vec<CardDef> {
    let filler = CardDef {
        id: CardId(0),
        name: "Bloom Ally".into(),
        cost: 1,
        attack: 10,
        defense: 0,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Harmony,
        kind: CardKind::Spirit,
        imprints: vec!["Bloom".into()],
        ..Default::default()
    };
    let gentle = CardDef {
        id: CardId(1),
        name: "Lost Lamb".into(),
        cost: 0,
        attack: 10,
        defense: 0,
        hp: 30,
        reach: Reach::Cross,
        resonance: Resonance::Neutral,
        kind: CardKind::Foundling,
        rarity: "G".into(),
        imprints: vec!["Bloom".into()],
        rules: "Gentle. follows the kind".into(),
        ..Default::default()
    };
    vec![filler, gentle]
}

#[test]
fn an_occupied_telegraph_tile_means_the_stray_does_not_come() {
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat(), deck.clone(), deck);
    // Construct a due telegraph on an inner tile, then occupy it.
    let st = e.state_mut_for_test();
    st.stray_telegraph = Some(StrayTelegraph {
        tile: 12,
        surface_round: st.round,
        midnight: false,
    });
    // Occupy tile 12 with a spirit.
    recollect_core::test_support::put_spirit(st, 12, CardId(0), Seat::A);
    // Advance a turn so the surfacing pass runs.
    let evs = e.apply(Seat::A, Command::EndTurn).unwrap();
    assert!(
        e.state().stray.is_none(),
        "the clearing was filled — the wild stays away"
    );
    // …and the cancelled telegraph must be CLEARED in the COMMITTED state (not just on
    // decide's clone): a `StrayTelegraphCleared` event carries the drop through `evolve`.
    // Without the event the shimmer would linger forever in the view (a lost mutation).
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::StrayTelegraphCleared)),
        "a cancelled surfacing emits StrayTelegraphCleared: {evs:?}"
    );
    assert!(
        e.state().stray_telegraph.is_none(),
        "the phantom shimmer is gone from committed state"
    );
}

#[test]
fn a_surfaced_gentle_stray_is_befriended_by_an_adjacent_shared_imprint() {
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat(), deck.clone(), deck);
    let st = e.state_mut_for_test();
    // A surfaced Gentle Stray at tile 12; a Bloom ally adjacent at 7.
    st.stray = Some(Stray {
        card: CardId(1),
        tile: 12,
        temperament: Temperament::Gentle,
        veiled: false,
        courtship: 0,
        courted_by: None,
        hp: 40,
        hp_max: 40,
    });
    recollect_core::test_support::put_spirit(st, 7, CardId(0), Seat::A);
    // The acting seat's next turn-start runs courtship: shared "Bloom" → befriend.
    e.apply(Seat::A, Command::EndTurn).unwrap(); // → B
    e.apply(Seat::B, Command::EndTurn).unwrap(); // → A, courtship runs
    assert!(e.state().stray.is_none(), "befriended");
    let sp = e.state().board[12]
        .spirit
        .as_ref()
        .expect("now an owned spirit");
    assert_eq!(sp.owner, Seat::A);
    assert_eq!(sp.card, CardId(1));
}

#[test]
fn befriending_pigeon_fires_its_onbefriend_and_the_befriender_draws() {
    // OnBefriend fires through the generic `fire_doctrine` dispatch. Pigeon Carrying a
    // Message Never Delivered: "the message is for whoever befriends it" → its OnBefriend draws
    // for the befriending seat. We trace a REAL Pigeon (canon catalog + its authored Draw clause)
    // through befriending and PROVE the trigger fires AND the draw executes.
    //
    // Red-team (the standing lesson): a passing test ≠ a live effect. The befriend lands at A's
    // turn-start, where A ALSO takes its income draw — so a bare "A's deck shrank" assertion would
    // pass even if OnBefriend were dead. We isolate the effect with a CONTROL: the same scenario
    // with a no-effect Foundling draws ONE card at A's turn-start (income only); the Pigeon draws
    // TWO (income + OnBefriend). The +1 delta is the OnBefriend draw, and nothing else.
    use recollect_core::cards::canon_catalog;
    let cat = canon_catalog();
    let id_of = |name: &str| cat.iter().find(|c| c.name == name).unwrap().id;
    let pigeon = id_of("Pigeon Carrying a Message Never Delivered"); // Gentle, imprints [Song, Wanderer]
    let moth = id_of("Moth of Small Hours"); // a Wanderer spirit with no effects (clean courter)
    // Cat From Three Houses Down — a Gentle Foundling sharing Wanderer (so the same Moth befriends
    // it) but with NO OnBefriend (no effects at all): the income-only control.
    let cat_control = id_of("Cat From Three Houses Down");

    // Run the befriend at A's turn-start and report (befriend fired?, A's CardDrawn count there).
    let run = |foundling: CardId| -> (Vec<Event>, usize) {
        let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
        let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
        {
            let st = e.state_mut_for_test();
            let d = cat.iter().find(|c| c.id == foundling).unwrap();
            st.stray = Some(Stray {
                card: foundling,
                tile: 12,
                temperament: Temperament::Gentle,
                veiled: false,
                courtship: 0,
                courted_by: None,
                hp: d.hp,
                hp_max: d.hp,
            });
            recollect_core::test_support::put_spirit(st, 7, moth, Seat::A); // adjacent Wanderer ally
        }
        e.apply(Seat::A, Command::EndTurn).unwrap(); // → B
        let evs = e.apply(Seat::B, Command::EndTurn).unwrap(); // → A turn-start: courtship + befriend
        let a_draws = evs
            .iter()
            .filter(|ev| matches!(ev, Event::CardDrawn { seat: Seat::A }))
            .count();
        (evs, a_draws)
    };

    let (pigeon_evs, pigeon_draws) = run(pigeon);
    let (control_evs, control_draws) = run(cat_control);
    // Sanity: the control IS befriended too (same courter, shared Wanderer) — so the only
    // difference between the runs is the Pigeon's OnBefriend, not whether befriending happened.
    assert!(
        control_evs
            .iter()
            .any(|ev| matches!(ev, Event::StrayBefriended { seat: Seat::A, .. })),
        "control Foundling is also befriended (events: {control_evs:?})"
    );

    // Precondition: the Pigeon was actually befriended (the trigger's firing moment occurred).
    assert!(
        pigeon_evs.iter().any(|ev| matches!(
            ev,
            Event::StrayBefriended { seat: Seat::A, card, .. } if *card == pigeon
        )),
        "the Pigeon is befriended by the adjacent Song ally (events: {pigeon_evs:?})"
    );
    // The live effect, isolated from income by the no-OnBefriend control: the Pigeon draws exactly
    // one MORE than the control at the same turn-start — that extra draw is OnBefriend, dispatched
    // through `fire_doctrine` and executed by the Owner/Draw clause.
    assert_eq!(
        pigeon_draws,
        control_draws + 1,
        "Pigeon's OnBefriend adds a draw over the income-only control \
         (pigeon {pigeon_draws} vs control {control_draws})"
    );
}

#[test]
fn banishing_a_stray_leaves_the_banishers_impression() {
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat(), deck.clone(), deck);
    let st = e.state_mut_for_test();
    st.stray = Some(Stray {
        card: CardId(1),
        tile: 12,
        temperament: Temperament::Gentle,
        veiled: false,
        courtship: 0,
        courted_by: None,
        hp: 40,
        hp_max: 40,
    });
    let evs = e.apply(Seat::A, Command::BanishStray).unwrap();
    assert!(evs.iter().any(|ev| matches!(
        ev,
        Event::StrayBanished {
            tile: 12,
            impression: Seat::A
        }
    )));
    assert!(e.state().stray.is_none());
    assert_eq!(
        e.state().board[12].impressions.first().copied(),
        Some(Seat::A)
    );
}

// --- further Stray coverage: seeding/pity, Wary veil, Feral interception+Echo, Midnight ---

#[test]
fn stray_match_seeding_is_one_in_seven_and_telegraphs() {
    // The 1-in-7 roll is seeded from the match seed, independent of play. Over
    // many seeds, ~1/7 of matches are stray-matches, and each stray-match has a
    // telegraph generated at construction.
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let mut stray_matches = 0;
    let n = 700;
    for seed in 0..n {
        let (e, _) = Engine::new(seed, cat(), deck.clone(), deck.clone());
        if e.state().stray_match {
            stray_matches += 1;
            assert!(
                e.state().stray_telegraph.is_some(),
                "a stray-match telegraphs its surfacing"
            );
        }
    }
    // ~1/7 = ~100; allow a wide band (binomial noise) but confirm it's neither 0 nor all.
    assert!(
        stray_matches > 60 && stray_matches < 160,
        "roughly 1-in-7 matches host a Stray: {stray_matches}/{n}"
    );
}

#[test]
fn wary_strays_unveil_by_adjacency() {
    let mut c = cat();
    c.push(CardDef {
        id: CardId(2),
        name: "Shy Fawn".into(),
        cost: 0,
        attack: 10,
        defense: 0,
        hp: 30,
        reach: Reach::Cross,
        resonance: Resonance::Neutral,
        kind: CardKind::Foundling,
        rarity: "G".into(),
        imprints: vec!["Bloom".into()],
        rules: "Wary. watches first".into(),
        ..Default::default()
    });
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, c, deck.clone(), deck);
    let st = e.state_mut_for_test();
    // A VEILED Wary Stray at 12; a Bloom ally adjacent at 7.
    st.stray = Some(Stray {
        card: CardId(2),
        tile: 12,
        temperament: Temperament::Wary,
        veiled: true,
        courtship: 0,
        courted_by: None,
        hp: 30,
        hp_max: 30,
    });
    recollect_core::test_support::put_spirit(st, 7, CardId(0), Seat::A);
    e.apply(Seat::A, Command::EndTurn).unwrap(); // → B
    let evs = e.apply(Seat::B, Command::EndTurn).unwrap(); // → A turn-start: courtship
    assert!(
        evs.iter().any(|ev| matches!(ev, Event::StrayUnveiled)),
        "a teller ending adjacent unveils the Wary Stray"
    );
    assert!(!e.state().stray.as_ref().unwrap().veiled, "it is now seen");
}

#[test]
fn feral_strays_are_befriendable_only_once_echo_wounded() {
    let mut c = cat();
    c.push(CardDef {
        id: CardId(2),
        name: "Cornered Lynx".into(),
        cost: 0,
        attack: 30,
        defense: 10,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Neutral,
        kind: CardKind::Foundling,
        rarity: "G".into(),
        imprints: vec!["Bloom".into()],
        rules: "Feral. fights then trusts".into(),
        ..Default::default()
    });
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, c, deck.clone(), deck);
    let st = e.state_mut_for_test();
    // A Feral Stray at FULL hp — adjacency must NOT befriend it.
    st.stray = Some(Stray {
        card: CardId(2),
        tile: 12,
        temperament: Temperament::Feral,
        veiled: false,
        courtship: 0,
        courted_by: None,
        hp: 40,
        hp_max: 40,
    });
    recollect_core::test_support::put_spirit(st, 7, CardId(0), Seat::A);
    e.apply(Seat::A, Command::EndTurn).unwrap(); // → B
    e.apply(Seat::B, Command::EndTurn).unwrap(); // → A turn-start: courtship runs
    assert!(
        e.state().stray.is_some(),
        "still wild at full HP (Feral needs an Echo)"
    );
    // Now wound it below half (an Echo) and let A's turn-start come round again.
    if let Some(s) = e.state_mut_for_test().stray.as_mut() {
        s.hp = 15;
    } // < 20 = half of 40
    e.apply(Seat::A, Command::EndTurn).unwrap(); // → B
    let evs = e.apply(Seat::B, Command::EndTurn).unwrap(); // → A turn-start
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::StrayBefriended { .. })),
        "an Echo-wounded Feral Stray, courted by a shared-Imprint ally, is befriended"
    );
}

// --- §2: Overwrite reaches a Stray (the ruling: revealed → fought; hidden → denied) ---

/// A small catalog for the Overwrite-onto-Stray cases: a projection anchor, a revealed
/// Gentle Stray that retaliates, a STRONG overwriter (one-shots the wild), a GLASS
/// overwriter (cannot kill it and dies to the retaliation), and a veiled Wary Stray.
fn cat_ow() -> Vec<CardDef> {
    let base = |id, name: &str, attack, defense, hp, kind, rules: &str| CardDef {
        id: CardId(id),
        name: name.into(),
        cost: 1,
        attack,
        defense,
        hp,
        reach: Reach::Cross,
        resonance: Resonance::Neutral,
        kind,
        rarity: "G".into(),
        imprints: vec!["Bloom".into()],
        rules: rules.into(),
        ..Default::default()
    };
    vec![
        base(0, "Bloom Ally", 10, 0, 40, CardKind::Spirit, ""),
        base(
            1,
            "Lost Lamb",
            10,
            0,
            30,
            CardKind::Foundling,
            "Gentle. follows the kind",
        ),
        // Strong overwriter: 40 atk fells a 30-HP Stray outright; 50 HP survives the bite.
        base(3, "Kiln Bull", 40, 0, 50, CardKind::Spirit, ""),
        // Glass overwriter: 10 atk can't fell a 30-HP Stray; 5 HP and 0 def dies to the
        // Stray's 10-atk retaliation — so it dissolves and the wild survives, wounded.
        base(4, "Spark Wisp", 10, 0, 5, CardKind::Spirit, ""),
        // A veiled Wary Stray (its identity hidden until unveiled).
        base(
            5,
            "Shy Fawn",
            10,
            0,
            30,
            CardKind::Foundling,
            "Wary. watches first",
        ),
    ]
}

/// Stand a Stray on the centre inner tile (12) with an A ally adjacent (7) so A's
/// projection reaches it, then return the live engine on A's turn.
fn engine_with_stray(stray: Stray, a_hand: &[u16]) -> Engine {
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat_ow(), deck.clone(), deck);
    let st = e.state_mut_for_test();
    recollect_core::test_support::put_spirit(st, 7, CardId(0), Seat::A); // projector
    st.player_a.hand = a_hand.iter().map(|i| CardId(*i)).collect();
    st.player_a.anima = 20;
    st.player_a.first_placement_done = true;
    st.stray = Some(stray);
    e
}

fn gentle_at_12(hp: i16) -> Stray {
    Stray {
        card: CardId(1),
        tile: 12,
        temperament: Temperament::Gentle,
        veiled: false,
        courtship: 0,
        courted_by: None,
        hp,
        hp_max: 30,
    }
}

#[test]
fn overwriting_a_revealed_stray_that_falls_takes_the_tile_and_lays_the_impression() {
    // §2: a revealed Stray is fought. A strong overwriter fells it → the overwriter takes
    // the cleared tile, the wild's slot empties, and the banisher's impression sits beneath
    // (here a Lorekeeper, so a board mark for A).
    let mut e = engine_with_stray(gentle_at_12(30), &[3]); // play Kiln Bull (40 atk)
    let evs = e
        .apply(
            Seat::A,
            Command::Overwrite {
                hand_index: 0,
                tile: 12,
            },
        )
        .expect("the Overwrite onto the revealed Stray resolves");
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::OverwroteStray {
                tile: 12,
                success: true,
                ..
            }
        )),
        "the wild fell to the overwriter: {evs:?}"
    );
    assert!(
        e.state().stray.is_none(),
        "the Stray slot is empty — it was banished"
    );
    let sp = e.state().board[12]
        .spirit
        .as_ref()
        .expect("the overwriter stands on the tile");
    assert_eq!(sp.owner, Seat::A);
    assert_eq!(sp.card, CardId(3));
    assert!(!sp.fading, "a healthy overwriter stands");
    // The banisher's impression sits beneath (a Lorekeeper lays a board mark).
    assert_eq!(
        e.state().board[12].impressions.first().copied(),
        Some(Seat::A),
        "the banisher's impression is laid beneath the overwriter"
    );
    recollect_core::invariants::check(e.state()).unwrap();
}

#[test]
fn overwriting_a_revealed_stray_that_survives_dissolves_the_overwriter() {
    // §2: the occupant survives → the overwriter dissolves, no impression; the damage it
    // dealt persists on the wild. A glass overwriter (10 atk, 5 HP, 0 def) can't fell a
    // 30-HP Stray and dies to its 10-atk retaliation.
    let mut e = engine_with_stray(gentle_at_12(30), &[4]); // play Spark Wisp (glass)
    let evs = e
        .apply(
            Seat::A,
            Command::Overwrite {
                hand_index: 0,
                tile: 12,
            },
        )
        .expect("the Overwrite resolves");
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::OverwroteStray {
                tile: 12,
                success: false,
                damage_to_stray: 10,
                ..
            }
        )),
        "the wild survived; the overwriter dealt 10 and dissolved: {evs:?}"
    );
    // The Stray still stands, wounded by the 10 it took.
    let s = e
        .state()
        .stray
        .as_ref()
        .expect("the wild survives in its slot");
    assert_eq!(
        s.hp, 20,
        "the damage the overwriter dealt persists on the wild"
    );
    // The overwriter dissolved: no body on the tile, and NO impression (the failed
    // overwriter leaves nothing).
    assert!(
        e.state().board[12].spirit.is_none(),
        "the dissolved overwriter never took the tile"
    );
    assert!(
        e.state().board[12].impressions.is_empty(),
        "a failed Overwrite lays no impression"
    );
    recollect_core::invariants::check(e.state()).unwrap();
}

#[test]
fn overwriting_a_hidden_wary_stray_denies_it_entry_and_the_overwriter_lands() {
    // The ruling: a HIDDEN (veiled Wary) Stray is denied entry by an Overwrite — it leaves
    // with no impression and no reveal, and the overwriter takes the cleared tile
    // uncontested (no exchange). "Denied entry, and it just disappears."
    let veiled = Stray {
        card: CardId(5),
        tile: 12,
        temperament: Temperament::Wary,
        veiled: true,
        courtship: 0,
        courted_by: None,
        hp: 30,
        hp_max: 30,
    };
    let mut e = engine_with_stray(veiled, &[3]); // any overwriter; it is not fought
    let evs = e
        .apply(
            Seat::A,
            Command::Overwrite {
                hand_index: 0,
                tile: 12,
            },
        )
        .expect("the Overwrite onto the hidden Stray's tile resolves (deny entry)");
    // The hidden thing was denied entry (it leaves), and the overwriter landed uncontested.
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::StrayDenied { tile: 12 })),
        "the hidden Stray is denied entry — it simply leaves: {evs:?}"
    );
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::OverwroteStray {
                tile: 12,
                success: true,
                damage_to_stray: 0,
                ..
            }
        )),
        "the overwriter takes the cleared tile uncontested (no exchange): {evs:?}"
    );
    // No StrayBanished (it was NOT banished — it was denied entry), no reveal/unveil, and
    // NO strike of any kind: the hidden thing was never fought, so no exchange and no
    // Momentum bonus-engage fires (a denied entry is not a defeat).
    assert!(
        !evs.iter().any(|ev| matches!(
            ev,
            Event::StrayBanished { .. }
                | Event::StrayUnveiled
                | Event::StrayStruck { .. }
                | Event::Struck { .. }
        )),
        "denial is not a banish, a reveal, a strike, or a Momentum chain: {evs:?}"
    );
    assert!(e.state().stray.is_none(), "the wild is gone — denied entry");
    let sp = e.state().board[12]
        .spirit
        .as_ref()
        .expect("the overwriter stands");
    assert_eq!(sp.card, CardId(3));
    assert_eq!(
        sp.hp, sp.hp_max,
        "it arrived unwounded — there was nothing to fight"
    );
    // The hidden Stray left NOTHING — no impression forms from the denial itself (the
    // overwriter's own arrival lays its banisher-impression beneath, A's mark, which is
    // the overwriter taking the tile — not a mark from the denied wild).
    assert_eq!(
        e.state().board[12].impressions.first().copied(),
        Some(Seat::A),
        "the impression beneath is the overwriter's own (it took the tile), not the wild's"
    );
    recollect_core::invariants::check(e.state()).unwrap();
}

#[test]
fn a_denied_hidden_stray_never_leaks_its_identity_to_the_opponent() {
    // REDACTION (the core guard for Part 1): a veiled Wary Stray denied entry must NOT
    // surface its `CardId` anywhere in the OPPONENT's serialized view — across the whole
    // deny-entry resolution. The `StrayDenied` event carries only the tile; the Stray is
    // never projected into `PlayerView`; and the overwriter that lands is the actor's own
    // public spirit. So the veil's identity (CardId 5) appears in NO field B can see.
    use recollect_core::view::view_for;
    let veiled = Stray {
        card: CardId(5), // the secret identity that must never leak
        tile: 12,
        temperament: Temperament::Wary,
        veiled: true,
        courtship: 0,
        courted_by: None,
        hp: 30,
        hp_max: 30,
    };
    let mut e = engine_with_stray(veiled, &[3]);
    // Before the deny: B (the opponent) must not see the veiled Stray's identity either.
    let vb_before = serde_json::to_string(&view_for(&e, Seat::B)).unwrap();
    assert!(
        !vb_before.contains("\"card\":5"),
        "the veiled Stray's identity is hidden from the opponent even before the deny: {vb_before}"
    );
    e.apply(
        Seat::A,
        Command::Overwrite {
            hand_index: 0,
            tile: 12,
        },
    )
    .expect("the deny-entry Overwrite resolves");
    // After the deny: the opponent's view still never names the (now-gone) veiled Stray.
    let vb_after = serde_json::to_string(&view_for(&e, Seat::B)).unwrap();
    assert!(
        !vb_after.contains("\"card\":5"),
        "the denied veiled Stray's identity never leaks to the opponent: {vb_after}"
    );
    // And the overwriter that took the tile IS public — its identity (CardId 3) shows,
    // confirming the view is live (the redaction is targeted, not a blanket blackout).
    let va_after = serde_json::to_string(&view_for(&e, Seat::A)).unwrap();
    assert!(
        va_after.contains("\"card\":3"),
        "the overwriter that landed is public in its owner's view: {va_after}"
    );
}

#[test]
fn a_spirit_may_not_be_played_onto_a_strays_tile() {
    // The ruling's corollary: a Stray occupies its tile, so a spirit is OVERWRITTEN onto it,
    // never PLAYED. A direct Play onto the Stray's tile is rejected (TileOccupied) — and the
    // offered menu reflects this: the only command on that tile is the Overwrite.
    let mut e = engine_with_stray(gentle_at_12(30), &[3]);
    let rej = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: 12,
                engage: None,
                chain_prefs: vec![],
            },
        )
        .expect_err("a Play onto the Stray's tile is illegal");
    assert!(
        matches!(rej, recollect_core::engine::Reject::TileOccupied),
        "got {rej:?}"
    );
    // The legal menu offers Overwrite onto tile 12, never PlaySpirit onto it.
    let legal = e.legal_commands(Seat::A);
    assert!(
        legal
            .iter()
            .any(|c| matches!(c, Command::Overwrite { tile: 12, .. })),
        "the menu offers the Overwrite onto the Stray's tile"
    );
    assert!(
        !legal
            .iter()
            .any(|c| matches!(c, Command::PlaySpirit { tile: 12, .. })),
        "the menu never offers a Play onto the Stray's tile"
    );
}

/// Invariant 1b's TERRAIN sibling — a Stray occupies its tile, so neither a **Landmark**
/// nor a **Fabrication** may be placed onto it (terrain + Stray coexist is as illegal as
/// spirit + Stray). Without this guard a Landmark drops onto a Stray's tile, and then an
/// Overwrite onto that Stray (the §2 revealed-Stray fight) lands the overwriter on top of
/// the terrain — the illegal `spirit AND terrain coexist` state (invariant #1). This was a
/// LIVE bug a full-catalog playthrough hit (seed 21863 step 44): the placement handlers and
/// `legal_commands` rejected a faded / spirit-held / terrain-held tile but not a Stray's.
#[test]
fn terrain_may_not_be_placed_onto_a_strays_tile() {
    use recollect_core::engine::Reject;
    // A catalog with a Landmark and a Fabrication A can place. cat_ow() has a spirit (0); add
    // terrain cards locally so the placement paths are exercised against a Stray's tile.
    let mut catalog = cat_ow();
    let mk_terrain = |id: u16, name: &str, kind: CardKind| CardDef {
        id: CardId(id),
        name: name.into(),
        cost: 1,
        attack: 0,
        defense: 0,
        hp: 0,
        reach: Reach::Cross,
        resonance: Resonance::Neutral,
        kind,
        rarity: "C".into(),
        imprints: vec![],
        rules: String::new(),
        ..Default::default()
    };
    catalog.push(mk_terrain(10, "Cairn", CardKind::Landmark));
    catalog.push(mk_terrain(11, "Mirage", CardKind::Fabrication));

    // Stand a Gentle Stray on tile 12 with an A spirit adjacent (11) so tile 12 is in A's
    // projection (and adjacent tiles are too). A holds the Landmark + Fabrication.
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, catalog, deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 11, CardId(0), Seat::A); // projector
        st.player_a.hand = vec![CardId(10), CardId(11)]; // [Landmark, Fabrication]
        st.player_a.anima = 20;
        st.player_a.first_placement_done = true;
        st.stray = Some(gentle_at_12(30));
    }
    // A direct Landmark placement onto the Stray's tile is rejected.
    let rej_l = e
        .apply(
            Seat::A,
            Command::PlaceLandmark {
                hand_index: 0,
                tile: 12,
            },
        )
        .expect_err("a Landmark onto the Stray's tile is illegal");
    assert!(matches!(rej_l, Reject::TileHeld), "got {rej_l:?}");
    // …and a Fabrication too.
    let rej_f = e
        .apply(
            Seat::A,
            Command::SetFabrication {
                hand_index: 1,
                tile: 12,
            },
        )
        .expect_err("a Fabrication onto the Stray's tile is illegal");
    assert!(matches!(rej_f, Reject::TileHeld), "got {rej_f:?}");
    // The offered menu never proposes terrain onto the Stray's tile.
    let legal = e.legal_commands(Seat::A);
    assert!(
        !legal.iter().any(|c| matches!(
            c,
            Command::PlaceLandmark { tile: 12, .. } | Command::SetFabrication { tile: 12, .. }
        )),
        "legal_commands offered terrain onto the Stray's tile (illegal coexistence)"
    );
    // The Stray still stands alone on its tile — no terrain crept on.
    assert!(
        e.state().board[12].terrain.is_none(),
        "no terrain on the Stray's tile"
    );
    assert!(
        e.state()
            .stray
            .as_ref()
            .map(|s| s.tile == 12)
            .unwrap_or(false)
    );
    recollect_core::invariants::check(e.state()).unwrap();
}

#[test]
fn midnight_stray_surfaces_after_the_dusk() {
    // A Midnight stray-match telegraphs a late, post-contraction surfacing.
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let mut found_midnight = false;
    for seed in 0..3000u64 {
        let (e, _) = Engine::new(seed, cat(), deck.clone(), deck.clone());
        if let Some(t) = &e.state().stray_telegraph
            && t.midnight
        {
            found_midnight = true;
            assert!(
                t.surface_round > e.state().rules.contraction_after,
                "Midnight surfaces after the Dusk (round {} > contraction {})",
                t.surface_round,
                e.state().rules.contraction_after
            );
            break;
        }
    }
    assert!(
        found_midnight,
        "some stray-match in 3000 seeds is a Midnight (10% of ~1/7)"
    );
}
