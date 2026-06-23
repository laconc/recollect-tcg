//! The embedded card catalog: `canon_catalog()` returns the card definitions parsed
//! from the generated `catalog.json`, keyed by the stable card `key` — the runtime
//! source of card truth (no DB, no fetch; version-locked to the engine).

use crate::types::{CardDef, CardId, Reach, Resonance};

/// The full canon — every one of the 419 designed cards, generated from
/// `data/cards.toml` (the card source-of-truth) by tools/gen_catalog.py (`make catalog`;
/// CI diffs via `make catalog-check` to catch source/code drift). Stats are Attack/Defense/HP;
/// ids are stable and dense (`0..419`, id-sorted — relied on by the engine's `card`
/// resolver and [`crate::effects::specs_for`]).
///
/// Every card class plays: Spirits, Callers, Rituals, Bonds, Landmarks,
/// Fabrications, evolution forms (incl. the 12 Solace Primal Deepenings — 4 bases offer a
/// gentle-or-malign menu), Strays, and the Solace's Unwritten. The effects ratchet
/// (`tests/suites/effects_coverage.rs`) stands at **294/0** — every deck-playable
/// card is engine-backed, none data-only.
pub fn canon_catalog() -> Vec<CardDef> {
    let raw = include_str!("../data/catalog.json");
    let cards: Vec<CardDef> = serde_json::from_str(raw).expect("canon catalog parses");
    cards
}

/// Resolve a card's display `name` to its stable `key` (the canonical identity effects are
/// keyed by). Built once from the catalog, which carries both, so it survives renames: a
/// renamed card's new name still maps to its frozen key. Unknown names map to `""` (no spec).
/// This is the single seam that turns a name-in-hand into the key effects + engine logic use —
/// associations are by `key`, never by grepping prose.
pub fn key_of(name: &str) -> &'static str {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    MAP.get_or_init(|| {
        canon_catalog()
            .into_iter()
            .map(|c| (c.name, c.key))
            .collect()
    })
    .get(name)
    .map(String::as_str)
    .unwrap_or("")
}

pub fn test_catalog() -> Vec<CardDef> {
    use Reach::*;
    use Resonance::*;
    struct K {
        arcane: bool,
        warded: bool,
        mobile: bool,
        steadfast: bool,
        relentless: bool,
    }
    const PLAIN: K = K {
        arcane: false,
        warded: false,
        mobile: false,
        steadfast: false,
        relentless: false,
    };
    let c = |id: u16,
             name: &'static str,
             cost: u8,
             a: i16,
             d: i16,
             h: i16,
             reach: Reach,
             res: Resonance,
             k: K| CardDef {
        id: CardId(id),
        name: name.into(),
        cost,
        attack: a,
        defense: d,
        hp: h,
        reach,
        resonance: res,
        arcane: k.arcane,
        warded: k.warded,
        mobile: k.mobile,
        steadfast: k.steadfast,
        relentless: k.relentless,
        ..Default::default()
    };
    vec![
        c(0, "Dawnling", 1, 10, 20, 30, Cross, Wonder, PLAIN),
        c(
            1,
            "Stargazer Heron",
            3,
            40,
            20,
            40,
            Cross,
            Wonder,
            K {
                arcane: true,
                ..PLAIN
            },
        ),
        c(2, "Hushling", 1, 10, 20, 30, Cross, Fear, PLAIN),
        c(3, "Pale Stalker", 3, 40, 20, 40, Slant, Fear, PLAIN),
        c(4, "Tearling", 1, 10, 20, 30, Cross, Sorrow, PLAIN),
        c(5, "Greyfin Seal", 2, 20, 30, 40, Cross, Sorrow, PLAIN),
        c(6, "Sproutling", 1, 10, 20, 30, Cross, Harmony, PLAIN),
        c(
            7,
            "Hymnal Hart",
            3,
            20,
            30,
            50,
            Cross,
            Harmony,
            K {
                warded: true,
                ..PLAIN
            },
        ),
        c(8, "Cinderling", 1, 20, 20, 20, Cross, Fury, PLAIN),
        c(9, "Bristleboar", 2, 40, 10, 40, Lance, Fury, PLAIN),
        c(10, "Pebbling", 1, 0, 30, 30, Cross, Resolve, PLAIN),
        c(
            11,
            "Warded Ram",
            3,
            30,
            20,
            50,
            Cross,
            Resolve,
            K {
                warded: true,
                ..PLAIN
            },
        ),
        c(12, "Charging Auroch", 3, 40, 20, 50, Lance, Fury, PLAIN),
        c(13, "Aurora Elk", 4, 30, 40, 70, Cross, Wonder, PLAIN),
        c(
            14,
            "Spark Shrew",
            1,
            20,
            10,
            20,
            Cross,
            Fury,
            K {
                mobile: true,
                ..PLAIN
            },
        ),
        c(
            15,
            "Watchful Marmot",
            1,
            10,
            30,
            20,
            Cross,
            Resolve,
            K {
                steadfast: true,
                ..PLAIN
            },
        ),
        c(
            16,
            "Brand-Bearer Macaque",
            3,
            40,
            10,
            40,
            Lance,
            Fury,
            K {
                relentless: true,
                ..PLAIN
            },
        ),
        c(17, "Kilnhorn Rhino", 5, 60, 30, 70, Lance, Fury, PLAIN),
    ]
}

/// Constructed-deck legality: exactly 20 cards, max 2 copies. (Type-mix caps
/// arrive with Rituals/Bonds/etc.)
pub const DECK_SIZE: usize = 20;

pub fn validate_deck(deck: &[CardId], catalog: &[CardDef]) -> Result<(), String> {
    if deck.len() != DECK_SIZE {
        return Err(format!(
            "deck must be {} cards, got {}",
            DECK_SIZE,
            deck.len()
        ));
    }
    let mut counts: std::collections::BTreeMap<CardId, u8> = std::collections::BTreeMap::new();
    for id in deck {
        if !catalog.iter().any(|c| c.id == *id) {
            return Err(format!("unknown card id {:?}", id));
        }
        let n = counts.entry(*id).or_insert(0);
        *n += 1;
        if *n > 2 {
            return Err(format!("more than 2 copies of {:?}", id));
        }
    }
    Ok(())
}

/// The deck standard: a faction-pure, **singleton** deck of exactly `DECK_SIZE`
/// cards. Every seat builds to this regardless of faction — the deck-builder and
/// the match-builder both validate against it. This is the house rule;
/// [`validate_deck`] is the looser ≤2-copy check.
///
/// The **no-orphan-evolutions** constraint: a deck-playable evolution FORM
/// must be paired with **at least one base that reaches it** (a base whose `evolves_to`
/// names this form). You can never draw a form you can never land — so a deck holding an
/// orphan form is illegal, here and in the deck-builder.
pub fn validate_deck_for(
    deck: &[CardId],
    catalog: &[CardDef],
    faction: crate::types::Faction,
) -> Result<(), String> {
    if deck.len() != DECK_SIZE {
        return Err(format!(
            "deck must be {} cards, got {}",
            DECK_SIZE,
            deck.len()
        ));
    }
    let mut seen: std::collections::BTreeSet<CardId> = std::collections::BTreeSet::new();
    for id in deck {
        let Some(c) = catalog.iter().find(|c| c.id == *id) else {
            return Err(format!("unknown card id {:?}", id));
        };
        if !c.kind.deck_playable_for(faction) {
            return Err(format!("{} is not deck-playable for {:?}", c.name, faction));
        }
        if !seen.insert(*id) {
            return Err(format!("duplicate card {} — decks are singleton", c.name));
        }
    }
    // No orphan evolutions: every form in the deck needs a base in the deck that reaches it.
    let names_in_deck: std::collections::BTreeSet<&str> = deck
        .iter()
        .filter_map(|id| catalog.iter().find(|c| c.id == *id))
        .map(|c| c.name.as_str())
        .collect();
    for id in deck {
        let c = catalog.iter().find(|c| c.id == *id).unwrap();
        if c.kind == crate::types::CardKind::Evolution {
            // The Solace deepens — Primal forms ONLY, never Fabled (design §5).
            // A Solace deck may not hold a Fabled; its forms are all Deepenings.
            // (The base must be Solace too — but that already follows: a Lorekeeper
            // base is not `deck_playable_for(Solace)`, so an off-faction form would
            // fall to the orphan check below. This guard pins the rarity directly.)
            if faction == crate::types::Faction::Solace && c.rarity != "Primal" {
                return Err(format!(
                    "{} is a {} form — the Solace deepens to Primal only, never Fabled",
                    c.name, c.rarity
                ));
            }
            let has_base = catalog.iter().any(|b| {
                names_in_deck.contains(b.name.as_str()) && b.evolves_to.iter().any(|f| f == &c.name)
            });
            if !has_base {
                return Err(format!(
                    "{} is an orphan evolution — no base in the deck reaches it",
                    c.name
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod deck_standard_tests {
    use super::*;
    use crate::types::{CardKind, Faction};

    #[test]
    fn singleton_faction_pure_decks_validate() {
        let cat = canon_catalog();
        // 20 distinct Solace CREATURES/EVENTS = a legal Solace deck. We exclude the
        // Solace Deepenings (Evolution forms) here: a form alone is an orphan (it needs
        // a base in the deck to land), so a bare "first 20 kind-eligible" slice could pull
        // an unpaired form. The base↔form pairing is covered by the deck-gen + the
        // Solace Deepening suite; this test pins the plain singleton legality.
        let solace: Vec<CardId> = cat
            .iter()
            .filter(|c| c.kind.deck_playable_for(Faction::Solace) && c.kind != CardKind::Evolution)
            .take(DECK_SIZE)
            .map(|c| c.id)
            .collect();
        assert_eq!(
            solace.len(),
            DECK_SIZE,
            "the Solace creature/event pool covers a full deck"
        );
        assert!(validate_deck_for(&solace, &cat, Faction::Solace).is_ok());
        // The same cards are off-faction under Lorekeeper rules.
        assert!(validate_deck_for(&solace, &cat, Faction::Lorekeeper).is_err());
        // A duplicate breaks the singleton rule.
        let mut dup = solace.clone();
        dup[1] = dup[0];
        assert!(validate_deck_for(&dup, &cat, Faction::Solace).is_err());

        // And a Solace deck holding a base↔Deepening PAIR validates (the Deepening is
        // deck-playable, the pair is no-orphan), while a Fabled form is rejected for
        // the Solace (it deepens to Primal only).
        let mut paired = solace.clone();
        let spite = cat.iter().find(|c| c.name == "Spite").unwrap().id;
        let spite_form = cat
            .iter()
            .find(|c| c.name == "Spite, Made Whole")
            .unwrap()
            .id;
        // Ensure both are present and distinct from the slice's cards.
        paired.retain(|&id| id != spite && id != spite_form);
        paired.truncate(DECK_SIZE - 2);
        paired.push(spite);
        paired.push(spite_form);
        assert!(
            validate_deck_for(&paired, &cat, Faction::Solace).is_ok(),
            "a Solace deck with a base↔Deepening pair is legal"
        );
        // Swap the Primal Deepening for a Lorekeeper Fabled — rejected (Primal only).
        let fabled = cat
            .iter()
            .find(|c| c.kind == CardKind::Evolution && c.rarity == "Fabled")
            .unwrap()
            .id;
        let mut with_fabled = paired.clone();
        *with_fabled.last_mut().unwrap() = fabled;
        assert!(
            validate_deck_for(&with_fabled, &cat, Faction::Solace).is_err(),
            "a Fabled form is illegal in a Solace deck — the Solace deepens to Primal only"
        );
    }

    // ---- Boundary tests over a tiny local catalog, pinning every accept/reject edge of
    // BOTH validators precisely (size 19/20/21, the ≤2-copy singleton boundary, faction
    // purity, and the no-orphan-evolution pairing). Independent of canon drift. ----

    /// A small catalog: 22 plain Lorekeeper spirits (ids 0..22 for size sweeps), plus a
    /// base→form line (ids 100/101) and an off-faction Solace creature (id 200).
    fn mini() -> Vec<CardDef> {
        use crate::types::{Reach, Resonance};
        let spirit = |id: u16, name: &'static str| CardDef {
            id: CardId(id),
            name: name.into(),
            cost: 1,
            attack: 10,
            defense: 0,
            hp: 30,
            reach: Reach::Cross,
            resonance: Resonance::Harmony,
            kind: CardKind::Spirit,
            ..Default::default()
        };
        let mut v: Vec<CardDef> = (0..22u16)
            .map(|i| {
                // Distinct leaked names so each id is a unique card.
                spirit(i, Box::leak(format!("Spirit{i}").into_boxed_str()))
            })
            .collect();
        // A base→Primal line: the form (101) is an orphan unless the base (100) is present.
        let mut base = spirit(100, "Cub");
        base.evolves_to = vec!["Direwolf".into()];
        v.push(base);
        v.push(CardDef {
            id: CardId(101),
            name: "Direwolf".into(),
            cost: 2,
            attack: 40,
            defense: 10,
            hp: 50,
            reach: Reach::Cross,
            resonance: Resonance::Harmony,
            kind: CardKind::Evolution,
            rarity: "Primal".into(),
            evolves_from: Some("Cub".into()),
            ..Default::default()
        });
        // A Solace-only creature (not deck-playable for a Lorekeeper).
        v.push(CardDef {
            id: CardId(200),
            name: "Spite".into(),
            kind: CardKind::Unwritten,
            ..spirit(200, "Spite")
        });
        v
    }

    /// A singleton run of `n` distinct plain spirits (ids 0..n).
    fn distinct(n: u16) -> Vec<CardId> {
        (0..n).map(CardId).collect()
    }

    #[test]
    fn validate_deck_pins_the_size_boundary() {
        let cat = mini();
        // Exactly DECK_SIZE (20) is the only accepted length: 19 and 21 both reject.
        assert!(
            validate_deck(&distinct(20), &cat).is_ok(),
            "20 cards is legal"
        );
        assert!(
            validate_deck(&distinct(19), &cat).is_err(),
            "19 cards is too few"
        );
        assert!(
            validate_deck(&distinct(21), &cat).is_err(),
            "21 cards is too many"
        );
    }

    #[test]
    fn validate_deck_pins_the_two_copy_singleton_boundary() {
        let cat = mini();
        // 2 copies of a card is the MAX allowed; 3 is over. Build a 20-card deck that is
        // 2× id0 + 18 distinct others (legal), then push to 3× id0 (illegal).
        let mut two: Vec<CardId> = vec![CardId(0), CardId(0)];
        two.extend((1..19u16).map(CardId)); // 2 + 18 = 20
        assert_eq!(two.len(), 20);
        assert!(
            validate_deck(&two, &cat).is_ok(),
            "exactly 2 copies is the singleton boundary — allowed"
        );
        // 3× id0 + 17 distinct = 20 cards, but over the copy cap.
        let mut three: Vec<CardId> = vec![CardId(0), CardId(0), CardId(0)];
        three.extend((1..18u16).map(CardId)); // 3 + 17 = 20
        assert_eq!(three.len(), 20);
        let err = validate_deck(&three, &cat).unwrap_err();
        assert!(
            err.contains("more than 2 copies"),
            "3 copies of a card is rejected, got: {err}"
        );
    }

    #[test]
    fn validate_deck_rejects_an_unknown_card_id() {
        let cat = mini();
        // 19 known + 1 id that is not in the catalog.
        let mut deck = distinct(19);
        deck.push(CardId(9999));
        let err = validate_deck(&deck, &cat).unwrap_err();
        assert!(
            err.contains("unknown card id"),
            "an id absent from the catalog is rejected, got: {err}"
        );
    }

    #[test]
    fn validate_deck_for_pins_the_size_boundary() {
        use crate::types::Faction;
        let cat = mini();
        assert!(
            validate_deck_for(&distinct(20), &cat, Faction::Lorekeeper).is_ok(),
            "20 distinct Lorekeeper spirits is legal"
        );
        assert!(validate_deck_for(&distinct(19), &cat, Faction::Lorekeeper).is_err());
        assert!(validate_deck_for(&distinct(21), &cat, Faction::Lorekeeper).is_err());
    }

    #[test]
    fn validate_deck_for_is_strictly_singleton() {
        use crate::types::Faction;
        let cat = mini();
        // Unlike validate_deck (≤2), the deck STANDARD is singleton: even 2 copies reject.
        let mut two: Vec<CardId> = vec![CardId(0), CardId(0)];
        two.extend((1..19u16).map(CardId));
        let err = validate_deck_for(&two, &cat, Faction::Lorekeeper).unwrap_err();
        assert!(
            err.contains("singleton"),
            "the deck standard rejects even a 2nd copy, got: {err}"
        );
    }

    #[test]
    fn validate_deck_for_enforces_faction_purity() {
        use crate::types::Faction;
        let cat = mini();
        // 19 Lorekeeper spirits + the Solace creature (id 200): off-faction for a
        // Lorekeeper deck.
        let mut deck = distinct(19);
        deck.push(CardId(200));
        let err = validate_deck_for(&deck, &cat, Faction::Lorekeeper).unwrap_err();
        assert!(
            err.contains("not deck-playable"),
            "a Solace creature is off-faction for a Lorekeeper, got: {err}"
        );
    }

    #[test]
    fn validate_deck_for_rejects_an_orphan_evolution() {
        use crate::types::Faction;
        let cat = mini();
        // The form (101) WITHOUT its base (100): an orphan — no base in the deck reaches it.
        // 19 distinct plain spirits + the form = 20.
        let mut orphan = distinct(19);
        orphan.push(CardId(101));
        let err = validate_deck_for(&orphan, &cat, Faction::Lorekeeper).unwrap_err();
        assert!(
            err.contains("orphan evolution"),
            "a form with no base that reaches it is rejected, got: {err}"
        );
        // Add the base (and drop a filler to stay at 20): the pair now validates.
        let mut paired = distinct(18); // 18 plain
        paired.push(CardId(100)); // the base
        paired.push(CardId(101)); // its form
        assert_eq!(paired.len(), 20);
        assert!(
            validate_deck_for(&paired, &cat, Faction::Lorekeeper).is_ok(),
            "the base↔form pair is no longer an orphan"
        );
    }

    #[test]
    fn validate_deck_for_orphan_check_keys_on_the_exact_form_name() {
        use crate::types::Faction;
        // Pins the `b.evolves_to.iter().any(|f| f == &c.name)` name match (L265): a base
        // whose `evolves_to` names a DIFFERENT form does NOT satisfy the orphan check for
        // THIS form. We add a decoy base whose `evolves_to` is "Elsewhere" (not "Direwolf").
        let mut cat = mini();
        let decoy = CardDef {
            id: CardId(150),
            name: "Decoy".into(),
            kind: CardKind::Spirit,
            evolves_to: vec!["Elsewhere".into()],
            ..cat[0].clone()
        };
        cat.push(decoy);
        // 18 plain + the decoy base + the orphan form (101). The decoy reaches "Elsewhere",
        // NOT "Direwolf", so 101 is still an orphan.
        let mut deck = distinct(18);
        deck.push(CardId(150));
        deck.push(CardId(101));
        assert_eq!(deck.len(), 20);
        let err = validate_deck_for(&deck, &cat, Faction::Lorekeeper).unwrap_err();
        assert!(
            err.contains("orphan evolution"),
            "a base that reaches a DIFFERENT form does not satisfy this form, got: {err}"
        );
    }
}
