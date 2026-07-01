//! Actor-per-match: one owning task per live match.
//!
//! **One task owns the [`Session`] by value** — no lock at all, because nothing
//! else touches it. This avoids two structural hazards a `tokio::Mutex<Session>`
//! plus a lossy `broadcast` fan-out would carry:
//!   - A journaled apply that held the session mutex across the append `.await`
//!     is a footgun — a future `.await` under the lock would serialize or deadlock
//!     the match.
//!   - A bounded broadcast channel *drops* frames under a slow consumer; a dropped
//!     `Update` would leave an opponent's board stale until they reconnected.
//!
//! Connections talk to the actor over an mpsc [`MatchCmd`] channel (back-pressured,
//! never silently dropped), and the actor pushes each seat its frames over
//! **per-seat unbounded mpsc** senders it holds in a registry. A seat subscribes by
//! handing the actor a sender ([`MatchCmd::Subscribe`]); the actor addresses fan-out
//! to exactly that seat's sender. There is no shared broadcast and no drop class: a
//! per-seat queue grows for a wedged socket rather than dropping a neighbour's frame,
//! and the socket task tears down on its own send error.
//!
//! Determinism, redaction, and the journaled seam: the actor calls the [`Session`]
//! methods (`apply_journaled`/`apply`, `drive_bot`, the slot twins) in order, as the
//! single, lock-free writer per match. The append-before-ack discipline
//! (`decide_journaled`/`Decided` + `AsyncStore`) lives in `Session`; the actor just
//! owns the one `.await`.
//!
//! **Mode seam:** [`MatchActor`] dispatches on the [`Session`]'s mode (1v1 vs 2v2)
//! inside `apply` and `welcome`. The **opener drive** is unified: both welcome arms
//! call ONE [`drive_openers`](MatchActor::drive_openers) before the first view, so a
//! bot that won the seeded opener toss plays its turn(s) in 1v1 AND 2v2 alike — the
//! human is never stranded on the welcome. The transport (`ws.rs`) is mode-agnostic
//! (it forwards bytes + a [`Principal`]).
//!
//! **Reconnection:** because the actor outlives any one socket and addresses fan-out
//! per-principal, a dropped seat reconnecting is just a fresh [`MatchCmd::Subscribe`]
//! replacing that principal's sender (the actor sends a full view on subscribe — the
//! welcome). A resume token and in-flight replay drop onto this seam without touching
//! the engine path.
use super::*;
use crate::session::{ApplyErr, ApplyOk, ApplyOkSlot, ApplyOutcome, Session};
use recollect_core::Command;
use recollect_core::types::{Seat, SeatSlot};
use tokio::sync::{mpsc, oneshot};

/// Which player a frame is addressed to — a 1v1 seat or a 2v2 slot. The actor
/// keys its per-recipient sender registry on this, so fan-out is exact (a seat
/// only ever receives frames addressed to it — the redaction filter the old
/// `for_seat == seat` broadcast guard enforced, now by construction: an opponent
/// has no sender for your view).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Principal {
    Seat(Seat),
    Slot(SeatSlot),
}

impl Principal {
    /// The per-principal index (also the [`Session`] seq-dedup slot): A/A1=0, B/B1=1,
    /// A2=2, B2=3. 1v1 uses `[0,1]`; 2v2 all four.
    pub(crate) fn idx(self) -> usize {
        match self {
            Principal::Seat(Seat::A) => 0,
            Principal::Seat(Seat::B) => 1,
            Principal::Slot(SeatSlot::A1) => 0,
            Principal::Slot(SeatSlot::B1) => 1,
            Principal::Slot(SeatSlot::A2) => 2,
            Principal::Slot(SeatSlot::B2) => 3,
        }
    }

    /// The seat whose hand/turn this principal acts on: a 1v1 seat IS that seat; a
    /// 2v2 slot acts for its team. The one value `engine.apply`/`decide_journaled`
    /// is keyed on, so the [`Session`] apply funnel derives it once and shares the
    /// rest of the path across modes.
    pub(crate) fn acting_seat(self) -> Seat {
        match self {
            Principal::Seat(s) => s,
            Principal::Slot(sl) => sl.team(),
        }
    }

    /// The physical slot, when this is a 2v2 principal — `Some` drives the slot
    /// turn-guard (a slot must be the engine's *active_slot*, not merely on the
    /// active team, so A2 can't act on A1's turn). `None` for a 1v1 seat, whose
    /// turn the engine itself gates.
    pub(crate) fn slot(self) -> Option<SeatSlot> {
        match self {
            Principal::Seat(_) => None,
            Principal::Slot(sl) => Some(sl),
        }
    }
}

/// A serialized [`ServerMsg`] (JSON) bound for one principal's socket. The actor
/// produces these; the per-seat sender carries them to the socket task verbatim.
pub(crate) type Frame = String;

/// Messages the actor accepts over its command channel. Every connection holds a
/// clone of the sending half ([`MatchHandle`]); the actor is the sole receiver.
pub(crate) enum MatchCmd {
    /// A seat/slot socket joins (or rejoins): register its frame sender and
    /// reply with the welcome frame to send first. The actor plays any pending bot
    /// opener before computing the welcome — in 1v1 AND 2v2 (one
    /// `drive_openers` call), so a bot opener never strands the human on the welcome.
    Subscribe {
        who: Principal,
        /// Where the actor pushes this principal's fan-out frames hereafter.
        tx: mpsc::UnboundedSender<Frame>,
        /// Optional session display name from this principal's `Hello` (1v1 only;
        /// the actor stores it and echoes it to the table in the welcome).
        name: Option<String>,
        /// The welcome frame (full current view) to send before the loop starts.
        reply: oneshot::Sender<Frame>,
    },
    /// A `Cmd`/`Ping`/`Hello` text frame from a principal's socket. The actor
    /// applies it (journaled-or-in-memory), drives any bot, fans the opponent
    /// view(s) out per-seat, emits the seed reveal on finish, and replies with the
    /// acting principal's own frame (the `Applied`/`TeamApplied`/`Pong`/`Welcome`).
    Text {
        who: Principal,
        text: String,
        reply: oneshot::Sender<Frame>,
    },
    /// A principal's socket tore down (ws Close / `recv()`→`None` / a send error). The
    /// `socket_loop` fires this best-effort as it ends, so the actor learns of a drop
    /// **eagerly** rather than lazily on the next reconnect. The actor arms the absence-
    /// forfeit grace timer for a HUMAN principal (a bot has no socket and never sends
    /// this); a reconnect ([`MatchCmd::Subscribe`]) disarms it. No reply — it's a one-way
    /// signal, and a stale `SeatVacated` racing a fresh `Subscribe` is harmless (the
    /// reconnect supersedes it, and the arm-check ignores a principal with a live sender).
    SeatVacated { who: Principal },
    /// Test-only inspection seam: snapshot the actor-owned session + whether a bot
    /// is attached. The recovery tests assert a rebuilt match reproduces the live
    /// state bit-identically through the actor that now owns it (the session is no
    /// longer reachable directly). Not on any production path.
    #[cfg(test)]
    Inspect { reply: oneshot::Sender<Inspection> },
}

/// What [`MatchCmd::Inspect`] returns: the session snapshot, the re-attached 1v1
/// bot's `(seat, difficulty)` if any, and the published seed commitment — the
/// surface the recovery tests check. `seed_commit_hex` lets the recovery
/// test prove a rebuilt match re-commits to the ORIGINAL commitment (same salt).
#[cfg(test)]
pub(crate) struct Inspection {
    pub(crate) snapshot: (recollect_core::GameState, recollect_core::DrawPos),
    pub(crate) bot: Option<(Seat, recollect_bot::Difficulty)>,
    pub(crate) seed_commit_hex: String,
}

/// The connection-side handle to a match actor: a cloneable command sender. Stored
/// on the [`Match`] in place of the old `tokio::Mutex<Session>` + `broadcast::Sender`.
/// When the last handle drops, the actor's `recv` returns `None` and the task ends —
/// so a swept/finished match's actor shuts down once its sockets close.
#[derive(Clone)]
pub(crate) struct MatchHandle {
    tx: mpsc::Sender<MatchCmd>,
}

impl MatchHandle {
    /// Subscribe a principal's socket, returning the welcome frame to send first.
    /// `None` if the actor is gone (its task ended) — the socket then closes.
    pub(crate) async fn subscribe(
        &self,
        who: Principal,
        tx: mpsc::UnboundedSender<Frame>,
        name: Option<String>,
    ) -> Option<Frame> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(MatchCmd::Subscribe {
                who,
                tx,
                name,
                reply,
            })
            .await
            .ok()?;
        rx.await.ok()
    }

    /// Route a text frame to the actor and await the principal's own reply frame.
    /// `None` if the actor is gone — the socket then closes.
    pub(crate) async fn text(&self, who: Principal, text: String) -> Option<Frame> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(MatchCmd::Text { who, text, reply })
            .await
            .ok()?;
        rx.await.ok()
    }

    /// Signal that this principal's socket tore down — best-effort, fire-and-forget. The
    /// `socket_loop` calls this as it ends so the actor arms the absence-forfeit timer
    /// eagerly; a send failure (the actor is already gone) is fine to ignore.
    pub(crate) async fn seat_vacated(&self, who: Principal) {
        let _ = self.tx.send(MatchCmd::SeatVacated { who }).await;
    }

    /// Test-only: snapshot the actor-owned session + bot presence (the recovery
    /// tests' inspection seam).
    #[cfg(test)]
    pub(crate) async fn inspect(&self) -> Option<Inspection> {
        let (reply, rx) = oneshot::channel();
        self.tx.send(MatchCmd::Inspect { reply }).await.ok()?;
        rx.await.ok()
    }
}

/// One match's owning task: holds the [`Session`] by value (no lock) and the
/// per-principal frame senders. Spawned by [`spawn`]; reachable only through its
/// [`MatchHandle`].
pub(crate) struct MatchActor {
    rx: mpsc::Receiver<MatchCmd>,
    config: ActorConfig,
    session: Session,
    /// Per-principal frame senders (index = `Principal::idx`). A 1v1 uses `[0,1]`;
    /// a 2v2 uses `[0..4]`. `None` until that principal subscribes; replaced on a
    /// reconnect. Fan-out addresses these directly — no broadcast.
    seats: [Option<mpsc::UnboundedSender<Frame>>; 4],
    /// Per-principal absence-forfeit deadlines (index = `Principal::idx`). Set to
    /// `vacated_at + grace` when a HUMAN principal's socket tears down ([`MatchCmd::SeatVacated`]);
    /// cleared on its reconnect ([`Self::on_subscribe`]). The run loop selects on the earliest
    /// pending deadline; when one passes and the principal is still absent and the match isn't
    /// finished, it forfeits ([`Self::fire_abandonment`]). `None` ⇒ that principal is present, is a
    /// bot, or the forfeit is disabled (`abandon_grace == 0`).
    vacated_deadline: [Option<tokio::time::Instant>; 4],
    /// Set once the match finishes, so the seed reveal is emitted exactly once
    /// even if a late frame arrives after the terminal one.
    revealed: bool,
}

/// The non-`Session` state an actor needs: the journal wiring, the bot config, the
/// seed commitment, and (1v1) the optional session display names. Moved here off
/// the old `MatchEntry`/`Lobby2v2` so the actor — the sole writer — owns it all.
pub(crate) struct ActorConfig {
    pub(crate) mode: ActorMode,
    pub(crate) db_id: String,
    pub(crate) journal: Option<Arc<tokio::sync::Mutex<recollect_journal_postgres::Journal>>>,
    pub(crate) event_client: Option<Arc<tokio_postgres::Client>>,
    pub(crate) seed_commit: SeedCommitment,
    /// Absence-forfeit grace: a HUMAN principal that disconnects and doesn't reconnect within this
    /// long forfeits ([`MatchActor::on_vacated`]). `Duration::ZERO` DISABLES it (a drop never ends
    /// the match — the reconnect tests rely on this). Threaded from the create-match query
    /// (`?abandon_grace_secs=N`, default [`crate::match_settings::DEFAULT_ABANDON_GRACE`]).
    pub(crate) abandon_grace: std::time::Duration,
}

/// Mode-specific actor state (the mode dispatch seam). 1v1 carries the optional
/// Seat-B bot and the session display names; 2v2 carries its bot slots.
pub(crate) enum ActorMode {
    OneVsOne {
        bot: Option<Bot>,
        names: [Option<String>; 2],
    },
    TwoVsTwo {
        bots: Vec<(SeatSlot, SlotBot)>,
    },
}

/// Spawn a match actor, returning the connection-side handle. The channel is
/// bounded (back-pressure on a flood of commands is correct — a match processes
/// one command at a time anyway); per-seat *output* is unbounded so a slow socket
/// never blocks the writer or drops a neighbour's frame.
pub(crate) fn spawn(session: Session, config: ActorConfig) -> MatchHandle {
    let (tx, rx) = mpsc::channel(64);
    let actor = MatchActor {
        rx,
        config,
        session,
        seats: [None, None, None, None],
        vacated_deadline: [None, None, None, None],
        revealed: false,
    };
    tokio::spawn(actor.run());
    MatchHandle { tx }
}

impl MatchActor {
    /// The actor loop: own the session, serve commands until every handle drops.
    /// Single-threaded over the session, so the journaled append `.await` (inside
    /// `Session::apply_journaled`) can never race or deadlock — there is no lock.
    ///
    /// **Absence forfeit.** The loop `select!`s the command channel against the earliest
    /// armed absence-forfeit deadline ([`Self::next_deadline`]). A HUMAN principal whose
    /// socket drops arms a deadline (`vacated_at + grace`); if it passes before the
    /// principal reconnects (and the match isn't already finished), the loop forfeits its
    /// team ([`Self::fire_abandonment`]). With no deadline armed the timer is an
    /// indefinitely-pending future, so the select degenerates to the plain command loop —
    /// zero overhead when the forfeit is disabled or everyone is present.
    #[tracing::instrument(skip_all, fields(db_id = %self.config.db_id))]
    async fn run(mut self) {
        loop {
            // A far-future instant stands in for "no deadline armed", so the timer branch
            // is always present but only ever fires when a real deadline is set. `sleep_until`
            // with a past instant fires immediately, which is exactly right for a deadline
            // that already lapsed while we were busy in another arm.
            let deadline = self.next_deadline().unwrap_or_else(|| {
                tokio::time::Instant::now() + std::time::Duration::from_secs(3600)
            });
            tokio::select! {
                cmd = self.rx.recv() => {
                    let Some(cmd) = cmd else { break };
                    self.handle_cmd(cmd).await;
                }
                _ = tokio::time::sleep_until(deadline) => {
                    self.on_deadline().await;
                }
            }
        }
        tracing::debug!("match actor stopped (all handles dropped)");
    }

    /// Dispatch one command from the channel (the body of the old `while let` loop).
    async fn handle_cmd(&mut self, cmd: MatchCmd) {
        match cmd {
            MatchCmd::Subscribe {
                who,
                tx,
                name,
                reply,
            } => {
                let welcome = self.on_subscribe(who, tx, name).await;
                // A dropped reply means the socket went away mid-handshake; fine.
                let _ = reply.send(welcome);
            }
            MatchCmd::Text { who, text, reply } => {
                let frame = self.on_text(who, &text).await;
                let _ = reply.send(frame);
            }
            MatchCmd::SeatVacated { who } => self.on_vacated(who),
            #[cfg(test)]
            MatchCmd::Inspect { reply } => {
                let bot = match &self.config.mode {
                    ActorMode::OneVsOne { bot, .. } => bot.as_ref().map(|b| (b.seat, b.difficulty)),
                    // 2v2 recovery comes back all-human (no bot descriptor).
                    ActorMode::TwoVsTwo { .. } => None,
                };
                let _ = reply.send(Inspection {
                    snapshot: self.session.snapshot(),
                    bot,
                    seed_commit_hex: self.config.seed_commit.commit_hex(),
                });
            }
        }
    }

    // --- absence forfeit (the disconnect → grace → MatchAbandoned wiring) ------

    /// Whether a principal is HUMAN under this match's mode — only a human seat/slot
    /// arms an absence forfeit (a bot has no socket and must never forfeit its seat).
    /// 1v1: Seat B is the bot iff one is configured. 2v2: a slot is a bot iff it's in
    /// the `bots` list.
    fn is_human(&self, who: Principal) -> bool {
        match (&self.config.mode, who) {
            (ActorMode::OneVsOne { bot, .. }, Principal::Seat(seat)) => {
                bot.as_ref().map(|b| b.seat) != Some(seat)
            }
            (ActorMode::TwoVsTwo { bots }, Principal::Slot(slot)) => {
                !bots.iter().any(|(s, _)| *s == slot)
            }
            // A principal whose kind mismatches the mode never reaches the live path
            // (the router gates it); treat it as non-human so it never arms a timer.
            _ => false,
        }
    }

    /// A principal's socket tore down: arm its absence-forfeit deadline. Only a HUMAN
    /// principal arms (a bot never forfeits), only when the grace is enabled
    /// (`abandon_grace > 0`), and only while the match is live — a drop after the finish
    /// is moot. A reconnect ([`Self::on_subscribe`]) clears the deadline before it fires.
    /// Idempotent: re-arming an already-armed principal just refreshes the (later) deadline.
    fn on_vacated(&mut self, who: Principal) {
        if self.config.abandon_grace.is_zero() || !self.is_human(who) || self.session.is_finished()
        {
            return;
        }
        let deadline = tokio::time::Instant::now() + self.config.abandon_grace;
        self.vacated_deadline[who.idx()] = Some(deadline);
        tracing::info!(
            ?who,
            grace_s = self.config.abandon_grace.as_secs(),
            "seat vacated; arming absence-forfeit grace"
        );
    }

    /// The earliest armed absence-forfeit deadline across all principals, if any. The
    /// run loop sleeps until this; `None` ⇒ nothing armed (the timer is parked far out).
    fn next_deadline(&self) -> Option<tokio::time::Instant> {
        self.vacated_deadline.iter().flatten().copied().min()
    }

    /// The principal occupying `seats`/`vacated_deadline` index `i` under this mode — the
    /// inverse of [`Principal::idx`], disambiguated by the match's kind (idx 0 is `Seat::A`
    /// in 1v1 but `Slot::A1` in 2v2). 1v1 occupies `[0,1]` (a slot index returns `None`);
    /// 2v2 occupies `[0..4]`.
    fn principal_at(&self, i: usize) -> Option<Principal> {
        match &self.config.mode {
            ActorMode::OneVsOne { .. } => match i {
                0 => Some(Principal::Seat(Seat::A)),
                1 => Some(Principal::Seat(Seat::B)),
                _ => None,
            },
            ActorMode::TwoVsTwo { .. } => match i {
                0 => Some(Principal::Slot(SeatSlot::A1)),
                1 => Some(Principal::Slot(SeatSlot::B1)),
                2 => Some(Principal::Slot(SeatSlot::A2)),
                3 => Some(Principal::Slot(SeatSlot::B2)),
                _ => None,
            },
        }
    }

    /// A deadline (or the parked far-future one) fired: forfeit every principal whose
    /// deadline has now passed and which is still vacated, provided the match is live. A
    /// principal that reconnected cleared its deadline already, so it isn't here; a
    /// spurious wake (the parked timer) finds nothing due and is a no-op. One pass can
    /// resolve at most one forfeit (the first ends the match); the rest are skipped by the
    /// `is_finished` guard inside [`Self::fire_abandonment`].
    async fn on_deadline(&mut self) {
        let now = tokio::time::Instant::now();
        // Collect the due principals first (can't hold the array borrow across the await).
        let due: Vec<Principal> = (0..4)
            .filter(|&i| self.vacated_deadline[i].is_some_and(|d| d <= now))
            .filter_map(|i| self.principal_at(i))
            .collect();
        for who in due {
            // Clear the deadline whether or not we forfeit (a finished match disarms all).
            self.vacated_deadline[who.idx()] = None;
            if self.session.is_finished() {
                continue;
            }
            self.fire_abandonment(who).await;
        }
    }

    /// Issue the system forfeit for a vacated principal: `Command::MatchAbandoned` against
    /// its TEAM seat (1v1: the seat; 2v2: `slot.team()`), through the SAME apply funnel the
    /// command path uses — so it journals (append-before-ack) and replays via `resume_async`
    /// identically (determinism holds; the forfeit is a journaled command, not an out-of-band
    /// mutation). The present opponent(s) are fanned the finished view + the seed reveal over
    /// the existing finish path. A no-op if the match already finished (a race with a real
    /// finishing move).
    async fn fire_abandonment(&mut self, who: Principal) {
        if self.session.is_finished() {
            return;
        }
        let seat = who.acting_seat();
        let cmd = Command::MatchAbandoned { seat };
        // The system command has no client seq — the session mints one past the last, exactly
        // as the bot-drive path does. Postgres-authoritative when journaled, else in-memory.
        let outcome = match self.config.event_client.as_ref() {
            Some(client) => {
                let store = AsyncStore::attach(client.as_ref(), self.config.db_id.clone());
                self.session.apply_journaled_server(&store, who, cmd).await
            }
            None => self.session.apply_server(who, cmd),
        };
        match outcome {
            Ok(out) => {
                tracing::info!(?who, ?seat, "absence forfeit issued (MatchAbandoned)");
                self.fan_out_finish(out).await;
            }
            // A rejection here would mean the match raced to a finish between our guard and the
            // apply (decide rejects MatchAbandoned only when already Finished). Nothing to do.
            Err(e) => tracing::warn!(?who, ?e, "MatchAbandoned not applied (match already over?)"),
        }
    }

    /// Fan a finished-by-forfeit outcome out to every PRESENT principal: push the finished
    /// view + legal menu, close the match record + count it, then emit the seed reveal —
    /// the same end-of-match sequence the command path runs, minus an acting-socket reply
    /// (the forfeiter has no socket). Reuses `on_finish`/`emit_seed_reveal` so the finish
    /// handling can't drift from the played-out path.
    async fn fan_out_finish(&mut self, out: ApplyOutcome) {
        let result = out.finished().cloned();
        match out {
            ApplyOutcome::Solo(ok) => {
                // 1v1: push BOTH seats their own finished view (the forfeiter's socket is
                // gone, so its push is a harmless no-op; the present opponent gets the finish).
                for (seat, view) in [
                    (ok.your_view.seat, ok.your_view),
                    (ok.other_seat, ok.other_view),
                ] {
                    let frame = ServerMsg::Update {
                        v: PROTOCOL_VERSION,
                        view,
                        legal: self.session.legal_labeled(seat),
                    };
                    self.push(
                        Principal::Seat(seat),
                        serde_json::to_string(&frame).unwrap(),
                    );
                }
                if let Some(result) = &result {
                    // Same 1v1 faction the played-out finish uses — a vs-bot human CAN
                    // abandon (the bot holds the other seat), so a forfeit there is still a
                    // PvE result and carries the bot's faction; a PvP forfeit is "pvp".
                    self.on_finish(
                        result.clone(),
                        self.faction_1v1(),
                        crate::metrics::FinishReason::Abandoned,
                    );
                    self.emit_seed_reveal(&[Principal::Seat(Seat::A), Principal::Seat(Seat::B)]);
                }
            }
            ApplyOutcome::Team(ok) => {
                for (slot, view) in ok.views {
                    let frame = ServerMsg::TeamUpdate {
                        v: PROTOCOL_VERSION,
                        slot,
                        view,
                        legal: self.session.legal_labeled_slot(slot),
                    };
                    self.push(
                        Principal::Slot(slot),
                        serde_json::to_string(&frame).unwrap(),
                    );
                }
                if let Some(result) = &result {
                    self.on_finish(
                        result.clone(),
                        "lorekeeper",
                        crate::metrics::FinishReason::Abandoned,
                    );
                    self.emit_seed_reveal(&[
                        Principal::Slot(SeatSlot::A1),
                        Principal::Slot(SeatSlot::B1),
                        Principal::Slot(SeatSlot::A2),
                        Principal::Slot(SeatSlot::B2),
                    ]);
                }
            }
        }
    }

    /// Register a principal's frame sender and compute its welcome. First plays any
    /// pending bot opener — in 1v1 AND 2v2, via one
    /// [`drive_openers`](Self::drive_openers) — so the first view is never the
    /// opponent's turn with no moves for the human (the old strand-on-the-welcome
    /// bug). Lock-free throughout.
    ///
    /// **First-class reconnection.** This is the whole resume path. A dropped
    /// seat rejoins by opening a new socket and re-`Hello`ing with the SAME seat
    /// token (`ws.rs` re-authorises it against the stored hash — a stranger can't
    /// hijack the seat). That routes here as a fresh [`MatchCmd::Subscribe`] for the
    /// same [`Principal`]. We **supersede** any still-registered sender: replacing
    /// `seats[idx]` drops the previous `tx`, so a lingering old socket's
    /// `rx.recv()` returns `None` and its `socket_loop` tears down — exactly one
    /// live socket per seat, the latest. The returned welcome is the FULL current
    /// redacted view (+ legal menu), so the resumed client is consistent with
    /// authoritative state in one frame — no incremental replay, and redaction +
    /// determinism are untouched (a Welcome is just `view(seat)` like any other).
    /// The client's command sequence keeps climbing across the reconnect, so its
    /// first post-resume `Cmd` is never seen as stale by the session's seq guard.
    async fn on_subscribe(
        &mut self,
        who: Principal,
        tx: mpsc::UnboundedSender<Frame>,
        name: Option<String>,
    ) -> Frame {
        crate::metrics::connection_opened();
        // A live sender already here ⇒ this is a reconnect superseding a socket
        // that hasn't torn down yet (the common drop→rejoin race). Count it and
        // trace it; dropping the old `tx` (the assignment below) closes the stale
        // socket. A *closed* leftover sender is just tidy-up, not a reconnect.
        if let Some(old) = &self.seats[who.idx()]
            && !old.is_closed()
        {
            crate::metrics::reconnection();
            tracing::info!(?who, "seat reconnected; superseding the previous socket");
        }
        self.seats[who.idx()] = Some(tx);
        // Reconnect DISARMS any pending absence forfeit for this principal — the seat is
        // back before its grace lapsed, so it never forfeits. (Clearing here, before the
        // welcome's bot-drive await, means a deadline that would fire during that await is
        // already cancelled.)
        self.vacated_deadline[who.idx()] = None;
        // A principal whose kind doesn't match the mode — the router never produces
        // this (a 1v1 token routes to a Seat, a 2v2 token to a Slot, and the match's
        // mode matches its kind), but answer with a typed error rather than mis-key.
        let mode_ok = matches!(
            (&self.config.mode, who),
            (ActorMode::OneVsOne { .. }, Principal::Seat(_))
                | (ActorMode::TwoVsTwo { .. }, Principal::Slot(_))
        );
        if !mode_ok {
            crate::metrics::frame_rejected(crate::metrics::RejectReason::ModeMismatch);
            return serde_json::to_string(&ServerMsg::Error {
                v: PROTOCOL_VERSION,
                message: "mode_mismatch".into(),
            })
            .unwrap();
        }
        // Stash the optional 1v1 session display name on the connecting seat.
        self.stash_name(who, name);
        // First-player: if the seeded toss opened on a bot seat/slot, play
        // its opening turn(s) before the human's first view — so the human is never
        // stranded staring at the opponent's turn with no legal moves. ONE call for
        // both modes (the 1v1/2v2 welcome+drive consolidation): it
        // dispatches on mode internally and is a no-op when the opener is a human or
        // there is no bot. After it returns, the welcome below reads the fresh view.
        self.drive_openers().await;
        // The full current redacted view — Welcome (1v1) or TeamWelcome (2v2), built
        // in the one shared `welcome_frame` (the Hello refresh uses it too).
        self.welcome_frame(who)
    }

    /// Handle one text frame from a principal, returning that principal's own reply
    /// frame (and pushing fan-out frames to the others). **The one command path (H4):**
    /// parse → (Ping/Hello/Cmd) → on a Cmd, the unified apply funnel (journaled-or-in-
    /// memory, keyed by [`Principal`]) → `on_applied` (which branches on the
    /// [`ApplyOutcome`] kind for the fan-out). The lock-free heir of the old
    /// `handle_text`/`handle_text_2v2`, now ONE body — the seq-dedup, the journal
    /// dispatch, the metrics funnel, and the parse/version/malformed handling are
    /// mode-agnostic; only the Welcome frame (`welcome_frame`) and the post-apply
    /// fan-out differ by principal-kind.
    async fn on_text(&mut self, who: Principal, text: &str) -> Frame {
        // A principal whose kind doesn't match the mode — the router never produces
        // this, but answer with a typed error rather than mis-key the session.
        let mode_ok = matches!(
            (&self.config.mode, who),
            (ActorMode::OneVsOne { .. }, Principal::Seat(_))
                | (ActorMode::TwoVsTwo { .. }, Principal::Slot(_))
        );
        if !mode_ok {
            crate::metrics::frame_rejected(crate::metrics::RejectReason::ModeMismatch);
            return serde_json::to_string(&ServerMsg::Error {
                v: PROTOCOL_VERSION,
                message: "mode_mismatch".into(),
            })
            .unwrap();
        }
        let msg: Result<ClientMsg, _> = serde_json::from_str(text);
        let reply = match msg {
            Ok(ClientMsg::Ping { .. }) => ServerMsg::Pong {
                v: PROTOCOL_VERSION,
            },
            Ok(ClientMsg::Hello { name, .. }) => {
                // A re-Hello on a live socket refreshes the view (the transport
                // already routed the principal); fold in any updated 1v1 display name.
                // `welcome_frame` returns the serialized frame directly.
                self.stash_name(who, name);
                return self.welcome_frame(who);
            }
            Ok(ClientMsg::Cmd { v, seq, command }) if v == PROTOCOL_VERSION => {
                let has_bot = matches!(&self.config.mode, ActorMode::OneVsOne { bot: Some(_), .. });
                let started = std::time::Instant::now();
                // Postgres-authoritative when a journal connection is present
                // (append-before-ack); else the in-memory degrade. The session
                // owns the one `.await`; the actor is its sole, lock-free caller.
                let outcome = match self.config.event_client.as_ref() {
                    Some(client) => {
                        let store = AsyncStore::attach(client.as_ref(), self.config.db_id.clone());
                        self.session
                            .apply_journaled(&store, who, seq, command)
                            .await
                    }
                    None => self.session.apply(who, seq, command),
                };
                crate::metrics::command_applied(
                    outcome.is_ok(),
                    started.elapsed().as_millis() as u64,
                );
                match outcome {
                    Ok(out) => return self.on_applied(who, seq, out, has_bot).await,
                    Err(e) => reject(seq, e),
                }
            }
            Ok(ClientMsg::Cmd { seq, .. }) => {
                crate::metrics::frame_rejected(
                    crate::metrics::RejectReason::UnsupportedProtocolVersion,
                );
                ServerMsg::Rejected {
                    v: PROTOCOL_VERSION,
                    seq,
                    reason: "unsupported_protocol_version".into(),
                }
            }
            Err(_) => {
                crate::metrics::frame_rejected(crate::metrics::RejectReason::MalformedMessage);
                ServerMsg::Error {
                    v: PROTOCOL_VERSION,
                    message: "malformed_message".into(),
                }
            }
        };
        serde_json::to_string(&reply).unwrap()
    }

    /// Push a frame to one principal's socket. A send error means that socket's
    /// task has ended; we clear the slot so we stop addressing a dead receiver
    /// (and so a reconnect re-registers cleanly).
    fn push(&mut self, who: Principal, frame: Frame) {
        let idx = who.idx();
        if let Some(tx) = &self.seats[idx]
            && tx.send(frame).is_err()
        {
            self.seats[idx] = None;
        }
    }

    /// Stash a connecting/​re-Hello'ing principal's optional session display name (1v1
    /// only — slots carry no name). Clamped defensively (the untrusted name is already
    /// ≤16 KB from the frame cap); when a UI renders it, it MUST use `textContent`.
    /// Shared by `on_subscribe` and the Hello refresh so the clamp lives in one place.
    fn stash_name(&mut self, who: Principal, name: Option<String>) {
        if let (ActorMode::OneVsOne { names, .. }, Principal::Seat(seat), Some(n)) =
            (&mut self.config.mode, who, &name)
        {
            names[seat as usize] = Some(n.chars().take(40).collect());
        }
    }

    /// The full current redacted view for a principal — its [`ServerMsg::Welcome`]
    /// (1v1, with the public seat-name roster) or [`ServerMsg::TeamWelcome`] (2v2),
    /// serialized. The one place the welcome frame is built, shared by `on_subscribe`
    /// (the join/resume) and the Hello-refresh; branches once on principal-kind.
    fn welcome_frame(&self, who: Principal) -> Frame {
        match who {
            Principal::Seat(seat) => {
                let names = match &self.config.mode {
                    ActorMode::OneVsOne { names, .. } => names.to_vec(),
                    // A seat principal only ever reaches here in 1v1 (the mode guard
                    // upstream rejects a seat on a 2v2 lobby); answer safely regardless.
                    ActorMode::TwoVsTwo { .. } => vec![None, None],
                };
                serde_json::to_string(&ServerMsg::Welcome {
                    v: PROTOCOL_VERSION,
                    seat,
                    view: self.session.view(seat),
                    legal: self.session.legal_labeled(seat),
                    seat_names: names,
                })
                .unwrap()
            }
            Principal::Slot(slot) => serde_json::to_string(&ServerMsg::TeamWelcome {
                v: PROTOCOL_VERSION,
                slot,
                view: self.session.view_slot(slot),
                legal: self.session.legal_labeled_slot(slot),
            })
            .unwrap(),
        }
    }

    /// After a successful apply, branch ONCE on the [`ApplyOutcome`] kind (the H4
    /// single seam): a 1v1 `Solo` fans the lone opponent its `Update`; a 2v2 `Team`
    /// fans the three non-acting slots their `TeamUpdate`s. Both close the record +
    /// reveal the seed on a finish (shared `on_finish`). Returns the acting
    /// principal's own ack frame (`Applied`/`TeamApplied`).
    async fn on_applied(
        &mut self,
        who: Principal,
        seq: u64,
        out: ApplyOutcome,
        has_bot: bool,
    ) -> Frame {
        match (who, out) {
            (Principal::Seat(seat), ApplyOutcome::Solo(ok)) => {
                let reply = self.on_applied_1v1(seat, seq, *ok, has_bot).await;
                serde_json::to_string(&reply).unwrap()
            }
            (Principal::Slot(slot), ApplyOutcome::Team(ok)) => {
                self.on_applied_2v2(slot, seq, *ok).await
            }
            // The funnel pairs a Seat with Solo and a Slot with Team by construction;
            // a mismatch is unreachable, but answer with a typed error over a panic.
            _ => serde_json::to_string(&ServerMsg::Error {
                v: PROTOCOL_VERSION,
                message: "mode_mismatch".into(),
            })
            .unwrap(),
        }
    }

    /// After a successful 1v1 apply: drive the bot (vs-bot), push the opponent's
    /// Update (PvP), emit the seed reveal + close the record on finish, and return
    /// the acting seat's `Applied`.
    async fn on_applied_1v1(
        &mut self,
        seat: Seat,
        seq: u64,
        ok: ApplyOk,
        has_bot: bool,
    ) -> ServerMsg {
        // vs-bot: let the AI take its whole turn before we ack, so the human's
        // returned view already reflects the reply (no human opponent to push to).
        let (your_view, finished, other) = if has_bot {
            self.drive_bots().await;
            (self.session.view(seat), self.session.finished(), None)
        } else {
            (
                ok.your_view,
                ok.finished,
                Some((ok.other_seat, ok.other_view)),
            )
        };
        if let Some(result) = &finished {
            self.on_finish(
                result.clone(),
                self.faction_1v1(),
                crate::metrics::FinishReason::PlayedOut,
            );
        }
        // Push the opponent's fresh Update (PvP only) — per-seat, never broadcast.
        if let Some((other_seat, other_view)) = other {
            let push = ServerMsg::Update {
                v: PROTOCOL_VERSION,
                view: other_view,
                legal: self.session.legal_labeled(other_seat),
            };
            self.push(
                Principal::Seat(other_seat),
                serde_json::to_string(&push).unwrap(),
            );
        }
        // Commit–reveal: once the terminal views are out, disclose the seed +
        // salt to BOTH seats (after the Update, so the finished view lands first).
        // The seed reaches a client only HERE, at the end — never a view/event.
        if finished.is_some() {
            self.emit_seed_reveal(&[Principal::Seat(Seat::A), Principal::Seat(Seat::B)]);
        }
        ServerMsg::Applied {
            v: PROTOCOL_VERSION,
            seq,
            view: your_view,
            legal: self.session.legal_labeled(seat),
        }
    }

    /// The `faction` label for a 1v1 finish — the bot's faction (a PvE result), else "pvp".
    /// Shared by the played-out and absence-forfeit finishes so the `matches.finished`
    /// faction cut stays consistent across both (a vs-bot human-abandon is still PvE).
    fn faction_1v1(&self) -> &'static str {
        match &self.config.mode {
            ActorMode::OneVsOne { bot: Some(b), .. } => {
                if matches!(b.faction, recollect_bot::Faction::Solace) {
                    "solace"
                } else {
                    "lorekeeper"
                }
            }
            _ => "pvp",
        }
    }

    /// Log the finish, count it (with the §16 contraction-leader correlation + the
    /// played-out/abandoned `reason`), and close the `matches` record (best-effort,
    /// spawned). Shared by the 1v1 and 2v2 post-apply handlers AND the absence-forfeit
    /// path — `faction` is the only per-mode datum (the bot's faction in 1v1; "lorekeeper"
    /// in 2v2 until the P4 Solace pair), `reason` distinguishes a forfeit. The actor task
    /// lives until its sockets close; the map's `evict` flag lets the sweeper reclaim it.
    fn on_finish(&self, result: String, faction: &str, reason: crate::metrics::FinishReason) {
        tracing::info!(result = %result, reason = ?reason, "match finished");
        // §16: correlate the contraction leader (captured as the match ran) with
        // this result, so the winrate-when-leading-at-contraction panel resolves.
        crate::metrics::match_finished(&result, faction, self.session.contraction_lead(), reason);
        if let Some(j) = self.config.journal.clone() {
            let (mid, res) = (self.config.db_id.clone(), result);
            tokio::spawn(async move {
                if let Err(e) = j.lock().await.finish_match(&mid, &res).await {
                    tracing::warn!(error = %e, "finish_match update failed");
                }
            });
        }
    }

    /// After a successful 2v2 slot apply: drive any bot slots, emit the reveal on
    /// finish, push the three non-acting slot views per-seat, and return the acting
    /// slot's `TeamApplied`.
    async fn on_applied_2v2(&mut self, slot: SeatSlot, seq: u64, ok: ApplyOkSlot) -> Frame {
        let ok = self.drive_bots_2v2(ok).await;
        if let Some(result) = &ok.finished {
            // 2v2 today is Lorekeeper-only until the P4 Solace pair.
            self.on_finish(
                result.clone(),
                "lorekeeper",
                crate::metrics::FinishReason::PlayedOut,
            );
            // Commit–reveal: disclose the seed + salt to all four slots.
            self.emit_seed_reveal(&[
                Principal::Slot(SeatSlot::A1),
                Principal::Slot(SeatSlot::B1),
                Principal::Slot(SeatSlot::A2),
                Principal::Slot(SeatSlot::B2),
            ]);
        }
        // Redaction by construction: each slot gets its OWN fresh TeamView. The
        // acting slot's comes back as the ack; the other three are pushed per-seat.
        let mut mine = String::new();
        for (s2, view) in ok.views {
            let legal = self.session.legal_labeled_slot(s2);
            if s2 == slot {
                mine = serde_json::to_string(&ServerMsg::TeamApplied {
                    v: PROTOCOL_VERSION,
                    seq,
                    slot: s2,
                    view,
                    legal,
                })
                .unwrap();
            } else {
                let push = ServerMsg::TeamUpdate {
                    v: PROTOCOL_VERSION,
                    slot: s2,
                    view,
                    legal,
                };
                self.push(Principal::Slot(s2), serde_json::to_string(&push).unwrap());
            }
        }
        mine
    }

    /// Emit the seed reveal to the listed principals, exactly once per match. The
    /// seed reaches a client only here, at match end (never a view or in-flight
    /// event) — the determinism + redaction invariants hold throughout play.
    fn emit_seed_reveal(&mut self, to: &[Principal]) {
        if self.revealed {
            return;
        }
        self.revealed = true;
        let r = self.config.seed_commit.reveal();
        let reveal = serde_json::to_string(&ServerMsg::SeedRevealed {
            v: PROTOCOL_VERSION,
            seed: r.seed,
            salt_hex: r.salt_hex,
            commit_hex: r.commit_hex,
        })
        .unwrap();
        for &p in to {
            self.push(p, reveal.clone());
        }
    }

    /// Play any pending bot OPENER before a seat's first view — the unified
    /// welcome-drive. The seeded opener toss (`decide_opener` /
    /// `decide_opener_2v2`) can land on a bot seat/slot; if so the human, on
    /// subscribing, would otherwise see the opponent's turn with no legal moves of
    /// its own and be stranded on the welcome. One call dispatches on mode so
    /// the drive lives in ONE place: 1v1 drives the Seat-B bot if it opened; 2v2
    /// drives any consecutive bot slots from the opening state. A no-op when the
    /// opener is human, when there is no bot, or once it is the human's turn.
    /// Idempotent and safe on a reconnect (it only acts while it is a bot's turn,
    /// which it never is once the human is up).
    async fn drive_openers(&mut self) {
        self.drive_bots().await;
    }

    /// Drive every consecutive bot turn from the CURRENT state — the welcome/opening
    /// path AND the 1v1 post-command drive. ONE loop over [`step_bot`](Self::step_bot),
    /// which branches on mode internally: 1v1 rolls the Seat-B bot through its turn(s); 2v2
    /// rolls any consecutive bot slots (B1→A2→B2…). The intermediate fan-out views are
    /// discarded — the caller re-reads the fresh `view`/`view_slot` afterwards. Stops
    /// when the active principal is human, when there are no bots, when a step fails,
    /// or when the match finishes. The `~256` bound guards a chooser that fails to
    /// advance the turn; the `std` RNG mutex never crosses the journal `.await` (it is
    /// dropped inside `step_bot` before the append).
    async fn drive_bots(&mut self) {
        for _ in 0..256 {
            match self.step_bot().await {
                // A bot acted and the match continues — keep driving.
                BotStep::Acted(ok) if ok.as_ref().is_none_or(|o| o.finished.is_none()) => continue,
                // Human turn / no bots / a failure / a finishing move — stop.
                _ => break,
            }
        }
    }

    /// Drive any consecutive 2v2 bot slots AFTER a slot command, threading the
    /// `ApplyOkSlot` so the caller gets the final fan-out views + finish flag (the 2v2
    /// post-command path needs those views; the welcome path discards them, hence the
    /// separate-but-shared loop). Reuses the one [`step_bot`](Self::step_bot).
    async fn drive_bots_2v2(&mut self, mut ok: ApplyOkSlot) -> ApplyOkSlot {
        if ok.finished.is_some() {
            return ok;
        }
        for _ in 0..256 {
            match self.step_bot().await {
                // A bot slot acted: keep its fresh fan-out result; stop once finished.
                // (In 2v2 `step_bot` always carries the views, so the `else` is dead —
                // it just keeps the last good `ok` defensively.)
                BotStep::Acted(Some(next)) => {
                    let done = next.finished.is_some();
                    ok = *next;
                    if done {
                        break;
                    }
                }
                // The active slot is human, there are no bots, a step failed, or (1v1,
                // which never reaches here) had no slot views — hand back the last result.
                _ => break,
            }
        }
        ok
    }

    /// One bot turn, dispatching ONCE on mode (the H4 single-branch seam): if the
    /// engine's active principal is a bot, choose its command and apply it (durable
    /// when journaled, else in-memory). [`BotStep::Idle`] ⇒ the active principal is
    /// human, there are no bots, or the match is over — nothing to drive.
    /// [`BotStep::Failed`] ⇒ a rule rejection (chooser bug) or a journal failure; the
    /// caller stops rather than spin. [`BotStep::Acted`] carries the fresh `ApplyOkSlot`
    /// in 2v2 (for the post-command fan-out) and `None` in 1v1 (the caller re-reads its
    /// own view). The `std` RNG mutex is released before the journal `.await` (it never
    /// crosses an await point).
    async fn step_bot(&mut self) -> BotStep {
        match &self.config.mode {
            // 1v1: the lone Seat-B bot, while it is its turn and the match continues.
            ActorMode::OneVsOne { bot: Some(bot), .. } => {
                let (bot_seat, difficulty, faction) = (bot.seat, bot.difficulty, bot.faction);
                if self.session.is_finished() || self.session.active_seat() != bot_seat {
                    return BotStep::Idle;
                }
                let cmd = {
                    let ActorMode::OneVsOne { bot: Some(bot), .. } = &self.config.mode else {
                        return BotStep::Idle;
                    };
                    let mut rng = bot.rng.lock().unwrap();
                    recollect_bot::choose_as(
                        self.session.engine(),
                        bot_seat,
                        difficulty,
                        faction,
                        recollect_bot::Faction::Lorekeeper,
                        &mut rng,
                    )
                };
                let res = match self.config.event_client.as_ref() {
                    Some(client) => {
                        let store = AsyncStore::attach(client.as_ref(), self.config.db_id.clone());
                        self.session
                            .apply_journaled_server(&store, Principal::Seat(bot_seat), cmd)
                            .await
                    }
                    None => self.session.apply_server(Principal::Seat(bot_seat), cmd),
                };
                // 1v1 yields a `Solo`; the caller re-reads its own view, so we drop it.
                match res {
                    Ok(_) => BotStep::Acted(None),
                    Err(_) => BotStep::Failed,
                }
            }
            // 2v2: any consecutive bot slot whose turn it now is (B1→A2→B2…).
            ActorMode::TwoVsTwo { bots } => {
                if bots.is_empty() {
                    return BotStep::Idle;
                }
                let slot = self.session.engine().state().active_slot;
                let Some((_, bot)) = bots.iter().find(|(sl, _)| *sl == slot) else {
                    return BotStep::Idle;
                };
                let (difficulty, faction, opp_faction) =
                    (bot.difficulty, bot.faction, bot.opp_faction);
                let cmd = {
                    let mut rng = bot.rng.lock().unwrap();
                    recollect_bot::choose_as(
                        self.session.engine(),
                        slot.team(),
                        difficulty,
                        faction,
                        opp_faction,
                        &mut rng,
                    )
                };
                let res = match self.config.event_client.as_ref() {
                    Some(client) => {
                        let store = AsyncStore::attach(client.as_ref(), self.config.db_id.clone());
                        self.session
                            .apply_journaled_server(&store, Principal::Slot(slot), cmd)
                            .await
                    }
                    None => self.session.apply_server(Principal::Slot(slot), cmd),
                };
                // 2v2 yields a `Team` — thread its (boxed) four-slot fan-out along.
                match res {
                    Ok(ApplyOutcome::Team(ok)) => BotStep::Acted(Some(ok)),
                    // A `Solo` from a slot is unreachable (the funnel pairs Slot↔Team);
                    // treat any non-Team as a no-op step rather than panic.
                    Ok(ApplyOutcome::Solo(_)) => BotStep::Acted(None),
                    Err(_) => BotStep::Failed,
                }
            }
            // 1v1 with no bot attached (PvP) — never a bot's turn.
            ActorMode::OneVsOne { bot: None, .. } => BotStep::Idle,
        }
    }
}

/// The outcome of one [`MatchActor::step_bot`] turn — the uniform result the two
/// bot-drive loops (welcome + 2v2 post-command) share across modes.
enum BotStep {
    /// The active principal isn't a bot (human, no bots), or the match is over.
    Idle,
    /// A bot acted. 2v2 carries the fresh four-slot fan-out (`Some`) for the
    /// post-command push; 1v1 re-reads its own view, so it's `None`. Boxed: the
    /// four-slot result is large, so the box keeps the enum small.
    Acted(Option<Box<ApplyOkSlot>>),
    /// A rule rejection (chooser bug) or a journal failure — stop, don't spin.
    Failed,
}

/// Map an [`ApplyErr`] to its `Rejected` frame — shared by the 1v1 and 2v2 apply
/// paths (H4: one funnel, no per-mode twin). The acting principal is keyed by SEAT
/// vs SLOT upstream; the rejection itself is mode-agnostic (same `seq`, same reasons),
/// so it has exactly one definition. The actor's `#[instrument]` run span already
/// carries `db_id`, so the Journal-failure log needs no mode prefix to be traceable.
fn reject(seq: u64, e: ApplyErr) -> ServerMsg {
    match e {
        ApplyErr::StaleOrReplayedSeq => ServerMsg::Rejected {
            v: PROTOCOL_VERSION,
            seq,
            reason: "stale_or_replayed_seq".into(),
        },
        ApplyErr::Rule(r) => ServerMsg::Rejected {
            v: PROTOCOL_VERSION,
            seq,
            reason: format!("{r:?}"),
        },
        ApplyErr::Journal(e) => {
            // The command was NOT made durable, so it did not take. The engine was
            // rewound; the client may retry the same seq.
            tracing::error!(error = %e, "authoritative append failed; command rejected");
            ServerMsg::Rejected {
                v: PROTOCOL_VERSION,
                seq,
                reason: "journal_unavailable".into(),
            }
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

    /// An in-memory (no-journal) 1v1 actor over a fixed seed — the degrade path,
    /// so these tests run in the normal suite. The absence forfeit is DISABLED
    /// (`abandon_grace == 0`) so a dropped receiver in the existing reconnect/fan-out
    /// tests never forfeits; the forfeit tests below use [`spawn_1v1_grace`].
    fn spawn_1v1(seed: u64, bot: Option<Bot>) -> MatchHandle {
        spawn_1v1_grace(seed, bot, std::time::Duration::ZERO)
    }

    /// As [`spawn_1v1`] but with an explicit absence-forfeit grace — the forfeit tests
    /// pass a tiny window (~100 ms) so a vacated seat forfeits fast.
    fn spawn_1v1_grace(
        seed: u64,
        bot: Option<Bot>,
        abandon_grace: std::time::Duration,
    ) -> MatchHandle {
        let session = Session::new(seed, deck(), deck());
        spawn(
            session,
            ActorConfig {
                mode: ActorMode::OneVsOne {
                    bot,
                    names: [None, None],
                },
                db_id: "test-1v1".into(),
                journal: None,
                event_client: None,
                seed_commit: SeedCommitment::new(seed),
                abandon_grace,
            },
        )
    }

    fn cmd(seq: u64, command: Command) -> String {
        serde_json::to_string(&ClientMsg::Cmd {
            v: PROTOCOL_VERSION,
            seq,
            command,
        })
        .unwrap()
    }

    fn parse(frame: &str) -> ServerMsg {
        serde_json::from_str(frame).unwrap()
    }

    /// The core actor property: a PvP command produces the acting seat's ack as the
    /// reply AND fans the opponent its own fresh `Update` over the opponent's
    /// per-seat sender — no broadcast, no shared channel. (Seat A opens on this seed.)
    #[tokio::test]
    async fn pvp_command_acks_the_actor_and_pushes_the_opponent_per_seat() {
        let h = spawn_1v1(7, None);
        let (a_tx, mut a_rx) = mpsc::unbounded_channel();
        let (b_tx, mut b_rx) = mpsc::unbounded_channel();
        // Both seats subscribe; each gets its own Welcome.
        let wa = h
            .subscribe(Principal::Seat(Seat::A), a_tx, None)
            .await
            .unwrap();
        let wb = h
            .subscribe(Principal::Seat(Seat::B), b_tx, None)
            .await
            .unwrap();
        assert!(matches!(
            parse(&wa),
            ServerMsg::Welcome { seat: Seat::A, .. }
        ));
        assert!(matches!(
            parse(&wb),
            ServerMsg::Welcome { seat: Seat::B, .. }
        ));

        // Seat A ends its turn. The reply is A's Applied; B is pushed an Update.
        let ack = h
            .text(Principal::Seat(Seat::A), cmd(1, Command::EndTurn))
            .await
            .unwrap();
        match parse(&ack) {
            ServerMsg::Applied { seq, view, .. } => {
                assert_eq!(seq, 1);
                assert_eq!(
                    view.seat,
                    Seat::A,
                    "the ack carries the acting seat's own view"
                );
                assert_eq!(view.active, Seat::B, "EndTurn passed the turn");
            }
            other => panic!("expected Applied, got {other:?}"),
        }
        // B's per-seat channel carries exactly its own Update (redaction-by-construction).
        let push = b_rx.try_recv().expect("the opponent was pushed an Update");
        match parse(&push) {
            ServerMsg::Update { view, .. } => {
                assert_eq!(view.seat, Seat::B, "the push is B's OWN view, never A's");
                assert_eq!(view.active, Seat::B);
            }
            other => panic!("expected Update, got {other:?}"),
        }
        // A's own channel got NO fan-out frame (it acted; its ack was the reply).
        assert!(
            a_rx.try_recv().is_err(),
            "the acting seat gets no extra push"
        );
    }

    /// No drop class: a wedged/dropped opponent receiver does not block or fail the
    /// acting seat. The opponent simply has no live sender and the acting seat is
    /// acked regardless.
    #[tokio::test]
    async fn a_dropped_opponent_receiver_does_not_block_the_acting_seat() {
        let h = spawn_1v1(7, None);
        let (a_tx, _a_rx) = mpsc::unbounded_channel();
        let (b_tx, b_rx) = mpsc::unbounded_channel();
        h.subscribe(Principal::Seat(Seat::A), a_tx, None)
            .await
            .unwrap();
        h.subscribe(Principal::Seat(Seat::B), b_tx, None)
            .await
            .unwrap();
        // B's socket goes away (its receiver dropped) — as a lagged/closed consumer.
        drop(b_rx);
        // A can still act and is acked; the actor clears B's dead sender internally.
        let ack = h
            .text(Principal::Seat(Seat::A), cmd(1, Command::EndTurn))
            .await
            .expect("the acting seat is acked even though the opponent is gone");
        assert!(matches!(parse(&ack), ServerMsg::Applied { .. }));
    }

    /// The acting seat's stale/replayed seq is rejected through the actor (the
    /// session's anti-replay guard, surfaced as a `Rejected` reply frame).
    #[tokio::test]
    async fn the_actor_rejects_a_replayed_seq() {
        let h = spawn_1v1(7, None);
        let (a_tx, _rx) = mpsc::unbounded_channel();
        h.subscribe(Principal::Seat(Seat::A), a_tx, None)
            .await
            .unwrap();
        let _ = h
            .text(Principal::Seat(Seat::A), cmd(1, Command::Glimpse))
            .await;
        let replay = h
            .text(Principal::Seat(Seat::A), cmd(1, Command::EndTurn))
            .await
            .unwrap();
        match parse(&replay) {
            ServerMsg::Rejected { reason, .. } => assert_eq!(reason, "stale_or_replayed_seq"),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    /// Transport/session fuzz (BEYOND the engine's `decide` fuzz): a battery of
    /// HOSTILE WIRE FRAMES through the live actor seam must each get a typed reply,
    /// never panic the actor, never desync the authoritative state, and never wedge the
    /// socket. The engine fuzz proves `decide` is total over arbitrary `Command`s; this proves the
    /// layer ABOVE it — the JSON parse, the protocol-version gate, and the apply/ack
    /// path — is equally total over arbitrary BYTES. The cases:
    ///   - non-JSON / truncated / wrong-type / unknown-tag frames → `malformed_message`
    ///     (the actor parses into a `Result`, never `unwrap`s untrusted input);
    ///   - a `Cmd` with the wrong protocol `v` → `unsupported_protocol_version`;
    ///   - a structurally-valid but out-of-turn / illegal command → `Rejected` (a rule
    ///     reject, surfaced — not applied);
    /// and through ALL of it the session's view never advances (no hostile frame moved
    /// authoritative state), and a legal command still applies afterward (the actor
    /// is not wedged — exactly-one-writer survived the assault).
    #[tokio::test]
    async fn hostile_wire_frames_never_panic_desync_or_wedge_the_actor() {
        let h = spawn_1v1(7, None); // Seat A opens on seed 7; Seat B is human (PvP).
        let (a_tx, _a_rx) = mpsc::unbounded_channel();
        let (b_tx, _b_rx) = mpsc::unbounded_channel();
        let welcome = h
            .subscribe(Principal::Seat(Seat::A), a_tx, None)
            .await
            .unwrap();
        h.subscribe(Principal::Seat(Seat::B), b_tx, None)
            .await
            .unwrap();
        // The authoritative opening view (active seat etc.) — nothing hostile may move it.
        let view0 = match parse(&welcome) {
            ServerMsg::Welcome { view, .. } => view,
            other => panic!("expected Welcome, got {other:?}"),
        };
        let snapshot = serde_json::to_string(&view0).unwrap();

        // (a) Garbage that must parse-reject with `malformed_message`.
        let malformed = [
            "",                                                                // empty frame
            "}{",                                                              // not JSON
            "{",                                                               // truncated object
            "[1,2,3]",                                                         // JSON, wrong shape
            "\"just a string\"",                                               // JSON scalar
            "null",                                                            // JSON null
            "{\"t\":\"cmd\"}",                     // tagged but missing fields
            "{\"t\":\"unknown_variant\",\"v\":1}", // unknown tag
            "{\"t\":\"cmd\",\"v\":\"NaN\",\"seq\":1,\"command\":\"EndTurn\"}", // wrong type for v
            "{\"t\":\"cmd\",\"v\":1,\"seq\":-1,\"command\":\"EndTurn\"}", // negative seq (u64)
            "\u{0}\u{1}\u{2}not text",             // control bytes
            &"x".repeat(4096),                     // a large non-JSON blob
        ];
        for frame in malformed {
            let reply = h
                .text(Principal::Seat(Seat::A), frame.to_string())
                .await
                .expect("the actor replies (never drops) to a malformed frame");
            match parse(&reply) {
                ServerMsg::Error { message, .. } => assert_eq!(
                    message, "malformed_message",
                    "a malformed frame is a typed error, never a panic: {frame:?}"
                ),
                other => panic!("expected an Error for {frame:?}, got {other:?}"),
            }
        }

        // (b) A well-formed Cmd with the WRONG protocol version → version reject.
        let bad_ver = serde_json::to_string(&ClientMsg::Cmd {
            v: PROTOCOL_VERSION + 9,
            seq: 1,
            command: Command::EndTurn,
        })
        .unwrap();
        match parse(&h.text(Principal::Seat(Seat::A), bad_ver).await.unwrap()) {
            ServerMsg::Rejected { reason, .. } => {
                assert_eq!(reason, "unsupported_protocol_version")
            }
            other => panic!("expected a version Rejected, got {other:?}"),
        }

        // (c) A structurally-valid but OUT-OF-TURN command (Seat B acting on A's turn)
        // → a rule Rejected, surfaced, not applied.
        match parse(
            &h.text(Principal::Seat(Seat::B), cmd(1, Command::EndTurn))
                .await
                .unwrap(),
        ) {
            ServerMsg::Rejected { .. } => {}
            other => panic!("expected an out-of-turn Rejected, got {other:?}"),
        }

        // No hostile frame moved the authoritative state — the view is byte-identical.
        let now = match parse(
            &h.text(
                Principal::Seat(Seat::A),
                serde_json::to_string(&ClientMsg::Ping {
                    v: PROTOCOL_VERSION,
                })
                .unwrap(),
            )
            .await
            .unwrap(),
        ) {
            // Ping → Pong proves the actor is responsive; then read the view via Hello.
            ServerMsg::Pong { .. } => {
                let hello = serde_json::to_string(&ClientMsg::Hello {
                    v: PROTOCOL_VERSION,
                    match_token: String::new(),
                    name: None,
                    session_id: None,
                })
                .unwrap();
                match parse(&h.text(Principal::Seat(Seat::A), hello).await.unwrap()) {
                    ServerMsg::Welcome { view, .. } => view,
                    other => panic!("expected a refreshed Welcome, got {other:?}"),
                }
            }
            other => panic!("expected Pong, got {other:?}"),
        };
        assert_eq!(
            serde_json::to_string(&now).unwrap(),
            snapshot,
            "no hostile frame advanced the authoritative state — no desync"
        );

        // The actor is NOT wedged: a legal command from the active seat still applies.
        match parse(
            &h.text(Principal::Seat(Seat::A), cmd(1, Command::EndTurn))
                .await
                .unwrap(),
        ) {
            ServerMsg::Applied { view, .. } => {
                assert_eq!(view.active, Seat::B, "the legal EndTurn passed the turn")
            }
            other => panic!("the actor wedged after the assault; got {other:?}"),
        }
    }

    /// vs-bot: a single human seat subscribes, the actor drives Seat B itself after
    /// the human's move, and the match reaches a result with only one socket — which
    /// it could not unless the actor were driving B. The end-of-match SeedRevealed
    /// reaches the human over its per-seat channel.
    #[tokio::test]
    async fn vs_bot_actor_drives_seat_b_and_reveals_the_seed_at_the_end() {
        use recollect_core::state::Phase;
        // A bot on Seat B; the human is Seat A. (Seat A opens on seed 7.)
        let bot = Bot {
            seat: Seat::B,
            difficulty: recollect_bot::Difficulty::Normal,
            faction: recollect_bot::Faction::Lorekeeper,
            rng: std::sync::Mutex::new(recollect_core::rng::Rng::from_seed(7 ^ 0xB07)),
        };
        let h = spawn_1v1(7, Some(bot));
        let (a_tx, mut a_rx) = mpsc::unbounded_channel();
        let welcome = h
            .subscribe(Principal::Seat(Seat::A), a_tx, None)
            .await
            .unwrap();
        let mut view = match parse(&welcome) {
            ServerMsg::Welcome { view, .. } => view,
            other => panic!("expected Welcome, got {other:?}"),
        };
        let pick = |phase: &Phase| match phase {
            Phase::PendingRelease { .. } => Command::Release { hand_index: 0 },
            Phase::PendingChoice { .. } => Command::Choose { index: 0 },
            _ => Command::EndTurn,
        };
        let mut finished = false;
        for seq in 1u64..=400 {
            if matches!(view.phase, Phase::Finished { .. }) {
                finished = true;
                break;
            }
            assert_eq!(
                view.active,
                Seat::A,
                "the actor drove B's turn; it is ours again"
            );
            let ack = h
                .text(Principal::Seat(Seat::A), cmd(seq, pick(&view.phase)))
                .await
                .unwrap();
            view = match parse(&ack) {
                ServerMsg::Applied { view, .. } => view,
                ServerMsg::Rejected { reason, .. } => panic!("rejected mid-game: {reason}"),
                other => panic!("expected Applied, got {other:?}"),
            };
        }
        assert!(
            finished,
            "the vs-bot match reached a result within 400 turns"
        );
        // The seed reveal was pushed to the human's per-seat channel at the end.
        let mut saw_reveal = false;
        while let Ok(frame) = a_rx.try_recv() {
            if matches!(parse(&frame), ServerMsg::SeedRevealed { .. }) {
                saw_reveal = true;
            }
        }
        assert!(saw_reveal, "the end-of-match seed reveal reached the seat");
    }

    // --- first-class reconnection ----------------------------------------
    // The actor outlives any one socket; a dropped seat rejoins by re-subscribing
    // (a fresh sender for the same Principal), and the welcome carries the FULL
    // current view, so the resumed client is consistent in one frame. (The token
    // gate is in `ws.rs`/`crypto.rs` and tested there + in lib.rs end-to-end; at
    // the actor seam a `Subscribe` is already authorised.)

    /// Drop → rejoin → consistent view: a seat plays a move, its socket drops, it
    /// re-subscribes, and the resume Welcome reflects the move that happened
    /// BEFORE the drop — the authoritative state, not a stale snapshot. (Seat A
    /// opens on seed 7.)
    #[tokio::test]
    async fn a_dropped_seat_rejoins_and_the_welcome_carries_the_current_state() {
        let h = spawn_1v1(7, None);
        let (a_tx, a_rx) = mpsc::unbounded_channel();
        let (b_tx, _b_rx) = mpsc::unbounded_channel();
        h.subscribe(Principal::Seat(Seat::A), a_tx, None)
            .await
            .unwrap();
        h.subscribe(Principal::Seat(Seat::B), b_tx, None)
            .await
            .unwrap();

        // A acts: end its turn. The state now has B active; A's seq is at 1.
        let ack = h
            .text(Principal::Seat(Seat::A), cmd(1, Command::EndTurn))
            .await
            .unwrap();
        assert!(matches!(parse(&ack), ServerMsg::Applied { .. }));

        // A's socket drops (its receiver goes away) — a mid-match disconnect.
        drop(a_rx);

        // A reconnects: a brand-new sender for the SAME principal. The actor
        // replies with a fresh Welcome that already reflects the pre-drop EndTurn.
        let (a_tx2, mut a_rx2) = mpsc::unbounded_channel();
        let welcome = h
            .subscribe(Principal::Seat(Seat::A), a_tx2, None)
            .await
            .expect("the actor outlived the dropped socket and re-welcomes A");
        match parse(&welcome) {
            ServerMsg::Welcome { seat, view, .. } => {
                assert_eq!(seat, Seat::A, "the resume re-seats A on its own seat");
                assert_eq!(
                    view.active,
                    Seat::B,
                    "the resume view reflects the move made before the drop — \
                     it is the authoritative current state, not a stale snapshot"
                );
                assert_eq!(view.seat, Seat::A, "the resumed view is A's own (redacted)");
            }
            other => panic!("expected a resume Welcome, got {other:?}"),
        }

        // And the resumed socket is live: B acting now pushes A's Update to the
        // NEW sender (the actor re-addressed fan-out to the reconnected seat).
        // B is active; end its turn to fan A an Update.
        let _ = h
            .text(Principal::Seat(Seat::B), cmd(1, Command::EndTurn))
            .await
            .unwrap();
        let push = a_rx2
            .try_recv()
            .expect("fan-out now reaches the reconnected socket");
        match parse(&push) {
            ServerMsg::Update { view, .. } => {
                assert_eq!(view.seat, Seat::A, "the push is A's own view");
                assert_eq!(view.active, Seat::A, "the turn came back to A");
            }
            other => panic!("expected an Update on the resumed socket, got {other:?}"),
        }
    }

    /// A reconnect SUPERSEDES the stale socket: with a live sender still registered
    /// for the seat, a fresh subscribe replaces it, dropping the old sender — so the
    /// old socket's receiver closes (its `socket_loop` would then tear down) and only
    /// the newest socket receives fan-out. This is the "exactly one live socket per
    /// seat" guarantee that makes a double-tab / flaky-network rejoin clean.
    #[tokio::test]
    async fn a_reconnect_supersedes_the_previous_socket_and_only_the_newest_is_fed() {
        let h = spawn_1v1(7, None);
        let (a_tx, mut a_rx_old) = mpsc::unbounded_channel();
        let (b_tx, _b_rx) = mpsc::unbounded_channel();
        h.subscribe(Principal::Seat(Seat::A), a_tx, None)
            .await
            .unwrap();
        h.subscribe(Principal::Seat(Seat::B), b_tx, None)
            .await
            .unwrap();

        // A reconnects on a NEW socket WITHOUT the old one having torn down (the
        // common drop-detection race). The new subscribe supersedes the old sender.
        let (a_tx2, mut a_rx_new) = mpsc::unbounded_channel();
        h.subscribe(Principal::Seat(Seat::A), a_tx2, None)
            .await
            .unwrap();

        // The old receiver is now closed — the actor dropped its sender on replace.
        // (A closed channel returns Disconnected, not Empty; the old socket's
        // `rx.recv()` would yield `None` and the loop would return.)
        assert!(
            matches!(
                a_rx_old.try_recv(),
                Err(mpsc::error::TryRecvError::Disconnected)
            ),
            "the superseded socket's channel is closed, so the stale socket tears down"
        );

        // Drive a fan-out to A: A ends its turn (its ack is the reply), then B ends
        // its turn so A is pushed an Update — it must land on the NEW socket only.
        let _ = h
            .text(Principal::Seat(Seat::A), cmd(1, Command::EndTurn))
            .await;
        let _ = h
            .text(Principal::Seat(Seat::B), cmd(1, Command::EndTurn))
            .await;
        let push = a_rx_new
            .try_recv()
            .expect("the newest socket receives the fan-out");
        assert!(matches!(parse(&push), ServerMsg::Update { .. }));
    }

    /// 2v2 reconnection: a slot drops and rejoins, and its resume `TeamWelcome`
    /// carries the current 6×6 board reflecting a move made while it was away —
    /// the same first-class flow as 1v1, over the slot seam.
    #[tokio::test]
    async fn a_2v2_slot_rejoins_and_resumes_the_current_team_board() {
        let session = Session::new_2v2(7, [deck(), deck(), deck(), deck()]);
        let h = spawn(
            session,
            ActorConfig {
                mode: ActorMode::TwoVsTwo { bots: Vec::new() },
                db_id: "test-2v2".into(),
                journal: None,
                event_client: None,
                seed_commit: SeedCommitment::new(7),
                abandon_grace: std::time::Duration::ZERO,
            },
        );
        // All four slots subscribe; capture A1's receiver and the active slot.
        let mut rxs = Vec::new();
        for slot in [SeatSlot::A1, SeatSlot::B1, SeatSlot::A2, SeatSlot::B2] {
            let (tx, rx) = mpsc::unbounded_channel();
            let w = h.subscribe(Principal::Slot(slot), tx, None).await.unwrap();
            rxs.push((slot, rx, w));
        }
        let opener = match parse(&rxs[0].2) {
            ServerMsg::TeamWelcome { view, .. } => view.active_slot,
            other => panic!("expected TeamWelcome, got {other:?}"),
        };
        // The opener ends its turn so the active slot advances.
        let ack = h
            .text(Principal::Slot(opener), cmd(1, Command::EndTurn))
            .await
            .unwrap();
        let advanced = match parse(&ack) {
            ServerMsg::TeamApplied { view, .. } => view.active_slot,
            other => panic!("expected TeamApplied, got {other:?}"),
        };
        assert_ne!(advanced, opener, "the slot rotation advanced");

        // The opener slot "drops" (its rx goes away) and rejoins; its resume
        // TeamWelcome must show the advanced active slot, not the pre-drop one.
        let opener_idx = rxs.iter().position(|(s, _, _)| *s == opener).unwrap();
        let _ = rxs.remove(opener_idx); // drop that slot's receiver
        let (tx2, _rx2) = mpsc::unbounded_channel();
        let welcome = h
            .subscribe(Principal::Slot(opener), tx2, None)
            .await
            .expect("the 2v2 actor re-welcomes the rejoining slot");
        match parse(&welcome) {
            ServerMsg::TeamWelcome { slot, view, .. } => {
                assert_eq!(slot, opener, "the slot resumes on its own seat");
                assert_eq!(view.board_w, 6, "the 6×6 team board");
                assert_eq!(
                    view.active_slot, advanced,
                    "the resume reflects the rotation that happened while away"
                );
            }
            other => panic!("expected a resume TeamWelcome, got {other:?}"),
        }
    }

    // --- the 2v2-vs-bot opening drive ------------------------------------
    // Parity with 1v1: when the seeded opener toss lands on a bot seat/slot, the
    // actor plays the bot opener BEFORE the human's first view, so the human is
    // never stranded on a TeamWelcome showing the opponent's turn with no moves.

    /// A server-driven 2v2 bot slot over a fixed rng (the in-memory degrade path).
    fn slot_bot() -> SlotBot {
        SlotBot {
            difficulty: recollect_bot::Difficulty::Normal,
            faction: recollect_bot::Faction::Lorekeeper,
            opp_faction: recollect_bot::Faction::Lorekeeper,
            rng: std::sync::Mutex::new(recollect_core::rng::Rng::from_seed(7 ^ 0xB07)),
        }
    }

    /// The opening-drive property: in a 2v2-vs-bot lobby whose opener is a BOT slot, the human
    /// slot's very first `TeamWelcome` already shows ITS OWN turn with a real legal
    /// menu — the actor drove the bot opener(s) on subscribe. Scenario: A1 is the
    /// lone human; A2, B1, B2 are bots; the toss opens B1. The drive must roll B1 →
    /// A2 → B2 (all bots) and stop at A1, so A1 is never stranded waiting on a turn
    /// it cannot take.
    #[tokio::test]
    async fn a_2v2_bot_opener_auto_plays_before_the_human_is_welcomed() {
        // Force the opener onto a bot slot (B1) — no seed search, deterministic.
        let session = Session::new_2v2_with_opener(
            7,
            [deck(), deck(), deck(), deck()],
            SeatSlot::B1,
            [recollect_core::types::Faction::Lorekeeper; 2],
        );
        // A1 human; A2, B1, B2 bots (the lone-human-vs-three-bots lobby).
        let bots = vec![
            (SeatSlot::B1, slot_bot()),
            (SeatSlot::A2, slot_bot()),
            (SeatSlot::B2, slot_bot()),
        ];
        let h = spawn(
            session,
            ActorConfig {
                mode: ActorMode::TwoVsTwo { bots },
                db_id: "test-2v2-bot-opener".into(),
                journal: None,
                event_client: None,
                seed_commit: SeedCommitment::new(7),
                abandon_grace: std::time::Duration::ZERO,
            },
        );

        // The human (A1) subscribes — the FIRST thing it ever sees.
        let (a1_tx, _a1_rx) = mpsc::unbounded_channel();
        let welcome = h
            .subscribe(Principal::Slot(SeatSlot::A1), a1_tx, None)
            .await
            .expect("the 2v2 actor welcomes the human slot");
        match parse(&welcome) {
            ServerMsg::TeamWelcome {
                slot, view, legal, ..
            } => {
                assert_eq!(slot, SeatSlot::A1, "the human is welcomed on its own slot");
                assert_eq!(
                    view.active_slot,
                    SeatSlot::A1,
                    "the bot opener (B1) and the bot allies (A2,B2) already played — \
                     the turn is the human's, NOT the opponent's; the human is not stranded"
                );
                assert!(
                    !legal.is_empty(),
                    "the human's first view carries its real legal menu — it can act immediately"
                );
            }
            other => panic!("expected a TeamWelcome, got {other:?}"),
        }
    }

    /// Control: with NO bots, the 2v2 opener drive is a no-op — a human opener is
    /// welcomed on its own turn unchanged (the drive must never advance past, or
    /// act for, a human seat). Guards against `drive_openers` over-driving.
    #[tokio::test]
    async fn a_2v2_human_opener_is_not_driven() {
        let session = Session::new_2v2_with_opener(
            7,
            [deck(), deck(), deck(), deck()],
            SeatSlot::A1,
            [recollect_core::types::Faction::Lorekeeper; 2],
        );
        // A bot ally exists (A2) but the opener (A1) is human — the drive must not
        // touch the state before A1's welcome.
        let bots = vec![(SeatSlot::A2, slot_bot())];
        let h = spawn(
            session,
            ActorConfig {
                mode: ActorMode::TwoVsTwo { bots },
                db_id: "test-2v2-human-opener".into(),
                journal: None,
                event_client: None,
                seed_commit: SeedCommitment::new(7),
                abandon_grace: std::time::Duration::ZERO,
            },
        );
        let (a1_tx, _a1_rx) = mpsc::unbounded_channel();
        let welcome = h
            .subscribe(Principal::Slot(SeatSlot::A1), a1_tx, None)
            .await
            .unwrap();
        match parse(&welcome) {
            ServerMsg::TeamWelcome { view, legal, .. } => {
                assert_eq!(
                    view.active_slot,
                    SeatSlot::A1,
                    "the human opener is welcomed on its own turn — the drive was a no-op"
                );
                assert!(!legal.is_empty(), "and with its legal menu intact");
            }
            other => panic!("expected a TeamWelcome, got {other:?}"),
        }
    }

    // --- absence forfeit (disconnect → grace → MatchAbandoned) ------------
    // The actor arms a grace timer when a HUMAN principal's socket vacates and, if it
    // doesn't reconnect in time, issues `Command::MatchAbandoned` against its team — a
    // clean `Win(present_seat)` forfeit. These use a tiny real grace (~80 ms) so the
    // forfeit fires fast; a generous slack on the wait keeps them non-flaky.

    /// The grace window the forfeit tests arm — small so they're fast, but comfortably
    /// above scheduler jitter so the arm/await ordering is reliable.
    const TINY_GRACE: std::time::Duration = std::time::Duration::from_millis(80);

    /// An in-memory 2v2 actor over seed 7 with an explicit absence-forfeit grace.
    fn spawn_2v2_grace(grace: std::time::Duration) -> MatchHandle {
        let session = Session::new_2v2(7, [deck(), deck(), deck(), deck()]);
        spawn(
            session,
            ActorConfig {
                mode: ActorMode::TwoVsTwo { bots: Vec::new() },
                db_id: "test-2v2-grace".into(),
                journal: None,
                event_client: None,
                seed_commit: SeedCommitment::new(7),
                abandon_grace: grace,
            },
        )
    }

    /// (a) Seat A drops and never returns: once the grace lapses the actor forfeits it,
    /// the match finishes `Win(B)`, and the PRESENT opponent (Seat B) is pushed the
    /// finished view plus the end-of-match seed reveal — all without a single command
    /// from B. (Seat A opens on seed 7; both seats are human.)
    #[tokio::test]
    async fn a_vacated_seat_forfeits_after_the_grace_and_the_opponent_sees_the_finish() {
        use recollect_core::state::Phase;
        let h = spawn_1v1_grace(7, None, TINY_GRACE);
        let (a_tx, _a_rx) = mpsc::unbounded_channel();
        let (b_tx, mut b_rx) = mpsc::unbounded_channel();
        h.subscribe(Principal::Seat(Seat::A), a_tx, None)
            .await
            .unwrap();
        h.subscribe(Principal::Seat(Seat::B), b_tx, None)
            .await
            .unwrap();

        // Seat A's socket tears down — the transport signals the actor eagerly.
        h.seat_vacated(Principal::Seat(Seat::A)).await;

        // Wait past the grace; the actor issues MatchAbandoned against Seat A → Win(B).
        tokio::time::sleep(TINY_GRACE * 4).await;

        // Seat B's per-seat channel carries the finished view (the present opponent is
        // told) and the seed reveal — drain and assert both.
        let (mut saw_finish, mut saw_reveal) = (false, false);
        while let Ok(frame) = b_rx.try_recv() {
            match parse(&frame) {
                ServerMsg::Update { view, .. } => {
                    if let Phase::Finished { result, .. } = &view.phase {
                        assert_eq!(
                            format!("{result:?}"),
                            "Win(B)",
                            "the absent Seat A forfeits; the present Seat B wins"
                        );
                        assert_eq!(view.seat, Seat::B, "B's own redacted finished view");
                        saw_finish = true;
                    }
                }
                ServerMsg::SeedRevealed { .. } => saw_reveal = true,
                _ => {}
            }
        }
        assert!(
            saw_finish,
            "the present opponent was pushed the forfeit finish"
        );
        assert!(
            saw_reveal,
            "the end-of-match seed reveal reached the opponent"
        );

        // The match is genuinely over: a fresh subscribe resumes onto a Finished view.
        let (a_tx2, _a_rx2) = mpsc::unbounded_channel();
        let welcome = h
            .subscribe(Principal::Seat(Seat::A), a_tx2, None)
            .await
            .unwrap();
        match parse(&welcome) {
            ServerMsg::Welcome { view, .. } => assert!(
                matches!(view.phase, Phase::Finished { .. }),
                "the match stays finished after the forfeit"
            ),
            other => panic!("expected a Welcome onto the finished match, got {other:?}"),
        }
    }

    /// (b) Seat A drops but RECONNECTS within the grace: no forfeit. The reconnect
    /// disarms the timer, so after the (would-be) deadline the match is still live and
    /// the turn is unchanged. Proves the disarm-on-reconnect path.
    #[tokio::test]
    async fn a_reconnect_within_grace_cancels_the_forfeit() {
        use recollect_core::state::Phase;
        let h = spawn_1v1_grace(7, None, TINY_GRACE);
        let (a_tx, a_rx) = mpsc::unbounded_channel();
        let (b_tx, _b_rx) = mpsc::unbounded_channel();
        h.subscribe(Principal::Seat(Seat::A), a_tx, None)
            .await
            .unwrap();
        h.subscribe(Principal::Seat(Seat::B), b_tx, None)
            .await
            .unwrap();

        // A drops, then reconnects well within the grace (a fresh sender, same principal).
        drop(a_rx);
        h.seat_vacated(Principal::Seat(Seat::A)).await;
        let (a_tx2, _a_rx2) = mpsc::unbounded_channel();
        let welcome = h
            .subscribe(Principal::Seat(Seat::A), a_tx2, None)
            .await
            .expect("A reconnects within the grace");
        assert!(
            matches!(parse(&welcome), ServerMsg::Welcome { view, .. } if !matches!(view.phase, Phase::Finished { .. })),
            "the resume is onto a LIVE match — the reconnect cancelled the pending forfeit"
        );

        // Wait past the original deadline; nothing forfeits — the match is still live
        // and A can still act (its turn, on this seed), proving no MatchAbandoned fired.
        tokio::time::sleep(TINY_GRACE * 4).await;
        let ack = h
            .text(Principal::Seat(Seat::A), cmd(1, Command::EndTurn))
            .await
            .expect("the actor still serves the live match");
        match parse(&ack) {
            ServerMsg::Applied { view, .. } => assert_eq!(
                view.active,
                Seat::B,
                "the match never forfeited — A's legal EndTurn applied normally"
            ),
            other => panic!("expected Applied on the still-live match, got {other:?}"),
        }
    }

    /// (d) 2v2: one slot (A1) drops and doesn't return — its WHOLE TEAM forfeits. The
    /// forfeit is keyed on the team seat (`A1.team() == Seat::A`), so the result is
    /// `Win(B)` (team B), and a present team-B slot is pushed the finished board.
    #[tokio::test]
    async fn a_2v2_slot_drop_forfeits_its_whole_team() {
        use recollect_core::state::Phase;
        let h = spawn_2v2_grace(TINY_GRACE);
        // Subscribe all four slots; keep B1's receiver to read the finish.
        let mut b1_rx = None;
        for slot in [SeatSlot::A1, SeatSlot::B1, SeatSlot::A2, SeatSlot::B2] {
            let (tx, rx) = mpsc::unbounded_channel();
            h.subscribe(Principal::Slot(slot), tx, None).await.unwrap();
            if slot == SeatSlot::B1 {
                b1_rx = Some(rx);
            }
        }
        let mut b1_rx = b1_rx.unwrap();

        // A1 vacates and never returns → team A forfeits after the grace.
        h.seat_vacated(Principal::Slot(SeatSlot::A1)).await;
        tokio::time::sleep(TINY_GRACE * 4).await;

        // A present team-B slot (B1) is pushed the finished board: Win(B).
        let mut saw_finish = false;
        while let Ok(frame) = b1_rx.try_recv() {
            if let ServerMsg::TeamUpdate { view, .. } = parse(&frame)
                && let Phase::Finished { result, .. } = &view.phase
            {
                assert_eq!(
                    format!("{result:?}"),
                    "Win(B)",
                    "team A's absent slot forfeits the whole team; team B wins"
                );
                saw_finish = true;
            }
        }
        assert!(saw_finish, "a present team-B slot saw the forfeit finish");
    }

    /// (e) A BOT seat never arms a forfeit: in a vs-bot 1v1 the bot holds Seat B, so a
    /// (spurious) SeatVacated for Seat B must NOT forfeit — a bot has no socket and can't
    /// abandon. After the grace the match is still live and the human (Seat A) plays on.
    #[tokio::test]
    async fn a_bot_seat_never_forfeits() {
        use recollect_core::state::Phase;
        let bot = Bot {
            seat: Seat::B,
            difficulty: recollect_bot::Difficulty::Normal,
            faction: recollect_bot::Faction::Lorekeeper,
            rng: std::sync::Mutex::new(recollect_core::rng::Rng::from_seed(7 ^ 0xB07)),
        };
        let h = spawn_1v1_grace(7, Some(bot), TINY_GRACE);
        let (a_tx, _a_rx) = mpsc::unbounded_channel();
        let welcome = h
            .subscribe(Principal::Seat(Seat::A), a_tx, None)
            .await
            .unwrap();
        assert!(matches!(parse(&welcome), ServerMsg::Welcome { .. }));

        // Signal a vacate for the BOT seat (B). `is_human` rejects it — no timer arms.
        h.seat_vacated(Principal::Seat(Seat::B)).await;
        tokio::time::sleep(TINY_GRACE * 4).await;

        // The match is still live: the human ends its turn and the actor drives the bot,
        // returning a non-finished (or normally-progressing) view — never a forfeit.
        let ack = h
            .text(Principal::Seat(Seat::A), cmd(1, Command::EndTurn))
            .await
            .expect("the match is still live — the bot seat never forfeited");
        match parse(&ack) {
            ServerMsg::Applied { view, .. } => assert!(
                !matches!(
                    view.phase,
                    Phase::Finished {
                        result: recollect_core::state::MatchResult::Win(Seat::A),
                        ..
                    }
                ) || view.active == Seat::A,
                "the bot seat's vacate signal did not forfeit the match to A"
            ),
            other => panic!("expected Applied on the live vs-bot match, got {other:?}"),
        }
    }
}
