//! Rebalance instrumentation — the combat-weight + held-ground tuning probe. Runs the mirror
//! Quick-Play 1v1 at two difficulties: Normal (depth-1, no lookahead) vs Expert (depth-2,
//! `move − opp_best_reply`). The contrast is the churn signal — if the lookahead over-penalizes
//! board presence, Normal holds a board + scores while Expert churns play↔reclaim.
//!
//! Reports, per difficulty:
//!   - first / second / draw share (seat fairness — A moves first)
//!   - mean total Score
//!   - peak AND final inner occupancy (presence at Nightfall is the win condition)
//!   - plays / reclaims per match (the churn)
//!   - banishments per match (combat decisiveness — should be >0, not ~0)
//!   - mean hits-to-banish (damage-dealing strikes ÷ banishments — target ~3)
//!
//! Sweep the catalog re-stat (`RESTAT_KA/KD/KH` + `make catalog`) and the bot held-ground weights
//! against this until combat bites (~3 hits, banishments up), the inner board is populated at
//! Nightfall, draws stay low, and first/second trend toward 50/50.
//!   cargo run -p recollect-bot --bin rebalance
use recollect_bot::evidence::qp_decks;
use recollect_bot::{Difficulty, choose, drive_match};
use recollect_core::cards::canon_catalog;
use recollect_core::rng::Rng;
use recollect_core::state::{Event, MatchResult, Phase};
use recollect_core::types::{CardDef, CardId, is_rim};
use recollect_core::{Command, Engine, Seat};

fn inner_spirits(e: &Engine) -> u32 {
    let st = e.state();
    (0..st.board.len() as u8)
        .filter(|&t| !is_rim(t) && st.board[t as usize].spirit.is_some())
        .count() as u32
}

#[derive(Default)]
struct Tally {
    first: u32,
    second: u32,
    draws: u32,
    score_sum: u64,
    peak_inner_sum: u64,
    final_inner_sum: u64,
    plays: u64,
    reclaims: u64,
    // A combat banish is a `SpiritBecameFading` with a banisher (uncontested
    // contraction fades carry `banished_by: None` and are excluded).
    banishes: u64,
    // Damage-dealing strikes (the "hits") — `Struck`/`EffectDamaged` with damage > 0.
    hits: u64,
}

fn measure(
    label: &str,
    n: u64,
    cat: &[CardDef],
    diff: Difficulty,
    decks: impl Fn(u64, &[CardDef]) -> (Vec<CardId>, Vec<CardId>),
) {
    let mut tl = Tally::default();
    for seed in 0..n {
        let (da, db) = decks(seed, cat);
        let (mut e, _) = Engine::new(seed, cat.to_vec(), da, db);
        let mut rng = Rng::from_seed(seed ^ 0xF1EE7);
        let mut pk = 0u32;
        drive_match(
            &mut e,
            |e, seat| {
                let c = choose(e, seat, diff, &mut rng);
                if matches!(c, Command::PlaySpirit { .. }) {
                    tl.plays += 1;
                }
                c
            },
            |e, evs| {
                for ev in evs {
                    match ev {
                        Event::SpiritReclaimed { .. } => tl.reclaims += 1,
                        Event::SpiritBecameFading {
                            banished_by: Some(_),
                            ..
                        } => tl.banishes += 1,
                        Event::Struck { damage, .. }
                        | Event::EffectDamaged { amount: damage, .. }
                            if *damage > 0 =>
                        {
                            tl.hits += 1
                        }
                        _ => {}
                    }
                }
                pk = pk.max(inner_spirits(e));
            },
        );
        tl.peak_inner_sum += pk as u64;
        tl.final_inner_sum += inner_spirits(&e) as u64;
        if let Phase::Finished {
            result,
            score_a,
            score_b,
        } = e.state().phase
        {
            tl.score_sum += (score_a + score_b) as u64;
            match result {
                MatchResult::Win(Seat::A) => tl.first += 1,
                MatchResult::Win(Seat::B) => tl.second += 1,
                MatchResult::Draw => tl.draws += 1,
            }
        }
    }
    let nf = n as f64;
    let hits_to_banish = if tl.banishes > 0 {
        tl.hits as f64 / tl.banishes as f64
    } else {
        f64::NAN
    };
    println!(
        "{label}: first {:.0}% / second {:.0}% / draw {:.0}%   score {:.2}   inner peak {:.2} / final {:.2}   plays {:.1} / reclaims {:.1}   banishes {:.2}/match   hits-to-banish {:.2}",
        100.0 * tl.first as f64 / nf,
        100.0 * tl.second as f64 / nf,
        100.0 * tl.draws as f64 / nf,
        tl.score_sum as f64 / nf,
        tl.peak_inner_sum as f64 / nf,
        tl.final_inner_sum as f64 / nf,
        tl.plays as f64 / nf,
        tl.reclaims as f64 / nf,
        tl.banishes as f64 / nf,
        hits_to_banish,
    );
}

fn main() {
    let cat = canon_catalog();
    let n = 60u64;
    println!("=== Mirror QP 1v1, A first; lookahead OFF (Normal) vs ON (Expert) ===");
    measure(
        "Normal (depth-1, no lookahead)",
        n,
        &cat,
        Difficulty::Normal,
        qp_decks,
    );
    measure(
        "Expert (depth-2, lookahead)   ",
        n,
        &cat,
        Difficulty::Expert,
        qp_decks,
    );
}
