//! How winnable is the Solace PvE fight, across player skill? Seat A (the player) is a Lorekeeper
//! piloted at varying difficulty; seat B is a Solace character the bot pilots. We report the win
//! rate with Wilson CIs. A healthy PvE curve: a weak player struggles, a strong one wins
//! comfortably — and nobody faces an unwinnable wall.
use recollect_bot::{Difficulty, Faction, choose_as};
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::{generate_deck, solace_character_deck};
use recollect_core::rng::Rng;
use recollect_core::state::{MatchResult, Phase};
use recollect_core::types::CardDef;
use recollect_core::{Engine, Seat};

const N: u64 = 300;
fn wilson(p: f64, n: f64) -> f64 {
    1.96 * (p * (1.0 - p) / n).sqrt()
}

/// One match: the player (A, Lorekeeper) at `player` skill vs a Solace character (B) the bot
/// pilots at `solace` skill. Returns (result, player score, Solace score).
fn play(
    seed: u64,
    player: Difficulty,
    solace: Difficulty,
    cat: &[CardDef],
) -> (MatchResult, u8, u8) {
    let da = generate_deck((seed % 6) as u8, seed, cat);
    let db = solace_character_deck((seed % 20) as usize, seed ^ 0x5EED, cat);
    let (mut e, _) = Engine::new(seed, cat.to_vec(), da, db);
    let mut rng = Rng::from_seed(seed ^ 0xA);
    let mut steps = 0;
    loop {
        if let Phase::Finished {
            result,
            score_a,
            score_b,
        } = e.state().phase
        {
            return (result, score_a, score_b);
        }
        if steps > 5000 {
            return (MatchResult::Draw, 0, 0);
        }
        let seat = e.state().active;
        let cmd = if seat == Seat::B {
            choose_as(
                &e,
                seat,
                solace,
                Faction::Solace,
                Faction::Lorekeeper,
                &mut rng,
            )
        } else {
            choose_as(
                &e,
                seat,
                player,
                Faction::Lorekeeper,
                Faction::Solace,
                &mut rng,
            )
        };
        e.apply(seat, cmd).expect("legal");
        steps += 1;
    }
}

fn main() {
    let cat = canon_catalog();
    println!("Solace PvE winnability by player skill ({N} matches each, Solace at Hard)\n");
    println!(
        "{:>10}  {:>14}  {:>10}",
        "player", "win rate ±CI", "avg score"
    );
    for player in Difficulty::ALL {
        let (mut wins, mut ps, mut hs) = (0u64, 0u64, 0u64);
        for seed in 0..N {
            let (r, a, b) = play(seed, player, Difficulty::Hard, &cat);
            if matches!(r, MatchResult::Win(Seat::A)) {
                wins += 1;
            }
            ps += a as u64;
            hs += b as u64;
        }
        let p = wins as f64 / N as f64;
        println!(
            "{:>10}  {:>7}% ± {:>3.0}  {:>4.1} vs {:>4.1}",
            player.name(),
            p * 100.0,
            wilson(p, N as f64) * 100.0,
            ps as f64 / N as f64,
            hs as f64 / N as f64
        );
    }

    // The skill ladder — a fixed Hard player against the Solace bot at each skill tier. The
    // player should win more as the Solace plays worse, confirming the bot's difficulty scales.
    println!("\nSolace skill ladder (Hard player, {N} matches each)\n");
    println!("{:>10}  {:>14}", "Solace", "player win");
    for solace in Difficulty::ALL {
        let mut wins = 0u64;
        for seed in 0..N {
            let (r, _, _) = play(seed, Difficulty::Hard, solace, &cat);
            if matches!(r, MatchResult::Win(Seat::A)) {
                wins += 1;
            }
        }
        let p = wins as f64 / N as f64;
        println!(
            "{:>10}  {:>7}% ± {:>3.0}",
            solace.name(),
            p * 100.0,
            wilson(p, N as f64) * 100.0
        );
    }
}
