//! The rules engine, in the Ironstate family shape: every rule lives in
//! `GameState::decide`; `evolve` is mechanical application of self-sufficient
//! events. `decide` runs the resolution on a clone, applying each event as it
//! is recorded — so decide-time simulation and evolve-time replay agree by
//! construction (tested in tests/determinism.rs).
use crate::aggregate::{GameLifecycle, TurnCtx};
use crate::rng::Rng;
use crate::state::*;
use crate::types::*;
use ironstate_aggregate::{AggregateRules, DrawPos, EntropySource, LogicalTime};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    MatchOver,
    NotYourTurn,
    NothingPending,
    ChoicePending,
    WrongCardKind,
    PendingReleaseFirst,
    NotPendingRelease,
    BadHandIndex,
    NotEnoughAnima,
    /// Held Ground: a lingering rim tile accepts no new writing.
    TileHeld,
    BadTile,
    TileFaded,
    TileOccupied,
    TileEmpty,
    NotYourSpirit,
    SpiritFading,
    NotMobile,
    /// Stand Ground: a this-round restriction forbids this spirit from moving.
    MovementRestricted,
    /// This spirit has already used its one free Move this turn (or arrived this turn —
    /// summoning sickness). Each Mobile spirit moves at most once per turn.
    AlreadyMoved,
    /// Don't Look: this spirit can neither initiate an engage nor intercept.
    EngageRestricted,
    NotAdjacent,
    OutsideProjection,
    FirstPlacementHomeRows,
    TargetNotInReach,
    TargetNotEnemy,
    AlreadyGlimpsedThisTurn,
    /// Glimpse (§5): the activation cost is burning a hand card, so a glimpse with
    /// an EMPTY HAND has nothing to spend. (Both gates also keep Glimpse out of
    /// `legal_commands`; the rejects guard the direct-call path.)
    NothingToBurn,
    /// Glimpse (§5): an EMPTY PAGE has no top card to peek, so there is nothing to
    /// glimpse. (See `NothingToBurn` for the empty-hand twin.)
    NothingToPeek,
    /// Strict evolution pairing: the base-state does not match the chosen form-type.
    /// A Primal form requires a Fading base; a Fabled form requires a healthy base
    /// that did not arrive this turn (summoning-sick bases cannot leap to a Fabled).
    EvolveConditionUnmet,
    /// Devolution (§5): the recede conditions are unmet — the target is not a
    /// standing-Faded FORM you own (a Primal/Fabled banished in combat, still in its
    /// §0.5 window), or the played hand card is not a base in that form's line.
    DevolveConditionUnmet,
    /// Mulligan (§5): outside the opening window — not round 1, not this seat's
    /// turn, the seat has already acted (placed/Glimpsed/spent anima), or it has
    /// already spent its once-per-match mulligan.
    MulliganUnavailable,
}

pub const MAX_HAND: usize = 8;
/// Twelve rounds — the Memory keeps a clock. Round 12 is Nightfall.
pub const LAST_ROUND: u8 = 12;
pub const CONTRACTION_AFTER_ROUND: u8 = 8;
pub const ECHO_NUM: u64 = 1; // 20%: draw_below(1, 5)
pub const ECHO_DEN: u64 = 5;
pub const ECHO_BONUS: i16 = 20;
pub const MOMENTUM_PER_LINK: i16 = 10;
pub const EDGE: i16 = 10;
pub const ARCANE_PIERCE: i16 = 20;

// ---------------------------------------------------------------------------
// evolve: one journaled fact → one mechanical mutation. Never panics on
// well-formed streams; total over the event enum.
// ---------------------------------------------------------------------------
/// The rules engine, implemented as an event-sourced aggregate.
///
/// Two methods carry the whole engine, and the split is the most important
/// thing to understand before changing anything:
///
/// - [`decide`](GameState::decide) is **all the rules**. Given the current
///   state and a `Command`, it validates the move and returns the `Vec<Event>`
///   that *should* happen — or a `Reject`. It mutates nothing real; it reasons
///   on a working clone (see `decide_impl`), `push`ing each event so later
///   steps in the same command see earlier effects.
/// - [`evolve`](GameState::evolve) is **dumb application**. Given one event, it
///   mutates state mechanically. It contains no rules and never rejects — a
///   well-formed event always applies. This is what makes replay exact: folding
///   the journaled events with `evolve` reproduces any state bit-for-bit.
///
/// So: to change a *rule*, edit `decide`/its helpers. To change how a *fact*
/// is recorded, edit `evolve`. Never put a decision in `evolve` — it would not
/// survive replay and would break determinism.
impl GameState {
    /// A choice just resolved: surface the next one a multi-clause play queued
    /// (Dig In, The Long Watch), or fall back to Acting when the queue is empty.
    fn resume_after_choice(&mut self) {
        if !matches!(self.phase, Phase::Acting | Phase::PendingChoice { .. }) {
            return;
        }
        if self.choice_queue.is_empty() {
            self.phase = Phase::Acting;
        } else {
            let next = self.choice_queue.remove(0);
            let seat = next.seat();
            self.pending_choice = Some(next);
            self.phase = Phase::PendingChoice { seat };
        }
    }
}

// ---------------------------------------------------------------------------
// decide: validation + resolution on a clone, recording self-sufficient events.
// ---------------------------------------------------------------------------
/// Append an event to the running list AND apply it to the working state
/// immediately, so later steps in the same `decide` see its effect. Every
/// state change in `decide` goes through here — that's what keeps the event
/// log and the resolved state in lock-step (replay folds the same events).
fn push(sim: &mut GameState, evs: &mut Vec<Event>, ev: Event) {
    sim.evolve(&ev);
    evs.push(ev);
}

/// Resolve a card by id. The **canon** catalog is dense and id-sorted (ids run
/// `0..len`, so `catalog[i].id == CardId(i)`), and this is its hot path — called
/// inside the `combat_stats` aura folds for every standing spirit — so it tries
/// the O(1) index first. Some *test* catalogs are sparse/unsorted (a card pushed
/// with id 98 onto an 18-card base), so a non-matching slot (or an id past the
/// end) falls back to the O(n) scan the engine used before. Correct for every
/// caller; O(1) wherever it matters.
fn card(catalog: &[CardDef], id: CardId) -> &CardDef {
    if let Some(def) = catalog.get(id.0 as usize)
        && def.id == id
    {
        return def;
    }
    catalog
        .iter()
        .find(|c| c.id == id)
        .expect("card id validated at deck load")
}

/// Whether `tile` can receive an arriving/relocating spirit: it must be **unfaded**,
/// **spirit-free**, **terrain-free** (invariant #1), AND **not a Stray's tile** (the wild
/// stands there — invariant 1b). The single predicate every spirit-landing site shares
/// (placement, Move, push, summon, Unwriting shift), so none can drift and admit a spirit
/// on top of terrain or a Stray. (Stepping onto an enemy face-down Fabrication is a
/// *spring*, not an arrival, and is handled before this check at those sites.)
pub(crate) fn tile_open_for_arrival(sim: &GameState, tile: u8) -> bool {
    let t = &sim.board[tile as usize];
    !t.faded
        && t.spirit.is_none()
        && t.terrain.is_none()
        && sim.stray.as_ref().map(|s| s.tile != tile).unwrap_or(true)
}

// ---------------------------------------------------------------------------
// Engine: the Aggregate wrapper + journal-owned entropy (Ironstate shape:
// entropy lives outside the state; snapshots carry the stream position).
// ---------------------------------------------------------------------------
pub struct Engine {
    /// The aggregate state. recollect drives `decide`/`evolve` (the ironstate
    /// `AggregateRules` trait) on it directly; ironstate's `Aggregate` runtime
    /// and journal arrive with the persistent (postgres-authoritative) step.
    state: GameState,
    /// The owned turn context: the live entropy stream (outside aggregate state,
    /// Ironstate — never in state, never in events) and the shared catalog.
    /// `actor`/`now` are refreshed per command in `apply`.
    ctx: TurnCtx,
}

/// A decided-but-not-yet-durable command on the live engine — recollect's
/// in-memory twin of ironstate's `Prepared`. `decide` has already run (the
/// entropy stream advanced) but the events are NOT yet applied. The persistent
/// server holds one of these across its durable journal append, then
/// [`commit`](Decided::commit)s on success or [`abort`](Decided::abort)s on
/// failure — so the events land on disk *before* the in-memory state moves and
/// before the client is told "Applied" (append-before-ack). The borrow of the
/// engine plus `#[must_use]` make "forgot to resolve it" hard to write.
///
/// The in-memory-only counterpart is [`Engine::apply`]; this is its split form,
/// the same discipline `store::execute_async` proves against an ironstate
/// `Aggregate`. recollect's `GameState` rides through `Engine` rather than a raw
/// `Aggregate` (the `state_mut_for_test` seam), so the prepare/commit/abort lives
/// here; the *storage* both paths drive is the same, contract-proven schema.
#[must_use = "a Decided must be committed or aborted, else the engine is left with entropy advanced but state unevolved"]
pub struct Decided<'e> {
    engine: &'e mut Engine,
    events: Vec<Event>,
    /// The entropy position before `decide` — the rewind target on abort.
    before: DrawPos,
}

impl Decided<'_> {
    /// The events to journal, in order.
    pub fn events(&self) -> &[Event] {
        &self.events
    }
    /// The entropy position the draws consumed — persisted *atomically* with the
    /// events: a resume repositions the stream here.
    pub fn draws(&self) -> DrawPos {
        DrawPos(self.engine.ctx.entropy.draws())
    }
    /// Apply the events after the append succeeded, advancing the in-memory engine
    /// to match the durable log. Returns the journaled events.
    pub fn commit(self) -> Vec<Event> {
        for ev in &self.events {
            self.engine.state.evolve(ev);
        }
        self.events
    }
    /// Roll back after the append failed (or to discard): rewind the entropy
    /// stream to the pre-decide position. The state was never evolved, so nothing
    /// is left observable.
    pub fn abort(self) {
        self.engine.ctx.entropy.seek(self.before.0);
    }
}

impl Engine {
    /// Assemble a live engine from a freshly constructed state, its entropy
    /// stream at the post-opening position, and the catalog.
    fn assemble(state: GameState, rng: Rng, catalog: Vec<CardDef>) -> Engine {
        let actor = state.active;
        let now = LogicalTime(state.round as u64);
        Engine {
            state,
            ctx: TurnCtx {
                catalog: Arc::new(catalog),
                entropy: rng,
                actor,
                now,
                conspiracy_active: false,
            },
        }
    }

    pub fn card(&self, id: CardId) -> &CardDef {
        card(&self.ctx.catalog, id)
    }
    pub fn catalog_ref(&self) -> &[CardDef] {
        &self.ctx.catalog
    }
    /// The catalog as a shared handle (a refcount bump, no clone). Hand this to
    /// [`Engine::from_state_shared`] to fork the engine — for the bot's depth-2
    /// lookahead — without deep-cloning all 407 cards per legal move.
    pub fn catalog_arc(&self) -> Arc<Vec<CardDef>> {
        Arc::clone(&self.ctx.catalog)
    }
    /// The effective Warded of the spirit at `tile` (intrinsic OR aura-granted) —
    /// used by the protocol forecast to match `full_exchange`'s defender routing.
    pub fn warded_at(&self, tile: u8) -> bool {
        eff_warded(&self.state, &self.ctx.catalog, tile)
    }
    #[doc(hidden)]
    pub fn echo_suppressed_for_test(&self, tile: u8) -> bool {
        echo_suppressed(&self.state, &self.ctx.catalog, tile)
    }
    /// Borrow the raw engine state. NOTE: this is the unredacted truth — never
    /// send it to a client; clients see only a `PlayerView` (see `view.rs`).
    pub fn state(&self) -> &GameState {
        &self.state
    }
    pub fn entropy_draws(&self) -> u64 {
        self.ctx.entropy.draws()
    }
    /// Snapshot = (state, entropy position). The seed
    /// travels beside the journal, not inside the snapshot.
    pub fn snapshot(&self) -> (GameState, DrawPos) {
        (self.state.clone(), DrawPos(self.ctx.entropy.draws()))
    }
    /// Reconstruct a live engine from a snapshot (state + entropy position).
    /// The inverse of [`Engine::snapshot`]; used by persistence/restore and by
    /// the stateright model-check, which round-trips state through this. Owns the
    /// catalog by value; wraps it in an `Arc` and delegates to
    /// [`Engine::from_state_shared`], so a caller that already holds an
    /// `Arc<Vec<CardDef>>` (the bot's per-move lookahead fork) can hand the
    /// catalog over without deep-cloning all 407 cards. Existing callers are
    /// untouched — the `Vec` they pass is wrapped here exactly as before.
    pub fn from_state(state: GameState, seed: u64, pos: DrawPos, catalog: Vec<CardDef>) -> Engine {
        Engine::from_state_shared(state, seed, pos, Arc::new(catalog))
    }

    /// [`Engine::from_state`] over a **shared** catalog — the additive twin that
    /// takes an `Arc<Vec<CardDef>>` instead of an owned `Vec`. The catalog is
    /// immutable for the engine's life, so a fork can share its parent's `Arc`
    /// (a cheap refcount bump) rather than deep-cloning the whole 407-card table.
    /// The bot's depth-2 lookahead reconstructs an engine once per legal move;
    /// pairing this with [`Engine::catalog_arc`] turns N deep clones of the full
    /// catalog into N refcount bumps.
    pub fn from_state_shared(
        state: GameState,
        seed: u64,
        pos: DrawPos,
        catalog: Arc<Vec<CardDef>>,
    ) -> Engine {
        let actor = state.active;
        let now = LogicalTime(state.round as u64);
        Engine {
            state,
            ctx: TurnCtx {
                catalog,
                entropy: Rng::at(seed, pos.0),
                actor,
                now,
                conspiracy_active: false,
            },
        }
    }

    /// Start a 1v1 match from a seed and two decks. Returns the engine and the
    /// opening events (`MatchStarted`, the first `start_turn`). The seed drives
    /// all entropy (shuffles, Echo, Stray seeding) and appears in NO event or
    /// view — same seed + same commands ⇒ identical state and events.
    pub fn new(
        seed: u64,
        catalog: Vec<CardDef>,
        deck_a: Vec<CardId>,
        deck_b: Vec<CardId>,
    ) -> (Engine, Vec<Event>) {
        Engine::new_with_rules(
            seed,
            catalog,
            deck_a,
            deck_b,
            crate::state::MatchRules::default(),
            Seat::A,
        )
    }

    /// A 2v2 telling — 6×6, four players A1→B1→A2→B2, two teams.
    /// Each player has their own deck/hand/anima; projection and score are
    /// shared per team.
    /// 2v2 entry that opens A1. For a chosen opener use [`Engine::new_2v2_with_opener`].
    pub fn new_2v2(
        seed: u64,
        catalog: Vec<CardDef>,
        decks: [Vec<CardId>; 4], // A1, B1, A2, B2
    ) -> (Engine, Vec<Event>) {
        Self::new_2v2_with_opener(
            seed,
            catalog,
            decks,
            crate::types::SeatSlot::A1,
            [crate::types::Faction::Lorekeeper; 2],
        )
    }

    /// 2v2 entry with a chosen opener: `first_slot` opens, and the A1→B1→A2→B2 cycle rotates to
    /// begin there (still team-alternating). The genesis is otherwise identical.
    pub fn new_2v2_with_opener(
        seed: u64,
        catalog: Vec<CardDef>,
        decks: [Vec<CardId>; 4], // A1, B1, A2, B2
        first_slot: crate::types::SeatSlot,
        factions: [crate::types::Faction; 2], // per-team [A, B]
    ) -> (Engine, Vec<Event>) {
        let mut rng = Rng::from_seed(seed);
        let mut hands: [Vec<CardId>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        let mut left = decks;
        for i in 0..4 {
            rng.shuffle(&mut left[i]);
            hands[i] = left[i].drain(..5.min(left[i].len())).collect();
        }
        let mk = |hand: Vec<CardId>, deck: Vec<CardId>| PlayerState {
            hand,
            deck,
            anima: 0,
            glimpsed_this_turn: false,
            peeked_top: None,
            first_placement_done: false,
        };
        let [d1, d2, d3, d4] = left;
        let [h1, h2, h3, h4] = hands;
        let mut rules = crate::state::MatchRules::default();
        rules.last_round = 10; // 2v2 round count (open tuning item; 9–10)
        rules.factions = factions;
        let mut state = GameState {
            rules,
            contracted: false,
            round: 1,
            active: first_slot.team(),
            phase: Phase::Acting,
            board: vec![TileState::default(); BOARD_TILES_2V2],
            player_a: mk(h1, d1),
            player_b: mk(h2, d2),
            board_w: 6,
            player_a2: Some(mk(h3, d3)),
            player_b2: Some(mk(h4, d4)),
            active_slot: first_slot,
            solace_erasures: 0,
            moved_this_turn: Vec::new(),
            temp_mods: Vec::new(),
            temp_reach: Vec::new(),
            temp_restrict: Vec::new(),
            dissolved: Vec::new(),
            next_ritual_discount: [0, 0],
            card_tax: [(0, 0), (0, 0)],
            dissolved_this_turn: [0, 0],
            pending_choice: None,
            choice_queue: Vec::new(),
            fade_delayed: Vec::new(),
            pending_flow_anima: [0, 0],
            ignore_imprint_this_turn: [false, false],
            next_arrival_atk: [0, 0],
            next_arrival_2nd_engage: [None, None],
            temp_retaliation: Vec::new(),
            peeked_fabs: [Vec::new(), Vec::new()],
            ritual_extra_targets: 0,
            impressions_dormant_round: None,
            silenced_terrain: None,
            calm_tiles: Vec::new(),
            last_played_spirit: [None, None],
            bonds: Vec::new(),
            stray_telegraph: None,
            stray: None,
            stray_match: false,
            unwriting_told_this_round: false,
            mulliganed: [false, false],
        };
        let mut events = Vec::new();
        push(&mut state, &mut events, Event::MatchStarted);
        start_turn(&mut state, &mut events, &catalog);
        (Engine::assemble(state, rng, catalog), events)
    }

    /// Experiment/2v2 entry: same match, different rules.
    pub fn new_with_rules(
        seed: u64,
        catalog: Vec<CardDef>,
        deck_a: Vec<CardId>,
        deck_b: Vec<CardId>,
        rules: crate::state::MatchRules,
        first_player: Seat,
    ) -> (Engine, Vec<Event>) {
        let mut rng = Rng::from_seed(seed);
        let mut da = deck_a;
        let mut db = deck_b;
        rng.shuffle(&mut da);
        rng.shuffle(&mut db);
        let hand_a: Vec<CardId> = da.drain(..5.min(da.len())).collect();
        let hand_b: Vec<CardId> = db.drain(..5.min(db.len())).collect();
        let mk = |hand, deck, anima| PlayerState {
            hand,
            deck,
            anima,
            glimpsed_this_turn: false,
            peeked_top: None,
            first_placement_done: false,
        };
        let mut state = GameState {
            rules,
            contracted: false,
            round: 1,
            active: first_player,
            phase: Phase::Acting,
            board: vec![TileState::default(); BOARD_TILES],
            player_a: mk(hand_a, da, 0),
            player_b: mk(hand_b, db, 0),
            board_w: 5,
            player_a2: None,
            player_b2: None,
            active_slot: crate::types::SeatSlot::A1,
            solace_erasures: 0,
            moved_this_turn: Vec::new(),
            temp_mods: Vec::new(),
            temp_reach: Vec::new(),
            temp_restrict: Vec::new(),
            dissolved: Vec::new(),
            next_ritual_discount: [0, 0],
            card_tax: [(0, 0), (0, 0)],
            dissolved_this_turn: [0, 0],
            pending_choice: None,
            choice_queue: Vec::new(),
            fade_delayed: Vec::new(),
            pending_flow_anima: [0, 0],
            ignore_imprint_this_turn: [false, false],
            next_arrival_atk: [0, 0],
            next_arrival_2nd_engage: [None, None],
            temp_retaliation: Vec::new(),
            peeked_fabs: [Vec::new(), Vec::new()],
            ritual_extra_targets: 0,
            impressions_dormant_round: None,
            silenced_terrain: None,
            calm_tiles: Vec::new(),
            last_played_spirit: [None, None],
            bonds: Vec::new(),
            stray_telegraph: None,
            stray: None,
            stray_match: false,
            unwriting_told_this_round: false,
            mulliganed: [false, false],
        };
        // Seed whether THIS match hosts a Stray (1-in-7), deterministically
        // from the match seed — independent of in-match entropy draws so it
        // can't shift with play. A stray-match also rolls a Midnight variant
        // (10% of stray-matches: a late round-after-Dusk surfacing).
        let stray_roll = Rng::from_seed(seed ^ 0x57A1_0000);
        let mut sr = stray_roll;
        state.stray_match = sr.below(7) == 0;
        if state.stray_match {
            // Telegraph the FIRST surfacing: an inner tile, one round ahead,
            // surfacing on a mid-match round. Midnight (10%) surfaces post-Dusk.
            let midnight = sr.below(10) == 0;
            let surface_round = if midnight {
                state
                    .rules
                    .last_round
                    .saturating_sub(1)
                    .max(state.rules.contraction_after + 1)
            } else {
                // A mid-match surfacing, after the opening but before the Dusk.
                3 + (sr.below(3) as u8) // rounds 3..5
            };
            let tile = INNER_TILES[sr.below(INNER_TILES.len() as u64) as usize];
            state.stray_telegraph = Some(crate::state::StrayTelegraph {
                tile,
                surface_round,
                midnight,
            });
        }
        let mut events = Vec::new();
        push(&mut state, &mut events, Event::MatchStarted);
        start_turn(&mut state, &mut events, &catalog);
        (Engine::assemble(state, rng, catalog), events)
    }

    /// The only way state changes. decide → evolve-all, entropy journaled.
    /// On rejection the live stream is re-seeked: a
    /// failed command leaves nothing observable — no state change, no
    /// position change.
    pub fn apply(&mut self, seat: Seat, cmd: Command) -> Result<Vec<Event>, Reject> {
        let before = self.ctx.entropy.draws();
        self.ctx.actor = seat;
        self.ctx.now = LogicalTime(self.state.round as u64);
        match self.state.decide(&cmd, &mut self.ctx) {
            Ok(evs) => {
                for ev in &evs {
                    self.state.evolve(ev);
                }
                Ok(evs)
            }
            Err(r) => {
                // A rejected command leaves nothing observable: rewind the live
                // stream to the pre-decide position.
                self.ctx.entropy.seek(before);
                Err(r)
            }
        }
    }

    /// The prepare half of the persistent loop: set actor/now, run `decide`
    /// (advancing the entropy stream), and hand back a [`Decided`] WITHOUT
    /// evolving. The caller wedges a durable journal append between this and
    /// [`Decided::commit`]/[`Decided::abort`] — the append-before-ack the
    /// postgres-authoritative server needs. A rejected command rewinds the stream
    /// before returning, exactly like [`apply`](Engine::apply).
    pub fn decide_journaled(&mut self, seat: Seat, cmd: &Command) -> Result<Decided<'_>, Reject> {
        let before = DrawPos(self.ctx.entropy.draws());
        self.ctx.actor = seat;
        self.ctx.now = LogicalTime(self.state.round as u64);
        match self.state.decide(cmd, &mut self.ctx) {
            Ok(events) => Ok(Decided {
                engine: self,
                events,
                before,
            }),
            Err(r) => {
                self.ctx.entropy.seek(before.0);
                Err(r)
            }
        }
    }

    /// Speculative legality (probe entropy: the live stream never moves).
    pub fn why_not(&self, seat: Seat, cmd: &Command) -> Option<Reject> {
        let mut probe_ctx = TurnCtx {
            catalog: self.ctx.catalog.clone(),
            entropy: self.ctx.entropy.probe(),
            actor: seat,
            now: LogicalTime(self.state.round as u64),
            conspiracy_active: false,
        };
        self.state.decide(cmd, &mut probe_ctx).err()
    }

    /// Bounded legal-command enumeration for bots, props, and the ink wash.
    #[doc(hidden)]
    pub fn state_mut_for_test(&mut self) -> &mut GameState {
        &mut self.state
    }
    #[doc(hidden)]
    pub fn apply_event_for_test(&mut self, ev: Event) {
        self.state.evolve(&ev);
    }
    /// Test seam: force the **Fade dissolution** of `seat`'s Fading spirits NOW —
    /// firing their Partings (+ OnAnyBanish, the Unwritten-no-mark rule, etc.), exactly
    /// as the turn-END Fade phase does. Decoupled from turn placement so a test can stage
    /// a Fading spirit and trigger its dissolution in one step. (The Fade phase moved to
    /// turn-END and the Dusk is now instant, so there is no turn-start fade for this to
    /// stand in for; it drives the same `dissolve_faded_at` body the real Fade uses.)
    #[doc(hidden)]
    pub fn force_fade_step_for_test(&mut self, seat: Seat) -> Vec<Event> {
        let mut evs = Vec::new();
        // Build the dissolves on a CLONE, exactly like the real `decide` path, then
        // replay the emitted events onto the live state via `evolve`. `dissolve_faded_at`
        // expresses some removals as events (an Unwritten's `TokenDissolved`, a Lacuna's
        // `SpiritReleased`), which only take effect through `evolve` — so a test seam that
        // ran it on the live state and skipped the replay would diverge from real play.
        let mut sim = self.state.clone();
        sim.active = seat;
        let fading: Vec<(u8, Seat)> = sim
            .board
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                t.spirit.as_ref().and_then(|sp| {
                    (sp.owner == seat && sp.fading)
                        .then_some((i as u8, sp.banished_by.unwrap_or(sp.owner)))
                })
            })
            .collect();
        for (tile, impression) in fading {
            dissolve_faded_at(&mut sim, &mut evs, &self.ctx.catalog, tile, impression);
        }
        self.state.active = seat;
        for ev in &evs {
            self.state.evolve(ev);
        }
        evs
    }

    /// Test seam: fire an Unwriting EVENT card's effect with no board source, as if the
    /// Solace played it from hand — mirrors `fire_arrival_effects_for_test` for events (which
    /// have no tile). Returns the emitted events.
    pub fn fire_unwriting_for_test(&mut self, name: &str, owner: Seat) -> Vec<Event> {
        let mut evs = Vec::new();
        let st = &mut self.state;
        fire_with_engager(
            st,
            &mut evs,
            &self.ctx.catalog,
            name,
            crate::effects::Trigger::OnPlay,
            None,
            owner,
            None,
        );
        evs
    }

    /// Test hook: fire the OnPlay and OnReveal effects of the spirit at `tile`
    /// (as if it just arrived / was revealed), returning the events produced.
    /// Used by the card-effect execution-coverage probe to confirm no authored
    /// instant effect is a silent no-op.
    pub fn fire_arrival_effects_for_test(&mut self, tile: u8, owner: Seat) -> Vec<Event> {
        let mut evs = Vec::new();
        let st = &mut self.state;
        let Some(sp) = st.spirit_at(tile) else {
            return evs;
        };
        let name = card(&self.ctx.catalog, sp.card).name.clone();
        fire_with_engager(
            st,
            &mut evs,
            &self.ctx.catalog,
            &name,
            crate::effects::Trigger::OnPlay,
            Some(tile),
            owner,
            None,
        );
        fire_with_engager(
            st,
            &mut evs,
            &self.ctx.catalog,
            &name,
            crate::effects::Trigger::OnReveal,
            Some(tile),
            owner,
            Some(7),
        );
        evs
    }

    /// Test hook: attempt to push the spirit at `tile` away from `source`,
    /// returning whether it moved. Used by the keyword suite to assert that
    /// Steadfast resists displacement (and that a normal spirit does move).
    pub fn try_push_for_test(&mut self, source: u8, tile: u8, pusher: Seat) -> bool {
        let mut evs = Vec::new();
        let st = &mut self.state;
        push_away(st, &mut evs, &self.ctx.catalog, source, tile, 1, pusher);
        evs.iter().any(|e| matches!(e, Event::SpiritPushed { .. }))
    }

    /// Test hook: resolve a full engage exchange between the spirits at `att_tile`
    /// and `def_tile` (the attacker strikes, the defender retaliates), firing the
    /// real combat triggers — OnEngageResolved and, if the attacker defeats the
    /// defender and still stands, OnDefeat. Lets tests exercise combat-trigger
    /// effects (bond OnDefeat, retaliation riders) without staging an interception.
    pub fn resolve_engage_for_test(&mut self, att_tile: u8, def_tile: u8) -> Vec<Event> {
        let mut evs = Vec::new();
        full_exchange(
            &mut self.state,
            &mut evs,
            &mut self.ctx,
            att_tile,
            def_tile,
            StrikeKind::Engage,
            0,
        );
        evs
    }

    #[doc(hidden)]
    pub fn run_interception_for_test(&mut self, arrival: u8, actor: Seat) -> Vec<Event> {
        let mut evs = Vec::new();
        interception(&mut self.state, &mut evs, &mut self.ctx, arrival, actor);
        evs
    }

    #[doc(hidden)]
    pub fn resolve_chain_for_test(&mut self, att_tile: u8, def_tile: u8, link: u8) -> Vec<Event> {
        let mut evs = Vec::new();
        full_exchange(
            &mut self.state,
            &mut evs,
            &mut self.ctx,
            att_tile,
            def_tile,
            StrikeKind::Chain(link),
            0,
        );
        evs
    }

    /// Every command `seat` may legally play right now — the menu a UI shows
    /// and a bot samples. Enumerates placements (in projection), moves,
    /// evolutions, spellbook plays, reveals, trap-springs, Standing Orders,
    /// Glimpse/Release/EndTurn, and any pending choice's options. Used by clients
    /// and the props/fuzz harness; whatever it returns, `apply` must accept.
    pub fn legal_commands(&self, seat: Seat) -> Vec<Command> {
        let st = &self.state;
        let mut out = Vec::new();
        if matches!(st.phase, Phase::Finished { .. }) || st.active != seat {
            return out;
        }
        // Read the active slot's own hand/anima (2v2 A1≠A2).
        let slot = st.active_slot;
        let w = st.board_w; // width-aware geometry (5 for 1v1, 6 for 2v2)
        let n_tiles = st.board.len() as u8;
        if let Phase::PendingChoice { seat: s, .. } = st.phase {
            if s == seat
                && let Some(pc) = &st.pending_choice
            {
                let n = match pc {
                    crate::state::PendingChoice::Peek { looked, .. } => looked.len(),
                    // Glimpse (§5) step 1 — the BURN: one option per hand card to spend.
                    crate::state::PendingChoice::GlimpseBurn { burnable, .. } => burnable.len(),
                    // Glimpse (§5) step 2 — exactly two: 0 = keep, 1 = bottom for +1.
                    crate::state::PendingChoice::Glimpse { .. } => 2,
                    crate::state::PendingChoice::Target { options, .. } => options.len(),
                    crate::state::PendingChoice::Recover { options, .. } => options.len(),
                };
                for i in 0..n {
                    out.push(Command::Choose { index: i as u8 });
                }
            }
            return out;
        }
        if let Phase::PendingRelease { seat: s, .. } = st.phase {
            if s == seat {
                for i in 0..st.player_slot(slot).hand.len() {
                    out.push(Command::Release {
                        hand_index: i as u8,
                    });
                }
            }
            return out;
        }
        // Every seat — the Solace (B) included — plays normally from its faction
        // deck; the Solace's cards come from its hand like any player's.
        // Glimpse (§5): offered once per turn, and only when both halves are
        // payable — a NON-EMPTY HAND to burn (the activation cost) AND a NON-EMPTY
        // PAGE to peek. An empty hand has nothing to spend; an empty page glimpses
        // nothing.
        let pl = st.player_slot(slot);
        if !pl.glimpsed_this_turn && !pl.hand.is_empty() && !pl.deck.is_empty() {
            out.push(Command::Glimpse);
        }
        out.push(Command::EndTurn);
        // Mulligan (§5 — London-lite): the opening beat. Offered ONLY in the
        // opening window (round 1, this seat untouched, once per seat; 1v1 only —
        // see `mulligan_window`). It IS a player choice, so unlike the system
        // forfeit it belongs in the menu; the window gate keeps it from ever
        // appearing mid-telling or twice.
        if mulligan_window(st, seat) {
            out.push(Command::Mulligan { seat });
        }
        // A surfaced, unveiled Stray may be banished (befriending is
        // positional and happens automatically at turn-start via courtship).
        if st.stray.as_ref().map(|s| !s.veiled).unwrap_or(false) {
            out.push(Command::BanishStray);
        }
        let proj = projection(st, seat, &self.ctx.catalog);
        // Overwrite uses the acting slot's own projection. In 1v1 `projection_slot`
        // early-returns the team `projection` — identical to `proj` (slot.team() == seat
        // for the active slot) — so reuse it instead of recomputing the full board scan.
        let proj_slot_owned = st
            .is_2v2()
            .then(|| projection_slot(st, slot, &self.ctx.catalog));
        let proj_slot: &[bool] = proj_slot_owned.as_deref().unwrap_or(&proj);
        let margin = !st
            .board
            .iter()
            .enumerate()
            // A projected tile only counts as a legal placement if it is also terrain-free
            // (a spirit/Landmark/Fabrication never lands on existing terrain). Matches
            // `any_projected_placement` so the Margin Rule agrees with the deciders.
            .any(|(i, t)| proj[i] && !t.faded && t.spirit.is_none() && t.terrain.is_none());
        let cost_delta = cost_aura(st, seat, &self.ctx.catalog);
        for (hi, cid) in st.player_slot(slot).hand.iter().enumerate() {
            let def = self.card(*cid);
            let eff_cost = (def.cost as i16 + cost_delta).max(0) as u8;
            if st.player_slot(slot).anima < eff_cost {
                continue;
            }
            // Spellbook plays by kind.
            match def.kind {
                CardKind::Ritual => {
                    out.push(Command::CastRitual {
                        hand_index: hi as u8,
                    });
                    continue;
                }
                CardKind::Unwriting => {
                    out.push(Command::TellUnwriting {
                        hand_index: hi as u8,
                    });
                    continue;
                }
                CardKind::Bond => {
                    let mine: Vec<u8> = (0..n_tiles)
                        .filter(|&t| matches!(st.spirit_at(t), Some(sp) if sp.owner == seat && !sp.fading))
                        .collect();
                    for &a in &mine {
                        for &b in &mine {
                            if a < b && manhattan(a, b) == 1 {
                                out.push(Command::AttachBond {
                                    hand_index: hi as u8,
                                    tile_a: a,
                                    tile_b: b,
                                });
                            }
                        }
                    }
                    continue;
                }
                CardKind::Landmark | CardKind::Fabrication => {
                    for tile in 0..n_tiles {
                        let t = &st.board[tile as usize];
                        // A Stray holds its tile (§6) — never offer terrain onto it (the
                        // coexistence guard mirrored from the placement handlers).
                        let stray_here = st.stray.as_ref().map(|s| s.tile == tile).unwrap_or(false);
                        if t.faded || t.spirit.is_some() || t.terrain.is_some() || stray_here {
                            continue;
                        }
                        if !(proj[tile as usize] || margin) {
                            continue;
                        }
                        if def.kind == CardKind::Landmark {
                            out.push(Command::PlaceLandmark {
                                hand_index: hi as u8,
                                tile,
                            });
                        } else {
                            out.push(Command::SetFabrication {
                                hand_index: hi as u8,
                                tile,
                            });
                        }
                    }
                    continue;
                }
                _ => {}
            }
            for tile in 0..n_tiles {
                let t = &st.board[tile as usize];
                // A faded tile (the Curl) OR a calm tile (The Quiet Spreads) rejects placement —
                // `decide_arrival` rejects both as TileFaded, so legal_commands must not offer them.
                if t.faded
                    || st
                        .calm_tiles
                        .iter()
                        .any(|&(ct, r)| ct == tile && st.round >= r)
                {
                    continue;
                }
                // A Stray stands on its tile (it lives in `st.stray`, not `board.spirit`),
                // so that tile is NOT empty: a spirit may not be Played onto it — it is
                // Overwritten (§2). Offer the Overwrite (revealed → fought; veiled → denied
                // entry), gated on the acting slot's own projection, exactly like a spirit
                // Overwrite. Mirrors the `decide_play_spirit`/`decide_overwrite` rejects.
                let stray_here = st.stray.as_ref().map(|s| s.tile == tile).unwrap_or(false);
                if stray_here {
                    if proj_slot[tile as usize] && !(st.contracted && is_rim_w(tile, w)) {
                        out.push(Command::Overwrite {
                            hand_index: hi as u8,
                            tile,
                        });
                    }
                    continue;
                }
                // A spirit is played onto an EMPTY tile — no spirit AND no terrain (a
                // Landmark / revealed Fabrication is not empty). Mirrors the
                // `decide_play_spirit` reject so the offered set never includes a
                // placement the decider would refuse.
                if t.spirit.is_none() && t.terrain.is_none() {
                    if !(proj[tile as usize] || margin) {
                        continue;
                    }
                    if seat == Seat::A
                        && !st.player_a.first_placement_done
                        && !Seat::A.home_rows_w(w).contains(&tile_xy_w(tile, w).1)
                    {
                        continue;
                    }
                    out.push(Command::PlaySpirit {
                        hand_index: hi as u8,
                        tile,
                        engage: None,
                        chain_prefs: Vec::new(),
                    });
                    // A lurker enters face-down and cannot strike on arrival.
                    if !def.lurk {
                        for tgt in targeting_reach(st, &self.ctx.catalog, def.reach, tile, seat, w)
                        {
                            if matches!(st.spirit_at(tgt), Some(e) if e.owner != seat && !e.fading)
                            {
                                out.push(Command::PlaySpirit {
                                    hand_index: hi as u8,
                                    tile,
                                    engage: Some(tgt),
                                    chain_prefs: Vec::new(),
                                });
                            }
                        }
                    }
                } else if proj_slot[tile as usize]
                    && matches!(st.spirit_at(tile), Some(e) if e.owner != seat && !e.fading)
                    && !(st.contracted && is_rim_w(tile, w))
                {
                    out.push(Command::Overwrite {
                        hand_index: hi as u8,
                        tile,
                    });
                }
            }
        }
        for from in 0..n_tiles {
            if let Some(sp) = st.spirit_at(from) {
                if sp.owner == seat
                    && !sp.fading
                    && keyword_active(st, &self.ctx.catalog, from, crate::effects::Keyword::Mobile)
                    && !restricted(st, seat, crate::effects::Restriction::Move)
                    && !st.moved_this_turn.contains(&from)
                // One Move per spirit, never the arrival turn
                {
                    for to in adjacent4_w(from, w) {
                        let t = &st.board[to as usize];
                        // A faded tile, an occupied tile, OR a Stray's tile (the wild stands
                        // there) blocks a Mobile step — mirrors the `decide_move_spirit`
                        // rejects so the menu never offers a step the decider would refuse.
                        if t.faded
                            || t.spirit.is_some()
                            || st.stray.as_ref().map(|s| s.tile == to).unwrap_or(false)
                        {
                            continue;
                        }
                        match &t.terrain {
                            None => out.push(Command::MoveSpirit {
                                from,
                                to,
                                engage: None,
                            }),
                            // Stepping onto an enemy face-down Fabrication
                            // springs the trap (offered so the UI/bot can do it).
                            Some(terr)
                                if terr.kind == crate::state::TerrainKind::Fabrication
                                    && terr.face_down
                                    && terr.owner != seat =>
                            {
                                out.push(Command::MoveSpirit {
                                    from,
                                    to,
                                    engage: None,
                                })
                            }
                            Some(_) => {} // your own terrain / open Landmark blocks
                        }
                    }
                }
                // A face-down lurker may step into the light (and may
                // engage from the reveal — Arrival Law).
                if sp.owner == seat && !sp.fading && sp.face_down {
                    out.push(Command::Reveal {
                        tile: from,
                        engage: None,
                    });
                    // Don't Look: a restricted lurker may reveal but not strike.
                    if sp.no_engage_until < st.round {
                        for tgt in targeting_reach(
                            st,
                            &self.ctx.catalog,
                            self.card(sp.card).reach,
                            from,
                            seat,
                            w,
                        ) {
                            if matches!(st.spirit_at(tgt), Some(e) if e.owner != seat && !e.fading)
                            {
                                out.push(Command::Reveal {
                                    tile: from,
                                    engage: Some(tgt),
                                });
                            }
                        }
                    }
                }
                // Standing Orders: free, for any standing own spirit.
                if sp.owner == seat && !sp.fading {
                    out.push(Command::SetOrders {
                        tile: from,
                        hold: !sp.holding,
                    });
                    // Fade reclaim: voluntarily cash a standing own spirit for Anima.
                    out.push(Command::Reclaim { tile: from });
                }
                // Spring an enemy lie from range (counterplay).
                if sp.owner == seat && !sp.fading && !sp.face_down {
                    for tgt in targeting_reach(
                        st,
                        &self.ctx.catalog,
                        self.card(sp.card).reach,
                        from,
                        seat,
                        w,
                    ) {
                        if let Some(terr) = &st.board[tgt as usize].terrain
                            && terr.kind == crate::state::TerrainKind::Fabrication
                            && terr.face_down
                            && terr.owner != seat
                        {
                            out.push(Command::StrikeFabrication { from, tile: tgt });
                        }
                    }
                }
                // A base evolves by PLAYING a form card from hand onto it, strictly
                // paired by base-state. A Fading base ← its self-fueled Primal (no donor); a
                // HEALTHY base that did NOT arrive this turn (summoning sickness) ← a
                // donor-fueled Fabled. The form must be one you HOLD (a hand card whose
                // `evolves_from` is this base, admitted by the shared-Imprint rule), the
                // discounted cost (`form.cost − ⌊base.cost/2⌋`, cost-aura adjusted) affordable.
                // Both may engage on arrival. We enumerate over the hand, so the same form held
                // twice yields a command per copy (distinct `form_hand`).
                if sp.owner == seat {
                    let healthy_ready = !sp.fading && !st.moved_this_turn.contains(&from);
                    let base = self.card(sp.card);
                    let base_cost = base.cost as i16;
                    // The forms this base may legally become (shared-Imprint rule + carriers).
                    let legal_forms = legal_evolutions(st, &self.ctx.catalog, base, seat);
                    // Eligible Fabled donors: your own non-token spirits ≠ base.
                    let donors: Vec<u8> = (0..n_tiles)
                        .filter(|&t| t != from)
                        .filter(|&t| {
                            matches!(st.spirit_at(t),
                            Some(d) if d.owner == seat && !d.is_token)
                        })
                        .collect();
                    for (hi, &form_id) in st.player_slot(slot).hand.iter().enumerate() {
                        // Only a held form card that is THIS base's legal becoming.
                        if !legal_forms.contains(&form_id) {
                            continue;
                        }
                        let form = self.card(form_id);
                        let is_fabled = form.rarity == "Fabled";
                        // Strict base-state ↔ form-type gate (mirrors decide_evolve).
                        let allowed = if is_fabled { healthy_ready } else { sp.fading };
                        if !allowed {
                            continue;
                        }
                        // Affordability: the discounted, cost-aura-adjusted cost.
                        let eff_cost = (form.cost as i16 - base_cost / 2 + cost_delta).max(0) as u8;
                        if st.player_slot(slot).anima < eff_cost {
                            continue;
                        }
                        let reach =
                            targeting_reach(st, &self.ctx.catalog, form.reach, from, seat, w);
                        let targets: Vec<u8> = reach.iter().copied()
                            .filter(|&t| matches!(st.spirit_at(t), Some(e) if e.owner != seat && !e.fading))
                            .collect();
                        if is_fabled {
                            for &donor in &donors {
                                out.push(Command::Evolve {
                                    tile: from,
                                    form_hand: hi as u8,
                                    fuel: Some(donor),
                                    engage: None,
                                });
                                for &tgt in &targets {
                                    out.push(Command::Evolve {
                                        tile: from,
                                        form_hand: hi as u8,
                                        fuel: Some(donor),
                                        engage: Some(tgt),
                                    });
                                }
                            }
                        } else {
                            out.push(Command::Evolve {
                                tile: from,
                                form_hand: hi as u8,
                                fuel: None,
                                engage: None,
                            });
                            for &tgt in &targets {
                                out.push(Command::Evolve {
                                    tile: from,
                                    form_hand: hi as u8,
                                    fuel: None,
                                    engage: Some(tgt),
                                });
                            }
                        }
                    }
                }
                // Devolution (§5): a standing-Faded FORM you own (a Primal/Fabled
                // banished in combat, still in its §0.5 window — `fading` +
                // `fade_deadline` Some) may RECEDE to a base in its line that you hold.
                // The played card is a hand card whose name is the form's direct
                // `evolves_from` (lines are 2-stage, no chains); cost is ⌊form.cost/2⌋.
                // We enumerate over the hand, so a base held twice yields one command
                // per copy (distinct `base_hand`).
                if sp.owner == seat && sp.fading && sp.fade_deadline.is_some() {
                    let form = self.card(sp.card);
                    if let Some(base_name) = form.evolves_from.as_deref() {
                        let eff_cost = form.cost / 2;
                        if st.player_slot(slot).anima >= eff_cost {
                            for (hi, &cid) in st.player_slot(slot).hand.iter().enumerate() {
                                if self.card(cid).name == base_name {
                                    out.push(Command::Devolve {
                                        tile: from,
                                        base_hand: hi as u8,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// The effect executor. Effects draw NO entropy — determinism suites cover them
// by construction.

mod combat;
pub use combat::*;

mod projection;
pub use projection::*;
mod decide;
pub(crate) use decide::*;
mod flow;
pub(crate) use flow::*;
mod effects_exec;
pub(crate) use effects_exec::*;
mod conditions;
pub use conditions::*;
mod combat_stats;
pub use combat_stats::*;
mod clause;
pub use clause::*;
mod evolve;

mod throughline;
pub(crate) use throughline::*;
mod strays;
pub(crate) use strays::*;
mod aura_helpers;
pub(crate) use aura_helpers::*;
mod choice_effects;
pub(crate) use choice_effects::*;
mod effects_fire;
pub(crate) use effects_fire::*;
mod effects_phases;
pub(crate) use effects_phases::*;

mod decide_spellbook;
pub(crate) use decide_spellbook::*;

mod decide_arrival;
pub(crate) use decide_arrival::*;
