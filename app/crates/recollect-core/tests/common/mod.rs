#![allow(dead_code)]
use recollect_core::Engine;
use recollect_core::cards::test_catalog;
use recollect_core::state::*;
use recollect_core::types::*;

pub fn deck20() -> Vec<CardId> {
    (0..10u16).chain(0..10u16).map(CardId).collect()
}

pub fn new_match(seed: u64) -> Engine {
    Engine::new(seed, test_catalog(), deck20(), deck20()).0
}

pub fn new_solace_match(seed: u64) -> Engine {
    Engine::new(seed, test_catalog(), deck20(), deck20()).0
    // (PvE flag set by the test via state_mut_for_test)
}

/// A bare board for surgical rule tests: empty decks/hands, rich anima,
/// P1's first placement already done unless a test says otherwise.
pub fn blank() -> GameState {
    let mk = || PlayerState {
        hand: vec![],
        deck: vec![],
        anima: 20,
        glimpsed_this_turn: false,
        peeked_top: None,
        first_placement_done: true,
    };
    GameState {
        temp_mods: Vec::new(),
        temp_reach: Vec::new(),
        temp_restrict: Vec::new(),
        dissolved: Vec::new(),
        next_ritual_discount: [0, 0],
        card_tax: [(0, 0), (0, 0)],
        dissolved_this_turn: [0, 0],
        pending_choice: None,
        choice_queue: Vec::new(),
        fade_delayed: Vec::new(),
        pending_flow_anima: [0, 0],
        ignore_imprint_this_turn: [false, false],
        next_arrival_atk: [0, 0],
        next_arrival_2nd_engage: [None, None],
        temp_retaliation: Vec::new(),
        peeked_fabs: [Vec::new(), Vec::new()],
        ritual_extra_targets: 0,
        impressions_dormant_round: None,
        silenced_terrain: None,
        calm_tiles: Vec::new(),
        last_played_spirit: [None, None],
        bonds: Vec::new(),
        board_w: 5,
        player_a2: None,
        player_b2: None,
        active_slot: recollect_core::types::SeatSlot::A1,
        solace_erasures: 0,
        moved_this_turn: Vec::new(),
        stray_telegraph: None,
        stray: None,
        stray_match: false,
        unwriting_told_this_round: false,
        mulliganed: [false, false],
        rules: recollect_core::state::MatchRules::default(),
        contracted: false,
        round: 1,
        active: Seat::A,
        phase: Phase::Acting,
        board: vec![TileState::default(); BOARD_TILES],
        player_a: mk(),
        player_b: mk(),
    }
}

pub fn t(x: i8, y: i8) -> u8 {
    xy_tile(x, y).unwrap()
}

pub fn def(id: u16) -> CardDef {
    test_catalog()
        .into_iter()
        .find(|c| c.id == CardId(id))
        .unwrap()
}

/// Put a spirit on the board at full printed stats (optionally wounded).
pub fn put(st: &mut GameState, tile: u8, card_id: u16, owner: Seat, hp: Option<i16>) {
    let d = def(card_id);
    st.board[tile as usize].spirit = Some(Spirit {
        replacement_used: false,
        holding: false,
        face_down: false,
        is_token: false,
        placed_by: None,
        card: d.id,
        owner,
        attack: d.attack,
        defense: d.defense,
        hp: hp.unwrap_or(d.hp),
        hp_max: d.hp,
        fading: false,
        banished_by: None,
        intercepted_this_round: false,
        traits_stripped: false,
        traits_stripped_until: None,
        kw_grants: Vec::new(),
        no_engage_until: 0,
        throughline_done: false,
        copied_reach: None,
        fade_deadline: None,
    });
}

pub fn hand(st: &mut GameState, seat: Seat, ids: &[u16]) {
    st.player_mut(seat).hand = ids.iter().map(|i| CardId(*i)).collect();
}

pub fn eng(st: GameState, seed: u64) -> Engine {
    Engine::from_state(st, seed, recollect_core::DrawPos(0), test_catalog())
}

pub fn strikes(evs: &[Event]) -> Vec<(u8, u8, i16, bool, StrikeKind)> {
    evs.iter()
        .filter_map(|e| match e {
            Event::Struck {
                from_tile,
                to_tile,
                damage,
                echo,
                kind,
            } => Some((*from_tile, *to_tile, *damage, *echo, *kind)),
            _ => None,
        })
        .collect()
}

/// Drive a match with a deterministic policy (first legal command) until
/// finished or `cap` commands. Returns commands applied.
pub fn drive_first_legal(e: &mut Engine, cap: usize) -> usize {
    let mut n = 0;
    while n < cap {
        if matches!(e.state().phase, Phase::Finished { .. }) {
            break;
        }
        let seat = e.state().active;
        let legal = e.legal_commands(seat);
        let cmd = legal.first().expect("some command is always legal").clone();
        e.apply(seat, cmd).expect("legal command applies");
        n += 1;
    }
    n
}

/// A 2v2 match for property playouts.
pub fn new_2v2_match(seed: u64) -> Engine {
    let cat = recollect_core::cards::canon_catalog();
    let deck: Vec<recollect_core::types::CardId> = cat
        .iter()
        .filter(|c| {
            c.kind.deck_playable() && matches!(c.kind, recollect_core::types::CardKind::Spirit)
        })
        .take(20)
        .map(|c| c.id)
        .collect();
    let decks = [deck.clone(), deck.clone(), deck.clone(), deck];
    recollect_core::Engine::new_2v2(seed, cat, decks).0
}
