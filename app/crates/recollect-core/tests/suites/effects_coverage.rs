//! The honesty ratchet: how much of the canon does the ENGINE implement,
//! versus carry as data? Keyword spirits (Arcane/Warded/Mobile/Steadfast/
//! Relentless + vanilla) are fully engine-backed and tested in rules.rs.
//! The whole spellbook and the evolution **forms** (deck-playable: drawn, then
//! played onto a base) are engine-backed too; what remains DATA is Foundling
//! temperaments and some Solace behaviors. These numbers may only move toward
//! implementation: the data-only count may only shrink, and an unimplemented
//! card class may never silently enter decks.
use recollect_core::cards::canon_catalog;
use recollect_core::types::CardKind;

const KEYWORDS: [&str; 9] = [
    "Arcane",
    "Warded",
    "Mobile",
    "Steadfast",
    "Relentless",
    "Gentle",
    "Wary",
    "Feral",
    "—",
];

fn keyword_only(rules: &str) -> bool {
    let stripped: String = rules
        .replace(['.', ',', ';'], " ")
        .split_whitespace()
        .filter(|w| !KEYWORDS.contains(w))
        .collect::<Vec<_>>()
        .join(" ");
    stripped.trim().is_empty()
}

/// Drift gate: every behavior-bearing playable card is authored in
/// effects.json or explicitly pending; no orphans (doc renames fail here);
/// every Summon target resolves to a real card.
#[test]
fn authored_effects_cover_the_playable_canon() {
    use recollect_core::effects::{Effect, canon_effects};
    let cat = canon_catalog();
    let ef = canon_effects();
    // effects.json keys off the stable card `key` (not the display name) — so the orphan
    // check and all membership tests are by `key`. (`names` stays only for the Summon
    // target check below, whose card_name is a name reference resolved via key_of.)
    let names: std::collections::HashSet<_> = cat.iter().map(|c| c.name.as_str()).collect();
    let keys: std::collections::HashSet<_> = cat.iter().map(|c| c.key.as_str()).collect();
    for key in ef
        .specs
        .keys()
        .chain(ef.pending.iter())
        .chain(ef.behavior.iter())
    {
        assert!(
            keys.contains(key.as_str()),
            "orphan effects entry (unknown card key): {key}"
        );
    }
    let mut authored = 0;
    for c in &cat {
        if !keyword_only(&c.rules) && !c.rules.trim().is_empty() {
            // A card listed in `behavior` is keyword-backed (its text adds no
            // engine mechanic beyond keywords already on the card, or is pure
            // flavor). This covers Foundling dispositions AND keyword-only
            // antagonist creatures whose flavor tail trips keyword_only.
            if !ef.specs.contains_key(&c.key) && ef.behavior.contains(&c.key) {
                continue;
            }
            assert!(
                ef.specs.contains_key(&c.key) || ef.pending.contains(&c.key),
                "playable card neither authored nor pending: {} ({})",
                c.name,
                c.key
            );
            if ef.specs.contains_key(&c.key) {
                authored += 1;
            }
        }
    }
    assert!(
        authored >= 185,
        "authored ratchet (playable 59 + tokens 6 + rituals 42): {authored}"
    );
    // The Solace expansion added cards; many are now authored, the rest pending.
    // The ceiling tracks the current pending set and only shrinks from here as
    // more effects get authored. 51 → 46 so far (all behavior-tested in
    // evolution_arrivals.rs): Colossus (Defense aura), Verdure (arrival damage,
    // was mis-filed), `RevealFabrication` wired → Cirrus un-pended + Zenith's
    // decorative reveal fixed, Standard-Bearer (Flame tribal Attack aura), and
    // Felis Umbra (face-down adjacent-enemy Attack debuff).
    assert!(
        ef.pending.len() <= 46,
        "pending only shrinks: {}",
        ef.pending.len()
    );
    for c in &cat {
        if c.kind.deck_playable() {
            assert!(
                !ef.pending.contains(&c.key),
                "playable may not be pending: {}",
                c.name
            );
        }
    }
    for specs in ef.specs.values() {
        for s in specs {
            for clause in &s.clauses {
                if let Effect::Summon { card_name } = &clause.effect {
                    assert!(
                        names.contains(card_name.as_str()),
                        "Summon target missing: {card_name}"
                    );
                }
            }
        }
    }
}

/// Red-team guard: every TRIGGER an authored spec relies on must be one the engine
/// actually fires. `supported_trigger` is the audited fired-set (the supported↔exec test
/// keeps it honest), so a spec trigger outside it is dead — the clauses under it never run.
/// This catches the class a deck-playable-only ratchet misses: a declared-but-unfired
/// trigger silently kills its card's effect (e.g. an `OnMove` declared but fired nowhere).
#[test]
fn every_authored_spec_trigger_is_fired() {
    use recollect_core::effects::{canon_effects, supported_trigger};
    let cat = canon_catalog();
    let ef = canon_effects();
    let by_key: std::collections::HashMap<_, _> = cat.iter().map(|c| (c.key.as_str(), c)).collect();
    let mut dead: Vec<(String, String)> = Vec::new();
    for (key, specs) in &ef.specs {
        for s in specs {
            if !supported_trigger(s.trigger) {
                let name = by_key
                    .get(key.as_str())
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| key.clone());
                dead.push((name, format!("{:?}", s.trigger)));
            }
        }
    }
    dead.sort();
    // Every authored spec trigger is fired through `fire_doctrine`: OnMove (The
    // Devouring Margin's eat via the inward shift), OnUnwrite (Sentence Fragment /
    // Footnote mill on the fade step, `flow.rs`), and OnBefriend (Pigeon draws when
    // befriended, `strays.rs`). This set must stay EMPTY: a new entry is a regression
    // (a declared-but-dead trigger).
    let known: Vec<(String, String)> = Vec::new();
    assert_eq!(
        dead, known,
        "dead-trigger set is non-empty: a spec declares a trigger that is fired nowhere — wire it through fire_doctrine"
    );
}

/// The NON-DECK coverage gate (closes a card-red-team blind spot). The
/// `engine_implemented_versus_data_only_ratchet` below gates only DECK-PLAYABLE cards —
/// so an effect-bearing NON-deck card (the Solace's Unwritten/IllIntent creatures, the
/// Unwriting events, the Foundlings, the Kindred) could carry an authored spec that
/// reaches NO executor and fire nothing, invisibly. This gate closes that hole: EVERY
/// effect-bearing card — deck-playable or not — must be `card_fully_supported` (its trigger
/// fired AND every clause shape executed). A non-deck card with an authored-but-dead clause
/// is a HARD FAIL here, not a silent no-op.
///
/// The non-deck set is where this class hides — e.g. TraitSilence (The Smudge / Null
/// Choir), Restrict(BeTargetedByRituals) (What's-Its-Name), CopyPrintedStats (The
/// Misremembered), CostDelta/BothNarrators (Ink Runs Dry), TraitStrip/EngagedEnemy
/// (Smear), The Devouring Margin's RestoreForm heal-on-eat, and the Erasure's Patience
/// (Restrict(GainImpressions) → AdjacentImpressionsDontScore) — all engine-backed and
/// outcome-tested next to their kin (solace_effects.rs / effects_red_team.rs). Every
/// clause shape they use is in the `supported_*_clause` tranche, so this gate holds
/// them implemented.
#[test]
fn every_non_deck_effect_bearing_card_is_engine_backed() {
    use recollect_core::effects::{canon_effects, card_fully_supported};
    let cat = canon_catalog();
    let ef = canon_effects();
    let non_deck: Vec<_> = cat
        .iter()
        .filter(|c| !c.kind.deck_playable())
        .filter(|c| ef.specs.contains_key(&c.key))
        .collect();
    let dead: Vec<String> = non_deck
        .iter()
        .filter(|c| !card_fully_supported(&c.name))
        .map(|c| format!("{:?} {} ({})", c.kind, c.name, c.key))
        .collect();
    assert!(
        dead.is_empty(),
        "{} non-deck effect-bearing card(s) carry an authored spec that reaches NO executor \
         (dead effect — the non-deck blind spot). Wire each clause through an executor + the \
         `supported_*_clause` tranche, or move pure-flavor text to the `behavior` table:\n  {}",
        dead.len(),
        dead.join("\n  ")
    );
    // The non-deck effect-bearing surface is sizeable — this is the count the gate now guards.
    // (A ratchet floor: it only grows as Solace/Foundling effects are authored.)
    assert!(
        non_deck.len() >= 64,
        "non-deck effect-bearing coverage shrank: {} (every one must stay engine-backed)",
        non_deck.len()
    );
}

#[test]
fn engine_implemented_versus_data_only_ratchet() {
    let cat = canon_catalog();
    // The deck-playable set is spirits + callers + the spellbook + the
    // evolution FORMS (drawn, then played onto a base).
    let playable: Vec<_> = cat.iter().filter(|c| c.kind.deck_playable()).collect();
    let spirits = cat
        .iter()
        .filter(|c| matches!(c.kind, CardKind::Spirit | CardKind::Caller))
        .count();
    let forms = cat.iter().filter(|c| c.kind == CardKind::Evolution).count();
    assert_eq!(
        spirits, 114,
        "spirits + callers (incl. the curve-fill bases + 3 lurkers)"
    );
    assert_eq!(
        forms, 60,
        "the 48 evolution forms are deck-playable, + the 12 Solace Primal Deepenings"
    );
    assert_eq!(
        playable.len(),
        114 + 120 + 60,
        "114 spirits/callers + the 120-card spellbook + 60 evolution forms (48 Lorekeeper + 12 Solace Deepenings)"
    );

    let fully_implemented = playable
        .iter()
        .filter(|c| {
            keyword_only(&c.rules) || recollect_core::effects::card_fully_supported(&c.name)
        })
        .count();
    let effect_text_pending = playable.len() - fully_implemented;

    // RATCHET (only tighten): the implemented count may rise and the data-only
    // count may fall, never the reverse. The bar below is the current floor —
    // every one of the 294 deck-playable cards is engine-backed (keyword-only or
    // `card_fully_supported`), none data-only. Per-mechanic coverage lives in the
    // feature suites (bond_auras.rs, terrain_auras.rs, target_choice.rs,
    // reach_auras.rs, restrictions.rs, recover.rs, fabrication_traps.rs,
    // flow_effects.rs, evolve.rs, solace_deepenings.rs).
    assert!(
        fully_implemented >= 294,
        "implemented count regressed: {fully_implemented}"
    );
    assert!(
        effect_text_pending == 0,
        "data-only count grew: {effect_text_pending}"
    );

    // Unplayable kinds must never leak into deck legality. (Evolution forms ARE deck-playable,
    // so they leave this list — they're covered by the playable-count + ratchet asserts above.)
    for c in &cat {
        if matches!(
            c.kind,
            CardKind::Kindred
                | CardKind::Unwritten
                | CardKind::IllIntent
                | CardKind::Unwriting
                | CardKind::Foundling
        ) {
            assert!(
                !c.kind.deck_playable(),
                "{} ({:?}) must stay non-deck",
                c.name,
                c.kind
            );
        }
    }
}
