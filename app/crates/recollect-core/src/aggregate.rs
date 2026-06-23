//! Ironstate aggregate wiring for `GameState`: the owned turn context, the
//! coarse lifecycle phase machine, and the trait obligations (`EventKind`,
//! `std::error::Error`) that let recollect's engine run on ironstate's aggregate
//! runtime. The rules themselves stay in `engine::decide`/`evolve`.
use std::sync::Arc;

use ironstate::prelude::*;
use ironstate_aggregate::{CtxEntropy, EntropySource, LogicalTime};

use crate::Reject;
use crate::rng::Rng;
use crate::state::Command;
use crate::types::{CardDef, Seat};

/// The match lifecycle: `Live` until the match is Finished, then `Over`
/// (terminal). recollect computes this directly from `GameState` in `phase()` —
/// it never drives the transition itself — and maps the resulting structural
/// `TerminalPhase` rejection back to `Reject::MatchOver`, so the fine-grained
/// rules (Acting / PendingRelease / Finished) and every domain rejection stay
/// exactly as `decide` emits them today.
#[derive(StateMachine, Clone, Debug, PartialEq)]
#[state_machine(initial = Live, terminal = [Over])]
pub enum GameLifecycle {
    Live,
    Over,
}

/// The lifecycle's only transition fact (the match concluded). Present so the
/// machine's terminal state is reachable for analysis; recollect never emits it.
#[derive(Event, Clone, Debug, PartialEq)]
pub enum LifecycleStep {
    Conclude,
}

impl TransitionRules for GameLifecycle {
    type Event = LifecycleStep;
    fn transition(&self, step: &LifecycleStep) -> Option<GameLifecycle> {
        match (self, step) {
            (GameLifecycle::Live, LifecycleStep::Conclude) => Some(GameLifecycle::Over),
            _ => None,
        }
    }
}

/// recollect's `Command` carries no phase-kind restrictions: the coarse `Live`
/// phase accepts every command and `decide` enforces the real rules. Commands
/// carry data, so `#[derive(Event)]` can't generate this — we hand-write it.
impl EventKind for Command {
    fn kinds(&self) -> Option<&'static [Kind]> {
        None
    }
    fn variant_name(&self) -> &'static str {
        match self {
            Command::PlaySpirit { .. } => "PlaySpirit",
            Command::Overwrite { .. } => "Overwrite",
            Command::MoveSpirit { .. } => "MoveSpirit",
            Command::Glimpse => "Glimpse",
            Command::Release { .. } => "Release",
            Command::EndTurn => "EndTurn",
            Command::TellUnwriting { .. } => "TellUnwriting",
            Command::Choose { .. } => "Choose",
            Command::SetOrders { .. } => "SetOrders",
            Command::Reveal { .. } => "Reveal",
            Command::StrikeFabrication { .. } => "StrikeFabrication",
            Command::CastRitual { .. } => "CastRitual",
            Command::AttachBond { .. } => "AttachBond",
            Command::PlaceLandmark { .. } => "PlaceLandmark",
            Command::SetFabrication { .. } => "SetFabrication",
            Command::Evolve { .. } => "Evolve",
            Command::Devolve { .. } => "Devolve",
            Command::BanishStray => "BanishStray",
            Command::Reclaim { .. } => "Reclaim",
            Command::MatchAbandoned { .. } => "MatchAbandoned",
            Command::Mulligan { .. } => "Mulligan",
        }
    }
    fn event_variants() -> Vec<Self> {
        // recollect's commands carry data, so they can't be enumerated as bare
        // variants. `event_variants` feeds ironstate's structural analysis and
        // proptest tooling, which recollect does not run against `Command` (it
        // has its own fuzz + model-check suites); an empty set satisfies the bound.
        Vec::new()
    }
}

// `AggregateRules::Error` must be `std::error::Error`; `Reject` already derives
// `Debug`, so a transparent `Display` completes the obligation.
impl std::fmt::Display for Reject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Reject {}

/// The owned turn context. ironstate's `AggregateRules::Ctx` is a plain
/// associated type and cannot name a borrow's lifetime, so the catalog rides by
/// `Arc` and the live entropy is owned. recollect's entropy is always its own
/// `Rng` (a concrete, `Send` counter-mode stream that reseeks in O(1)), so it's
/// held directly rather than as a `Box<dyn EntropySource>` — keeping the engine
/// `Send` for the async server while still satisfying `CtxEntropy`.
pub struct TurnCtx {
    pub catalog: Arc<Vec<CardDef>>,
    pub entropy: Rng,
    pub actor: Seat,
    pub now: LogicalTime,
    /// Transient recursion guard for Conspiracy's reactive counter-engage: set
    /// only for the duration of a counter so it cannot re-trigger another. Never
    /// journaled and always false between `apply` calls (the engage resets it).
    pub conspiracy_active: bool,
}

impl CtxEntropy for TurnCtx {
    fn entropy_mut(&mut self) -> Option<&mut dyn EntropySource> {
        Some(&mut self.entropy)
    }
}
