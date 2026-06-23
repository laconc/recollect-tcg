//! Solace remainders. The Solace PvE state space is guarded by the stateright
//! bridge in the `recollect-verify` crate: an exhaustive BFS (the
//! `solace-modelcheck` bin explores 15,874 states clean, with a 3,000-state CI
//! gate on every test run), so round-coupled Solace spawning is covered there.
