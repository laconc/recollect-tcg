//! The effect IR. Data only: these types describe what cards DO — `effects.json`
//! maps every canon card onto them, and the engine interprets them (no per-card
//! branches).
//!
//! Shape: an [`EffectSpec`] binds a [`Trigger`] (when), an optional
//! [`Condition`] (gate), and a list of [`Clause`]s — each clause aims an
//! [`Effect`] (what) at a [`Selector`] (whom) for a [`Duration`] (how long).
//! Card rulings that are NOT effects: Foundling temperament notes are
//! Stray-runtime behavior; Solace spawn/flavor lines are Solace director
//! policy; pure flavor maps to [`Effect::NoEffect`].

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

/// The authored canon effects: every behavior-bearing playable card (and the
/// six Kindred) mapped to its [`EffectSpec`]s, authored as the `[[card.effect]]`
/// blocks in `data/cards.toml` and generated into this file by `make catalog`
/// (NEVER parsed from rules prose). Drift-gated by tests/effects_coverage.rs.
/// The `pending` list is the explicit mark for not-yet-authored cards.
pub fn canon_effects() -> &'static EffectsFile {
    static FILE: OnceLock<EffectsFile> = OnceLock::new();
    FILE.get_or_init(|| {
        serde_json::from_str(include_str!("../data/effects.json")).expect("effects.json parses")
    })
}

/// The authored [`EffectSpec`]s for a card, resolved **by id** — the one seam
/// the engine should reach for a card's behavior. It folds the whole id → key →
/// spec chain ([`canon_effects`] is keyed by the card's frozen `key`) into a
/// single call: resolve the card once (O(1) index on the dense canon catalog,
/// the same scan-fallback as the engine's `card` for sparse test catalogs), then
/// look the key up — so hot scans never clone a `name` to carry it to the lookup.
/// `None` for a card with no authored spec (a vanilla spirit), or one absent from
/// the catalog.
///
/// The canon catalog populates `key` directly (the O(1) path). A hand-built
/// **test** catalog leaves `key` empty but names its cards after canon ones, so
/// an empty key falls back to [`crate::cards::key_of`] over the display `name` —
/// the same name→key bridge the engine used before this seam existed.
pub fn specs_for(
    catalog: &[crate::types::CardDef],
    id: crate::types::CardId,
) -> Option<&'static [EffectSpec]> {
    // Dense-index the canon catalog (its hot path), with a scan fallback for the
    // sparse/unsorted catalogs some tests build — same resolution as the engine's
    // `card` helper.
    let def = match catalog.get(id.0 as usize) {
        Some(d) if d.id == id => d,
        _ => catalog.iter().find(|c| c.id == id)?,
    };
    let key = if def.key.is_empty() {
        crate::cards::key_of(&def.name)
    } else {
        def.key.as_str()
    };
    canon_effects().specs.get(key).map(Vec::as_slice)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectsFile {
    pub specs: HashMap<String, Vec<EffectSpec>>,
    pub pending: Vec<String>,
    /// Cards whose text is CONTROLLER DISPOSITION, not card behavior — how
    /// the wild seat plays the piece (escort whom, befriend whom, surface
    /// where). Same reason the bot's strategy isn't in this file. Owned by
    /// the behavior table; validated against the catalog like specs.
    #[serde(default)]
    pub behavior: Vec<String>,
}

/// When an effect fires. `Static` effects don't fire — they hold while the
/// source is present (auras), gated by their [`Condition`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trigger {
    /// Resolves on arrival (spirit enters) or on cast (Ritual / Unwriting).
    OnPlay,
    /// The source leaves the board — banished, Faded, or released.
    Parting,
    /// After the source's engage resolves (both strikes settled).
    OnEngageResolved,
    /// The source (or its bonded pair) defeats an enemy.
    OnDefeat,
    /// Any spirit fully dissolves, anywhere (Mourner-class).
    OnAnyBanish,
    /// Unwritten only: the source Unwrites something.
    OnUnwrite,
    /// The source moves.
    OnMove,
    /// At the owner's Flow (economy step).
    AtFlow,
    /// A face-down Fabrication is revealed by an engager (Traps).
    OnReveal,
    /// No firing moment: holds while present (aura), per [`Condition`].
    Static,
    /// An ally begins Fading (Keenling).
    OnAllyFading,
    /// You play a Bond (Linnet of the Lea).
    OnYouPlayBond,
    /// Attune's condition becomes true (The Unfiled).
    OnAttuned,
    /// A Foundling is befriended.
    OnBefriend,
    /// one of the owner's Throughlines completes (Vale Eternal: draw 2, gain 2 Anima).
    OnThroughlineComplete,
    /// Any Fabrication is revealed anywhere (Magistrate of Masks: this +10/+10).
    OnFabricationRevealed,
    /// This spirit defeats a WARDED enemy (Warden-Breaker, Crowned in Smoke: this +20/+20).
    OnDefeatWarded,
    /// An allied spirit Parts within this spirit's Reach (Elegist Wren: this +10/+10).
    OnAllyPartsInReach,
}

/// Gates on a spec or clause. `Always` for unconditional.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Condition {
    Always,
    WhileDamaged,
    /// Attune: adjacent to `n`+ allies sharing a Resonance.
    AdjacentAlliesShareResonance {
        n: u8,
    },
    /// Bond clauses that require the pair adjacent.
    PairAdjacent,
    /// Target/engager cost gate (e.g. The Perfect Lie: Cost ≤ 3).
    CostAtMost {
        cost: u8,
    },
    /// Fires at most once per match (Unyielding, Promise).
    OncePerMatch,
    /// Fires at most once per round.
    OncePerRound,
    /// May-pay gates (Ragewoken Bison: pay 20 HP to…).
    PayForm {
        amount: i16,
    },
    /// The Graven Elder: +20 Defense while undamaged.
    WhileUndamaged,
    /// Duetling: while adjacent to at least one ally.
    WhileAdjacentToAlly,
    /// Madrigal: the source still lurks face-down.
    WhileFaceDown,
    /// Gather In: bonus clause if you control a Bond.
    YouControlABond,
}

/// Whom a clause touches. Selection happens at resolution under the law of
/// the design doc (owner chooses where the text says "an"/"a").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Selector {
    SelfSpirit,
    Owner,
    BothNarrators,
    /// The spirit that engaged the source (Traps, retaliation riders).
    Engager,
    /// The enemy the source engaged.
    EngagedEnemy,
    /// Either surviving party of the source's engage (Vertigo).
    Survivor,
    /// Owner picks one adjacent ally.
    AdjacentAllyChoose,
    /// Owner picks one adjacent enemy.
    AdjacentEnemyChoose,
    AdjacentAlliesAll,
    AdjacentEnemiesAll,
    AllOtherSpirits,
    AlliesAll,
    EnemiesAll,
    AlliesInReach,
    /// The other spirit of this Bond.
    BondedPartner,
    /// Spirits standing on this Landmark's tile.
    OccupantsHere,
    /// Enemies adjacent to any allied spirit of a Resonance (Eruption).
    EnemiesAdjacentToAlliesOf {
        resonance: crate::types::Resonance,
    },
    /// Madrigal: enemies within `tiles` of the source.
    EnemiesWithinTiles {
        tiles: u8,
    },
    /// Choir of the Vale: allied spirits carrying an Imprint.
    AlliesWithImprint {
        imprint: String,
    },
    /// Bonds: the two spirits this Bond joins.
    BondedPair,
    /// Landmarks: spirits standing on this terrain's tile.
    OccupantHere,
    /// Landmarks: occupant plus orthogonally adjacent allies.
    OccupantAndAdjacentAllies,
    /// Marrow Whisperer: enemies while engaging the source's adjacent allies.
    EnemiesEngagingAdjacentAllies,
    /// Ritual targeting (caster chooses).
    TargetSpirit,
    TargetAllySpirit,
    TargetEnemySpirit,
    TargetTwoAdjacentAllies,
    TargetBondedPair,
    TargetFadingSpirit,
    /// Kindle / Again!: your next spirit to arrive this turn.
    NextArrivalThisTurn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Duration {
    Instant,
    ThisRound,
    NextRound,
    /// While the source remains on the board (auras, Bonds, Landmarks).
    WhilePresent,
    Permanent,
}

/// Displacement family: who moves whom, how.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Displacement {
    Push {
        tiles: u8,
    },
    Pull {
        tiles: u8,
    },
    /// Controller moves the target any direction.
    MoveAny {
        tiles: u8,
    },
    /// Swap two spirits' positions (Behind You).
    Swap,
}

/// What an action may no longer do (Restrict) — Held Ground's "accepts no
/// new writing" stays a board law, not an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Restriction {
    Engage,
    Move,
    BePushed,
    BeTargetedByRituals,
    GainImpressions,
}

/// Bounded bespoke: named rule exceptions. Each variant is one card's (or
/// one small family's) license to bend a named rule. Adding a variant is a
/// design decision — cite the card and the doc section in review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleException {
    /// Bearer of Small Stones: evolutions ignore the shared-Imprint rule this turn.
    EvolveIgnoresSharedImprint,
    /// Otterling Magus: your Rituals affect one extra target.
    RitualsExtraTarget,
    /// Matron of the Long Goodbye: spirits you evolve arrive +10/+10.
    EvolveArrivesBuffed,
    /// Duckling Following Anyone: ALL Imprints count as shared.
    AllImprintsShared,
    /// Twin Telling: each of the pair counts as having the other's Imprints.
    PairSharesImprints,
    /// Unbreakable: the Bond persists unless separated by more than 1 tile.
    BondHoldsUnlessSeparated,
    /// Waystone: Mobile spirits may move 2 from here.
    MobileMovesTwoFromHere,
    /// Last-Light Koi: Parting reclaims full Cost instead of half.
    PartingReclaimsFullCost,
    /// Shrine of the Nameless: while a fading owned spirit rests on this Landmark, the
    /// owner's evolutions ignore the shared-Imprint rule (consulted positionally in
    /// legal_evolutions, NOT through exception_active).
    FadingFuelIgnoresImprint,
    /// The Long Rest (Unwritten): spirits that fully dissolve ADJACENT to it leave no impression.
    NoImpressionOnAdjacentDissolve,
    /// The Closing Book (Unwritten): cannot be intercepted on arrival.
    ImmuneToInterception,
    /// The Forgiven Debt (Unwritten): cannot be banished by an attacker at Echo (below half
    /// HP) — a lethal blow from such a spirit is capped to leave it at 1 HP.
    UnbanishableByEcho,
    /// Ferrier of the Salt Road: while present, the owner's Fade reclaim regains +1 Anima.
    FadeReclaimsExtraAnima,
    /// The Unforgiving (ill intent, Arcane): its strikes ignore the defender's Warded — so its
    /// arcane pierce lands even on a Warded defender (which would normally negate it).
    StrikesIgnoreWarded,
    /// Hush (Unwritten): a spirit adjacent to it cannot trigger its Parting.
    SuppressesAdjacentParting,
    /// The Lullaby (Unwritten): enemy spirits adjacent to it lose Echo eligibility.
    SuppressesAdjacentEnemyEcho,
    /// The Vigilkeeper / Rainpool: Parting effects trigger twice.
    PartingTriggersTwice,
    /// Zenith: your Glimpses look at one extra card.
    GlimpseLooksOneMore,
    /// Rondel, the Joining: your Bonds cost 0.
    BondsCostZero,
    /// Common Ground: a Bond is free if either endpoint stands on this Landmark.
    /// POSITIONAL — read at the bond-attach chokepoint by tile, NOT seat-wide via
    /// `exception_active` (which is selector-agnostic and would zero every Bond).
    BondsFreeOnLandmark,
    /// Erasure's Patience (Unwritten): while it stands, the impressions on tiles
    /// ORTHOGONALLY ADJACENT to it do not score at Nightfall (the marks near it cool
    /// one by one). POSITIONAL — read at the scoring chokepoint (`flow.rs`) by tile,
    /// not seat-wide via `exception_active`.
    AdjacentImpressionsDontScore,
}

/// What happens instead — the replacement-effect family (Unyielding, Promise).
/// Resolves BEFORE the replaced event is journaled: the replaced thing never
/// happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Replacement {
    /// The first time the source would be banished: survives at `form` HP.
    SurviveBanishAt { form: i16 },
    /// If the source would Fade/take lethal, the bonded partner takes it.
    PartnerTakesIt,
    /// Hold the Memory: a Fading spirit dissolves one Fade step later.
    DelayFadeOneStep,
}

/// The primitives. Every canon rules text resolves to clauses of these (or
/// to a documented runtime ruling).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    StatDelta {
        attack: i16,
        defense: i16,
        form: i16,
    },
    /// Sworn Shields / Borrowed Courage: pair shares the higher stat.
    StatShareHigher {
        attack: bool,
        defense: bool,
    },
    /// Reach auras are targeting-only unless stated.
    ReachDelta {
        forward: i8,
        all_directions: bool,
        targeting_only: bool,
    },
    AnimaDelta {
        amount: i8,
    },
    RestoreForm {
        amount: i16,
    },
    Damage {
        amount: i16,
    },
    /// Glimpse: look at `look`, take `take`, bottom the rest.
    PeekDeck {
        look: u8,
        take: u8,
    },
    Draw {
        count: u8,
    },
    Displace(Displacement),
    /// Return to its owner's hand.
    Bounce,
    /// From the banished, into your hand (The Returning).
    Recover,
    CostDelta {
        delta: i8,
    },
    GrantKeyword {
        keyword: Keyword,
    },
    /// Attune resolution: gains the shared Resonance.
    GrantSharedResonance,
    RetaliationDelta {
        amount: i16,
    },
    /// Conspiracy / Pack Tactics: an out-of-turn engage or pre-chip.
    GrantEngage {
        immediate: bool,
        pre_chip: i16,
    },
    /// Ragewoken Bison: an additional engage this arrival.
    ExtraEngage,
    /// Momentum rule modifiers (Sparkfather, Brand-Bearer). `per_link_bonus` is extra Attack
    /// added to MOMENTUM_PER_LINK on each chain link (Embermane: +10 → +20 per link).
    MomentumMod {
        first_engage_bonus: bool,
        chain_while_defeating: bool,
        #[serde(default)]
        per_link_bonus: i16,
        /// Pyrrhic: the spirit's chain strikes from link 2 on take no retaliation.
        #[serde(default)]
        chain_no_retaliation: bool,
    },
    /// Badgermarshal: adjacent allies take `amount` LESS damage from enemy Momentum chains.
    ChainDamageReductionAura {
        amount: i16,
    },
    /// Grudge-Kept (ill intent): +`amount` Attack for each ENEMY impression on the board.
    AttackPerEnemyImpression {
        amount: i16,
    },
    /// The Worst Version (ill intent): its Attack copies the highest among enemy spirits.
    CopyHighestEnemyAttack,
    /// Footnote / Sentence Fragment (Unwritten): on Unwrite, mill a deck's top card —
    /// the owner's (Footnote) or the opponent's (Sentence Fragment "steals a word").
    MillTopDeck {
        opponent: bool,
    },
    /// The Half-Remembered (Unwritten): on reveal, copy the stats of the owner's last
    /// face-up spirit played (it becomes that memory).
    CopyLastPlayed,
    /// The Almost-Said (Unwritten): copy the Reach of the enemy that just engaged it.
    CopyEngagerReach,
    Restrict(Restriction),
    /// Eat or place an impression.
    ImpressionEat,
    ImpressionPlace {
        seat_color: bool,
    },
    /// The Page Turns: shift every standing Unwritten one tile toward the Memory's center.
    ShiftUnwrittenInward,
    /// A Mercy for the Rim: release every held spirit standing on a rim tile (no impression).
    ReleaseHeldRim,
    /// Silence Spreads: the first standing Landmark loses its text this round.
    SilenceLandmark,
    /// Lost Paragraph: manifest a cost-2 Unwritten on a free rim tile.
    ManifestUnwrittenOnRim,
    /// Let It Lie: impressions stop scoring for a round (go dormant).
    StopImpressionScoring,
    /// The Quiet Spreads: two inner tiles go calm (uncallable) next round.
    ScheduleCalmTiles,
    /// Look at a face-down Fabrication.
    RevealFabrication,
    /// Call / Solace manifestation: summon by card name.
    Summon {
        card_name: String,
    },
    /// The Misremembered: copies printed A/D of the last spirit it fought.
    CopyPrintedStats,
    /// The Perfect Lie: take control of the engaging spirit.
    TakeControl,
    /// Chorus-class stacking counters with a cap.
    CounterAura {
        trait_name: String,
        max: u8,
    },
    /// Second Farewell: re-trigger a Parting.
    ReTriggerParting,
    /// Grief Split: damage to one may be redirected to the other.
    RedirectDamageToPartner,
    Replace(Replacement),
    Exception(RuleException),
    /// Silence / strip / blur printed text or traits.
    TraitSilence,
    TraitStrip,
    /// Scaling rituals (What Remains / Harvest Together / Remember Them).
    AnimaPerBanishedAlly {
        max: u8,
    },
    AnimaPerAdjacentAlliedPair {
        max: u8,
    },
    DrawPerBanishedThisTurn {
        max: u8,
    },
    /// Scrub the Margin: remove an impression from a tile.
    ImpressionRemoveTarget,
    /// Again!: next arrival may engage a second target at a penalty.
    SecondTargetEngage {
        attack_penalty: i16,
    },
    /// Queen of the Quiet Garden: your Throughline completion grants this much EXTRA
    /// stat (added on top of the base +10/+10, summed over your Queens).
    ThroughlineGrant {
        attack: i16,
        defense: i16,
    },
    /// Herald of the Ill Intent: an enemy pushed onto an Impression-bearing tile takes this much damage
    /// (a standing Owner aura, honored in push_away).
    ImpressionPushDamage {
        amount: i16,
    },
    /// Whisperer at the Door: an enemy ENGAGING this spirit has its Attack shifted by
    /// `attack` (a defensive engage-aura, applied in combat_stats via the `vs` context).
    EngagerAttackDelta {
        attack: i16,
    },
    /// Duet Ascendant, Both Halves: a standing spirit grants every one of the owner's
    /// bonded spirits this extra stat (a seat-wide bond amplifier, like Rondel's share).
    BondStatGrant {
        attack: i16,
        defense: i16,
    },
    /// The First Forgotten: enemies lose Resonance edge against your spirits.
    EdgeNegate,
    /// Adamant: flat damage reduction from all sources.
    DamageReduction {
        amount: i16,
    },
    /// Foal: grows +step/+step at the controller's Flow, capped.
    GrowEachFlow {
        step: i16,
        max: i16,
    },
    /// The Solace's mercy: release a **fading** spirit from the board leaving NO
    /// impression — a gentle ending for the dying (The Kind Erasure, The Mercy
    /// Itself, The Soft Close, and the merciful Deepenings). Targeting is
    /// **fading-only**: the card text is "release every adjacent *fading* spirit",
    /// so a healthy spirit is never touched (the aggressive "banish a healthy
    /// enemy, no impression" line is [`Effect::Banish`], a different effect).
    Release,
    /// The IllIntent erasure: **banish** a spirit outright — any owner, any state —
    /// leaving NO impression, as if it never was (You Were Never Really Here, I Too
    /// Can Create Desolation). The aggressive cousin of [`Effect::Release`]: where
    /// Release spares only the dying, Banish takes the living. No impression either
    /// way, so the keeper loses both the spirit AND the tile's score.
    Banish,
    /// Bluffs and pure flavor: explicitly nothing.
    NoEffect,
}

/// The big-five keywords plus Lurk (Lurk is a keyword, not an effect).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Keyword {
    Arcane,
    Warded,
    Mobile,
    Steadfast,
    Relentless,
    Lurk,
}

/// One aimed effect: what, at whom, for how long.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clause {
    pub selector: Selector,
    pub effect: Effect,
    pub duration: Duration,
}

/// A card's complete behavior: when, gated how, doing what. A card may
/// carry several specs (e.g. a keyword line plus a Parting line).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectSpec {
    pub trigger: Trigger,
    pub condition: Condition,
    pub clauses: Vec<Clause>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_roundtrips(spec: &EffectSpec) {
        let json = serde_json::to_string(spec).unwrap();
        assert_eq!(&serde_json::from_str::<EffectSpec>(&json).unwrap(), spec);
    }

    /// Worked examples — five real canon cards expressed in the IR, proving
    /// the shape holds at the corners it was designed from.
    #[test]
    fn census_worked_examples_express_in_the_ir() {
        // Frenzy — +10 Attack while damaged (static, conditional, self).
        spec_roundtrips(&EffectSpec {
            trigger: Trigger::Static,
            condition: Condition::WhileDamaged,
            clauses: vec![Clause {
                selector: Selector::SelfSpirit,
                effect: Effect::StatDelta {
                    attack: 10,
                    defense: 0,
                    form: 0,
                },
                duration: Duration::WhilePresent,
            }],
        });
        // Ashen Sparrow — Parting: an adjacent ally +10/+10.
        spec_roundtrips(&EffectSpec {
            trigger: Trigger::Parting,
            condition: Condition::Always,
            clauses: vec![Clause {
                selector: Selector::AdjacentAllyChoose,
                effect: Effect::StatDelta {
                    attack: 10,
                    defense: 0,
                    form: 10,
                },
                duration: Duration::Permanent,
            }],
        });
        // Glimpse Beyond — look at top 3, take 1, bottom the rest.
        spec_roundtrips(&EffectSpec {
            trigger: Trigger::OnPlay,
            condition: Condition::Always,
            clauses: vec![Clause {
                selector: Selector::Owner,
                effect: Effect::PeekDeck { look: 3, take: 1 },
                duration: Duration::Instant,
            }],
        });
        // Anchorite Ox — Unyielding: first banish each match, survives.
        spec_roundtrips(&EffectSpec {
            trigger: Trigger::Static,
            condition: Condition::OncePerMatch,
            clauses: vec![Clause {
                selector: Selector::SelfSpirit,
                effect: Effect::Replace(Replacement::SurviveBanishAt { form: 10 }),
                duration: Duration::WhilePresent,
            }],
        });
        // Eruption — 20 damage to every enemy adjacent to your Flame spirits.
        spec_roundtrips(&EffectSpec {
            trigger: Trigger::OnPlay,
            condition: Condition::Always,
            clauses: vec![Clause {
                selector: Selector::EnemiesAdjacentToAlliesOf {
                    resonance: crate::types::Resonance::Fury,
                },
                effect: Effect::Damage { amount: 20 },
                duration: Duration::Instant,
            }],
        });
    }
}

/// Pre-release the schema is simply 1 and changes freely (owner decision);
/// the whole-enum versioning ceremony begins at first release.
pub const EVENTS_SCHEMA_VERSION: i32 = 1;

mod support;
pub use support::*;
