//! The canon catalog: 407 designed cards (the Solace set includes 36 creatures),
//! loaded from data generated off the card source
//! `data/cards.toml` (tools/gen_catalog.py). Source/code drift fails here.
use recollect_core::Engine;
use recollect_core::cards::{canon_catalog, validate_deck};
use recollect_core::quickplay::{STYLES, generate_deck};
use recollect_core::state::Phase;
use recollect_core::types::{CardKind, Resonance};

#[test]
fn the_full_telling_card_count_and_architecture_math_holds() {
    let cat = canon_catalog();
    assert_eq!(
        cat.len(),
        419,
        "was 407; the Solace Deepenings added 12 Primal forms (8 seed + 4 menu partners — the Solace deepens, never ascends)"
    );
    let count = |k: CardKind| cat.iter().filter(|c| c.kind == k).count();
    assert_eq!(
        count(CardKind::Spirit) + count(CardKind::Caller),
        114,
        "base spirits incl. the curve-fill bases + lurkers"
    );
    assert_eq!(
        count(CardKind::Evolution),
        60,
        "48 Lorekeeper forms + 12 Solace Primal Deepenings (4 bases offer a gentle-or-malign menu)"
    );
    assert_eq!(
        count(CardKind::Unwritten) + count(CardKind::IllIntent) + count(CardKind::Unwriting),
        92,
        "Solace set: 41 Unwritten + 39 ill intent + 12 Unwriting events"
    );
    assert_eq!(count(CardKind::Foundling), 27);
    assert_eq!(count(CardKind::Kindred), 6);
    let rblf = count(CardKind::Ritual)
        + count(CardKind::Bond)
        + count(CardKind::Landmark)
        + count(CardKind::Fabrication);
    assert_eq!(rblf, 120, "the spellbook");
    // collectible = total − evolutions − Solace(92) − Kindred(6). The 12 Solace
    // Deepenings raised total (419) and evolutions (60) together, so the
    // collectible count is unchanged: forms are never collectible.
    assert_eq!(419 - 60 - 92 - 6, 261);
    // ids dense and unique
    for (i, c) in cat.iter().enumerate() {
        assert_eq!(c.id.0 as usize, i);
    }
}

#[test]
fn common_and_uncommon_spirits_land_on_the_budget_curve() {
    // The stat budget is a LINEAR curve: A+D+H ≈ K·cost + B. Rather than pin K/B (the
    // combat-weight re-stat re-scales them — tools/restat.py + the derived fit in gen_catalog.py),
    // we FIT the curve from the priced spirits themselves (least squares over every C/U
    // resonance spirit) and assert each lands within a tolerance band of it. This is
    // self-calibrating — it rides any future re-stat — while still catching a genuinely
    // fat-fingered, off-curve card (the drift this test exists to guard).
    let cat = canon_catalog();
    let priced: Vec<(f64, f64)> = cat
        .iter()
        .filter(|c| {
            c.kind == CardKind::Spirit
                && (c.rarity == "C" || c.rarity == "U")
                && c.resonance != Resonance::Neutral
        })
        .map(|c| (c.cost as f64, (c.attack + c.defense + c.hp) as f64))
        .collect();
    assert!(priced.len() >= 30, "too few priced spirits to fit a curve");
    let n = priced.len() as f64;
    let (sx, sy) = priced
        .iter()
        .fold((0.0, 0.0), |(ax, ay), (x, y)| (ax + x, ay + y));
    let sxx: f64 = priced.iter().map(|(x, _)| x * x).sum();
    let sxy: f64 = priced.iter().map(|(x, y)| x * y).sum();
    let k = (n * sxy - sx * sy) / (n * sxx - sx * sx);
    let b = (sy - k * sx) / n;
    // Residual spread: the band is the larger of ±50 (the authored tax range, widened for the
    // re-stat's per-stat rounding) or 3σ of the residuals — generous enough to admit deliberate
    // tilts, tight enough that a doubled/halved stat (residual in the hundreds) still trips.
    let resid = |c: &recollect_core::types::CardDef| {
        (c.attack + c.defense + c.hp) as f64 - (k * c.cost as f64 + b)
    };
    let var: f64 = priced
        .iter()
        .map(|(x, y)| {
            let r = y - (k * x + b);
            r * r
        })
        .sum::<f64>()
        / n;
    let band = (3.0 * var.sqrt()).max(50.0);
    for c in &cat {
        if c.kind == CardKind::Spirit
            && (c.rarity == "C" || c.rarity == "U")
            && c.resonance != Resonance::Neutral
        {
            let r = resid(c);
            assert!(
                r.abs() <= band,
                "{} off the fitted budget curve (sum {}, cost {}): residual {r:.0} exceeds ±{band:.0}",
                c.name,
                c.attack + c.defense + c.hp,
                c.cost
            );
        }
    }
}

#[test]
fn quick_play_deals_only_playable_kinds_from_the_canon() {
    let cat = canon_catalog();
    for style in STYLES {
        for seed in 0..20u64 {
            let deck = generate_deck(style.id, seed, &cat);
            validate_deck(&deck, &cat).unwrap();
            for id in &deck {
                assert!(cat[id.0 as usize].kind.deck_playable());
            }
        }
    }
}

#[test]
fn canon_matches_run_to_midnight() {
    let cat = canon_catalog();
    for seed in 0..4u64 {
        let da = generate_deck(0, seed, &cat);
        let db = generate_deck(1, seed + 9, &cat);
        let (mut e, _) = Engine::new(seed, cat.clone(), da, db);
        let mut steps = 0;
        while !matches!(e.state().phase, Phase::Finished { .. }) {
            assert!(steps < 800);
            let seat = e.state().active;
            let cmd = e.legal_commands(seat).first().unwrap().clone();
            e.apply(seat, cmd).unwrap();
            steps += 1;
        }
        assert_eq!(e.state().round, 12);
    }
}

#[test]
fn the_solace_set_splits_into_unwritten_and_ill_intent() {
    use recollect_core::types::CardKind;
    let cat = canon_catalog();
    let creatures: Vec<_> = cat
        .iter()
        .filter(|c| c.kind.is_antagonist_creature())
        .collect();
    let ill_intent = creatures.iter().filter(|c| c.is_ill_intent()).count();
    let plain = creatures.iter().filter(|c| c.is_plain_unwritten()).count();
    // Every antagonist creature is exactly one of the two.
    assert_eq!(
        ill_intent + plain,
        creatures.len(),
        "every Unwritten is plain or ill intent"
    );
    // Both subsets are non-empty (a gentle Solace deck AND a cruel one are both buildable).
    assert!(
        ill_intent >= 5,
        "need a real ill-intent subset for cruel Solace decks: {ill_intent}"
    );
    assert!(
        plain >= 15,
        "need a real plain-Unwritten pool for gentle decks: {plain}"
    );
    // The menace flag only ever sits on antagonist creatures.
    for c in &cat {
        if c.is_ill_intent() {
            assert_eq!(
                c.kind,
                CardKind::IllIntent,
                "{} ill_intent but wrong kind",
                c.name
            );
        }
    }
}

#[test]
fn no_evolution_chains_in_the_canon_a_form_never_evolves_from_a_form() {
    // The no-chain Lorekeeper lock (design §5): "a base evolves to a Primal OR a
    // Fabled — a Primal cannot evolve to a Fabled." Both forms branch from the
    // *base* (base→Primal and base→Fabled, never base→Primal→Fabled). The only path
    // from a Primal to a Fabled is to recede it to a base first (Devolution) and
    // evolve that base anew. We assert the structural invariant over the WHOLE canon
    // catalog: no form's base is itself a form. (The engine enforces it too —
    // `legal_evolutions` returns nothing for a base with `evolves_from` set, so
    // `decide_evolve` rejects a form-onto-form; this guards the DATA the engine reads.)
    let cat = canon_catalog();
    let by_name: std::collections::HashMap<&str, &recollect_core::types::CardDef> =
        cat.iter().map(|c| (c.name.as_str(), c)).collect();
    for c in &cat {
        if let Some(base_name) = c.evolves_from.as_deref() {
            // The thing it evolves FROM must exist and must NOT itself be a form.
            let base = by_name
                .get(base_name)
                .unwrap_or_else(|| panic!("{} evolves_from unknown card {base_name:?}", c.name));
            assert_ne!(
                base.kind,
                CardKind::Evolution,
                "CHAIN: {} ({}) evolves from {} which is itself an Evolution form — \
                 the no-chain lock forbids base→Primal→Fabled; forms must branch from a true base",
                c.name,
                c.rarity,
                base.name
            );
            assert_eq!(
                base.evolves_from, None,
                "CHAIN: {}'s base {} is itself a form (has evolves_from)",
                c.name, base.name
            );
        }
        // The dual: every name in a base's `evolves_to` is a real Evolution form.
        for form_name in &c.evolves_to {
            let form = by_name
                .get(form_name.as_str())
                .unwrap_or_else(|| panic!("{} evolves_to unknown card {form_name:?}", c.name));
            assert_eq!(
                form.kind,
                CardKind::Evolution,
                "{}'s evolves_to entry {} is not an Evolution form",
                c.name,
                form.name
            );
        }
    }
}

#[test]
fn spirit_resonances_are_balanced_not_defaulted() {
    // Regression: gen_catalog once defaulted resonance to Wonder when the section
    // header was UPPERCASE ("Index — WONDER"), mislabeling 101/108 spirits as
    // Wonder. The six resonances should each carry a real share of the roster.
    use recollect_core::types::{CardKind, Resonance};
    let cat = canon_catalog();
    let count = |r: Resonance| {
        cat.iter()
            .filter(|c| matches!(c.kind, CardKind::Spirit | CardKind::Caller) && c.resonance == r)
            .count()
    };
    for r in [
        Resonance::Wonder,
        Resonance::Fear,
        Resonance::Sorrow,
        Resonance::Harmony,
        Resonance::Fury,
        Resonance::Resolve,
    ] {
        let n = count(r);
        assert!(
            n >= 10,
            "{r:?} has only {n} spirits — resonance parsing likely regressed (default-to-Wonder bug)"
        );
    }
    // And no single resonance hogs the roster (the bug symptom was ~101 Wonder).
    assert!(
        count(Resonance::Wonder) <= 40,
        "Wonder hogging the roster — parser regressed"
    );
}
