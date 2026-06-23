//! Tier knob sweep — the search that re-derives the four difficulty tiers' `(temperature,
//! depth)` from scratch for a clean MONOTONE, well-separated ladder. This is the analysis
//! binary behind a re-band: `calibrate` reports the *shipped* tiers' ladder, but it can only
//! see the four named points; this binary sweeps the whole knob grid through the same
//! `choose_params` seam so a candidate point is evaluated *before* it becomes a tier.
//!
//! It answers three questions a re-band has to answer together:
//!   1. **Ladder** — for a candidate four-tuple of knob points, the full pairwise win-rate
//!      matrix (the `calibrate` table), so we can SEE monotonicity + adjacent separation.
//!   2. **1v1 PvE-Solace** — player(Lorekeeper)-win vs a Solace at the candidate Hard/Expert
//!      knobs (the `solace_winnability` band, the "is the Solace brutal" check).
//!   3. **2v2 PvE-Solace** — the same on the 6×6, where a depth-2 Solace *pair* under-walls
//!      (the 2v2-Expert too-easy cell).
//!
//! It does NOT change the engine, cards, or the shipped tiers — it's pure measurement that
//! the maintainer reads to choose the four points, which then go into `agent.rs`. Everything
//! runs through the same public `legal_commands`/`apply` seam every client uses; the bots get
//! no private state.
//!
//! Modes (first CLI arg):
//!   - `profile`  — each knob point's strength vs a fixed reference anchor (a cheap 1-D
//!     strength axis to pre-rank candidates before the O(n²) ladder).
//!   - `ladder T1,D1 T2,D2 T3,D3 T4,D4` — the full pairwise `calibrate` matrix for that
//!     candidate four-tuple (temp,depth pairs), plus the monotonicity verdict.
//!   - `pve T,D` — 1v1 + 2v2 player-win vs the Solace at a candidate (temp,depth) mirror.
//!   - `grid`     — runs `profile`, then the chosen finalist `ladder` + `pve` for the
//!     re-band's recommended tuple, as a one-shot reproduction.
//!
//!   cargo run -p recollect-bot --bin tier_sweep --release -- profile
//!   cargo run -p recollect-bot --bin tier_sweep --release -- ladder 400,1 90,1 22,1 4,2
//!   cargo run -p recollect-bot --bin tier_sweep --release -- pve 22,1
use recollect_bot::{Faction, choose_params};
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::{generate_deck, generate_deck_for};
use recollect_core::rng::Rng;
use recollect_core::state::{MatchResult, MatchRules, Phase};
use recollect_core::types::{CardDef, SeatSlot};
use recollect_core::{Engine, Seat};

/// A candidate knob point: a (temperature, depth) the sweep evaluates as a would-be tier.
#[derive(Clone, Copy, PartialEq)]
struct Knob {
    temp: f64,
    depth: u8,
}

impl std::fmt::Display for Knob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "t{}/d{}", self.temp as i64, self.depth)
    }
}

fn wilson(p: f64, n: f64) -> f64 {
    1.96 * (p * (1.0 - p) / n).sqrt()
}

/// One Lorekeeper-mirror match: seat A at knob `a`, seat B at knob `b`. Decks derived from the
/// seed (decorrelated A/B) exactly as `calibrate` does, so a `ladder` cell reproduces the
/// shipped `calibrate` number when the knobs match a tier. Returns A's result.
fn play_mirror(seed: u64, a: Knob, b: Knob, cat: &[CardDef]) -> MatchResult {
    let da = generate_deck((seed % 6) as u8, seed, cat);
    let db = generate_deck(((seed + 3) % 6) as u8, seed ^ 0x5EED, cat);
    let (mut e, _) = Engine::new(seed, cat.to_vec(), da, db);
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
        let (k, rng) = if seat == Seat::A {
            (a, &mut ra)
        } else {
            (b, &mut rb)
        };
        let cmd = choose_params(
            &e,
            seat,
            k.temp,
            k.depth,
            Faction::Lorekeeper,
            Faction::Lorekeeper,
            rng,
        );
        e.apply(seat, cmd).expect("legal");
        steps += 1;
    }
}

/// A's win rate over N mirror matches vs B (each seed once; seat fairness is a separate concern,
/// matched to `calibrate`).
fn mirror_winrate(a: Knob, b: Knob, n: u64, cat: &[CardDef]) -> (f64, f64) {
    let mut wins = 0u64;
    for seed in 0..n {
        if let MatchResult::Win(Seat::A) = play_mirror(seed, a, b, cat) {
            wins += 1;
        }
    }
    let p = wins as f64 / n as f64;
    (p, wilson(p, n as f64))
}

/// 1v1 PvE: player (A, Lorekeeper) vs Solace (B), both at knob `k` (the mirror tier). Real Solace
/// economy in force (factions = [Lorekeeper, Solace]) so the erasure tally scores — exactly the
/// `solace_winnability` / `char_sweep` contest. Returns the PLAYER win rate over N seeds.
fn pve_1v1_playerwin(k: Knob, n: u64, cat: &[CardDef]) -> (f64, f64) {
    let mut pw = 0u64;
    for seed in 0..n {
        let da = generate_deck((seed % 6) as u8, seed, cat);
        let db = generate_deck_for(Faction::Solace, ((seed + 2) % 6) as u8, seed ^ 0x5EED, cat);
        let mut rules = MatchRules::default();
        rules.factions = [Faction::Lorekeeper, Faction::Solace];
        let (mut e, _) = Engine::new_with_rules(seed, cat.to_vec(), da, db, rules, Seat::A);
        let mut rng = Rng::from_seed(seed ^ 0xA);
        let mut steps = 0;
        loop {
            if let Phase::Finished { result, .. } = e.state().phase {
                if matches!(result, MatchResult::Win(Seat::A)) {
                    pw += 1;
                }
                break;
            }
            if steps > 5000 {
                break;
            }
            let seat = e.state().active;
            let (fac, opp) = if seat == Seat::B {
                (Faction::Solace, Faction::Lorekeeper)
            } else {
                (Faction::Lorekeeper, Faction::Solace)
            };
            let cmd = choose_params(&e, seat, k.temp, k.depth, fac, opp, &mut rng);
            e.apply(seat, cmd).expect("legal");
            steps += 1;
        }
    }
    let p = pw as f64 / n as f64;
    (p, wilson(p, n as f64))
}

/// 2v2 PvE: team A (two Lorekeepers) vs team B (two Solace), both teams at knob `k`. Mirrors
/// `char_sweep_2v2`'s engine setup. Returns the PLAYER (team A) win rate over N seeds.
fn pve_2v2_playerwin(k: Knob, n: u64, cat: &[CardDef]) -> (f64, f64) {
    let mut pw = 0u64;
    for seed in 0..n {
        let a1 = generate_deck((seed % 6) as u8, seed, cat);
        let a2 = generate_deck(((seed + 3) % 6) as u8, seed ^ 0x1, cat);
        let b1 = generate_deck_for(Faction::Solace, ((seed + 2) % 6) as u8, seed ^ 0x5EED, cat);
        let b2 = generate_deck_for(
            Faction::Solace,
            ((seed + 4) % 6) as u8,
            seed ^ 0x5EED ^ 0x2,
            cat,
        );
        let (mut e, _) = Engine::new_2v2_with_opener(
            seed,
            cat.to_vec(),
            [a1, b1, a2, b2],
            SeatSlot::A1,
            [Faction::Lorekeeper, Faction::Solace],
        );
        let mut rng = Rng::from_seed(seed ^ 0xA);
        let mut steps = 0;
        loop {
            if let Phase::Finished { result, .. } = e.state().phase {
                if matches!(result, MatchResult::Win(Seat::A)) {
                    pw += 1;
                }
                break;
            }
            if steps > 20_000 {
                break;
            }
            let seat = e.state().active;
            let (fac, opp) = if seat == Seat::B {
                (Faction::Solace, Faction::Lorekeeper)
            } else {
                (Faction::Lorekeeper, Faction::Solace)
            };
            let cmd = choose_params(&e, seat, k.temp, k.depth, fac, opp, &mut rng);
            e.apply(seat, cmd).expect("legal");
            steps += 1;
        }
    }
    let p = pw as f64 / n as f64;
    (p, wilson(p, n as f64))
}

/// The candidate knob grid for the profile axis. Spans the depth-1 temperature range (near-random
/// down to sharp) and the depth-2 range, so we can see where each depth's strength sits on ONE
/// axis (vs a fixed strong reference) before paying for the pairwise ladder.
fn grid() -> Vec<Knob> {
    let mut v = Vec::new();
    for &temp in &[
        400.0, 200.0, 120.0, 90.0, 60.0, 45.0, 35.0, 25.0, 16.0, 8.0, 4.0,
    ] {
        v.push(Knob { temp, depth: 1 });
    }
    for &temp in &[120.0, 90.0, 60.0, 45.0, 35.0, 25.0, 16.0, 8.0, 4.0] {
        v.push(Knob { temp, depth: 2 });
    }
    v
}

/// `profile` mode: rank every grid knob by its win rate vs a fixed strong reference anchor
/// (t4/d2 — the would-be Expert). A monotone 1-D strength axis to pre-rank candidates.
fn run_profile(cat: &[CardDef]) {
    let n = 160u64;
    let anchor = Knob {
        temp: 4.0,
        depth: 2,
    };
    println!(
        "PROFILE — each knob (seat A) vs the reference anchor {anchor} (seat B), {n} matches."
    );
    println!("Win% vs anchor is a 1-D strength axis: higher = stronger. Pick tier points by");
    println!("spreading evenly along it (Easy low, Expert ≈ anchor).\n");
    println!("{:>10}  {:>14}", "knob", "win% vs anchor");
    let mut rows: Vec<(Knob, f64, f64)> = grid()
        .into_iter()
        .map(|k| {
            let (p, hw) = mirror_winrate(k, anchor, n, cat);
            (k, p, hw)
        })
        .collect();
    rows.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    for (k, p, hw) in rows {
        println!(
            "{:>10}  {:>9}±{:>3}",
            k.to_string(),
            format!("{:.0}%", p * 100.0),
            format!("{:.0}", hw * 100.0)
        );
    }
}

/// Parse a `temp,depth` CLI token into a Knob.
fn parse_knob(s: &str) -> Knob {
    let (t, d) = s
        .split_once(',')
        .expect("knob must be temp,depth e.g. 35,2");
    Knob {
        temp: t.parse().expect("temp"),
        depth: d.parse().expect("depth"),
    }
}

/// `ladder` mode: the full pairwise win matrix for a candidate four-tuple + a monotonicity verdict.
fn run_ladder(tiers: &[Knob], cat: &[CardDef]) {
    let n = 200u64;
    let names = ["T1(Easy)", "T2(Norm)", "T3(Hard)", "T4(Exp)"];
    println!("LADDER — candidate tiers, {n} matches/ordered pairing, A(row) win% vs B(col):\n");
    print!("{:>10}", "");
    for (i, k) in tiers.iter().enumerate() {
        print!("{:>14}", format!("{}={}", names[i], k));
    }
    println!();
    // Store A-vs-B win% to check monotonicity by column afterwards.
    let mut cell = vec![vec![f64::NAN; tiers.len()]; tiers.len()];
    for (ai, a) in tiers.iter().enumerate() {
        print!("{:>10}", names[ai]);
        for (bi, b) in tiers.iter().enumerate() {
            if ai == bi {
                print!("{:>14}", "—");
                continue;
            }
            let (p, hw) = mirror_winrate(*a, *b, n, cat);
            cell[ai][bi] = p;
            print!(
                "{:>9}±{:>3}",
                format!("{:.0}%", p * 100.0),
                format!("{:.0}", hw * 100.0)
            );
        }
        println!();
    }
    // Monotonicity: down every column, a stronger (lower-index → we order weak→strong) row must
    // beat the column tier at a rate no lower than a weaker row does. Also the headline gaps.
    println!("\nMONOTONICITY (down each column, win% must be non-decreasing weak→strong):");
    let mut ok = true;
    for bi in 0..tiers.len() {
        let col: Vec<(usize, f64)> = (0..tiers.len())
            .filter(|&ai| ai != bi)
            .map(|ai| (ai, cell[ai][bi]))
            .collect();
        for w in col.windows(2) {
            // rows are in weak→strong order already (index order), so later index must be ≥ earlier
            if w[1].1 + 1e-9 < w[0].1 {
                println!(
                    "  ✗ col {}: {} ({:.0}%) < {} ({:.0}%) — NON-MONOTONE",
                    names[bi],
                    names[w[1].0],
                    w[1].1 * 100.0,
                    names[w[0].0],
                    w[0].1 * 100.0
                );
                ok = false;
            }
        }
    }
    // Adjacent separation: each stronger tier should beat its immediate weaker neighbour clearly
    // above 50% (the "well-separated" half of the goal).
    println!("\nADJACENT separation (stronger beats next-weaker; want a clear >50%):");
    for i in 1..tiers.len() {
        let p = cell[i][i - 1];
        let verdict = if p >= 0.60 {
            "good (>=60%)"
        } else if p >= 0.55 {
            "ok (55-60%)"
        } else {
            "TIGHT (<55%)"
        };
        println!(
            "  {} beats {}: {:.0}%  [{}]",
            names[i],
            names[i - 1],
            p * 100.0,
            verdict
        );
    }
    println!(
        "\nVERDICT: ladder is {}.",
        if ok { "MONOTONE" } else { "NOT monotone" }
    );
}

/// `pve` mode: 1v1 + 2v2 player-win vs the Solace at a candidate mirror knob.
fn run_pve(k: Knob, cat: &[CardDef]) {
    let n = 200u64;
    println!("PVE vs Solace at mirror {k}, {n} seeds (player = Lorekeeper win%):\n");
    let (p1, h1) = pve_1v1_playerwin(k, n, cat);
    let (p2, h2) = pve_2v2_playerwin(k, n, cat);
    let band1 = if (0.25..=0.86).contains(&p1) {
        "in solace_winnability band 0.25-0.86"
    } else {
        "OUT of band"
    };
    println!(
        "  1v1 player-win: {:.1}% ± {:.0}   ({band1})",
        p1 * 100.0,
        h1 * 100.0
    );
    println!("  2v2 player-win: {:.1}% ± {:.0}", p2 * 100.0, h2 * 100.0);
}

fn main() {
    let cat = canon_catalog();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("profile") => run_profile(&cat),
        Some("ladder") => {
            let tiers: Vec<Knob> = args[1..].iter().map(|s| parse_knob(s)).collect();
            assert_eq!(tiers.len(), 4, "ladder needs four temp,depth tuples");
            run_ladder(&tiers, &cat);
        }
        Some("pve") => run_pve(parse_knob(&args[1]), &cat),
        _ => {
            println!("usage: tier_sweep <profile | ladder T,D T,D T,D T,D | pve T,D>");
        }
    }
}
