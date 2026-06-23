//! Fairness tripwires — the fairness evidence as automated balance gates. The `fleet`
//! bin prints the precise N=300 report; these are fast, DETERMINISTIC (fixed-seed)
//! alarms that fail if a balance metric drifts past a fair band. A small inherent
//! first-mover edge is fine; a severe imbalance (or a stalled board) trips them.
use recollect_bot::evidence::{fairness_1v1, fairness_2v2, quickplay_spirit_fraction};
use recollect_bot::{selfplay, selfplay_2v2};
use recollect_core::cards::canon_catalog;
use recollect_core::state::MatchResult;

// Modest, fixed N: deterministic + fast. The band is a regression alarm, not a tight
// fairness proof — widen the bin's N=300 read for precision.
const N: u64 = 32;

// The 1v1 anchor samples MORE seeds (the §5 Glimpse burn-cost made the metric
// variance-sensitive at the low end: the first ~32 seeds skew toward B even though
// the balance converges fair — N=100 reads A=44%, N=200 reads A≈48.5%). A coarse
// 32-seed read here is a noisy alarm (±14pp Wilson half-width); 100 seeds is a
// trustworthy gate that still runs in a few seconds. 2v2 stays at the shared N (its
// metric did not shift). See `bin/fleet` for the precise N=300 evidence read.
const N_1V1: u64 = 100;

#[test]
fn two_v_two_first_team_edge_stays_in_a_fair_band() {
    let cat = canon_catalog();
    let r = fairness_2v2(&cat, 10, N);
    assert!(
        (0.30..=0.70).contains(&r.a_pct()),
        "2v2 first-team win share {:.1}% outside the fair band (first edge {:+.1}pp, n={})",
        r.a_pct() * 100.0,
        r.first_edge_pp(),
        r.n(),
    );
    assert!(
        r.draw_pct() < 0.50,
        "2v2 draws dominate ({:.1}%) — a stalled-board regression",
        r.draw_pct() * 100.0,
    );
}

#[test]
fn one_v_one_is_a_fair_anchor() {
    let cat = canon_catalog();
    let r = fairness_1v1(&cat, 12, N_1V1);
    assert!(
        (0.30..=0.70).contains(&r.a_pct()),
        "1v1 A win share {:.1}% outside the fair band (n={})",
        r.a_pct() * 100.0,
        r.n(),
    );
}

#[test]
fn quickplay_decks_are_spirit_led_not_spell_flooded() {
    let cat = canon_catalog();
    let frac = quickplay_spirit_fraction(&cat, 16);
    assert!(
        (0.30..=0.95).contains(&frac),
        "Quick-Play spirit fraction {:.1}% — texture drifted (spell-flooded, or all-spirit)",
        frac * 100.0,
    );
}

// --- Decisiveness guard -----------------------------------------------------------
//
// The game must DECIDE, not stall to a draw. Glimpse's burn cost and evolution-from-
// hand make hoarding cards/Anima under the burn a real temptation — a too-passive bot
// can drift toward a 0-0 board at R12. The held-ground / presence terms in the eval
// (`tile_hold_value`, `SOLACE_PRESENCE_NUM`, the depth-2 `positional_objective`) are
// the counterweight that makes the bot BUILD and HOLD a board instead of churning it
// away; this gate locks that decisiveness in so an eval change can't silently slide
// the mirror back toward the stalled-board 0-0 regime. Measured A/B: with the
// presence terms the std-deck Expert mirror draws ~3% (0% at N=100); zeroing them
// roughly doubles it — and a genuine break (both sides passive) spikes far higher (the
// design's §S-6 double-wall lab case hit a 31% draw mass). See
// `docs/decisions/recollect_balance_decisive_retune.md` for the full diagnosis + A/B.

// Decisiveness sample. Larger than the fairness N (a draw rate is a low-frequency event,
// so it needs more matches to read trustworthily) but still a few seconds at Expert.
const N_DECISIVE: u64 = 200;

/// Draw share of an `selfplay`-style mirror over `0..n` fixed seeds.
fn draw_share(n: u64, mut play: impl FnMut(u64) -> MatchResult) -> (f64, u64) {
    let draws = (0..n)
        .filter(|&seed| matches!(play(seed), MatchResult::Draw))
        .count() as u64;
    (draws as f64 / n as f64, draws)
}

#[test]
fn bot_v_bot_1v1_is_decisive_not_a_stalemate() {
    // The std-deck Expert mirror — the `selfplay` path most at risk of a
    // "0-0 at R12" stall. It must resolve decisively the vast majority of the time. The bound
    // (≤22%) sits above the measured rate yet far below a real stall (a passive-both-sides
    // regression spikes to 30%+), so it stays a true alarm.
    //
    // Re-tune note (the §0.5 round-12 refinement): a spirit **banished on round 12** now
    // **lingers standing-Faded** through the rest of the round and dissolves in the
    // Nightfall `finish` (laying the banisher's impression) instead of vanishing on
    // defeat. On the low-stat std *test* catalog (no evolution lines, so no upside to the
    // window — only the lingering body) this resolves more last-round positions to the
    // standing-Faded-then-impression outcome rather than a final-turn swing, lifting the
    // mirror's draw share from ~3% to ~16%. That is the RULE's price, not a passivity
    // regression, so the bound moved with it (still well clear of the 30%+ stall alarm).
    let (rate, draws) = draw_share(N_DECISIVE, |seed| selfplay(seed, seed ^ 0xABCD).0);
    assert!(
        rate <= 0.22,
        "1v1 self-play stalled: {draws}/{N_DECISIVE} draws ({:.1}%) — the board is not being \
         pressed (the 0-0 passivity regression; see the decisive re-tune note)",
        rate * 100.0,
    );
}

#[test]
fn bot_v_bot_2v2_is_decisive_not_a_stalemate() {
    // The 6×6 2v2 std-deck mirror. The wider board + longer clock draw a little more
    // often than 1v1 (~9% measured), so the bound is looser (≤20%) — still a decisive
    // majority, still a clear alarm against a stalled-board regression.
    let (rate, draws) = draw_share(N_DECISIVE, |seed| selfplay_2v2(seed).0);
    assert!(
        rate <= 0.20,
        "2v2 self-play stalled: {draws}/{N_DECISIVE} draws ({:.1}%) — the board is not being \
         pressed (the 0-0 passivity regression; see the decisive re-tune note)",
        rate * 100.0,
    );
}

#[test]
fn quickplay_1v1_mirror_is_decisive() {
    // The Quick-Play (generated-deck) 1v1 mirror at the standard R12 clock, via the same
    // evidence playout the fairness anchor uses. Decks carry real interaction, so this
    // resolves even more reliably (~5% draws); the ≤18% bound guards the stalled-board
    // regression on the *played* decks, complementing the std-deck guard above.
    let cat = canon_catalog();
    let r = fairness_1v1(&cat, 12, N_DECISIVE);
    assert!(
        r.draw_pct() <= 0.18,
        "Quick-Play 1v1 mirror stalled: draws {:.1}% (n={}) — the board is not being pressed",
        r.draw_pct() * 100.0,
        r.n(),
    );
}
