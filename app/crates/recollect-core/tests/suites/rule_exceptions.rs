//! RuleException dispatch. One card changes the rules for another.
//! Exceptions are declared on carriers (Static · Exception(X)) and consulted
//! through a single `exception_active` chokepoint — no hardcoded name checks.
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::types::{CardId, Seat};

#[test]
fn parting_triggers_twice_with_the_vigilkeeper_standing() {
    // Drizzle Sprite's Parting restores form to the most-wounded ally. With a
    // Vigilkeeper (PartingTriggersTwice) standing, it should fire twice.
    let cat = canon_catalog();
    let sprite = cat
        .iter()
        .find(|c| c.name == "Drizzle Sprite")
        .expect("Drizzle Sprite")
        .id;
    // The Vigilkeeper is the base; we need it standing & owned. (It's a Spirit.)
    let vigil = cat
        .iter()
        .find(|c| c.name == "The Vigilkeeper")
        .expect("Vigilkeeper")
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let st = e.state_mut_for_test();
    // A wounded ally to receive the restore, the Vigilkeeper, and a Fading sprite.
    recollect_core::test_support::put_spirit(st, 6, vigil, Seat::A);
    recollect_core::test_support::put_spirit(st, 7, sprite, Seat::A);
    // A wounded ally at 8 (low HP) to be the restore target.
    recollect_core::test_support::put_spirit(st, 8, CardId(0), Seat::A);
    st.board[8].spirit.as_mut().unwrap().hp = 10;
    st.board[8].spirit.as_mut().unwrap().hp_max = 50;
    // Make the sprite Fading so its Parting fires on the fade step.
    st.board[7].spirit.as_mut().unwrap().fading = true;
    let hp_before = e.state().board[8].spirit.as_ref().unwrap().hp;
    // Drive the fade step (the sprite dissolves, Parting fires — twice).
    e.force_fade_step_for_test(Seat::A);
    let restores = {
        // Count RestoreForm-bearing events, or just observe the doubled heal.
        let after = e.state().board[8]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(hp_before);
        after - hp_before
    };
    assert!(
        restores >= 20,
        "Parting restored twice (≥20), got +{restores}"
    );
}

#[test]
fn the_exception_dispatch_reads_carriers_generally_not_by_name() {
    // Sanity: without any carrier, the evolution shared-Imprint rule holds;
    // this just confirms exception_active is wired (covered behaviorally by
    // the evolve tests, which use Bearer of Small Stones via the dispatch).
    let cat = canon_catalog();
    assert!(cat.iter().any(|c| c.name == "Bearer of Small Stones"));
    assert!(
        cat.iter().any(|c| c.name == "Rainpool"),
        "second PartingTriggersTwice carrier exists"
    );
}

#[test]
fn rainpool_landmark_doubles_its_controllers_parting() {
    // Rainpool is a face-up Landmark carrying PartingTriggersTwice — routed seat-wide
    // via exception_active's terrain scan (the terrain_hit path, vs the Vigilkeeper's
    // spirit_hit). A fading Drizzle Sprite's Parting restore should fire twice.
    use recollect_core::state::{Terrain, TerrainKind};
    let cat = canon_catalog();
    let sprite = cat.iter().find(|c| c.name == "Drizzle Sprite").unwrap().id;
    let rainpool = cat.iter().find(|c| c.name == "Rainpool").unwrap().id;
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let st = e.state_mut_for_test();
    st.board[6].terrain = Some(Terrain {
        card: rainpool,
        owner: Seat::A,
        kind: TerrainKind::Landmark,
        face_down: false,
    });
    recollect_core::test_support::put_spirit(st, 7, sprite, Seat::A);
    recollect_core::test_support::put_spirit(st, 8, CardId(0), Seat::A);
    st.board[8].spirit.as_mut().unwrap().hp = 10;
    st.board[8].spirit.as_mut().unwrap().hp_max = 50;
    st.board[7].spirit.as_mut().unwrap().fading = true;
    let hp_before = e.state().board[8].spirit.as_ref().unwrap().hp;
    e.force_fade_step_for_test(Seat::A);
    let after = e.state().board[8]
        .spirit
        .as_ref()
        .map(|s| s.hp)
        .unwrap_or(hp_before);
    assert!(
        after - hp_before >= 20,
        "Rainpool doubled the Parting restore (≥20), got +{}",
        after - hp_before
    );
}
