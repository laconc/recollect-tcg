//! Render-contract tests. The TUI and web shell render a PlayerView /
//! TeamView; these pin EVERY field a renderer reads, so a view refactor that
//! drops one fails in CI, not in front of a player. We construct a board that
//! exercises the states renderers must show: a spirit, an impression, a Landmark, a
//! face-down Fabrication (ours and the enemy's, to check redaction), and an
//! evolvable Fading base (so the evolution lineage appears).
use recollect_core::Engine;
use recollect_core::state::{Terrain, TerrainKind};
use recollect_core::types::{CardId, CardKind, Seat};
use recollect_core::view::view_for;

#[test]
fn player_view_json_carries_every_field_the_renderers_read() {
    let cat = recollect_core::cards::canon_catalog();
    let deck: Vec<CardId> = cat
        .iter()
        .filter(|c| matches!(c.kind, CardKind::Spirit))
        .take(20)
        .map(|c| c.id)
        .collect();
    let (mut e, _) = Engine::new(1, cat.clone(), deck.clone(), deck);
    // Place a spirit (exercises SpiritView + evolutions if evolvable).
    let cmd = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, recollect_core::Command::PlaySpirit { engage: None, .. }))
        .unwrap();
    e.apply(Seat::A, cmd).unwrap();
    // Put a Landmark and a Fabrication on the board so terrain renders.
    let landmark = cat
        .iter()
        .find(|c| c.kind == CardKind::Landmark)
        .unwrap()
        .id;
    let fab = cat
        .iter()
        .find(|c| c.kind == CardKind::Fabrication)
        .unwrap()
        .id;
    let st = e.state_mut_for_test();
    st.board[20].terrain = Some(Terrain {
        card: landmark,
        owner: Seat::A,
        kind: TerrainKind::Landmark,
        face_down: false,
    });
    st.board[21].terrain = Some(Terrain {
        card: fab,
        owner: Seat::B,
        kind: TerrainKind::Fabrication,
        face_down: true,
    }); // enemy lie → redacted
    let json = serde_json::to_string(&view_for(&e, Seat::A)).unwrap();
    // Every field a renderer needs, including the formerly-missing terrain +
    // the evolution lineage. If any is dropped from the view, this fails.
    for field in [
        "\"round\"",
        "\"active\"",
        "\"phase\"",
        "\"tiles\"",
        "\"spirit\"",
        "\"attack\"",
        "\"defense\"",
        "\"hp\"",
        "\"hp_max\"",
        "\"fading\"",
        "\"echo\"",
        "\"mobile\"",
        "\"face_down\"",
        "\"evolutions\"",
        "\"impression\"",
        "\"faded\"",
        "\"in_your_projection\"",
        "\"terrain\"",
        "\"hand\"",
        "\"anima\"",
        "\"deck_count\"",
        "\"solace_erasures\"",
        "\"moved_this_turn\"",
    ] {
        assert!(
            json.contains(field),
            "render contract: PlayerView JSON missing {field}"
        );
    }
}

#[test]
fn enemy_face_down_terrain_is_redacted_in_the_view() {
    let cat = recollect_core::cards::canon_catalog();
    let fab = cat
        .iter()
        .find(|c| c.kind == CardKind::Fabrication)
        .unwrap()
        .id;
    let deck: Vec<CardId> = cat
        .iter()
        .filter(|c| matches!(c.kind, CardKind::Spirit))
        .take(20)
        .map(|c| c.id)
        .collect();
    let (mut e, _) = Engine::new(1, cat, deck.clone(), deck);
    e.state_mut_for_test().board[21].terrain = Some(Terrain {
        card: fab,
        owner: Seat::B,
        kind: TerrainKind::Fabrication,
        face_down: true,
    });
    let v = view_for(&e, Seat::A);
    let tv = v.tiles[21]
        .terrain
        .as_ref()
        .expect("enemy terrain is visible AS a lie");
    assert!(tv.face_down, "shown as a face-down lie");
    assert_eq!(tv.card, CardId(u16::MAX), "but its identity is redacted");
    assert_eq!(tv.kind, "Fabrication");
}

#[test]
fn your_own_terrain_shows_its_identity() {
    let cat = recollect_core::cards::canon_catalog();
    let lm = cat
        .iter()
        .find(|c| c.kind == CardKind::Landmark)
        .unwrap()
        .id;
    let deck: Vec<CardId> = cat
        .iter()
        .filter(|c| matches!(c.kind, CardKind::Spirit))
        .take(20)
        .map(|c| c.id)
        .collect();
    let (mut e, _) = Engine::new(1, cat, deck.clone(), deck);
    e.state_mut_for_test().board[20].terrain = Some(Terrain {
        card: lm,
        owner: Seat::A,
        kind: TerrainKind::Landmark,
        face_down: false,
    });
    let v = view_for(&e, Seat::A);
    let tv = v.tiles[20]
        .terrain
        .as_ref()
        .expect("your Landmark is visible");
    assert_eq!(tv.card, lm, "your own terrain shows its real identity");
    assert!(!tv.face_down);
}
