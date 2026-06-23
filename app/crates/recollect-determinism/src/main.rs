//! The wasm32 determinism differential.
//!
//! Runs a fixed, AI-free seeded playout (always the phase-forced move:
//! `EndTurn`, or `Release`/`Choose` when the phase demands it) and prints a hash
//! of the final state for each of several seeds. CI runs this binary **natively**
//! and on **wasm32-wasip1 under `wasmtime`**, then diffs the two outputs — they
//! must be byte-identical. That proves the engine is bit-deterministic across
//! targets (no float drift, no UB, no platform-dependent iteration), the same
//! invariant `make determinism-check` guards for the native build, now extended
//! to wasm.
//!
//! The hashes are NOT pinned to fixed values (the engine evolves) — only the
//! native-vs-wasm *equality* matters, so this stays green as rules change.
#![forbid(unsafe_code)]
use recollect_core::Engine;
use recollect_core::state::{Command, Phase};
use recollect_core::types::CardId;

/// The seeds the differential covers — a spread, including a high (i64-negative)
/// one to exercise the full entropy range.
const SEEDS: [u64; 5] = [1, 7, 42, 0xD00D_1234, 0x0123_4567_89AB_CDEF];

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Drive a deterministic playout to its end (or a generous step cap) and hash the
/// final state + entropy position. Pure integer work — identical on any target.
fn playout_hash(seed: u64) -> u64 {
    let cat = recollect_core::cards::canon_catalog();
    let deck: Vec<CardId> = (0..10u16).chain(0..10u16).map(CardId).collect();
    let (mut engine, _opening) = Engine::new(seed, cat, deck.clone(), deck);
    for _ in 0..2000 {
        let cmd = match engine.state().phase {
            Phase::Finished { .. } => break,
            Phase::PendingRelease { .. } => Command::Release { hand_index: 0 },
            Phase::PendingChoice { .. } => Command::Choose { index: 0 },
            _ => Command::EndTurn,
        };
        let seat = engine.state().active;
        if engine.apply(seat, cmd).is_err() {
            break; // a forced move was somehow illegal — stop; the state still hashes.
        }
    }
    let (state, pos) = engine.snapshot();
    let bytes = postcard::to_allocvec(&state).expect("serialize state");
    fnv1a(&bytes) ^ pos.0
}

fn main() {
    // One line per seed; the CI differential compares this verbatim across targets.
    for seed in SEEDS {
        println!("{seed:016x}:{:016x}", playout_hash(seed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playout_is_deterministic_within_a_target() {
        // The differential only means anything if the playout is stable: same seed
        // ⇒ same hash, every run. (CI then checks native == wasm.)
        for seed in SEEDS {
            assert_eq!(
                playout_hash(seed),
                playout_hash(seed),
                "seed {seed:#x} drifted"
            );
        }
        // And distinct seeds should (overwhelmingly) differ — a sanity check that
        // the hash actually depends on the playout, not a constant.
        assert_ne!(playout_hash(1), playout_hash(7));
    }
}
