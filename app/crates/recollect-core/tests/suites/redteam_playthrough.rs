//! ALL-CARDS red-team — INTERACTION / sequencing / illegal-state bugs
//! that only surface in actual play (the class per-card outcome tests miss). Each
//! test is a minimal repro distilled from a full-catalog random-playthrough fuzz
//! (the harness builds canon decks for 1v1, 1v1-vs-Solace, and 2v2 and checks the
//! `invariants::check` suite + redaction after every legal command). The design
//! doc is law: every fix here makes the engine match `docs/design.md`.
use crate::common::*;
use recollect_core::Seat;
use recollect_core::invariants::check as check_invariants;
use recollect_core::state::{Command, Event, Terrain, TerrainKind};
use recollect_core::types::*;

/// §4: "A spirit may be placed on any **empty** tile." A tile holding terrain (a
/// Landmark or a revealed Fabrication) is NOT empty — placing a spirit onto it
/// creates the illegal `spirit AND terrain coexist` state (invariants.rs #1).
///
/// Bug: `decide_play_spirit` rejected only `t.spirit.is_some()`, never
/// `t.terrain.is_some()` — unlike `decide_place_landmark`/`decide_set_fabrication`,
/// which both reject a terrain-occupied tile. A tile adjacent to your own Landmark
/// is in your projection, so playing a spirit onto a *second* adjacent Landmark
/// (or any terrain tile) slipped through. `legal_commands` offered it, too.
#[test]
fn a_spirit_cannot_be_played_onto_a_tile_holding_terrain() {
    let mut st = blank();
    // A owns a spirit at (2,2) (projects), and a Landmark sits on the adjacent
    // tile (2,3) — that tile is in A's projection but is NOT empty.
    put(&mut st, t(2, 2), 0, Seat::A, None);
    let land_tile = t(2, 3);
    st.board[land_tile as usize].terrain = Some(Terrain {
        card: CardId(1),
        owner: Seat::A,
        kind: TerrainKind::Landmark,
        face_down: false,
    });
    hand(&mut st, Seat::A, &[0]); // a cheap spirit to play
    let mut e = eng(st, 1);

    // The direct command must be rejected (the tile is not empty).
    let rej = e.apply(
        Seat::A,
        Command::PlaySpirit {
            hand_index: 0,
            tile: land_tile,
            engage: None,
            chain_prefs: vec![],
        },
    );
    assert!(
        rej.is_err(),
        "playing a spirit onto a terrain tile must be rejected — got {rej:?}"
    );

    // …and `legal_commands` must never OFFER it.
    let offered = e
        .legal_commands(Seat::A)
        .into_iter()
        .any(|c| matches!(c, Command::PlaySpirit { tile, .. } if tile == land_tile));
    assert!(
        !offered,
        "legal_commands offered a placement onto a terrain tile (illegal)"
    );

    // The state is still well-formed (nothing slipped through).
    check_invariants(e.state()).unwrap();
}

/// §5 (the Dusk) + invariants.rs #2 ("no terrain on a faded tile"). At the Curl,
/// every EMPTY rim tile darkens. A rim tile holding only terrain (a Landmark or a
/// face-down Fabrication — no spirit) is empty of any *held* body, so it darkens —
/// but the `MemoryContracted` apply darkened the tile WITHOUT removing the terrain,
/// leaving the illegal `terrain on a faded tile` state. The lie/landmark must be
/// swept into the dark with the ground it sat on.
#[test]
fn the_dusk_sweeps_terrain_off_a_darkening_rim_tile() {
    let mut st = blank();
    st.round = 8; // the round whose end triggers the contraction
    st.active = Seat::B; // B's turn-end runs the wrap (contraction_after = 8)
    // A Landmark and a face-down Fabrication, each alone on an empty rim tile.
    let land_rim = t(0, 0); // a corner — rim
    let fab_rim = t(4, 0); // another rim tile
    assert!(is_rim_w(land_rim, st.board_w) && is_rim_w(fab_rim, st.board_w));
    st.board[land_rim as usize].terrain = Some(Terrain {
        card: CardId(1),
        owner: Seat::A,
        kind: TerrainKind::Landmark,
        face_down: false,
    });
    st.board[fab_rim as usize].terrain = Some(Terrain {
        card: CardId(2),
        owner: Seat::A,
        kind: TerrainKind::Fabrication,
        face_down: true,
    });
    let mut e = eng(st, 1);
    e.apply(Seat::B, Command::EndTurn).unwrap();

    // The contraction fired and both rim tiles darkened…
    assert!(
        e.state().contracted,
        "the Curl should have contracted the rim"
    );
    assert!(e.state().board[land_rim as usize].faded);
    assert!(e.state().board[fab_rim as usize].faded);
    // …and the terrain was swept (a faded tile can hold no terrain).
    assert!(
        e.state().board[land_rim as usize].terrain.is_none(),
        "a Landmark on a darkening rim tile must be swept by the Dusk"
    );
    assert!(
        e.state().board[fab_rim as usize].terrain.is_none(),
        "a Fabrication on a darkening rim tile must be swept by the Dusk"
    );
    check_invariants(e.state()).unwrap();
}

/// §10 ("Kindred … dissolve to no impression") + invariants.rs #5 (a token leaves
/// no impression). A KINDRED token banished in combat cannot evolve and leaves no
/// mark — so it must dissolve AT ONCE, with no impression, never entering the
/// standing-Faded window.
///
/// Bug: `banish_or_replace` stamped `SpiritBecameFading { banished_by: Some }` on
/// EVERY defeated spirit, so a banished token (a) recorded a banisher — the illegal
/// `a token recorded a banisher` state — and (b) at its turn-END `dissolve_faded_at`
/// would lay the banisher's impression, wrongly scoring the tile for the foe. (An
/// UNWRITTEN token is excepted: it CAN be Primal-Deepened from that state, and its
/// own dissolve still leaves nothing.)
#[test]
fn a_banished_kindred_token_leaves_no_impression_and_no_illegal_state() {
    // A plays a fresh body that ENGAGES (the combat path through `banish_or_replace`)
    // B's low-HP KINDRED token and fells it. The token must dissolve at once
    // (TokenDissolved), never recording a banisher, never entering the standing-Faded
    // window, and leaving NO impression on its tile.
    let mut st = blank();
    // A owns a spirit at (2,1) so the play tile (2,2) sits in projection.
    put(&mut st, t(2, 1), 0, Seat::A, None);
    // B's KINDRED token at (2,3): def 0, 5 HP — a single Dawnling strike (10 atk) fells it.
    put(&mut st, t(2, 3), 8, Seat::B, Some(5));
    {
        let tok = st.board[t(2, 3) as usize].spirit.as_mut().unwrap();
        tok.is_token = true;
        tok.defense = 0;
        tok.attack = 0; // no retaliation, so the arriver surely survives to witness
    }
    hand(&mut st, Seat::A, &[0]); // a Dawnling (Cross reach) to play at (2,2) and engage (2,3)
    let mut e = eng(st, 1);
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: t(2, 2),
                engage: Some(t(2, 3)),
                chain_prefs: vec![],
            },
        )
        .expect("the arrival engage resolves");

    // The token never became a standing-Faded base (no banisher recorded on it)…
    assert!(
        !evs.iter().any(|ev| matches!(
            ev,
            Event::SpiritBecameFading { tile, banished_by: Some(_) } if *tile == t(2, 3)
        )),
        "a banished Kindred must not enter the standing-Faded window: {evs:?}"
    );
    // …it dissolved via the no-impression token path (TokenDissolved — the shared
    // ephemeral-body event for Kindred AND Unwritten)…
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::TokenDissolved { tile } if *tile == t(2, 3))),
        "the defeated Kindred dissolved immediately, no impression: {evs:?}"
    );
    // …and its tile is left EMPTY with no impression (Kindred leave no mark, §10).
    let tile = &e.state().board[t(2, 3) as usize];
    assert!(tile.spirit.is_none(), "the token is gone");
    assert!(
        tile.impressions.is_empty(),
        "a banished Kindred leaves NO impression (§10)"
    );
    check_invariants(e.state()).unwrap();
}

/// §11 ("the Unwritten leave nothing … a player who banishes an Unwritten gets
/// nothing — it dissolves leaving no mark of having been") at NIGHTFALL. An
/// Unwritten banished in combat on the FINAL round (12) lingers standing-Faded
/// through the rest of the round and is dissolved by the `finish` pass before
/// scoring (§0.5). That final dissolve must leave NOTHING — no impression for the
/// banisher — exactly like the turn-END `dissolve_faded_at` does on rounds 1–11.
///
/// Bug: `finish` dissolved EVERY remaining fading spirit with a bare
/// `SpiritDissolved { impression = banished_by }`, and `lay_mark` lays the
/// banisher's board impression — so a Lorekeeper who banished a Solace Unwritten on
/// round 12 wrongly SCORED its tile (a free point the design forbids). The
/// turn-end path special-cased Unwritten (leave nothing); the Nightfall pass did not.
#[test]
fn a_round_12_banished_unwritten_leaves_no_impression_at_nightfall() {
    use recollect_core::Engine;
    use recollect_core::cards::canon_catalog;
    let cat = canon_catalog();
    // A real Solace creature (an Unwritten) by kind, and a Lorekeeper spirit for A.
    let unwritten = cat
        .iter()
        .find(|c| c.kind == CardKind::Unwritten)
        .expect("an Unwritten exists")
        .id;
    let a_spirit = cat
        .iter()
        .find(|c| c.kind == CardKind::Spirit && !c.lurk)
        .expect("a plain spirit exists")
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| a_spirit).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let uw_tile = 6u8; // an inner tile (not rim) so the Dusk never swept it
    let a_tile = 18u8; // A's own scoring spirit, elsewhere
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Lorekeeper, Faction::Solace];
        st.round = 12; // the final round
        st.active = Seat::B; // B's turn-end runs the Nightfall finish
        st.contracted = true; // post-Dusk (rounds 9–12)
        // B's Unwritten (a token), banished by A and standing-Faded into round 12. Its
        // deadline is 13 (as if banished on B's own turn) so it is NOT due at B's
        // round-12 turn-END fade — it survives to the Nightfall `finish` pass, exercising
        // the §0.5 final-round lingering path directly.
        recollect_core::test_support::put_spirit(st, uw_tile, unwritten, Seat::B);
        {
            let sp = st.board[uw_tile as usize].spirit.as_mut().unwrap();
            sp.is_token = true;
            sp.fading = true;
            sp.banished_by = Some(Seat::A);
            sp.fade_deadline = Some(13);
        }
        // A's standing spirit holds one tile (its real point).
        recollect_core::test_support::put_spirit(st, a_tile, a_spirit, Seat::A);
        st.player_a.deck.clear();
        st.player_b.deck.clear();
    }
    let evs = e
        .apply(Seat::B, Command::EndTurn)
        .expect("Nightfall resolves");

    // The Unwritten dissolved leaving NOTHING — no impression on its tile.
    let uw = &e.state().board[uw_tile as usize];
    assert!(uw.spirit.is_none(), "the round-12 Unwritten dissolved");
    assert!(
        uw.impressions.is_empty(),
        "a banished Unwritten leaves NO impression, even at Nightfall (§11): {:?}",
        uw.impressions
    );
    // The final score must NOT credit A for the Unwritten's tile (A scores only its
    // own standing spirit). The MatchEnded score_a counts A's real tiles only.
    let ended = evs.iter().find_map(|ev| match ev {
        Event::MatchEnded { score_a, .. } => Some(*score_a),
        _ => None,
    });
    assert_eq!(
        ended,
        Some(1),
        "A scores ONLY its own standing spirit (1) — never the banished Unwritten's tile"
    );
    check_invariants(e.state()).unwrap();
}

/// §11/§217 — the **forward** direction of "the Unwritten leave nothing" (the reverse
/// being that banishing an Unwritten leaves nothing). When the Solace banishes a
/// **player** spirit, the standing-Faded window closes with **NO mark** (the Unwritten
/// leave nothing where they erase) and the Solace banks **+1** on its off-board erasure
/// tally — that off-board +1 is how erasing a foothold scores. This drives the FULL
/// `dissolve_faded_at` path (the turn-END Fade), not the bare `SpiritDissolved` event:
/// the dissolving spirit is a Lorekeeper (so it is NOT the is_unwritten branch), its
/// banisher is the Solace, and `lay_mark` — keyed on the BANISHER's faction — must clear
/// the tile and tally instead of stamping a (Solace-colored, wrongly-scoring) board mark.
#[test]
fn the_solace_banishing_a_player_spirit_lays_no_mark_and_tallies_off_board() {
    use recollect_core::Engine;
    use recollect_core::cards::canon_catalog;
    let cat = canon_catalog();
    // A real Lorekeeper spirit for A (the victim) — a plain, non-lurking spirit.
    let a_spirit = cat
        .iter()
        .find(|c| c.kind == CardKind::Spirit && !c.lurk)
        .expect("a plain spirit exists")
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| a_spirit).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let victim_tile = 12u8; // inner tile, off the rim (no Dusk interaction)
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Lorekeeper, Faction::Solace];
        st.round = 4;
        st.active = Seat::A; // the victim's owner; its turn-END runs the Fade
        // A's spirit, banished IN COMBAT by the Solace (seat B), standing-Faded with a due
        // deadline so this very turn-END dissolves it (the window closes now).
        recollect_core::test_support::put_spirit(st, victim_tile, a_spirit, Seat::A);
        {
            let sp = st.board[victim_tile as usize].spirit.as_mut().unwrap();
            sp.fading = true;
            sp.banished_by = Some(Seat::B); // the Solace felled it
            sp.fade_deadline = Some(st.round); // due at this turn-END
        }
        st.solace_erasures = 0;
        st.player_a.deck.clear();
        st.player_b.deck.clear();
    }
    let evs = e
        .apply(Seat::A, Command::EndTurn)
        .expect("the Fade resolves the standing-Faded victim");

    // The dissolve laid NO board impression — the Unwritten leave nothing where they erase.
    let tile = &e.state().board[victim_tile as usize];
    assert!(tile.spirit.is_none(), "the victim dissolved");
    assert!(
        tile.impressions.is_empty(),
        "the Solace's banish leaves NO mark on the tile (§11): {:?}",
        tile.impressions
    );
    // …and the erasure is banked off-board: the Solace's tally went +1.
    assert_eq!(
        e.state().solace_erasures,
        1,
        "the Solace banks +1 on its off-board erasure tally for the banish (§11/§217)"
    );
    // The dissolve rode the normal SpiritDissolved (impression = the Solace seat B) — NOT
    // TokenDissolved (that is the Unwritten's OWN dissolve) — and `lay_mark`'s faction
    // branch is what turned B's "impression" into the no-mark tally.
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::SpiritDissolved { tile, impression: Seat::B } if *tile == victim_tile
        )),
        "the player spirit dissolves via SpiritDissolved with the Solace as banisher: {evs:?}"
    );
    check_invariants(e.state()).unwrap();
}

/// The CONTROL for the test above: a player spirit banished by a **Lorekeeper** (the PvP
/// default) lays a normal scoring impression and never touches the erasure tally — the
/// no-mark-plus-tally behavior is the Solace's asymmetry alone, keyed on the banisher's
/// faction. (Kills a mutant that makes `dissolve_faded_at` clear+tally unconditionally.)
#[test]
fn a_lorekeeper_banishing_a_player_spirit_lays_a_scoring_mark_and_no_tally() {
    use recollect_core::Engine;
    use recollect_core::cards::canon_catalog;
    let cat = canon_catalog();
    let a_spirit = cat
        .iter()
        .find(|c| c.kind == CardKind::Spirit && !c.lurk)
        .expect("a plain spirit exists")
        .id;
    let deck: Vec<CardId> = (0..20).map(|_| a_spirit).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    let victim_tile = 12u8;
    {
        let st = e.state_mut_for_test();
        // BOTH seats Lorekeeper (the PvP default — no Solace anywhere).
        st.rules.factions = [Faction::Lorekeeper, Faction::Lorekeeper];
        st.round = 4;
        st.active = Seat::A;
        recollect_core::test_support::put_spirit(st, victim_tile, a_spirit, Seat::A);
        {
            let sp = st.board[victim_tile as usize].spirit.as_mut().unwrap();
            sp.fading = true;
            sp.banished_by = Some(Seat::B); // a Lorekeeper opponent felled it
            sp.fade_deadline = Some(st.round);
        }
        st.solace_erasures = 0;
        st.player_a.deck.clear();
        st.player_b.deck.clear();
    }
    e.apply(Seat::A, Command::EndTurn)
        .expect("the Fade resolves");
    // The banisher's scoring impression sits on the tile, and the tally is untouched.
    assert_eq!(
        e.state().board[victim_tile as usize]
            .impressions
            .first()
            .copied(),
        Some(Seat::B),
        "a Lorekeeper banish leaves its scoring impression (the PvP default)"
    );
    assert_eq!(
        e.state().solace_erasures,
        0,
        "no erasure tally for a Lorekeeper banish — the asymmetry is the Solace's alone"
    );
    check_invariants(e.state()).unwrap();
}

/// Invariant #1 (a tile never holds both a spirit and terrain): a Kindred manifests
/// only on an EMPTY tile — never onto one already holding terrain (a Landmark /
/// Fabrication). `summon_kindred` chose the lowest adjacent tile that was unfaded and
/// spirit-free but did NOT check terrain, so a caller flanked by terrain could mint a
/// token onto a Landmark, creating the illegal coexistence.
#[test]
fn a_kindred_never_manifests_onto_a_terrain_tile() {
    // Build around the canon "Choirmother Lark" caller (summons "Hum"), with every
    // adjacent tile but one holding terrain — the token must land on the lone open one.
    use recollect_core::Engine;
    use recollect_core::cards::canon_catalog;
    let cat = canon_catalog();
    let lark = cat
        .iter()
        .find(|c| c.name == "Choirmother Lark")
        .map(|c| c.id);
    let Some(lark) = lark else {
        return; // caller not in catalog — nothing to assert
    };
    let deck: Vec<CardId> = (0..20).map(|_| lark).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    let center = t(2, 2);
    {
        let st = e.state_mut_for_test();
        st.player_a.hand = vec![lark];
        st.player_a.anima = 20;
        st.player_a.first_placement_done = true;
        // Block three of the four tiles adjacent to where the caller will sit with
        // terrain; leave one open. The caller is placed at `center`; its neighbours are
        // (1,2),(3,2),(2,1),(2,3). Block all but (2,3).
        for blocked in [t(1, 2), t(3, 2), t(2, 1)] {
            st.board[blocked as usize].terrain = Some(Terrain {
                card: CardId(0),
                owner: Seat::A,
                kind: TerrainKind::Landmark,
                face_down: false,
            });
        }
        // Give A a spirit so `center` is in projection.
        put(st, t(2, 0), 0, Seat::A, None);
    }
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: center,
                engage: None,
                chain_prefs: vec![],
            },
        )
        .expect("the caller is played");
    // If it manifested a Kindred, that token must be on the open tile (2,3), never on a
    // terrain tile — and the state is well-formed regardless.
    for ev in &evs {
        if let Event::SpiritManifested { tile, .. } = ev {
            assert!(
                e.state().board[*tile as usize].terrain.is_none(),
                "a Kindred manifested onto a terrain tile (illegal coexistence)"
            );
        }
    }
    check_invariants(e.state()).unwrap();
}

/// §2 (Overwrite) + the Bond-break law ("a Bond breaks if either spirit has left").
///
/// An Overwrite that DEFEATS its target seats the overwriter on that tile. A Bond is a
/// **same-owner** construct (`AttachBond` requires both endpoints be your own standing
/// spirits), yet bonds were only pruned at the next Flow — so for the rest of the turn the
/// banished occupant's Bond still pointed at the tile the *enemy* overwriter now stood on.
/// When Momentum then chained off that tile into the bond's partner, the partner's
/// **Promise** (`Replace(PartnerTakesIt)`) redirected the lethal blow back to its
/// "partner" — the OVERWRITER itself — driving it to negative HP while it kept standing
/// (no banish, because `full_exchange`'s attacker-banish check read the pre-exchange HP
/// snapshot, blind to the redirect that had just wounded the same tile). That is the
/// invariant break the 3h soak found: `canon-1v1 seed 894494 step 117` left a standing
/// spirit at hp -15.
///
/// The fix breaks the stale bond the instant the Overwrite takes the tile
/// (`prune_broken_bonds`, ownership-aware), so the dead occupant's Promise can never reach
/// across the stolen tile; `full_exchange` also now reads live HP for the attacker banish.
/// Hand-built so it pins the interaction, not a 117-step seed.
#[test]
fn an_overwrite_breaks_the_defeated_occupants_bond_before_momentum_chains() {
    use recollect_core::Engine;
    use recollect_core::cards::canon_catalog;
    use recollect_core::state::{Bond, Spirit};
    let cat = canon_catalog();
    // Canon ids: Promise (Bond, Replace=PartnerTakesIt) and Strider of the Far Blue
    // (atk45/def20/hp65, Cross reach) — the overwriter. The bonded enemies wear a Dawnling's
    // CardDef (Wonder, non-arcane, so no resonance edge skews the math) at overridden stats.
    let id_of = |name: &str| cat.iter().find(|c| c.name == name).expect(name).id;
    let promise = id_of("Promise");
    let strider = id_of("Strider of the Far Blue");
    let dawnling = id_of("Dawnling");

    let (mut e, _) = Engine::new(894_494, cat, vec![strider; 20], vec![dawnling; 20]);
    // Width-5 1v1 geometry: T=(2,2) is the Overwrite target; P=(2,3) is its bonded partner
    // (manhattan 1); A's projector sits at (2,1) so T is inside A's projection.
    let t_target = t(2, 2);
    let t_partner = t(2, 3);
    let t_proj = t(2, 1);
    // A custom-stat standing spirit (lets us set HP/Attack and pre-empt the partner's arrival
    // interception, isolating the Momentum→Promise path).
    let spirit =
        |card: CardId, owner: Seat, atk: i16, def: i16, hp: i16, intercepted: bool| Spirit {
            replacement_used: false,
            holding: false,
            face_down: false,
            is_token: false,
            placed_by: None,
            card,
            owner,
            attack: atk,
            defense: def,
            hp,
            hp_max: hp.max(65),
            fading: false,
            banished_by: None,
            intercepted_this_round: intercepted,
            traits_stripped: false,
            traits_stripped_until: None,
            kw_grants: Vec::new(),
            no_engage_until: 0,
            throughline_done: false,
            copied_reach: None,
            fade_deadline: None,
        };
    {
        let st = e.state_mut_for_test();
        // The occupant of T: a near-dead enemy with no bite — Strider's 45 atk fells it
        // (success) and takes 0 back, so the overwriter arrives at its FULL 65 HP.
        st.board[t_target as usize].spirit = Some(spirit(dawnling, Seat::B, 5, 0, 5, false));
        // The bonded partner at P. The Momentum chain (Strider 45 + 10 link − 0 def = 55)
        // is lethal to its 50 HP, so the Promise fires; its 35 atk retaliates for 15 (− Strider's
        // 20 def). 55 redirect onto the overwriter (arrived 65) leaves it at 10 — alive, so the
        // redirect's own banish does NOT fire — then the 15 retaliation drives it to −5. Before
        // the fix the attacker-banish read the stale pre-exchange 65 (65 − 15 = 50 > 0), so it
        // kept STANDING at −5: exactly the soak's `hp -15 <= 0` invariant break. `intercepted`
        // so P does not also bite the arrival (keeps the arithmetic about the redirect alone).
        st.board[t_partner as usize].spirit = Some(spirit(dawnling, Seat::B, 35, 0, 50, true));
        // A's projector (its Cross reach covers T) — present only to make the Overwrite legal.
        st.board[t_proj as usize].spirit = Some(spirit(strider, Seat::A, 45, 20, 65, true));
        // The same-owner (B) Promise bonding T↔P. Before the fix this outlived the occupant —
        // so the dead occupant's Promise redirected the chain blow onto the enemy overwriter.
        st.bonds.push(Bond {
            card: promise,
            owner: Seat::B,
            tile_a: t_target,
            tile_b: t_partner,
        });
        st.player_a.hand = vec![strider];
        st.player_a.anima = 20;
        st.player_a.first_placement_done = true;
        st.player_a.deck.clear();
        st.player_b.deck.clear();
    }

    let evs = e
        .apply(
            Seat::A,
            Command::Overwrite {
                hand_index: 0,
                tile: t_target,
            },
        )
        .expect("the Overwrite is legal");

    // The invariant the soak tripped: no standing (non-fading) spirit may sit at ≤0 HP.
    check_invariants(e.state()).expect("no standing spirit ends at ≤0 HP");

    // The overwriter took T and STANDS with positive HP — never wounded by its own dead
    // victim's Promise (the stale bond is gone, so the chain blow could not boomerang).
    let occ = e.state().board[t_target as usize]
        .spirit
        .as_ref()
        .expect("the overwriter holds the tile");
    assert_eq!(occ.owner, Seat::A, "A's overwriter took the tile");
    assert!(
        !occ.fading && occ.hp > 0,
        "the overwriter stands with positive HP, not a fading/negative body: hp={}",
        occ.hp
    );

    // The stale enemy Bond broke the instant the tile changed hands — before Momentum.
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::BondBroken { card } if *card == promise)),
        "the defeated occupant's Bond must break on the Overwrite, not at the next Flow"
    );
    assert!(
        e.state().bonds.is_empty(),
        "no stale bond may linger pointing at the stolen tile"
    );
    // No DamageRedirected onto the overwriter's tile (the Promise can't reach across the
    // stolen tile once the bond is broken).
    assert!(
        !evs.iter().any(
            |ev| matches!(ev, Event::DamageRedirected { to_tile, .. } if *to_tile == t_target)
        ),
        "a broken bond must not redirect the chain blow back onto the overwriter"
    );
}

/// Companion to the Overwrite-bond fix (commit 90b911b): `full_exchange`'s attacker-banish
/// now reads the LIVE board HP, not the pre-exchange `att.hp - dmg_att` snapshot. This pins
/// that the change did **not** regress the legitimate "wounded but survives" case where an
/// `OnDefeat` self-heal lifts the attacker mid-exchange — the heal lands BEFORE the
/// retaliation, so the live read must see the healed-then-struck HP and let the spirit STAND.
///
/// Ashcub (Fury 45/10/40, `OnDefeat → RestoreForm 10`) is pre-wounded to 15 HP, then engages a
/// Neutral-bodied target it fells (no resonance edge muddies the math): the engage snapshot is
/// 15 − 10 = 5 > 0, so OnDefeat fires and heals +10 (→ 25); the 10 retaliation then drops it to
/// a LIVE 15. The old snapshot (`15 − 10`) and the live read (`25 − 10`) agree it STANDS here —
/// but the heal is observable (final HP 15, not the un-healed 5), and a live read that wrongly
/// banished a ≤0 attacker would never reach 15. Hand-built so it pins the live-HP path.
#[test]
fn the_live_hp_attacker_banish_lets_an_ondefeat_self_heal_keep_the_attacker_standing() {
    use recollect_core::Engine;
    use recollect_core::cards::canon_catalog;
    let cat = canon_catalog();
    let id_of = |name: &str| cat.iter().find(|c| c.name == name).expect(name).id;
    let ashcub = id_of("Ashcub"); // Fury 45/10/40, OnDefeat → RestoreForm 10
    let latchling = id_of("Latchling"); // Neutral body → no wheel edge against Fury

    let (mut e, _) = Engine::new(7, cat, vec![ashcub; 20], vec![latchling; 20]);
    let att_tile = t(2, 1);
    let def_tile = t(2, 2);
    {
        let st = e.state_mut_for_test();
        // Ashcub at its printed 45 atk / 10 def, hp_max 40, pre-wounded to 15 (so a +10 heal
        // adds HP, un-capped). `put_spirit` seeds fixed stats — set Ashcub's explicitly.
        recollect_core::test_support::put_spirit(st, att_tile, ashcub, Seat::A);
        {
            let a = st.board[att_tile as usize].spirit.as_mut().unwrap();
            a.attack = 45;
            a.defense = 10;
            a.hp = 15;
            a.hp_max = 40;
        }
        // The target: a Neutral-bodied enemy at 0 def / 5 HP (Ashcub's 45 fells it), 20 atk
        // (its retaliation is 20 − Ashcub's 10 def = exactly 10).
        recollect_core::test_support::put_spirit(st, def_tile, latchling, Seat::B);
        let tgt = st.board[def_tile as usize].spirit.as_mut().unwrap();
        tgt.attack = 20;
        tgt.defense = 0;
        tgt.hp = 5;
        tgt.hp_max = 5;
    }
    let evs = e.resolve_engage_for_test(att_tile, def_tile);

    // OnDefeat fired: the self-heal landed (the snapshot gated it in, 5 > 0).
    assert!(
        evs.iter().any(
            |ev| matches!(ev, Event::EffectRestored { tile, amount: 10 } if *tile == att_tile)
        ),
        "Ashcub's OnDefeat RestoreForm(10) fired (snapshot 5 > 0 gated it in): {evs:?}"
    );
    // The attacker STANDS at the LIVE healed-then-struck HP (15 + 10 − 10 = 15), never banished.
    let att = e
        .state()
        .spirit_at(att_tile)
        .expect("the healed attacker stands");
    assert!(
        !att.fading,
        "the live-HP read must NOT banish a spirit the OnDefeat heal kept above 0"
    );
    assert_eq!(
        att.hp, 15,
        "live HP = 15 (wounded) + 10 (heal, pre-retaliation) − 10 (retaliation); the un-healed \
         path would read 5, and a ≤0 misread would have banished — both are wrong"
    );
    check_invariants(e.state()).unwrap();
}

/// The ownership-aware `prune_broken_bonds` (commit 90b911b) breaks a bond the instant an
/// endpoint stops holding a STANDING spirit owned by the bond owner — not just on a banish, but
/// on **any owner-flip**, e.g. The Perfect Lie taking control of the engaging spirit. A bonded
/// Mobile spirit that walks into the trap is seized by the enemy: its bond now spans an
/// enemy-held tile. The eager Overwrite-time prune does NOT run on the trap path — but the
/// stale cross-owner bond is harmless within the turn (no redirect/Momentum reaches it after
/// `spring_fabrication` returns) and is pruned at the next Flow (`start_turn`), BEFORE the new
/// active seat acts. This pins that no standing-≤0 / boomerang state survives the control-take,
/// and the bond is gone by the time the controller can engage.
#[test]
fn a_take_control_trap_on_a_bonded_spirit_leaves_no_stale_redirect() {
    use recollect_core::Engine;
    use recollect_core::cards::canon_catalog;
    use recollect_core::state::{Bond, Spirit, Terrain, TerrainKind};
    let cat = canon_catalog();
    let id_of = |name: &str| cat.iter().find(|c| c.name == name).expect(name).id;
    let perfect_lie = id_of("The Perfect Lie"); // Fabrication trap: OnReveal → TakeControl (cost ≤3)
    let promise = id_of("Promise"); // Bond: Replace(PartnerTakesIt)
    let latchling = id_of("Latchling"); // Neutral body, cost ≤3 (so the trap seizes it)

    let (mut e, _) = Engine::new(7, cat.clone(), vec![latchling; 20], vec![latchling; 20]);
    // B owns a bonded pair (mover X at (2,2)↔partner Y at (2,3)). A owns the trap on (2,1) and a
    // projector. X is Mobile + steps onto the trap tile's engage — but the trap takes (2,1)…
    let x_from = t(2, 2);
    let partner = t(2, 3);
    let trap_tile = t(2, 1);
    let spirit = |card: CardId, owner: Seat, intercepted: bool| Spirit {
        replacement_used: false,
        holding: false,
        face_down: false,
        is_token: false,
        placed_by: None,
        card,
        owner,
        attack: 20,
        defense: 0,
        hp: 40,
        hp_max: 40,
        fading: false,
        banished_by: None,
        intercepted_this_round: intercepted,
        traits_stripped: false,
        traits_stripped_until: None,
        kw_grants: Vec::new(),
        no_engage_until: 0,
        throughline_done: false,
        copied_reach: None,
        fade_deadline: None,
    };
    {
        let st = e.state_mut_for_test();
        st.active = Seat::B;
        // B's Mobile mover X and its bonded partner Y (a same-owner Promise bond X↔Y).
        st.board[x_from as usize].spirit = Some(spirit(latchling, Seat::B, false));
        st.board[partner as usize].spirit = Some(spirit(latchling, Seat::B, true));
        // Grant X the Mobile keyword (this round and onward) so it can step into the trap.
        let until = st.round + 1;
        st.board[x_from as usize]
            .spirit
            .as_mut()
            .unwrap()
            .kw_grants
            .push((recollect_core::effects::Keyword::Mobile, until));
        st.bonds.push(Bond {
            card: promise,
            owner: Seat::B,
            tile_a: x_from,
            tile_b: partner,
        });
        // A's face-down trap on the tile X will step onto.
        st.board[trap_tile as usize].terrain = Some(Terrain {
            card: perfect_lie,
            owner: Seat::A,
            kind: TerrainKind::Fabrication,
            face_down: true,
        });
        st.player_b.anima = 20;
        st.player_b.first_placement_done = true;
        st.player_b.deck.clear();
        st.player_a.deck.clear();
    }
    // X steps onto the trap tile — springing it. The trap takes control of X (it is the engager).
    let evs = e
        .apply(
            Seat::B,
            Command::MoveSpirit {
                from: x_from,
                to: trap_tile,
                engage: None,
            },
        )
        .expect("the Mobile step springs the trap");
    // The trap seized X: it is now A's, on `x_from` (the mover never left for the trap tile — a
    // sprung enemy trap fires on the engager where it stands; control flips its owner).
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::ControlTaken {
                new_owner: Seat::A,
                ..
            }
        )),
        "The Perfect Lie took control of the bonded mover: {evs:?}"
    );
    // No standing-≤0 body, no boomerang redirect anywhere in the resolution.
    check_invariants(e.state()).unwrap();
    assert!(
        !evs.iter()
            .any(|ev| matches!(ev, Event::DamageRedirected { .. })),
        "the control-take must not let the stale bond redirect within the turn: {evs:?}"
    );
    // The bond now spans an enemy-held tile (X is A's). It is pruned at B's-opponent's next
    // Flow — i.e. before the controller (A) can act on it. Drive A's turn-start.
    e.apply(Seat::B, Command::EndTurn)
        .expect("B ends; A's Flow runs start_turn → prune_broken_bonds");
    assert!(
        e.state().bonds.iter().all(|b| b.card != promise),
        "the stale cross-owner bond is pruned by the Flow, before the controller acts"
    );
    check_invariants(e.state()).unwrap();
}
