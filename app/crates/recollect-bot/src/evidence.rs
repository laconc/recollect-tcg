//! Evidence: the fairness simulation, shared by `bin/fleet.rs` so the
//! reporting bin AND the `tests/fleet_tripwires.rs` balance gates drive the SAME
//! playout + metrics — no duplicated loop. Everything plays through
//! `Engine::legal_commands` + `apply` like any client; bots get no private state
//! and no rule exemptions. Seeds are fixed (`0..n`), so a report is deterministic.
use crate::{SIM_DIFFICULTY, choose, drive_match};
use recollect_core::Engine;
use recollect_core::quickplay::{STYLES, generate_deck};
use recollect_core::rng::Rng;
use recollect_core::state::{MatchResult, MatchRules};
use recollect_core::types::{CardDef, CardId, CardKind, Seat};

/// One fairness cell: decisive outcomes + draws over `n` seeded matches, from
/// seat/team A's vantage. A moves first, so `a_pct > 0.5` is a first-mover edge.
pub struct FairnessReport {
    pub wins_a: u32,
    pub wins_b: u32,
    pub draws: u32,
}

impl FairnessReport {
    pub fn n(&self) -> u32 {
        self.wins_a + self.wins_b + self.draws
    }
    pub fn a_pct(&self) -> f64 {
        self.wins_a as f64 / self.n() as f64
    }
    pub fn b_pct(&self) -> f64 {
        self.wins_b as f64 / self.n() as f64
    }
    pub fn draw_pct(&self) -> f64 {
        self.draws as f64 / self.n() as f64
    }
    /// First-mover edge in percentage points: `A% − 50`.
    pub fn first_edge_pp(&self) -> f64 {
        (self.a_pct() - 0.5) * 100.0
    }
    /// Wilson 95% half-width on `A%`, in percentage points.
    pub fn halfwidth_pp(&self) -> f64 {
        let (p, n) = (self.a_pct(), self.n() as f64);
        1.96 * (p * (1.0 - p) / n).sqrt() * 100.0
    }
}

pub fn play_1v1(
    seed: u64,
    last_round: u8,
    cat: &[CardDef],
    da: Vec<CardId>,
    db: Vec<CardId>,
) -> MatchResult {
    let mut rules = MatchRules::default();
    rules.last_round = last_round;
    let (mut e, _) =
        Engine::new_with_rules(seed, cat.to_vec(), da, db, rules, recollect_core::Seat::A);
    let mut bot = Rng::from_seed(seed ^ 0xF1EE7);
    drive_match(
        &mut e,
        |e, seat| choose(e, seat, SIM_DIFFICULTY, &mut bot),
        |_, _| {},
    )
}

pub fn play_2v2(
    seed: u64,
    last_round: u8,
    cat: &[CardDef],
    decks: [Vec<CardId>; 4],
) -> MatchResult {
    let (mut e, _) = Engine::new_2v2(seed, cat.to_vec(), decks);
    e.state_mut_for_test().rules.last_round = last_round;
    let mut bot = Rng::from_seed(seed ^ 0x2002);
    drive_match(
        &mut e,
        |e, seat| choose(e, seat, SIM_DIFFICULTY, &mut bot),
        |_, _| {},
    )
}

/// Quick-Play 1v1 decks: both narrators draw the same style, different seeds.
pub fn qp_decks(seed: u64, cat: &[CardDef]) -> (Vec<CardId>, Vec<CardId>) {
    let s = STYLES[(seed % STYLES.len() as u64) as usize].id;
    (
        generate_deck(s, seed, cat),
        generate_deck(s, seed ^ 0x99, cat),
    )
}

/// Quick-Play 2v2 decks: one style across the four seats, four seeds.
pub fn qp_decks_2v2(seed: u64, cat: &[CardDef]) -> [Vec<CardId>; 4] {
    let s = STYLES[(seed % STYLES.len() as u64) as usize].id;
    [
        generate_deck(s, seed, cat),
        generate_deck(s, seed ^ 0x1, cat),
        generate_deck(s, seed ^ 0x2, cat),
        generate_deck(s, seed ^ 0x3, cat),
    ]
}

/// 1v1 fairness over `n` seeded Quick-Play matches at the given last-round clock.
pub fn fairness_1v1(cat: &[CardDef], last_round: u8, n: u64) -> FairnessReport {
    tally(n, |seed| {
        let (da, db) = qp_decks(seed, cat);
        play_1v1(seed, last_round, cat, da, db)
    })
}

/// 2v2 fairness — `a_pct` is the FIRST team's win share, the seat-order fairness signal.
pub fn fairness_2v2(cat: &[CardDef], last_round: u8, n: u64) -> FairnessReport {
    tally(n, |seed| {
        play_2v2(seed, last_round, cat, qp_decks_2v2(seed, cat))
    })
}

/// Deck texture: across `n` Quick-Play decks of every style, the mean fraction of
/// cards that are Spirits. Low ⇒ the deck-gen has gone spell-heavy (a texture signal).
pub fn quickplay_spirit_fraction(cat: &[CardDef], n: u64) -> f64 {
    let (mut spirits, mut total) = (0u64, 0u64);
    for style in STYLES {
        for seed in 0..n {
            for id in generate_deck(style.id, seed, cat) {
                total += 1;
                if matches!(cat[id.0 as usize].kind, CardKind::Spirit) {
                    spirits += 1;
                }
            }
        }
    }
    spirits as f64 / total as f64
}

fn tally(n: u64, mut play: impl FnMut(u64) -> MatchResult) -> FairnessReport {
    let (mut wins_a, mut wins_b, mut draws) = (0u32, 0u32, 0u32);
    for seed in 0..n {
        match play(seed) {
            MatchResult::Win(Seat::A) => wins_a += 1,
            MatchResult::Win(Seat::B) => wins_b += 1,
            MatchResult::Draw => draws += 1,
        }
    }
    FairnessReport {
        wins_a,
        wins_b,
        draws,
    }
}
