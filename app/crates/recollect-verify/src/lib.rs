//! Verification crate: the stateright bridge over the real recollect-core
//! aggregate. The reusable `model::EngineModel` checks both the Solace PvE and
//! the 1v1 state spaces. The `solace-modelcheck` binary runs the full bounded
//! frontier for both modes; `tests/solace_bridge.rs` runs a smaller frontier on
//! every test run as a fast CI gate.
#![deny(rustdoc::broken_intra_doc_links)]
pub mod model;
