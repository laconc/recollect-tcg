//! The `Event` vocabulary: the facts `evolve` applies (the engine's event enum).
//! A sibling of `state.rs`; `use super::*` pulls the crate types.
use super::*;

/// Facts. Every event is self-sufficient: `evolve` applies it without
/// recomputation, so (snapshot, events) replays to an identical state.
///
/// VERSIONING POLICY: any change to the
/// wire shape of this enum — additive included — bumps the whole-enum
/// version and ships an identity-on-old-variants `MigrateFrom` impl, so
/// every mismatch surfaces as the typed `NewerThanBinary` error on every
/// binary in every deploy topology. The seed appears in NO event: clients
/// receiving the stream must never be able to precompute an Echo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    MatchStarted,
    AnimaGained {
        seat: Seat,
        amount: u8,
        reason: AnimaReason,
    },
    AnimaSpent {
        seat: Seat,
        amount: u8,
    },
    CardDrawn {
        seat: Seat,
    },
    ReleaseRequired {
        seat: Seat,
    },
    CardReleased {
        seat: Seat,
        hand_index: u8,
    },
    /// Glimpse (§5) opened: the seat spent its once-per-turn Glimpse. This marks
    /// `glimpsed_this_turn`; a `ChoiceOffered { GlimpseBurn }` rides behind it (the
    /// BURN cost — choose a hand card to spend), then the keep-or-bottom `Glimpse`,
    /// then a `GlimpseResolved`. `peeked` is the top card the seat will see
    /// (owner-only, redacted), carried for narration/journal.
    Glimpsed {
        seat: Seat,
        peeked: Option<CardId>,
    },
    /// Glimpse (§5) BURN cost paid: the seat spent `hand[hand_index]` to activate the
    /// glimpse — the card leaves play entirely. `evolve` removes it from hand; the
    /// keep-or-bottom `Glimpse` opens behind this. The burned card is owner-private
    /// (the opponent sees only the hand count fall — redacted in `view.rs`).
    GlimpseBurned {
        seat: Seat,
        hand_index: u8,
    },
    /// Glimpse (§5) settled: `kept` ⇒ the peeked card stays on top (no Anima);
    /// otherwise it is bottomed (the `+1` rides as a separate `AnimaGained`).
    GlimpseResolved {
        seat: Seat,
        kept: bool,
    },
    SpiritPlayed {
        seat: Seat,
        card: CardId,
        tile: u8,
        attack: i16,
        defense: i16,
        hp: i16,

        face_down: bool,
    },
    /// One strike (engage, retaliation half, chain link, or interception).
    Struck {
        from_tile: u8,
        to_tile: u8,
        damage: i16,
        echo: bool,
        kind: StrikeKind,
    },
    SpiritBecameFading {
        tile: u8,
        banished_by: Option<Seat>,
    },
    /// Overwrite resolution — fully self-sufficient. On success the defender
    /// dissolves immediately (banisher's impression = seat) and the attacker stands on
    /// `tile` at `attacker_hp_left` (fading if ≤0: mutual banishment). On
    /// failure the attacker dissolved — no impression — and `damage_to_defender`
    /// persists on the occupant.
    Overwrote {
        seat: Seat,
        card: CardId,
        tile: u8,
        success: bool,
        damage_to_defender: i16,
        defender_echo: bool,
        attack: i16,
        defense: i16,
        attacker_hp_left: i16,
        attacker_hp_max: i16,
    },
    SpiritMoved {
        from: u8,
        to: u8,
    },
    SpiritDissolved {
        tile: u8,
        impression: Seat,
    },
    TurnEnded {
        seat: Seat,
    },
    RoundAdvanced {
        round: u8,
    },
    MemoryContracted {
        faded_tiles: Vec<u8>,
    },
    /// Held Ground: a lingering rim tile fades the moment it is vacated.
    TileFaded {
        tile: u8,
    },
    MatchEnded {
        result: MatchResult,
        score_a: u8,
        score_b: u8,
    },
    /// Absence forfeit: `seat` abandoned; the present player wins.
    /// A DISTINCT payload from `MatchEnded` so the journal records the match
    /// ended by abandonment, not by score; `score_*` are the standing tally at
    /// the moment of forfeit (informative — the win is by forfeit, not by score).
    MatchAbandoned {
        seat: Seat,
        score_a: u8,
        score_b: u8,
    },
    /// Mulligan (§5 — London-lite): `seat` spent its opening mulligan. The event
    /// carries the FULL resulting `hand` and `deck` (post-reshuffle, post-bottom)
    /// so `evolve` reproduces them without touching entropy — the reshuffle's
    /// entropy was drawn in `decide` and is journaled in the draw counter, exactly
    /// like a match-start shuffle. REDACTION: this event names the seat's private
    /// cards, but clients never receive the raw event stream — the server fans out
    /// a redacted `PlayerView` per seat, so the opponent sees only the public
    /// `mulliganed` beat + truthful counts (the redaction invariant; AGENTS.md invariant 2).
    Mulliganed {
        seat: Seat,
        hand: Vec<CardId>,
        deck: Vec<CardId>,
    },
    /// Effect facts. Pre-release: the schema moves freely; the whole-enum
    /// versioning ceremony begins at first release.
    EffectRestored {
        tile: u8,
        amount: i16,
    },
    EffectDamaged {
        tile: u8,
        amount: i16,
    },
    EffectStat {
        tile: u8,
        attack: i16,
        defense: i16,
        form: i16,
    },
    /// A ThisRound modifier: expires when `until_round` ends.
    EffectTempStat {
        tile: u8,
        attack: i16,
        defense: i16,
        until_round: u8,
    },
    /// A this-round, seat-wide arrival-targeting reach buff (Tempestrider Roc,
    /// The Sky's Whole Weight, Hidden Vista). Pruned at round advance.
    ReachBuffed {
        seat: Seat,
        forward: i8,
        all_directions: bool,
        until_round: u8,
        /// True = targeting-only; false = full (also projection + interception).
        #[serde(default = "crate::state::default_true")]
        targeting_only: bool,
        /// Some = per-spirit (Tailwind); None = seat-wide (Open Sky).
        #[serde(default)]
        tile: Option<u8>,
    },
    /// Reckless Charge: the spirit at `tile` has its retaliation shifted `delta`
    /// until `until_round` (pruned at round advance).
    RetaliationBuffed {
        tile: u8,
        delta: i16,
        until_round: u8,
    },
    /// A this-round, seat-wide movement/push restriction (Stand Ground). Pruned
    /// at round advance.
    Restricted {
        seat: Seat,
        restriction: crate::effects::Restriction,
        until_round: u8,
    },
    /// Recover: a fully-dissolved spirit returns from the `dissolved` pool to its
    /// owner's hand (The Returning).
    RecoverTaken {
        seat: Seat,
        card: CardId,
    },
    /// TakeControl: the spirit at `tile` changes sides (The Perfect Lie steals the
    /// engager that sprang it).
    ControlTaken {
        tile: u8,
        new_owner: Seat,
    },
    /// TraitStrip: the spirit at `tile` loses its printed Keywords and Traits (The Blank Page).
    TraitsStripped {
        tile: u8,
    },
    /// TraitStrip (this round): the spirit at `tile` loses its printed Keywords/Traits
    /// through `until_round`, then regains them at the round advance (Smear).
    TraitsStrippedUntil {
        tile: u8,
        until_round: u8,
    },
    /// Star-Strewn Otter: a one-shot discount on `seat`'s next Ritual is granted…
    RitualDiscountGranted {
        seat: Seat,
        amount: u8,
    },
    /// Ink Runs Dry: `seat`'s next card costs `amount` more, through `until_round`.
    CardTaxed {
        seat: Seat,
        amount: u8,
        until_round: u8,
    },
    /// …and spent when that seat next plays a card (spirit or Ritual): the surcharge is lifted.
    CardTaxSpent {
        seat: Seat,
    },
    /// …and spent when that seat next casts a Ritual.
    RitualDiscountConsumed {
        seat: Seat,
    },
    /// The banishment this replaces was never journaled.
    ReplacementSurvived {
        tile: u8,
        form: i16,
    },
    /// Grief Split / Promise: a (lethal) blow against a bonded spirit is borne by
    /// its partner instead. `amount` is dealt to `to_tile`; `from_tile` keeps the
    /// remainder. `consume` => the Promise (OncePerMatch) is spent on `from_tile`.
    DamageRedirected {
        from_tile: u8,
        to_tile: u8,
        amount: i16,
        consume: bool,
    },
    /// Dig In / The Long Watch: a played card grants the spirit at `tile` a Keyword
    /// until `until_round` (`u8::MAX` = permanent). Rides on the spirit (follows moves).
    KeywordGranted {
        tile: u8,
        keyword: crate::effects::Keyword,
        until_round: u8,
    },
    /// Behind You: exchange the spirits at `a` and `b` (both occupied).
    SpiritsSwapped {
        a: u8,
        b: u8,
    },
    /// Don't Look: the spirit at `tile` can't engage or intercept through
    /// `until_round` (inclusive).
    EngageRestricted {
        tile: u8,
        until_round: u8,
    },
    /// Hold the Memory: the Fading spirit at `tile` skips its next Fade step.
    FadeDelayed {
        tile: u8,
    },
    /// Hold the Memory, spent: the one-turn skip at `tile` is consumed (the tile leaves
    /// `fade_delayed`), so its NEXT due turn-end dissolves it. Must ride an event — the
    /// Fade runs on `decide`'s clone, and a bare `fade_delayed` removal there would be lost
    /// on the committed board (the skip would never be spent → the base never dissolves).
    FadeDelayConsumed {
        tile: u8,
    },
    /// Patience: `seat` is owed `amount` Anima at its next Flow (scheduled at play).
    FlowAnimaScheduled {
        seat: Seat,
        amount: u8,
    },
    /// Patience: the owed Flow anima is paid to `seat` and the debt reset.
    FlowAnimaPaid {
        seat: Seat,
    },
    /// Bearer of Small Stones: `seat`'s evolutions ignore the shared-Imprint rule
    /// for the rest of this turn.
    EvolveImprintFreed {
        seat: Seat,
    },
    /// Fade reclaim: the spirit at `tile` is cashed back to the page — it leaves with
    /// NO impression (its Parting and the anima refund are separate events).
    SpiritReclaimed {
        tile: u8,
    },
    /// Kindle: `seat`'s next arriving spirit this turn gets +`attack` this round.
    NextArrivalBuffed {
        seat: Seat,
        attack: i16,
    },
    /// Kindle: the queued next-arrival buff was applied (or the turn ended) — reset.
    NextArrivalConsumed {
        seat: Seat,
    },
    /// Again!: `seat`'s next arrival may engage a second target at `penalty` this turn.
    NextArrivalSecondEngage {
        seat: Seat,
        penalty: i16,
    },
    /// Again!: the second-engage offer was consumed (or the turn ended) — reset.
    NextArrivalSecondConsumed {
        seat: Seat,
    },
    /// Curio Fox: `seat` privately looked at the face-down Fabrication (`card`) at `tile`.
    FabricationPeeked {
        seat: Seat,
        tile: u8,
        card: CardId,
    },
    /// Otterling Magus: the just-opened ritual target choice will also hit `count` extra targets.
    RitualExtraTargetsArmed {
        count: u8,
    },
    /// Otterling Magus: the extra ritual targets were applied (or the choice resolved) — reset.
    RitualExtraTargetsConsumed,
    /// Throughline: the spirit at `tile` completed a Throughline — gains `attack`/
    /// `defense` (base +10/+10, plus any Queen of the Quiet Garden bonus) and a full HP
    /// restore (once per spirit).
    ThroughlineCompleted {
        tile: u8,
        attack: i16,
        defense: i16,
    },
    /// own-turn choice opened (peek contents redacted in views).
    ChoiceOffered {
        choice: PendingChoice,
    },
    /// Peek resolved: `index` to hand, the rest bottomed in order.
    PeekTaken {
        seat: Seat,
        index: u8,
    },
    /// Target choice resolved into a concrete fact below.
    TargetChosen {
        tile: u8,
    },
    SpiritPushed {
        from: u8,
        to: u8,
    },
    OrdersSet {
        tile: u8,
        hold: bool,
    },
    /// A face-down spirit turns face-up (by its own telling or by force).
    SpiritRevealed {
        tile: u8,
    },
    /// A caller manifests its Kindred on an adjacent empty tile.
    SpiritManifested {
        seat: Seat,
        card: CardId,
        tile: u8,
        attack: i16,
        defense: i16,
        hp: i16,
    },
    /// The base at `tile` becomes its evolved form (`from` → `to`), full-HP
    /// (eternal); evolution clears Fading and the old form's dissolution triggers
    /// Parting. `seat` owns the base; `evolve` removes the played form card (`to`) from that
    /// seat's hand — the form was a deck-playable card held in hand.
    ///
    /// `keeps_throughline` carries the §5.4 once-per-body lifecycle decision across the
    /// catalog-free reducer: **Fabled** (a leap from a *healthy* base, a continuation)
    /// **keeps** the base's `throughline_done`, so a form whose base had completed arrives
    /// already done; **Primal** (a *Fading* base's last becoming) does **not** — it arrives
    /// `throughline_done = false` and may complete the Throughline anew. `decide` computes
    /// this by tier (it holds the form's rarity); `evolve` just applies it.
    SpiritEvolved {
        seat: Seat,
        tile: u8,
        from: CardId,
        to: CardId,
        attack: i16,
        defense: i16,
        hp: i16,
        keeps_throughline: bool,
    },
    /// Devolution (design §5): the standing-Faded **form** at `tile` is RECEDED a
    /// tier down to its `to` base (a base card the seat played from hand). The base
    /// arrives at FULL HP with its fade cleared (rescued), and is **summoning-sick**
    /// (it cannot evolve or move until the owner's next turn — `evolve` marks
    /// `moved_this_turn`). The recede **is an arrival, symmetric with evolution**: it
    /// fires the same arrival triggers (`check_throughline` — so a base receding into a
    /// standing 3-line re-completes on the spot — and a queued next-arrival buff), but
    /// **engages no one** (no strike target) and fires **no OnPlay**.
    /// `seat` owns the form; `evolve` removes the played base card (`to`) from that
    /// seat's hand. Player-facing text says the Lorekeeper **reverts**, the Solace
    /// **recedes** — one engine action, the faction's verb in UI/log. Redaction-safe:
    /// the opponent sees the Devolve + the resulting base, never the rest of the hand.
    SpiritDevolved {
        seat: Seat,
        tile: u8,
        from: CardId,
        to: CardId,
        attack: i16,
        defense: i16,
        hp: i16,
    },
    /// The Memory stirs — a tile shimmers one round ahead.
    StrayTelegraphed {
        tile: u8,
        surface_round: u8,
        midnight: bool,
    },
    /// A telegraphed surfacing was cancelled (the clearing was filled, or no Stray could
    /// come): the shimmer is dropped. Must ride an event — the surfacing pass runs on
    /// `decide`'s clone, so a bare `stray_telegraph = None` there would be lost on the
    /// committed board, leaving a phantom shimmer in the view forever.
    StrayTelegraphCleared,
    /// A Stray surfaces (NOT an arrival — no engage, no interception).
    StraySurfaced {
        card: CardId,
        tile: u8,
        temperament: Temperament,
        veiled: bool,
        hp: i16,
    },
    /// A veiled (Wary) Stray is unveiled by adjacency or patience.
    StrayUnveiled,
    /// Courtship progresses (adjacency with a shared Imprint, etc.).
    StrayCourted {
        seat: Seat,
        courtship: u8,
    },
    /// A Stray is befriended — yours thereafter, under Held Ground law.
    StrayBefriended {
        seat: Seat,
        card: CardId,
        tile: u8,
        attack: i16,
        defense: i16,
        hp: i16,
    },
    /// Feral: a Stray is wounded by an arrival it intercepted (Echo path).
    StrayStruck {
        tile: u8,
        damage: i16,
        hp: i16,
    },
    /// A Stray is banished (a normal banisher's impression).
    StrayBanished {
        tile: u8,
        impression: Seat,
    },
    /// An Overwrite resolved against a **revealed** Stray (§2 — the shimmer is on the
    /// board, so an Overwrite reaches it). The overwriter pays its cost and trades one
    /// simultaneous exchange with the wild. On `success` the Stray is banished, the
    /// overwriter takes the cleared tile (its banisher-impression beneath, faction-aware),
    /// and the Stray slot empties; on failure the overwriter dissolves (no impression),
    /// the `damage_to_stray` persisting on the wild. The overwriter arrives at full HP, so
    /// only the *defender* (here the Stray) could vary — but a Stray never engages and
    /// never Echoes (§1), so `defender_echo` is absent: the exchange is deterministic.
    /// (The hidden-Stray *deny-entry* case is `StrayDenied`, not this; only a face-up
    /// Stray is fought.)
    OverwroteStray {
        seat: Seat,
        card: CardId,
        tile: u8,
        success: bool,
        /// Damage the overwriter dealt the Stray (banks as the Stray's wound on failure).
        damage_to_stray: i16,
        attack: i16,
        defense: i16,
        /// The overwriter's HP after the Stray's retaliation (≤0 ⇒ it dissolved).
        attacker_hp_left: i16,
        attacker_hp_max: i16,
    },
    /// A **hidden** Stray (a veiled Wary, or any Stray not yet surfaced face-up) is
    /// **denied entry** by an Overwrite aimed at its tile (§2): it leaves with **no
    /// impression and no reveal** — it never *was* there to be fought, so it simply
    /// disappears. Redaction-critical: this carries ONLY the tile, never the Stray's
    /// `CardId` — the veil's identity must not leak when it is denied. The overwriter
    /// then takes the cleared tile via the `OverwroteStray { success: true }` that
    /// follows (an uncontested arrival, `damage_to_stray = 0`).
    StrayDenied {
        tile: u8,
    },
    /// The Solace manifests an Unwritten (an arrival — zones bite it).
    UnwrittenManifested {
        card: CardId,
        tile: u8,
        attack: i16,
        defense: i16,
        hp: i16,
    },
    /// Unwriting — an impression is erased, leaving NOTHING (not an impression).
    /// Used for *suppression* (The Long Rest, Lacuna): the memory is gone, no mark.
    ImpressionUnwritten {
        tile: u8,
    },
    /// The Solace *forgets* a player's impression: the mark is erased (nothing is left on the tile)
    /// and the Solace's off-board erasure tally goes +1, so forgetting SCORES — the same swing as a
    /// banish. Distinct from `ImpressionUnwritten`, which clears the mark without the tally. Emitted
    /// when an Unwritten *eats* the impression it shifts onto (e.g. The Devouring Margin).
    ImpressionForgotten {
        tile: u8,
    },
    /// The Solace's mercy: a spirit is RELEASED — removed from the board leaving
    /// nothing, no impression. Distinct from SpiritDissolved (which leaves an impression)
    /// and from ImpressionUnwritten (which erases an existing impression). Used by The Kind
    /// Erasure, The Mercy Itself, The Soft Close, etc.
    SpiritReleased {
        tile: u8,
    },
    /// Bounce: a spirit is returned to its owner's hand (The Unasked Question's
    /// engager, The Fog of Elsewhere). Leaves no body and no impression — it was
    /// un-played, not banished, so it fires no Parting/OnAnyBanish. A token has
    /// no card to return to: it simply ceases (removed, nothing added to hand).
    SpiritBounced {
        tile: u8,
    },
    /// An Unwritten shifts one tile toward the center (an arrival).
    UnwrittenShifted {
        from: u8,
        to: u8,
        eats_impression: bool,
    },
    /// An Unwriting is told (telegraphed cadence).
    UnwritingTold {
        seat: Seat,
        card: CardId,
    },
    /// Footnote / Sentence Fragment: a seat's top deck card is milled (removed to oblivion).
    DeckMilled {
        seat: Seat,
    },
    /// Let It Lie (Unwriting): impressions stop scoring for `round`.
    ImpressionsStoppedScoring {
        round: u8,
    },
    /// Silence Spreads (Unwriting): the Landmark at `tile` loses its text for `round`.
    LandmarkSilenced {
        tile: u8,
        round: u8,
    },
    /// The Quiet Spreads (Unwriting): `tiles` go calm (cannot be Played onto) through `round`.
    TilesGoneCalm {
        tiles: Vec<u8>,
        round: u8,
    },
    /// The Half-Remembered: on reveal it copies a spirit's stats (becomes that memory).
    SpiritCopiedStats {
        tile: u8,
        attack: i16,
        defense: i16,
        hp_max: i16,
    },
    /// The Almost-Said: it copies the Reach of the enemy that just engaged it.
    ReachCopied {
        tile: u8,
        reach: crate::types::Reach,
    },
    /// A Ritual is cast — its effect resolves, then it is spent (no board
    /// presence). The hand index is consumed; effects ride the usual events.
    RitualCast {
        seat: Seat,
        card: CardId,
    },
    /// A Bond joins two allied spirits (auras while both stand & adjacent).
    BondAttached {
        seat: Seat,
        card: CardId,
        tile_a: u8,
        tile_b: u8,
    },
    /// A Bond ends (a partner left, or a push separated them).
    BondBroken {
        card: CardId,
    },
    /// A Landmark is placed on an empty tile (terrain; auras nearby).
    LandmarkPlaced {
        seat: Seat,
        card: CardId,
        tile: u8,
    },
    /// A Fabrication is set face-down (a lie; projects adjacency only).
    FabricationSet {
        seat: Seat,
        card: CardId,
        tile: u8,
    },
    /// A Fabrication is revealed (by an engager or its own telling).
    FabricationRevealed {
        tile: u8,
    },
    /// Follow-up: a sprung Fabrication is consumed (the lie is spent).
    FabricationSpent {
        tile: u8,
    },
    /// A token dissolves to NO impression (caller left, or its own fade).
    TokenDissolved {
        tile: u8,
    },
}
