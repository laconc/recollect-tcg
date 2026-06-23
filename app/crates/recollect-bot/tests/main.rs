//! One linked binary for the whole crate's integration suite.
//!
//! Cargo links a separate test binary per `.rs` file directly under `tests/`.
//! Each integration file lives under `tests/suites/` (a subdirectory Cargo does
//! not auto-discover as a target) and is pulled in here as a module — four
//! binaries collapse to one link. The test bodies are byte-for-byte unchanged;
//! they only gain a `suites::<file>::` module path. No Makefile target invokes
//! any of these by `--test <name>`, so all four consolidate. See
//! `docs/testing.md` → "Test-binary consolidation".

mod suites {
    mod agent;
    mod fleet_tripwires;
    mod selfplay;
    mod solace_winnability;
}
