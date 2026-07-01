//! Game state + command vocabulary: `GameState`/`PlayerState`/`Phase`, the `Command`
//! enum (player moves), `ChoiceEffect`, `MatchRules`. (The `Event` enum is in `events`.)

use crate::types::{CardId, Seat};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spirit {
    pub card: CardId,
    pub owner: Seat,
    pub attack: i16,
    pub defense: i16,
    pub hp: i16,
    pub hp_max: i16,
    pub fading: bool,
    /// Who banished it (impression color); None = uncontested fade (owner impression).
    pub banished_by: Option<Seat>,
    /// The standing-Faded window. A spirit **banished in combat** does not
    /// dissolve at once: it stands Faded and lingers until the END of its owner's
    /// next turn, which gives the owner one Main phase to **Primal-evolve** it (a
    /// Primal needs a Fading-but-standing base). This is the round by the end of
    /// which it must dissolve — computed at the banish (the round of the owner's
    /// next turn-end). At the owner's turn-end the lingering base dissolves once
    /// `round >= fade_deadline` (so a base banished on the owner's OWN turn skips
    /// that turn's end and dissolves at the next). `None` ⇒ NOT a lingering combat
    /// fade: an uncontested fade (the Dusk's sweep) that dissolves at the
    /// turn-START Fade step. Cleared when the base evolves.
    #[serde(default)]
    pub fade_deadline: Option<u8>,
    /// Interception cap: once per round per spirit.
    pub intercepted_this_round: bool,
    /// The Blank Page: stripped of its printed Keywords and Traits (imprints),
    /// permanently. Aura-granted keywords still apply; only the card's own are silenced.
    #[serde(default)]
    pub traits_stripped: bool,
    /// Smear: a ROUND-SCOPED trait blanking — its printed Keywords/Traits are silenced
    /// through this round (pruned at the round advance). `None` ⇒ not round-stripped.
    /// (A documented simplification of Smear's "loses one Imprint this round": the engine
    /// blanks the whole printed trait set for the round rather than diffing a single Imprint.)
    #[serde(default)]
    pub traits_stripped_until: Option<u8>,
    #[serde(default)]
    pub replacement_used: bool,
    /// Standing Orders: Held — stands its ground, never intercepts.
    #[serde(default)]
    pub holding: bool,
    /// Lurk: face-down — unseen, unprojecting, never intercepting.
    #[serde(default)]
    pub face_down: bool,
    /// Kindred: a manifested token; fades to NO impression when its caller
    /// leaves play (`summoned_by` = the caller's tile at summon time, used
    /// only as a "this is a token" marker — the tie is by-card, below).
    #[serde(default)]
    pub is_token: bool,
    /// Which physical SLOT placed this (2v2). None in 1v1 (the team's sole
    /// player). Overwrite legality reads the ACTING slot's own
    /// projection, which is built from the spirits this slot placed + shared
    /// impressions/terrain — a partner's reach does not authorize your Overwrite.
    #[serde(default)]
    pub placed_by: Option<crate::types::SeatSlot>,
    /// Played-card keyword grants (Dig In: Steadfast this round; The Long Watch:
    /// Warded + Steadfast permanently). Each is `(keyword, until_round)` — pruned
    /// when `until_round < round` at round advance; `u8::MAX` is permanent. Lives
    /// on the spirit so it follows a move, unlike the tile-keyed `temp_mods`.
    #[serde(default)]
    pub kw_grants: Vec<(crate::effects::Keyword, u8)>,
    /// Don't Look: this spirit can neither initiate an engage NOR intercept while
    /// `round <= no_engage_until` (0 = unrestricted). On the spirit so it follows a
    /// move (a restricted spirit can't dodge the lock by relocating).
    #[serde(default)]
    pub no_engage_until: u8,
    /// Throughline: this spirit has already completed a Throughline (got its
    /// +10/+10 and full restore). One-time per spirit, so it doesn't re-trigger.
    #[serde(default)]
    pub throughline_done: bool,
    /// The Almost-Said: the Reach copied from the last enemy that engaged it. `None` ⇒ use
    /// the card's own Reach.
    #[serde(default)]
    pub copied_reach: Option<crate::types::Reach>,
}

impl Spirit {
    /// Echo eligibility: at or below half printed HP (the ripple).
    pub fn echo_eligible(&self) -> bool {
        self.hp * 2 <= self.hp_max
    }

    /// Whether this spirit's PRINTED Keywords/Traits are silenced at `round` — either
    /// permanently (The Blank Page) or for the round (Smear). The single predicate the
    /// trait/keyword read-sites consult so both blanking sources behave identically.
    pub fn traits_blanked(&self, round: u8) -> bool {
        self.traits_stripped || self.traits_stripped_until.is_some_and(|u| u >= round)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TileState {
    pub spirit: Option<Spirit>,
    /// The stack of impressions on this tile: each scores one point for its owner, even when a
    /// spirit covers them. Banishing pushes a mark; erasing pops one. Empty = unmarked.
    pub impressions: Vec<Seat>,
    /// True once the Memory has contracted past this tile. Impressions here are locked.
    pub faded: bool,
    /// A Landmark or face-down Fabrication occupying this tile.
    #[serde(default)]
    pub terrain: Option<Terrain>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Terrain {
    pub card: CardId,
    pub owner: Seat,
    pub kind: TerrainKind,
    /// Fabrications start hidden; Landmarks are always open.
    pub face_down: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerrainKind {
    Landmark,
    Fabrication,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerState {
    pub hand: Vec<CardId>,
    pub deck: Vec<CardId>,
    pub anima: u8,
    pub glimpsed_this_turn: bool,
    /// The Glimpse peek (§5) — the top card this seat saw and chose to KEEP, set
    /// when a Glimpse resolves keep. Owner-visible only (redaction in view.rs);
    /// cleared on the next draw. (BOTTOM clears it — the card is no longer on top.)
    pub peeked_top: Option<CardId>,
    /// Seat A only: first placement of the match is restricted to home rows.
    pub first_placement_done: bool,
}

/// Match rules are match DATA, not constants: 2v2 keeps a different clock,
/// and experiments (alternate contraction, grace levers) are configurations, not
/// forks. Defaults are the 1v1 law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchRules {
    pub last_round: u8,
    /// Set past last_round to play without contraction.
    pub contraction_after: u8,
    /// Held Ground variant: at contraction only EMPTY rim tiles fade;
    /// occupied ones linger (stand, score, act) but accept no new writing,
    /// and fade the moment their spirit leaves or dissolves.
    pub held_ground: bool,
    pub listeners_grace: u8,
    /// Each side's faction, `[A, B]`. Drives deck-building, the bot, and the asymmetric Solace
    /// scoring — when the Solace removes a player's scoring presence, it tallies instead of leaving
    /// a board impression. The turn mechanics stay faction-agnostic. Defaults to Lorekeeper PvP.
    #[serde(default)]
    pub factions: [crate::types::Faction; 2],
}

impl Default for MatchRules {
    fn default() -> Self {
        MatchRules {
            last_round: 12,
            contraction_after: 8,
            held_ground: true,
            listeners_grace: 0,
            factions: [crate::types::Faction::Lorekeeper; 2],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchResult {
    Win(Seat),
    Draw,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Active seat is mid-turn: it Plays / Calls freely (Anima-gated) and moves each Mobile spirit
    /// once, until it ends the turn. There is no fixed action count.
    Acting,
    /// Hand cap hit at Flow: only Release{hand_index} is legal for `seat`.
    PendingRelease { seat: Seat },
    /// own-turn choice: only Choose{index} is legal for `seat`.
    /// What is being chosen lives in `GameState::pending_choice`.
    PendingChoice { seat: Seat },
    Finished {
        result: MatchResult,
        score_a: u8,
        score_b: u8,
    },
}

/// The match aggregate (the family's `MatchState`). Entropy lives OUTSIDE —
/// owned by the Engine/journal — and the seed never
/// enters aggregate state: it is a journal-side datum, revealed post-match.
fn default_board_w() -> i8 {
    5
}
fn default_slot() -> crate::types::SeatSlot {
    crate::types::SeatSlot::A1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameState {
    pub rules: MatchRules,
    /// Set once the Memory has contracted (evolve of MemoryContracted).
    pub contracted: bool,
    pub round: u8,
    pub active: Seat,
    pub phase: Phase,
    pub board: Vec<TileState>, // row-major; Seat A's home rows are y∈{0,1}
    pub player_a: PlayerState,
    pub player_b: PlayerState,
    /// Board width (5 for 1v1, 6 for 2v2).
    #[serde(default = "default_board_w")]
    pub board_w: i8,
    /// In 2v2, the second player on each team (A2, B2). None in 1v1.
    #[serde(default)]
    pub player_a2: Option<PlayerState>,
    #[serde(default)]
    pub player_b2: Option<PlayerState>,
    /// The physical player whose turn it is (A1→B1→A2→B2). In 1v1 this
    /// tracks A1/B1 in lockstep with `active`.
    #[serde(default = "default_slot")]
    pub active_slot: crate::types::SeatSlot,
    /// The Solace's off-board erasure tally: +1 each time the Solace erases a player's scoring
    /// presence — banishing a spirit or unwriting an impression. Added to seat B's score at
    /// Nightfall; the Unwritten leave no board mark, so this tally is how forgetting scores.
    #[serde(default)]
    pub solace_erasures: u8,
    /// Tiles whose spirit has spent its one Move this turn OR arrived this turn (summoning
    /// sickness). A Mobile spirit may Move only if its tile is NOT listed; a move or an arrival adds
    /// the destination. Cleared at each turn start — a per-turn transient like `dissolved_this_turn`.
    #[serde(default)]
    pub moved_this_turn: Vec<u8>,
    /// ThisRound stat modifiers; pruned at round advance.
    #[serde(default)]
    pub temp_mods: Vec<TempMod>,
    /// Seat-scoped this-round reach buffs (Tempestrider Roc, Open Sky, …): each
    /// widens that seat's spirits' arrival-targeting reach until `until_round`.
    /// Pruned at round advance alongside `temp_mods`.
    #[serde(default)]
    pub temp_reach: Vec<TempReach>,
    /// Seat-scoped this-round movement/push restrictions (Stand Ground). Pruned
    /// at round advance alongside `temp_mods`.
    #[serde(default)]
    pub temp_restrict: Vec<TempRestrict>,
    /// Spirits that fully dissolved this match (left an impression), by owner — the pool
    /// Recover draws back to hand (The Returning, The Library Remembers). Tokens
    /// are not recorded (they have no card to return to).
    #[serde(default)]
    pub dissolved: Vec<(Seat, CardId)>,
    /// A one-shot Ritual cost reduction per seat ([A, B]), granted by Star-Strewn
    /// Otter ("your next Ritual costs 1 less") and consumed by the next ritual cast.
    #[serde(default)]
    pub next_ritual_discount: [u8; 2],
    /// A this-round surcharge on a seat's next card ([A, B]) — `(amount, until_round)`,
    /// raised on BOTH seats by Ink Runs Dry ("both Narrators' next card costs 1 more").
    /// Added to the cost of the seat's next play (spirit or Ritual) via `cost_aura`, then
    /// spent (the amount on that seat zeroed); any leftover expires at the round advance.
    #[serde(default)]
    pub card_tax: [(u8, u8); 2],
    /// Count of each seat's spirits that fully dissolved during its current turn
    /// (reset when its turn begins); read by Remember Them.
    #[serde(default)]
    pub dissolved_this_turn: [u8; 2],
    /// own-turn choice in flight (peeked cards are redacted from the
    /// opponent's view).
    #[serde(default)]
    pub pending_choice: Option<PendingChoice>,
    /// Choices a single play opened behind the one in flight (Dig In, The Long
    /// Watch fire several target clauses). FIFO: as each resolves, the next moves
    /// into `pending_choice`. All belong to the acting seat (same-turn), so they
    /// never open a forbidden cross-turn window; never surfaced in a `PlayerView`.
    #[serde(default)]
    pub choice_queue: Vec<PendingChoice>,
    /// Hold the Memory: tiles whose Fading spirit skips ONE Fade step before
    /// dissolving. Tile-keyed is safe — a Fading spirit can't move. Cleared as the
    /// skip is spent at the Fade step.
    #[serde(default)]
    pub fade_delayed: Vec<u8>,
    /// Patience: anima owed to each seat at its NEXT Flow (a played card's delayed
    /// AtFlow/OncePerMatch grant). Paid out and reset at the seat's income step.
    #[serde(default)]
    pub pending_flow_anima: [u8; 2],
    /// Bearer of Small Stones: a Parting grant — this seat's evolutions ignore the
    /// shared-Imprint rule for the rest of THIS turn (set at the Parting's Fade step,
    /// cleared when the seat's turn ends). A transient exception no standing carrier holds.
    #[serde(default)]
    pub ignore_imprint_this_turn: [bool; 2],
    /// Kindle: +Attack this round for the seat's NEXT arriving spirit this turn
    /// (applied + reset at the next arrival; cleared at turn end if unused).
    #[serde(default)]
    pub next_arrival_atk: [i16; 2],
    /// Again!: the seat's NEXT arrival may engage a SECOND target this turn (the
    /// stored value is the attack penalty, e.g. −10). Some = armed; consumed after
    /// the arrival's first engage, or cleared at turn end.
    #[serde(default)]
    pub next_arrival_2nd_engage: [Option<i16>; 2],
    /// Reckless Charge: this-round per-tile retaliation shifts `(tile, delta,
    /// until_round)` — read by combat_stats, pruned at round advance (like temp_mods).
    #[serde(default)]
    pub temp_retaliation: Vec<(u8, i16, u8)>,
    /// Otterling Magus: extra targets the PENDING ritual target choice should also hit
    /// (set when a ritual opens a target choice while you control Otterling; consumed at the
    /// Choose). 0 = none.
    #[serde(default)]
    pub ritual_extra_targets: u8,
    /// Let It Lie: the round during which impressions do not score. None ⇒ impressions score normally.
    #[serde(default)]
    pub impressions_dormant_round: Option<u8>,
    /// Silence Spreads: a silenced Landmark as `(tile, round)` — its text is inert that round.
    #[serde(default)]
    pub silenced_terrain: Option<(u8, u8)>,
    /// The Quiet Spreads: tiles that cannot be Played onto, as `(tile, through_round)`.
    #[serde(default)]
    pub calm_tiles: Vec<(u8, u8)>,
    /// The Half-Remembered: each seat's last FACE-UP spirit played — the memory it copies.
    #[serde(default)]
    pub last_played_spirit: [Option<CardId>; 2],
    /// Curio Fox: face-down Fabrications a seat has privately peeked, as `(tile, card)`.
    /// The view un-redacts a hidden enemy Fabrication for that seat ONLY when the current
    /// terrain at the tile still matches the peeked card (no leak to the opponent's view).
    #[serde(default)]
    pub peeked_fabs: [Vec<(u8, CardId)>; 2],
    /// Active Bonds (auras hold while both spirits stand and adjacent).
    #[serde(default)]
    pub bonds: Vec<Bond>,
    /// A telegraphed surfacing — tile named one round ahead, identity hidden.
    #[serde(default)]
    pub stray_telegraph: Option<StrayTelegraph>,
    /// A surfaced, unclaimed Stray (scores for no one until befriended).
    #[serde(default)]
    pub stray: Option<Stray>,
    /// Whether THIS match was seeded to host a Stray (1-in-7).
    #[serde(default)]
    pub stray_match: bool,
    /// Whether the Solace has told its scheduled Unwriting this round
    /// (the cadence fires once per cadence round). Reset at round advance.
    #[serde(default)]
    pub unwriting_told_this_round: bool,
    /// Mulligan (§5, London-lite): whether each seat `[A, B]` has spent its
    /// once-per-match opening mulligan. PUBLIC — the opponent learns THAT you
    /// mulliganed (a public beat surfaced in the view), never WHAT: the redrawn
    /// hand, the bottomed card, and the new deck order stay hidden by the usual
    /// `PlayerView` redaction. Gates the command to once per seat; the opening
    /// window itself (round 1, before that seat has acted) is checked in `decide`.
    #[serde(default)]
    pub mulliganed: [bool; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrayTelegraph {
    pub tile: u8,
    pub surface_round: u8,
    /// True for the round-11 Midnight Stray (Gentle-weighted).
    pub midnight: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stray {
    pub card: CardId,
    pub tile: u8,
    pub temperament: Temperament,
    /// Wary surfaces veiled (identity hidden until unveiled).
    pub veiled: bool,
    /// Courtship turns accrued (Gentle: 1 adjacency; Wary: 2 consecutive).
    pub courtship: u8,
    /// The seat that was adjacent last turn (for Wary's "consecutive" count).
    pub courted_by: Option<Seat>,
    /// Feral: the Stray's living HP. A Feral Stray fights (intercepts
    /// arrivals) and is befriendable only once wounded below half (an Echo of
    /// pain that finally lets it trust). Gentle/Wary ignore this. Defaults to
    /// the card's printed HP at surfacing.
    #[serde(default)]
    pub hp: i16,
    #[serde(default)]
    pub hp_max: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Temperament {
    Gentle,
    Wary,
    Feral,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bond {
    pub card: CardId,
    pub owner: Seat,
    pub tile_a: u8,
    pub tile_b: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingChoice {
    /// An effect's scry: take one of `looked` to hand; the rest bottom in order.
    Peek { seat: Seat, looked: Vec<CardId> },
    /// Glimpse (§5), step 1 — the BURN cost: the seat chooses which hand card to
    /// spend to activate the Glimpse. `Choose { index }` burns `burnable[index]`
    /// (it leaves play entirely); the keep-or-bottom `Glimpse` then opens behind it.
    /// `burnable` is the seat's hand at the moment of the glimpse (owner-visible
    /// only, redacted from the opponent — see `view.rs`; the field is NOT named
    /// `hand` so it never trips the `"hand":` opponent-leak probe). This is the
    /// activation cost that makes the glimpse a real decision, not a free
    /// every-turn action.
    GlimpseBurn { seat: Seat, burnable: Vec<CardId> },
    /// Glimpse (§5), step 2 — the seat has burned a hand card and peeked its top
    /// card (`top`); now it chooses — `Choose { index: 0 }` KEEPS it on top (no
    /// Anima); `Choose { index: 1 }` BOTTOMS it for +1 Anima. The peeked card is
    /// owner-visible only (redacted from the opponent, who sees only THAT a choice
    /// is in flight — see `view.rs`).
    Glimpse { seat: Seat, top: CardId },
    /// Pick a target tile; the stored effect then applies to it.
    Target {
        seat: Seat,
        options: Vec<u8>,
        effect: ChoiceEffect,
        source: u8,
    },
    /// Recover: take one of your fully-dissolved spirits (`options`) back to hand.
    Recover { seat: Seat, options: Vec<CardId> },
}

impl PendingChoice {
    /// The seat this choice belongs to. THE single place the variant→seat mapping
    /// lives — the redaction filters in `view.rs` and the resume routing in
    /// `engine` all defer here, so a new variant cannot drift one copy of the
    /// match out of sync with another (a pending choice leaking to the wrong seat
    /// is a redaction break, invariant 2).
    pub fn seat(&self) -> Seat {
        match self {
            PendingChoice::Peek { seat, .. }
            | PendingChoice::GlimpseBurn { seat, .. }
            | PendingChoice::Glimpse { seat, .. }
            | PendingChoice::Target { seat, .. }
            | PendingChoice::Recover { seat, .. } => *seat,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChoiceEffect {
    PushAway {
        tiles: u8,
    },
    RestoreForm {
        amount: i16,
    },
    StatDelta {
        attack: i16,
        defense: i16,
        form: i16,
        this_round: bool,
    },
    /// Direct damage to the chosen spirit (Stoke: damage your own ally for value).
    Damage {
        amount: i16,
    },
    /// Erase the impression on the chosen tile (Scrub the Margin).
    RemoveImpression,
    /// Grant the chosen spirit a Keyword until `until_round` (Dig In: this round;
    /// The Long Watch: `u8::MAX` permanent).
    GrantKeyword {
        keyword: crate::effects::Keyword,
        until_round: u8,
    },
    /// Two-step displacement, step 1 (DECIDED ruling): the chosen spirit is the one
    /// to move; resolving this opens a second Target over the empty tiles within
    /// `tiles` of it (Quiet Step, Misstep, Cold Spot — a played-card push is
    /// player-directed, so it shares this seam with MoveAny).
    DisplaceFrom {
        tiles: u8,
    },
    /// Two-step displacement, step 2: move the spirit at `from` to the chosen tile.
    MoveTo {
        from: u8,
    },
    /// Two-adjacent-allies buff, step 1 (Round, Close Ranks): the chosen ally is the
    /// first of the pair; resolving opens a choice over its adjacent allies.
    PairBuffStep1 {
        attack: i16,
        defense: i16,
        this_round: bool,
    },
    /// Two-adjacent-allies buff, step 2: buff both `from` and the chosen ally.
    PairBuffWith {
        from: u8,
        attack: i16,
        defense: i16,
        this_round: bool,
    },
    /// Swap, step 1 (Behind You): the chosen enemy is one of the pair; resolving
    /// opens a choice over the enemies adjacent to it.
    SwapStep1,
    /// Swap, step 2: exchange the spirits at `from` and the chosen tile.
    SwapWith {
        from: u8,
    },
    /// Hold the Note: the chosen tile is one endpoint of a Bond; restore both
    /// endpoints by `amount`.
    HealBondedPair {
        amount: i16,
    },
    /// The Fog of Elsewhere: return the chosen spirit to its owner's hand.
    Bounce,
    /// Sudden Clearing: flip the chosen face-down Fabrication face-up, publicly.
    RevealFabricationAt,
    /// Tailwind: grant the chosen spirit a FULL reach buff this round (per-spirit).
    ReachBuff {
        forward: i8,
        all_directions: bool,
    },
    /// Again! / Reckless Charge: the spirit at `from` engages the chosen target (a
    /// full exchange), with `bonus` to its attack (a penalty, e.g. −10).
    EngageFrom {
        from: u8,
        bonus: i16,
    },
    /// Reckless Charge, step 1: the chosen ally is the engager; resolving opens a
    /// target choice over the enemies in its reach (→ EngageFrom).
    EngageStep1 {
        bonus: i16,
    },
    /// Reckless Charge: the chosen ally's retaliation is shifted `delta` this round.
    RetaliateThisRound {
        delta: i16,
    },
    /// Ragewoken Bison: pay `hp` (self-damage to `from`) then engage the chosen target
    /// at `bonus`. Choosing `from` itself DECLINES (the "may pay" — no pay, no engage).
    PayEngage {
        from: u8,
        hp: i16,
        bonus: i16,
    },
    /// Second Farewell: fire the chosen ally's Parting effect again (the spirit stays).
    ReTriggerParting,
    /// Curio Fox: privately look at the chosen face-down Fabrication (records it in the
    /// owner's peeked_fabs; the opponent's view is unaffected).
    PeekFabrication,
    /// Don't Look: the chosen enemy can't engage or intercept through `until_round`.
    RestrictEngage {
        until_round: u8,
    },
    /// Hold the Memory: the chosen Fading spirit skips its next Fade step.
    DelayFade,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TempMod {
    pub tile: u8,
    pub attack: i16,
    pub defense: i16,
    pub until_round: u8,
}

pub(crate) fn default_true() -> bool {
    true
}

/// A this-round reach buff covering a seat's spirits (seat-wide) or one tile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TempReach {
    pub seat: Seat,
    pub forward: i8,
    pub all_directions: bool,
    pub until_round: u8,
    /// True = widens TARGETING only (Tempestrider Roc &c., the original buffs).
    /// False = a FULL reach buff (Tailwind, Open Sky) — also widens projection and
    /// interception. Defaults true so older states keep the targeting-only behavior.
    #[serde(default = "crate::state::default_true")]
    pub targeting_only: bool,
    /// `Some(tile)` = a per-spirit buff (Tailwind); `None` = seat-wide (Open Sky).
    #[serde(default)]
    pub tile: Option<u8>,
}

/// A this-round restriction covering a whole seat's spirits (Stand Ground:
/// can't be pushed or moved this round).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TempRestrict {
    pub seat: Seat,
    pub restriction: crate::effects::Restriction,
    pub until_round: u8,
}

impl GameState {
    pub fn player(&self, seat: Seat) -> &PlayerState {
        match seat {
            Seat::A => &self.player_a,
            Seat::B => &self.player_b,
        }
    }
    /// The PlayerState for a physical slot (hands/anima are per-slot).
    /// In 1v1 only A1/B1 exist and resolve to player_a/player_b.
    pub fn player_slot(&self, slot: crate::types::SeatSlot) -> &PlayerState {
        use crate::types::SeatSlot::*;
        match slot {
            A1 => &self.player_a,
            B1 => &self.player_b,
            A2 => self.player_a2.as_ref().unwrap_or(&self.player_a),
            B2 => self.player_b2.as_ref().unwrap_or(&self.player_b),
        }
    }
    pub fn player_slot_mut(&mut self, slot: crate::types::SeatSlot) -> &mut PlayerState {
        use crate::types::SeatSlot::*;
        match slot {
            A1 => &mut self.player_a,
            B1 => &mut self.player_b,
            A2 => self.player_a2.as_mut().expect("A2 exists in 2v2"),
            B2 => self.player_b2.as_mut().expect("B2 exists in 2v2"),
        }
    }
    /// Is this a 2v2 match?
    pub fn is_2v2(&self) -> bool {
        self.player_a2.is_some()
    }
    pub fn player_mut(&mut self, seat: Seat) -> &mut PlayerState {
        match seat {
            Seat::A => &mut self.player_a,
            Seat::B => &mut self.player_b,
        }
    }
    pub fn spirit_at(&self, t: u8) -> Option<&Spirit> {
        self.board[t as usize].spirit.as_ref()
    }
}

/// A player's intent. Validated by decide — never trusted.
/// The Arrival Law: combat is born from arrival — placement, Overwrite,
/// or a Mobile step. Glimpse replaces Pass; Release resolves the hand cap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Place a spirit. `engage` optionally strikes on arrival; `chain_prefs`
    /// is the ordered chain-target preference list — each Momentum link
    /// takes the first legal entry, falling back to the engine's heuristic.
    PlaySpirit {
        hand_index: u8,
        tile: u8,
        engage: Option<u8>,
        #[serde(default)]
        chain_prefs: Vec<u8>,
    },
    Overwrite {
        hand_index: u8,
        tile: u8,
    },
    MoveSpirit {
        from: u8,
        to: u8,
        engage: Option<u8>,
    },
    /// Glimpse (§5): the once-per-turn burn-then-peek. Burn a hand card (the
    /// activation cost — it leaves play), then see your top card and KEEP it or
    /// BOTTOM it for +1 Anima. Resolves through two `Choose` steps (`GlimpseBurn`
    /// then the keep-or-bottom `Glimpse`).
    Glimpse,
    Release {
        hand_index: u8,
    },
    EndTurn,
    /// The Solace plays an Unwriting event from hand — a one-shot, like `CastRitual`.
    TellUnwriting {
        hand_index: u8,
    },
    /// resolve the pending own-turn choice.
    Choose {
        index: u8,
    },
    /// Standing Orders (free action): Held spirits never intercept.
    SetOrders {
        tile: u8,
        hold: bool,
    },
    /// The fourth arrival form — a Lurker steps into the light, and
    /// may engage from the reveal (Arrival Law).
    Reveal {
        tile: u8,
        engage: Option<u8>,
    },
    /// Follow-up: a standing spirit at `from` springs an enemy face-down
    /// Fabrication in its reach (counterplay — clear the lie from range,
    /// eating the trap but removing the projection-anchor). The striker stays.
    StrikeFabrication {
        from: u8,
        tile: u8,
    },
    /// Cast a Ritual from hand (its effect resolves immediately).
    CastRitual {
        hand_index: u8,
    },
    /// Attach a Bond to two of your adjacent standing spirits.
    AttachBond {
        hand_index: u8,
        tile_a: u8,
        tile_b: u8,
    },
    /// Place a Landmark on an empty tile within your projection.
    PlaceLandmark {
        hand_index: u8,
        tile: u8,
    },
    /// Set a Fabrication face-down within your projection.
    SetFabrication {
        hand_index: u8,
        tile: u8,
    },
    /// Evolve by **playing a form card from hand onto its matching base** during
    /// Main. `form_hand` is the hand index of the Primal/Fabled form card (a deck-playable
    /// card you drew); `tile` is the owned base it lands on. The form's `evolves_from` must
    /// name the base, the shared-Imprint rule must admit it, and the base-state ↔ form-type
    /// pairing is strict: a **Primal** lands on a **Fading** base (its own last becoming,
    /// the fade the fuel — `fuel` is None), a **Fabled** lands on a **healthy** base the
    /// turn after it arrived (donor-fueled — `fuel` is Some(donor_tile), a standing-or-fading
    /// ally spent). Cost is the form's cost − ⌊base.cost/2⌋ (cost-aura adjusted). `engage`
    /// is an optional arrival strike — both forms may strike on arrival, like any other
    /// arrival (Kindred/Reveal/Overwrite).
    Evolve {
        tile: u8,
        form_hand: u8,
        fuel: Option<u8>,
        engage: Option<u8>,
    },
    /// Devolution (design §5) — **recede a banished form to a base** during Main, the
    /// rescue. `tile` is a STANDING-FADED form you own (a Primal/Fabled banished in
    /// combat, still in its §0.5 window: `fading` + `fade_deadline` Some); `base_hand`
    /// is the hand index of a **base card in that form's line** (the form's
    /// `evolves_from` chain) you play onto it. The base arrives at FULL HP with its
    /// fade cleared (rescued, one tier down) and is **summoning-sick** until the
    /// owner's next turn. It costs **half the banished form's Anima, rounded down**, and
    /// **is an arrival, symmetric with evolution** (the maintainer's ruling): it fires the
    /// same arrival triggers — `check_throughline` (a base receding into a standing 3-line
    /// re-completes on the spot) and a queued next-arrival buff — but **engages no one**
    /// (no strike target) and fires **no OnPlay**. A spirit
    /// may cycle evolve↔devolve without limit. Vocabulary: the engine command is
    /// `Devolve`; the Lorekeeper **reverts**, the Solace **recedes** in player-facing
    /// text/UI/log. Offered in `legal_commands` when valid; redaction-safe (the
    /// opponent sees the Devolve + the resulting base, never the rest of your hand).
    Devolve {
        tile: u8,
        base_hand: u8,
    },
    /// Banish a surfaced Stray (a normal banisher's impression).
    BanishStray,
    /// Fade reclaim (voluntary): cash one of your own standing spirits back to the
    /// page — it leaves (no impression), its Parting fires, and you regain ⌊cost/2⌋ Anima
    /// (full for Last-Light Koi). See the design doc's Fade-reclaim rule.
    Reclaim {
        tile: u8,
    },
    /// Absence forfeit: the platform issues this
    /// after 120s of continuous absence. A SYSTEM-kind command — never offered in
    /// `legal_commands`, resolvable on EITHER seat's turn (it precedes the
    /// turn-ownership check in `decide`); `seat` is who abandoned. The transport
    /// layer gates who may issue it; the engine just resolves it, journaled.
    MatchAbandoned {
        seat: Seat,
    },
    /// Mulligan (§5 — London-lite): the seat's once-per-match opening reshuffle.
    /// Draw a fresh full hand from the (reshuffled) deck, then bottom one card —
    /// the cost — chosen deterministically from the seed (no player pick, so it
    /// stays a single atomic command, never a choice prompt). Legal ONLY in the
    /// opening window: round 1, the active seat's own turn, before that seat has
    /// acted, and at most once per seat (`GameState::mulliganed`). The opponent
    /// learns THAT it happened (a public beat in the view) but never the cards —
    /// the redraw is redacted by the usual `PlayerView` rules. Unlike a normal
    /// turn move it is offered by `legal_commands` (it IS a player choice), but
    /// it precedes the action loop, like the opener's first beat.
    Mulligan {
        seat: Seat,
    },
}

mod events;
pub use events::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrikeKind {
    Engage,
    Retaliation,
    /// After the arrival's engage, BEFORE any Momentum chain: the brake.
    Interception,
    /// Momentum link n (1-based). Base grants one; Relentless continues.
    Chain(u8),
    OverwriteExchange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimaReason {
    Income,
    Glimpse,
    /// Granted by a card effect.
    Effect,
}
