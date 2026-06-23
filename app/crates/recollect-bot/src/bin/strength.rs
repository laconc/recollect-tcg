//! Does the agent beat random at all? If the strongest tier can't clear ~60%
//! against random play, the heuristic has no signal and difficulty scaling is
//! moot. This isolates "is the brain any good" from "are the tiers distinct".
use recollect_bot::{Difficulty, choose};
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::generate_deck;
use recollect_core::rng::Rng;
use recollect_core::state::{MatchResult, Phase};
use recollect_core::{Engine, Seat};

const N: u64 = 300;

fn play_vs_random(seed: u64, diff: Difficulty, agent_seat: Seat) -> MatchResult {
    let cat = canon_catalog();
    let da = generate_deck((seed % 6) as u8, seed, &cat);
    let db = generate_deck(((seed + 3) % 6) as u8, seed ^ 0x5EED, &cat);
    let (mut e, _) = Engine::new(seed, cat, da, db);
    let mut ag = Rng::from_seed(seed ^ 0xA);
    let mut rr = Rng::from_seed(seed ^ 0xB);
    let mut steps = 0;
    loop {
        if let Phase::Finished { result, .. } = e.state().phase {
            return result;
        }
        if steps > 5000 {
            return MatchResult::Draw;
        }
        let seat = e.state().active;
        let cmd = if seat == agent_seat {
            choose(&e, seat, diff, &mut ag)
        } else {
            let legal = e.legal_commands(seat);
            // random but resolve pending choices (only Choose is legal then)
            legal[(rr.next_u64() as usize) % legal.len().max(1)].clone()
        };
        e.apply(seat, cmd).expect("legal");
        steps += 1;
    }
}

fn main() {
    let cat = canon_catalog();
    let _ = &cat;
    for diff in [Difficulty::Easy, Difficulty::Expert] {
        let mut wins = 0u64;
        let mut games = 0u64;
        for seed in 0..N {
            // Play the agent on BOTH seats across seeds to cancel seat bias.
            for seat in [Seat::A, Seat::B] {
                games += 1;
                if let MatchResult::Win(w) =
                    play_vs_random(seed * 2 + (seat == Seat::B) as u64, diff, seat)
                    && w == seat
                {
                    wins += 1;
                }
            }
        }
        let p = wins as f64 / games as f64;
        let hw = 1.96 * (p * (1.0 - p) / games as f64).sqrt();
        println!(
            "{:>12} vs random: {:.0}% ± {:.0}%  ({games} games)",
            diff.name(),
            p * 100.0,
            hw * 100.0
        );
    }
}
