//! The reusable bounded model-check bridge: fold real `recollect-core` engine
//! events over the aggregate (no re-modeling of the rules), explore the reachable
//! state space exhaustively to a bounded depth, and assert the shared invariant
//! suite on every state. Any violation is a true engine bug, not a model artifact.
//!
//! ONE definition checks THREE shapes, selected by [`Mode`]:
//! - **1v1 duel** — a Lorekeeper-vs-Lorekeeper telling.
//! - **Solace PvE** — 1v1 where seat B plays Unwritten/IllIntent through the same path
//!   as any seat. (The Solace is just another faction; no director to special-case.)
//!   These two differ only by **seat B's faction** (its deck).
//! - **2v2 team** — the four-slot 6×6 telling: init via `Engine::new_2v2_with_opener`,
//!   actions from the active SLOT's team (`legal_commands(active_slot.team())`), redaction
//!   checked from all four slots via `view_for_slot`. The 6×6 board × four hands branches
//!   hard, so it runs a TIGHT bound (small decks, low `max_round`) — a shallow frontier on
//!   which the same invariants are exhaustively asserted, closing the gap where the 2v2
//!   path had no formal coverage at all.
//!
//! State = the engine snapshot (`GameState` + entropy `DrawPos`), serialized so
//! stateright can hash/dedup reachable states. Actions = the active seat's (1v1) or the
//! active slot's team's (2v2) legal commands.
//!
//! ## Properties — every engine invariant, exhaustively
//! Each property is evaluated on EVERY reachable state of the bounded frontier, in
//! EVERY mode. They were red-teamed against the architecture.md invariants table —
//! a property that cannot fail is worthless, so each is paired with the mutation
//! that would turn it red:
//! - **state validity** (`invariants::check`) — overheal / phantom score / runaway
//!   round all go red.
//! - **liveness** (no stuck telling) — a phase with no legal command goes red.
//! - **determinism** (`decide` re-run on independent clones is byte-identical) — a
//!   clock/HashMap/non-counter-RNG read in `decide` would diverge the two clones.
//! - **no seed leak** (the seed string is in no state, no event, no per-seat view) —
//!   threading the seed into state/event/view goes red.
//! - **redaction** (a seat's view exposes the opponent only as counts; counts are
//!   truthful) — copying the opponent's hand/peek/deck-order into the view goes red.
//! - **abandonment** (`MatchAbandoned { s }` from any reachable state finishes the
//!   match as `Win(s.other())`) — scoring it by tally, or naming the wrong winner,
//!   goes red.
//!
//! See architecture.md (the invariants table).
use recollect_core::state::{Command, MatchRules, Phase};
use recollect_core::types::{CardDef, CardId, CardKind, SeatSlot};
use recollect_core::view::{view_for, view_for_slot};
use recollect_core::{Engine, MatchResult, Seat};
use stateright::{Checker, Model, Property};

pub type SnapJson = String;

/// Which telling shape the bridge explores. 1v1 (the Solace/Lorekeeper duel —
/// `deck_a` vs `deck_b`, opener seat A) or 2v2 (four slots A1→B1→A2→B2 on the
/// 6×6 board, both A-slots fielding `deck_a`, both B-slots `deck_b`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    OneVsOne,
    TwoVsTwo,
}

pub struct EngineModel {
    pub catalog: Vec<CardDef>,
    pub deck_a: Vec<CardId>,
    pub deck_b: Vec<CardId>,
    pub seed: u64,
    /// Stop exploring past this round so the frontier stays finite and small.
    pub max_round: u8,
    /// 1v1 (the default) or the 2v2 four-slot telling.
    pub mode: Mode,
    /// An optional pre-seeded **initial snapshot** to BFS from instead of genesis.
    /// Used to make a mid-telling configuration reachable that pure genesis-BFS would
    /// take too long to reach — notably a **standing-Faded form + its base in hand**, so
    /// the **Devolve** action (the §5 rescue) is immediately legal and EVERY property
    /// (validity, liveness, determinism, no-seed-leak, redaction) is asserted across the
    /// recede and the states it reaches. `None` ⇒ the normal genesis init for `mode`.
    pub init_override: Option<SnapJson>,
}

impl EngineModel {
    pub fn engine_from(&self, snap: &SnapJson) -> Engine {
        let (state, pos): (recollect_core::state::GameState, recollect_core::DrawPos) =
            serde_json::from_str(snap).expect("snapshot round-trips");
        Engine::from_state(state, self.seed, pos, self.catalog.clone())
    }
    fn snap_of(e: &Engine) -> SnapJson {
        serde_json::to_string(&e.snapshot()).expect("snapshot serializes")
    }
    /// A few cheap cards — small decks keep the branching factor exhaustively checkable.
    /// `solace` picks the faction's cheap bodies (Unwritten/IllIntent) vs Lorekeeper spirits.
    pub fn cheap_deck(catalog: &[CardDef], n: usize, solace: bool) -> Vec<CardId> {
        catalog
            .iter()
            .filter(|c| {
                c.cost <= 2
                    && if solace {
                        matches!(c.kind, CardKind::Unwritten | CardKind::IllIntent)
                    } else {
                        matches!(c.kind, CardKind::Spirit)
                    }
            })
            .take(n)
            .map(|c| c.id)
            .collect()
    }
    /// The seat that acts from this state: `active` in 1v1; in 2v2 the active SLOT's
    /// team (`active_slot.team()`) — the one `legal_commands`/`apply` are keyed on
    /// (the engine derives WHICH slot from `active_slot` internally). One accessor so
    /// `actions`/`next_state` and the determinism property stay mode-agnostic.
    fn acting_seat(e: &Engine) -> Seat {
        match e.state().is_2v2() {
            true => e.state().active_slot.team(),
            false => e.state().active,
        }
    }

    /// Every per-player view this state exposes, serialized: the two `PlayerView`s
    /// (1v1) or the four `TeamView`s (2v2). The no-seed-leak property scans them all,
    /// so 2v2 redaction is held to the same bar — the seed must hide from every slot's
    /// vantage, not just seat A/B's.
    fn participant_views(e: &Engine) -> Vec<String> {
        if e.state().is_2v2() {
            SeatSlot::all_2v2()
                .iter()
                .map(|&sl| serde_json::to_string(&view_for_slot(e, sl)).unwrap())
                .collect()
        } else {
            [Seat::A, Seat::B]
                .iter()
                .map(|&seat| serde_json::to_string(&view_for(e, seat)).unwrap())
                .collect()
        }
    }

    /// BFS the bounded frontier, assert every property on every reachable state,
    /// and return how many unique states were explored.
    pub fn run(self, target_states: usize) -> usize {
        let checker = self
            .checker()
            .target_state_count(target_states)
            .spawn_bfs()
            .join();
        checker.assert_properties();
        checker.unique_state_count()
    }
}

impl Model for EngineModel {
    type State = SnapJson;
    type Action = Command;

    fn init_states(&self) -> Vec<Self::State> {
        // A pre-seeded mid-telling snapshot (e.g. a standing-Faded form + its base in
        // hand, so Devolve is immediately reachable) overrides genesis when present.
        if let Some(snap) = &self.init_override {
            return vec![snap.clone()];
        }
        let e = match self.mode {
            Mode::OneVsOne => {
                let (e, _) = Engine::new_with_rules(
                    self.seed,
                    self.catalog.clone(),
                    self.deck_a.clone(),
                    self.deck_b.clone(),
                    MatchRules::default(),
                    Seat::A,
                );
                e
            }
            // Four slots A1,B1,A2,B2 — both A-slots field deck_a, both B-slots
            // deck_b; A1 opens. `new_2v2_with_opener` is the same genesis the server uses.
            Mode::TwoVsTwo => {
                let decks = [
                    self.deck_a.clone(),
                    self.deck_b.clone(),
                    self.deck_a.clone(),
                    self.deck_b.clone(),
                ];
                let (e, _) = Engine::new_2v2_with_opener(
                    self.seed,
                    self.catalog.clone(),
                    decks,
                    SeatSlot::A1,
                    [recollect_core::types::Faction::Lorekeeper; 2],
                );
                e
            }
        };
        vec![Self::snap_of(&e)]
    }

    fn actions(&self, s: &Self::State, out: &mut Vec<Self::Action>) {
        let e = self.engine_from(s);
        if matches!(e.state().phase, Phase::Finished { .. }) || e.state().round > self.max_round {
            return; // bound the exploration depth
        }
        out.extend(e.legal_commands(Self::acting_seat(&e)));
    }

    fn next_state(&self, s: &Self::State, a: Self::Action) -> Option<Self::State> {
        let mut e = self.engine_from(s);
        let seat = Self::acting_seat(&e);
        e.apply(seat, a).ok()?;
        Some(Self::snap_of(&e))
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            // The shared state-validity suite (recollect_core::invariants) — the SAME
            // definition the fuzz checks after every command, verified here exhaustively
            // over every reachable state, in BOTH modes.
            Property::<Self>::always("state invariants hold", |m, s| {
                recollect_core::invariants::check(m.engine_from(s).state()).is_ok()
            }),
            // Liveness (reachability — needs the engine, not just the state): every
            // non-finished, in-bound state offers at least one legal command (to the
            // active seat in 1v1, the active slot's team in 2v2).
            Property::<Self>::always("no stuck telling (liveness)", |m, s| {
                let e = m.engine_from(s);
                if matches!(e.state().phase, Phase::Finished { .. })
                    || e.state().round > m.max_round
                {
                    return true;
                }
                !e.legal_commands(Self::acting_seat(&e)).is_empty()
            }),
            // DETERMINISM (invariant 1): same state + same command ⇒ identical events
            // AND identical resulting snapshot, every time. We re-run `decide`/`evolve`
            // on TWO independent clones of the reachable state and compare byte-for-byte.
            // Exhaustive here means it holds at every reachable position, not just from
            // genesis. (RED if `decide` ever read a clock / iterated a HashMap / drew
            // from anything but the counter-mode stream — the two runs would diverge.)
            Property::<Self>::always("decide is deterministic (re-run is identical)", |m, s| {
                let e0 = m.engine_from(s);
                if matches!(e0.state().phase, Phase::Finished { .. }) {
                    return true;
                }
                let seat = Self::acting_seat(&e0);
                let Some(cmd) = e0.legal_commands(seat).into_iter().next() else {
                    return true;
                };
                let mut a = m.engine_from(s);
                let mut b = m.engine_from(s);
                let ea = a.apply(seat, cmd.clone());
                let eb = b.apply(seat, cmd);
                ea == eb && Self::snap_of(&a) == Self::snap_of(&b)
            }),
            // NO SEED LEAK (invariant 1, second clause): the seed appears in no state,
            // no event, and no per-seat view. A client holding every event and every
            // view must not be able to precompute Echo/shuffle order. (RED the moment
            // anything threads the seed into aggregate state, an event, or a view.)
            Property::<Self>::always("the seed leaks into no state, event, or view", |m, s| {
                let needle = m.seed.to_string();
                let e = m.engine_from(s);
                // State.
                if serde_json::to_string(e.state()).unwrap().contains(&needle) {
                    return false;
                }
                // Every per-player view: both seats in 1v1, all four slots in 2v2.
                if Self::participant_views(&e)
                    .iter()
                    .any(|v| v.contains(&needle))
                {
                    return false;
                }
                // The events this position can emit (the first legal command's batch).
                if !matches!(e.state().phase, Phase::Finished { .. }) {
                    let seat = Self::acting_seat(&e);
                    if let Some(cmd) = e.legal_commands(seat).into_iter().next() {
                        let mut probe = m.engine_from(s);
                        if let Ok(evs) = probe.apply(seat, cmd)
                            && serde_json::to_string(&evs).unwrap().contains(&needle)
                        {
                            return false;
                        }
                    }
                }
                true
            }),
            // REDACTION (invariant 2): a seat's `PlayerView` exposes the opponent ONLY
            // as counts — never the hand cards, the deck order, or the Glimpse peek — and
            // those counts are truthful. Checked from both vantages on every reachable
            // state. (RED if `view_for` ever copied the opponent's `hand`/`peeked_top`
            // into the view: the serialized view would then carry a SECOND `"hand":` /
            // `"peeked_top"` key, or the count would stop matching the real hand.)
            Property::<Self>::always("the opponent crosses only as truthful counts", |m, s| {
                let e = m.engine_from(s);
                let st = e.state();
                match m.mode {
                    Mode::OneVsOne => {
                        for seat in [Seat::A, Seat::B] {
                            let v = view_for(&e, seat);
                            let json = serde_json::to_string(&v).unwrap();
                            // Structural redaction: exactly ONE hand / peek / pending in the
                            // serialized view — your own. The opponent block is counts-only by
                            // type; a leak would add a second occurrence of these keys.
                            if json.matches("\"hand\":").count() != 1
                                || json.matches("\"peeked_top\":").count() != 1
                                || json.matches("\"pending\":").count() != 1
                            {
                                return false;
                            }
                            // The counts the opponent block reports must equal the real
                            // hidden state — redaction hides contents, never lies about size.
                            let opp = st.player(seat.other());
                            if v.opponent.hand_count as usize != opp.hand.len()
                                || v.opponent.deck_count as usize != opp.deck.len()
                            {
                                return false;
                            }
                        }
                        true
                    }
                    // Each of the four slots sees its OWN hand and everyone else —
                    // teammate AND both opponents — as truthful counts only. A leak would
                    // add a second `"hand":` (a `Vec<CardId>`) to the serialized `TeamView`,
                    // or make a reported count diverge from the real slot's hand/deck.
                    Mode::TwoVsTwo => {
                        for slot in SeatSlot::all_2v2() {
                            let v = view_for_slot(&e, slot);
                            let json = serde_json::to_string(&v).unwrap();
                            // Only `you` carries a real hand; teammate + opponents are
                            // counts-only by type — so exactly one `"hand":` key.
                            if json.matches("\"hand\":").count() != 1
                                || json.matches("\"peeked_top\":").count() != 1
                                || json.matches("\"pending\":").count() != 1
                            {
                                return false;
                            }
                            // Teammate + both opponents: their reported counts must equal
                            // the real hidden hands/decks (size disclosed, contents hidden).
                            let mate = match slot {
                                SeatSlot::A1 => SeatSlot::A2,
                                SeatSlot::A2 => SeatSlot::A1,
                                SeatSlot::B1 => SeatSlot::B2,
                                SeatSlot::B2 => SeatSlot::B1,
                            };
                            let opp_slots = if slot.team() == Seat::A {
                                [SeatSlot::B1, SeatSlot::B2]
                            } else {
                                [SeatSlot::A1, SeatSlot::A2]
                            };
                            let truthful =
                                |ov: &recollect_core::view::OpponentView, sl: SeatSlot| {
                                    let p = st.player_slot(sl);
                                    ov.hand_count as usize == p.hand.len()
                                        && ov.deck_count as usize == p.deck.len()
                                };
                            if !truthful(&v.teammate, mate)
                                || !truthful(&v.opponents[0], opp_slots[0])
                                || !truthful(&v.opponents[1], opp_slots[1])
                            {
                                return false;
                            }
                        }
                        true
                    }
                }
            }),
            // ABANDONMENT (lifecycle invariant): from ANY reachable, non-finished state,
            // a system-issued `MatchAbandoned { seat }` finishes the match as a forfeit
            // win for the PRESENT seat — `Win(seat.other())` — regardless of the standing
            // tally. `legal_commands` never offers it (the transport gates issuance), so
            // the BFS frontier can't reach it as an action; we exercise it here as a
            // property over every reachable position instead. (RED if abandonment scored
            // by tally, named the abandoner the winner, or failed to finish the match.)
            Property::<Self>::always("abandonment forfeits to the present seat", |m, s| {
                let e0 = m.engine_from(s);
                if matches!(e0.state().phase, Phase::Finished { .. }) {
                    return true; // already over — abandonment is a no-op subject
                }
                for abandoner in [Seat::A, Seat::B] {
                    let mut e = m.engine_from(s);
                    // Resolvable on EITHER seat's turn; the engine ignores `active` here.
                    if e.apply(
                        e.state().active,
                        Command::MatchAbandoned { seat: abandoner },
                    )
                    .is_err()
                    {
                        return false;
                    }
                    match e.state().phase {
                        Phase::Finished {
                            result: MatchResult::Win(winner),
                            ..
                        } if winner == abandoner.other() => {}
                        _ => return false,
                    }
                }
                true
            }),
            // MULLIGAN (§5 — opening state space): wherever a mulligan is reachable
            // (the BFS offers it through `legal_commands`, so the post-mulligan
            // states are already explored against EVERY property above — state
            // validity, determinism, redaction, no-seed-leak), it must additionally
            // (a) apply cleanly and bottom exactly one card (hand −1, page
            // conserved, the once-flag set), and (b) leak NOTHING extra to the
            // opponent: the redrawn hand never appears in the opponent's view —
            // only the public `mulliganed` beat and truthful counts. (RED if the
            // mulligan corrupted the page, forgot to spend the once-flag, or copied
            // the fresh hand / deck order into the opponent's view.)
            Property::<Self>::always(
                "a mulligan reshuffles cleanly and never leaks the hand",
                |m, s| {
                    let e0 = m.engine_from(s);
                    let active = Self::acting_seat(&e0);
                    let Some(cmd) = e0
                        .legal_commands(active)
                        .into_iter()
                        .find(|c| matches!(c, Command::Mulligan { .. }))
                    else {
                        return true; // not a mulligan-offering state — nothing to assert
                    };
                    let mut e = m.engine_from(s);
                    let before = e.state().player(active);
                    let (hand0, total0) =
                        (before.hand.len(), before.hand.len() + before.deck.len());
                    if e.apply(active, cmd).is_err() {
                        return false; // whatever `legal_commands` offers, `apply` must accept
                    }
                    let after = e.state().player(active);
                    if after.hand.len() + 1 != hand0
                        || after.hand.len() + after.deck.len() != total0
                        || !e.state().mulliganed[active as usize]
                    {
                        return false; // bottom-one cost, page conserved, once-flag set
                    }
                    // The opponent's view carries the public beat + counts, never the cards.
                    let vo = view_for(&e, active.other());
                    let json = serde_json::to_string(&vo).unwrap();
                    json.matches("\"hand\":").count() == 1
                        && json.matches("\"deck\":").count() == 0
                        && vo.opponent.hand_count as usize == after.hand.len()
                        && vo.opponent.deck_count as usize == after.deck.len()
                },
            ),
            // GLIMPSE (§5 — the burn-then-keep-or-bottom state space): the BFS reaches
            // both Glimpse choices through `legal_commands` (Glimpse → Choose [burn] →
            // Choose [keep/bottom]), so the post-choice states are already checked
            // against EVERY property above — state validity, determinism, redaction,
            // no-seed-leak. This pins the two choices' own contracts on every
            // reachable Glimpse-pending state:
            //   STEP 1 (GlimpseBurn): every burn option applies cleanly, removes
            //   EXACTLY one card from the hand (the burn cost — hand−1, deck/Anima
            //   untouched), and never leaks the burnable hand or burned card.
            //   STEP 2 (Glimpse): KEEP grants no Anima and leaves the page length
            //   unchanged; BOTTOM grants exactly +1 and conserves the page (rotates
            //   one card under). Neither leaks the peeked card.
            // Across both: the chooser's `pending` NEVER surfaces in the opponent's
            // view, and no `top`/`burnable` field appears there. (RED if a branch
            // corrupted the page/hand, mis-credited Anima, or copied a private card
            // into the opponent's view.)
            Property::<Self>::always(
                "a glimpse burns then keeps-or-bottoms cleanly and never leaks a private card",
                |m, s| {
                    let e0 = m.engine_from(s);
                    let active = Self::acting_seat(&e0);
                    let total =
                        |p: &recollect_core::state::PlayerState| p.hand.len() + p.deck.len();
                    match e0.state().pending_choice {
                        // STEP 1 — the BURN cost.
                        Some(recollect_core::state::PendingChoice::GlimpseBurn {
                            ref burnable,
                            ..
                        }) => {
                            let before = e0.state().player(active);
                            let (anima0, deck0, hand0) =
                                (before.anima, before.deck.len(), before.hand.len());
                            // Every burnable index applies cleanly and spends exactly one card.
                            for i in 0..burnable.len() {
                                let mut e = m.engine_from(s);
                                if e.apply(active, Command::Choose { index: i as u8 }).is_err() {
                                    return false;
                                }
                                let after = e.state().player(active);
                                if after.hand.len() != hand0 - 1
                                    || after.deck.len() != deck0
                                    || after.anima != anima0
                                {
                                    return false; // the burn must spend exactly one hand card
                                }
                                if leaks_glimpse(&e, active) {
                                    return false;
                                }
                            }
                            true
                        }
                        // STEP 2 — keep-or-bottom.
                        Some(recollect_core::state::PendingChoice::Glimpse { .. }) => {
                            let before = e0.state().player(active);
                            let (anima0, deck0, total0) =
                                (before.anima, before.deck.len(), total(before));
                            // KEEP (0): no Anima, the page is untouched (card stays on top).
                            {
                                let mut e = m.engine_from(s);
                                if e.apply(active, Command::Choose { index: 0 }).is_err() {
                                    return false;
                                }
                                let after = e.state().player(active);
                                if after.anima != anima0
                                    || after.deck.len() != deck0
                                    || total(after) != total0
                                {
                                    return false;
                                }
                                if leaks_glimpse(&e, active) {
                                    return false;
                                }
                            }
                            // BOTTOM (1): exactly +1 Anima, the page is conserved.
                            {
                                let mut e = m.engine_from(s);
                                if e.apply(active, Command::Choose { index: 1 }).is_err() {
                                    return false;
                                }
                                let after = e.state().player(active);
                                if after.anima != anima0 + 1 || total(after) != total0 {
                                    return false;
                                }
                                if leaks_glimpse(&e, active) {
                                    return false;
                                }
                            }
                            true
                        }
                        _ => true, // not a Glimpse-pending state — nothing to assert
                    }
                },
            ),
        ]
    }
}

/// Redaction probe for the §5 Glimpse: no private card may reach the opponent's
/// view — neither the keep-or-bottom peek (`top`) nor the burnable hand
/// (`burnable`), and the chooser's `pending` must never surface. True ⇒ a leak
/// (the property treats it as a failure).
fn leaks_glimpse(e: &Engine, chooser: Seat) -> bool {
    let vo = view_for(e, chooser.other());
    if vo.you.pending.is_some() {
        return true; // the opponent must never see the chooser's pending Glimpse
    }
    let json = serde_json::to_string(&vo).unwrap();
    json.contains("\"top\":") || json.contains("burnable")
}
