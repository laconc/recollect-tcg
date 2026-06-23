//! Shared test aggregate: a tiny entropy-bearing `Tally` (a roll draws a random
//! increment). `GameState` never needs `StableHash`/`AggregateArbitrary` — the
//! journal contract proves the *storage* (generic over A), so this minimal
//! aggregate that draws entropy in `decide` exercises all seven properties and
//! the async loop's entropy discipline.
#![allow(dead_code)] // each integration test uses a different subset

use ironstate::prelude::*;
use ironstate_aggregate::{
    AggregateArbitrary, AggregateRules, EntropySource, LogicalTime, OwnedDeterministicCtx,
    StableHash,
};
use proptest::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(StateMachine, StableHash, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[state_machine(initial = Live, terminal = [Sealed])]
pub enum Phase {
    Live,
    Sealed,
}

#[derive(Event, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PhaseStep {
    Seal,
}

impl TransitionRules for Phase {
    type Event = PhaseStep;
    fn transition(&self, _: &PhaseStep) -> Option<Phase> {
        matches!(self, Phase::Live).then_some(Phase::Sealed)
    }
}

#[derive(Event, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Roll,
    Seal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TallyEvent {
    Rolled(u8),
    Sealed,
}

#[derive(Debug, thiserror::Error)]
#[error("the tally is sealed")]
pub struct SealedError;

#[derive(StableHash, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tally {
    pub phase: Phase,
    pub total: u32,
}

pub fn genesis() -> Tally {
    Tally {
        phase: Phase::Live,
        total: 0,
    }
}

impl AggregateRules for Tally {
    type Phase = Phase;
    type Command = Command;
    type Event = TallyEvent;
    type Error = SealedError;
    type Ctx = OwnedDeterministicCtx<u32>;

    fn phase(&self) -> Phase {
        self.phase.clone()
    }

    fn decide(&self, cmd: &Command, ctx: &mut Self::Ctx) -> Result<Vec<TallyEvent>, SealedError> {
        if self.phase != Phase::Live {
            return Err(SealedError);
        }
        Ok(match cmd {
            // The one draw — replay reproduces it from the recorded position.
            Command::Roll => vec![TallyEvent::Rolled(ctx.entropy.draw_range(1..7) as u8)],
            Command::Seal => vec![TallyEvent::Sealed],
        })
    }

    fn evolve(&mut self, event: &TallyEvent) {
        match event {
            TallyEvent::Rolled(n) => self.total += u32::from(*n),
            TallyEvent::Sealed => self.phase = Phase::Sealed,
        }
    }
}

impl AggregateArbitrary for Tally {
    fn initial_state_strategy() -> BoxedStrategy<Self> {
        Just(genesis()).boxed()
    }
    fn command_strategy(_state: &Self) -> BoxedStrategy<Command> {
        prop_oneof![8 => Just(Command::Roll), 1 => Just(Command::Seal)].boxed()
    }
    fn test_ctx(entropy: Box<dyn EntropySource>, step: u64) -> Self::Ctx {
        OwnedDeterministicCtx {
            entropy,
            actor: 0,
            now: LogicalTime(step),
        }
    }
}
