//! The Solace Deepenings — the Solace's 8 **Primal** evolution forms (design §5,
//! "The Solace deepens"). Where a Lorekeeper spirit ascends toward legend, an
//! Unwritten deepens: the erasure sharpens, Primal-only, never Fabled. These tests
//! prove the catalog wiring (each form is a Primal that branches from a Solace
//! creature base), that each form's authored EffectSpec actually does its thing
//! (not just that it fires), and that a Solace deck can hold and LAND them (the
//! base↔Primal pairing is deck-legal and the evolution resolves). The full
//! evolve↔devolve cycle for these is in `devolution.rs`.
use recollect_core::Engine;
use recollect_core::cards::{canon_catalog, validate_deck_for};
use recollect_core::engine::combat_stats_for_test;
use recollect_core::state::{Command, Event};
use recollect_core::types::{CardId, CardKind, Faction, Seat, SeatSlot};

fn id_of(name: &str) -> CardId {
    canon_catalog()
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.id)
        .unwrap_or_else(|| panic!("card not found: {name}"))
}

/// The 12 Deepenings, each as (form, base it recedes to). Four bases appear TWICE —
/// the gentle-or-malign MENUS (The Kind Erasure, Quiet Tide, Grudge-Kept, The Gnawing).
const DEEPENINGS: [(&str, &str); 12] = [
    ("The Kindest Erasure", "The Kind Erasure"),
    ("The Unkindest Erasure", "The Kind Erasure"),
    ("The Last Lullaby", "The Lullaby"),
    ("The Surer Tide", "Quiet Tide"),
    ("The Drowning Tide", "Quiet Tide"),
    ("The Widening Absence", "Negative Space"),
    ("The Long Erasure", "Page-Eater"),
    ("The Grudge Entire", "Grudge-Kept"),
    ("The Grudge Forgiven", "Grudge-Kept"),
    ("The Gnawing Unending", "The Gnawing"),
    ("The Gnawing Stilled", "The Gnawing"),
    ("Spite, Made Whole", "Spite"),
];

/// The four gentle-or-malign menu bases (each offers two Primals of opposing temperament).
const MENU_BASES: [(&str, &str, &str); 4] = [
    // (base, gentle form, malign form)
    (
        "The Kind Erasure",
        "The Kindest Erasure",
        "The Unkindest Erasure",
    ),
    ("Quiet Tide", "The Surer Tide", "The Drowning Tide"),
    ("Grudge-Kept", "The Grudge Forgiven", "The Grudge Entire"),
    ("The Gnawing", "The Gnawing Stilled", "The Gnawing Unending"),
];

#[test]
fn every_deepening_is_a_primal_branching_from_a_solace_creature_never_fabled() {
    let cat = canon_catalog();
    let by_name = |n: &str| cat.iter().find(|c| c.name == n).unwrap();
    for (form, base) in DEEPENINGS {
        let f = by_name(form);
        let b = by_name(base);
        assert_eq!(f.kind, CardKind::Evolution, "{form} is an Evolution form");
        assert_eq!(
            f.rarity, "Primal",
            "{form} is a PRIMAL — the Solace deepens, never ascends to Fabled"
        );
        assert_eq!(
            f.evolves_from.as_deref(),
            Some(base),
            "{form} recedes to {base}"
        );
        assert!(
            b.evolves_to.iter().any(|n| n == form),
            "{base} reaches {form}"
        );
        // The base is a Solace creature (Unwritten or IllIntent) — a true base, not
        // itself a form (the no-chain lock).
        assert!(
            matches!(b.kind, CardKind::Unwritten | CardKind::IllIntent),
            "{base} is a Solace creature"
        );
        assert_eq!(b.evolves_from, None, "{base} is a true base (no chain)");
    }
    // Exactly 12 Solace (Neutral-resonance) Primal Deepenings — a real set, not token.
    let solace_primals = cat
        .iter()
        .filter(|c| {
            c.kind == CardKind::Evolution
                && c.rarity == "Primal"
                && c.evolves_from.as_deref().is_some_and(|b| {
                    cat.iter().any(|x| {
                        x.name == b && matches!(x.kind, CardKind::Unwritten | CardKind::IllIntent)
                    })
                })
        })
        .count();
    assert_eq!(
        solace_primals, 12,
        "the 12 Solace Deepenings (8 seed + 4 menu partners)"
    );
}

#[test]
fn no_deepening_carries_a_fabled_form_the_solace_has_no_apex() {
    // The "Solace deepens, never ascends" invariant in the large: NO Solace creature
    // base reaches a Fabled form (it has no summit, only return).
    let cat = canon_catalog();
    for c in &cat {
        if matches!(c.kind, CardKind::Unwritten | CardKind::IllIntent) {
            for form_name in &c.evolves_to {
                let form = cat.iter().find(|x| &x.name == form_name).unwrap();
                assert_eq!(
                    form.rarity, "Primal",
                    "{} is a Solace base reaching a {} form — the Solace deepens to Primal only",
                    c.name, form.rarity
                );
            }
        }
    }
}

// ── Each Deepening's authored effect actually does the thing ──

/// A PvE engine with `card` (seat B) at tile 12 and a seat-A Cloudling adjacent at 11.
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
fn the_kindest_erasure_releases_every_adjacent_fading_spirit() {
    // OnPlay/AdjacentEnemiesAll/Release: the mercy widened — every adjacent fading
    // enemy is released (no impression). We make the neighbour fading first.
    let (mut e, tile) = pve_with("The Kindest Erasure", 12);
    e.state_mut_for_test().board[(tile - 1) as usize]
        .spirit
        .as_mut()
        .unwrap()
        .fading = true;
    let evs = e.fire_arrival_effects_for_test(tile, Seat::B);
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritReleased { .. })),
        "The Kindest Erasure releases the fading neighbour (events: {evs:?})"
    );
    assert!(
        e.state().board[(tile - 1) as usize].spirit.is_none(),
        "and it leaves — no body"
    );
    assert!(
        e.state().board[(tile - 1) as usize].impressions.is_empty(),
        "and no impression (the Solace mercy)"
    );
}

#[test]
fn the_mercy_release_is_fading_only_a_healthy_enemy_is_spared() {
    // Red-team regression: the merciful Release cards say "release every adjacent
    // *fading* spirit" — they must spare the LIVING. A bug had Release reach every
    // adjacent enemy (healthy included), turning a gentle mercy into a free
    // board-clear. This pins the fading-only contract for every mercy Release card,
    // including The Mercy Itself (the pre-existing board-wide mercy). The aggressive
    // "banish the living" line is Effect::Banish (the IllIntent cards), tested apart.
    for form in [
        "The Kindest Erasure", // gentle Deepening (menu)
        "The Grudge Forgiven", // gentle Deepening (menu)
        "The Mercy Itself",    // pre-existing board-wide mercy
    ] {
        let (mut e, tile) = pve_with(form, 12);
        // The neighbour is HEALTHY — pve_with seats a fresh (non-fading) Cloudling.
        assert!(
            !e.state().board[(tile - 1) as usize]
                .spirit
                .as_ref()
                .unwrap()
                .fading,
            "{form}: the adjacent enemy starts healthy"
        );
        let evs = e.fire_arrival_effects_for_test(tile, Seat::B);
        assert!(
            !evs.iter()
                .any(|ev| matches!(ev, Event::SpiritReleased { .. })),
            "{form}: a HEALTHY enemy must NOT be released by a 'release fading' card ({evs:?})"
        );
        assert!(
            e.state().board[(tile - 1) as usize].spirit.is_some(),
            "{form}: the healthy enemy still stands (the mercy spares the living)"
        );
    }
}

#[test]
fn the_last_lullaby_softens_adjacent_enemies_attack_and_their_echo() {
    // Static: adjacent enemies −10 Attack AND lose Echo eligibility (too soothed).
    let cat = canon_catalog();
    let measure_attack = |aura: &str| -> i16 {
        let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
        let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 12, id_of(aura), Seat::B);
        recollect_core::test_support::put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        combat_stats_for_test(e.state(), &cat, 11).attack
    };
    let with = measure_attack("The Last Lullaby");
    let without = measure_attack("Static"); // a plain Unwritten, no attack aura
    assert_eq!(
        without - with,
        10,
        "The Last Lullaby lowers adjacent enemy Attack by 10 (with={with}, without={without})"
    );
    // Echo suppression: the adjacent enemy can't Echo while the Lullaby stands.
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 12, id_of("The Last Lullaby"), Seat::B);
        recollect_core::test_support::put_spirit(st, 11, id_of("Cloudling"), Seat::A);
    }
    assert!(
        e.echo_suppressed_for_test(11),
        "The Last Lullaby suppresses the adjacent enemy's Echo"
    );
}

#[test]
fn the_surer_tide_heals_its_adjacent_allies_at_flow() {
    // AtFlow/AdjacentAlliesAll/RestoreForm{20}: the calm spreads — each adjacent ally
    // heals 20 at the owner's Flow.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    let tide = 12u8;
    let ally = 11u8; // Cross-adjacent
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, tide, id_of("The Surer Tide"), Seat::B);
        recollect_core::test_support::put_spirit(st, ally, id_of("Static"), Seat::B);
        // Wound the ally so the heal is observable.
        let sp = st.board[ally as usize].spirit.as_mut().unwrap();
        sp.hp = 5;
        // It is A's turn; ending it begins B's turn, whose Flow fires the heal.
        st.active = Seat::A;
        st.active_slot = SeatSlot::A1;
        st.player_a.deck.clear();
        st.player_b.deck.clear();
    }
    let before = e.state().board[ally as usize].spirit.as_ref().unwrap().hp;
    e.apply(Seat::A, Command::EndTurn).unwrap();
    let after = e.state().board[ally as usize].spirit.as_ref().unwrap().hp;
    assert!(
        after > before,
        "The Surer Tide heals its adjacent ally at the Flow (before={before}, after={after})"
    );
}

#[test]
fn the_widening_absence_lowers_adjacent_enemy_defense_by_twenty() {
    // Static aura: −20 Defense to adjacent enemies (the page gives way further) — a
    // deeper cut than its base Negative Space's −10.
    let cat = canon_catalog();
    let measure = |aura: &str| -> i16 {
        let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
        let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 12, id_of(aura), Seat::B);
        recollect_core::test_support::put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        combat_stats_for_test(e.state(), &cat, 11).defense
    };
    let with = measure("The Widening Absence");
    let without = measure("Static");
    assert_eq!(
        without - with,
        20,
        "The Widening Absence lowers adjacent enemy Defense by 20 (with={with}, without={without})"
    );
}

#[test]
fn the_long_erasure_eats_an_enemy_impression_on_arrival_and_it_scores() {
    // OnPlay/Owner/ImpressionRemoveTarget: the hunger sharpened — on arrival it eats
    // one enemy impression on the board. Piloted by the Solace, erasing an existing
    // mark FORGETS it (the off-board tally +1 — forgetting scores). We fire the arrival
    // effect, then resolve the impression-removal choice.
    use recollect_core::state::PendingChoice;
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    let tile = 12u8;
    let mark_tile = 8u8;
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Lorekeeper, Faction::Solace]; // seat B is the Solace
        recollect_core::test_support::put_spirit(st, tile, id_of("The Long Erasure"), Seat::B);
        // A player (seat A) impression sits on a tile — the memory it will eat.
        st.board[mark_tile as usize].impressions = vec![Seat::A];
        st.active = Seat::B;
        st.active_slot = SeatSlot::B1;
    }
    let tally0 = e.state().solace_erasures;
    let _ = e.fire_arrival_effects_for_test(tile, Seat::B);
    let Some(PendingChoice::Target { options, .. }) = e.state().pending_choice.clone() else {
        panic!("The Long Erasure opens an impression-removal choice on arrival");
    };
    let idx = options
        .iter()
        .position(|&t| t == mark_tile)
        .expect("the player's impression is a target") as u8;
    e.apply(Seat::B, Command::Choose { index: idx }).unwrap();
    assert!(
        e.state().board[mark_tile as usize].impressions.is_empty(),
        "the eaten mark is gone"
    );
    assert_eq!(
        e.state().solace_erasures,
        tally0 + 1,
        "and the erasure scores (tally +1) — the Solace banks the forgetting"
    );
}

#[test]
fn the_grudge_entire_gains_attack_per_enemy_impression() {
    // Static/AttackPerEnemyImpression{10}: +10 Attack for each enemy impression on
    // the board — the count made teeth. (Its +20 retaliation is a self-retaliation
    // aura, covered for firing by the static-support credit.)
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    let grudge = 12u8;
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, grudge, id_of("The Grudge Entire"), Seat::B);
    }
    let base_attack = combat_stats_for_test(e.state(), &cat, grudge).attack;
    // Lay two of seat A's (the enemy of B) impressions on empty tiles.
    {
        let st = e.state_mut_for_test();
        st.board[0].impressions = vec![Seat::A];
        st.board[1].impressions = vec![Seat::A];
    }
    let with_two = combat_stats_for_test(e.state(), &cat, grudge).attack;
    assert_eq!(
        with_two - base_attack,
        20,
        "The Grudge Entire gains +10 Attack per enemy impression (+20 for two)"
    );
}

#[test]
fn the_gnawing_unending_scours_all_adjacent_enemies_on_arrival() {
    // OnPlay/AdjacentEnemiesAll/Damage{10}: a deliberate scouring — 10 to every
    // adjacent enemy on arrival.
    let (mut e, tile) = pve_with("The Gnawing Unending", 12);
    let before = e.state().board[(tile - 1) as usize]
        .spirit
        .as_ref()
        .unwrap()
        .hp;
    let evs = e.fire_arrival_effects_for_test(tile, Seat::B);
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::EffectDamaged { tile: t, amount: 10 } if *t == tile - 1)),
        "The Gnawing Unending deals 10 to the adjacent enemy on arrival (events: {evs:?})"
    );
    let after = e.state().board[(tile - 1) as usize]
        .spirit
        .as_ref()
        .map(|s| s.hp)
        .unwrap_or(i16::MIN);
    assert_eq!(before - after, 10, "the scour landed for 10");
}

#[test]
fn spite_made_whole_lashes_an_enemy_in_reach_for_twenty_on_arrival() {
    // OnPlay/TargetEnemySpirit/Damage{20}: the barb deepened — on arrival it lashes
    // one enemy in reach for 20 (it cannot win, but it will not let you win clean).
    // Fire the arrival effect, then resolve the target choice onto the enemy.
    use recollect_core::state::PendingChoice;
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    let spite = 12u8;
    let enemy = 11u8; // Slant reach from 12 includes the diagonals; place a clear enemy in reach
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, spite, id_of("Spite, Made Whole"), Seat::B);
        // A seat-A enemy with a fat HP pool to take the 20 cleanly.
        recollect_core::test_support::put_spirit(st, enemy, id_of("Kilnhorn Rhino"), Seat::A);
        let k = st.board[enemy as usize].spirit.as_mut().unwrap();
        k.hp = 70;
        k.hp_max = 70;
        st.active = Seat::B;
        st.active_slot = SeatSlot::B1;
    }
    let before = e.state().board[enemy as usize].spirit.as_ref().unwrap().hp;
    let _ = e.fire_arrival_effects_for_test(spite, Seat::B);
    // The arrival opens a target choice; pick the enemy in reach.
    let Some(PendingChoice::Target { options, .. }) = e.state().pending_choice.clone() else {
        panic!("Spite, Made Whole opens an arrival-strike target choice");
    };
    let idx = options
        .iter()
        .position(|&t| t == enemy)
        .expect("the enemy is a reachable target") as u8;
    e.apply(Seat::B, Command::Choose { index: idx }).unwrap();
    let after = e.state().board[enemy as usize].spirit.as_ref().unwrap().hp;
    assert_eq!(
        before - after,
        20,
        "Spite, Made Whole lashes the chosen enemy for 20 on arrival (hp {before} → {after})"
    );
}

// ── A Solace deck can HOLD and LAND a Deepening ──

#[test]
fn a_solace_deck_can_hold_a_deepening_and_evolve_a_banished_base() {
    // The end-to-end: a Solace creature base banished in combat lingers standing-Faded
    // (D1), and its Primal Deepening — a card the Solace holds — is played onto it. The
    // base must be one of the Solace's own (a true Unwritten/IllIntent base).
    let cat = canon_catalog();
    let base_id = id_of("Spite"); // an IllIntent base with a Deepening
    let form_id = id_of("Spite, Made Whole");
    // A legal singleton Solace deck holding the base↔form pair.
    let mut deck: Vec<CardId> = cat
        .iter()
        .filter(|c| c.kind.deck_playable_for(Faction::Solace) && c.kind != CardKind::Evolution)
        .map(|c| c.id)
        .filter(|&id| id != base_id && id != form_id)
        .take(18)
        .collect();
    deck.push(base_id);
    deck.push(form_id);
    assert_eq!(deck.len(), 20);
    // The deck is a legal Solace deck (the Deepening is deck-playable, the pair is no-orphan).
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck.clone());
    // Surgically stage: a Spite base (seat B = the Solace), banished and standing-Faded
    // in B's Main, with the form card in B's hand and Anima to pay.
    let tile = 12u8;
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Lorekeeper, Faction::Solace];
        recollect_core::test_support::put_spirit(st, tile, base_id, Seat::B);
        {
            let sp = st.board[tile as usize].spirit.as_mut().unwrap();
            sp.fading = true;
            sp.banished_by = Some(Seat::A);
            sp.fade_deadline = Some(st.round); // due at B's coming turn-end
        }
        st.active = Seat::B;
        st.active_slot = SeatSlot::B1;
        st.player_b.hand = vec![form_id];
        st.player_b.anima = 20;
        st.player_b.deck.clear();
        st.player_a.deck.clear();
    }
    // The Deepening is OFFERED on the Fading Solace base (a Primal — no donor).
    assert!(
        e.legal_commands(Seat::B).iter().any(|c| matches!(
            c,
            Command::Evolve { tile: t, form_hand: 0, fuel: None, .. } if *t == tile
        )),
        "the Solace base is offered its Primal Deepening in B's Main"
    );
    let evs = e
        .apply(
            Seat::B,
            Command::Evolve {
                tile,
                form_hand: 0,
                fuel: None,
                engage: None,
            },
        )
        .expect("the Solace Primal Deepening resolves");
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritEvolved { to, .. } if *to == form_id)),
        "the Unwritten base deepened into its Primal"
    );
    let sp = e.state().board[tile as usize].spirit.as_ref().unwrap();
    assert_eq!(sp.card, form_id, "now Spite, Made Whole");
    assert!(!sp.fading, "the deepening cleared Fading");
    assert_eq!(sp.hp, sp.hp_max, "arrived at full HP");
    // No impression: it BECAME the form (the Solace leaves nothing anyway).
    assert!(
        validate_deck_for(&deck, &cat, Faction::Solace).is_ok(),
        "the staged Solace deck is legal (Deepening deck-playable, base↔form no-orphan)"
    );
}

// ── The gentle-or-malign MENUS: a base offers a moral choice at evolution ──

#[test]
fn four_solace_bases_offer_a_gentle_or_malign_menu_of_two_primals() {
    // The Solace's signature of the Lorekeeper's Primal-OR-Fabled fork: four bases each
    // reach TWO Primals of opposing temperament (mercy vs appetite). Both are real Primal
    // forms branching from the SAME base — comfort that devours, made a player choice.
    let cat = canon_catalog();
    let by_name = |n: &str| cat.iter().find(|c| c.name == n).unwrap();
    for (base, gentle, malign) in MENU_BASES {
        let b = by_name(base);
        // The base reaches BOTH forms.
        assert!(
            b.evolves_to.iter().any(|f| f == gentle) && b.evolves_to.iter().any(|f| f == malign),
            "{base} offers both {gentle} (gentle) and {malign} (malign)"
        );
        assert_eq!(b.evolves_to.len(), 2, "{base} is exactly a 2-form menu");
        // Both forms are Primal (the Solace has no Fabled apex) and recede to this base.
        for f in [gentle, malign] {
            let form = by_name(f);
            assert_eq!(form.rarity, "Primal", "{f} is a Primal");
            assert_eq!(
                form.evolves_from.as_deref(),
                Some(base),
                "{f} recedes to {base}"
            );
        }
    }
    // Exactly 4 menu bases among the Solace Deepenings; the other Solace bases are 1:1.
    let menu_bases = cat
        .iter()
        .filter(|b| {
            matches!(b.kind, CardKind::Unwritten | CardKind::IllIntent) && b.evolves_to.len() == 2
        })
        .count();
    assert_eq!(
        menu_bases, 4,
        "four Solace bases are gentle-or-malign menus"
    );
}

#[test]
fn a_faded_menu_base_is_offered_either_deepening_in_hand() {
    // The crux of the menu: a Fading menu base with BOTH its gentle and malign forms in
    // hand is offered an Evolve for EACH — the player picks the temperament. We use The
    // Kind Erasure (→ The Kindest Erasure / The Unkindest Erasure).
    let cat = canon_catalog();
    let id = |n: &str| cat.iter().find(|c| c.name == n).unwrap().id;
    let base = id("The Kind Erasure");
    let gentle = id("The Kindest Erasure");
    let malign = id("The Unkindest Erasure");
    let deck: Vec<CardId> = (0..20).map(|_| base).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    let tile = 12u8;
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Lorekeeper, Faction::Solace];
        recollect_core::test_support::put_spirit(st, tile, base, Seat::B);
        {
            let sp = st.board[tile as usize].spirit.as_mut().unwrap();
            sp.fading = true; // a Fading base (Primal-evolvable)
            sp.banished_by = Some(Seat::A);
            sp.fade_deadline = Some(st.round);
        }
        st.active = Seat::B;
        st.active_slot = SeatSlot::B1;
        st.player_b.hand = vec![gentle, malign]; // BOTH forms held
        st.player_b.anima = 20;
        st.player_b.deck.clear();
        st.player_a.deck.clear();
    }
    let legal = e.legal_commands(Seat::B);
    // Index 0 (gentle) and index 1 (malign) each offer an Evolve onto the base.
    assert!(
        legal.iter().any(|c| matches!(
            c, Command::Evolve { tile: t, form_hand: 0, .. } if *t == tile
        )),
        "the gentle Deepening is offered"
    );
    assert!(
        legal.iter().any(|c| matches!(
            c, Command::Evolve { tile: t, form_hand: 1, .. } if *t == tile
        )),
        "the malign Deepening is offered — the player chooses the temperament"
    );
    // Pick the malign one; the base becomes it.
    e.apply(
        Seat::B,
        Command::Evolve {
            tile,
            form_hand: 1,
            fuel: None,
            engage: None,
        },
    )
    .expect("the malign Deepening resolves");
    assert_eq!(
        e.state().board[tile as usize].spirit.as_ref().unwrap().card,
        malign,
        "the base deepened into the malign form the player chose"
    );
}

#[test]
fn the_unkindest_erasure_scours_all_adjacent_enemies_on_arrival() {
    // OnPlay/AdjacentEnemiesAll/Damage{10} (the malign partner of The Kindest Erasure):
    // the same merciful hand, but it scours the living now.
    let (mut e, tile) = pve_with("The Unkindest Erasure", 12);
    let before = e.state().board[(tile - 1) as usize]
        .spirit
        .as_ref()
        .unwrap()
        .hp;
    let evs = e.fire_arrival_effects_for_test(tile, Seat::B);
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::EffectDamaged { tile: t, amount: 10 } if *t == tile - 1)),
        "The Unkindest Erasure deals 10 to the adjacent enemy ({evs:?})"
    );
    let after = e.state().board[(tile - 1) as usize]
        .spirit
        .as_ref()
        .map(|s| s.hp)
        .unwrap_or(i16::MIN);
    assert_eq!(before - after, 10);
}

#[test]
fn the_drowning_tide_pulls_one_enemy_under_for_twenty_on_arrival() {
    // OnPlay/TargetEnemySpirit/Damage{20} (the malign partner of The Surer Tide): the
    // same patient water, closing over one mouth.
    use recollect_core::state::PendingChoice;
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    let tide = 12u8;
    let enemy = 11u8;
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, tide, id_of("The Drowning Tide"), Seat::B);
        recollect_core::test_support::put_spirit(st, enemy, id_of("Kilnhorn Rhino"), Seat::A);
        let k = st.board[enemy as usize].spirit.as_mut().unwrap();
        k.hp = 70;
        k.hp_max = 70;
        st.active = Seat::B;
        st.active_slot = SeatSlot::B1;
    }
    let before = e.state().board[enemy as usize].spirit.as_ref().unwrap().hp;
    let _ = e.fire_arrival_effects_for_test(tide, Seat::B);
    let Some(PendingChoice::Target { options, .. }) = e.state().pending_choice.clone() else {
        panic!("The Drowning Tide opens an arrival-strike target choice");
    };
    let idx = options
        .iter()
        .position(|&t| t == enemy)
        .expect("the enemy is reachable") as u8;
    e.apply(Seat::B, Command::Choose { index: idx }).unwrap();
    let after = e.state().board[enemy as usize].spirit.as_ref().unwrap().hp;
    assert_eq!(before - after, 20, "the tide pulled the enemy under for 20");
}

#[test]
fn the_grudge_forgiven_releases_every_adjacent_fading_spirit() {
    // OnPlay/AdjacentEnemiesAll/Release (the gentle partner of The Grudge Entire): it set
    // the ledger down — a sorrowful letting-go of the dying.
    let (mut e, tile) = pve_with("The Grudge Forgiven", 12);
    e.state_mut_for_test().board[(tile - 1) as usize]
        .spirit
        .as_mut()
        .unwrap()
        .fading = true;
    let evs = e.fire_arrival_effects_for_test(tile, Seat::B);
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritReleased { .. })),
        "The Grudge Forgiven releases the fading neighbour ({evs:?})"
    );
    assert!(
        e.state().board[(tile - 1) as usize].spirit.is_none()
            && e.state().board[(tile - 1) as usize].impressions.is_empty(),
        "it leaves — no body, no impression (mercy)"
    );
}

#[test]
fn the_gnawing_stilled_calms_adjacent_enemies_attack_and_their_echo() {
    // Static (the gentle partner of The Gnawing Unending): adjacent enemies −10 Attack AND
    // lose Echo eligibility — the hunger, finally quiet, asks the room to rest.
    let cat = canon_catalog();
    let measure_attack = |aura: &str| -> i16 {
        let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
        let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 12, id_of(aura), Seat::B);
        recollect_core::test_support::put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        combat_stats_for_test(e.state(), &cat, 11).attack
    };
    let with = measure_attack("The Gnawing Stilled");
    let without = measure_attack("Static");
    assert_eq!(without - with, 10, "−10 Attack to the adjacent enemy");
    // Echo suppression too.
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        recollect_core::test_support::put_spirit(st, 12, id_of("The Gnawing Stilled"), Seat::B);
        recollect_core::test_support::put_spirit(st, 11, id_of("Cloudling"), Seat::A);
    }
    assert!(
        e.echo_suppressed_for_test(11),
        "The Gnawing Stilled suppresses the adjacent enemy's Echo"
    );
}

// ── Lore completeness: every Deepening has its §9.9 "Telling Completed" entry ──

#[test]
fn every_deepening_has_full_lore_and_physical_in_the_toml_source() {
    // The signature-tier consistency bar: every Solace Deepening must carry BOTH its
    // narrative `lore` and its art-direction `physical` in the card source-of-truth
    // (`data/cards.toml`). This guards the full-prose requirement for them. We scope to the Deepenings
    // (a closed, table-defined set), since prose is deliberately not universal (Kindred and
    // the procedural Solace fill carry no authored lore), so a blanket "every card" check
    // would be wrong by design. Parsed as text (no `toml` crate in the engine's graph): we
    // slice each card's `[[card]]` block by its `key = "…"` line and assert non-empty
    // `lore = """` / `physical = """` within it.
    let toml = include_str!("../../data/cards.toml");
    let cat = canon_catalog();

    for (form, _base) in DEEPENINGS {
        let key = cat
            .iter()
            .find(|c| c.name == form)
            .unwrap_or_else(|| panic!("Deepening {form:?} is in the catalog"))
            .key
            .as_str();

        // The card's block runs from its `key = "<key>"` line to the next `[[card]]`.
        let marker = format!("key = \"{key}\"");
        let from_key = toml
            .find(&marker)
            .unwrap_or_else(|| panic!("Deepening {form:?} (key {key:?}) has a [[card]] block"));
        let block = &toml[from_key..];
        let block = block
            .split_once("\n[[card]]")
            .map(|(b, _)| b)
            .unwrap_or(block);

        assert!(
            block.contains("\nlore = \"\"\"\n") && !block.contains("\nlore = \"\"\"\n\"\"\""),
            "Deepening {form:?} (key {key:?}) has no non-empty `lore` in cards.toml — \
             every §5.8 form must carry full-sentence lore"
        );
        assert!(
            block.contains("\nphysical = \"\"\"\n")
                && !block.contains("\nphysical = \"\"\"\n\"\"\""),
            "Deepening {form:?} (key {key:?}) lacks a non-empty `physical` in cards.toml — \
             a form's entry is lore + an art-direction physical, not a bare stat-line"
        );
    }
}
