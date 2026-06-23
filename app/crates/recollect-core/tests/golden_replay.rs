//! Behavior-preserving regression corpus — the refactor safety net.
//!
//! `determinism.rs` proves *internal* consistency (same seed + commands ⇒ same
//! state). This proves *external* stability: a fixed seed set, each driven
//! first-legal to completion, with the full event stream + final state folded
//! into a STABLE (FNV-1a, version-independent) fingerprint and pinned to a
//! recorded baseline. A consistent-but-changed behavior — which `determinism.rs`
//! cannot see (both runs change the same way) — trips here.
//!
//! This is the net under the engine.rs module split + giant-function
//! decomposition: those must be byte-for-byte behavior-preserving. If a replay
//! drifts and the change was INTENDED, regenerate the baseline
//! (`RECOLLECT_GOLDEN_PRINT=1 cargo test -p recollect-core --test golden_replay
//! -- --nocapture` prints the new array). If it was NOT intended, you just
//! caught a refactor that changed the rules.
mod common;
use common::*;
use recollect_core::state::Phase;

fn fnv1a_update(h: &mut u64, s: &str) {
    for b in s.bytes() {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x100000001b3);
    }
}

/// Fold a whole first-legal playout (every event, then the final state and the
/// entropy counter) into one stable fingerprint.
fn replay_fingerprint(seed: u64) -> u64 {
    let mut e = new_match(seed);
    let mut h = 0xcbf29ce484222325u64;
    let mut steps = 0;
    while steps < 400 && !matches!(e.state().phase, Phase::Finished { .. }) {
        let seat = e.state().active;
        let Some(cmd) = e.legal_commands(seat).first().cloned() else {
            break;
        };
        for ev in &e.apply(seat, cmd).expect("a first-legal command applies") {
            fnv1a_update(&mut h, &format!("{ev:?}"));
        }
        steps += 1;
    }
    fnv1a_update(&mut h, &serde_json::to_string(e.state()).unwrap());
    fnv1a_update(&mut h, &e.entropy_draws().to_string());
    h
}

const SEEDS: [u64; 16] = [
    1, 2, 3, 5, 7, 11, 13, 17, 42, 99, 100, 256, 1000, 4096, 9999, 123456,
];
// Rebaselined whenever the engine's state shape or event vocabulary changes — the corpus folds the
// full state JSON + every event, so any such change shifts every hash (Lorekeeper play is otherwise
// stable; these replays are Lorekeeper-only). Current shape: impressions `Vec<Seat>` (one per tile,
// last-wins), the Solace's `solace_erasures` tally, per-seat `factions`, the action-economy
// shape (`Phase::Acting` is a unit variant, `moved_this_turn` tracks the free move), and the
// §5 mulligan (`GameState.mulliganed: [bool; 2]` + `Event::Mulliganed`).
//
// §5 Glimpse rebaseline (INTENDED trajectory shift): Glimpse no longer hands a free +1 Anima — it
// glimpses the top card and raises a keep-or-bottom `PendingChoice::Glimpse`. Glimpse still LEADS the
// opening menu, so first-legal now runs Glimpse → `Choose { index: 0 }` (KEEP: the card stays on top,
// no Anima) instead of the old free-anima Glimpse. That is a real change to the first-legal path (a new
// event vocabulary — `GlimpseResolved` + `ChoiceOffered { Glimpse }` — and a different Anima curve),
// so EVERY hash shifts; confirmed deliberate (the mechanic changed, the corpus must follow).
//
// §5 Glimpse BURN-COST rebaseline (INTENDED, the maintainer's final design): activating Glimpse now
// BURNS a chosen hand card first (the activation cost — `PendingChoice::GlimpseBurn` →
// `Event::GlimpseBurned`), THEN raises the keep-or-bottom `Glimpse`. Glimpse still LEADS the opening
// menu, so first-legal now runs Glimpse → `Choose { index: 0 }` (BURN the first hand card) →
// `Choose { index: 0 }` (KEEP the peeked top) — a new event (`GlimpseBurned`), a new pending choice,
// a shrinking hand, and a self-thinning deck. EVERY hash shifts again; confirmed deliberate.
//
// §5 Study→Glimpse rename rebaseline (pure rename, no behavior change): `Command::Study`→
// `Command::Glimpse`, `AnimaReason::Study`→`Glimpse`, `Event::Studied`→`Glimpsed`, and the state
// field `studied_this_turn`→`glimpsed_this_turn`. The fingerprint hashes the state JSON, so the
// renamed field key shifts every hash. No decision logic changed — the workspace + the rest of the
// suite are green; the corpus follows the renamed vocabulary.
//
// Non-deck coverage rebaseline (STATE-SHAPE ONLY, no behavior change): the non-deck dead-effect
// fixes added two state fields that default OFF — `GameState.card_tax: [(u8,u8); 2]` (Ink Runs Dry's
// surcharge) and `Spirit.traits_stripped_until: Option<u8>` (Smear's this-round blank). These
// replays are Lorekeeper-only, so neither field ever leaves its default and no new event fires; the
// fingerprint folds the state JSON, so the added (default-valued) keys shift every hash. Determinism
// + the full suite stay green — pure shape addition, the corpus follows.
const BASELINE: [u64; 16] = [
    17766974010043826505,
    4505508637802398414,
    4036827605898028960,
    18220253478915453693,
    6776741278701059809,
    8892700485800519694,
    6048963724443219679,
    6842692654371198563,
    15755970938960083942,
    2023983995939465069,
    1850972173315776254,
    4285792010721925766,
    9343841489769885830,
    3787272595897514051,
    6343625646090796786,
    14341254273544126788,
];

#[test]
fn replay_corpus_is_behavior_stable() {
    let got: Vec<u64> = SEEDS.iter().map(|&s| replay_fingerprint(s)).collect();
    if std::env::var("RECOLLECT_GOLDEN_PRINT").is_ok() {
        eprintln!("const BASELINE: [u64; 16] = {got:?};");
    }
    for (i, &s) in SEEDS.iter().enumerate() {
        assert_eq!(
            got[i], BASELINE[i],
            "replay drift on seed {s} — engine behavior changed (intended? regenerate baseline)"
        );
    }
}
