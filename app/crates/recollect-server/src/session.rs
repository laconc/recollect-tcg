//! A match session: the single writer for one game's state. All command
//! validation happens here through the same `recollect-core` the clients
//! embed — the server never trusts a client's view of legality.
// A couple of slot/view accessors are kept for symmetry and tests but aren't
// all reached by the server binary yet; allow the residue.
#![allow(dead_code)]

use crate::actor::Principal;
use recollect_core::cards::canon_catalog;
use recollect_core::state::{Command, GameState};
use recollect_core::types::Seat;
use recollect_core::view::{PlayerView, TeamView, view_for, view_for_slot};
use recollect_core::{Engine, Reject};
use recollect_journal_postgres::store::AsyncStore;

pub struct Session {
    engine: Engine,
    /// Last applied client sequence number per SLOT (A1,B1,A2,B2). 1v1 uses
    /// the first two; 2v2 uses all four.
    last_seq: [u64; 4],
    /// §16 game-design telemetry for THIS match. Every applied batch (player and
    /// bot, 1v1 and 2v2) flows through `finish_apply` — the one post-apply funnel — so
    /// this accumulator sees them all: evolutions, Throughlines, the turn count, and the
    /// contraction leader. Aggregate counts only; it never touches a hand, the deck, or
    /// the seed (redaction holds). See `crate::metrics`.
    metrics: crate::metrics::MatchMetrics,
}

#[derive(Debug)]
pub struct ApplyOk {
    pub your_view: PlayerView,
    pub other_view: PlayerView,
    pub other_seat: Seat,
    /// For the journal record: this command's event batch + entropy position.
    pub events: Vec<recollect_core::Event>,
    pub draws_after: u64,
    /// Set when this command ended the match: a result string for the
    /// match row, so the server can close the record and evict the session.
    pub finished: Option<String>,
}

/// Result of a 2v2 slot-applied command — fresh views for all four
/// slots to fan out over their sockets.
#[derive(Debug)]
pub struct ApplyOkSlot {
    pub views: [(recollect_core::types::SeatSlot, TeamView); 4],
    pub events: Vec<recollect_core::Event>,
    pub draws_after: u64,
    pub finished: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ApplyErr {
    StaleOrReplayedSeq,
    Rule(Reject),
    /// The durable append failed; the command was NOT acked and the engine was
    /// rewound (nothing observable changed). The string is the journal error.
    Journal(String),
}

/// The result of an applied command, keyed by the acting [`Principal`]'s
/// KIND — the one place the 1v1/2v2 apply funnel branches (H4). A 1v1 seat yields a
/// [`Solo`](ApplyOutcome::Solo) (the acting seat's view + the single opponent's); a 2v2 slot yields
/// a [`Team`](ApplyOutcome::Team) (fresh `TeamView`s for all four slots to fan out). The seq-dedup,
/// the journaled-vs-in-memory dispatch, and the §16 metrics funnel are identical across
/// modes and shared verbatim; only the view-fan differs, here.
///
/// Both arms are boxed: each result carries several full `PlayerView`/`TeamView`s
/// (hundreds of bytes), so boxing keeps `ApplyOutcome` itself pointer-sized — it is
/// passed and returned all along the apply funnel (no `large_enum_variant`, no fat
/// move on the happy path).
#[derive(Debug)]
pub enum ApplyOutcome {
    Solo(Box<ApplyOk>),
    Team(Box<ApplyOkSlot>),
}

impl ApplyOutcome {
    /// The result string when this command ended the match (for the match record +
    /// the actor's finish handling) — mode-agnostic.
    pub fn finished(&self) -> Option<&String> {
        match self {
            ApplyOutcome::Solo(ok) => ok.finished.as_ref(),
            ApplyOutcome::Team(ok) => ok.finished.as_ref(),
        }
    }

    /// Test seam: the 1v1 `ApplyOk` (panics if this was a 2v2 `Team` result).
    #[cfg(test)]
    pub fn solo(self) -> ApplyOk {
        match self {
            ApplyOutcome::Solo(ok) => *ok,
            ApplyOutcome::Team(_) => panic!("expected a 1v1 Solo outcome, got a 2v2 Team"),
        }
    }

    /// Test seam: the 2v2 `ApplyOkSlot` (panics if this was a 1v1 `Solo` result).
    #[cfg(test)]
    pub fn team(self) -> ApplyOkSlot {
        match self {
            ApplyOutcome::Team(ok) => *ok,
            ApplyOutcome::Solo(_) => panic!("expected a 2v2 Team outcome, got a 1v1 Solo"),
        }
    }
}

impl Session {
    pub fn new(
        seed: u64,
        deck_a: Vec<recollect_core::CardId>,
        deck_b: Vec<recollect_core::CardId>,
    ) -> Session {
        let (engine, _events) = Engine::new(seed, canon_catalog(), deck_a, deck_b);
        Session {
            engine,
            last_seq: [0; 4],
            metrics: crate::metrics::MatchMetrics::new(),
        }
    }

    /// First-player: a 1v1/PvE session with a chosen opener (the seeded toss). Real matches
    /// flip via [`recollect_core::quickplay::decide_opener`]; tests use [`Session::new`] (A opens).
    pub fn new_with_opener(
        seed: u64,
        deck_a: Vec<recollect_core::CardId>,
        deck_b: Vec<recollect_core::CardId>,
        first_player: Seat,
        factions: [recollect_core::types::Faction; 2],
    ) -> Session {
        let rules = recollect_core::state::MatchRules {
            factions,
            ..Default::default()
        };
        let (engine, _events) =
            Engine::new_with_rules(seed, canon_catalog(), deck_a, deck_b, rules, first_player);
        Session {
            engine,
            last_seq: [0; 4],
            metrics: crate::metrics::MatchMetrics::new(),
        }
    }

    /// A 2v2 match — four decks, four slots A1→B1→A2→B2.
    pub fn new_2v2(seed: u64, decks: [Vec<recollect_core::CardId>; 4]) -> Session {
        let (engine, _events) = Engine::new_2v2(seed, canon_catalog(), decks);
        Session {
            engine,
            last_seq: [0; 4],
            metrics: crate::metrics::MatchMetrics::new(),
        }
    }

    /// 2v2 with a chosen opener: `first_slot` opens, the A1→B1→A2→B2 cycle rotating from there.
    pub fn new_2v2_with_opener(
        seed: u64,
        decks: [Vec<recollect_core::CardId>; 4],
        first_slot: recollect_core::types::SeatSlot,
        factions: [recollect_core::types::Faction; 2],
    ) -> Session {
        let (engine, _events) =
            Engine::new_2v2_with_opener(seed, canon_catalog(), decks, first_slot, factions);
        Session {
            engine,
            last_seq: [0; 4],
            metrics: crate::metrics::MatchMetrics::new(),
        }
    }

    pub fn view(&self, seat: Seat) -> PlayerView {
        view_for(&self.engine, seat)
    }

    /// The four-seat view for a physical slot.
    pub fn view_slot(
        &self,
        slot: recollect_core::types::SeatSlot,
    ) -> recollect_core::view::TeamView {
        recollect_core::view::view_for_slot(&self.engine, slot)
    }

    /// The slot turn-guard (2v2 only): a slot must be the engine's *active_slot*, not
    /// merely on the active team — else A2 could act on A1's turn (both are team A, so
    /// `engine.apply` keyed on the team alone would accept it). A 1v1 seat has no such
    /// guard; the engine gates its turn. `who.slot()` is `None` for a seat, so this is
    /// a no-op there — the one place the turn-check branches on principal-kind.
    fn slot_turn_ok(&self, who: Principal) -> Result<(), ApplyErr> {
        match who.slot() {
            Some(slot) if slot != self.engine.state().active_slot => {
                Err(ApplyErr::Rule(Reject::NotYourTurn))
            }
            _ => Ok(()),
        }
    }

    /// **The one in-memory apply funnel (H4).** Apply a client command from any
    /// [`Principal`] — a 1v1 seat or a 2v2 slot. The slot turn-guard, the seq-dedup,
    /// the engine apply (keyed on the acting seat), and the §16 metrics are identical
    /// across modes and run here verbatim; the result's view-fan is the sole per-kind
    /// branch (`finish_apply` → [`Solo`](ApplyOutcome::Solo)/[`Team`](ApplyOutcome::Team)).
    /// The four old `apply`/`apply_slot` twins collapsed into this.
    pub fn apply(
        &mut self,
        who: Principal,
        seq: u64,
        cmd: Command,
    ) -> Result<ApplyOutcome, ApplyErr> {
        self.slot_turn_ok(who)?;
        let idx = who.idx();
        if seq <= self.last_seq[idx] {
            return Err(ApplyErr::StaleOrReplayedSeq);
        }
        match self.engine.apply(who.acting_seat(), cmd) {
            Ok(events) => {
                self.last_seq[idx] = seq;
                let draws = self.engine.entropy_draws();
                Ok(self.finish_apply(who, events, draws))
            }
            Err(r) => Err(ApplyErr::Rule(r)),
        }
    }

    /// **The one postgres-authoritative apply funnel (H4).** decide → **append
    /// (durable)** → evolve, for any [`Principal`]. The command lands on disk before
    /// the in-memory state moves and before the caller can ack it (append-before-ack);
    /// on an append failure the engine is rewound and the command is NOT acked —
    /// nothing observable changed. Same legality, slot turn-guard, sequence guard, and
    /// view-fan as [`apply`](Self::apply); only the durability boundary differs. This
    /// owns the one `.await` (the append) and nothing else. The four old
    /// `apply_journaled`/`apply_slot_journaled` twins collapsed into this.
    pub async fn apply_journaled(
        &mut self,
        store: &AsyncStore<'_, GameState>,
        who: Principal,
        seq: u64,
        cmd: Command,
    ) -> Result<ApplyOutcome, ApplyErr> {
        self.slot_turn_ok(who)?;
        let idx = who.idx();
        if seq <= self.last_seq[idx] {
            return Err(ApplyErr::StaleOrReplayedSeq);
        }
        let decided = self
            .engine
            .decide_journaled(who.acting_seat(), &cmd)
            .map_err(ApplyErr::Rule)?;
        let draws = decided.draws();
        match store.append(decided.events(), draws).await {
            Ok(_seq) => {
                let events = decided.commit();
                self.last_seq[idx] = seq;
                Ok(self.finish_apply(who, events, draws.0))
            }
            Err(e) => {
                decided.abort();
                Err(ApplyErr::Journal(e.to_string()))
            }
        }
    }

    // --- Server-driven principals (the AI opponent) -------------------------
    // A bot seat/slot has no client and no client sequence, so the session mints the
    // next seq itself (one past the last) and routes through the same guarded funnel
    // above. Bot principals have no client replay to guard, so recording the seq is
    // benign — it keeps the server path a thin wrapper over the client path.

    /// In-memory apply for a server-driven principal (auto sequence).
    pub fn apply_server(&mut self, who: Principal, cmd: Command) -> Result<ApplyOutcome, ApplyErr> {
        let seq = self.last_seq[who.idx()] + 1;
        self.apply(who, seq, cmd)
    }

    /// Journaled apply for a server-driven principal (auto sequence).
    pub async fn apply_journaled_server(
        &mut self,
        store: &AsyncStore<'_, GameState>,
        who: Principal,
        cmd: Command,
    ) -> Result<ApplyOutcome, ApplyErr> {
        let seq = self.last_seq[who.idx()] + 1;
        self.apply_journaled(store, who, seq, cmd).await
    }

    /// Feed an applied event batch to the §16 game-design metrics against the
    /// post-batch engine state (the one chokepoint every player- and bot-applied
    /// batch passes through). Disjoint-field borrow: `metrics` is mutated while
    /// `engine` is read for the post-batch state and the form-tier lookup. Aggregate
    /// counts only — never a hand, the deck, or the seed (redaction holds).
    fn observe_metrics(&mut self, events: &[recollect_core::Event]) {
        let engine = &self.engine;
        self.metrics.observe(events, engine.state(), |form| {
            // A Fabled form (donor-fueled leap) vs a Primal (self-fueled) — the only
            // datum the scan needs from outside the events, read off the catalog.
            engine.card(form).rarity == "Fabled"
        });
    }

    /// The post-apply finish — observe the §16 metrics, then fan the views by the
    /// acting principal's KIND (the H4 single branch): a 1v1 seat gets its own view +
    /// the opponent's ([`Solo`](ApplyOutcome::Solo)); a 2v2 slot gets fresh `TeamView`s
    /// for all four slots ([`Team`](ApplyOutcome::Team)). Shared by the in-memory and
    /// journaled funnels so their view/finish handling can't drift.
    fn finish_apply(
        &mut self,
        who: Principal,
        events: Vec<recollect_core::Event>,
        draws: u64,
    ) -> ApplyOutcome {
        self.observe_metrics(&events);
        let finished = self.finished();
        match who {
            Principal::Seat(seat) => {
                let other = seat.other();
                ApplyOutcome::Solo(Box::new(ApplyOk {
                    your_view: view_for(&self.engine, seat),
                    other_view: view_for(&self.engine, other),
                    other_seat: other,
                    events,
                    draws_after: draws,
                    finished,
                }))
            }
            Principal::Slot(_) => {
                use recollect_core::types::SeatSlot::*;
                let views = [A1, B1, A2, B2].map(|s| (s, view_for_slot(&self.engine, s)));
                ApplyOutcome::Team(Box::new(ApplyOkSlot {
                    views,
                    events,
                    draws_after: draws,
                    finished,
                }))
            }
        }
    }

    /// The live engine — the AI chooser needs the full (non-redacted) state to
    /// pick its move. Only the server holds this; clients never see it.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// §16: which seat (if any) led the board at the Dusk contraction — captured by
    /// the metrics accumulator as the match ran, read at match end so the finish
    /// counter can correlate "did the side leading at contraction go on to win?".
    /// `None` until the contraction happens (or if it was a tie).
    pub(crate) fn contraction_lead(&self) -> crate::metrics::ContractionLead {
        self.metrics.contraction_lead()
    }

    /// Test seam: the §16 turn count the accumulator has tallied for this match.
    #[cfg(test)]
    pub(crate) fn metrics_turns(&self) -> u64 {
        self.metrics.turns()
    }

    /// Whose turn it is in 1v1.
    pub fn active_seat(&self) -> Seat {
        self.engine.state().active
    }

    pub fn is_finished(&self) -> bool {
        matches!(
            self.engine.state().phase,
            recollect_core::state::Phase::Finished { .. }
        )
    }

    /// The result string for a finished match (for the `matches` record).
    pub fn finished(&self) -> Option<String> {
        match &self.engine.state().phase {
            recollect_core::state::Phase::Finished {
                result,
                score_a,
                score_b,
            } => Some(format!("{result:?} {score_a}-{score_b}")),
            _ => None,
        }
    }

    /// `seat`'s legal moves, each with a human label (empty when it isn't their
    /// turn). Shipped to the client so a networked seat gets the same legal-move
    /// menu a local one has without its own engine.
    pub fn legal_labeled(&self, seat: Seat) -> Vec<recollect_protocol::LegalMove> {
        self.engine
            .legal_commands(seat)
            .into_iter()
            .map(|cmd| recollect_protocol::LegalMove {
                label: recollect_protocol::label(&self.engine, seat, &cmd),
                forecast: recollect_protocol::forecast(&self.engine, seat, &cmd),
                cmd,
            })
            .collect()
    }

    /// A 2v2 slot's legal moves — non-empty only when it's this slot's turn
    /// (the acting team is `slot.team()`). Labels reference the team's vantage.
    pub fn legal_labeled_slot(
        &self,
        slot: recollect_core::types::SeatSlot,
    ) -> Vec<recollect_protocol::LegalMove> {
        if slot != self.engine.state().active_slot {
            return Vec::new();
        }
        self.legal_labeled(slot.team())
    }

    /// The journal genesis: the opening state and the entropy position it already
    /// sits at (post-shuffle). Handed to `AsyncStore::open` at match creation.
    pub fn snapshot(&self) -> (GameState, recollect_core::DrawPos) {
        self.engine.snapshot()
    }

    /// Rebuild a session from an engine restored out of the journal (the
    /// postgres-authoritative restart path). Sequence guards reset — the client
    /// reconnects with a fresh sequence after a resume.
    pub fn from_engine(engine: Engine) -> Session {
        Session {
            engine,
            last_seq: [0; 4],
            metrics: crate::metrics::MatchMetrics::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recollect_core::types::CardId;

    fn deck() -> Vec<CardId> {
        [
            0u16, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10,
        ]
        .iter()
        .map(|i| CardId(*i))
        .collect()
    }

    #[test]
    fn sessions_reject_replayed_sequence_numbers() {
        let mut s = Session::new(1, deck(), deck());
        let a = Principal::Seat(Seat::A);
        // Glimpse (§5) opens two choices — burn a hand card, then keep-or-bottom;
        // resolve both (burn, then keep) before ending the turn.
        assert!(s.apply(a, 1, Command::Glimpse).is_ok());
        assert!(s.apply(a, 2, Command::Choose { index: 0 }).is_ok()); // burn
        assert!(s.apply(a, 3, Command::Choose { index: 0 }).is_ok()); // keep
        // A replayed seq (1, already consumed) is rejected as stale.
        assert_eq!(
            s.apply(a, 1, Command::EndTurn).unwrap_err(),
            ApplyErr::StaleOrReplayedSeq
        );
        assert!(s.apply(a, 4, Command::EndTurn).is_ok());
    }

    #[test]
    fn sessions_route_rule_rejections_without_state_change() {
        let mut s = Session::new(1, deck(), deck());
        let before = serde_json::to_string(&s.view(Seat::A)).unwrap();
        assert_eq!(
            s.apply(Principal::Seat(Seat::B), 1, Command::Glimpse)
                .unwrap_err(),
            ApplyErr::Rule(Reject::NotYourTurn)
        );
        let after = serde_json::to_string(&s.view(Seat::A)).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn each_seat_receives_only_its_own_view_after_apply() {
        let mut s = Session::new(2, deck(), deck());
        let ok = s
            .apply(Principal::Seat(Seat::A), 1, Command::Glimpse)
            .unwrap()
            .solo();
        assert_eq!(ok.your_view.seat, Seat::A);
        assert_eq!(ok.other_view.seat, Seat::B);
        assert_eq!(ok.other_seat, Seat::B);
        // Cross-check the redaction property at the session boundary too.
        let json_b = serde_json::to_string(&ok.other_view).unwrap();
        assert_eq!(json_b.matches("\"hand\":").count(), 1);
    }

    /// §5: a mulligan flows through the actor/session decide path like any command.
    /// The acting seat's own view reflects the fresh hand + the public beat; the
    /// opponent's pushed view (`other_view`) carries the public beat and truthful
    /// counts but never the redrawn cards or the deck order.
    #[test]
    fn mulligan_routes_through_the_session_and_redacts_the_opponent_view() {
        let mut s = Session::new(2, deck(), deck());
        // Offered in the opener's legal menu the server hands clients.
        assert!(
            s.legal_labeled(Seat::A)
                .iter()
                .any(|m| matches!(m.cmd, Command::Mulligan { .. })),
            "the wire legal menu offers the opening mulligan"
        );
        let ok = s
            .apply(
                Principal::Seat(Seat::A),
                1,
                Command::Mulligan { seat: Seat::A },
            )
            .expect("the mulligan applies through the session")
            .solo();
        // The acting seat's own view: the public beat is set; it sees its own hand.
        assert!(ok.your_view.mulliganed[Seat::A as usize]);
        // The opponent's pushed view: the public beat, but no card leak.
        assert_eq!(ok.other_view.seat, Seat::B);
        assert!(
            ok.other_view.mulliganed[Seat::A as usize],
            "B learns THAT A mulliganed"
        );
        let json_b = serde_json::to_string(&ok.other_view).unwrap();
        assert_eq!(
            json_b.matches("\"hand\":").count(),
            1,
            "only B's own hand crosses — A's redraw is redacted"
        );
        assert_eq!(
            json_b.matches("\"deck\":").count(),
            0,
            "no deck ordering crosses the session boundary"
        );
    }

    /// §16 metrics wiring, end to end through the real engine: a full match driven
    /// to its result feeds every applied batch through the session's one funnel, so
    /// the accumulator tallies the turns over the whole match and observes the Dusk
    /// contraction. An EndTurn-only match keeps an empty board — so the contraction
    /// fires with NO leader (a dead-even 0–0), exactly the `None` the panel must not
    /// count — and the turn tally climbs to the full 12-round clock (both seats each
    /// turn). This proves the events→`observe`→accumulator path the dashboard reads,
    /// not just the classifier in isolation.
    #[test]
    fn the_metrics_accumulator_tracks_a_full_match() {
        use recollect_core::state::Phase;
        let mut s = Session::new(7, deck(), deck());
        // Drive both seats to do nothing but end their turns until the match ends.
        // EndTurn alternates the active seat (A→B→A…); the round (and the Dusk
        // contraction at round 8) advance on B's end-of-turn, finishing at round 12.
        // Answer the phase the way any client must: the hand-cap Release and any
        // choice prompt, else just end the turn.
        let mut applied_turns = 0u64;
        for _ in 0..400 {
            if s.is_finished() {
                break;
            }
            let seat = s.active_seat();
            let cmd = match s.engine().state().phase {
                Phase::PendingRelease { .. } => Command::Release { hand_index: 0 },
                Phase::PendingChoice { .. } => Command::Choose { index: 0 },
                _ => Command::EndTurn,
            };
            let is_end = matches!(cmd, Command::EndTurn);
            s.apply_server(Principal::Seat(seat), cmd)
                .expect("the phase-appropriate command always applies");
            if is_end {
                applied_turns += 1;
            }
        }
        assert!(s.is_finished(), "the EndTurn-only match reached a result");
        // Every TurnEnded was counted — the length histogram's input is the real
        // per-turn tally, climbing in lockstep with the turns we drove.
        assert_eq!(
            s.metrics_turns(),
            applied_turns,
            "the accumulator counted exactly the turns the match played"
        );
        assert!(
            s.metrics_turns() >= 12,
            "a full 12-round match tallies at least a dozen turns"
        );
        // The contraction fired on an empty board: a 0–0 tie ⇒ no leader to correlate
        // (the panel's denominator excludes it — leading-at-contraction must be strict).
        assert_eq!(
            s.contraction_lead(),
            crate::metrics::ContractionLead::None,
            "an empty-board contraction has no strict leader"
        );
    }
}

#[cfg(test)]
mod twovtwo_tests {
    use super::*;
    use recollect_core::types::{CardId, SeatSlot};

    fn decks() -> [Vec<CardId>; 4] {
        [
            (0..20).map(|_| CardId(0)).collect(),
            (0..20).map(|_| CardId(0)).collect(),
            (0..20).map(|_| CardId(0)).collect(),
            (0..20).map(|_| CardId(0)).collect(),
        ]
    }

    #[test]
    fn a_2v2_session_routes_commands_by_slot_and_fans_out_four_views() {
        let mut s = Session::new_2v2(7, decks());
        // A1 acts first; the view it gets is its own.
        let v = s.view_slot(SeatSlot::A1);
        assert_eq!(v.board_w, 6);
        assert_eq!(v.active_slot, SeatSlot::A1);
        // B1 trying to act out of turn is refused.
        let early = s.apply(
            Principal::Slot(SeatSlot::B1),
            1,
            recollect_core::Command::EndTurn,
        );
        assert!(matches!(early, Err(ApplyErr::Rule(Reject::NotYourTurn))));
        // A1 ends its turn; we get fresh views for all four slots, and the
        // active slot advances to B1.
        let ok = s
            .apply(
                Principal::Slot(SeatSlot::A1),
                1,
                recollect_core::Command::EndTurn,
            )
            .expect("A1 acts")
            .team();
        assert_eq!(ok.views.len(), 4);
        assert_eq!(
            s.view_slot(SeatSlot::B1).active_slot,
            SeatSlot::B1,
            "turn passed to B1"
        );
        // Stale seq from A1 is rejected even on its next turn-attempt.
        let stale = s.apply(
            Principal::Slot(SeatSlot::A1),
            1,
            recollect_core::Command::EndTurn,
        );
        assert!(matches!(
            stale,
            Err(ApplyErr::Rule(Reject::NotYourTurn)) | Err(ApplyErr::StaleOrReplayedSeq)
        ));
    }

    #[test]
    fn solace_pve_session_runs_seat_b_as_a_normal_player() {
        // In a Solace match, seat B plays from its deck like any seat —
        // there is no bespoke director command. It takes (and ends) its turn through the same
        // journaled path as the player.
        let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
        let mut s = Session::new(7, deck.clone(), deck);
        s.apply_server(
            Principal::Seat(recollect_core::Seat::A),
            recollect_core::Command::EndTurn,
        )
        .expect("the Lorekeeper ends its turn");
        assert_eq!(
            s.snapshot().0.active,
            recollect_core::Seat::B,
            "the turn passes to the Solace"
        );
        s.apply_server(
            Principal::Seat(recollect_core::Seat::B),
            recollect_core::Command::EndTurn,
        )
        .expect("the Solace takes a normal turn");
        assert_eq!(
            s.snapshot().0.active,
            recollect_core::Seat::A,
            "and the turn passes back to the player"
        );
    }

    #[test]
    fn apply_slot_server_drives_the_active_slot_without_a_seq() {
        let mut s = Session::new_2v2(7, decks());
        // A non-active slot is refused, same as the client path.
        assert!(matches!(
            s.apply_server(
                Principal::Slot(SeatSlot::B1),
                recollect_core::Command::EndTurn
            ),
            Err(ApplyErr::Rule(Reject::NotYourTurn))
        ));
        // The active slot advances with no client seq (the 2v2 bot-drive path):
        // A1 → B1 → A2, each a server-initiated move.
        let ok = s
            .apply_server(
                Principal::Slot(SeatSlot::A1),
                recollect_core::Command::EndTurn,
            )
            .expect("A1 server-acts")
            .team();
        assert_eq!(ok.views.len(), 4);
        assert_eq!(s.view_slot(SeatSlot::B1).active_slot, SeatSlot::B1);
        let ok2 = s
            .apply_server(
                Principal::Slot(SeatSlot::B1),
                recollect_core::Command::EndTurn,
            )
            .expect("B1 server-acts")
            .team();
        assert_eq!(ok2.views.len(), 4);
        assert_eq!(s.view_slot(SeatSlot::A2).active_slot, SeatSlot::A2);
    }
}

/// The postgres-authoritative round-trip: a match driven through `apply_journaled`
/// resumes from the journal bit-identical. Proves the whole path end to end —
/// append-before-ack, GameState/Event postcard serialization, the head-position
/// resume, and `Engine::from_state` — against a live database.
///
/// Run with Postgres: `make up && PG_URL=… cargo test -p recollect-server -- --ignored`.
#[cfg(test)]
mod journaled_pg {
    use super::*;
    use recollect_core::cards::canon_catalog;
    use recollect_core::types::CardId;
    use recollect_journal_postgres::store::{AsyncStore, JOURNAL_SCHEMA, resume_async};

    fn deck() -> Vec<CardId> {
        (0..10u16).chain(0..10u16).map(CardId).collect()
    }

    #[tokio::test]
    #[ignore = "requires postgres (make up && PG_URL=… cargo test -p recollect-server -- --ignored)"]
    async fn a_journaled_match_resumes_bit_identical() {
        let url = std::env::var("PG_URL").expect("PG_URL for the journaled-resume test");
        let (client, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = conn.await;
        });
        client.batch_execute(JOURNAL_SCHEMA).await.unwrap();

        let seed = 0xD00D_1234_5678_9ABC;
        let stream = format!("match-test-{}", std::process::id());
        let mut live = Session::new(seed, deck(), deck());

        // Genesis snapshot at the post-opening entropy position.
        let (genesis, pos) = live.snapshot();
        let store = AsyncStore::open(&client, stream.clone(), &genesis, pos)
            .await
            .unwrap();

        // Drive a handful of turns through the authoritative path. EndTurn flips
        // the active seat in 1v1, so the acting seat alternates A→B→A…; round
        // boundaries draw entropy, which is exactly what the resume must reproduce.
        let mut seat = Seat::A;
        let mut seq = [0u64; 2];
        let mut applied = 0;
        for _ in 0..8 {
            let idx = if seat == Seat::A { 0 } else { 1 };
            seq[idx] += 1;
            match live
                .apply_journaled(&store, Principal::Seat(seat), seq[idx], Command::EndTurn)
                .await
            {
                Ok(_) => {
                    applied += 1;
                    seat = seat.other();
                }
                Err(_) => break,
            }
        }
        assert!(
            applied >= 2,
            "drove at least a couple of journaled commands"
        );

        // A fresh process would rebuild the match from the journal alone.
        let (resumed_agg, resume_pos) = resume_async::<GameState>(&store).await.unwrap();
        let resumed = Session::from_engine(Engine::from_state(
            resumed_agg.state().clone(),
            seed,
            resume_pos,
            canon_catalog(),
        ));

        assert_eq!(
            resumed.snapshot(),
            live.snapshot(),
            "resume reproduced the live state and entropy position exactly"
        );
    }

    #[tokio::test]
    #[ignore = "requires postgres (make up && PG_URL=… cargo test -p recollect-server -- --ignored)"]
    async fn a_journaled_2v2_match_resumes_bit_identical() {
        let url = std::env::var("PG_URL").expect("PG_URL for the journaled-resume test");
        let (client, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = conn.await;
        });
        client.batch_execute(JOURNAL_SCHEMA).await.unwrap();

        let seed = 0x2222_4444_6666_8888;
        let stream = format!("match-2v2-test-{}", std::process::id());
        let mut live = Session::new_2v2(seed, [deck(), deck(), deck(), deck()]);

        let (genesis, pos) = live.snapshot();
        let store = AsyncStore::open(&client, stream.clone(), &genesis, pos)
            .await
            .unwrap();

        // Drive commands through whichever slot is active (the rotation is
        // A1→B1→A2→B2); EndTurn at the round boundary draws entropy, which the
        // resume must reproduce. Read the live active slot each step so the test
        // doesn't bake in the rotation order.
        let slot_idx = |s: recollect_core::types::SeatSlot| {
            use recollect_core::types::SeatSlot::*;
            match s {
                A1 => 0,
                B1 => 1,
                A2 => 2,
                B2 => 3,
            }
        };
        let mut seq = [0u64; 4];
        let mut applied = 0;
        for _ in 0..8 {
            let slot = live.snapshot().0.active_slot;
            let i = slot_idx(slot);
            seq[i] += 1;
            match live
                .apply_journaled(&store, Principal::Slot(slot), seq[i], Command::EndTurn)
                .await
            {
                Ok(_) => applied += 1,
                Err(_) => break,
            }
        }
        assert!(
            applied >= 4,
            "drove a full slot rotation through the journal"
        );

        let (resumed_agg, resume_pos) = resume_async::<GameState>(&store).await.unwrap();
        let resumed = Session::from_engine(Engine::from_state(
            resumed_agg.state().clone(),
            seed,
            resume_pos,
            canon_catalog(),
        ));

        assert_eq!(
            resumed.snapshot(),
            live.snapshot(),
            "2v2 resume reproduced the team board, slot rotation, and entropy exactly"
        );
    }

    /// DETERMINISM (the forfeit replays): an absence forfeit issued the way the actor
    /// issues it — `Command::MatchAbandoned` through the SAME journaled funnel
    /// (`apply_journaled_server`) — is a journaled command, so a fresh process rebuilding
    /// from the stream alone reaches the IDENTICAL Finished state. Proves the forfeit isn't
    /// an out-of-band mutation that escapes the journal: it appends, and `resume_async`
    /// reproduces the `Win(present_seat)` finish bit-for-bit.
    #[tokio::test]
    #[ignore = "requires postgres (make up && PG_URL=… cargo test -p recollect-server -- --ignored)"]
    async fn an_abandoned_match_resumes_to_the_same_finished_state() {
        use crate::actor::Principal;
        let url = std::env::var("PG_URL").expect("PG_URL for the abandon-resume test");
        let (client, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = conn.await;
        });
        client.batch_execute(JOURNAL_SCHEMA).await.unwrap();

        let seed = 0xABAD_0FF0_1234_5678;
        let stream = format!("match-abandon-{}", std::process::id());
        let mut live = Session::new(seed, deck(), deck());
        let (genesis, pos) = live.snapshot();
        let store = AsyncStore::open(&client, stream.clone(), &genesis, pos)
            .await
            .unwrap();

        // Drive a couple of journaled turns, then forfeit Seat A (system command, no
        // client seq — exactly the actor's `fire_abandonment` path).
        live.apply_journaled(&store, Principal::Seat(Seat::A), 1, Command::EndTurn)
            .await
            .expect("A ends its turn (journaled)");
        live.apply_journaled(&store, Principal::Seat(Seat::B), 1, Command::EndTurn)
            .await
            .expect("B ends its turn (journaled)");
        let out = live
            .apply_journaled_server(
                &store,
                Principal::Seat(Seat::A),
                Command::MatchAbandoned { seat: Seat::A },
            )
            .await
            .expect("the forfeit appends + applies");
        assert_eq!(
            out.finished().map(String::as_str),
            Some("Win(B) 0-0"),
            "the forfeit finishes the match as Win(present_seat)=Win(B)"
        );

        // A fresh process rebuilds from the journal alone — same Finished state + entropy.
        let (resumed_agg, resume_pos) = resume_async::<GameState>(&store).await.unwrap();
        let resumed = Session::from_engine(Engine::from_state(
            resumed_agg.state().clone(),
            seed,
            resume_pos,
            canon_catalog(),
        ));
        assert_eq!(
            resumed.snapshot(),
            live.snapshot(),
            "the abandoned match resumes bit-identical — the forfeit is a journaled command"
        );
        assert_eq!(
            resumed.finished().as_deref(),
            Some("Win(B) 0-0"),
            "the resumed match is finished by the same forfeit result"
        );
    }
}
