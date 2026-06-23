//! Recollect's deterministic entropy source. It implements ironstate's
//! `EntropySource` *contract* (counter-addressable: `at(seed, pos)` is an O(1)
//! seek, so replay/fork/rewind/probe are exact and cheap) but keeps recollect's
//! own splitmix64-at-position stream and modulo-zone rejection — deliberately
//! NOT ironstate's ChaCha reference impl, because every golden/replay/
//! model-check seed in the suite is pinned to this exact draw sequence. The
//! `EntropySource` impl below overrides the derived draws (`draw_range`,
//! `draw_below`, `shuffle_len`) so the contract is satisfied without changing a
//! single produced value.
use serde::{Deserialize, Serialize};

const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

fn mix(z: u64) -> u64 {
    let mut x = z;
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rng {
    seed: u64,
    pos: u64,
}

/// Published name in the family; local alias until delivery.
pub type SeededEntropy = Rng;

impl Rng {
    pub fn from_seed(seed: u64) -> Self {
        Rng::at(seed, 0)
    }

    /// O(1) seek to an absolute draw position.
    pub fn at(seed: u64, pos: u64) -> Self {
        Rng { seed, pos }
    }

    /// Re-seek the live stream (failed-append rewind).
    pub fn seek(&mut self, pos: u64) {
        self.pos = pos;
    }

    pub fn next_u64(&mut self) -> u64 {
        self.pos += 1;
        mix(self.seed.wrapping_add(self.pos.wrapping_mul(GAMMA)))
    }

    /// Unbiased draw in [0, n). n must be > 0.
    pub fn below(&mut self, n: u64) -> u64 {
        debug_assert!(n > 0);
        let zone = u64::MAX - (u64::MAX % n);
        loop {
            let v = self.next_u64();
            if v < zone {
                return v % n;
            }
        }
    }

    /// True with probability num/den. The only probability API: no floats exist.
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }

    pub fn shuffle<T>(&mut self, xs: &mut [T]) {
        for i in (1..xs.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            xs.swap(i, j);
        }
    }

    /// The journal-owned draw counter.
    pub fn draws(&self) -> u64 {
        self.pos
    }

    /// Uncounted fork for speculative legality (`why_not`): draws are real
    /// values but advance no journal-owned counter.
    pub fn probe(&self) -> Rng {
        self.clone()
    }
}

/// recollect speaks ironstate's `EntropySource` contract, but with recollect's
/// own (splitmix64 + modulo-zone) algorithms — the derived draws are OVERRIDDEN
/// so the produced sequence is byte-identical to history. Do not delete the
/// overrides: ironstate's defaults use a different (bit-mask) algorithm and
/// would silently change every seeded outcome.
impl ironstate_aggregate::EntropySource for Rng {
    fn draw_u64(&mut self) -> u64 {
        self.next_u64()
    }
    fn seek(&mut self, pos: ironstate_aggregate::DrawPos) {
        self.pos = pos.0;
    }
    fn draws(&self) -> ironstate_aggregate::DrawPos {
        ironstate_aggregate::DrawPos(self.pos)
    }
    fn probe(&self) -> Box<dyn ironstate_aggregate::EntropySource> {
        Box::new(self.clone())
    }
    fn draw_range(&mut self, range: core::ops::Range<u64>) -> u64 {
        range.start + self.below(range.end - range.start)
    }
    fn draw_below(&mut self, num: u64, den: u64) -> bool {
        self.chance(num, den)
    }
    fn shuffle_len(&mut self, len: usize) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..len).collect();
        for i in (1..len).rev() {
            let j = self.below(i as u64 + 1) as usize;
            indices.swap(i, j);
        }
        indices
    }
}

#[cfg(test)]
mod tests {
    //! Two jobs, because `make test` drives the engine through the **inherent**
    //! `Rng` methods — leaving the `EntropySource` trait overrides
    //! (`draw_range`/`draw_below`/`shuffle_len`) and the inherent `below`/`shuffle`
    //! otherwise unpinned by the fast suite:
    //!   1. **Golden vectors** — for a fixed seed, the precise modulo-zone results.
    //!      These freeze the arithmetic so a mutated `+`/`-`/`<`/`%` is caught, and
    //!      they guard the documented footgun: an accidental override-removal would
    //!      silently swap to ironstate's bit-mask algorithm and change every value.
    //!   2. **The contract properties** — range membership + coverage, valid
    //!      permutation, exact draw accounting, seek round-trip + rewind, and probe
    //!      purity — via ironstate's reusable `assert_entropy_contract`
    //!      (`ironstate-aggregate` 0.1.4); it checks properties, not our bytes.
    use super::*;
    use ironstate_aggregate::{EntropySource, assert_entropy_contract};

    // ---- Golden vectors (the modulo-zone outputs, captured as the truth). A change
    // here means the produced sequence moved — a pinned-seed regression, NOT a
    // free re-baseline. ----

    #[test]
    fn next_u64_golden_sequence() {
        let mut r = Rng::from_seed(42);
        let got: Vec<u64> = (0..5).map(|_| r.next_u64()).collect();
        assert_eq!(
            got,
            vec![
                13679457532755275413,
                2949826092126892291,
                5139283748462763858,
                6349198060258255764,
                701532786141963250,
            ],
            "splitmix64-at-position stream for seed 42 is pinned (mix/GAMMA arithmetic)"
        );
        // The counter advanced exactly once per draw.
        assert_eq!(r.draws(), 5);
    }

    #[test]
    fn below_golden_sequence() {
        let mut r = Rng::from_seed(42);
        let got: Vec<u64> = (0..8).map(|_| r.below(6)).collect();
        assert_eq!(
            got,
            vec![1, 1, 0, 0, 4, 0, 1, 2],
            "modulo-zone below(6) for seed 42 is pinned"
        );
    }

    #[test]
    fn shuffle_golden_permutation_and_draw_count() {
        let mut r = Rng::from_seed(7);
        let mut xs: Vec<usize> = (0..6).collect();
        r.shuffle(&mut xs);
        assert_eq!(
            xs,
            vec![1, 5, 0, 2, 4, 3],
            "Fisher–Yates for seed 7 is pinned"
        );
        assert_eq!(r.draws(), 5, "shuffle of n=6 issues exactly n-1 draws");
    }

    #[test]
    fn shuffle_len_golden_matches_inherent_shuffle() {
        let mut r = Rng::from_seed(7);
        let perm = r.shuffle_len(6);
        assert_eq!(
            perm,
            vec![1, 5, 0, 2, 4, 3],
            "shuffle_len(6) reproduces the inherent shuffle's permutation for seed 7"
        );
        assert_eq!(r.draws(), 5, "shuffle_len(6) issues exactly n-1 draws");
    }

    #[test]
    fn draw_range_golden_sequence() {
        let mut r = Rng::from_seed(99);
        let got: Vec<u64> = (0..6).map(|_| r.draw_range(10..20)).collect();
        assert_eq!(
            got,
            vec![13, 14, 17, 17, 16, 19],
            "draw_range(10..20) = start + below(end-start), pinned for seed 99"
        );
    }

    #[test]
    fn draw_below_golden_sequence() {
        let mut r = Rng::from_seed(99);
        let got: Vec<bool> = (0..10).map(|_| r.draw_below(1, 2)).collect();
        assert_eq!(
            got,
            vec![
                false, true, false, false, true, false, false, false, true, true
            ],
            "draw_below(1,2) = chance(1,2), pinned for seed 99"
        );
    }

    #[test]
    fn chance_golden_sequence() {
        let mut r = Rng::from_seed(5);
        let got: Vec<bool> = (0..10).map(|_| r.chance(1, 3)).collect();
        assert_eq!(
            got,
            vec![
                false, false, false, false, false, false, true, true, false, false
            ],
            "chance(1,3) = below(3) < 1, pinned for seed 5"
        );
    }

    // ---- Contract properties — delegated to ironstate's reusable conformance
    // check (`testkit::assert_entropy_contract`, adopted here). It proves range
    // membership + coverage, a valid permutation, exact draw accounting, seek
    // round-trip + rewind, and probe purity for the `EntropySource` impl — the
    // same properties the hand-rolled tests pinned, now maintained upstream. The
    // golden vectors above stay: they pin recollect's *exact* bytes, which the
    // property contract deliberately does not. ----

    #[test]
    fn rng_obeys_the_entropy_source_contract() {
        assert_entropy_contract(|| Rng::from_seed(0xC0FFEE));
    }
}
