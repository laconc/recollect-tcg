//! Difficulty calibration. Plays each tier head-to-head against the others and
//! reports win rates with 95% Wilson intervals, so we can SEE whether the
//! tiers are actually distinct (a harder tier should beat an easier one well
//! above 50%). Honest numbers over flattering ones — if two tiers are
//! statistically tied, that's a finding to report, not hide.
//!
//! Run: `cargo run -p recollect-bot --bin calibrate --release`
use recollect_bot::{Difficulty, choose};
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::generate_deck;
use recollect_core::rng::Rng;
use recollect_core::state::{MatchResult, Phase};
use recollect_core::{Engine, Seat};

const N: u64 = 200; // matches per ordered pairing

fn wilson(p: f64, n: f64) -> f64 {
    1.96 * (p * (1.0 - p) / n).sqrt()
}

/// Play one match: seat A uses difficulty `a`, seat B uses `b`. Decks are
/// generated from the match seed so the pairing isn't deck-biased. Returns the
/// result from A's perspective.
fn play(seed: u64, a: Difficulty, b: Difficulty) -> MatchResult {
    let cat = canon_catalog();
    let da = generate_deck((seed % 6) as u8, seed, &cat);
    let db = generate_deck(((seed + 3) % 6) as u8, seed ^ 0x5EED, &cat);
    let (mut e, _) = Engine::new(seed, cat, da, db);
    let mut ra = Rng::from_seed(seed ^ 0xA);
    let mut rb = Rng::from_seed(seed ^ 0xB);
    let mut steps = 0;
    loop {
        if let Phase::Finished { result, .. } = e.state().phase {
            return result;
        }
        if steps > 5000 {
            return MatchResult::Draw;
        }
        let seat = e.state().active;
        let (diff, rng) = if seat == Seat::A {
            (a, &mut ra)
        } else {
            (b, &mut rb)
        };
        let cmd = choose(&e, seat, diff, rng);
        e.apply(seat, cmd).expect("legal");
        steps += 1;
    }
}

/// A's win rate over N matches against B (averaging out seat advantage by
/// playing each seed once; seat fairness is a separate fleet concern).
fn winrate(a: Difficulty, b: Difficulty) -> (f64, f64) {
    let mut wins = 0u64;
    for seed in 0..N {
        match play(seed, a, b) {
            MatchResult::Win(Seat::A) => wins += 1,
            MatchResult::Win(Seat::B) => {}
            MatchResult::Draw => {}
        }
    }
    let p = wins as f64 / N as f64;
    (p, wilson(p, N as f64))
}

fn main() {
    let tiers = [
        Difficulty::Easy,
        Difficulty::Normal,
        Difficulty::Hard,
        Difficulty::Expert,
    ];
    println!("Difficulty calibration — {N} matches per pairing, A's win rate ± 95% CI\n");
    println!("Each row = that tier as seat A vs the column tier as seat B.");
    print!("{:>12}", "");
    for b in tiers {
        print!("{:>16}", b.name());
    }
    println!();
    for a in tiers {
        print!("{:>12}", a.name());
        for b in tiers {
            if a == b {
                print!("{:>16}", "—");
                continue;
            }
            let (p, hw) = winrate(a, b);
            print!(
                "{:>11}±{:>3}",
                format!("{:.0}%", p * 100.0),
                format!("{:.0}", hw * 100.0)
            );
        }
        println!();
    }
    println!("\nExpectation: a stronger tier beats a weaker one well above 50%.");
    println!("Adjacent tiers tied within CI = the ladder needs more separation.");
}
