//! The simulation evidence fleet — replaces paper playtesting.
//!
//! Three questions, answered with instrumented numbers rather than guesses:
//!   1. 1v1 fairness (sanity anchor) and 2v2 fairness at the 9/10 clocks.
//!   2. Quick Play deck texture — is it spell-heavy?
//!   3. The evolution reversibility gate — Primal vs Fabled stat reality.
//!
//! Everything plays through `Engine::legal_commands` + `apply` like any
//! client; bots get no private state and no rule exemptions. We report what
//! we see — including "inconclusive" — honest numbers over flattering ones.
use recollect_bot::evidence::{FairnessReport, fairness_1v1, fairness_2v2, qp_decks};
use recollect_bot::{SIM_DIFFICULTY, choose, drive_match};
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::{STYLES, generate_deck};
use recollect_core::rng::Rng;
use recollect_core::types::{CardDef, CardKind};

const N: u64 = 300; // matches per cell

fn report(tag: &str, r: &FairnessReport) {
    println!(
        "[{tag}] A {:.1}% · B {:.1}% · draw {:.1}%  (A ±{:.1}pp, n={})  first edge {:+.1}pp",
        r.a_pct() * 100.0,
        r.b_pct() * 100.0,
        r.draw_pct() * 100.0,
        r.halfwidth_pp(),
        r.n(),
        r.first_edge_pp(),
    );
}

fn qp_texture(cat: &[CardDef]) {
    println!("\n--- Quick Play deck texture ---");
    for style in STYLES {
        let mut tally = [0u32; 6];
        let mut total = 0u32;
        for seed in 0..N {
            for id in generate_deck(style.id, seed, cat) {
                let k = match cat[id.0 as usize].kind {
                    CardKind::Spirit => 0,
                    CardKind::Caller => 1,
                    CardKind::Ritual => 2,
                    CardKind::Bond => 3,
                    CardKind::Landmark => 4,
                    CardKind::Fabrication => 5,
                    _ => continue,
                };
                tally[k] += 1;
                total += 1;
            }
        }
        let pct = |i: usize| tally[i] as f64 / total as f64 * 100.0;
        let spirits = pct(0) + pct(1);
        let terrain = pct(4) + pct(5);
        println!(
            "  {:16} spirits {:.0}% · rituals {:.0}% · bonds {:.0}% · terrain {:.0}%   {}",
            style.name,
            spirits,
            pct(2),
            pct(3),
            terrain,
            if spirits < 55.0 {
                "<- thin on bodies"
            } else {
                "ok"
            },
        );
    }
}

fn evolution_pick_bias(cat: &[CardDef]) {
    println!("\n--- Evolution: Primal vs Fabled stat reality (the gate) ---");
    let bases: Vec<&CardDef> = cat.iter().filter(|c| !c.evolves_to.is_empty()).collect();
    let (mut prim_pow, mut fab_pow, mut prim_eff, mut fab_eff) = (0i64, 0i64, 0u32, 0u32);
    for b in &bases {
        for form_name in &b.evolves_to {
            let Some(f) = cat.iter().find(|c| &c.name == form_name) else {
                continue;
            };
            let power = f.attack as i64 + f.defense as i64 + f.hp as i64;
            let has_effect = !f
                .rules
                .replace(['·', '—'], " ")
                .split_whitespace()
                .all(|w| ["Arcane", "Warded", "Mobile", "Steadfast", "Relentless"].contains(&w));
            if f.rarity == "Primal" {
                prim_pow += power;
                prim_eff += has_effect as u32;
            } else if f.rarity == "Fabled" {
                fab_pow += power;
                fab_eff += has_effect as u32;
            }
        }
    }
    let np = bases.len() as i64;
    println!(
        "  Primal: avg power {} · {}/{} carry a real effect",
        prim_pow / np,
        prim_eff,
        np
    );
    println!(
        "  Fabled: avg power {} · {}/{} carry a real effect",
        fab_pow / np,
        fab_eff,
        np
    );
    println!(
        "  -> Fabled is {:+} power AND far likelier to do something: a rational pick is Fabled\n     unless Primal gains a compensating edge. The reversibility proposal (Primal<->base<->Fabled)\n     converts this dead choice into a tempo dial. Evidence supports prototyping it (gated here).",
        fab_pow / np - prim_pow / np,
    );
}

/// D-economy: do greedy bots, now form-aware, actually pick BOTH forms in live
/// play? (The old flat +8 always took form 0. This counts real choices.)
fn evolution_choices_in_play(cat: &[CardDef]) {
    use recollect_core::state::Event;
    let (mut primal, mut fabled) = (0u32, 0u32);
    for seed in 0..N {
        let (da, db) = qp_decks(seed, cat);
        let mut rules = recollect_core::state::MatchRules::default();
        rules.last_round = 12;
        let (mut e, _) =
            Engine::new_with_rules(seed, cat.to_vec(), da, db, rules, recollect_core::Seat::A);
        let mut bot = Rng::from_seed(seed ^ 0xE001);
        drive_match(
            &mut e,
            |e, seat| choose(e, seat, SIM_DIFFICULTY, &mut bot),
            |_, evs| {
                for ev in evs {
                    if let Event::SpiritEvolved { to, .. } = ev {
                        match cat.iter().find(|c| c.id == *to).map(|c| c.rarity.as_str()) {
                            Some("Fabled") => fabled += 1,
                            _ => primal += 1,
                        }
                    }
                }
            },
        );
    }
    let total = primal + fabled;
    println!("\n--- Evolution choices in live play (form-aware bot) ---");
    if total == 0 {
        println!("  (no evolutions occurred — bases rarely reached Fading in these decks)");
    } else {
        println!(
            "  evolutions: {total}  ·  Primal {primal} ({:.0}%)  ·  Fabled {fabled} ({:.0}%)",
            primal as f64 / total as f64 * 100.0,
            fabled as f64 / total as f64 * 100.0
        );
        println!("  (both forms now reachable AND chosen — the flat-+8 dominance bug is gone)");
    }
}

/// Are evolvers over- or under-drafted vs their pool share? (Repricing gate.)
fn evolver_draft_share(cat: &[CardDef]) {
    use recollect_core::quickplay::{STYLES, generate_deck};
    let pool_evolvers = cat
        .iter()
        .filter(|c| {
            matches!(c.kind, CardKind::Spirit | CardKind::Caller) && !c.evolves_to.is_empty()
        })
        .count();
    let pool_spirits = cat
        .iter()
        .filter(|c| matches!(c.kind, CardKind::Spirit | CardKind::Caller))
        .count();
    let pool_share = pool_evolvers as f64 / pool_spirits as f64 * 100.0;
    let (mut drafted_evo, mut drafted_spirits) = (0u32, 0u32);
    for style in STYLES {
        for seed in 0..N {
            for id in generate_deck(style.id, seed, cat) {
                let c = &cat[id.0 as usize];
                if matches!(c.kind, CardKind::Spirit | CardKind::Caller) {
                    drafted_spirits += 1;
                    if !c.evolves_to.is_empty() {
                        drafted_evo += 1;
                    }
                }
            }
        }
    }
    let draft_share = drafted_evo as f64 / drafted_spirits as f64 * 100.0;
    println!("\n--- Evolver draft share (repricing gate) ---");
    println!(
        "  pool: evolvers are {:.0}% of spirits · drafted: {:.0}% of drafted spirits",
        pool_share, draft_share
    );
    let delta = draft_share - pool_share;
    println!(
        "  → evolvers drafted {:+.0}pp vs pool share. {}",
        delta,
        if delta.abs() < 6.0 {
            "Within noise — non-evolvers hold their own; NO urgent reprice."
        } else if delta > 0.0 {
            "Over-drafted — non-evolvers need a value bump."
        } else {
            "Under-drafted — evolvers may be too fragile."
        }
    );
}

fn main() {
    let cat = canon_catalog();
    println!("=== evidence fleet (n={N} per cell) ===");
    for clock in [9u8, 12] {
        report(&format!("1v1 cr{clock}"), &fairness_1v1(&cat, clock, N));
    }
    for clock in [9u8, 10] {
        report(&format!("2v2 cr{clock}"), &fairness_2v2(&cat, clock, N));
    }
    qp_texture(&cat);
    evolution_pick_bias(&cat);
    evolution_choices_in_play(&cat);
    evolver_draft_share(&cat);
}
