//! The 2v2 engine — 6×6 board, four players A1→B1→A2→B2, two teams
//! sharing projection and score.
use recollect_core::Engine;
use recollect_core::state::{Command, Phase};
use recollect_core::types::{
    BOARD_TILES_2V2, CardDef, CardId, CardKind, Reach, Resonance, Seat, SeatSlot,
};

fn spirit(id: u16, name: &str) -> CardDef {
    CardDef {
        id: CardId(id),
        name: name.into(),
        cost: 1,
        attack: 20,
        defense: 0,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Wonder,
        kind: CardKind::Spirit,
        ..Default::default()
    }
}

fn four_decks() -> [Vec<CardId>; 4] {
    // Each player gets a distinct card id so we can tell the hands apart.
    [
        (0..20).map(|_| CardId(0)).collect(),
        (0..20).map(|_| CardId(1)).collect(),
        (0..20).map(|_| CardId(2)).collect(),
        (0..20).map(|_| CardId(3)).collect(),
    ]
}

#[test]
fn a_2v2_match_is_six_by_six_with_four_distinct_hands() {
    let cat = vec![
        spirit(0, "A1"),
        spirit(1, "B1"),
        spirit(2, "A2"),
        spirit(3, "B2"),
    ];
    let (e, _) = Engine::new_2v2(7, cat, four_decks());
    assert_eq!(e.state().board.len(), BOARD_TILES_2V2, "6×6 = 36 tiles");
    assert_eq!(e.state().board_w, 6);
    assert!(e.state().is_2v2());
    // Each slot holds its own card id (separate hands, not a shared team hand).
    assert!(
        e.state()
            .player_slot(SeatSlot::A1)
            .hand
            .iter()
            .all(|c| c.0 == 0)
    );
    assert!(
        e.state()
            .player_slot(SeatSlot::A2)
            .hand
            .iter()
            .all(|c| c.0 == 2)
    );
    assert_ne!(
        e.state().player_slot(SeatSlot::A1).hand,
        e.state().player_slot(SeatSlot::A2).hand,
        "teammates do not share a hand"
    );
}

#[test]
fn turns_rotate_a1_b1_a2_b2() {
    let cat = vec![
        spirit(0, "A1"),
        spirit(1, "B1"),
        spirit(2, "A2"),
        spirit(3, "B2"),
    ];
    let (mut e, _) = Engine::new_2v2(7, cat, four_decks());
    let order = [
        SeatSlot::A1,
        SeatSlot::B1,
        SeatSlot::A2,
        SeatSlot::B2,
        SeatSlot::A1,
    ];
    for win in order.windows(2) {
        assert_eq!(e.state().active_slot, win[0]);
        assert_eq!(
            e.state().active,
            win[0].team(),
            "active team tracks the slot"
        );
        let seat = e.state().active;
        // End the turn (release first if the hand cap demands it).
        if matches!(e.state().phase, Phase::PendingRelease { .. }) {
            e.apply(seat, Command::Release { hand_index: 0 }).unwrap();
        }
        e.apply(seat, Command::EndTurn).unwrap();
        assert_eq!(e.state().active_slot, win[1], "rotation A1→B1→A2→B2→A1");
    }
}

#[test]
fn a1_plays_from_its_own_hand_and_the_board_grows_to_36() {
    let cat = vec![
        spirit(0, "A1"),
        spirit(1, "B1"),
        spirit(2, "A2"),
        spirit(3, "B2"),
    ];
    let (mut e, _) = Engine::new_2v2(7, cat, four_decks());
    // A1 places a spirit on a legal (home-row) tile.
    let cmd = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::PlaySpirit { engage: None, .. }))
        .expect("A1 has a legal placement");
    let tile = match cmd {
        Command::PlaySpirit { tile, .. } => tile,
        _ => 0,
    };
    e.apply(Seat::A, cmd).unwrap();
    let sp = e.state().board[tile as usize]
        .spirit
        .as_ref()
        .expect("A1's spirit stands");
    assert_eq!(sp.card, CardId(0), "it came from A1's hand");
    assert_eq!(sp.owner, Seat::A, "owned by team A");
    // A1's hand shrank by the placement; A2's is untouched (separate hands).
    let a1 = e.state().player_slot(SeatSlot::A1).hand.len();
    let a2 = e.state().player_slot(SeatSlot::A2).hand.len();
    assert_eq!(
        a1, 5,
        "6 at turn start (5 dealt + Flow draw), 5 after one play"
    );
    assert_eq!(a2, 5, "A2's hand is untouched by A1's turn");
}

#[test]
fn dominion_scoring_sums_both_teammates_on_the_six_by_six() {
    let cat = vec![
        spirit(0, "A1"),
        spirit(1, "B1"),
        spirit(2, "A2"),
        spirit(3, "B2"),
    ];
    let (mut e, _) = Engine::new_2v2(7, cat, four_decks());
    // Drop spirits for both A-team players and one B; team A should out-score.
    let st = e.state_mut_for_test();
    recollect_core::test_support::put_spirit(st, 0, CardId(0), Seat::A); // A1's
    recollect_core::test_support::put_spirit(st, 35, CardId(2), Seat::A); // A2's
    recollect_core::test_support::put_spirit(st, 18, CardId(1), Seat::B);
    st.board[6].impressions = vec![Seat::A]; // a team-A impression counts too
    // Tally the way finish() does, by team.
    let (mut a, mut b) = (0, 0);
    for tile in e.state().board.iter() {
        match tile
            .spirit
            .as_ref()
            .map(|s| s.owner)
            .or(tile.impressions.first().copied())
        {
            Some(Seat::A) => a += 1,
            Some(Seat::B) => b += 1,
            None => {}
        }
    }
    assert_eq!(
        (a, b),
        (3, 1),
        "both A teammates + an A impression vs one B spirit"
    );
}

#[test]
fn each_seat_sees_only_its_own_hand_in_the_four_seat_view() {
    use recollect_core::view::view_for_slot;
    let cat = vec![
        spirit(0, "A1"),
        spirit(1, "B1"),
        spirit(2, "A2"),
        spirit(3, "B2"),
    ];
    let (e, _) = Engine::new_2v2(7, cat, four_decks());
    let a1 = view_for_slot(&e, SeatSlot::A1);
    // A1 sees its own hand (all CardId(0)), and only counts for everyone else.
    assert!(
        a1.you.hand.iter().all(|c| c.0 == 0),
        "A1's own hand is visible"
    );
    assert_eq!(a1.team, Seat::A);
    assert_eq!(a1.board_w, 6);
    assert_eq!(
        a1.teammate.hand_count, 5,
        "teammate A2 is a count, not cards"
    );
    assert_eq!(
        a1.opponents.len(),
        2,
        "both rival slots are visible as counts"
    );
    // A2's view shows A2's distinct hand (CardId(2)).
    let a2 = view_for_slot(&e, SeatSlot::A2);
    assert!(
        a2.you.hand.iter().all(|c| c.0 == 2),
        "A2 sees ITS hand, not A1's"
    );
    assert_ne!(a1.you.hand, a2.you.hand, "teammates' private hands differ");
    // The opponents' identities are public counts but never card lists — the
    // TeamView simply has no field that could leak an opponent's hand.
    assert_eq!(a1.opponents[0].hand_count, 5);
}

// --- 2v2 rulings: own-projection Overwrite, untargetable partners ---

#[test]
fn f25_overwrite_uses_your_own_projection_not_your_partners() {
    use recollect_core::engine::{projection, projection_slot};
    let cat = vec![
        spirit(0, "A1"),
        spirit(1, "B1"),
        spirit(2, "A2"),
        spirit(3, "B2"),
    ];
    let (mut e, _) = Engine::new_2v2(7, cat, four_decks());
    let st = e.state_mut_for_test();
    // A2 places a spirit at tile 14 (its reach projects around it). A1 placed nothing.
    recollect_core::test_support::put_spirit(st, 14, CardId(2), Seat::A);
    st.board[14].spirit.as_mut().unwrap().placed_by = Some(SeatSlot::A2);
    // The TEAM projection includes A2's reach; A1's OWN projection does not.
    let team = projection(
        e.state(),
        Seat::A,
        &[
            spirit(0, "A1"),
            spirit(1, "B1"),
            spirit(2, "A2"),
            spirit(3, "B2"),
        ],
    );
    let a1 = projection_slot(
        e.state(),
        SeatSlot::A1,
        &[
            spirit(0, "A1"),
            spirit(1, "B1"),
            spirit(2, "A2"),
            spirit(3, "B2"),
        ],
    );
    let a2 = projection_slot(
        e.state(),
        SeatSlot::A2,
        &[
            spirit(0, "A1"),
            spirit(1, "B1"),
            spirit(2, "A2"),
            spirit(3, "B2"),
        ],
    );
    // A tile that A2's spirit reaches (one of its oriented targets) is in the team
    // proj and A2's own proj, but NOT in A1's own proj (A1 placed nothing there).
    let reached: Vec<usize> = (0..36)
        .filter(|&i| a2[i] && e.state().board[i].spirit.is_none())
        .collect();
    let only_via_a2 = reached.into_iter().find(|&i| team[i] && !a1[i]);
    assert!(
        only_via_a2.is_some(),
        "there is a tile A2's reach covers that A1 may NOT Overwrite into"
    );
}

#[test]
fn f30_partners_are_untargetable() {
    let cat = vec![
        spirit(0, "A1"),
        spirit(1, "B1"),
        spirit(2, "A2"),
        spirit(3, "B2"),
    ];
    let (mut e, _) = Engine::new_2v2(7, cat, four_decks());
    let st = e.state_mut_for_test();
    // A2's spirit stands at 14; it is A1's TEAMMATE (same team A).
    recollect_core::test_support::put_spirit(st, 14, CardId(2), Seat::A);
    st.board[14].spirit.as_mut().unwrap().placed_by = Some(SeatSlot::A2);
    // A1 (active) cannot Overwrite the teammate's tile — it's not an enemy.
    let legal = e.legal_commands(Seat::A);
    let targets_teammate = legal
        .iter()
        .any(|c| matches!(c, Command::Overwrite { tile: 14, .. }));
    assert!(!targets_teammate, "you cannot Overwrite a partner's spirit");
}

#[test]
fn the_six_by_six_dusk_contracts_the_rim_to_the_inner_four_by_four() {
    use recollect_core::types::is_rim_w;
    let cat = vec![
        spirit(0, "A1"),
        spirit(1, "B1"),
        spirit(2, "A2"),
        spirit(3, "B2"),
    ];
    let (mut e, _) = Engine::new_2v2(7, cat, four_decks());
    // Drive to the contraction round (default contraction_after = 8).
    let until = e.state().rules.contraction_after;
    e.state_mut_for_test().round = until;
    // Empty the board so the Held Ground law doesn't keep any rim tile.
    for t in e.state_mut_for_test().board.iter_mut() {
        t.spirit = None;
    }
    // End B's turn at the contraction round to trigger the Curl.
    e.state_mut_for_test().active = Seat::B;
    e.state_mut_for_test().active_slot = SeatSlot::B2;
    e.apply(Seat::B, Command::EndTurn).unwrap();
    assert!(e.state().contracted, "the Memory contracted at the Dusk");
    // Every empty RIM tile of the 6×6 has faded; the inner 4×4 survives.
    let w = 6;
    for t in 0..36u8 {
        let faded = e.state().board[t as usize].faded;
        if is_rim_w(t, w) {
            assert!(faded, "rim tile {t} should fade at the 6×6 Dusk");
        } else {
            assert!(!faded, "inner tile {t} must survive (the inner 4×4)");
        }
    }
    // The inner 4×4 is 16 tiles; the rim is 20.
    let survivors = (0..36u8)
        .filter(|&t| !e.state().board[t as usize].faded)
        .count();
    assert_eq!(survivors, 16, "the inner 4×4 (16 tiles) remains");
}
