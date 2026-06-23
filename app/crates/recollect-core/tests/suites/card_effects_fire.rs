//! Card-effect execution coverage. The ratchet proves every card is
//! *authored*; this proves every authored ON-ARRIVAL / ON-REVEAL effect
//! actually *fires* — emits at least one event or makes an observable change
//! when triggered in a constructed scenario. It catches the silent-no-op bug
//! class: a card with a spec whose selector resolves to nothing or whose
//! effect the engine drops on the floor.
//!
//! Static auras and choice-routed targets are exercised elsewhere (combat_stats
//! derivation, effects_choices.rs); this focuses on the instant triggers that
//! should produce events directly.
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::effects::canon_effects;
use recollect_core::types::{CardId, CardKind, Seat};

/// Build a 1v1 engine and force a spirit of `card` onto an inner tile with a
/// couple of enemies adjacent, so on-arrival/on-reveal effects have something
/// to target. Returns the engine and the tile the card sits on.
fn scenario(card: CardId, cat: &[recollect_core::types::CardDef]) -> (Engine, u8) {
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(1, cat.to_vec(), deck.clone(), deck);
    let st = e.state_mut_for_test();
    // Card at tile 12 (center). Give it: an enemy at 11 (adjacent target), a
    // FADING ally at 13 (heal/RestoreForm target), and leave 7 and 17 OPEN so
    // Callers have an empty adjacent tile to summon onto. This single scenario
    // satisfies the common selector needs without over-crowding the board.
    recollect_core::test_support::put_spirit(st, 12, card, Seat::A);
    let filler = cat
        .iter()
        .find(|c| c.kind == CardKind::Spirit && c.id != card)
        .map(|c| c.id)
        .unwrap_or(CardId(0));
    recollect_core::test_support::put_spirit(st, 11, filler, Seat::B);
    recollect_core::test_support::put_spirit(st, 13, filler, Seat::A);
    if let Some(sp) = st.board[13].spirit.as_mut() {
        sp.fading = true;
        sp.hp = 10;
    }
    // An ENEMY (Seat::B) impression on an empty tile, so impression-targeting arrivals
    // (The Long Erasure: Owner/ImpressionRemoveTarget — eat one enemy mark) have a
    // target to open a choice over. tiles 7 and 17 (the other neighbors of 12) stay open.
    st.board[7].impressions = vec![Seat::B];
    (e, 12)
}

#[test]
fn every_on_arrival_and_on_reveal_effect_actually_fires() {
    let cat = canon_catalog();
    let ef = canon_effects();
    // The instant triggers whose specs SHOULD produce an observable event when
    // fired with targets present. (Static = derived auras, exercised by combat
    // tests; choice triggers route through PendingChoice, tested separately.)
    let instant_triggers = ["OnPlay", "OnReveal"];
    let mut fired = 0;
    let mut silent: Vec<String> = Vec::new();

    for (key, specs) in ef.specs.iter() {
        // effects.json is keyed by the stable card `key`; resolve to the card by key.
        // Only spirits/callers/evolutions arrive on a tile and fire OnPlay/OnReveal.
        let Some(def) = cat.iter().find(|c| &c.key == key) else {
            continue;
        };
        if !matches!(
            def.kind,
            CardKind::Spirit
                | CardKind::Caller
                | CardKind::Evolution
                | CardKind::Unwritten
                | CardKind::IllIntent
        ) {
            continue;
        }
        // Does this card have an instant trigger that should emit something?
        let has_instant = specs.iter().any(|s| {
            instant_triggers.contains(&format!("{:?}", s.trigger).as_str())
                // Only unconditional, Instant-duration clauses should emit an
                // event immediately. Deferred (ThisRound) auras register a temp
                // modifier silently; conditional (PayForm) effects opt in;
                // choice/exception/reveal clauses route elsewhere.
                && format!("{:?}", s.condition) == "Always"
                && s.clauses.iter().any(|c| {
                    let e = format!("{:?}", c.effect);
                    let d = format!("{:?}", c.duration);
                    d == "Instant"
                        && !e.contains("NoEffect") && !e.contains("Exception")
                        && !e.contains("Reveal") && !e.contains("CostDelta")
                        && !e.contains("ReachDelta") && !e.contains("GrantKeyword")
                        && !e.contains("ExtraEngage") && !e.contains("GrantEngage")
                })
        });
        if !has_instant {
            continue;
        }

        let (mut e, tile) = scenario(def.id, &cat);
        // Fire the card's OnPlay via the test hook (re-fire arrival effects).
        let before_len = serde_json::to_string(e.state()).unwrap().len();
        let evs = e.fire_arrival_effects_for_test(tile, Seat::A);
        let after_len = serde_json::to_string(e.state()).unwrap().len();
        if evs.is_empty() && before_len == after_len {
            silent.push(def.name.clone());
        } else {
            fired += 1;
        }
    }
    assert!(
        silent.is_empty(),
        "{} authored on-arrival/reveal effects fired nothing (silent no-op): {:?}",
        silent.len(),
        silent
    );
    assert!(
        fired >= 25,
        "the probe should exercise many cards, only fired {fired}"
    );
}
