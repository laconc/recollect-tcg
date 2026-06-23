use recollect_bot::{selfplay, selfplay_2v2};
use recollect_core::Seat;
use recollect_core::state::MatchResult;

#[test]
fn a_thousand_random_matches_all_terminate_legally() {
    for seed in 0..1000u64 {
        let (_result, rounds, _steps) = selfplay(seed, seed ^ 0xABCD);
        assert!(
            rounds == 12,
            "the Memory keeps twelve hours; round 12 is Nightfall (got {rounds})"
        );
    }
}

/// 2v2 regression guard. NOT a balance measurement — the formal fairness run
/// (with tripwires) lives in docs/decisions/. This only asserts 2v2 stays
/// *playable*: every greedy self-play reaches a legal result, and across seeds
/// BOTH teams win at least once (no team is structurally locked out — the kind
/// of gross 2v2 break a card change could introduce).
#[test]
fn greedy_2v2_matches_terminate_and_are_not_degenerate() {
    let (mut a_wins, mut b_wins) = (0u32, 0u32);
    for seed in 0..60u64 {
        let (result, steps) = selfplay_2v2(seed);
        assert!(
            steps > 0 && steps < 8000,
            "2v2 seed {seed} did not terminate sanely ({steps} steps)"
        );
        match result {
            MatchResult::Win(Seat::A) => a_wins += 1,
            MatchResult::Win(Seat::B) => b_wins += 1,
            MatchResult::Draw => {}
        }
    }
    assert!(
        a_wins > 0 && b_wins > 0,
        "a 2v2 team is structurally locked out: A {a_wins} / B {b_wins} of 60"
    );
}
