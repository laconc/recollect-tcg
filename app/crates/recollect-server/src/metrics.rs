//! App-level operational metrics, recorded onto the global OTLP meter that
//! `telemetry::init` installs. When no collector is configured the meter is a
//! cheap no-op, so these calls are always safe to make on the hot path.
//!
//! Emitted (all under the `recollect-server` meter, visible in the LGTM stack's
//! Prometheus/Grafana — see docs/operations.md):
//! - `recollect.commands.applied` (counter; attr: `outcome=ok|reject`)
//! - `recollect.command.duration_ms` (histogram; same attr) — apply latency
//! - `recollect.matches.created` (counter; attrs: `mode`, `vs_bot`, `difficulty`, `faction`)
//! - `recollect.matches.finished` (counter; attrs: `result` = outcome, `faction`,
//!   `reason` = `played_out|abandoned`, and the §16 winrate-when-leading pair
//!   `led_at_contraction`, `won`)
//! - `recollect.matches.forfeits` (counter) — one per absence forfeit (a human seat/slot
//!   that disconnected past the grace; the `reason=abandoned` cut of `finished`, as a bare
//!   rate that doesn't need summing across `result`).
//! - `recollect.ws.connections.opened` (counter)
//! - `recollect.ws.reconnections` (counter) — a seat re-subscribed over a
//!   fresh socket while a live one for that principal was still registered (a
//!   supersede), i.e. a first-class reconnect onto an already-occupied seat.
//! - `recollect.ws.frames_rejected` (counter; attr: `reason`) — a wire frame the
//!   transport refused with a typed error (never applied): a bad token, a malformed
//!   frame, an unsupported protocol version, a mode mismatch, or a missing opening
//!   Hello. The cheap flood signal a hostile client (or a broken one) lights up on
//!   the RED board — these returns previously incremented NO metric, so a bad-token
//!   storm was invisible.
//!
//! ## §16 game-design metrics (the deeper cuts the dashboard reads)
//! These light up the `recollect_game_design` Grafana dashboard, whose panels query
//! the Prometheus-form names below (dots→underscores; counters gain `_total`; the
//! histogram exposes `_bucket`):
//! - `recollect.evolutions` → `recollect_evolutions_total{kind=primal|fabled}` —
//!   one per evolution as it resolves (a `SpiritEvolved` event).
//! - `recollect.devolutions` → `recollect_devolutions_total` — one per Devolution
//!   (§5) as it resolves (a `SpiritDevolved` event — a banished form receded to a
//!   base, the rescue). No labels; the dashboard reads the bare rate.
//! - `recollect.throughline_completed` → `recollect_throughline_completed_total` —
//!   one per Throughline completion (a `ThroughlineCompleted` event).
//! - `recollect.match_length_turns` → `recollect_match_length_turns_bucket` — a
//!   histogram of a telling's length in TURNS (count of `TurnEnded` over the match),
//!   recorded once at match end.
//! - the winrate-when-leading-at-contraction signal rides as the two boolean labels
//!   on `recollect.matches.finished`: `led_at_contraction` (a side held a strict
//!   board-score lead at the Dusk contraction) and `won` (that leader went on to win).
//!
//! REDACTION + CARDINALITY: every metric here is an aggregate count or a duration.
//! Labels are seats/enums/booleans only — NEVER a per-match id, a seed, a hand, or
//! any per-player private state. The seed never enters a metric (it never enters an
//! event either); the §16 scan reads only public board facts (who leads, how many
//! turns, that an evolution/Throughline happened), never a card a client couldn't see.
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram};
use recollect_core::state::{Event, GameState, TileState};
use recollect_core::types::{CardId, Seat};
use std::sync::LazyLock;

struct Instruments {
    commands: Counter<u64>,
    command_ms: Histogram<u64>,
    matches: Counter<u64>,
    finished: Counter<u64>,
    forfeits: Counter<u64>,
    connections: Counter<u64>,
    reconnections: Counter<u64>,
    frames_rejected: Counter<u64>,
    // §16 game-design instruments.
    evolutions: Counter<u64>,
    devolutions: Counter<u64>,
    throughline_completed: Counter<u64>,
    match_length_turns: Histogram<u64>,
}

static M: LazyLock<Instruments> = LazyLock::new(|| {
    let meter = opentelemetry::global::meter("recollect-server");
    Instruments {
        commands: meter.u64_counter("recollect.commands.applied").build(),
        command_ms: meter.u64_histogram("recollect.command.duration_ms").build(),
        matches: meter.u64_counter("recollect.matches.created").build(),
        finished: meter.u64_counter("recollect.matches.finished").build(),
        forfeits: meter.u64_counter("recollect.matches.forfeits").build(),
        connections: meter.u64_counter("recollect.ws.connections.opened").build(),
        reconnections: meter.u64_counter("recollect.ws.reconnections").build(),
        frames_rejected: meter.u64_counter("recollect.ws.frames_rejected").build(),
        evolutions: meter.u64_counter("recollect.evolutions").build(),
        devolutions: meter.u64_counter("recollect.devolutions").build(),
        throughline_completed: meter.u64_counter("recollect.throughline_completed").build(),
        match_length_turns: meter.u64_histogram("recollect.match_length_turns").build(),
    }
});

/// A command was applied: count it (by `ok`/`reject` outcome) and record its apply
/// latency in whole milliseconds. Reject rate + latency are the key health signals.
pub(crate) fn command_applied(ok: bool, dur_ms: u64) {
    let attrs = [KeyValue::new("outcome", if ok { "ok" } else { "reject" })];
    M.commands.add(1, &attrs);
    M.command_ms.record(dur_ms, &attrs);
}

/// A new match was created, tagged with its shape so usage breaks down by mode,
/// opponent kind, bot difficulty, and faction.
pub(crate) fn match_created(mode: &str, vs_bot: bool, difficulty: &str, faction: &str) {
    M.matches.add(
        1,
        &[
            KeyValue::new("mode", mode.to_string()),
            KeyValue::new("vs_bot", vs_bot),
            KeyValue::new("difficulty", difficulty.to_string()),
            KeyValue::new("faction", faction.to_string()),
        ],
    );
}

/// How a match reached its result — the low-cardinality `reason` label on
/// `recollect.matches.finished`, so a forfeit is distinguishable from a played-out win.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinishReason {
    /// The telling reached a normal terminal (`MatchEnded` — the Memory faded out, a
    /// resignation-free score result).
    PlayedOut,
    /// A human seat/slot abandoned (disconnected past the grace) — `Event::MatchAbandoned`,
    /// resolved as `Win(present_seat)`. Also bumps the dedicated `recollect.matches.forfeits`
    /// counter so the forfeit rate reads cleanly without summing across `result`.
    Abandoned,
}

impl FinishReason {
    /// The stable label value (`played_out` | `abandoned`).
    fn label(self) -> &'static str {
        match self {
            FinishReason::PlayedOut => "played_out",
            FinishReason::Abandoned => "abandoned",
        }
    }
}

/// A match reached a result. Only the outcome (`Win(A)` / `Win(B)` / `Draw`) is kept —
/// the score is dropped to keep the metric low-cardinality. `lead` carries the §16
/// winrate-when-leading-at-contraction signal: who (if anyone) held a strict board
/// lead at the Dusk contraction, which we correlate with the outcome here. `reason`
/// distinguishes a played-out finish from an absence forfeit.
pub(crate) fn match_finished(
    result: &str,
    faction: &str,
    lead: ContractionLead,
    reason: FinishReason,
) {
    let outcome = result.split_whitespace().next().unwrap_or("unknown");
    M.finished.add(
        1,
        &[
            KeyValue::new("result", outcome.to_string()),
            // The opponent faction, so Solace PvE win-rate is measurable
            // (created carries `faction`; finished matches it).
            KeyValue::new("faction", faction.to_string()),
            // §16: did a side lead at the contraction, and did that side win? Both
            // are low-cardinality booleans (never the seat itself, never a score).
            KeyValue::new("led_at_contraction", lead.had_leader()),
            KeyValue::new("won", lead.led_winner(result)),
            // Forfeit vs played-out — the abandonment signal on the finish counter.
            KeyValue::new("reason", reason.label()),
        ],
    );
    // A forfeit also bumps the dedicated counter (the cheap forfeit-rate read).
    if reason == FinishReason::Abandoned {
        M.forfeits.add(1, &[]);
    }
}

/// A websocket seat connection was opened.
pub(crate) fn connection_opened() {
    M.connections.add(1, &[]);
}

/// A seat reconnected — a fresh socket subscribed to a principal that still
/// had a live sender registered, superseding it. (A first connect onto an empty
/// seat is `connection_opened` only; this fires on the *re*-subscribe, the
/// drop→rejoin path.)
pub(crate) fn reconnection() {
    M.reconnections.add(1, &[]);
}

/// The closed set of reasons the transport refuses a wire frame — the only values
/// the `reason` label on [`frame_rejected`] ever takes, so the cardinality is bounded
/// and the dashboard can break the flood down by cause. Each maps to the typed error
/// frame the client already gets back (the `Error.message` / `Rejected.reason`).
#[derive(Debug, Clone, Copy)]
pub(crate) enum RejectReason {
    /// The opening frame on a fresh socket was not a `Hello` (`ws.rs`).
    HelloFirst,
    /// The presented seat/slot token matched no seat in the match (`ws.rs`).
    BadToken,
    /// A frame that did not parse as a `ClientMsg` (`actor.rs`).
    MalformedMessage,
    /// A `Cmd` whose protocol version is not [`PROTOCOL_VERSION`](recollect_protocol::PROTOCOL_VERSION) (`actor.rs`).
    UnsupportedProtocolVersion,
    /// A principal whose kind doesn't match the match's mode — a seat frame on a 2v2
    /// lobby, or a slot frame on a 1v1 (`actor.rs`).
    ModeMismatch,
}

impl RejectReason {
    /// The stable label value (matches the `Error.message` / `Rejected.reason` string
    /// the client receives, so a board alert ties straight back to the wire frame).
    fn label(self) -> &'static str {
        match self {
            RejectReason::HelloFirst => "hello_first",
            RejectReason::BadToken => "bad_token",
            RejectReason::MalformedMessage => "malformed_message",
            RejectReason::UnsupportedProtocolVersion => "unsupported_protocol_version",
            RejectReason::ModeMismatch => "mode_mismatch",
        }
    }
}

/// A wire frame was refused with a typed error (never applied) — count it by reason.
/// The reject rate by cause is the cheap flood signal: a bad-token / malformed storm
/// is an intrusion or a broken client, and (unlike a rule `Rejected`, which rides the
/// `commands.applied{outcome=reject}` counter) these transport refusals never reach
/// the engine, so without this they were invisible to the RED board.
pub(crate) fn frame_rejected(reason: RejectReason) {
    M.frames_rejected
        .add(1, &[KeyValue::new("reason", reason.label())]);
}

/// §16: one evolution resolved (a `SpiritEvolved`). `fabled` tags the form's tier
/// (Primal vs Fabled) — a low-cardinality enum, so the dashboard's bare
/// `recollect_evolutions_total` still sums across it while the breakdown is there
/// for free.
fn evolution(fabled: bool) {
    M.evolutions.add(
        1,
        &[KeyValue::new(
            "kind",
            if fabled { "fabled" } else { "primal" },
        )],
    );
}

/// §16: one Devolution resolved (a `SpiritDevolved` — a banished form receded to a
/// base, the §5 rescue). No labels — the dashboard reads the bare rate.
fn devolution() {
    M.devolutions.add(1, &[]);
}

/// §16: one Throughline completed (a `ThroughlineCompleted`). No labels — the
/// dashboard reads the bare rate.
fn throughline_completed() {
    M.throughline_completed.add(1, &[]);
}

/// §16: a telling's length in turns, recorded once at match end.
fn match_length_turns(turns: u64) {
    M.match_length_turns.record(turns, &[]);
}

/// Which seat (if any) held a strict board-score lead at the Dusk contraction —
/// the §16 winrate-when-leading-at-contraction signal. `None` means the contraction
/// either hasn't happened (a match that ended before it, or runs without one) or was
/// a tie: in both cases there is no "leader" to correlate, so `led_at_contraction`
/// is false. Carried on the per-match accumulator from the contraction step to match
/// end, where [`match_finished`] folds it against the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ContractionLead {
    /// No contraction yet, or a dead-even board at it — nobody to correlate.
    #[default]
    None,
    /// `seat` held the strict board lead when the Memory contracted.
    Seat(Seat),
}

impl ContractionLead {
    /// `true` iff a definite side led at the contraction (the denominator of the
    /// winrate-when-leading panel).
    fn had_leader(self) -> bool {
        matches!(self, ContractionLead::Seat(_))
    }

    /// `true` iff a side led at the contraction AND that side is the result's
    /// winner (the numerator). A tie/abandon/no-contraction match is never counted
    /// as a "leader won".
    fn led_winner(self, result: &str) -> bool {
        match self {
            ContractionLead::Seat(Seat::A) => result.starts_with("Win(A)"),
            ContractionLead::Seat(Seat::B) => result.starts_with("Win(B)"),
            ContractionLead::None => false,
        }
    }
}

/// The per-match §16 accumulator. The [`Session`](crate::session::Session) owns one
/// per telling and feeds it every applied event batch (human AND bot moves, 1v1 and
/// 2v2, in-memory and journaled — `Session` is the single writer that sees them all).
/// It counts evolutions/Throughlines as they stream, tallies the turn count, and
/// captures the contraction leader; at match end it records the length histogram and
/// hands the captured lead to [`match_finished`].
///
/// Splitting the *classification* (here, pure) from the OTel *emission* (the private
/// helpers above) is what lets the unit tests assert "the right events emit the right
/// metrics" without a live collector.
#[derive(Debug, Default)]
pub(crate) struct MatchMetrics {
    turns: u64,
    lead_at_contraction: ContractionLead,
    finished: bool,
}

impl MatchMetrics {
    pub(crate) fn new() -> MatchMetrics {
        MatchMetrics::default()
    }

    /// The contraction leader captured so far — handed to [`match_finished`] at the
    /// seam when the telling ends (and read by the tests).
    pub(crate) fn contraction_lead(&self) -> ContractionLead {
        self.lead_at_contraction
    }

    /// Test seam: the turn count tallied so far (the length-histogram input).
    #[cfg(test)]
    pub(crate) fn turns(&self) -> u64 {
        self.turns
    }

    /// Observe one applied event batch against the post-batch state. Emits the
    /// streaming counters (evolutions, Throughlines) immediately and, on the batch
    /// that contains the contraction or the finish, captures the lead / records the
    /// length. `is_fabled` resolves a form `CardId` to its tier (Primal/Fabled) — the
    /// caller supplies it from the engine catalog (the only state outside the events).
    pub(crate) fn observe(
        &mut self,
        events: &[Event],
        state: &GameState,
        is_fabled: impl Fn(CardId) -> bool,
    ) {
        for ev in events {
            match ev {
                Event::SpiritEvolved { to, .. } => evolution(is_fabled(*to)),
                Event::SpiritDevolved { .. } => devolution(),
                Event::ThroughlineCompleted { .. } => throughline_completed(),
                Event::TurnEnded { .. } => self.turns += 1,
                Event::MemoryContracted { .. } => {
                    // The board has just contracted; record who leads NOW (the
                    // post-batch state already reflects the fade). A strict lead
                    // only — a tie leaves `None`, so it never inflates either side.
                    self.lead_at_contraction = leader(&state.board, state.solace_erasures);
                }
                // Record the length exactly once, at the terminal event (the guard
                // keeps a defensive second terminal event from double-recording).
                Event::MatchEnded { .. } | Event::MatchAbandoned { .. } if !self.finished => {
                    self.finished = true;
                    match_length_turns(self.turns);
                }
                _ => {}
            }
        }
    }
}

/// Who strictly leads the board by the §16 score model — the same tally
/// `flow::finish` uses for the final score: one point per tile to its standing
/// spirit's owner, else to the most-recent impression, plus the Solace's off-board
/// erasure tally (seat B). A strict lead returns that seat; a tie returns `None`.
/// Reads only public board state (no hands, no deck, no seed).
fn leader(board: &[TileState], solace_erasures: u8) -> ContractionLead {
    let (mut a, mut b) = (0u32, 0u32);
    for t in board.iter() {
        if let Some(sp) = &t.spirit {
            match sp.owner {
                Seat::A => a += 1,
                Seat::B => b += 1,
            }
        } else if let Some(&s) = t.impressions.first() {
            match s {
                Seat::A => a += 1,
                Seat::B => b += 1,
            }
        }
    }
    b = b.saturating_add(solace_erasures as u32);
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => ContractionLead::Seat(Seat::A),
        std::cmp::Ordering::Less => ContractionLead::Seat(Seat::B),
        std::cmp::Ordering::Equal => ContractionLead::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recollect_core::Engine;
    use recollect_core::cards::canon_catalog;
    use recollect_core::state::MatchResult;

    fn impression_tile(owner: Seat) -> TileState {
        TileState {
            spirit: None,
            impressions: vec![owner],
            faded: false,
            terrain: None,
        }
    }

    /// A real opening `GameState` whose board we overwrite — keeps the §16 scan
    /// tests honest (a genuine state shape) without a brittle hand-built literal.
    fn fresh_state() -> GameState {
        let deck: Vec<CardId> = (0..20u16).map(CardId).collect();
        let (engine, _) = Engine::new(1, canon_catalog(), deck.clone(), deck);
        engine.state().clone()
    }

    #[test]
    fn leader_reads_the_score_model_strictly() {
        // Two A impressions vs one B impression ⇒ A leads.
        assert_eq!(
            leader(
                &[
                    impression_tile(Seat::A),
                    impression_tile(Seat::A),
                    impression_tile(Seat::B),
                ],
                0
            ),
            ContractionLead::Seat(Seat::A)
        );
        // A dead-even board is no leader (so it never inflates the panel).
        assert_eq!(
            leader(&[impression_tile(Seat::A), impression_tile(Seat::B)], 0),
            ContractionLead::None
        );
        // The Solace's off-board erasure tally counts toward seat B's lead.
        assert_eq!(
            leader(&[impression_tile(Seat::A)], 3),
            ContractionLead::Seat(Seat::B)
        );
    }

    #[test]
    fn contraction_lead_correlates_with_the_result() {
        // A led at contraction and A won ⇒ counted in both numerator and denominator.
        let a_led = ContractionLead::Seat(Seat::A);
        assert!(a_led.had_leader());
        assert!(a_led.led_winner("Win(A) 5-3"));
        assert!(!a_led.led_winner("Win(B) 3-5"));
        assert!(!a_led.led_winner("Draw 4-4"));
        // No leader (tie/no-contraction) ⇒ never numerator, never denominator.
        let none = ContractionLead::None;
        assert!(!none.had_leader());
        assert!(!none.led_winner("Win(A) 5-3"));
    }

    #[test]
    fn observe_captures_the_contraction_leader_from_the_batch_state() {
        let mut m = MatchMetrics::new();
        // The post-contraction board: B ahead by two impressions.
        let mut st = fresh_state();
        st.board = vec![
            impression_tile(Seat::B),
            impression_tile(Seat::B),
            impression_tile(Seat::A),
        ];
        m.observe(
            &[Event::MemoryContracted {
                faded_tiles: vec![],
            }],
            &st,
            |_| false,
        );
        assert_eq!(m.contraction_lead(), ContractionLead::Seat(Seat::B));
    }

    #[test]
    fn observe_tallies_turns_until_the_terminal_event() {
        let mut m = MatchMetrics::new();
        let st = fresh_state();
        // Three turns elapse, then the match ends.
        m.observe(&[Event::TurnEnded { seat: Seat::A }], &st, |_| false);
        m.observe(&[Event::TurnEnded { seat: Seat::B }], &st, |_| false);
        m.observe(&[Event::TurnEnded { seat: Seat::A }], &st, |_| false);
        assert_eq!(m.turns, 3);
        assert!(!m.finished);
        let ended = Event::MatchEnded {
            result: MatchResult::Win(Seat::A),
            score_a: 1,
            score_b: 0,
        };
        m.observe(std::slice::from_ref(&ended), &st, |_| false);
        assert!(m.finished, "the terminal event records the length once");
        // A second terminal event (defensive) does not double-record.
        m.observe(std::slice::from_ref(&ended), &st, |_| false);
        assert!(m.finished);
    }

    #[test]
    fn observe_classifies_evolution_tiers_and_throughlines() {
        // The streaming counters are no-ops without a collector, but the scan must
        // route each event correctly (and call the tier resolver only for evolutions).
        let mut m = MatchMetrics::new();
        let st = fresh_state();
        let evolution_lookups = std::cell::Cell::new(0);
        m.observe(
            &[
                Event::SpiritEvolved {
                    seat: Seat::A,
                    tile: 0,
                    from: CardId(1),
                    to: CardId(2),
                    attack: 10,
                    defense: 10,
                    hp: 30,
                    keeps_throughline: false,
                },
                Event::ThroughlineCompleted {
                    tile: 0,
                    attack: 10,
                    defense: 10,
                },
                // A Devolution (§5) — counted on the bare `devolutions` rate, and it must
                // NOT consult the evolution tier resolver (it has no Primal/Fabled label).
                Event::SpiritDevolved {
                    seat: Seat::A,
                    tile: 0,
                    from: CardId(2),
                    to: CardId(1),
                    attack: 10,
                    defense: 0,
                    hp: 20,
                },
            ],
            &st,
            |_| {
                evolution_lookups.set(evolution_lookups.get() + 1);
                true
            },
        );
        assert_eq!(
            evolution_lookups.get(),
            1,
            "the tier resolver is consulted once per evolution, never for a Throughline or a Devolution"
        );
    }

    /// The absence-forfeit `reason` label is the stable, low-cardinality pair the
    /// `recollect.matches.finished` counter carries — `played_out` for a normal terminal,
    /// `abandoned` for a disconnect forfeit. (The emission itself is a no-op without a
    /// collector; per the add-a-metric convention we assert the classification, not the
    /// OTel call.)
    #[test]
    fn finish_reason_labels_are_stable() {
        assert_eq!(FinishReason::PlayedOut.label(), "played_out");
        assert_eq!(FinishReason::Abandoned.label(), "abandoned");
    }

    /// A `MatchAbandoned` terminal still records the match-length histogram exactly once,
    /// just like a played-out `MatchEnded` — the forfeit is a real finish, so the §16
    /// length is captured for it too (the accumulator folds both terminal events).
    #[test]
    fn an_abandoned_terminal_records_the_match_length_once() {
        use recollect_core::types::Seat;
        let mut m = MatchMetrics::new();
        let st = fresh_state();
        m.observe(&[Event::TurnEnded { seat: Seat::A }], &st, |_| false);
        assert!(!m.finished);
        let abandoned = Event::MatchAbandoned {
            seat: Seat::A,
            score_a: 0,
            score_b: 0,
        };
        m.observe(std::slice::from_ref(&abandoned), &st, |_| false);
        assert!(m.finished, "an abandonment records the length once");
        // A defensive second terminal does not double-record.
        m.observe(std::slice::from_ref(&abandoned), &st, |_| false);
        assert!(m.finished);
    }
}
