//! Recollect rules engine, in the Ironstate family shape.
//!
//! Hard rules (enforced by review + CI lints):
//! - No `std::time`, no threads, no I/O, no network.
//! - No floating point anywhere (cross-target determinism).
//! - No `HashMap`/`HashSet` in state. Use `Vec`/`BTreeMap`.
//! - All randomness flows through the journal-owned entropy stream
//!   (`rng::Rng` behind `ironstate_aggregate::EntropySource`), draw-counted so
//!   snapshots resume and replays verify.
//! - Every rule lives in `AggregateRules::decide`; `evolve` is mechanical.
//!   `Engine::apply(seat, command) -> Result<Vec<Event>, Reject>` is the only
//!   way state changes. Same seed + same commands ⇒ same state hash + same
//!   draw count, on every platform, forever.
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]
pub mod aggregate;
pub mod cards;
pub mod effects;
pub mod engine;
pub mod invariants;
pub mod quickplay;
pub mod rng;
pub mod state;
pub mod types;
pub mod view;

pub use engine::{Decided, Engine, Reject};
/// The ironstate-aggregate vocabulary recollect's public surface speaks
/// (`Engine::snapshot`/`from_state` trade in `DrawPos`). The canonical path now
/// that the crates have landed (the old `family_shim` re-export is retired).
pub use ironstate_aggregate::{AggregateRules, DrawPos, EntropySource};
pub use state::{Command, Event, GameState, MatchResult, Phase};
pub use types::{CardDef, CardId, Reach, Resonance, Seat};

#[doc(hidden)]
pub mod test_support {
    use crate::state::{GameState, Spirit};
    use crate::types::{CardId, Seat};
    pub fn put_spirit(st: &mut GameState, tile: u8, card: CardId, owner: Seat) {
        st.board[tile as usize].spirit = Some(Spirit {
            replacement_used: false,
            holding: false,
            face_down: false,
            is_token: false,
            placed_by: None,
            card,
            owner,
            attack: 10,
            defense: 0,
            hp: 40,
            hp_max: 40,
            fading: false,
            banished_by: None,
            intercepted_this_round: false,
            traits_stripped: false,
            traits_stripped_until: None,
            kw_grants: Vec::new(),
            no_engage_until: 0,
            throughline_done: false,
            copied_reach: None,
            fade_deadline: None,
        });
    }
}
