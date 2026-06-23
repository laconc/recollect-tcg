//! Uniform-seat: the Solace plays its faction deck like any seat — there is no
//! director. These guard the antagonist *mechanics*: an
//! Unwritten leaves no impression when it dissolves, persists through the orphan sweep,
//! and Lacuna denies a foothold. The director's *decision* logic (targeting, manifest
//! cadence, the difficulty cap) is gone, so those tests are retired with it. Page-Eater's
//! eat-on-move and White-Out's forgetting were exercised through the director's `lower()`;
//! they return as Phase-3 play-path tests once the Solace *plays* those cards (the OnMove /
//! OnPlay path that fires their effects), where they belong.
use recollect_core::Engine;
use recollect_core::state::{Command, Event};
use recollect_core::test_support::put_spirit;
use recollect_core::types::{CardDef, CardId, CardKind, Reach, Resonance, Seat};

/// A two-card catalog: a Narrator spirit (seat A) and an Unwritten (the Solace's creature).
fn solace_cat() -> Vec<CardDef> {
    let narrator = CardDef {
        id: CardId(0),
        name: "Narrator Spirit".into(),
        cost: 1,
        attack: 20,
        defense: 0,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Wonder,
        kind: CardKind::Spirit,
        ..Default::default()
    };
    let wolf = CardDef {
        id: CardId(1),
        name: "Unwritten Wolf".into(),
        cost: 0,
        attack: 40,
        defense: 20,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Neutral,
        kind: CardKind::Unwritten,
        ..Default::default()
    };
    vec![narrator, wolf]
}

/// Place the Solace's Unwritten (a token, seat B) at `tile`.
fn put_unwritten(e: &mut Engine, tile: u8) {
    put_spirit(e.state_mut_for_test(), tile, CardId(1), Seat::B);
    e.state_mut_for_test().board[tile as usize]
        .spirit
        .as_mut()
        .unwrap()
        .is_token = true;
}

#[test]
fn an_unwritten_leaves_no_impression_when_it_dissolves() {
    // Route B: an Unwritten is keyed by CardKind to leave NOTHING when it falls.
    let cat = solace_cat();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    put_unwritten(&mut e, 12);
    e.state_mut_for_test().board[12]
        .spirit
        .as_mut()
        .unwrap()
        .fading = true;
    // It is seat B's spirit; the Solace's fade step dissolves it.
    e.force_fade_step_for_test(Seat::B);
    assert!(
        e.state().board[12].spirit.is_none(),
        "the Unwritten is gone"
    );
    assert_eq!(
        e.state().board[12].impressions.first().copied(),
        None,
        "and it left NOTHING — no impression"
    );
}

#[test]
fn unwritten_persist_through_the_orphan_sweep() {
    // Route B: an Unwritten is a standalone creature, not a parentless summon-token, so the
    // orphan sweep (which runs at every end_turn) must NOT dissolve it — combat is the point.
    let cat = solace_cat();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    put_unwritten(&mut e, 12);
    assert!(
        e.state().board[12].spirit.as_ref().unwrap().is_token,
        "the Unwritten stands"
    );
    e.apply(Seat::A, Command::EndTurn).expect("A ends its turn");
    assert!(
        e.state().board[12]
            .spirit
            .as_ref()
            .map(|s| s.is_token)
            .unwrap_or(false),
        "the Unwritten persists through the orphan sweep — not a parentless summon-token"
    );
}

#[test]
fn lacuna_denies_a_foothold_impression() {
    // A Narrator spirit dissolving next to a standing Lacuna leaves NO impression — and because the
    // Solace denied the player a foothold where a spirit died, it SCORES (the erasure tally +1).
    // Keyed purely on the Lacuna card's presence — it denies wherever it stands.
    let cat = recollect_core::cards::canon_catalog();
    let lacuna = cat
        .iter()
        .find(|c| c.name == "Lacuna")
        .expect("Lacuna exists")
        .id;
    let narrator = cat
        .iter()
        .find(|c| c.kind == CardKind::Spirit && !c.lurk)
        .unwrap()
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    // Lacuna (seat B) at 11; a Narrator (seat A) at 12 (adjacent), Fading.
    put_spirit(e.state_mut_for_test(), 11, lacuna, Seat::B);
    put_spirit(e.state_mut_for_test(), 12, narrator, Seat::A);
    e.state_mut_for_test().board[12]
        .spirit
        .as_mut()
        .unwrap()
        .fading = true;
    e.force_fade_step_for_test(Seat::A);
    assert!(e.state().board[12].spirit.is_none(), "the spirit dissolved");
    assert_eq!(
        e.state().board[12].impressions.first().copied(),
        None,
        "Lacuna denied the foothold — no impression forms"
    );
    assert_eq!(
        e.state().solace_erasures,
        1,
        "denying the foothold on a dissolve scores for the Solace"
    );
}

#[test]
fn dusk_sweeps_unwritten_from_the_rim_immediately_while_player_spirits_hold() {
    // The Dusk is INSTANT (§0.5/§5): at the Curl, the Solace gets no Held Ground — an
    // Unwritten on the rim dissolves AT ONCE in the contraction (the tile darkens and the
    // Unwritten is gone, leaving nothing — no body, no impression), NOT set fading to wait
    // for a turn-start Fade (there is no turn-start Fade anymore). A player's standing rim
    // spirit lingers (keeps its tile, keeps scoring). Tiles 0 and 4 are rim.
    let cat = solace_cat();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    put_unwritten(&mut e, 0);
    put_spirit(e.state_mut_for_test(), 4, CardId(0), Seat::A);
    {
        let st = e.state_mut_for_test();
        st.round = st.rules.contraction_after; // the Curl fires as seat B ends this round
        st.active = Seat::B;
        st.player_b.hand.clear(); // no hand-cap release intercepts EndTurn
    }
    let evs = e.apply(Seat::B, Command::EndTurn).unwrap();
    // The contraction fired and the Unwritten is GONE the instant it did — not waiting.
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::MemoryContracted { .. })),
        "the Curl fired (MemoryContracted)"
    );
    assert!(
        e.state().board[0].faded && e.state().board[0].spirit.is_none(),
        "the Unwritten's rim tile faded AND the Unwritten dissolved at once (no fading body waits)"
    );
    assert!(
        e.state().board[0].impressions.is_empty(),
        "the Unwritten leaves nothing — no impression on the swept tile"
    );
    assert!(
        !e.state().board[4].faded
            && e.state().board[4]
                .spirit
                .as_ref()
                .is_some_and(|s| !s.fading),
        "the player's rim spirit holds its ground"
    );
    // It is now A's turn (the round wrapped); the swept tile stays empty — nothing
    // dissolved at A's turn-start, because the sweep already happened instantly.
    assert_eq!(e.state().active, Seat::A);
    assert!(
        e.state().board[0].spirit.is_none(),
        "still empty at A's turn-start — the Dusk did not defer a fade"
    );
}
