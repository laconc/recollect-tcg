//! Analysis (red-team): are the deck styles/dispositions REAL distinct themes, or cosmetic?
//! Prints the composition each style derives. Run: `cargo test --test deck_themes -- --nocapture`.
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::{STYLES, generate_deck, generate_deck_for};
use recollect_core::types::{CardDef, CardId, CardKind, Faction};
use std::collections::HashMap;

fn find(cat: &[CardDef], id: CardId) -> &CardDef {
    cat.iter().find(|c| c.id == id).unwrap()
}

#[test]
fn report_lorekeeper_style_themes() {
    let cat = canon_catalog();
    println!("\n=== Lorekeeper styles — avg per 20-card deck over 20 seeds ===");
    for s in 0..STYLES.len() as u8 {
        let mut res: HashMap<String, f64> = HashMap::new();
        let (mut cost, mut mobile, mut warded, mut n) = (0.0, 0.0, 0.0, 0.0);
        for seed in 0..20u64 {
            for id in generate_deck(s, seed, &cat) {
                let c = find(&cat, id);
                *res.entry(format!("{:?}", c.resonance)).or_default() += 1.0 / 20.0;
                cost += c.cost as f64;
                mobile += c.mobile as u8 as f64;
                warded += c.warded as u8 as f64;
                n += 1.0;
            }
        }
        let mut v: Vec<_> = res.into_iter().collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top: Vec<String> = v
            .iter()
            .take(3)
            .map(|(k, c)| format!("{k} {c:.1}"))
            .collect();
        println!(
            "{:<16} avg-cost {:.1}  mobile {:.1}  warded {:.1}  | resonance: {}",
            STYLES[s as usize].name,
            cost / n,
            mobile / 20.0,
            warded / 20.0,
            top.join(", ")
        );
    }
}

#[test]
fn report_solace_disposition_themes() {
    let cat = canon_catalog();
    let labels = [
        "Cruelty",
        "Erasure",
        "Relentless",
        "LongForgetting",
        "Sorrow",
        "(balanced)",
    ];
    println!("\n=== Solace dispositions — avg per 20-card deck over 20 seeds ===");
    for (di, label) in (0u8..=5).zip(labels) {
        let style = if di == 5 { 99 } else { di };
        let (mut ill, mut unw, mut ev, mut atk, mut rel, mut sf) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        for seed in 0..20u64 {
            for id in generate_deck_for(Faction::Solace, style, seed, &cat) {
                let c = find(&cat, id);
                match c.kind {
                    CardKind::IllIntent => ill += 1.0,
                    CardKind::Unwritten => unw += 1.0,
                    CardKind::Unwriting => ev += 1.0,
                    _ => {}
                }
                atk += c.attack as f64;
                rel += c.relentless as u8 as f64;
                sf += c.steadfast as u8 as f64;
            }
        }
        println!(
            "{:<16} ill {:>4.1}  unwritten {:>4.1}  events {:>3.1}  | avg-atk {:>3.0}  relentless {:.1}  steadfast {:.1}",
            label,
            ill / 20.0,
            unw / 20.0,
            ev / 20.0,
            atk / (20.0 * 20.0),
            rel / 20.0,
            sf / 20.0
        );
    }
}

#[test]
fn report_deck_overlap() {
    let cat = canon_catalog();
    let shared = |a: &[CardId], b: &[CardId]| a.iter().filter(|x| b.contains(x)).count();
    println!("\n=== Shared cards of 20 (same seed, different style) ===");
    // Same style, different seed = the natural singleton churn (baseline).
    let base = shared(&generate_deck(0, 1, &cat), &generate_deck(0, 2, &cat));
    println!("Embertide seed1 vs Embertide seed2 (same theme):   {base}/20 shared");
    for (a, b) in [(0u8, 1u8), (0, 2), (0, 3), (1, 2)] {
        let s = shared(&generate_deck(a, 7, &cat), &generate_deck(b, 7, &cat));
        println!(
            "{:<12} vs {:<14} (diff theme): {s}/20 shared",
            STYLES[a as usize].name, STYLES[b as usize].name
        );
    }
    println!("--- Solace ---");
    for (a, b) in [(0u8, 1u8), (0, 4), (2, 3)] {
        let s = shared(
            &generate_deck_for(Faction::Solace, a, 7, &cat),
            &generate_deck_for(Faction::Solace, b, 7, &cat),
        );
        println!("disposition {a} vs {b}: {s}/20 shared");
    }
}
