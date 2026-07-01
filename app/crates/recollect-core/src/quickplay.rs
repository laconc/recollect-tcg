//! Quick Play: seeded, style-weighted random decks that are always legal.
//!
//! The player is offered a few deck STYLES (seeded choice of three), picks
//! one, and `generate_deck(style, seed)` builds a valid deck — DECK_SIZE
//! cards, max two copies, opening-curve guaranteed. Generation is a pure
//! function of (style, seed, catalog), so the server re-derives the exact
//! deck during journal verification and the client can build it offline:
//! Quick Play rides the same determinism covenant as everything else.
//! (Per the AI adversaries doc, the "vs AI" opponent is also replayable.)
use crate::cards::DECK_SIZE;
use crate::rng::Rng;
use crate::types::{CardDef, CardId, Resonance};

/// A Quick Play deck archetype the player may pick. Carries BOTH a subjective
/// `blurb` (the voice — how the deck *feels*) and an objective
/// [`SelectionInfo`] (the shape — what the deck *is*), so the picker can show a
/// player the flavor AND let them choose informedly. The objective traits
/// are not hand-waved: each is the qualitative label of this style's measured
/// deck-gen tendency, and a test (`selection_info_matches_deckgen`) pins every
/// field to what [`generate_deck`] actually produces — if a rebalance shifts a
/// style's character, that test fails until the label is re-cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DeckStyle {
    pub id: u8,
    pub name: &'static str,
    /// The subjective voice — one line of how the deck plays, in flavor.
    pub blurb: &'static str,
    /// The objective shape — resonance lean, tempo/aggression, body-vs-spell
    /// mix — so the pick is informed, not blind. Derived from the deck-gen
    /// weighting (see [`SelectionInfo`]).
    pub selection: SelectionInfo,
}

/// Objective, at-a-glance facts about a [`DeckStyle`], so a player chooses on
/// substance and not just voice. Every field is a coarse, honest summary
/// of what this style's deck-gen weighting actually yields — the empirical
/// measurement is [`measured_selection_info`], and the authored labels are
/// asserted to match it. Three dimensions a player wants before committing:
/// what energy the deck speaks ([`resonance`](Self::resonance)), how it wants to
/// play the clock ([`tempo`](Self::tempo)) and how hard it hits
/// ([`aggression`](Self::aggression)), and whether it leans on bodies or the
/// spellbook ([`body_mix`](Self::body_mix)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SelectionInfo {
    /// The resonance(s) the deck speaks loudest — its colors on the wheel.
    pub resonance: ResonanceLean,
    /// How the deck wants to play the clock (a function of its mana curve).
    pub tempo: Tempo,
    /// How hard the deck hits — body punch + the Relentless chain.
    pub aggression: Aggression,
    /// Bodies (spirits/callers) versus the spellbook.
    pub body_mix: BodyMix,
}

/// A style's color on the resonance wheel: one or two resonances it leans into,
/// or `Balanced` for a deck (Drifter's Bindle) that plays the whole wheel evenly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ResonanceLean {
    /// No lean — an even spread across the wheel.
    Balanced,
    /// One dominant resonance.
    Mono(Resonance),
    /// Two co-dominant resonances (e.g. The Choir's Sorrow + Harmony).
    Dual(Resonance, Resonance),
}

/// How a style wants to play the clock — its mana-curve posture. Cheap, low
/// curves want to flood early (`Fast`); dearer, body-rich curves grind
/// (`Grindy`); the rest sit `Even`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Tempo {
    /// Low curve — presses the early rounds.
    Fast,
    /// A middling curve — neither rushes nor stalls.
    Even,
    /// A dearer, top-heavier curve — plays the long game.
    Grindy,
}

/// How hard a style hits: the punch of its bodies and how much it chains through
/// the Relentless keyword. An ordinal, `Defensive` (walls, low punch) →
/// `Aggressive` (high attack, Relentless chains).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Aggression {
    /// Built to absorb — low punch, leans on Warded/Steadfast walls.
    Defensive,
    /// A measured offense — hits, but does not race.
    Measured,
    /// Built to push — the hardest bodies and the Relentless chain.
    Aggressive,
}

/// Bodies (spirits/callers) versus spellbook cards in the generated deck. Every
/// Quick Play deck is spirit-led (the spirit-majority multiplier guarantees it),
/// so this is the degree, `Balanced` (still a clear majority, ~2:1) →
/// `SpiritHeavy` (bodies dominate, ~3:1).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum BodyMix {
    /// A spirit majority with a healthy spellbook (~2 bodies per spell).
    Balanced,
    /// Bodies dominate (~3 bodies per spell) — a board-first deck.
    SpiritHeavy,
}

/// One labelled dimension of a [`SelectionInfo`], display-ready: a short
/// `dimension` heading, the `value` word a player reads at a glance, and a plain
/// `detail` gloss that says what the value *means*. The picker renders these as
/// chips (web) and a line (CLI), and — crucially for a11y — the same `dimension`/
/// `value`/`detail` strings ARE the accessible text, so a screen reader hears the
/// substance, not just a colour. Authored in core so web and CLI never drift in
/// wording, and so the labels live beside the data they summarize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionFacet {
    /// The dimension this facet measures (e.g. `"Resonance"`).
    pub dimension: &'static str,
    /// The at-a-glance value word (e.g. `"Aggressive"`, `"Fast"`).
    pub value: &'static str,
    /// A short plain-language gloss of what the value means.
    pub detail: &'static str,
}

impl ResonanceLean {
    /// The at-a-glance label for the lean: the resonance word(s) it speaks, or
    /// `"Balanced"`. A `Dual` joins its two with `" + "` (e.g. `"Wonder + Fear"`).
    /// Allocation-free for the unit/mono cases; a `Dual` borrows a small static
    /// table so the joined string is still `&'static str`.
    pub fn label(self) -> &'static str {
        match self {
            ResonanceLean::Balanced => "Balanced",
            ResonanceLean::Mono(r) => r.label(),
            ResonanceLean::Dual(a, b) => dual_label(a, b),
        }
    }
}

impl Tempo {
    /// The at-a-glance tempo word.
    pub fn label(self) -> &'static str {
        match self {
            Tempo::Fast => "Fast",
            Tempo::Even => "Even",
            Tempo::Grindy => "Grindy",
        }
    }
    /// What the tempo means for how the deck plays the clock.
    pub fn detail(self) -> &'static str {
        match self {
            Tempo::Fast => "a low curve that presses the early rounds",
            Tempo::Even => "a middling curve — neither rushes nor stalls",
            Tempo::Grindy => "a dearer, top-heavier curve that plays the long game",
        }
    }
}

impl Aggression {
    /// The at-a-glance aggression word.
    pub fn label(self) -> &'static str {
        match self {
            Aggression::Defensive => "Defensive",
            Aggression::Measured => "Measured",
            Aggression::Aggressive => "Aggressive",
        }
    }
    /// What the aggression means for how hard the deck hits.
    pub fn detail(self) -> &'static str {
        match self {
            Aggression::Defensive => "built to absorb — low punch, leans on walls",
            Aggression::Measured => "a measured offense — hits, but does not race",
            Aggression::Aggressive => "built to push — the hardest bodies and the Relentless chain",
        }
    }
}

impl BodyMix {
    /// The at-a-glance body-mix word.
    pub fn label(self) -> &'static str {
        match self {
            BodyMix::Balanced => "Balanced bodies",
            BodyMix::SpiritHeavy => "Spirit-heavy",
        }
    }
    /// What the body-mix means for bodies versus the spellbook.
    pub fn detail(self) -> &'static str {
        match self {
            BodyMix::Balanced => "a spirit majority with a healthy spellbook (~2 bodies per spell)",
            BodyMix::SpiritHeavy => "bodies dominate (~3 bodies per spell) — a board-first deck",
        }
    }
}

impl SelectionInfo {
    /// The four labelled dimensions, in reading order (resonance, tempo,
    /// aggression, body-mix). The picker renders these directly — as chips on the
    /// web canvas-adjacent DOM and a one-line summary in the CLI — and they double
    /// as the accessible text, so the objective shape is conveyed as words, never
    /// only as a chart (AGENTS.md invariant 7).
    pub fn facets(self) -> [SelectionFacet; 4] {
        [
            SelectionFacet {
                dimension: "Resonance",
                value: self.resonance.label(),
                detail: "the energy the deck speaks loudest",
            },
            SelectionFacet {
                dimension: "Tempo",
                value: self.tempo.label(),
                detail: self.tempo.detail(),
            },
            SelectionFacet {
                dimension: "Aggression",
                value: self.aggression.label(),
                detail: self.aggression.detail(),
            },
            SelectionFacet {
                dimension: "Body mix",
                value: self.body_mix.label(),
                detail: self.body_mix.detail(),
            },
        ]
    }

    /// A single plain-text line summarizing the shape — `"Fury · Aggressive ·
    /// even tempo · balanced bodies"` — for the CLI and as the picker card's
    /// accessible name on the web. Middot-separated like the rest of the UI's
    /// at-a-glance lines.
    pub fn summary(self) -> String {
        format!(
            "{} · {} · {} tempo · {}",
            self.resonance.label(),
            self.aggression.label(),
            self.tempo.label().to_lowercase(),
            self.body_mix.label().to_lowercase(),
        )
    }
}

/// The two-resonance `Dual` label as a `&'static str`. Each unordered pair maps to
/// a single authored `"A + B"` constant (the table is the 15 off-`Neutral` pairs),
/// so the joined label needs no allocation and the wheel order is fixed. An
/// unexpected pair (a repeat, or `Neutral`) falls back to the dominant resonance's
/// own label rather than panicking — the well-formed-ness test guards the inputs.
fn dual_label(a: Resonance, b: Resonance) -> &'static str {
    use Resonance::*;
    // Normalize to a canonical (lo, hi) by enum discriminant so order never matters.
    let (lo, hi) = if (a as u8) <= (b as u8) {
        (a, b)
    } else {
        (b, a)
    };
    match (lo, hi) {
        (Wonder, Fear) => "Wonder + Fear",
        (Wonder, Sorrow) => "Wonder + Sorrow",
        (Wonder, Harmony) => "Wonder + Harmony",
        (Wonder, Fury) => "Wonder + Fury",
        (Wonder, Resolve) => "Wonder + Resolve",
        (Fear, Sorrow) => "Fear + Sorrow",
        (Fear, Harmony) => "Fear + Harmony",
        (Fear, Fury) => "Fear + Fury",
        (Fear, Resolve) => "Fear + Resolve",
        (Sorrow, Harmony) => "Sorrow + Harmony",
        (Sorrow, Fury) => "Sorrow + Fury",
        (Sorrow, Resolve) => "Sorrow + Resolve",
        (Harmony, Fury) => "Harmony + Fury",
        (Harmony, Resolve) => "Harmony + Resolve",
        (Fury, Resolve) => "Fury + Resolve",
        _ => a.label(),
    }
}

/// Build [`DeckStyle`] with its objective [`SelectionInfo`] inline (keeps the
/// `STYLES` table readable).
const fn style(
    id: u8,
    name: &'static str,
    blurb: &'static str,
    resonance: ResonanceLean,
    tempo: Tempo,
    aggression: Aggression,
    body_mix: BodyMix,
) -> DeckStyle {
    DeckStyle {
        id,
        name,
        blurb,
        selection: SelectionInfo {
            resonance,
            tempo,
            aggression,
            body_mix,
        },
    }
}

pub const STYLES: [DeckStyle; 5] = [
    style(
        0,
        "Embertide",
        "All teeth. Arrive loudly, chain hard, apologize to no one.",
        ResonanceLean::Mono(Resonance::Fury),
        Tempo::Even,
        Aggression::Aggressive,
        BodyMix::Balanced,
    ),
    style(
        1,
        "The Long Watch",
        "Hold ground, blunt every arrival, and let Nightfall do the counting.",
        ResonanceLean::Mono(Resonance::Resolve),
        Tempo::Grindy,
        Aggression::Defensive,
        BodyMix::SpiritHeavy,
    ),
    style(
        2,
        "Mistwalk",
        "Arrive sideways. Pierce wards, step where the zones aren't.",
        ResonanceLean::Dual(Resonance::Wonder, Resonance::Fear),
        Tempo::Fast,
        Aggression::Measured,
        BodyMix::SpiritHeavy,
    ),
    style(
        3,
        "The Choir",
        // The balanced Choir carries the *lowest* Warded/Steadfast of any archetype
        // (~0.7 each) — it is the patient, passive deck, not a wall. The line keeps
        // the true note (gathered Sorrow + Harmony that hold together) without
        // claiming a durability it doesn't earn.
        "Stand together. Sorrow and Harmony, gathered voices that hold the line.",
        ResonanceLean::Dual(Resonance::Sorrow, Resonance::Harmony),
        Tempo::Fast,
        Aggression::Measured,
        BodyMix::Balanced,
    ),
    style(
        4,
        "Drifter's Bindle",
        "A bit of everything, honestly shuffled. The Memory provides.",
        ResonanceLean::Balanced,
        Tempo::Fast,
        Aggression::Measured,
        BodyMix::Balanced,
    ),
];

/// Seeded offer of three distinct styles for the picker UI.
pub fn offer(seed: u64) -> [DeckStyle; 3] {
    let mut rng = Rng::from_seed(seed ^ 0x0FFE_12B1);
    let mut ids: Vec<usize> = (0..STYLES.len()).collect();
    rng.shuffle(&mut ids);
    [STYLES[ids[0]], STYLES[ids[1]], STYLES[ids[2]]]
}

/// Style-specific weight for one card. Zero excludes (Drifter excludes nothing).
fn weight(style: u8, c: &CardDef) -> u64 {
    use Resonance::*;
    match style {
        // Embertide: Fury forward, Relentless and Lance prized.
        0 => match c.resonance {
            Fury => 4 + if c.relentless { 2 } else { 0 },
            Wonder | Fear => 1,
            _ => 1,
        },
        // The Long Watch: Defense, Warded, Steadfast, big HP.
        1 => {
            let mut w = 1;
            if c.resonance == Resolve {
                w += 3
            }
            if c.warded || c.steadfast {
                w += 2
            }
            if c.hp >= 50 {
                w += 2
            }
            w
        }
        // Mistwalk: Mobile, Arcane, slanted approaches.
        2 => {
            let mut w = 1;
            if c.mobile {
                w += 3
            }
            if c.arcane {
                w += 3
            }
            if matches!(c.resonance, Fear | Wonder) {
                w += 2
            }
            w
        }
        // The Choir: Sorrow + Harmony bodies that hold hands.
        3 => match c.resonance {
            Sorrow | Harmony => 4 + if c.warded { 1 } else { 0 },
            _ => 1,
        },
        // Drifter's Bindle: uniform chaos.
        _ => 1,
    }
}

/// Spirits are the backbone of every Quick Play deck. The catalog has
/// 120 spellbook cards to 105 spirits, so uniform style-weighting drowns the
/// bodies; this multiplier restores a healthy spirit majority (~55–65%).
///
/// Evolution **forms** are **excluded from the primary weighted draw**
/// (`Evolution => 0`) and seeded only as base↔form PAIRS by
/// [`ensure_evolution_floor`]. Forms are dear — cost 3–6 — so letting them flow
/// into the weighted draw would raise the mana curve and tip the first-mover
/// balance the 1v1 anchor guards; keeping them out of it holds the curve while the
/// floor guarantees evolution stays live play. A form never enters except alongside
/// a base that lands it, so the deck can never hold an orphan form.
fn kind_mult(c: &CardDef) -> u64 {
    use crate::types::CardKind::*;
    match c.kind {
        Spirit | Caller => 5,
        Ritual | Bond => 2,
        Landmark | Fabrication => 1,
        _ => 0, // Evolution forms enter via the pairing floor, not the primary draw.
    }
}

/// Is this card a **base** with an evolution line (it may become a Primal/Fabled form)?
/// A base IS in the primary draw; its forms are seeded as pairs alongside it. An
/// evolution-rich deck is one that holds base↔form PAIRS (the form landable on the base),
/// which [`ensure_evolution_floor`] guarantees so the play stays live.
fn is_evolution_base(c: &CardDef) -> bool {
    !c.evolves_to.is_empty()
}

/// Evolution-density floor: every generated Lorekeeper deck guarantees at least this many
/// **landable base↔form pairs** — a base in the deck AND one of its forms in the deck — so the
/// evolution phase is always live play (a bot that can never evolve isn't a representative opponent).
/// Forms are deck cards you must HOLD to evolve, so a live evolution costs two deck slots: the base
/// you land on and the form you play. The floor is faction-agnostic — it applies to ANY faction whose
/// pool has an evolving base, which now includes the **Solace** (its 8 Primal Deepenings), seeded the
/// same way (base↔Deepening pairs). A faction with no evolving base is a clean no-op.
///
/// **The pairing pass is the ONLY evolution-density lever, deliberately, and forms never flow through
/// the primary weighted draw** (`kind_mult` zeroes them). Folding an "evolution-affinity" multiplier
/// into the weighted draw to lift base density would break the 1v1 mirror anchor
/// (`fleet_tripwires::one_v_one_is_a_fair_anchor`): the evolving cards average a **higher cost** than
/// the rest of the pool, so over-weighting them raises the deck's mana curve, and a higher-curve deck
/// structurally disadvantages the FIRST mover (it must commit dear threats earlier, into the second
/// player's information). Density and first-mover fairness trade off through the curve, so the floor
/// is held just high enough to make evolution live play (2 pairs ≈ 4 cards ≈ 20% of a 20-card deck)
/// and no higher; the curve-aware swap-out (drop the least-essential non-evolution card, never below
/// 8 cheap cards) keeps the back-fill from tipping the curve. The eval — not a bigger deck buff —
/// carries the PvE difficulty.
pub(crate) const EVOLUTION_FLOOR: usize = 2;

/// Solace alignment-style weight: a character's disposition shapes its draw —
/// cruel foes field the ill intent; erasing ones, the patient Unwritten and the Unwriting events
/// that wipe the page. No style zeroes the whole pool, so every style still fills a legal 20-card
/// singleton from the 92-card set. Style indices are the dispositions in [`SOLACE_CHARACTERS`].
fn solace_weight(style: u8, c: &CardDef) -> u64 {
    use crate::types::CardKind::*;
    let (ill, unwritten, event) = (
        c.kind == IllIntent,
        c.kind == Unwritten,
        c.kind == Unwriting,
    );
    match style {
        // Cruelty — the ill intent forward.
        0 => {
            if ill {
                6
            } else if event {
                2
            } else {
                1
            }
        }
        // Erasure — patient Unwritten and the events that wipe the page.
        1 => {
            if unwritten || event {
                4
            } else {
                1
            }
        }
        // Relentless — whatever hits hardest, ill or not.
        2 => {
            let mut w = 1;
            if c.relentless {
                w += 4;
            }
            if c.attack >= 50 {
                w += 3;
            }
            if ill {
                w += 1;
            }
            w
        }
        // The Long Forgetting — walls that outlast you.
        3 => {
            let mut w = 1;
            if c.steadfast || c.warded {
                w += 3;
            }
            if c.hp >= 50 {
                w += 2;
            }
            if event {
                w += 1;
            }
            w
        }
        // Sorrow — the gentle, sadder Unwritten; the ill intent stays rare.
        4 => {
            if unwritten {
                5
            } else if ill {
                1
            } else {
                2
            }
        }
        // Balanced — honest chaos, the whole pool.
        _ => 1,
    }
}

fn draw_weighted(
    rng: &mut Rng,
    catalog: &[CardDef],
    faction: crate::types::Faction,
    style: u8,
    copies: &[u8],
    cheap_only: bool,
) -> usize {
    use crate::types::Faction;
    let elig: Vec<(usize, u64)> = catalog
        .iter()
        .enumerate()
        .filter(|(i, c)| {
            c.kind.deck_playable_for(faction)
                && copies[*i] < 1 // singleton — at most one of each card
                && (!cheap_only || c.cost <= 2)
        })
        .map(|(i, c)| {
            // Lorekeeper draws by archetype style × the spirit-majority multiplier (`kind_mult`,
            // which zeroes Evolution); the Solace draws by its character's alignment style. In BOTH,
            // **Evolution forms are excluded from the primary draw** — they enter only as base↔form
            // PAIRS via [`ensure_evolution_floor`]. For the Solace this is a flat `Evolution ⇒ 0`
            // gate (its non-form kinds — Unwritten/IllIntent/Unwriting — all weight equally by
            // disposition, so `solace_weight` is NOT pre-multiplied by `kind_mult`, which would
            // wrongly zero the Unwriting events too). Now that the Solace has Primal Deepenings,
            // this is what keeps a Lorekeeper form — or an unpaired Solace form — out of a Solace
            // deck; over-weighting the dearer evolving cards would also raise the curve and tip
            // first-mover balance (see the floor doc).
            let w = match faction {
                Faction::Lorekeeper => weight(style, c) * kind_mult(c),
                Faction::Solace if c.kind == crate::types::CardKind::Evolution => 0,
                Faction::Solace => solace_weight(style, c),
            };
            (i, w)
        })
        .collect();
    let total: u64 = elig.iter().map(|(_, w)| w).sum();
    let mut roll = rng.below(total);
    for (i, w) in elig {
        if roll < w {
            return i;
        }
        roll -= w;
    }
    unreachable!("weights are positive and total covers the roll")
}

/// Build a legal **singleton** deck for a faction: DECK_SIZE distinct cards with an
/// opening curve (≥8 of Cost ≤ 2 so the first rounds play), at least `EVOLUTION_FLOOR`
/// landable base↔form pairs, and **no orphan evolutions**. Pure in
/// (faction, style, seed, catalog); the server can re-derive it exactly.
pub fn generate_deck_for(
    faction: crate::types::Faction,
    style: u8,
    seed: u64,
    catalog: &[CardDef],
) -> Vec<CardId> {
    let mut rng = Rng::from_seed(seed ^ ((style as u64 + 1) * 0x9E37_79B9));
    let mut copies = vec![0u8; catalog.len()];
    let mut deck = Vec::with_capacity(DECK_SIZE);
    while deck.len() < DECK_SIZE {
        let cheap_phase = deck.len() < 8; // the opening curve, guaranteed first
        let i = draw_weighted(&mut rng, catalog, faction, style, &copies, cheap_phase);
        copies[i] += 1;
        deck.push(catalog[i].id);
    }
    ensure_evolution_floor(faction, style, &mut rng, catalog, &mut copies, &mut deck);
    debug_assert!(crate::cards::validate_deck_for(&deck, catalog, faction).is_ok());
    deck
}

/// Guarantee [`EVOLUTION_FLOOR`] landable **base↔form pairs** in a built deck so the evolution phase
/// is always live play. A pair is a base in the deck PLUS one of its forms in the deck (the
/// form is then a hand card you can play onto the standing base). The pass seeds whole pairs,
/// preferring the cheapest completion — if a base already drawn into the deck has no form yet, it
/// adds just that base's cheapest form (a 1-card, least-curve-disruptive completion); otherwise it
/// draws a fresh evolving base (style-weighted) and adds the base plus its cheapest form. Each
/// addition swaps out the least-essential **non-evolution** card (an off-style card first, then a
/// spellbook card before a body, a dearer card before a cheap one — so the opening curve and the
/// archetype's character both survive, never below 8 cheap cards). A form never enters except
/// alongside a base that lands it, so the deck can never hold an orphan form — the SAME shared
/// pairing for the Lorekeeper AND the Solace (the Solace's Primal Deepenings pair with their
/// Unwritten/IllIntent bases here too). A no-op only for a faction with no evolving base, and for
/// already-paired decks. Pure: it draws from the same seeded `rng` as the main loop, so the deck
/// stays re-derivable.
fn ensure_evolution_floor(
    faction: crate::types::Faction,
    style: u8,
    rng: &mut Rng,
    catalog: &[CardDef],
    copies: &mut [u8],
    deck: &mut Vec<CardId>,
) {
    let slot = |id: CardId| catalog.iter().position(|c| c.id == id);
    let is_evo_card = |id: CardId| {
        slot(id)
            .map(|i| {
                catalog[i].kind == crate::types::CardKind::Evolution
                    || is_evolution_base(&catalog[i])
            })
            .unwrap_or(false)
    };

    // Nothing to do if the faction's pool has no evolving base (a faction without lines).
    let pool_has_evo = catalog
        .iter()
        .any(|c| c.kind.deck_playable_for(faction) && is_evolution_base(c));
    if !pool_has_evo {
        return;
    }

    // Count landable pairs: bases in the deck for which a form they reach is also in the deck.
    let count_pairs = |deck: &[CardId]| -> usize {
        let names: std::collections::BTreeSet<&str> = deck
            .iter()
            .filter_map(|&id| slot(id).map(|i| catalog[i].name.as_str()))
            .collect();
        deck.iter()
            .filter_map(|&id| slot(id).map(|i| &catalog[i]))
            .filter(|b| {
                !b.evolves_to.is_empty() && b.evolves_to.iter().any(|f| names.contains(f.as_str()))
            })
            .count()
    };

    // The cheapest form a base reaches that is NOT yet in the deck (catalog slot).
    let cheapest_unused_form = |base: &CardDef, copies: &[u8]| -> Option<usize> {
        base.evolves_to
            .iter()
            .filter_map(|fname| catalog.iter().position(|c| &c.name == fname))
            .filter(|&fi| copies[fi] < 1)
            .min_by_key(|&fi| catalog[fi].cost)
    };

    // Add one card to the deck by swapping out the least-essential non-evolution victim, keeping
    // the opening curve (never below 8 cheap cards). Returns false if no swap is possible.
    let add_card = |add: usize, copies: &mut [u8], deck: &mut Vec<CardId>| -> bool {
        let cheap_count = deck
            .iter()
            .filter(|&&id| slot(id).map(|i| catalog[i].cost <= 2).unwrap_or(false))
            .count();
        let adding_cheap = catalog[add].cost <= 2;
        let victim = deck
            .iter()
            .copied()
            .enumerate()
            .filter(|&(_, id)| !is_evo_card(id))
            .max_by_key(|&(_, id)| {
                let c = &catalog[slot(id).unwrap()];
                let spell = !matches!(
                    c.kind,
                    crate::types::CardKind::Spirit | crate::types::CardKind::Caller
                );
                // Drop a dear card before a cheap one, but never tip below 8 cheap cards
                // (unless we're adding a cheap card back, which keeps the curve whole).
                let curve_safe = c.cost > 2 || cheap_count > 8 || adding_cheap;
                // Prefer evicting an OFF-style card so the pairing pass preserves the
                // archetype's character (Embertide keeps its Fury, The Choir its Sorrow):
                // higher reverse-weight = lower style affinity = more evictable.
                let off_style = 64u64.saturating_sub(weight(style, c));
                (curve_safe, off_style, spell, c.cost)
            });
        let Some((vidx, vid)) = victim else {
            return false;
        };
        if let Some(vi) = slot(vid) {
            copies[vi] = copies[vi].saturating_sub(1);
        }
        copies[add] += 1;
        deck[vidx] = catalog[add].id;
        true
    };

    let mut guard = 0;
    while count_pairs(deck) < EVOLUTION_FLOOR && guard < 64 {
        guard += 1;
        let names: std::collections::BTreeSet<&str> = deck
            .iter()
            .filter_map(|&id| slot(id).map(|i| catalog[i].name.as_str()))
            .collect();
        // (1) Cheapest completion: a base already in the deck that still lacks a form.
        let lone_base = deck
            .iter()
            .filter_map(|&id| slot(id).map(|i| &catalog[i]))
            .filter(|b| {
                !b.evolves_to.is_empty() && !b.evolves_to.iter().any(|f| names.contains(f.as_str()))
            })
            .find_map(|b| cheapest_unused_form(b, copies));
        if let Some(form_slot) = lone_base {
            if add_card(form_slot, copies, deck) {
                continue;
            }
            return; // no room to complete a pair
        }
        // (2) Otherwise seed a fresh pair: a new evolving base + its cheapest form.
        let Some(base_slot) = draw_evolving_base(rng, catalog, faction, style, copies) else {
            return; // pool exhausted of un-drawn evolving bases
        };
        let form_slot = cheapest_unused_form(&catalog[base_slot], copies);
        if !add_card(base_slot, copies, deck) {
            return;
        }
        // Pair the base with a form. Every base reaches ≥1 form, but guard defensively.
        if let Some(form_slot) = form_slot
            && !add_card(form_slot, copies, deck)
        {
            return;
        }
    }
}

/// Draw one un-drawn **evolving base**, style-weighted, or `None` if the pool is exhausted. Mirrors
/// [`draw_weighted`]'s seeded selection but restricted to bases that carry an evolution line (no
/// cheap-only phase — the floor runs after the curve is set). Forms are added explicitly alongside
/// their base, never drawn here.
fn draw_evolving_base(
    rng: &mut Rng,
    catalog: &[CardDef],
    faction: crate::types::Faction,
    style: u8,
    copies: &[u8],
) -> Option<usize> {
    let elig: Vec<(usize, u64)> = catalog
        .iter()
        .enumerate()
        .filter(|(i, c)| {
            c.kind.deck_playable_for(faction) && is_evolution_base(c) && copies[*i] < 1
        })
        .map(|(i, c)| (i, weight(style, c).max(1) * kind_mult(c).max(1)))
        .collect();
    let total: u64 = elig.iter().map(|(_, w)| w).sum();
    if total == 0 {
        return None;
    }
    let mut roll = rng.below(total);
    for (i, w) in elig {
        if roll < w {
            return Some(i);
        }
        roll -= w;
    }
    None
}

/// Lorekeeper convenience over [`generate_deck_for`] — the Quick Play styles are
/// Lorekeeper archetypes. Keeps the existing signature for current callers.
pub fn generate_deck(style: u8, seed: u64, catalog: &[CardDef]) -> Vec<CardId> {
    generate_deck_for(crate::types::Faction::Lorekeeper, style, seed, catalog)
}

/// A Solace PvE character: a named antagonist with a **disposition** (the alignment
/// style fed to `solace_weight`) and lore. The bot pilots its deck at the player's chosen
/// difficulty tier — difficulty is the bot's *skill* (softmax temperature + lookahead), orthogonal
/// to the character, so a gentle Sorrow foe at Expert is sound but kind, a Cruelty foe at Easy
/// vicious but blundering. Faced, never collected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolaceCharacter {
    pub name: &'static str,
    /// Disposition → `solace_weight`: 0 Cruelty, 1 Erasure, 2 Relentless, 3 The Long
    /// Forgetting, 4 Sorrow.
    pub disposition: u8,
    pub lore: &'static str,
}

impl SolaceCharacter {
    /// First-player initiative: aggressive dispositions lean toward opening — an edge fed as
    /// the bias to [`decide_opener`], never a guarantee. Cruelty and Relentless press for the first
    /// strike; the patient dispositions (Erasure, the Long Forgetting, Sorrow) are content to answer.
    pub fn initiative(&self) -> i32 {
        match self.disposition {
            0 | 2 => 15,
            _ => 0,
        }
    }
}

const fn ch(name: &'static str, disposition: u8, lore: &'static str) -> SolaceCharacter {
    SolaceCharacter {
        name,
        disposition,
        lore,
    }
}

/// The ~20 Solace characters — four per disposition, distinct in voice and (via the salted seed)
/// in the cards they field. The player faces these; they are never collected.
pub const SOLACE_CHARACTERS: [SolaceCharacter; 20] = [
    // Cruelty — converts whose mercy carries an edge; the keepers' "kindness" left them a scar.
    ch(
        "Aldous Vane",
        0,
        "The keepers drew his mother's fading out a whole year; he means to spare everyone that kindness.",
    ),
    ch(
        "Sister Halloran",
        0,
        "She offers release the way a surgeon does — quickly, and she does not ask twice.",
    ),
    ch(
        "Corin Ashe",
        0,
        "He burned his own Library the night he left, so that nothing could ever tempt him back.",
    ),
    ch(
        "Edmund",
        0,
        "The Solace's operational head: he believes the mercy, and that someone must be hard enough to make it spread.",
    ),
    // Erasure — those who hold that a page must be cleared all the way before it can be written again.
    ch(
        "Mara Quint",
        1,
        "She thinks a half-erased page the cruelest page of all, and finishes what the fading starts.",
    ),
    ch(
        "Lucan Reeve",
        1,
        "A tidy man, who cannot abide a thing left lingering.",
    ),
    ch(
        "Wenna Marsh",
        1,
        "She clears the ground for whatever comes next, and grieves nothing she pulls up.",
    ),
    ch(
        "Silas Howe",
        1,
        "He learned that a clean forgetting leaves no scar, and made it his whole life's work.",
    ),
    // Relentless — the tireless; they will hold the offer out for as long as it takes.
    ch(
        "Ona Verrin",
        2,
        "She will offer you peace a thousand times, with the patience of water on stone.",
    ),
    ch(
        "Brother Tace",
        2,
        "He never raises his voice, and he never stops.",
    ),
    ch(
        "Hesper Lund",
        2,
        "Rest is the one thing she thinks the keepers have earned and refuse, and she keeps holding it out.",
    ),
    ch(
        "Galen Roe",
        2,
        "He comes back, and comes back, gentle as a returning tide.",
    ),
    // The Long Forgetting — innocents raised in the Solace, who never kept a thing; slow and certain.
    ch(
        "Wren",
        3,
        "Raised in the Solace, she has never kept a single thing, and cannot imagine the ache of it.",
    ),
    ch(
        "Elin Mooring",
        3,
        "She thinks forgetting is only what love looks like once it is finished.",
    ),
    ch(
        "Tomas Wend",
        3,
        "Slow, kind, and certain, he has all the time in the world, and says that you do too.",
    ),
    ch(
        "Mother Senna",
        3,
        "She raised a generation who never grieved, and calls it the kindest thing she has done.",
    ),
    // Sorrow — those a particular grief brought to the creed; the gentlest, and the saddest.
    ch(
        "Cael Mourne",
        4,
        "He sat with his brother through the long forgetting, and came to envy him the peace of it.",
    ),
    ch(
        "Katherine",
        4,
        "A keeper's Dreamer who went too close to the fading and came back converted; the gentlest temptation, and the one whose opposition aches.",
    ),
    ch(
        "Anneke Lir",
        4,
        "She offers others the mercy she could not give herself in time.",
    ),
    ch(
        "Old Damaris",
        4,
        "She has buried everyone, and calls the forgetting the only friend that stayed.",
    ),
];

/// The deck a Solace character fields this match: its disposition shapes the draw, salted by the
/// roster `index` (so same-disposition characters differ) and varied by the match `seed`. Pure +
/// re-derivable like every Quick Play deck.
pub fn solace_character_deck(index: usize, seed: u64, catalog: &[CardDef]) -> Vec<CardId> {
    let ch = &SOLACE_CHARACTERS[index % SOLACE_CHARACTERS.len()];
    generate_deck_for(
        crate::types::Faction::Solace,
        ch.disposition,
        seed ^ ((index as u64 + 1) * 0xA24B_AED4),
        catalog,
    )
}

/// A Lorekeeper PvE/Skirmish character: a named opponent/ally with a **style** (the Quick Play
/// archetype fed to the deck generator) and lore — the Lorekeeper parity of [`SolaceCharacter`].
/// Faced as a Skirmish opponent, an ally, or a mirror.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LorekeeperCharacter {
    pub name: &'static str,
    /// Style index → the Quick Play archetypes [`STYLES`]: 0 Embertide, 1 The Long Watch,
    /// 2 Mistwalk, 3 The Choir, 4 Drifter's Bindle.
    pub style: u8,
    pub lore: &'static str,
}

impl LorekeeperCharacter {
    /// First-player initiative: the aggressive archetype (Embertide) presses to open; the
    /// rest are even. A positive "drive to open" — the caller signs it by the character's seat (a
    /// seat-B bot biases [`decide_opener`] toward B).
    pub fn initiative(&self) -> i32 {
        match self.style {
            0 => 15,
            _ => 0,
        }
    }
}

const fn lk(name: &'static str, style: u8, lore: &'static str) -> LorekeeperCharacter {
    LorekeeperCharacter { name, style, lore }
}

/// The ~20 Lorekeeper characters — four per Quick Play style, distinct in voice and (via the salted
/// seed) in the cards they field. The player faces them as Skirmish opponents, allies, and mirrors.
pub const LOREKEEPER_CHARACTERS: [LorekeeperCharacter; 20] = [
    // Embertide — keepers of the fierce memories, the ones worth staying angry for.
    lk(
        "Remembrancer Edda",
        0,
        "She remembers the loud way: every name said like a struck match.",
    ),
    lk(
        "Brand Carrow",
        0,
        "He keeps the angry memories, the ones worth staying angry for.",
    ),
    lk(
        "Ysabel Crane",
        0,
        "A Remembrancer who holds that a grief should burn, not gutter.",
    ),
    lk(
        "Tobias Hearth",
        0,
        "He tends the memories that still throw heat, and will not bank them.",
    ),
    // The Long Watch — the steadfast: Archivists and wardens who outlast the fading.
    lk(
        "Archivist Pell",
        1,
        "She files even her own griefs under the correct heading, and never closes the ledger early.",
    ),
    lk(
        "Warden Ames",
        1,
        "He has stood the night watch over the Archive longer than the wall behind him.",
    ),
    lk(
        "Sister Vance",
        1,
        "Patient as granite, she outlasts the fading by refusing to look away.",
    ),
    lk(
        "Cuthbert Stile",
        1,
        "He gave the dead his word that he would not let them go, and he has not.",
    ),
    // Mistwalk — Dreamers who go into the Memories and bring back what resists charting.
    lk(
        "Dreamer Juno",
        2,
        "She stood at a friend's turning, felt the same pull toward the Solace, and did not go.",
    ),
    lk(
        "Aldren Fay",
        2,
        "A Dreamer you only ever catch at the edge of a Memory, half-returned.",
    ),
    lk(
        "Thomas Reed",
        2,
        "He walks the dream-paths the maps forgot, and brings back the half-glimpsed.",
    ),
    lk(
        "Moren Ivy",
        2,
        "A Dreamer who comes back a little changed each time, and goes in again.",
    ),
    // The Choir — keepers of the shared memories, the ones it takes many voices to hold.
    lk(
        "Cantor Liss",
        3,
        "She keeps the memories that take many voices to hold, and gathers the voices.",
    ),
    lk(
        "Brother Anselm",
        3,
        "A grief is lighter shared, he says, and he makes a hearth of the keeping.",
    ),
    lk(
        "Even Calloway",
        3,
        "He keeps the quiet harmonies: the evening songs at the end of a long day.",
    ),
    lk(
        "Sister Rue",
        3,
        "She gathers the lonely memories into the choir, so that none keeps watch alone.",
    ),
    // Drifter's Bindle — the wandering keepers, who keep whatever the road turns up.
    lk(
        "Ferrier Bask",
        4,
        "She keeps the crossing, and asks only that you say the names aloud.",
    ),
    lk(
        "Magpie Wynn",
        4,
        "A keeper of odds and ends: a little of everything that ever caught her eye.",
    ),
    lk(
        "Tinker Sayer",
        4,
        "She makes do, and makes do well, with whatever the road turns up.",
    ),
    lk(
        "Wandering Cole",
        4,
        "He keeps what others drop along the way, and the road always turns up something.",
    ),
];

/// The deck a Lorekeeper character fields this match: its style shapes the draw, salted by the roster
/// `index` (so same-style characters differ) and varied by the match `seed`. Pure + re-derivable.
pub fn lorekeeper_character_deck(index: usize, seed: u64, catalog: &[CardDef]) -> Vec<CardId> {
    let ch = &LOREKEEPER_CHARACTERS[index % LOREKEEPER_CHARACTERS.len()];
    generate_deck(ch.style, seed ^ ((index as u64 + 1) * 0xB7E1_5163), catalog)
}

/// Who opens the match: a **seeded** coin-flip, optionally weighted by a
/// character's initiative. `bias` shifts the toss toward seat A (positive) or B (negative); 0 is a
/// fair 50/50, and the magnitude is an edge, never a guarantee (clamped to a 5–95% window). Pure in
/// (seed, bias) — the opener is deterministic and replay-verified like the rest of the genesis.
pub fn decide_opener(seed: u64, bias: i32) -> crate::types::Seat {
    let mut r = Rng::from_seed(seed ^ 0x0FED_5EED);
    let roll = r.below(100) as i32;
    let threshold = (50 + bias).clamp(5, 95);
    if roll < threshold {
        crate::types::Seat::A
    } else {
        crate::types::Seat::B
    }
}

/// Who opens the match in 2v2: a **seeded** 4-way pick of which seat opens — any of A1, B1,
/// A2, B2 — each weighted by `weights` (a base, plus a bot character's initiative on its own seat).
/// The A1→B1→A2→B2 cycle then rotates to begin at the opener. Pure in (seed, weights), deterministic
/// and replay-verified like the rest of the genesis.
pub fn decide_opener_2v2(seed: u64, weights: [u32; 4]) -> crate::types::SeatSlot {
    use crate::types::SeatSlot::{A1, A2, B1, B2};
    let slots = [A1, B1, A2, B2];
    let total: u64 = weights.iter().map(|&w| w as u64).sum::<u64>().max(1);
    let mut roll = Rng::from_seed(seed ^ 0x0FED_5EE2).below(total);
    for (i, &w) in weights.iter().enumerate() {
        if roll < w as u64 {
            return slots[i];
        }
        roll -= w as u64;
    }
    A1
}

/// A human-readable preview of the deck a style would derive, so
/// a player sees what they're choosing BEFORE they commit. Pure projection of
/// `generate_deck` — same seed + style yields the same preview. The UI renders
/// `cards` (name + cost + the A/D/H line) and `curve` (count by cost) so the
/// pick is informed, not blind.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeckPreview {
    pub style: u8,
    pub style_name: String,
    /// One row per card in draw order: (name, cost, attack, defense, hp).
    pub cards: Vec<PreviewCard>,
    /// Count of cards at each cost 0..=7 (index = cost), for a quick curve bar.
    pub curve: [u8; 8],
    /// How many spirits vs. spellbook cards (the deck's shape at a glance).
    pub spirit_count: u8,
    pub spell_count: u8,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PreviewCard {
    pub name: String,
    pub cost: u8,
    pub attack: i16,
    pub defense: i16,
    pub hp: i16,
    pub kind: String,
}

/// Build the [`DeckPreview`] for `style` at `seed` — what `generate_deck`
/// would produce, summarized for display. The picker shows this for each of
/// the three offered styles before the player accepts one.
pub fn preview(style: u8, seed: u64, catalog: &[CardDef]) -> DeckPreview {
    let ids = generate_deck(style, seed, catalog);
    let style_name = STYLES
        .iter()
        .find(|s| s.id == style)
        .map(|s| s.name.to_string())
        .unwrap_or_default();
    let mut curve = [0u8; 8];
    let (mut spirit_count, mut spell_count) = (0u8, 0u8);
    let mut cards = Vec::with_capacity(ids.len());
    for id in &ids {
        let c = &catalog[id.0 as usize];
        if (c.cost as usize) < curve.len() {
            curve[c.cost as usize] += 1;
        }
        match c.kind {
            crate::types::CardKind::Spirit | crate::types::CardKind::Caller => spirit_count += 1,
            _ => spell_count += 1,
        }
        cards.push(PreviewCard {
            name: c.name.clone(),
            cost: c.cost,
            attack: c.attack,
            defense: c.defense,
            hp: c.hp,
            kind: format!("{:?}", c.kind),
        });
    }
    DeckPreview {
        style,
        style_name,
        cards,
        curve,
        spirit_count,
        spell_count,
    }
}

/// **Empirically derive** the objective [`SelectionInfo`] for a Lorekeeper
/// `style` by sampling the decks [`generate_deck`] actually produces over
/// `seeds` seeds. This is the ground truth the authored
/// [`DeckStyle::selection`] labels are pinned to — `selection_info_matches_deckgen`
/// asserts the two agree, so a label can never drift from the deck-gen weighting
/// it summarizes. The dimensions are read straight off the generated cards:
///
/// - **resonance**: the share of *bodies* (spirits/callers/forms) by resonance.
///   A single resonance over the `MONO_RESONANCE_SHARE` bar (with a clear lead) is
///   a `Mono` lean; two each over `DUAL_RESONANCE_SHARE` are a `Dual`; a flat
///   spread (no resonance leading the field) is `Balanced`.
/// - **tempo**: the deck's average mana cost — `Fast` below `FAST_COST`, `Grindy`
///   above `GRINDY_COST`, else `Even`.
/// - **aggression**: a score from average body Attack and the Relentless share,
///   lowered by wall (Warded/Steadfast) density — `Aggressive` above `AGGRO_HI`,
///   `Defensive` below `AGGRO_LO`, else `Measured`.
/// - **body_mix**: the spirit/caller fraction of the deck — `SpiritHeavy` at or
///   above `SPIRIT_HEAVY_FRACTION`, else `Balanced`.
///
/// (The threshold consts above are documented at their definitions below.) Pure in
/// (style, seeds, catalog); a larger `seeds` tightens the estimate. The thresholds
/// are deliberately coarse with wide margins so the qualitative label is stable
/// under the sampling noise.
pub fn measured_selection_info(style: u8, seeds: u64, catalog: &[CardDef]) -> SelectionInfo {
    use crate::types::CardKind::{Caller, Evolution, Spirit};
    let seeds = seeds.max(1);
    let mut res_bodies: std::collections::BTreeMap<Resonance, f64> = Default::default();
    let (mut bodies, mut cards, mut cost, mut body_atk, mut relentless, mut spirits, mut walls) =
        (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for seed in 0..seeds {
        for id in generate_deck(style, seed, catalog) {
            let c = &catalog[id.0 as usize];
            cards += 1.0;
            cost += c.cost as f64;
            relentless += c.relentless as u8 as f64;
            walls += (c.warded as u8 + c.steadfast as u8) as f64;
            let is_body = matches!(c.kind, Spirit | Caller | Evolution);
            if matches!(c.kind, Spirit | Caller) {
                spirits += 1.0;
            }
            if is_body {
                bodies += 1.0;
                body_atk += c.attack as f64;
                *res_bodies.entry(c.resonance).or_default() += 1.0;
            }
        }
    }

    // Resonance lean: rank body-resonances by share; Neutral never counts as a lean.
    let mut ranked: Vec<(Resonance, f64)> = res_bodies
        .into_iter()
        .filter(|(r, _)| *r != Resonance::Neutral)
        .map(|(r, n)| (r, n / bodies.max(1.0)))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let lead = ranked.first().map_or(0.0, |&(_, s0)| {
        s0 - ranked.get(1).map_or(0.0, |&(_, s1)| s1)
    });
    let resonance = match (ranked.first(), ranked.get(1)) {
        // A `Mono` lean needs both a dominant share AND a clear gap to the
        // runner-up — otherwise the "top" is just the tallest blade of a flat
        // spread (Drifter), which is `Balanced`, not a lean.
        (Some(&(r0, s0)), _) if s0 >= MONO_RESONANCE_SHARE && lead >= MONO_RESONANCE_LEAD => {
            ResonanceLean::Mono(r0)
        }
        (Some(&(r0, s0)), Some(&(r1, s1)))
            if s0 >= DUAL_RESONANCE_SHARE && s1 >= DUAL_RESONANCE_SHARE =>
        {
            // Present the pair in a stable wheel order (lower enum discriminant first)
            // so the label is deterministic regardless of which edged out the other.
            if (r0 as u8) <= (r1 as u8) {
                ResonanceLean::Dual(r0, r1)
            } else {
                ResonanceLean::Dual(r1, r0)
            }
        }
        _ => ResonanceLean::Balanced,
    };

    let avg_cost = cost / cards.max(1.0);
    let tempo = if avg_cost < FAST_COST {
        Tempo::Fast
    } else if avg_cost > GRINDY_COST {
        Tempo::Grindy
    } else {
        Tempo::Even
    };

    // Aggression score: average body punch, lifted by the Relentless share (a
    // chain keyword is the aggressive lever the curve alone misses) and lowered
    // by wall density (Warded/Steadfast keywords per deck — a deck built to
    // absorb is, by that much, not built to push). This cleanly seats Embertide
    // (high punch + chain, few walls) at the top and The Long Watch (the most
    // walls) at the bottom, with the measured middle between.
    let avg_body_atk = body_atk / bodies.max(1.0);
    let relentless_frac = relentless / cards.max(1.0);
    let walls_per_deck = walls / seeds as f64;
    let aggro_score = avg_body_atk + relentless_frac * 100.0 - walls_per_deck * WALL_AGGRO_PENALTY;
    let aggression = if aggro_score >= AGGRO_HI {
        Aggression::Aggressive
    } else if aggro_score <= AGGRO_LO {
        Aggression::Defensive
    } else {
        Aggression::Measured
    };

    let spirit_fraction = spirits / cards.max(1.0);
    let body_mix = if spirit_fraction >= SPIRIT_HEAVY_FRACTION {
        BodyMix::SpiritHeavy
    } else {
        BodyMix::Balanced
    };

    SelectionInfo {
        resonance,
        tempo,
        aggression,
        body_mix,
    }
}

// Classification thresholds for [`measured_selection_info`]. Coarse by design —
// every band sits ≥3 sampling-units off the nearest style it must separate, so
// the qualitative label is stable under sampling noise (verified by
// `selection_info_matches_deckgen`). The measured values these bracket (over the
// 400-seed sample) are recorded inline.
/// A single body-resonance share at/above this — with the lead below — is a `Mono`
/// lean. (Embertide Fury ≈40%, The Long Watch Resolve ≈32%; Drifter's tallest ≈21%.)
const MONO_RESONANCE_SHARE: f64 = 0.30;
/// A `Mono` lean also needs the top resonance to lead the runner-up by this much,
/// so the tallest blade of a *flat* spread (Drifter, ≈5pt lead) reads `Balanced`.
const MONO_RESONANCE_LEAD: f64 = 0.12;
/// Two body-resonance shares each at/above this are a `Dual` lean. (Mistwalk
/// Wonder 35% / Fear 25%; The Choir Harmony 30% / Sorrow 30%.)
const DUAL_RESONANCE_SHARE: f64 = 0.25;
/// Average mana cost below this reads as `Fast` tempo. (Drifter 2.52, Mistwalk
/// 2.54, The Choir ≤2.57 are Fast; Embertide ≥2.59 is Even — the cut sits in the
/// clean gap between The Choir's top and Embertide's floor.)
const FAST_COST: f64 = 2.58;
/// Average mana cost above this reads as `Grindy` tempo. (The Long Watch 2.82.)
const GRINDY_COST: f64 = 2.70;
/// Wall density (Warded+Steadfast keyword cards/deck) costs this much aggression
/// score apiece — a deck built to absorb is, that much, not built to push.
const WALL_AGGRO_PENALTY: f64 = 2.0;
/// Aggression score (avg body Attack + 100×Relentless share − walls/deck × penalty)
/// at/above this is `Aggressive`. (Embertide ≈48; the measured middle ≈38–40.)
const AGGRO_HI: f64 = 44.0;
/// Aggression score at/below this is `Defensive`. (The Long Watch ≈31, dragged by
/// ~5 wall cards/deck; the measured middle ≈38.)
const AGGRO_LO: f64 = 35.0;
/// Spirit/caller fraction of the deck at/above this is `SpiritHeavy`. (The Long
/// Watch ≈74% and Mistwalk ≈68–69% are body-led; Embertide/Choir/Drifter ≈66%
/// sit just below — the cut threads Mistwalk's floor from Drifter's ceiling.)
const SPIRIT_HEAVY_FRACTION: f64 = 0.675;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::{canon_catalog, validate_deck_for};
    use crate::types::{CardKind, Faction};

    /// Seeds the selection-info verification samples — enough to settle the averages
    /// so the qualitative labels land in their bands (the labels are *defined* at this
    /// sample count and proven stable as it grows).
    const SELECTION_SAMPLE_SEEDS: u64 = 400;

    fn kind_of(id: CardId, cat: &[CardDef]) -> CardKind {
        cat.iter().find(|c| c.id == id).unwrap().kind
    }
    fn count<F: Fn(CardKind) -> bool>(deck: &[CardId], cat: &[CardDef], pred: F) -> usize {
        deck.iter().filter(|&&id| pred(kind_of(id, cat))).count()
    }

    /// A character's disposition biases its draw. Probabilistic, so we compare the
    /// tendency against a balanced (uniform) draw across many seeds rather than asserting one deck.
    #[test]
    fn solace_dispositions_bias_the_draw() {
        let cat = canon_catalog();
        let (mut cruel_ill, mut bal_ill, mut erase_ue, mut bal_ue) =
            (0usize, 0usize, 0usize, 0usize);
        for seed in 0..40u64 {
            cruel_ill += count(
                &generate_deck_for(Faction::Solace, 0, seed, &cat),
                &cat,
                |k| k == CardKind::IllIntent,
            );
            erase_ue += count(
                &generate_deck_for(Faction::Solace, 1, seed, &cat),
                &cat,
                |k| matches!(k, CardKind::Unwritten | CardKind::Unwriting),
            );
            let bal = generate_deck_for(Faction::Solace, 99, seed, &cat); // out-of-range → balanced
            bal_ill += count(&bal, &cat, |k| k == CardKind::IllIntent);
            bal_ue += count(&bal, &cat, |k| {
                matches!(k, CardKind::Unwritten | CardKind::Unwriting)
            });
        }
        assert!(
            cruel_ill > bal_ill,
            "Cruelty should field more ill intent than balanced ({cruel_ill} vs {bal_ill})"
        );
        assert!(
            erase_ue > bal_ue,
            "Erasure should field more Unwritten+events than balanced ({erase_ue} vs {bal_ue})"
        );
    }

    /// Evolution-aware generation: every generated Lorekeeper deck — bot or Quick Play —
    /// reliably fields **landable base↔form pairs** (a base AND a form it reaches), so the evolution
    /// phase is always live play (you must HOLD the form card to evolve). We assert across many
    /// seeds and every style that: no deck drops below [`EVOLUTION_FLOOR`] pairs; the opening curve
    /// survives the pairing pass (≥8 Cost≤2 cards); and there are **no orphan forms** (every form has
    /// a base in the deck — `validate_deck_for` enforces this). The floor is held modestly (2 pairs)
    /// on purpose: a form costs a deck slot now, and the evolving cards are dearer than the pool, so a
    /// higher floor would raise the curve and tip the first-mover balance the 1v1 anchor guards.
    #[test]
    fn lorekeeper_decks_reliably_field_evolutions() {
        let cat = canon_catalog();
        let by_id = |id: CardId| cat.iter().find(|c| c.id == id).unwrap();
        let count_pairs = |deck: &[CardId]| -> usize {
            let names: std::collections::BTreeSet<&str> =
                deck.iter().map(|&id| by_id(id).name.as_str()).collect();
            deck.iter()
                .map(|&id| by_id(id))
                .filter(|b| {
                    !b.evolves_to.is_empty()
                        && b.evolves_to.iter().any(|f| names.contains(f.as_str()))
                })
                .count()
        };
        for style in 0u8..STYLES.len() as u8 {
            let n = 300u64;
            for seed in 0..n {
                let deck = generate_deck(style, seed, &cat);
                // The hard floor: NO deck ever drops below it.
                let pairs = count_pairs(&deck);
                assert!(
                    pairs >= EVOLUTION_FLOOR,
                    "style {style} seed {seed}: only {pairs} landable pairs (floor {EVOLUTION_FLOOR})"
                );
                // The pairing pass must never break the opening curve …
                let cheap = deck.iter().filter(|&&id| by_id(id).cost <= 2).count();
                assert!(
                    cheap >= 8,
                    "style {style} seed {seed}: only {cheap} cheap cards"
                );
                // … nor deck legality (singleton, faction-pure, and NO orphan forms).
                validate_deck_for(&deck, &cat, Faction::Lorekeeper)
                    .unwrap_or_else(|e| panic!("style {style} seed {seed}: {e:?}"));
            }
        }
    }

    /// The Solace pool now carries evolution lines (the 8 Primal Deepenings), so the
    /// **same** `ensure_evolution_floor` + `validate_deck_for` no-orphan path that serves
    /// the Lorekeeper serves the Solace too (a SHARED path, not a parallel Solace copy):
    /// a Deepening is seeded only alongside a base that reaches it, and the deck validates
    /// no-orphan. Every Solace deck across styles/seeds is legal AND no-orphan — and any
    /// Deepening present has its base present (the shared `evolves_to` pairing).
    #[test]
    fn solace_decks_pair_deepenings_with_their_base_via_the_shared_no_orphan_path() {
        let cat = canon_catalog();
        for style in 0u8..6 {
            for seed in 0..20u64 {
                let deck = generate_deck_for(Faction::Solace, style, seed, &cat);
                // The SHARED no-orphan validator (identical for both factions) accepts it.
                validate_deck_for(&deck, &cat, Faction::Solace)
                    .unwrap_or_else(|e| panic!("Solace style {style} seed {seed}: {e:?}"));
                // And concretely: any Deepening (Evolution form) in the deck has a base in
                // the deck that reaches it — the no-orphan guarantee, the same rule the
                // Lorekeeper floor relies on.
                let names: std::collections::BTreeSet<&str> = deck
                    .iter()
                    .filter_map(|&id| cat.iter().find(|c| c.id == id).map(|c| c.name.as_str()))
                    .collect();
                for &id in &deck {
                    let c = cat.iter().find(|c| c.id == id).unwrap();
                    if c.kind == crate::types::CardKind::Evolution {
                        assert!(
                            cat.iter().any(|b| names.contains(b.name.as_str())
                                && b.evolves_to.iter().any(|f| f == &c.name)),
                            "Solace style {style} seed {seed}: {} is an orphan Deepening",
                            c.name
                        );
                    }
                }
            }
        }
    }

    /// Every authored character must generate a legal Solace deck across seeds — no disposition
    /// can starve the singleton draw of a valid 20.
    #[test]
    fn every_solace_character_builds_a_legal_deck() {
        let cat = canon_catalog();
        for (i, character) in SOLACE_CHARACTERS.iter().enumerate() {
            for seed in 0..8u64 {
                let deck = solace_character_deck(i, seed, &cat);
                validate_deck_for(&deck, &cat, Faction::Solace)
                    .unwrap_or_else(|e| panic!("{} (#{i}) seed {seed}: {e:?}", character.name));
            }
        }
    }

    /// The opener toss is seeded (deterministic), fair at bias 0, and the
    /// initiative bias actually shifts the odds — an edge, not a guarantee.
    #[test]
    fn opener_is_seeded_fair_and_biasable() {
        use crate::types::Seat;
        // Deterministic: same (seed, bias) → same opener.
        assert_eq!(decide_opener(42, 0), decide_opener(42, 0));
        // Fair at bias 0 — both seats open across seeds, near 50/50.
        let a = (0..200u64)
            .filter(|&s| decide_opener(s, 0) == Seat::A)
            .count();
        assert!(
            (70..=130).contains(&a),
            "bias-0 toss skewed: A opened {a}/200"
        );
        // The initiative bias shifts the toss: +favours A, −favours B.
        let a_plus = (0..200u64)
            .filter(|&s| decide_opener(s, 40) == Seat::A)
            .count();
        let a_minus = (0..200u64)
            .filter(|&s| decide_opener(s, -40) == Seat::A)
            .count();
        assert!(
            a_plus > a && a > a_minus,
            "initiative should shift the toss ({a_minus} < {a} < {a_plus})"
        );
    }

    /// Every Lorekeeper character builds a legal deck across seeds (no style starves the draw).
    #[test]
    fn every_lorekeeper_character_builds_a_legal_deck() {
        let cat = canon_catalog();
        for (i, character) in LOREKEEPER_CHARACTERS.iter().enumerate() {
            for seed in 0..8u64 {
                let deck = lorekeeper_character_deck(i, seed, &cat);
                validate_deck_for(&deck, &cat, Faction::Lorekeeper)
                    .unwrap_or_else(|e| panic!("{} (#{i}) seed {seed}: {e:?}", character.name));
            }
        }
    }

    /// The two rosters are well-formed parity twins — each is the full 20, every name is
    /// unique, and every alignment index is in range for its faction's weight table (Solace
    /// dispositions 0..=4 → [`solace_weight`]; Lorekeeper styles 0..=4 → [`STYLES`]). A roster
    /// authoring slip (a duplicate name, an out-of-range index that silently falls through to the
    /// balanced bucket) is caught here, not in the wild.
    #[test]
    fn the_character_rosters_are_well_formed() {
        assert_eq!(SOLACE_CHARACTERS.len(), 20, "Solace roster should be 20");
        assert_eq!(
            LOREKEEPER_CHARACTERS.len(),
            20,
            "Lorekeeper roster should be 20"
        );
        let solace_names: std::collections::BTreeSet<&str> =
            SOLACE_CHARACTERS.iter().map(|c| c.name).collect();
        assert_eq!(
            solace_names.len(),
            SOLACE_CHARACTERS.len(),
            "Solace names must be unique"
        );
        let lk_names: std::collections::BTreeSet<&str> =
            LOREKEEPER_CHARACTERS.iter().map(|c| c.name).collect();
        assert_eq!(
            lk_names.len(),
            LOREKEEPER_CHARACTERS.len(),
            "Lorekeeper names must be unique"
        );
        for c in &SOLACE_CHARACTERS {
            assert!(
                c.disposition <= 4,
                "{}: disposition {} out of range 0..=4",
                c.name,
                c.disposition
            );
        }
        for c in &LOREKEEPER_CHARACTERS {
            assert!(
                (c.style as usize) < STYLES.len(),
                "{}: style {} out of range",
                c.name,
                c.style
            );
        }
    }

    /// The distinctness gate: no two characters field the SAME deck. The deck a character
    /// fields is its alignment style salted by the roster index, so even the four characters that
    /// share a style (or disposition) draw materially different cards — the player meets a varied
    /// opponent, ally, and mirror, not the same 20 cards under different names. We assert it for
    /// BOTH factions across several seeds: at every seed all 20 decks (as multisets) are pairwise
    /// distinct, and same-style/same-disposition pairs overlap only loosely (a bounded Jaccard
    /// similarity), proving the salt actually re-rolls the draw rather than nudging it.
    #[test]
    fn every_character_fields_a_distinct_deck() {
        let cat = canon_catalog();
        // A deck as a sorted multiset of card ids, for set comparison.
        let bag = |mut d: Vec<CardId>| {
            d.sort_by_key(|c| c.0);
            d
        };
        let jaccard = |a: &[CardId], b: &[CardId]| -> f64 {
            let sa: std::collections::BTreeSet<u16> = a.iter().map(|c| c.0).collect();
            let sb: std::collections::BTreeSet<u16> = b.iter().map(|c| c.0).collect();
            let inter = sa.intersection(&sb).count() as f64;
            let union = sa.union(&sb).count().max(1) as f64;
            inter / union
        };
        for seed in [1u64, 7, 42, 1000] {
            // Solace: 20 decks, pairwise distinct; same-disposition pairs only loosely overlap.
            let solace: Vec<(u8, Vec<CardId>)> = (0..SOLACE_CHARACTERS.len())
                .map(|i| {
                    (
                        SOLACE_CHARACTERS[i].disposition,
                        bag(solace_character_deck(i, seed, &cat)),
                    )
                })
                .collect();
            // Lorekeeper: the same contract over its 20.
            let lk: Vec<(u8, Vec<CardId>)> = (0..LOREKEEPER_CHARACTERS.len())
                .map(|i| {
                    (
                        LOREKEEPER_CHARACTERS[i].style,
                        bag(lorekeeper_character_deck(i, seed, &cat)),
                    )
                })
                .collect();
            for (roster, decks) in [("Solace", &solace), ("Lorekeeper", &lk)] {
                for a in 0..decks.len() {
                    for b in (a + 1)..decks.len() {
                        assert_ne!(
                            decks[a].1, decks[b].1,
                            "{roster} characters #{a} and #{b} field identical decks at seed {seed}"
                        );
                        // Two characters of the same alignment share an archetype, so SOME overlap
                        // is expected — but the salt must keep them clearly different decks, not
                        // near-copies. (Cross-alignment pairs are even more distinct, so bounding
                        // the same-alignment case is the strict test.)
                        if decks[a].0 == decks[b].0 {
                            let sim = jaccard(&decks[a].1, &decks[b].1);
                            assert!(
                                sim < 0.6,
                                "{roster} #{a}/#{b} (same alignment {}) too similar at seed {seed}: jaccard {sim:.2}",
                                decks[a].0
                            );
                        }
                    }
                }
            }
        }
    }

    /// The objective **selection-info** must be ACCURATE, not hand-waved: every
    /// authored [`DeckStyle::selection`] label has to match what the style's deck-gen
    /// weighting actually produces. We measure each dimension off real generated decks
    /// (over [`SELECTION_SAMPLE_SEEDS`]) via [`measured_selection_info`] and assert the
    /// authored labels equal the measured ones, dimension by dimension so a drift names
    /// itself. This is the gate that keeps a label honest: re-tune the deck weighting and
    /// this fails until the trait is re-cut to the new reality — the player chooses on
    /// facts the engine can vouch for.
    #[test]
    fn selection_info_matches_deckgen() {
        let cat = canon_catalog();
        for s in &STYLES {
            let measured = measured_selection_info(s.id, SELECTION_SAMPLE_SEEDS, &cat);
            assert_eq!(
                s.selection.resonance, measured.resonance,
                "{}: authored resonance lean {:?} ≠ deck-gen {:?}",
                s.name, s.selection.resonance, measured.resonance
            );
            assert_eq!(
                s.selection.tempo, measured.tempo,
                "{}: authored tempo {:?} ≠ deck-gen {:?} (avg-cost band)",
                s.name, s.selection.tempo, measured.tempo
            );
            assert_eq!(
                s.selection.aggression, measured.aggression,
                "{}: authored aggression {:?} ≠ deck-gen {:?} (punch/chain/walls)",
                s.name, s.selection.aggression, measured.aggression
            );
            assert_eq!(
                s.selection.body_mix, measured.body_mix,
                "{}: authored body-mix {:?} ≠ deck-gen {:?} (spirit fraction)",
                s.name, s.selection.body_mix, measured.body_mix
            );
        }
    }

    /// The selection-info is **well-formed** for every style: each style id is the
    /// table index, a resonance lean never names the unaligned `Neutral` nor repeats a
    /// resonance, and the shape carries the expected spread — exactly one
    /// `Aggressive` style (Embertide, the lone aggressor), exactly one `Grindy` style (The
    /// Long Watch, the lone grinder), and at least one `Balanced` resonance (Drifter). A
    /// regression that flattened the archetypes into a single shape is caught here.
    #[test]
    fn selection_info_is_well_formed() {
        for (i, s) in STYLES.iter().enumerate() {
            assert_eq!(s.id as usize, i, "STYLES[{i}] has id {}", s.id);
            match s.selection.resonance {
                ResonanceLean::Mono(r) => {
                    assert_ne!(
                        r,
                        Resonance::Neutral,
                        "{}: Mono(Neutral) is not a lean",
                        s.name
                    )
                }
                ResonanceLean::Dual(a, b) => {
                    assert_ne!(a, Resonance::Neutral, "{}: Dual names Neutral", s.name);
                    assert_ne!(b, Resonance::Neutral, "{}: Dual names Neutral", s.name);
                    assert_ne!(a, b, "{}: Dual repeats a resonance", s.name);
                }
                ResonanceLean::Balanced => {}
            }
        }
        let aggressive = STYLES
            .iter()
            .filter(|s| s.selection.aggression == Aggression::Aggressive)
            .count();
        assert_eq!(
            aggressive, 1,
            "exactly one style should be Aggressive (Embertide)"
        );
        let grindy = STYLES
            .iter()
            .filter(|s| s.selection.tempo == Tempo::Grindy)
            .count();
        assert_eq!(
            grindy, 1,
            "exactly one style should be Grindy (The Long Watch)"
        );
        assert!(
            STYLES
                .iter()
                .any(|s| s.selection.resonance == ResonanceLean::Balanced),
            "at least one style should be resonance-Balanced (Drifter's Bindle)"
        );
    }

    /// [`measured_selection_info`] is itself **deterministic** (it must be, to gate the
    /// labels): same (style, seeds) yields the same measurement. And the labels are **stable**,
    /// not overfit to one sample size — DOUBLING the sample (which only tightens the estimate)
    /// lands the same qualitative label, so the authored traits sit comfortably inside their
    /// bands rather than on a knife-edge. (A few styles ride a band boundary — e.g. Embertide's
    /// curve straddles Fast/Even — so the bands are defined at [`SELECTION_SAMPLE_SEEDS`] and
    /// proven to hold as the sample grows, not shrink.)
    #[test]
    fn measured_selection_info_is_deterministic_and_stable() {
        let cat = canon_catalog();
        for s in &STYLES {
            assert_eq!(
                measured_selection_info(s.id, 256, &cat),
                measured_selection_info(s.id, 256, &cat),
                "{}: measurement is not deterministic",
                s.name
            );
            // Doubling the sample keeps the same label — the traits aren't sample-overfit.
            assert_eq!(
                measured_selection_info(s.id, SELECTION_SAMPLE_SEEDS * 2, &cat),
                s.selection,
                "{}: the label drifts when the sample doubles — band too tight",
                s.name
            );
        }
    }

    /// The display layer the picker renders is **total and non-empty** for every
    /// style: each of the four facets carries a dimension, a value word, and a
    /// detail gloss, and the one-line summary is non-empty. This is what the web
    /// chips and the CLI line read; an empty label would be a blank chip / a hole
    /// in the accessible text. The facets are in a stable reading order so the
    /// picker (and a screen reader) presents the same sequence every time.
    #[test]
    fn selection_facets_are_total_and_ordered() {
        for s in &STYLES {
            let facets = s.selection.facets();
            let dims: Vec<&str> = facets.iter().map(|f| f.dimension).collect();
            assert_eq!(
                dims,
                ["Resonance", "Tempo", "Aggression", "Body mix"],
                "{}: facet order/headings drifted",
                s.name
            );
            for f in &facets {
                assert!(
                    !f.value.is_empty(),
                    "{}: empty value for {}",
                    s.name,
                    f.dimension
                );
                assert!(
                    !f.detail.is_empty(),
                    "{}: empty detail for {}",
                    s.name,
                    f.dimension
                );
            }
            // The summary line (CLI + the web card's accessible name) is non-empty
            // and threads the four value words through — it IS the at-a-glance read.
            let summary = s.selection.summary();
            assert!(!summary.is_empty(), "{}: empty selection summary", s.name);
            assert!(
                summary.contains(s.selection.resonance.label()),
                "{}: summary drops the resonance lean",
                s.name
            );
        }
    }

    /// The facet labels are the EXACT words the authored style intends — a spot-check
    /// pinning the human-facing wording to a representative style of each shape, so a
    /// careless relabel (or a wrong enum→word mapping) is caught. Embertide is the lone
    /// `Aggressive`/`Mono(Fury)`; The Long Watch the lone `Grindy`/`Defensive`/`SpiritHeavy`;
    /// Mistwalk a `Dual` lean; Drifter the `Balanced` resonance.
    #[test]
    fn selection_labels_read_as_authored() {
        let by_name = |n: &str| STYLES.iter().find(|s| s.name == n).unwrap().selection;
        let ember = by_name("Embertide");
        assert_eq!(ember.resonance.label(), "Fury");
        assert_eq!(ember.aggression.label(), "Aggressive");
        assert_eq!(ember.tempo.label(), "Even");

        let watch = by_name("The Long Watch");
        assert_eq!(watch.tempo.label(), "Grindy");
        assert_eq!(watch.aggression.label(), "Defensive");
        assert_eq!(watch.body_mix.label(), "Spirit-heavy");

        // A Dual lean joins its pair in stable wheel order ("Wonder + Fear").
        assert_eq!(by_name("Mistwalk").resonance.label(), "Wonder + Fear");
        // The whole-wheel deck reads "Balanced", never a phantom lead.
        assert_eq!(by_name("Drifter's Bindle").resonance.label(), "Balanced");
    }

    /// `dual_label` is **order-insensitive** (the same pair either way round yields the
    /// same `&'static str`) and covers EVERY off-`Neutral` unordered pair with a real
    /// joined label (never the single-resonance fallback). This guards the picker against
    /// a missing arm silently degrading a two-colour deck to a one-colour label.
    #[test]
    fn dual_label_is_symmetric_and_total() {
        use Resonance::*;
        let palette = [Wonder, Fear, Sorrow, Harmony, Fury, Resolve];
        for (i, &a) in palette.iter().enumerate() {
            for &b in &palette[i + 1..] {
                let ab = dual_label(a, b);
                let ba = dual_label(b, a);
                assert_eq!(ab, ba, "dual_label({a:?},{b:?}) not symmetric");
                assert!(
                    ab.contains(" + "),
                    "dual_label({a:?},{b:?}) fell back to a mono label"
                );
                assert!(
                    ab.contains(a.label()) && ab.contains(b.label()),
                    "dual_label({a:?},{b:?}) = {ab:?} drops a resonance"
                );
            }
        }
    }
}
