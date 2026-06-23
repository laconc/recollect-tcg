//! One linked binary for the whole crate's fast integration suite.
//!
//! Cargo links a separate test binary for every `.rs` file directly under
//! `tests/`. To cut that link cost on constrained CI runners, every fast-suite
//! integration file lives under `tests/suites/` (a subdirectory Cargo does not
//! auto-discover as its own target) and is pulled in here as a module. The test
//! bodies are byte-for-byte unchanged; they only gain a `suites::<file>::`
//! module path. See `docs/testing.md` → "Test-binary consolidation".
//!
//! The canonical gameplay fuzz / red-team is `suites/fuzz.rs` (the full-catalog playthrough,
//! a consolidated module, NOT its own binary): `make fuzz` / `make soak` run it in release by
//! TEST-NAME filter (the `playthroughs_hold_every_invariant` arms plus `canon_replays_are_bit_identical`
//! and `canon_rejected_commands_leave_no_trace`), so no separate binary is needed — it supersedes
//! the earlier small-catalog gameplay fuzzer (which fuzzed only ~10 test-catalog cards).
//!
//! Deliberately NOT consolidated — kept as their own binaries because a
//! Makefile target invokes each by name with `--test <name>`:
//!   * `golden_replay.rs`→ `make nightly` (pinned behaviour baseline)
//!   * `canon.rs`        → `make catalog` / `make catalog-check` (catalog gate)

/// Shared test helpers (`tests/common/mod.rs`), referenced by the suites as
/// `crate::common::*`. `#![allow(dead_code)]` there keeps a suite that uses only
/// part of the toolkit warning-free.
mod common;

mod suites {
    mod action_economy;
    mod auras;
    mod bond_auras;
    mod card_effects_fire;
    mod carrier_exceptions;
    mod choice_engage_fabrication;
    mod d1_evolution_window;
    mod d4_cluster;
    mod decide_rejects;
    mod deck_themes;
    mod determinism;
    mod devolution;
    mod effects_backlog;
    mod effects_choices;
    mod effects_coverage;
    mod effects_engine;
    mod effects_red_team;
    mod evolution_arrivals;
    mod evolve;
    mod fabrication_traps;
    mod flow_effects;
    mod fuzz; // the full-catalog playthrough — the canonical `make fuzz` / `make soak` gameplay fuzz
    mod journaled_seam;
    mod keywords;
    mod lurk;
    mod m1_backlog;
    mod mulligan;
    mod props;
    mod quickplay;
    mod reach_auras;
    mod recover;
    mod redaction;
    mod redteam_playthrough;
    mod redteam_rules_change;
    mod render_contract;
    mod restrictions;
    mod rule_exceptions;
    mod rules;
    mod security;
    mod solace;
    mod solace_backlog;
    mod solace_deepenings;
    mod solace_effects;
    mod spellbook;
    mod strays;
    mod summon;
    mod target_choice;
    mod terrain_auras;
    mod twovtwo;
    mod twovtwo_backlog;
    mod twovtwo_transport_backlog;
}
