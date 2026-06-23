//! The `solace-modelcheck` binary: the full bounded model-check frontier, run for
//! the Solace PvE, the 1v1, AND the 2v2 state spaces over the real recollect-core
//! aggregate (no re-modeling — any invariant break is a true engine bug). Reuses
//! `recollect_verify::model::EngineModel`; `tests/solace_bridge.rs` is the fast CI
//! slice. See architecture.md (verification routing) + docs/testing.md.
use recollect_core::cards::canon_catalog;
use recollect_verify::model::{EngineModel, Mode};

fn main() {
    let catalog = canon_catalog();
    // Small decks (cheap spirits) so the branching factor stays exhaustively
    // explorable; the rules are the real engine's regardless of deck.
    let lorekeeper = EngineModel::cheap_deck(&catalog, 4, false);
    assert!(
        lorekeeper.len() >= 4,
        "need a few cheap spirits to seed the player deck"
    );

    for (label, solace) in [("Lorekeeper vs Solace", true), ("1v1 duel", false)] {
        println!("=== {label} stateright bridge (BFS, bounded frontier) ===");
        let model = EngineModel {
            catalog: catalog.clone(),
            deck_a: lorekeeper.clone(),
            deck_b: EngineModel::cheap_deck(&catalog, 4, solace),
            seed: 12345,
            max_round: 2, // tight bound: keeps the frontier exhaustively checkable
            mode: Mode::OneVsOne,
            init_override: None,
        };
        // A definitive result over a bounded frontier — the honest model-checking
        // claim (not "all states forever"): explore up to N reachable states (BFS),
        // checking every invariant on each.
        let n = model.run(20_000);
        println!("  explored {n} unique reachable states; all invariants hold.");
    }

    // The 2v2 four-slot telling. The 6×6 board × four hands branches HARD, so the
    // bound is tighter (3-card decks, round ≤ 2) — a shallow but exhaustive frontier on
    // which redaction (all four slots via `view_for_slot`), liveness, determinism,
    // no-seed-leak, validity, and abandonment all hold over the 2v2 path.
    println!("=== 2v2 team stateright bridge (BFS, tight bound) ===");
    let model = EngineModel {
        catalog: catalog.clone(),
        deck_a: EngineModel::cheap_deck(&catalog, 3, false),
        deck_b: EngineModel::cheap_deck(&catalog, 3, false),
        seed: 12345,
        max_round: 2,
        mode: Mode::TwoVsTwo,
        init_override: None,
    };
    let n = model.run(20_000);
    println!("  explored {n} unique reachable 2v2 states; all invariants hold.");
}
