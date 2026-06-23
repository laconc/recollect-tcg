//! Nightly monitoring probes under THE LAW (no experiment flags).
//! Tripwires: camper viability creeping up; rim-harvest share past ~30%.
use recollect_bot::{SIM_DIFFICULTY, choose, drive_match};
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::generate_deck;
use recollect_core::rng::Rng;
use recollect_core::state::Event;
use recollect_core::types::is_rim;
use recollect_core::{Command, Engine, MatchResult, Seat};

fn camper_cmd(e: &Engine, seat: Seat) -> Command {
    let legal = e.legal_commands(seat);
    if let Some(c) = legal
        .iter()
        .find(|c| matches!(c, Command::PlaySpirit { tile, engage: None, .. } if is_rim(*tile)))
    {
        return c.clone();
    }
    if let Some(c) = legal.iter().find(|c| matches!(c, Command::Glimpse)) {
        return c.clone();
    }
    legal.last().unwrap().clone()
}

fn decks(seed: u64) -> (Vec<recollect_core::CardId>, Vec<recollect_core::CardId>) {
    let cat = recollect_core::cards::canon_catalog();
    (
        generate_deck((seed % 5) as u8, seed, &cat),
        generate_deck(((seed + 2) % 5) as u8, seed + 1, &cat),
    )
}

fn main() {
    // 1. Fairness + texture under the law (greedy).
    let (mut a, mut b, mut d) = (0u32, 0u32, 0u32);
    let (mut rim_late, mut inner_late) = (0u64, 0u64);
    let n = 1500u64;
    for seed in 0..n {
        let (da, db) = decks(seed);
        let (mut e, _) = Engine::new(seed, canon_catalog(), da, db);
        let mut rng = Rng::from_seed(seed ^ 0x96EED);
        let result = drive_match(
            &mut e,
            |e, seat| choose(e, seat, SIM_DIFFICULTY, &mut rng),
            |e, evs| {
                for ev in evs {
                    if let Event::SpiritBecameFading { tile, .. } = ev
                        && e.state().round > e.state().rules.contraction_after
                    {
                        if is_rim(*tile) {
                            rim_late += 1
                        } else {
                            inner_late += 1
                        }
                    }
                }
            },
        );
        match result {
            MatchResult::Win(Seat::A) => a += 1,
            MatchResult::Win(Seat::B) => b += 1,
            MatchResult::Draw => d += 1,
        }
    }
    println!(
        "LAW fairness (greedy): A {:.3} / B {:.3} / draw {:.3}",
        a as f64 / n as f64,
        b as f64 / n as f64,
        d as f64 / n as f64
    );
    println!(
        "LAW endgame texture: rim harvest {:.1}% · inner climax {:.1}%  (tripwire: 30%)",
        100.0 * rim_late as f64 / (rim_late + inner_late) as f64,
        100.0 * inner_late as f64 / (rim_late + inner_late) as f64
    );

    // 2. Camper tripwire under the law.
    for (label, punisher_bot) in [("vs random", false), ("vs bot", true)] {
        let m = 1000u64;
        let mut bw = 0u32;
        for seed in 0..m {
            let (da, db) = decks(seed);
            let (mut e, _) = Engine::new(seed, canon_catalog(), da, db);
            let mut rng = Rng::from_seed(seed ^ 0x6CA);
            let result = drive_match(
                &mut e,
                |e, seat| {
                    if seat == Seat::B {
                        camper_cmd(e, seat)
                    } else if punisher_bot {
                        choose(e, seat, SIM_DIFFICULTY, &mut rng)
                    } else {
                        let legal = e.legal_commands(seat);
                        legal[(rng.next_u64() % legal.len() as u64) as usize].clone()
                    }
                },
                |_, _| {},
            );
            if matches!(result, MatchResult::Win(Seat::B)) {
                bw += 1;
            }
        }
        println!(
            "LAW camper {label}: {:.3}  (tripwire: approaching the playing baseline)",
            bw as f64 / m as f64
        );
    }
}
