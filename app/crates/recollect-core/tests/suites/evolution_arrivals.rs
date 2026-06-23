//! Behavioral proof for the evolution-form effects authored this tranche, against
//! the same executor the spellbook uses (Stormswell / Warden of the Glade carry
//! the identical clause shapes). Each test places the form and triggers its
//! arrival or reads its aura directly — no evolve scaffolding needed.
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::engine::combat_stats_for_test;
use recollect_core::state::{Event, PendingChoice, Terrain, TerrainKind};
use recollect_core::test_support::put_spirit;
use recollect_core::types::{CardId, Seat};

/// Place an enemy (seat B) face-down Fabrication at `tile`.
fn enemy_fab(e: &mut Engine, tile: u8) {
    e.state_mut_for_test().board[tile as usize].terrain = Some(Terrain {
        card: CardId(0),
        owner: Seat::B,
        kind: TerrainKind::Fabrication,
        face_down: true,
    });
}

fn id_of(name: &str) -> CardId {
    canon_catalog()
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.id)
        .unwrap_or_else(|| panic!("card not found: {name}"))
}

fn engine() -> Engine {
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    Engine::new(7, cat.clone(), deck.clone(), deck).0
}

/// Verdure, Overrunning — "On arrival: deal 10 to all adjacent enemies."
#[test]
fn verdure_arrival_damages_all_adjacent_enemies() {
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Verdure, Overrunning"), Seat::A);
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // adjacent enemy (left)
        put_spirit(st, 7, id_of("Cloudling"), Seat::B); // adjacent enemy (up)
    }
    let evs = e.fire_arrival_effects_for_test(12, Seat::A);
    for t in [11u8, 7u8] {
        assert!(
            evs.iter().any(|ev| matches!(
                ev,
                Event::EffectDamaged { tile, amount } if *tile == t && *amount == 10
            )),
            "Verdure should deal 10 to the adjacent enemy at {t} (events: {evs:?})"
        );
    }
}

/// Colossus of the Kept Promise — "Adjacent allies +10 Defense" (a derived-on-read
/// static aura, so no arrival needed). Measured as a delta: the same ally's combat
/// defense with Colossus adjacent minus without is exactly +10.
#[test]
fn colossus_grants_adjacent_allies_defense() {
    let cat = canon_catalog();

    let mut alone = engine();
    put_spirit(alone.state_mut_for_test(), 11, id_of("Cloudling"), Seat::A);
    let def_alone = combat_stats_for_test(alone.state(), &cat, 11).defense;

    let mut buffed = engine();
    {
        let st = buffed.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // adjacent ally
        put_spirit(st, 12, id_of("Colossus of the Kept Promise"), Seat::A);
    }
    let def_buffed = combat_stats_for_test(buffed.state(), &cat, 11).defense;

    assert_eq!(
        def_buffed,
        def_alone + 10,
        "Colossus gives adjacent allies +10 Defense"
    );
}

/// Cirrus, First of Rains — "On arrival: reveal all enemy Fabrications."
#[test]
fn cirrus_arrival_reveals_all_enemy_fabrications() {
    let mut e = engine();
    put_spirit(
        e.state_mut_for_test(),
        12,
        id_of("Cirrus, First of Rains"),
        Seat::A,
    );
    enemy_fab(&mut e, 7);
    enemy_fab(&mut e, 20);
    let evs = e.fire_arrival_effects_for_test(12, Seat::A);
    for t in [7u8, 20u8] {
        assert!(
            evs.iter()
                .any(|ev| matches!(ev, Event::FabricationRevealed { tile } if *tile == t)),
            "Cirrus should reveal the enemy Fabrication at {t} (events: {evs:?})"
        );
        assert!(
            !e.state().board[t as usize]
                .terrain
                .as_ref()
                .unwrap()
                .face_down,
            "fabrication at {t} is now face-up"
        );
    }
}

/// Zenith, Who Asks the Sky — "On arrival: reveal all Fabrications." This is the
/// deck-playable card whose reveal was decorative until `RevealFabrication` was
/// wired; the test pins that it now actually reveals.
#[test]
fn zenith_arrival_reveals_enemy_fabrications() {
    let mut e = engine();
    put_spirit(
        e.state_mut_for_test(),
        12,
        id_of("Zenith, Who Asks the Sky"),
        Seat::A,
    );
    enemy_fab(&mut e, 7);
    let evs = e.fire_arrival_effects_for_test(12, Seat::A);
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::FabricationRevealed { tile } if *tile == 7)),
        "Zenith should reveal the enemy Fabrication (events: {evs:?})"
    );
}

/// Standard-Bearer of the Burn — "Allied Flame spirits +10 Attack" (a tribal static
/// aura over allies carrying the Flame imprint, e.g. Cinderling).
#[test]
fn standard_bearer_buffs_flame_allies_attack() {
    let cat = canon_catalog();
    let mut alone = engine();
    put_spirit(alone.state_mut_for_test(), 7, id_of("Cinderling"), Seat::A);
    let atk_alone = combat_stats_for_test(alone.state(), &cat, 7).attack;

    let mut buffed = engine();
    {
        let st = buffed.state_mut_for_test();
        put_spirit(st, 7, id_of("Cinderling"), Seat::A); // Flame-imprint ally
        put_spirit(st, 12, id_of("Standard-Bearer of the Burn"), Seat::A);
    }
    let atk_buffed = combat_stats_for_test(buffed.state(), &cat, 7).attack;
    assert_eq!(
        atk_buffed,
        atk_alone + 10,
        "Standard-Bearer gives Flame allies +10 Attack"
    );
}

/// Felis Umbra, the Borrowed — "Lurk · Face-down: adjacent enemies −10 Attack" (a
/// static aura gated on being face-down, debuffing enemies within one tile).
#[test]
fn felis_umbra_face_down_weakens_adjacent_enemies() {
    let cat = canon_catalog();
    let mut without = engine();
    put_spirit(
        without.state_mut_for_test(),
        11,
        id_of("Cloudling"),
        Seat::B,
    );
    let atk_without = combat_stats_for_test(without.state(), &cat, 11).attack;

    let mut with = engine();
    {
        let st = with.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // enemy of Felis (seat A)
        put_spirit(st, 12, id_of("Felis Umbra, the Borrowed"), Seat::A);
        st.board[12].spirit.as_mut().unwrap().face_down = true; // Lurk: face-down
    }
    let atk_with = combat_stats_for_test(with.state(), &cat, 11).attack;
    assert_eq!(
        atk_with,
        atk_without - 10,
        "Felis Umbra (face-down) gives adjacent enemies -10 Attack"
    );
}

/// Zenith's "Your Glimpses take +1 card" (the GlimpseLooksOneMore rule exception):
/// Cloudling's Glimpse looks at 2 alone, 3 with Zenith on the board.
#[test]
fn zenith_makes_glimpses_look_one_more() {
    fn looked_count(with_zenith: bool) -> usize {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 12, id_of("Cloudling"), Seat::A); // Glimpse: look 2, take 1
            if with_zenith {
                put_spirit(st, 7, id_of("Zenith, Who Asks the Sky"), Seat::A);
            }
        }
        let evs = e.fire_arrival_effects_for_test(12, Seat::A);
        evs.iter()
            .find_map(|ev| match ev {
                Event::ChoiceOffered {
                    choice: PendingChoice::Peek { looked, .. },
                } => Some(looked.len()),
                _ => None,
            })
            .expect("a Peek choice was offered")
    }
    assert_eq!(looked_count(false), 2, "Cloudling alone looks at 2");
    assert_eq!(looked_count(true), 3, "with Zenith present, looks at 2+1");
}
