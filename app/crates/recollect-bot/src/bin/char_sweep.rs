//! Per-character win% sweep — the EVIDENCE that the combat stats are fair PER
//! CHARACTER, not just in aggregate. `bin/solace_sweep` reports the skill *curve* but folds all 20
//! Solace dispositions into one band (`seed % 20`); this binary breaks the PvE fight out to EACH of
//! the ~40 character decks — 20 Solace dispositions + 20 Lorekeeper characters — at a representative
//! skill tier (the Hard mirror), so a single too-easy / too-hard character can't hide inside the mean.
//!
//! Two tables, both at the **Hard mirror** (player and opponent both Hard — the mid-skill contest the
//! `solace_winnability` gate guards):
//!
//!   1. **Solace roster** — each Solace character (seat B) vs a rotating Lorekeeper player (seat A).
//!      The reported number is the **player** win rate. The contract is "the Solace fight is fair":
//!      a HIGH player win% ⇒ this disposition is a *pushover*; a LOW one ⇒ it's a *wall*. The fair
//!      band mirrors the gate: ~25–86% player win (post-Bal2 the Hard-mirror roster mean sits near
//!      even, ~46% — the combat stats structurally favor the board-SCORING Lorekeeper over the Solace's
//!      off-board erasure tally, but the Solace's depth-2 walling pulls it back to near-even — so we
//!      flag *relative* outliers against the roster mean, not just the absolute band).
//!
//!   2. **Lorekeeper roster** — each Lorekeeper character (seat A) vs a rotating Solace opponent
//!      (seat B). The reported number is the **Lorekeeper character's** win rate. Here a high number
//!      is that character winning *its own* matches; we flag the chars that over- or under-perform
//!      the roster mean.
//!
//! The Solace skew is the gap between "how much the average Lorekeeper beats the average Solace" —
//! quantified per-disposition so we can see whether it's uniform or concentrated. A targeted
//! `solace_weight`/disposition fix would lift the worst-off dispositions; this binary is the
//! before/after measure for any such tweak.
//!
//!   cargo run -p recollect-bot --bin char_sweep --release
use recollect_bot::{Difficulty, Faction, choose_as};
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::{
    LOREKEEPER_CHARACTERS, SOLACE_CHARACTERS, generate_deck, lorekeeper_character_deck,
    solace_character_deck,
};
use recollect_core::rng::Rng;
use recollect_core::state::{MatchResult, Phase};
use recollect_core::types::CardDef;
use recollect_core::{Engine, Seat};

/// Matches per character. 200 keeps the per-character CI tight (~±7pp at 50%) while the whole sweep
/// (40 chars × 200 = 8000 matches) stays well under a minute in release.
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

/// Drive one PvE match to the finish. Seat A pilots Lorekeeper, seat B pilots Solace; each models
/// its opponent as the other faction. Returns (result, score_a, score_b).
///
/// The engine is built with `factions = [Lorekeeper, Solace]` so seat B's scoring is the REAL Solace
/// economy — its Unwritten leave no impression, the Dusk sweeps them, and each banish/unwrite banks
/// the off-board erasure tally that joins B's score at Nightfall. (A plain `Engine::new` defaults
/// both seats to Lorekeeper, which would silently make seat B score like a Lorekeeper — stamping
/// impressions instead of tallying erasures — and measure a mirror, not the PvE fight.)
fn play(
    seed: u64,
    da: Vec<recollect_core::types::CardId>,
    db: Vec<recollect_core::types::CardId>,
    a_diff: Difficulty,
    b_diff: Difficulty,
    cat: &[CardDef],
) -> (MatchResult, u8, u8) {
    let mut rules = recollect_core::state::MatchRules::default();
    rules.factions = [Faction::Lorekeeper, Faction::Solace];
    let (mut e, _) = Engine::new_with_rules(seed, cat.to_vec(), da, db, rules, Seat::A);
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
    /// The win rate the table reports for this character (player-win for Solace rows; this
    /// character's own win for Lorekeeper rows).
    win: f64,
    /// Mean (own score, opponent score) for this character.
    score: f64,
    opp_score: f64,
}

fn print_table(title: &str, tier: &str, rows: &[Row], roster_mean: f64) {
    println!("\n{title}  ({tier}, {N} matches/char)\n");
    println!(
        "{:>16}  {:>14}  {:>14}  {:>11}  flag",
        "character", "group", "win% ±CI", "score"
    );
    for r in rows {
        let ci = wilson(r.win, N as f64) * 100.0;
        // Flag relative to the roster mean: ±12pp from the mean is a meaningful outlier given the
        // ~±7pp CI at this N. We name the *direction* in the roster's own terms below the table.
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
    println!(
        "  roster mean win%: {:.1}%   (flags: ±12pp from mean)",
        roster_mean * 100.0
    );
}

/// The Solace roster at a given mirror tier: each disposition (seat B) vs a rotating Lorekeeper
/// player (seat A). Returns (rows, roster-mean player-win). The reported per-row number is the
/// PLAYER win rate (high => this Solace is a pushover; low => a wall).
fn solace_roster(player: Difficulty, solace: Difficulty, cat: &[CardDef]) -> (Vec<Row>, f64) {
    let mut rows = Vec::with_capacity(SOLACE_CHARACTERS.len());
    let mut win_sum = 0.0;
    for (i, ch) in SOLACE_CHARACTERS.iter().enumerate() {
        let (mut pwins, mut ps, mut bs) = (0u64, 0u64, 0u64);
        for seed in 0..N {
            // The player rotates through the 6 Lorekeeper Quick-Play styles, decorrelated from the
            // Solace draw — so the row measures the *disposition*, not one player archetype.
            let da = generate_deck((seed % 6) as u8, seed, cat);
            let db = solace_character_deck(i, seed ^ 0x5EED, cat);
            let (r, a, b) = play(seed, da, db, player, solace, cat);
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

/// The Lorekeeper roster at a given mirror tier: each character (seat A) vs a rotating Solace
/// opponent (seat B). Returns (rows, roster-mean). The per-row number is THIS Lorekeeper's win rate.
fn lorekeeper_roster(player: Difficulty, solace: Difficulty, cat: &[CardDef]) -> (Vec<Row>, f64) {
    let mut rows = Vec::with_capacity(LOREKEEPER_CHARACTERS.len());
    let mut win_sum = 0.0;
    for (i, ch) in LOREKEEPER_CHARACTERS.iter().enumerate() {
        let (mut wins, mut ps, mut bs) = (0u64, 0u64, 0u64);
        for seed in 0..N {
            let da = lorekeeper_character_deck(i, seed, cat);
            // Opponent rotates through all 20 Solace dispositions, decorrelated.
            let db = solace_character_deck((seed % 20) as usize, seed ^ 0x5EED, cat);
            let (r, a, b) = play(seed, da, db, player, solace, cat);
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

/// Quantify the Solace skew per disposition: how far above 50% the Lorekeeper sits at this mirror,
/// aggregated by disposition, so we can see if the skew is uniform or concentrated. (Player-win =
/// 1 - Solace-win; a higher number = a more pushover disposition.)
fn print_skew(solace_rows: &[Row], solace_mean: f64, lk_mean: f64) {
    println!("\n--- SOLACE SKEW by disposition (player-win; 50% = balanced) ---\n");
    println!(
        "{:>16}  {:>16}  {:>11}",
        "disposition", "mean player-win", "skew vs 50%"
    );
    for dname in DISPOSITIONS.iter() {
        let group: Vec<&Row> = solace_rows.iter().filter(|r| r.group == *dname).collect();
        if group.is_empty() {
            continue;
        }
        let mean = group.iter().map(|r| r.win).sum::<f64>() / group.len() as f64;
        println!(
            "{:>16}  {:>15.1}%  {:>+9.1}pp",
            dname,
            mean * 100.0,
            (mean - 0.5) * 100.0
        );
    }
    println!(
        "\nroster: Solace player-win mean {:.1}% (skew {:+.1}pp); Lorekeeper char-win mean {:.1}%",
        solace_mean * 100.0,
        (solace_mean - 0.5) * 100.0,
        lk_mean * 100.0
    );
}

/// One full mirror tier: both rosters + the per-disposition skew.
fn run_tier(player: Difficulty, solace: Difficulty, cat: &[CardDef]) {
    let tier = format!("{} mirror", player.name());
    println!(
        "\n########## {} player vs {} Solace ##########",
        player.name(),
        solace.name()
    );
    let (solace_rows, solace_mean) = solace_roster(player, solace, cat);
    print_table(
        "=== SOLACE roster — PLAYER win rate vs each disposition ===",
        &tier,
        &solace_rows,
        solace_mean,
    );
    let (lk_rows, lk_mean) = lorekeeper_roster(player, solace, cat);
    print_table(
        "=== LOREKEEPER roster — this character's win rate vs the Solace ===",
        &tier,
        &lk_rows,
        lk_mean,
    );
    print_skew(&solace_rows, solace_mean, lk_mean);
}

fn main() {
    let cat = canon_catalog();
    println!(
        "Per-character win% sweep — the canon catalog.\n\
         N={N} matches/character; player-win reported. Solace fair band ~25-86% player-win (the gate).\n\
         Two tiers: the Hard mirror (the gate's mid-skill contest, reporting band 50-60% player-win) and\n\
         the Expert mirror (both sides at the toughest tier, reporting band 30-40% player-win).\n\
         Post-Bal2 the Solace is a near-even 1v1 fight at BOTH depth-2 tiers: player-win ~46% at the Hard\n\
         mirror and ~51% at Expert (a touch EASIER at Expert — its colder, less trade-happy Solace gives\n\
         the player more room). Both depth-2 mirrors land near even; depth-2 is where the Solace's denial\n\
         earns its keep (a depth-1 mirror leaves it a ~75% pushover). The residual tilt to the player is\n\
         structural (board-scoring LK > erasure tally). The per-disposition spread is the durable skew —\n\
         see docs/difficulty.md."
    );

    // The representative tier is the Hard mirror — the contest `solace_winnability` guards.
    run_tier(Difficulty::Hard, Difficulty::Hard, &cat);
    // A second tier: the Expert mirror — both depth-2 mirrors land near even (~46% Hard, ~51% Expert);
    // Expert is the genuinely harder tier in the *mirror* ladder (it tops Hard 59% head-to-head), even
    // though the Solace fight is a hair softer for the player at Expert than at Hard.
    run_tier(Difficulty::Expert, Difficulty::Expert, &cat);
}
