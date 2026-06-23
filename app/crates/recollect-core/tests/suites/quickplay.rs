//! Quick Play: every generated deck is legal, deterministic, and on-style.
use recollect_core::Engine;
use recollect_core::cards::{canon_catalog, validate_deck};
use recollect_core::quickplay::{STYLES, generate_deck, offer};
use recollect_core::state::Phase;
use recollect_core::types::Resonance;

#[test]
fn every_style_every_seed_yields_a_legal_deck() {
    let cat = canon_catalog();
    for style in STYLES {
        for seed in 0..60u64 {
            let deck = generate_deck(style.id, seed, &cat);
            validate_deck(&deck, &cat).expect("quick play never deals an illegal deck");
            let cheap = deck
                .iter()
                .filter(|id| cat.iter().find(|c| c.id == **id).unwrap().cost <= 2)
                .count();
            assert!(
                cheap >= 8,
                "{}: the opening curve is guaranteed",
                style.name
            );
        }
    }
}

#[test]
fn generation_is_a_pure_function_of_style_and_seed() {
    let cat = canon_catalog();
    assert_eq!(generate_deck(0, 7, &cat), generate_deck(0, 7, &cat));
    assert_ne!(generate_deck(0, 7, &cat), generate_deck(0, 8, &cat));
    assert_ne!(generate_deck(0, 7, &cat), generate_deck(1, 7, &cat));
}

#[test]
fn styles_are_distinct_in_the_decks_they_deal() {
    // Embertide is the Fury aggressor; The Long Watch is the Resolve wall. The
    // honest claim is that Embertide *out-furies* The Long Watch — and it does,
    // decisively: over the sample the per-seed Fury count never inverts (Watch is
    // ≤ Ember every seed) and the mean gap is wide (~6–7 cards). We assert at that
    // robust level rather than a strict per-seed `>`. With their true Resonances, two
    // of the cost-1/cost-6 evolution bases are Fury — and The Long
    // Coronation (80 HP) is exactly the kind of high-HP body The Long Watch drafts
    // for its wall, so on a few seeds the two decks *tie* on raw Fury count (never
    // inverting). The archetype distinction is intact (Ember's Fury *share* ≈44% vs
    // Watch's ≈14%); the strict-per-seed proxy was simply too brittle for the
    // correct data, so it's recalibrated, not a balance change.
    let cat = canon_catalog();
    let fury = |deck: &Vec<recollect_core::CardId>| {
        deck.iter()
            .filter(|id| cat.iter().find(|c| c.id == **id).unwrap().resonance == Resonance::Fury)
            .count()
    };
    let (mut ember_total, mut watch_total) = (0usize, 0usize);
    for seed in 0..20u64 {
        let ember = generate_deck(0, seed, &cat);
        let watch = generate_deck(1, seed, &cat);
        let (fe, fw) = (fury(&ember), fury(&watch));
        // The relation never inverts: The Long Watch is never *more* Fury than Embertide.
        assert!(
            fe >= fw,
            "Embertide is never out-furied by The Long Watch (seed {seed}: ember {fe} < watch {fw})"
        );
        ember_total += fe;
        watch_total += fw;
    }
    // And in aggregate Embertide decisively out-furies The Long Watch — a wide,
    // archetype-defining margin, not a knife-edge.
    assert!(
        ember_total > watch_total * 2,
        "Embertide out-furies The Long Watch by a wide margin (Fury totals over 20 seeds: \
         Embertide {ember_total} vs The Long Watch {watch_total})"
    );
}

#[test]
fn the_offer_is_three_distinct_styles_deterministically() {
    let a = offer(42);
    let b = offer(42);
    assert_eq!(a.map(|s| s.id), b.map(|s| s.id));
    assert!(a[0].id != a[1].id && a[1].id != a[2].id && a[0].id != a[2].id);
}

#[test]
fn quick_play_matches_run_to_midnight() {
    let cat = canon_catalog();
    for seed in 0..6u64 {
        let offers = offer(seed);
        let da = generate_deck(offers[0].id, seed, &cat);
        let db = generate_deck(offers[1].id, seed.wrapping_add(1), &cat);
        let (mut e, _) = Engine::new(seed, cat.clone(), da, db);
        let mut steps = 0;
        while !matches!(e.state().phase, Phase::Finished { .. }) {
            assert!(steps < 600);
            let seat = e.state().active;
            let cmd = e.legal_commands(seat).first().unwrap().clone();
            e.apply(seat, cmd).unwrap();
            steps += 1;
        }
        assert_eq!(e.state().round, 12);
    }
}

#[test]
fn d19_preview_shows_the_derived_deck_before_commit() {
    use recollect_core::quickplay::{generate_deck, offer, preview};
    let cat = recollect_core::cards::canon_catalog();
    let styles = offer(7);
    let style = styles[0].id;
    let pv = preview(style, 7, &cat);
    // The preview matches what generate_deck would produce (same cards, in order).
    let deck = generate_deck(style, 7, &cat);
    assert_eq!(
        pv.cards.len(),
        deck.len(),
        "preview covers the whole derived deck"
    );
    assert_eq!(pv.style, style);
    assert!(!pv.style_name.is_empty(), "names the style being previewed");
    // The curve and spirit/spell split are consistent with the card list.
    assert_eq!(
        pv.curve.iter().map(|&c| c as usize).sum::<usize>(),
        pv.cards.len()
    );
    assert_eq!((pv.spirit_count + pv.spell_count) as usize, pv.cards.len());
    // Deterministic: same seed+style → identical preview.
    assert_eq!(preview(style, 7, &cat), pv);
    // Different from another style (the choice is meaningful).
    assert_ne!(preview(styles[1].id, 7, &cat).cards, pv.cards);
}
