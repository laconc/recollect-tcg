//! Wire-format measurements on OUR actual types — a real mid-game view and a
//! real event batch — proving postcard round-trips them losslessly through the
//! same serde derives, and sizing it against the JSON wire.
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::generate_deck;
use recollect_core::view::view_for;
use recollect_core::{Engine, Seat};

fn mid_game() -> (Engine, Vec<recollect_core::Event>) {
    let cat = canon_catalog();
    let (mut e, _) = Engine::new(
        9,
        cat.clone(),
        generate_deck(0, 9, &cat),
        generate_deck(1, 10, &cat),
    );
    let mut batch = Vec::new();
    for _ in 0..30 {
        let seat = e.state().active;
        let cmd = e.legal_commands(seat).first().unwrap().clone();
        batch = e.apply(seat, cmd).unwrap();
    }
    (e, batch)
}

#[test]
fn postcard_roundtrips_views_and_events_and_is_small() {
    let (e, batch) = mid_game();
    let view = view_for(&e, Seat::A);

    let vj = serde_json::to_vec(&view).unwrap();
    let vp = postcard::to_allocvec(&view).unwrap();
    let bj = serde_json::to_vec(&batch).unwrap();
    let bp = postcard::to_allocvec(&batch).unwrap();

    let view2: recollect_core::view::PlayerView = postcard::from_bytes(&vp).unwrap();
    assert_eq!(
        serde_json::to_vec(&view2).unwrap(),
        vj,
        "lossless through the same derives"
    );
    let batch2: Vec<recollect_core::Event> = postcard::from_bytes(&bp).unwrap();
    assert_eq!(batch2, batch);

    eprintln!(
        "WIRE SIZES — PlayerView: json {} B, postcard {} B ({:.1}x)",
        vj.len(),
        vp.len(),
        vj.len() as f64 / vp.len() as f64
    );
    eprintln!(
        "WIRE SIZES — event batch ({} events): json {} B, postcard {} B ({:.1}x)",
        batch.len(),
        bj.len(),
        bp.len(),
        bj.len() as f64 / bp.len() as f64
    );
    assert!(
        vp.len() * 3 < vj.len(),
        "postcard should be at least 3x smaller on views"
    );
}

#[test]
fn postcard_bytes_are_deterministic_per_schema_version() {
    let (e, batch) = mid_game();
    let a = postcard::to_allocvec(&view_for(&e, Seat::B)).unwrap();
    let b = postcard::to_allocvec(&view_for(&e, Seat::B)).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        postcard::to_allocvec(&batch).unwrap(),
        postcard::to_allocvec(&batch).unwrap()
    );
}
