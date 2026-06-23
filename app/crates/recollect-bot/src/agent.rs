//! The opponent agent and its difficulty tiers.
//!
//! One agent, four difficulties, scaled by two honest knobs:
//!   - **temperature** — how often it picks a worse-than-best move (softmax over
//!     the greedy move scores). High temperature = makes mistakes = easier.
//!   - **depth** — how many plies it looks ahead (1 = greedy this move only;
//!     2 = considers the opponent's best reply). Deeper = stronger = slower.
//!
//! The `choose` interface is the seam a learned policy can sit behind (see
//! docs/decisions/bot_and_ml_plan.md): difficulty stays the contract, so the
//! brain behind it can change without touching callers or the engine.
use recollect_core::Engine;
use recollect_core::rng::Rng;
use recollect_core::state::Command;
use recollect_core::types::Seat;

/// The four difficulty tiers, labelled with the common names players expect.
/// Each maps to concrete (temperature, depth) the calibration fleet verifies.
/// `Normal` is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    /// Easy — plays nearly at random. A gentle first game.
    Easy,
    /// Normal — mostly sensible, makes occasional mistakes. The default.
    Normal,
    /// Hard — a real opponent: two-ply lookahead like Expert, but a hotter
    /// temperature, so it reads a reply ahead yet still slips. A genuine
    /// challenge for a strong player without Expert's near-perfect play.
    Hard,
    /// Expert — two-ply, picks the best line; the toughest shipped tier.
    Expert,
}

impl Default for Difficulty {
    /// New games default to Normal.
    fn default() -> Self {
        Difficulty::Normal
    }
}

/// Which side the agent is piloting — the engine's [`recollect_core::types::Faction`], re-exported
/// so the bot, engine, and server share ONE faction type (no conversions at the seams).
///
/// The factions optimize for different things: a **Lorekeeper** banks Score via standing
/// spirits AND impressions, so trading is fine — a banished spirit still leaves a scoring
/// impression. The **Solace** leaves no impression when its Unwritten fall (all-or-nothing), so a
/// trade nets it zero; it plays board presence + denial, and its removals score off-board (the
/// erasure tally). The same scoring function serves both, branched on this — keeping the
/// single-model property for the eventual ML swap.
pub use recollect_core::types::Faction;

impl Difficulty {
    /// All tiers, weakest → strongest, for a UI to enumerate.
    pub const ALL: [Difficulty; 4] = [
        Difficulty::Easy,
        Difficulty::Normal,
        Difficulty::Hard,
        Difficulty::Expert,
    ];
}

impl Difficulty {
    /// Softmax temperature, scaled to the greedy score range (scores span
    /// roughly ±120, so a temperature near that flattens the distribution to
    /// near-random; a small one is near-deterministic). Calibrated by
    /// `bin/calibrate.rs` to produce a real strength ladder.
    pub fn temperature(self) -> f64 {
        match self {
            Difficulty::Easy => 400.0,  // near-random: barely prefers good moves
            Difficulty::Normal => 90.0, // occasionally picks a sub-optimal line
            Difficulty::Hard => 35.0, // depth-2, hotter than Expert: a real challenge that still slips
            Difficulty::Expert => 8.0, // almost always the best move (depth-2; not so cold it turns passive)
        }
    }
    /// Search depth in plies (1 = this move only; 2 = consider the reply).
    /// Hard joins Expert at depth-2 (it differs only by a hotter temperature):
    /// lookahead is where the Solace earns its keep, so the depth split is
    /// Easy/Normal depth-1, Hard/Expert depth-2. These four `(temperature, depth)`
    /// points are the **Bal2 re-sweep** — re-derived from scratch (via
    /// `bin/tier_sweep`'s knob search) for a monotone, well-separated ladder
    /// (calibrate adjacent rungs ~69/63/59; see `docs/difficulty.md`).
    pub fn depth(self) -> u8 {
        match self {
            Difficulty::Easy | Difficulty::Normal => 1,
            Difficulty::Hard | Difficulty::Expert => 2,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Normal => "Normal",
            Difficulty::Hard => "Hard",
            Difficulty::Expert => "Expert",
        }
    }
}

/// Choose a command for `seat` at the given difficulty. Deterministic given the
/// rng. The whole knowledge it has is what any player sees: the public board,
/// its own hand, and the forecast math — no private opponent info.
pub fn choose(e: &Engine, seat: Seat, diff: Difficulty, rng: &mut Rng) -> Command {
    // The common case (Quick Play) is a Lorekeeper mirror; pilot accordingly.
    choose_as(e, seat, diff, Faction::Lorekeeper, Faction::Lorekeeper, rng)
}

/// Faction-aware [`choose`]: pilot `seat` as `faction`, modelling the
/// opponent's reply as `opp_faction`. The same softmax-by-difficulty selection;
/// only the heuristic weights differ by faction (see [`crate::greedy_score_as`]).
pub fn choose_as(
    e: &Engine,
    seat: Seat,
    diff: Difficulty,
    faction: Faction,
    opp_faction: Faction,
    rng: &mut Rng,
) -> Command {
    // A tier IS its (temperature, depth) pair — the enum is a named point in the knob
    // space the calibration fleet sweeps. Delegating to `choose_params` keeps ONE
    // selection path, so a swept candidate and the shipped tier that adopts the same
    // knobs play bit-identically (the calibration is exact, not approximate).
    choose_params(
        e,
        seat,
        diff.temperature(),
        diff.depth(),
        faction,
        opp_faction,
        rng,
    )
}

/// The parametric core of [`choose_as`]: pilot `seat` with an explicit
/// `(temperature, depth)` instead of a named [`Difficulty`]. The tier enum is just a
/// labelled point in this space, so the `calibrate`/`char_sweep` fleet sweeps the knob
/// grid through this seam and a swept candidate plays identically to the tier that later
/// adopts the same knobs. Deterministic given the rng.
#[allow(clippy::too_many_arguments)]
pub fn choose_params(
    e: &Engine,
    seat: Seat,
    temperature: f64,
    depth: u8,
    faction: Faction,
    opp_faction: Faction,
    rng: &mut Rng,
) -> Command {
    let legal = e.legal_commands(seat);
    if legal.is_empty() {
        return Command::EndTurn;
    }
    // A pending choice must be resolved before anything else — when one is open,
    // only Choose commands are legal, so pick among those directly (the softmax
    // over heuristic scores doesn't model choice value).
    let choices: Vec<&Command> = legal
        .iter()
        .filter(|c| matches!(c, Command::Choose { .. }))
        .collect();
    if !choices.is_empty() {
        // Glimpse (§5) is a real tradeoff — decide its two steps deliberately rather
        // than at random. Step 1 (GlimpseBurn): spend the LEAST valuable hand card.
        // Step 2 (Glimpse): keep a worthwhile draw, else bank +1 Anima. Other choice
        // kinds (target/recover/peek) keep the difficulty-flavoured random pick.
        match e.state().pending_choice {
            Some(recollect_core::state::PendingChoice::GlimpseBurn { .. }) => {
                return crate::glimpse_burn_choice(e, faction);
            }
            Some(recollect_core::state::PendingChoice::Glimpse { .. }) => {
                return crate::glimpse_keep_or_bottom(e, faction);
            }
            _ => {}
        }
        let idx = (rng.next_u64() as usize) % choices.len();
        return choices[idx].clone();
    }
    // Score every legal move (depth-1 base, optional depth-2 refinement).
    let mut scored: Vec<(f64, &Command)> = legal
        .iter()
        .map(|c| (base_score(e, seat, c, depth, faction, opp_faction, rng), c))
        .collect();
    // The stopping floor (reclaim-churn): when NOTHING constructive remains —
    // every non-EndTurn move scores ≤ 0 — End the turn deterministically rather
    // than softmax-sampling a net-negative move. Without this, the softmax happily
    // samples the least-bad loss (a Reclaim at −10−hold, a forced ugly Glimpse)
    // about as often as it picks EndTurn (−8), so the bot would PLAY a spirit and
    // RECLAIM it the same turn — the AI looked broken to a new player. Holding is
    // strictly better than any negative move (you keep your Anima and your board),
    // so once the constructive plays are exhausted, stopping must dominate at EVERY
    // temperature (a hot Easy bot churned hardest of all). This floors the choice
    // BEFORE the temperature is applied — the mistake-rate knob still shapes which
    // *positive* line gets picked; it just can't manufacture a self-harming one.
    let best_non_end = scored
        .iter()
        .filter(|(_, c)| !matches!(c, Command::EndTurn))
        .map(|(s, _)| *s)
        .fold(f64::MIN, f64::max);
    let can_end = legal.iter().any(|c| matches!(c, Command::EndTurn));
    if can_end && best_non_end <= 0.0 {
        return Command::EndTurn;
    }
    // Softmax sample by temperature. Low temp → almost always the best move;
    // high temp → frequently a worse one (the calibrated "mistake rate").
    let t = temperature;
    let max = scored.iter().map(|(s, _)| *s).fold(f64::MIN, f64::max);
    let weights: Vec<f64> = scored.iter().map(|(s, _)| ((s - max) / t).exp()).collect();
    let total: f64 = weights.iter().sum();
    let mut pick = (rng.next_u64() as f64 / u64::MAX as f64) * total;
    for (i, w) in weights.iter().enumerate() {
        if pick < *w {
            return scored.swap_remove(i).1.clone();
        }
        pick -= w;
    }
    scored.last().unwrap().1.clone()
}

/// Depth-1 greedy score, optionally refined one ply for Expert. Refinement
/// applies the move on a clone and subtracts the opponent's best reply value,
/// so Expert avoids moves that hand the opponent a strong answer.
fn base_score(
    e: &Engine,
    seat: Seat,
    c: &Command,
    depth: u8,
    faction: Faction,
    opp_faction: Faction,
    rng: &mut Rng,
) -> f64 {
    let s = crate::greedy_score_as(e, seat, c, faction) as f64;
    if depth < 2 {
        return s;
    }
    // One-ply lookahead: fork the state via snapshot, apply, value the
    // opponent's best reply. (Engine isn't Clone — it owns the catalog and the
    // entropy stream — so we round-trip through snapshot/from_state.) The
    // opponent is modelled with ITS faction, so an Expert Solace dodges the
    // moves a Lorekeeper would punish, and vice versa.
    //
    // The catalog is shared, not deep-cloned: this fork runs once per legal move
    // (Expert scores every candidate this way), so cloning all 407 cards here is
    // N full-catalog copies per turn. `from_state_shared` + `catalog_arc` make it
    // a refcount bump instead (H3).
    let (snap, pos) = e.snapshot();
    let mut fork = Engine::from_state_shared(snap, 0, pos, e.catalog_arc());
    if fork.apply(seat, c.clone()).is_err() {
        return s;
    }
    let opp = seat.other();
    let opp_legal = fork.legal_commands(opp);
    let opp_best = opp_legal
        .iter()
        .map(|oc| crate::greedy_score_as(&fork, opp, oc, opp_faction))
        .max()
        .unwrap_or(0) as f64;
    let _ = rng;
    // The positional term in the FORKED position, **faction-aware**: each side's win-condition
    // standing, net of the other's. This is the fix for "a smarter Solace optimized the WRONG
    // objective" — it used to credit held ground for BOTH seats, so the Solace was blind to the
    // erasure tally it actually wins on. `positional_objective` reads held ground for a Lorekeeper
    // and held ground PLUS the banked erasure tally for the Solace (see crate::positional_objective),
    // so a banish/unwrite the forked move produced now shows up as positional gain for the Solace.
    let mine = crate::positional_objective(&fork, seat, faction);
    let theirs = crate::positional_objective(&fork, opp, opp_faction);
    let held = (mine - theirs) as f64;
    // Two faction-aware coefficients tune the lookahead's character (the fix for "Expert played WORSE
    // than greedy Hard for the Solace"):
    //
    //   - `pos_coeff` weights my own objective gain. Heavier for the **Solace**: depth is where it
    //     earns its keep — a shallow (Hard) Solace banishes greedily by the depth-1 weights, but a
    //     deeper (Expert) one steers toward the lines that compound its objective (more standing
    //     Unwritten denying tiles, more banked erasures).
    //
    //   - `exposure` weights my FEAR of the opponent's best reply (`opp_best`). Lower for the
    //     **Solace** — and this is the crux of the inversion fix. The Solace plays *attrition*, not
    //     tempo: it WANTS exchanges (its body for a player erasure), so a strong player reply is often
    //     the price of a trade it's happy to make. At the symmetric 0.35 the lookahead read every
    //     contest as risk and steered the Solace into passivity, so its deep play came out WEAKER than
    //     its greedy play. A Lorekeeper, by contrast, is punished correctly by a strong reply (it
    //     hangs a spirit), so it keeps the full exposure weight. Net: a deeper Solace now presses the
    //     board instead of fleeing it — Expert plays meaningfully HARDER than Hard.
    let (pos_coeff, exposure) = match faction {
        Faction::Solace => (2.0, 0.10),
        Faction::Lorekeeper => (0.5, 0.35),
    };
    // A depth-2-ONLY own-presence term for the Solace: the standing value of MY board in the forked
    // position (not netted against the opponent). It re-ranks the lookahead toward lines that keep my
    // wall up, on top of what the greedy score already rewards — and because it lives here, not in the
    // shared greedy scorer, it sharpens the Expert tier WITHOUT touching Hard (depth-1 returns before
    // this). Small coefficient: the depth-1 presence weight ([`crate::SOLACE_PRESENCE_NUM`]) is the
    // heavy lever; this is the fine adjustment that centres the Expert band in both 1v1 and 2v2.
    let own = if matches!(faction, Faction::Solace) {
        4.0 * crate::held_ground(&fork, seat) as f64
    } else {
        0.0
    };
    s + pos_coeff * held + own - exposure * opp_best
}
