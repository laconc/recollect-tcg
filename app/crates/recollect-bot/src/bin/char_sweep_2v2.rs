//! 2v2 PvE win-rate probe — the 2v2 parity of `bin/char_sweep`.
//!
//! `char_sweep` measures the 1v1 PvE fight (player Lorekeeper vs Solace) per character; it does not
//! cover the 6×6 2v2 telling. This binary measures the **2v2 PvE win rate** so the maintainer can
//! see whether the "too easy" 1v1 picture holds in 2v2, where two Lorekeepers (slots A1+A2) contend
//! against two Solace (slots B1+B2) on the wider board with the longer (10-round) clock.
//!
//! It is a DATA probe only — no engine/card/balance change. It plays through the same public seam
//! every client uses (`Engine::new_2v2_with_opener` + `legal_commands`/`apply`), pilots each team in
//! its own faction via `choose_as`, and tells the engine the per-team factions so the 2v2 economics
//! (shared per-team projection/score) resolve faction-correctly — exactly the PvE
//! contest a real 2v2 game runs.
//!
//! Two rosters, mirroring `char_sweep`:
//!
//!   1. **Solace roster** — each Solace disposition fields BOTH B-slots (B1, B2) against a rotating
//!      Lorekeeper team (A1, A2 drawn from the Quick-Play styles). The reported number is the
//!      **player (team A / Lorekeeper) win rate**: HIGH ⇒ this disposition is a pushover in 2v2;
//!      LOW ⇒ a wall.
//!   2. **Lorekeeper roster** — each Lorekeeper character anchors slot A1 (with a rotating A2
//!      partner) against a rotating Solace team. The reported number is the **player team's** win
//!      rate when this character leads.
//!
//! Reported per mirror tier (Hard and Expert, both teams at the same tier), with the per-faction
//! aggregate (avg + range) — the numbers the maintainer compares against the target bands
//! (Hard 50–60%, Expert 30–40% player-win).
//!
//!   cargo run -p recollect-bot --bin char_sweep_2v2 --release
use recollect_bot::{Difficulty, Faction, choose_as};
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::{
    LOREKEEPER_CHARACTERS, SOLACE_CHARACTERS, generate_deck, lorekeeper_character_deck,
    solace_character_deck,
};
use recollect_core::rng::Rng;
use recollect_core::state::{MatchResult, Phase};
use recollect_core::types::{CardDef, CardId, SeatSlot};
use recollect_core::{Engine, Seat};

/// Matches per character. 200 keeps the per-character CI tight (~±7pp at 50%); 40 chars × 200 = 8000
/// 2v2 matches. The 6×6 board + 4 hands is heavier than 1v1, so this is slower than `char_sweep` —
/// still a couple of minutes in release.
const N: u64 = 200;

const DISPOSITIONS: [&str; 5] = [
    "Cruelty",
    "Erasure",
    "Relentless",
    "LongForgetting",
    "Sorrow",
];
const STYLES: [&str; 5] = ["Embertide", "LongWatch", "Mistwalk", "Choir", "Bindle"];

fn wilson(p: f64, n: f64) -> f64 {
    if n == 0.0 {
        return 0.0;
    }
    1.96 * (p * (1.0 - p) / n).sqrt()
}

/// Drive one 2v2 PvE match to the finish. Team A (slots A1+A2) pilots Lorekeeper; team B (B1+B2)
/// pilots Solace; each models its opponent as the other faction. The engine is told the per-team
/// factions so its own 2v2 economics resolve faction-correctly. Returns (result, score_a, score_b).
fn play(
    seed: u64,
    decks: [Vec<CardId>; 4], // A1, B1, A2, B2
    a_diff: Difficulty,
    b_diff: Difficulty,
    cat: &[CardDef],
) -> (MatchResult, u8, u8) {
    let (mut e, _) = Engine::new_2v2_with_opener(
        seed,
        cat.to_vec(),
        decks,
        SeatSlot::A1,
        [Faction::Lorekeeper, Faction::Solace],
    );
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
        if steps > 20_000 {
            return (MatchResult::Draw, 0, 0);
        }
        // `active` is the team to move; the active SLOT is read inside `legal_commands`/`apply`.
        let seat = e.state().active;
        let cmd = if seat == Seat::B {
            choose_as(
                &e,
                seat,
                b_diff,
                Faction::Solace,
                Faction::Lorekeeper,
                &mut rng,
            )
        } else {
            choose_as(
                &e,
                seat,
                a_diff,
                Faction::Lorekeeper,
                Faction::Solace,
                &mut rng,
            )
        };
        e.apply(seat, cmd).expect("legal");
        steps += 1;
    }
}

/// One row of a per-character table.
struct Row {
    name: &'static str,
    group: &'static str,
    /// The PLAYER (team A / Lorekeeper) win rate this row reports.
    win: f64,
    score: f64,
    opp_score: f64,
}

fn print_table(title: &str, tier: &str, rows: &[Row], roster_mean: f64) {
    println!("\n{title}  ({tier}, {N} matches/char)\n");
    println!(
        "{:>16}  {:>14}  {:>14}  {:>11}  flag",
        "character", "group", "player-win% ±CI", "score"
    );
    for r in rows {
        let ci = wilson(r.win, N as f64) * 100.0;
        let delta = (r.win - roster_mean) * 100.0;
        let flag = if delta >= 12.0 {
            "<< HIGH"
        } else if delta <= -12.0 {
            ">> LOW"
        } else {
            ""
        };
        println!(
            "{:>16}  {:>14}  {:>7.1}% ± {:>3.0}  {:>4.1} v {:>4.1}  {}",
            r.name,
            r.group,
            r.win * 100.0,
            ci,
            r.score,
            r.opp_score,
            flag
        );
    }
    let (lo, hi) = range(rows);
    println!(
        "  roster mean player-win%: {:.1}%   range {:.1}%..{:.1}%   (flags: ±12pp from mean)",
        roster_mean * 100.0,
        lo * 100.0,
        hi * 100.0
    );
}

fn range(rows: &[Row]) -> (f64, f64) {
    let lo = rows.iter().map(|r| r.win).fold(f64::MAX, f64::min);
    let hi = rows.iter().map(|r| r.win).fold(f64::MIN, f64::max);
    (lo, hi)
}

/// The Solace roster at a given mirror tier: each disposition fields BOTH B-slots vs a rotating
/// Lorekeeper team. The per-row number is the PLAYER (team A) win rate.
fn solace_roster(player: Difficulty, solace: Difficulty, cat: &[CardDef]) -> (Vec<Row>, f64) {
    let mut rows = Vec::with_capacity(SOLACE_CHARACTERS.len());
    let mut win_sum = 0.0;
    for (i, ch) in SOLACE_CHARACTERS.iter().enumerate() {
        let (mut pwins, mut ps, mut bs) = (0u64, 0u64, 0u64);
        for seed in 0..N {
            // Team A: two Lorekeepers from the rotating Quick-Play styles, decorrelated.
            let a1 = generate_deck((seed % 6) as u8, seed, cat);
            let a2 = generate_deck(((seed + 3) % 6) as u8, seed ^ 0x1, cat);
            // Team B: both B-slots field THIS disposition (salted so B1 ≠ B2).
            let b1 = solace_character_deck(i, seed ^ 0x5EED, cat);
            let b2 = solace_character_deck(i, seed ^ 0x5EED ^ 0x2, cat);
            let (r, a, b) = play(seed, [a1, b1, a2, b2], player, solace, cat);
            if matches!(r, MatchResult::Win(Seat::A)) {
                pwins += 1;
            }
            ps += a as u64;
            bs += b as u64;
        }
        let win = pwins as f64 / N as f64;
        win_sum += win;
        rows.push(Row {
            name: ch.name,
            group: DISPOSITIONS[ch.disposition as usize],
            win,
            score: ps as f64 / N as f64,
            opp_score: bs as f64 / N as f64,
        });
    }
    (rows, win_sum / SOLACE_CHARACTERS.len() as f64)
}

/// The Lorekeeper roster at a given mirror tier: each character anchors slot A1 (rotating A2 partner)
/// vs a rotating Solace team. The per-row number is the PLAYER team's win rate.
fn lorekeeper_roster(player: Difficulty, solace: Difficulty, cat: &[CardDef]) -> (Vec<Row>, f64) {
    let mut rows = Vec::with_capacity(LOREKEEPER_CHARACTERS.len());
    let mut win_sum = 0.0;
    for (i, ch) in LOREKEEPER_CHARACTERS.iter().enumerate() {
        let (mut wins, mut ps, mut bs) = (0u64, 0u64, 0u64);
        for seed in 0..N {
            let a1 = lorekeeper_character_deck(i, seed, cat);
            // A2 rotates through the roster so the row measures THIS character leading, not one pair.
            let a2 = lorekeeper_character_deck(
                (i + 1 + (seed % 19) as usize) % LOREKEEPER_CHARACTERS.len(),
                seed ^ 0x1,
                cat,
            );
            // Team B: two Solace, rotating dispositions, decorrelated.
            let b1 = solace_character_deck((seed % 20) as usize, seed ^ 0x5EED, cat);
            let b2 = solace_character_deck(((seed + 7) % 20) as usize, seed ^ 0x5EED ^ 0x2, cat);
            let (r, a, b) = play(seed, [a1, b1, a2, b2], player, solace, cat);
            if matches!(r, MatchResult::Win(Seat::A)) {
                wins += 1;
            }
            ps += a as u64;
            bs += b as u64;
        }
        let win = wins as f64 / N as f64;
        win_sum += win;
        rows.push(Row {
            name: ch.name,
            group: STYLES[ch.style as usize],
            win,
            score: ps as f64 / N as f64,
            opp_score: bs as f64 / N as f64,
        });
    }
    (rows, win_sum / LOREKEEPER_CHARACTERS.len() as f64)
}

/// One full mirror tier: both rosters + the player-win aggregate, annotated vs the target band.
fn run_tier(player: Difficulty, solace: Difficulty, cat: &[CardDef]) {
    let tier = format!("{} mirror 2v2", player.name());
    println!(
        "\n########## 2v2 — {} player team vs {} Solace team ##########",
        player.name(),
        solace.name()
    );
    let (solace_rows, solace_mean) = solace_roster(player, solace, cat);
    print_table(
        "=== SOLACE roster (2v2) — PLAYER win rate vs each disposition ===",
        &tier,
        &solace_rows,
        solace_mean,
    );
    let (lk_rows, lk_mean) = lorekeeper_roster(player, solace, cat);
    print_table(
        "=== LOREKEEPER roster (2v2) — PLAYER win rate when this character leads ===",
        &tier,
        &lk_rows,
        lk_mean,
    );

    // The overall player-win aggregate across BOTH rosters (the headline number).
    let all: Vec<&Row> = solace_rows.iter().chain(lk_rows.iter()).collect();
    let overall = all.iter().map(|r| r.win).sum::<f64>() / all.len() as f64;
    let (slo, shi) = range(&solace_rows);
    let (llo, lhi) = range(&lk_rows);
    let (band_lo, band_hi) = match player {
        Difficulty::Expert => (0.30, 0.40),
        _ => (0.50, 0.60),
    };
    println!("\n--- 2v2 PLAYER-WIN AGGREGATE ({tier}) ---\n");
    println!(
        "  Solace roster:     mean {:.1}%   range {:.1}%..{:.1}%",
        solace_mean * 100.0,
        slo * 100.0,
        shi * 100.0
    );
    println!(
        "  Lorekeeper roster: mean {:.1}%   range {:.1}%..{:.1}%",
        lk_mean * 100.0,
        llo * 100.0,
        lhi * 100.0
    );
    println!(
        "  OVERALL player-win: {:.1}%   target band {:.0}-{:.0}%   gap vs band: {}",
        overall * 100.0,
        band_lo * 100.0,
        band_hi * 100.0,
        gap_note(overall, band_lo, band_hi)
    );
}

/// How far the measured win-rate sits from the target band, as a human note.
fn gap_note(p: f64, lo: f64, hi: f64) -> String {
    if p > hi {
        format!("+{:.1}pp ABOVE band (too easy)", (p - hi) * 100.0)
    } else if p < lo {
        format!("-{:.1}pp BELOW band (too hard)", (lo - p) * 100.0)
    } else {
        "IN BAND".to_string()
    }
}

fn main() {
    let cat = canon_catalog();
    println!(
        "2v2 PvE win-rate probe — parity of bin/char_sweep for the 6×6 2v2 telling (data only).\n\
         N={N} matches/character; PLAYER (team A / two Lorekeepers) win reported.\n\
         Target player-win bands: Hard 50-60%, Expert 30-40% (Expert is the hardest tier).\n\
         Two mirror tiers: Hard (both teams Hard) and Expert (both teams Expert)."
    );
    run_tier(Difficulty::Hard, Difficulty::Hard, &cat);
    run_tier(Difficulty::Expert, Difficulty::Expert, &cat);
}
