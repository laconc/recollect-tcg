//! v1.2 rules as executable spec sentences (design.md).
use crate::common::*;
use recollect_core::state::*;
use recollect_core::types::*;
use recollect_core::{Command, Engine, Reject, Seat};

// ---- Glimpse (§5) & Release -------------------------------------------------
// Glimpse (`Command::Glimpse`) is burn-then-peek-then-
// spend: to ACTIVATE it you BURN a card of your choice from your hand (the activation
// cost — it leaves play entirely), THEN see your top card and KEEP it on top (no
// Anima) or BOTTOM it for +1 Anima. Two `PendingChoice`s resolved through `Choose`:
// step 1 `GlimpseBurn` (which hand card to spend), step 2 `Glimpse` (0 = keep, 1 =
// bottom). Net: keep = −1 card for foresight; bottom = −2 cards for +1 Anima.

#[test]
fn glimpse_opens_the_burn_choice_first_no_free_anima() {
    let mut st = blank();
    st.player_mut(Seat::A).hand = vec![CardId(3), CardId(7)];
    st.player_mut(Seat::A).deck = vec![CardId(9), CardId(0)];
    let mut e = eng(st, 1);
    let before = e.state().player(Seat::A).anima;
    e.apply(Seat::A, Command::Glimpse).unwrap();
    // The glimpse spends the once-per-turn flag and opens the BURN cost FIRST — NO
    // free Anima (the old free +1 is gone), nothing burned yet, no peek yet.
    assert_eq!(
        e.state().player(Seat::A).anima,
        before,
        "Glimpse alone grants no Anima"
    );
    assert!(e.state().player(Seat::A).glimpsed_this_turn);
    assert_eq!(
        e.state().player(Seat::A).hand,
        vec![CardId(3), CardId(7)],
        "the hand is untouched until a burn is chosen"
    );
    assert!(matches!(
        e.state().pending_choice,
        Some(PendingChoice::GlimpseBurn {
            seat: Seat::A,
            ref burnable,
        }) if *burnable == vec![CardId(3), CardId(7)]
    ));
    // One Choose option per burnable hand card.
    let choose: Vec<_> = e
        .legal_commands(Seat::A)
        .into_iter()
        .filter(|c| matches!(c, Command::Choose { .. }))
        .collect();
    assert_eq!(
        choose,
        vec![Command::Choose { index: 0 }, Command::Choose { index: 1 }]
    );
}

#[test]
fn glimpse_burn_spends_the_chosen_card_then_opens_keep_or_bottom() {
    let mut st = blank();
    st.player_mut(Seat::A).hand = vec![CardId(3), CardId(7)];
    st.player_mut(Seat::A).deck = vec![CardId(9), CardId(0)];
    let mut e = eng(st, 1);
    e.apply(Seat::A, Command::Glimpse).unwrap();
    // Burn the SECOND card (index 1, CardId(7)) — the player's free choice.
    e.apply(Seat::A, Command::Choose { index: 1 }).unwrap();
    let p = e.state().player(Seat::A);
    assert_eq!(
        p.hand,
        vec![CardId(3)],
        "the chosen card left the hand entirely (the burn cost)"
    );
    assert_eq!(
        p.deck,
        vec![CardId(9), CardId(0)],
        "burning doesn't touch the page"
    );
    // Now the keep-or-bottom choice is open on the (unchanged) top card.
    assert!(matches!(
        e.state().pending_choice,
        Some(PendingChoice::Glimpse {
            seat: Seat::A,
            top: CardId(9)
        })
    ));
    let choose: Vec<_> = e
        .legal_commands(Seat::A)
        .into_iter()
        .filter(|c| matches!(c, Command::Choose { .. }))
        .collect();
    assert_eq!(
        choose,
        vec![Command::Choose { index: 0 }, Command::Choose { index: 1 }],
        "keep (0) and bottom (1)"
    );
}

#[test]
fn glimpse_keep_costs_one_card_leaves_the_top_grants_no_anima() {
    let mut st = blank();
    st.player_mut(Seat::A).hand = vec![CardId(3)];
    st.player_mut(Seat::A).deck = vec![CardId(9), CardId(0)];
    let mut e = eng(st, 1);
    let before = e.state().player(Seat::A).anima;
    e.apply(Seat::A, Command::Glimpse).unwrap();
    e.apply(Seat::A, Command::Choose { index: 0 }).unwrap(); // BURN the only card
    e.apply(Seat::A, Command::Choose { index: 0 }).unwrap(); // KEEP the top
    let p = e.state().player(Seat::A);
    assert_eq!(p.anima, before, "keep grants no Anima");
    assert!(p.hand.is_empty(), "net −1 card: the burned card is gone");
    assert_eq!(
        p.deck,
        vec![CardId(9), CardId(0)],
        "the top card stays on top"
    );
    assert_eq!(p.peeked_top, Some(CardId(9)), "and the owner now knows it");
    assert!(
        matches!(e.state().phase, Phase::Acting),
        "both choices resolved"
    );
}

#[test]
fn glimpse_bottom_costs_two_cards_moves_the_top_under_grants_one_anima() {
    let mut st = blank();
    st.player_mut(Seat::A).hand = vec![CardId(3)];
    st.player_mut(Seat::A).deck = vec![CardId(9), CardId(0), CardId(5)];
    let mut e = eng(st, 1);
    let before = e.state().player(Seat::A).anima;
    e.apply(Seat::A, Command::Glimpse).unwrap();
    e.apply(Seat::A, Command::Choose { index: 0 }).unwrap(); // BURN the only card
    e.apply(Seat::A, Command::Choose { index: 1 }).unwrap(); // BOTTOM the top for +1
    let p = e.state().player(Seat::A);
    assert_eq!(p.anima, before + 1, "bottoming buys +1 Anima");
    assert!(
        p.hand.is_empty(),
        "net −2 cards: the burn AND the bottomed top"
    );
    assert_eq!(
        p.deck,
        vec![CardId(0), CardId(5), CardId(9)],
        "the glimpsed top went to the bottom, the rest slid up in order"
    );
    assert_eq!(p.peeked_top, None, "the top is unknown again");
    assert!(matches!(e.state().phase, Phase::Acting));
}

#[test]
fn glimpse_conserves_the_total_card_count() {
    // The burn leaves play; the bottom rotates within the page. Hand+deck shrinks by
    // exactly the cards that LEAVE (the burn): keep ⇒ −1 total, bottom ⇒ still −1
    // total (the bottomed card stays in the deck). This pins "a card leaves the hand,
    // the top is peeked/reordered" — no card is duplicated or lost beyond the burn.
    for keep_index in [0u8, 1u8] {
        let mut st = blank();
        st.player_mut(Seat::A).hand = vec![CardId(3), CardId(7)];
        st.player_mut(Seat::A).deck = vec![CardId(9), CardId(0), CardId(5)];
        let mut e = eng(st, 1);
        let total0 = {
            let p = e.state().player(Seat::A);
            p.hand.len() + p.deck.len()
        };
        e.apply(Seat::A, Command::Glimpse).unwrap();
        e.apply(Seat::A, Command::Choose { index: 0 }).unwrap(); // burn
        e.apply(Seat::A, Command::Choose { index: keep_index })
            .unwrap(); // keep or bottom
        let p = e.state().player(Seat::A);
        assert_eq!(
            p.hand.len() + p.deck.len(),
            total0 - 1,
            "exactly the burned card left play (keep_index {keep_index})"
        );
    }
}

#[test]
fn glimpse_once_per_turn() {
    let mut st = blank();
    st.player_mut(Seat::A).hand = vec![CardId(3)];
    st.player_mut(Seat::A).deck = vec![CardId(9), CardId(0)];
    let mut e = eng(st, 1);
    e.apply(Seat::A, Command::Glimpse).unwrap();
    // While a choice is open, a second Glimpse is barred by the pending choice…
    assert_eq!(
        e.apply(Seat::A, Command::Glimpse),
        Err(Reject::ChoicePending)
    );
    e.apply(Seat::A, Command::Choose { index: 0 }).unwrap(); // burn
    assert_eq!(
        e.apply(Seat::A, Command::Glimpse),
        Err(Reject::ChoicePending)
    );
    e.apply(Seat::A, Command::Choose { index: 0 }).unwrap(); // keep
    // …and once resolved, the once-per-turn flag bars a second Glimpse.
    assert_eq!(
        e.apply(Seat::A, Command::Glimpse),
        Err(Reject::AlreadyGlimpsedThisTurn)
    );
}

#[test]
fn glimpse_burn_rejects_an_out_of_range_card() {
    let mut st = blank();
    st.player_mut(Seat::A).hand = vec![CardId(3), CardId(7)];
    st.player_mut(Seat::A).deck = vec![CardId(9), CardId(0)];
    let mut e = eng(st, 1);
    e.apply(Seat::A, Command::Glimpse).unwrap();
    // Only 0..hand.len() are valid burns; anything else is rejected and leaves the
    // burn choice standing (no card spent).
    assert_eq!(
        e.apply(Seat::A, Command::Choose { index: 2 }),
        Err(Reject::BadHandIndex)
    );
    assert!(matches!(
        e.state().pending_choice,
        Some(PendingChoice::GlimpseBurn { .. })
    ));
    assert_eq!(e.state().player(Seat::A).hand.len(), 2, "nothing burned");
}

#[test]
fn glimpse_keep_or_bottom_rejects_an_out_of_range_option() {
    let mut st = blank();
    st.player_mut(Seat::A).hand = vec![CardId(3)];
    st.player_mut(Seat::A).deck = vec![CardId(9), CardId(0)];
    let mut e = eng(st, 1);
    e.apply(Seat::A, Command::Glimpse).unwrap();
    e.apply(Seat::A, Command::Choose { index: 0 }).unwrap(); // burn
    // Only 0 (keep) and 1 (bottom) are valid; anything else leaves the choice standing.
    assert_eq!(
        e.apply(Seat::A, Command::Choose { index: 2 }),
        Err(Reject::BadHandIndex)
    );
    assert!(matches!(
        e.state().pending_choice,
        Some(PendingChoice::Glimpse { .. })
    ));
}

#[test]
fn glimpse_not_offered_on_an_empty_page() {
    // An empty deck glimpses nothing — the menu omits Glimpse rather than dangling an
    // empty choice (an empty page can still End Turn). A direct call is rejected.
    let mut st = blank();
    st.player_mut(Seat::A).hand = vec![CardId(3)];
    st.player_mut(Seat::A).deck.clear();
    let mut e = eng(st, 1);
    assert!(
        !e.legal_commands(Seat::A).contains(&Command::Glimpse),
        "no card to peek ⇒ Glimpse is not offered"
    );
    assert_eq!(
        e.apply(Seat::A, Command::Glimpse),
        Err(Reject::NothingToPeek)
    );
}

#[test]
fn glimpse_not_offered_with_an_empty_hand() {
    // The activation cost is burning a hand card — an empty hand has nothing to spend,
    // so Glimpse is not offered (and a direct call is rejected). Self-limiting: once
    // your hand is dry you cannot glimpse.
    let mut st = blank();
    st.player_mut(Seat::A).hand.clear();
    st.player_mut(Seat::A).deck = vec![CardId(9), CardId(0)];
    let mut e = eng(st, 1);
    assert!(
        !e.legal_commands(Seat::A).contains(&Command::Glimpse),
        "no card to burn ⇒ Glimpse is not offered"
    );
    assert_eq!(
        e.apply(Seat::A, Command::Glimpse),
        Err(Reject::NothingToBurn)
    );
}

#[test]
fn hand_cap_opens_release_window_and_bottoms_the_choice() {
    let mut st = blank();
    st.active = Seat::B;
    st.player_mut(Seat::A).hand = (0..8u16).map(CardId).collect();
    st.player_mut(Seat::A).deck = vec![CardId(8), CardId(9)];
    let mut e = eng(st, 1);
    e.apply(Seat::B, Command::EndTurn).unwrap();
    assert!(matches!(
        e.state().phase,
        Phase::PendingRelease { seat: Seat::A, .. }
    ));
    assert_eq!(e.state().player(Seat::A).hand.len(), 9);
    // Only Release is legal in the window.
    assert_eq!(
        e.apply(Seat::A, Command::Glimpse),
        Err(Reject::PendingReleaseFirst)
    );
    e.apply(Seat::A, Command::Release { hand_index: 0 })
        .unwrap();
    assert_eq!(e.state().player(Seat::A).hand.len(), 8);
    // The released card went to the bottom of the page.
    assert_eq!(e.state().player(Seat::A).deck.last(), Some(&CardId(0)));
    assert!(matches!(e.state().phase, Phase::Acting));
    // And Release outside the window is meaningless.
    assert_eq!(
        e.apply(Seat::A, Command::Release { hand_index: 0 }),
        Err(Reject::NotPendingRelease)
    );
}

// ---- Rooted Telling --------------------------------------------------------

#[test]
fn rooted_telling_home_rows_reach_and_impressions() {
    let mut st = blank();
    put(&mut st, t(2, 1), 0, Seat::A, None); // Dawnling: cross projects (2,2)
    st.board[t(2, 3) as usize].impressions = vec![Seat::A]; // impressions are expansion fuel
    hand(&mut st, Seat::A, &[4, 4, 4, 4]);
    let mut e = eng(st, 1);
    // Home row: always legal before contraction.
    e.apply(
        Seat::A,
        Command::PlaySpirit {
            hand_index: 0,
            tile: t(0, 0),
            engage: None,
            chain_prefs: Vec::new(),
        },
    )
    .unwrap();
    // Within a spirit's reach.
    e.apply(
        Seat::A,
        Command::PlaySpirit {
            hand_index: 0,
            tile: t(2, 2),
            engage: None,
            chain_prefs: Vec::new(),
        },
    )
    .unwrap();
    // Adjacent to your impression — fight to grow. (Same turn — no action cap to reset.)
    e.apply(
        Seat::A,
        Command::PlaySpirit {
            hand_index: 0,
            tile: t(3, 3),
            engage: None,
            chain_prefs: Vec::new(),
        },
    )
    .unwrap();
    // But not into the unrooted dark.
    assert_eq!(
        e.apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: t(0, 3),
                engage: None,
                chain_prefs: Vec::new()
            }
        ),
        Err(Reject::OutsideProjection)
    );
}

#[test]
fn p1_first_placement_restricted_to_home_rows() {
    let mut st = blank();
    st.player_mut(Seat::A).first_placement_done = false;
    put(&mut st, t(2, 1), 0, Seat::A, None); // even with reach beyond...
    hand(&mut st, Seat::A, &[4]);
    let mut e = eng(st, 1);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: t(2, 2),
                engage: None,
                chain_prefs: Vec::new()
            }
        ),
        Err(Reject::FirstPlacementHomeRows)
    );
}

#[test]
fn margin_rule_when_nothing_is_rooted() {
    let mut st = blank();
    st.round = 9;
    st.contracted = true;
    for tile in 0..BOARD_TILES as u8 {
        if is_rim(tile) {
            st.board[tile as usize].faded = true;
        }
    }
    hand(&mut st, Seat::A, &[4]);
    let mut e = eng(st, 1);
    // No spirits, no impressions, home rows gone: any empty living tile is legal.
    e.apply(
        Seat::A,
        Command::PlaySpirit {
            hand_index: 0,
            tile: t(2, 2),
            engage: None,
            chain_prefs: Vec::new(),
        },
    )
    .unwrap();
}

// ---- The clock -------------------------------------------------------------

#[test]
fn twelve_rounds_contraction_after_eight() {
    let mut e = new_match(42);
    let mut contracted_at = None;
    let mut max_round = 0;
    for _ in 0..400 {
        if matches!(e.state().phase, Phase::Finished { .. }) {
            break;
        }
        let seat = e.state().active;
        let cmd = if matches!(e.state().phase, Phase::PendingRelease { .. }) {
            Command::Release { hand_index: 0 }
        } else {
            Command::EndTurn
        };
        let round_before = e.state().round;
        let evs = e.apply(seat, cmd).unwrap();
        for ev in &evs {
            if let Event::MemoryContracted { faded_tiles } = ev {
                contracted_at = Some((round_before, faded_tiles.len()));
            }
        }
        max_round = max_round.max(e.state().round);
    }
    assert!(
        matches!(e.state().phase, Phase::Finished { .. }),
        "the Memory ends"
    );
    assert_eq!(max_round, 12, "round 12 is Nightfall");
    let (r, n) = contracted_at.expect("contraction happened");
    assert_eq!(r, 8, "contraction fires at the end of round 8");
    assert_eq!(n, 16, "the rim is sixteen tiles");
}

#[test]
fn rim_impressions_lock_and_still_score() {
    let mut st = blank();
    st.board[t(0, 0) as usize].impressions = vec![Seat::A];
    let mut e = eng(st, 1);
    for _ in 0..200 {
        if matches!(e.state().phase, Phase::Finished { .. }) {
            break;
        }
        let seat = e.state().active;
        e.apply(seat, Command::EndTurn).unwrap();
    }
    match e.state().phase {
        Phase::Finished {
            result,
            score_a,
            score_b,
        } => {
            assert!(e.state().board[t(0, 0) as usize].faded);
            assert_eq!((score_a, score_b), (1, 0));
            assert_eq!(result, MatchResult::Win(Seat::A));
        }
        _ => panic!("match should be finished"),
    }
}

// ---- Engagement math -------------------------------------------------------

#[test]
fn engagement_is_simultaneous_both_can_die() {
    let mut st = blank();
    put(&mut st, t(2, 1), 17, Seat::B, Some(10)); // wounded Kilnhorn: 60 Atk
    hand(&mut st, Seat::A, &[9]); // Bristleboar 40/10/40 Lance
    let mut e = eng(st, 1);
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: t(2, 0),
                engage: Some(t(2, 1)),
                chain_prefs: Vec::new(),
            },
        )
        .unwrap();
    let s = strikes(&evs);
    // 40 − 30 = 10 fells the Rhino; its 60 − 10 = 50 (or 70 with the dying
    // beast's Echo — it sits at 10/70) fells the boar back either way.
    assert_eq!((s[0].2, s[0].4), (10, StrikeKind::Engage));
    assert_eq!(s[1].4, StrikeKind::Retaliation);
    assert_eq!(s[1].2, if s[1].3 { 70 } else { 50 });
    let att = e.state().spirit_at(t(2, 0)).unwrap();
    let dfn = e.state().spirit_at(t(2, 1)).unwrap();
    assert!(
        att.fading && dfn.fading,
        "memory, not ballistics: both strikes land"
    );
    assert_eq!(att.banished_by, Some(Seat::B));
    assert_eq!(dfn.banished_by, Some(Seat::A));
}

#[test]
fn wheel_edge_grants_ten() {
    let mut st = blank();
    put(&mut st, t(2, 1), 4, Seat::B, None); // Tearling (Sorrow) 10/20/30
    hand(&mut st, Seat::A, &[9]); // Bristleboar (Fury) edges Sorrow
    let mut e = eng(st, 1);
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: t(2, 0),
                engage: Some(t(2, 1)),
                chain_prefs: Vec::new(),
            },
        )
        .unwrap();
    let s = strikes(&evs);
    assert_eq!(s[0].2, 30, "40 + 10 edge − 20 defense");
    assert!(e.state().spirit_at(t(2, 1)).unwrap().fading);
}

#[test]
fn arcane_pierces_twenty_unless_warded() {
    let mut st = blank();
    put(&mut st, t(1, 1), 10, Seat::B, None); // Pebbling 0/30/30
    put(&mut st, t(3, 1), 7, Seat::B, None); // Hymnal Hart, Warded
    hand(&mut st, Seat::A, &[1, 1]); // Stargazer Heron, Arcane 40 Atk
    let mut e = eng(st, 1);
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: t(1, 0),
                engage: Some(t(1, 1)),
                chain_prefs: Vec::new(),
            },
        )
        .unwrap();
    assert_eq!(strikes(&evs)[0].2, 30, "40 − (30 − 20 pierced)");
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: t(3, 0),
                engage: Some(t(3, 1)),
                chain_prefs: Vec::new(),
            },
        )
        .unwrap();
    assert_eq!(
        strikes(&evs)[0].2,
        10,
        "Warded turns the pierce aside: 40 − 30"
    );
}

#[test]
fn echo_is_twenty_percent_plus_twenty_at_half_or_below() {
    // Greyfin Seal at half HP retaliates at 10 base, 30 with the Echo.
    let mut hits = 0;
    let trials = 200;
    for seed in 0..trials {
        let mut st = blank();
        put(&mut st, t(1, 1), 5, Seat::B, Some(20));
        hand(&mut st, Seat::A, &[2]); // Hushling (Fear): Sorrow edges Fear back
        let mut e = eng(st, seed);
        let evs = e
            .apply(
                Seat::A,
                Command::PlaySpirit {
                    hand_index: 0,
                    tile: t(1, 0),
                    engage: Some(t(1, 1)),
                    chain_prefs: Vec::new(),
                },
            )
            .unwrap();
        let s = strikes(&evs);
        assert!(!s[0].3, "a full-HP attacker never Echoes");
        let (dmg, echo) = (s[1].2, s[1].3);
        if echo {
            hits += 1;
            assert_eq!(dmg, 30, "Echo is +20, always +20");
        } else {
            assert_eq!(dmg, 10);
        }
    }
    assert!((18..=66).contains(&hits), "~20% of {trials}, got {hits}");
}

// ---- Interception ----------------------------------------------------------

#[test]
fn interception_after_engage_before_chain() {
    let mut st = blank();
    put(&mut st, t(2, 1), 4, Seat::B, None); // first victim
    put(&mut st, t(2, 2), 4, Seat::B, None); // chain victim
    put(&mut st, t(1, 1), 3, Seat::B, None); // Pale Stalker: slant covers (2,0)
    hand(&mut st, Seat::A, &[16]); // Brand-Bearer Macaque, Relentless
    let mut e = eng(st, 1);
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: t(2, 0),
                engage: Some(t(2, 1)),
                chain_prefs: Vec::new(),
            },
        )
        .unwrap();
    let kinds: Vec<StrikeKind> = strikes(&evs).iter().map(|s| s.4).collect();
    let pos = |k: StrikeKind| kinds.iter().position(|x| *x == k).unwrap();
    assert!(
        pos(StrikeKind::Engage) < pos(StrikeKind::Interception),
        "the zone answers the arrival"
    );
    assert!(
        pos(StrikeKind::Interception) < pos(StrikeKind::Chain(1)),
        "…and bites BEFORE the rampage continues: the brake on Momentum"
    );
    // The stalker's 40 − 10 = 30 left the Macaque at 10 — it chained anyway.
    assert!(
        e.state().spirit_at(t(2, 2)).unwrap().fading,
        "chain link 1 was still banished"
    );
}

#[test]
fn interception_caps_one_per_arrival_once_per_round_no_retaliation_no_chain() {
    let mut st = blank();
    put(&mut st, t(2, 1), 0, Seat::A, None); // projects (2,2) and (2,0)
    put(&mut st, t(1, 1), 3, Seat::B, None); // stalker one covers (2,2) & (2,0)
    put(&mut st, t(1, 3), 3, Seat::B, None); // stalker two covers (2,2)
    hand(&mut st, Seat::A, &[8, 2]);
    let mut e = eng(st, 1);
    // Arrival into doubly-covered ground: exactly ONE interception, and the
    // Cinderling (20 HP) is banished by it — no retaliation, no chain from the killer.
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: t(2, 2),
                engage: None,
                chain_prefs: Vec::new(),
            },
        )
        .unwrap();
    let s = strikes(&evs);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].4, StrikeKind::Interception);
    assert_eq!(s[0].2, 20, "40 − 20: the wall's full answer");
    assert!(e.state().spirit_at(t(2, 2)).unwrap().fading);
    assert!(!evs.iter().any(|e| matches!(
        e,
        Event::Struck {
            kind: StrikeKind::Chain(_),
            ..
        }
    )));
    // Second arrival this round in stalker one's zone: it has already spoken.
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: t(2, 0),
                engage: None,
                chain_prefs: Vec::new(),
            },
        )
        .unwrap();
    assert!(strikes(&evs).is_empty(), "once per spirit per round");
}

// ---- Momentum --------------------------------------------------------------

#[test]
fn momentum_base_is_one_link_relentless_continues() {
    use recollect_core::types::{CardDef, Reach, Resonance};
    let mut catalog = recollect_core::cards::test_catalog();
    catalog.push(CardDef {
        id: CardId(99),
        name: "Test Tempest".into(),
        cost: 1,
        attack: 40,
        defense: 10,
        hp: 90,
        reach: Reach::Burst,
        resonance: Resonance::Fury,
        arcane: false,
        warded: false,
        mobile: false,
        steadfast: false,
        relentless: true,
        ..Default::default()
    });
    catalog.push(CardDef {
        id: CardId(98),
        name: "Test Squall".into(),
        relentless: false,
        ..catalog.last().unwrap().clone()
    });
    for (card, expect_chains) in [(98u16, 1usize), (99, 2)] {
        let mut st = blank();
        put(&mut st, t(2, 1), 4, Seat::B, None);
        put(&mut st, t(1, 1), 4, Seat::B, None);
        put(&mut st, t(3, 1), 4, Seat::B, None);
        hand(&mut st, Seat::A, &[card]);
        let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), catalog.clone());
        let evs = e
            .apply(
                Seat::A,
                Command::PlaySpirit {
                    hand_index: 0,
                    tile: t(2, 0),
                    engage: Some(t(2, 1)),
                    chain_prefs: Vec::new(),
                },
            )
            .unwrap();
        let chains = strikes(&evs)
            .iter()
            .filter(|s| matches!(s.4, StrikeKind::Chain(_)))
            .count();
        assert_eq!(
            chains, expect_chains,
            "card {card}: base grants one; Relentless keeps going"
        );
    }
}

// ---- Overwrite -------------------------------------------------------------

#[test]
fn overwrite_banishment_takes_the_tile_over_a_banishers_impression() {
    let mut st = blank();
    put(&mut st, t(2, 1), 0, Seat::A, None); // projection onto (2,2)
    put(&mut st, t(2, 2), 4, Seat::B, None); // Tearling, 30 HP
    hand(&mut st, Seat::A, &[9]); // Bristleboar: 40 + 10 edge − 20 = 30
    let mut e = eng(st, 1);
    let evs = e
        .apply(
            Seat::A,
            Command::Overwrite {
                hand_index: 0,
                tile: t(2, 2),
            },
        )
        .unwrap();
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::Overwrote { success: true, .. }))
    );
    let tile = &e.state().board[t(2, 2) as usize];
    let sp = tile.spirit.as_ref().unwrap();
    assert_eq!((sp.owner, sp.card), (Seat::A, CardId(9)));
    assert_eq!(sp.hp, 40, "the Tearling's answer broke on the hide");
    assert_eq!(
        tile.impressions.first().copied(),
        Some(Seat::A),
        "the banisher's impression beneath the newcomer"
    );
}

#[test]
fn overwrite_failure_dissolves_no_impression_damage_persists() {
    let mut st = blank();
    put(&mut st, t(2, 1), 0, Seat::A, None);
    put(&mut st, t(2, 2), 11, Seat::B, None); // Warded Ram, 50 HP
    hand(&mut st, Seat::A, &[9]);
    let mut e = eng(st, 1);
    let evs = e
        .apply(
            Seat::A,
            Command::Overwrite {
                hand_index: 0,
                tile: t(2, 2),
            },
        )
        .unwrap();
    assert!(evs.iter().any(|ev| matches!(
        ev,
        Event::Overwrote {
            success: false,
            damage_to_defender: 20,
            ..
        }
    )));
    let tile = &e.state().board[t(2, 2) as usize];
    let sp = tile.spirit.as_ref().unwrap();
    assert_eq!((sp.owner, sp.hp), (Seat::B, 30), "the wound stays");
    assert_eq!(
        tile.impressions.first().copied(),
        None,
        "a failed unwriting leaves no mark"
    );
    assert!(
        e.state().player(Seat::A).hand.is_empty(),
        "the spirit is spent"
    );
    assert!(
        !evs.iter().any(|ev| matches!(ev, Event::Struck { .. })),
        "no chains from a dissolution"
    );
}

#[test]
fn overwrite_requires_projection() {
    let mut st = blank();
    put(&mut st, t(2, 3), 4, Seat::B, None); // beyond every root
    hand(&mut st, Seat::A, &[9]);
    let mut e = eng(st, 1);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Overwrite {
                hand_index: 0,
                tile: t(2, 3)
            }
        ),
        Err(Reject::OutsideProjection)
    );
}

// ---- Mobile ----------------------------------------------------------------

#[test]
fn mobile_step_is_an_arrival() {
    let mut st = blank();
    put(&mut st, t(2, 1), 14, Seat::A, None); // Spark Shrew, Mobile, 20 HP
    put(&mut st, t(1, 3), 3, Seat::B, None); // stalker covers (2,2)
    let mut e = eng(st, 1);
    let evs = e
        .apply(
            Seat::A,
            Command::MoveSpirit {
                from: t(2, 1),
                to: t(2, 2),
                engage: None,
            },
        )
        .unwrap();
    assert!(evs.iter().any(|ev| matches!(ev, Event::SpiritMoved { .. })));
    let s = strikes(&evs);
    assert_eq!(
        s[0].4,
        StrikeKind::Interception,
        "stepping into the zone is arriving in it"
    );
    assert_eq!(s[0].2, 30, "40 − 10");
    assert!(
        e.state().spirit_at(t(2, 2)).unwrap().fading,
        "the shrew learned about zones"
    );
    // And a non-Mobile spirit cannot step at all.
    let mut st = blank();
    put(&mut st, t(2, 1), 0, Seat::A, None);
    let mut e = eng(st, 1);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::MoveSpirit {
                from: t(2, 1),
                to: t(2, 2),
                engage: None
            }
        ),
        Err(Reject::NotMobile)
    );
}

// ---- Scoring ---------------------------------------------------------------

#[test]
fn dominion_counts_spirits_and_impressions() {
    let mut st = blank();
    st.round = 12;
    st.active = Seat::B;
    put(&mut st, t(2, 2), 0, Seat::A, None);
    put(&mut st, t(1, 2), 4, Seat::B, None);
    st.board[t(3, 2) as usize].impressions = vec![Seat::A];
    let mut e = eng(st, 1);
    let evs = e.apply(Seat::B, Command::EndTurn).unwrap();
    match evs.last().unwrap() {
        Event::MatchEnded {
            result,
            score_a,
            score_b,
        } => {
            assert_eq!((*score_a, *score_b), (2, 1));
            assert_eq!(*result, MatchResult::Win(Seat::A));
        }
        other => panic!("expected MatchEnded, got {other:?}"),
    }
}

#[test]
fn an_exact_tie_is_a_draw_not_a_win() {
    // Mutation-killer (flow.rs `finish`, the `a > b` / `b > a` result comparisons).
    // Equal dominion is a DRAW. One Seat-A spirit and one Seat-B spirit, nothing else,
    // tally (1,1) at Nightfall. If `a > b` were `>=` this tie would become an A-win;
    // if `b > a` were `>=`, a B-win. Only the strict `>` on BOTH sides yields Draw.
    let mut st = blank();
    st.round = 12;
    st.active = Seat::B;
    put(&mut st, t(1, 2), 0, Seat::A, None);
    put(&mut st, t(3, 2), 4, Seat::B, None);
    let mut e = eng(st, 1);
    let evs = e.apply(Seat::B, Command::EndTurn).unwrap();
    match evs.last().unwrap() {
        Event::MatchEnded {
            result,
            score_a,
            score_b,
        } => {
            assert_eq!((*score_a, *score_b), (1, 1), "one spirit each → a tie");
            assert_eq!(
                *result,
                MatchResult::Draw,
                "an exact tie is a Draw, never a Win"
            );
        }
        other => panic!("expected MatchEnded, got {other:?}"),
    }
}

#[test]
fn a_one_tile_lead_for_b_is_a_b_win() {
    // Mutation-killer (flow.rs `finish`, the `b > a` arm + the `Seat::B => b += 1`
    // tally). Seat B holds two tiles to Seat A's one; the result must be Win(B) at
    // (1,2). A `b += 1`→`-=`/`*=` mutation or a flipped `b > a` would not produce this.
    let mut st = blank();
    st.round = 12;
    st.active = Seat::B; // B ends round 12 → finish fires on this EndTurn
    put(&mut st, t(1, 2), 0, Seat::A, None);
    put(&mut st, t(3, 2), 4, Seat::B, None);
    st.board[t(1, 1) as usize].impressions = vec![Seat::B]; // B's second tile, on an inner tile
    let mut e = eng(st, 1);
    let evs = e.apply(Seat::B, Command::EndTurn).unwrap();
    match evs.last().unwrap() {
        Event::MatchEnded {
            result,
            score_a,
            score_b,
        } => {
            assert_eq!((*score_a, *score_b), (1, 2), "B leads by a tile");
            assert_eq!(*result, MatchResult::Win(Seat::B), "B's lead is a B-win");
        }
        other => panic!("expected MatchEnded, got {other:?}"),
    }
}

#[test]
fn the_solace_erasure_tally_joins_its_board_score() {
    // Mutation-killer (flow.rs `finish`, `b = b.saturating_add(sim.solace_erasures)`).
    // The Solace (Seat B) banks off-board erasures that JOIN its board score. Here Seat A
    // holds one tile and Seat B holds none on the board, but the Solace has erased twice:
    // the final tally is (1, 2) → a B-win. Drop the erasure add (the `+`→`-` mutant makes
    // `1 - 2` saturate to 0) and it flips to a 1-0 A-win, so the result assert catches it.
    use recollect_core::types::Faction;
    let mut st = blank();
    st.round = 12;
    st.active = Seat::B; // B (the Solace) ends round 12 → finish fires here
    st.rules.factions[Seat::B as usize] = Faction::Solace;
    st.solace_erasures = 2;
    put(&mut st, t(2, 2), 0, Seat::A, None); // A's only board tile
    let mut e = eng(st, 1);
    let evs = e.apply(Seat::B, Command::EndTurn).unwrap();
    match evs.last().unwrap() {
        Event::MatchEnded {
            result,
            score_a,
            score_b,
        } => {
            assert_eq!(
                (*score_a, *score_b),
                (1, 2),
                "A's one board tile vs the Solace's two banked erasures"
            );
            assert_eq!(
                *result,
                MatchResult::Win(Seat::B),
                "the Solace's erasure tally carries it to a win"
            );
        }
        other => panic!("expected MatchEnded, got {other:?}"),
    }
}

// ---- Held Ground (F-23 variant, behind MatchRules) -------------------------

#[test]
fn held_ground_lingers_then_fades_behind() {
    use recollect_core::state::Event;
    // B ends round 8 with A's Shrew standing on the rim: under the Held
    // Ground law the occupied tile lingers; empty rim fades.
    let mut st = blank();
    st.round = 8;
    st.active = Seat::B;
    put(&mut st, t(2, 0), 14, Seat::A, None); // Spark Shrew, Mobile, on the rim
    let mut e = eng(st, 1);
    let evs = e.apply(Seat::B, Command::EndTurn).unwrap();
    let faded = evs
        .iter()
        .find_map(|ev| match ev {
            Event::MemoryContracted { faded_tiles } => Some(faded_tiles.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        faded.len(),
        15,
        "fifteen empty rim tiles fade; the held one lingers"
    );
    assert!(!faded.contains(&t(2, 0)));
    let sp = e.state().spirit_at(t(2, 0)).unwrap();
    assert!(!sp.fading, "the Memory keeps what is loved");
    // The lingerer steps inward — and the ground fades behind it.
    let evs = e
        .apply(
            Seat::A,
            Command::MoveSpirit {
                from: t(2, 0),
                to: t(2, 1),
                engage: None,
            },
        )
        .unwrap();
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::TileFaded { tile } if *tile == t(2, 0)))
    );
    assert!(e.state().board[t(2, 0) as usize].faded);
}

#[test]
fn held_ground_rejects_overwrite_onto_lingering_rim() {
    let mut st = blank();
    st.contracted = true;
    for tile in 0..BOARD_TILES as u8 {
        if is_rim(tile) && tile != t(2, 0) {
            st.board[tile as usize].faded = true;
        }
    }
    put(&mut st, t(2, 0), 4, Seat::B, None); // lingering Tearling
    put(&mut st, t(2, 1), 0, Seat::A, None); // A projects onto it
    hand(&mut st, Seat::A, &[9]);
    let mut e = eng(st, 1);
    assert_eq!(
        e.apply(
            Seat::A,
            Command::Overwrite {
                hand_index: 0,
                tile: t(2, 0)
            }
        ),
        Err(Reject::TileHeld),
        "ground too thin to write on — but the lingerer can still be engaged"
    );
}

#[test]
fn dont_look_stops_interception_but_keeps_retaliation() {
    // Headline (ruling c): a restricted enemy does NOT intercept an arrival.
    let intercepted = |restrict: bool| -> bool {
        let mut st = blank();
        put(&mut st, t(2, 1), 0, Seat::A, None); // A anchor → projects (2,2)
        put(&mut st, t(1, 1), 3, Seat::B, None); // stalker covers (2,2)
        if restrict {
            let r = st.round;
            st.board[t(1, 1) as usize]
                .spirit
                .as_mut()
                .unwrap()
                .no_engage_until = r;
        }
        hand(&mut st, Seat::A, &[8]);
        let mut e = eng(st, 1);
        let evs = e
            .apply(
                Seat::A,
                Command::PlaySpirit {
                    hand_index: 0,
                    tile: t(2, 2),
                    engage: None,
                    chain_prefs: Vec::new(),
                },
            )
            .unwrap();
        strikes(&evs)
            .iter()
            .any(|s| s.4 == StrikeKind::Interception)
    };
    assert!(
        intercepted(false),
        "unrestricted: the stalker bites the arrival"
    );
    assert!(
        !intercepted(true),
        "Don't Look: the restricted stalker can't intercept"
    );

    // Boundary: a restricted spirit STILL retaliates when itself struck.
    let mut st = blank();
    put(&mut st, t(1, 1), 0, Seat::A, None); // attacker
    put(&mut st, t(2, 1), 0, Seat::B, None); // restricted defender (adjacent)
    let r = st.round;
    {
        let d = st.board[t(2, 1) as usize].spirit.as_mut().unwrap();
        d.no_engage_until = r;
        d.attack = 50;
    }
    let mut e = eng(st, 1);
    let atk0 = e.state().spirit_at(t(1, 1)).unwrap().hp;
    e.resolve_engage_for_test(t(1, 1), t(2, 1));
    assert!(
        e.state().spirit_at(t(1, 1)).map(|s| s.hp).unwrap_or(0) < atk0,
        "the restricted defender still retaliates when struck"
    );
}

#[test]
fn an_unwritten_leaves_no_impression_keyed_on_cardkind() {
    // The lore-stated rule: an Unwritten leaves NO impression — keyed on the
    // CardKind (Unwritten | IllIntent), NOT the old `is_token && solace_pve` proxy. A
    // NON-token Unwritten (is_token=false, not a PvE match) proves the point: under
    // the old proxy it would have left an impression; under the CardKind rule it doesn't.
    use recollect_core::cards::canon_catalog;
    use recollect_core::test_support::put_spirit;
    let cat = canon_catalog();
    let wolf = cat.iter().find(|c| c.name == "Unwritten Wolf").unwrap().id;
    let cloud = cat.iter().find(|c| c.name == "Cloudling").unwrap().id;
    let deck: Vec<CardId> = (0..20).map(|_| cloud).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, wolf, Seat::B); // a non-token Unwritten
        put_spirit(st, 12, cloud, Seat::B); // an ordinary spirit (control)
        st.board[11].spirit.as_mut().unwrap().fading = true;
        st.board[12].spirit.as_mut().unwrap().fading = true;
        st.active = Seat::B;
    }
    // Force B's Fade dissolution (the Fade phase is at turn-END now). Assert on the
    // dissolution EVENTS (a fresh Stray may surface onto the vacated tile, so tile
    // occupancy/impression is unreliable).
    let evs = e.force_fade_step_for_test(Seat::B);
    let dissolved_with_impression = |tile: u8| {
        evs.iter()
            .any(|ev| matches!(ev, Event::SpiritDissolved { tile: t, .. } if *t == tile))
    };
    assert!(
        !dissolved_with_impression(11),
        "the Unwritten left NO impression (no SpiritDissolved for the Unwritten tile)"
    );
    assert!(
        dissolved_with_impression(12),
        "the ordinary spirit DID leave an impression (the contrast that makes the rule meaningful)"
    );
}

#[test]
fn bearer_of_small_stones_parting_frees_this_turns_evolutions() {
    // Parting/Owner/Exception(EvolveIgnoresSharedImprint): when Bearer dissolves, its
    // owner's evolutions ignore the shared-Imprint rule for the rest of that turn.
    use recollect_core::cards::canon_catalog;
    use recollect_core::test_support::put_spirit;
    let cat = canon_catalog();
    let bearer = cat
        .iter()
        .find(|c| c.name == "Bearer of Small Stones")
        .unwrap()
        .id;
    let cloud = cat.iter().find(|c| c.name == "Cloudling").unwrap().id;
    let deck: Vec<CardId> = (0..20).map(|_| cloud).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, bearer, Seat::A);
        st.board[11].spirit.as_mut().unwrap().fading = true;
        st.active = Seat::A;
    }
    assert!(
        !e.state().ignore_imprint_this_turn[Seat::A as usize],
        "precondition: not yet freed"
    );
    // Force A's Fade dissolution (the Fade phase is at turn-END now); Bearer dissolves
    // there, its Parting fires and frees this turn's evolutions. (Using the hook keeps it
    // on A's turn, so the per-turn flag is not then cleared by a TurnEnded.)
    e.force_fade_step_for_test(Seat::A);
    assert!(
        e.state().ignore_imprint_this_turn[Seat::A as usize],
        "Bearer's Parting freed this turn's evolutions from the shared-Imprint rule"
    );
}

// ---- Combat math precision (mutation killers) ------------------------------
// These pin the EXACT post-combat state (both spirits' HP, the precise Struck
// damages, the banish/survive verdict at the lethal boundary) so a mutation of
// the combat arithmetic or comparisons in `engine/combat.rs` changes a value an
// assertion reads. The LOGIC is already correct and fuzz/model-checked; this is
// purely test precision. Grouped by the combat.rs function under test.

// --- full_exchange: the strike/retaliation arithmetic (L270–281) ---
#[test]
fn full_exchange_pins_exact_hp_both_sides_with_edge_and_defense() {
    // Bristleboar (Fury 40/10/40) engages Greyfin Seal (Sorrow 20/30/40), both full.
    // Fury edges Sorrow (+10); Sorrow does not edge Fury (0). No bonus, no Echo (both
    // full HP). dmg_def = 40 + 10 − 30 = 20 ⇒ Seal 40→20. dmg_att = 20 − 10 = 10 ⇒
    // boar 40→30. Every operand here is non-zero, so each `+`/`-` in the two damage
    // expressions changes one of the four asserted numbers.
    let mut st = blank();
    put(&mut st, t(2, 0), 9, Seat::A, None); // Bristleboar
    put(&mut st, t(2, 1), 5, Seat::B, None); // Greyfin Seal
    let mut e = eng(st, 1);
    let evs = e.resolve_engage_for_test(t(2, 0), t(2, 1));
    let s = strikes(&evs);
    assert_eq!(
        (s[0].2, s[0].4),
        (20, StrikeKind::Engage),
        "40 + 10 edge − 30 defense = 20 to the Seal"
    );
    assert_eq!(
        (s[1].2, s[1].4),
        (10, StrikeKind::Retaliation),
        "20 − 10 defense = 10 back to the boar (no edge for Sorrow over Fury)"
    );
    let dfn = e.state().spirit_at(t(2, 1)).unwrap();
    let att = e.state().spirit_at(t(2, 0)).unwrap();
    assert_eq!(
        (dfn.hp, dfn.fading),
        (20, false),
        "Seal survives at exactly 20"
    );
    assert_eq!(
        (att.hp, att.fading),
        (30, false),
        "boar survives at exactly 30"
    );
}

#[test]
fn full_exchange_banishes_the_defender_at_exactly_lethal() {
    // The lethal boundary for `dfn.hp - dmg_def <= 0` (L332): make dmg_def EXACTLY
    // the defender's HP. Bristleboar 40 + 10 edge − 20 def = 30 into a 30-HP Tearling
    // ⇒ 30 − 30 = 0 ≤ 0, banished. A `<=`→`>` flip would call 0 "survived"; a `-`→`+`
    // flip would make it 60 (not lethal). Attacker takes 10 − 10 = 0 back and stands.
    let mut st = blank();
    put(&mut st, t(2, 0), 9, Seat::A, None); // Bristleboar (Fury), full
    put(&mut st, t(2, 1), 4, Seat::B, None); // Tearling (Sorrow) 10/20/30, full
    let mut e = eng(st, 1);
    let evs = e.resolve_engage_for_test(t(2, 0), t(2, 1));
    let s = strikes(&evs);
    assert_eq!(
        s[0].2, 30,
        "40 + 10 edge − 20 = 30, exactly the Tearling's HP"
    );
    let dfn = e.state().spirit_at(t(2, 1)).unwrap();
    assert!(dfn.fading, "exactly-lethal banishes the defender");
    assert_eq!(dfn.banished_by, Some(Seat::A));
    let att = e.state().spirit_at(t(2, 0)).unwrap();
    assert_eq!(
        (att.hp, att.fading),
        (40, false),
        "Tearling's 10 − 10 def = 0: boar untouched"
    );
}

#[test]
fn full_exchange_retaliation_banishes_the_attacker_at_exactly_lethal() {
    // The lethal boundary for `att.hp - dmg_att <= 0` (L388). Both spirits stand at FULL
    // HP (so neither carries an Echo — Echo would perturb the numbers). A full-HP
    // Cinderling (Fury 20/20/20) engages a Pale Stalker (Fear 40/20/40): the Stalker's
    // retaliation is 40 + 0 edge (Fear does not edge Fury) − 20 = 20, EXACTLY the
    // Cinderling's 20 HP ⇒ 20 − 20 = 0 ≤ 0, banished. A `<=`→`>` flip on L388 keeps the
    // Cinderling alive at 0; a `-`→`+` makes it +40 (alive). The Cinderling's own strike
    // is 20 + 0 edge − 20 = 0, so the Stalker is untouched and stands at 40.
    let mut st = blank();
    put(&mut st, t(2, 0), 8, Seat::A, None); // Cinderling (Fury), full HP 20
    put(&mut st, t(2, 1), 3, Seat::B, None); // Pale Stalker (Fear), full HP 40
    let mut e = eng(st, 1);
    let evs = e.resolve_engage_for_test(t(2, 0), t(2, 1));
    let s = strikes(&evs);
    assert_eq!(
        (s[1].2, s[1].4),
        (20, StrikeKind::Retaliation),
        "the Stalker's 40 − 20 def = 20 retaliation, exactly the Cinderling's HP"
    );
    let att = e.state().spirit_at(t(2, 0)).unwrap();
    assert!(
        att.fading,
        "exactly-lethal retaliation banishes the attacker"
    );
    assert_eq!(att.banished_by, Some(Seat::B));
    let dfn = e.state().spirit_at(t(2, 1)).unwrap();
    assert_eq!(
        (dfn.hp, dfn.fading),
        (40, false),
        "the Stalker stands untouched at 40"
    );
}

#[test]
fn full_exchange_chain_link_bonus_is_exactly_ten_times_the_link() {
    // Momentum's chain bonus rides into full_exchange as `bonus` (L270 `+ bonus`) and is
    // computed in momentum_prefs (L655-657) as MOMENTUM_PER_LINK(10) * link. A Burst-reach
    // Relentless attacker kills its engage target, then chains TWICE — `chain_prefs` pins
    // the target order so the picks are deterministic (the heuristic is bypassed):
    //   link 1 bonus = 10 * 1 = 10 ⇒ 40 + 10 − 20 = 30 (exactly fells a 30-HP target)
    //   link 2 bonus = 10 * 2 = 20 ⇒ 40 + 20 − 20 = 40 (exactly fells a 40-HP target)
    // Two links with DIFFERENT damages pin the `* link` factor: a `*`→`/` flip makes
    // link 2's bonus 10/2 = 5 (dmg 25); a `*`→`+` makes it 10+2 = 12 (dmg 32); a `+ bonus`
    // flip in full_exchange moves both. Same-resonance (Fury) targets keep the edge at 0.
    use recollect_core::types::{CardDef, Reach, Resonance};
    let mut catalog = recollect_core::cards::test_catalog();
    catalog.push(CardDef {
        id: CardId(99),
        name: "Test Tempest".into(),
        cost: 1,
        attack: 40,
        defense: 10,
        hp: 90,
        reach: Reach::Burst,
        resonance: Resonance::Fury,
        arcane: false,
        warded: false,
        mobile: false,
        steadfast: false,
        relentless: true,
        ..Default::default()
    });
    let mut st = blank();
    // Arriver lands on the home row (2,0) — a legal placement; its Burst reaches the engage
    // target (2,1) and both chain targets (1,1) and (3,1). All Cinderling (Fury 20/20/20).
    put(&mut st, t(2, 1), 8, Seat::B, None); // engage target: 40 − 20 = 20 = HP ⇒ banished
    put(&mut st, t(1, 1), 8, Seat::B, Some(30)); // chain link 1 target: 30 HP
    put(&mut st, t(3, 1), 8, Seat::B, Some(40)); // chain link 2 target: 40 HP
    hand(&mut st, Seat::A, &[99]);
    let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), catalog);
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: t(2, 0),
                engage: Some(t(2, 1)),
                chain_prefs: vec![t(1, 1), t(3, 1)], // pin the chain order: link 1 → (1,1), link 2 → (3,1)
            },
        )
        .unwrap();
    let link1: Vec<_> = strikes(&evs)
        .into_iter()
        .filter(|s| s.4 == StrikeKind::Chain(1))
        .collect();
    let link2: Vec<_> = strikes(&evs)
        .into_iter()
        .filter(|s| s.4 == StrikeKind::Chain(2))
        .collect();
    assert_eq!(
        (link1.len(), link2.len()),
        (1, 1),
        "exactly two chain links landed"
    );
    assert_eq!(
        link1[0].2, 30,
        "chain link 1 = 40 attack + (10 * 1) momentum bonus − 20 defense"
    );
    assert_eq!(
        link2[0].2, 40,
        "chain link 2 = 40 attack + (10 * 2) momentum bonus − 20 defense"
    );
    assert!(
        e.state().spirit_at(t(1, 1)).unwrap().fading
            && e.state().spirit_at(t(3, 1)).unwrap().fading,
        "both chain targets were exactly felled by their links"
    );
}

#[test]
fn momentum_auto_targets_the_banishable_enemy_first() {
    // momentum_prefs' fallback heuristic (L683-697 `max_by_key((banishing, hp))`): with NO
    // chain_prefs, the engine auto-targets a foe it can BANISH this link over one it merely
    // dents. A non-Relentless Burst attacker gets exactly one momentum link; two foes are in
    // reach — A (30 HP, fellable by 40 + 10 − 20 = 30) and B (90 HP, not fellable). The
    // banishing-first policy must pick A: A is banished, B is left untouched. A `<=`→`>`
    // flip on the banishing test (or `+`→`-` in its damage estimate) would mis-rank them and
    // bite B instead. The single link can't reach both, so the pick is decisive.
    use recollect_core::types::{CardDef, Reach, Resonance};
    let mut catalog = recollect_core::cards::test_catalog();
    catalog.push(CardDef {
        id: CardId(98),
        name: "Test Squall".into(),
        cost: 1,
        attack: 40,
        defense: 10,
        hp: 90,
        reach: Reach::Burst,
        resonance: Resonance::Fury,
        arcane: false,
        warded: false,
        mobile: false,
        steadfast: false,
        relentless: false, // base Momentum: exactly ONE bonus link
        ..Default::default()
    });
    let mut st = blank();
    put(&mut st, t(2, 1), 8, Seat::B, None); // engage target ⇒ banished ⇒ momentum fires
    put(&mut st, t(1, 1), 8, Seat::B, Some(30)); // foe A: fellable (30 HP)
    put(&mut st, t(3, 1), 8, Seat::B, Some(90)); // foe B: not fellable (90 HP)
    hand(&mut st, Seat::A, &[98]);
    let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), catalog);
    e.apply(
        Seat::A,
        Command::PlaySpirit {
            hand_index: 0,
            tile: t(2, 0),
            engage: Some(t(2, 1)),
            chain_prefs: Vec::new(), // no prefs ⇒ the heuristic decides
        },
    )
    .unwrap();
    assert!(
        e.state().spirit_at(t(1, 1)).unwrap().fading,
        "the banishable foe A was chosen and felled"
    );
    assert_eq!(
        e.state().spirit_at(t(3, 1)).map(|s| (s.hp, s.fading)),
        Some((90, false)),
        "foe B (un-banishable) was not targeted — untouched at 90"
    );
}

// --- interception: the single-bite arithmetic + best-coverer pick (L492, L516) ---
#[test]
fn interception_pins_exact_bite_and_survival() {
    // One interceptor, one bite, no retaliation. Pale Stalker (Fear 40/20/40) at (1,1)
    // covers the arrival (2,0) on its Slant. The arriver Aurora Elk (Wonder 30/40/70)
    // survives the single strike at a pinned HP. dmg = 40 attack + 0 edge (Fear does
    // not edge Wonder) − 40 defense = 0 — too soft to bite, so use a softer defender to
    // get a non-zero number we can pin. Use a 20-defense arriver instead.
    let mut st = blank();
    put(&mut st, t(1, 1), 3, Seat::B, None); // Pale Stalker, Slant covers (2,0)
    // Arriver: Cinderling (Fury 20/20/20) but bumped HP so it survives a known bite.
    // Stalker 40 + 0 edge (Fear vs Fury: no) − 20 def = 20. Put arriver at 50 HP ⇒ 30 left.
    put(&mut st, t(2, 0), 8, Seat::A, Some(50));
    let mut e = eng(st, 1);
    let evs = e.run_interception_for_test(t(2, 0), Seat::A);
    let s = strikes(&evs);
    assert_eq!(s.len(), 1, "exactly one interception bite");
    assert_eq!(
        (s[0].2, s[0].4),
        (20, StrikeKind::Interception),
        "40 − 20 defense = 20 (no edge, Fear over Fury is not on the wheel)"
    );
    let arr = e.state().spirit_at(t(2, 0)).unwrap();
    assert_eq!(
        (arr.hp, arr.fading),
        (30, false),
        "arriver survives at exactly 30"
    );
}

#[test]
fn interception_banishes_the_arriver_at_exactly_lethal() {
    // The interception lethal boundary `arr.hp - dmg <= 0` (L533). Pale Stalker bites
    // for 40 − 20 = 20; the Cinderling has exactly 20 HP ⇒ banished. (This also pins
    // the bite arithmetic L516: a `-`→`+` flip makes the bite 60 — still lethal — but a
    // `+`→`-` on `attack + edge` with edge 0 is inert here, so the survival test above
    // carries the non-edge arithmetic and this one carries the boundary.)
    let mut st = blank();
    put(&mut st, t(1, 1), 3, Seat::B, None); // Pale Stalker
    put(&mut st, t(2, 0), 8, Seat::A, None); // Cinderling 20/20/20, full
    let mut e = eng(st, 1);
    let evs = e.run_interception_for_test(t(2, 0), Seat::A);
    let s = strikes(&evs);
    assert_eq!(s[0].2, 20, "40 − 20 = 20, exactly the Cinderling's HP");
    assert!(
        e.state().spirit_at(t(2, 0)).unwrap().fading,
        "exactly-lethal interception banishes the arriver"
    );
}

#[test]
fn interception_picks_the_highest_attack_coverer() {
    // The best-coverer comparison `sp.attack > a` (L492): two eligible interceptors both
    // reach the arrival; the higher-Attack one must bite. Stalker A (attack bumped to 60)
    // and Stalker B (attack 30) both cover (2,0). The bite must be 60 − 20 = 40 (the
    // strong one), not 30 − 20 = 10. A `>`→`<` flip would pick the weak coverer (10).
    let mut st = blank();
    put(&mut st, t(1, 1), 3, Seat::B, None); // Stalker A: Slant covers (2,0)
    st.board[t(1, 1) as usize].spirit.as_mut().unwrap().attack = 60;
    put(&mut st, t(3, 1), 3, Seat::B, None); // Stalker B: Slant covers (2,0)
    st.board[t(3, 1) as usize].spirit.as_mut().unwrap().attack = 30;
    put(&mut st, t(2, 0), 8, Seat::A, Some(90)); // arriver survives either way
    let mut e = eng(st, 1);
    let evs = e.run_interception_for_test(t(2, 0), Seat::A);
    let s = strikes(&evs);
    assert_eq!(
        s.len(),
        1,
        "still exactly one bite from doubly-covered ground"
    );
    assert_eq!(
        s[0].2, 40,
        "the highest-Attack coverer (60) bites: 60 − 20 = 40, not the weak 10"
    );
    assert_eq!(s[0].0, t(1, 1), "and it is Stalker A that spoke");
}

// --- feral_stray_intercepts: the bite + counter arithmetic (L566, L588) ---
fn feral_catalog() -> Vec<recollect_core::types::CardDef> {
    use recollect_core::types::{CardDef, CardKind, Reach, Resonance};
    let mut c = recollect_core::cards::test_catalog();
    c.push(CardDef {
        id: CardId(40),
        name: "Cornered Lynx".into(),
        cost: 0,
        attack: 30,
        defense: 10,
        hp: 40,
        reach: Reach::Cross,
        resonance: Resonance::Neutral, // no edge either way — isolates the raw arithmetic
        kind: CardKind::Foundling,
        rarity: "G".into(),
        ..Default::default()
    });
    c
}

// `feral_stray_intercepts` runs only at the tail of `interception`, AFTER a normal
// enemy coverer bites (combat.rs L536). So each Feral test stages a normal Pale
// Stalker interceptor (Fear 40/20/40, Slant) that reaches the arrival and bites for
// a fixed 40 − 10 = 30; the Feral Stray's own bite/counter is then asserted on top.
#[test]
fn feral_stray_bite_and_counter_are_exact() {
    // A Feral Stray (Cornered Lynx, Neutral 30/10/40) at (2,1) Cross-reaches the arrival
    // (2,2). A Bristleboar arriver (40/10/40) bumped to 90 HP survives both bites:
    //   normal Stalker bite = 40 − 10 = 30 → 90→60
    //   Stray bite          = 30 − 10 = 20 → 60→40
    //   counter on the Stray = 40 − 10 = 30 → Stray 40→10
    let mut st = blank();
    put(&mut st, t(2, 2), 9, Seat::A, Some(90)); // Bristleboar arriver at (2,2)
    put(&mut st, t(1, 1), 3, Seat::B, None); // Pale Stalker: Slant covers (2,2)
    st.stray = Some(Stray {
        card: CardId(40),
        tile: t(2, 1), // Cross of (2,1) includes (2,2): the arrival is in reach
        temperament: Temperament::Feral,
        veiled: false,
        courtship: 0,
        courted_by: None,
        hp: 40,
        hp_max: 40,
    });
    let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), feral_catalog());
    let evs = e.run_interception_for_test(t(2, 2), Seat::A);
    // The Stray's own bite (Struck from the Stray's tile).
    let bite = strikes(&evs)
        .into_iter()
        .find(|s| s.0 == t(2, 1) && s.4 == StrikeKind::Interception)
        .expect("the Feral Stray bit the arrival");
    assert_eq!(bite.2, 20, "Stray bite = 30 attack − 10 arriver-defense");
    assert_eq!(
        e.state().spirit_at(t(2, 2)).unwrap().hp,
        40,
        "arriver survives both bites at exactly 40 (90 − 30 normal − 20 Stray)",
    );
    // The counter wound on the Stray.
    let counter = evs
        .iter()
        .find_map(|ev| match ev {
            Event::StrayStruck { damage, hp, .. } => Some((*damage, *hp)),
            _ => None,
        })
        .expect("the arrival wounded the Stray back");
    assert_eq!(
        counter,
        (30, 10),
        "counter = 40 arriver-attack − 10 Stray-defense ⇒ Stray 40→10"
    );
    assert_eq!(
        e.state().stray.as_ref().unwrap().hp,
        10,
        "the Stray clings on at exactly 10",
    );
}

#[test]
fn feral_stray_counter_banishes_the_stray_at_exactly_lethal() {
    // The Stray-banish boundary `new_hp <= 0` (L606) and the `s.hp -= counter` (L590).
    // The arrival's counter is 40 − 10 = 30; the Stray sits at exactly 30 HP ⇒ drops to
    // 0 and is banished. A `-=`→`+=` flip would heal it; a `<=`→`>` flip leaves it at 0.
    let mut st = blank();
    put(&mut st, t(2, 2), 9, Seat::A, Some(90)); // Bristleboar arriver
    put(&mut st, t(1, 1), 3, Seat::B, None); // Pale Stalker: normal coverer
    st.stray = Some(Stray {
        card: CardId(40),
        tile: t(2, 1),
        temperament: Temperament::Feral,
        veiled: false,
        courtship: 0,
        courted_by: None,
        hp: 30,
        hp_max: 40,
    });
    let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), feral_catalog());
    let evs = e.run_interception_for_test(t(2, 2), Seat::A);
    assert!(
        evs.iter().any(
            |ev| matches!(ev, Event::StrayBanished { impression, .. } if *impression == Seat::A)
        ),
        "the counter (30) brought the 30-HP Stray to exactly 0 ⇒ banished"
    );
    assert!(
        e.state().stray.is_none(),
        "the Stray is gone from the board"
    );
}

#[test]
fn feral_stray_does_not_intercept_an_arrival_outside_its_reach() {
    // The reach gate `oriented_w(reach, stray.tile, …).contains(&arrival)` (L561): a Cross
    // Stray at (0,0) does NOT reach (2,2), so it neither bites nor is countered — no
    // Struck-from-stray, no StrayStruck. The normal Stalker still bites (30), proving the
    // Feral path was REACHED and declined purely on the reach gate. A dropped gate would
    // make the Stray bite anyway.
    let mut st = blank();
    put(&mut st, t(2, 2), 9, Seat::A, Some(90)); // Bristleboar arriver
    put(&mut st, t(1, 1), 3, Seat::B, None); // Pale Stalker: Slant covers (2,2)
    st.stray = Some(Stray {
        card: CardId(40),
        tile: t(0, 0), // far from (2,2): the arrival is NOT in its Cross
        temperament: Temperament::Feral,
        veiled: false,
        courtship: 0,
        courted_by: None,
        hp: 40,
        hp_max: 40,
    });
    let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), feral_catalog());
    let evs = e.run_interception_for_test(t(2, 2), Seat::A);
    assert!(
        !evs.iter().any(|ev| matches!(ev, Event::StrayStruck { .. })),
        "out-of-reach Feral Stray does not fight the arrival"
    );
    assert!(
        !strikes(&evs).iter().any(|s| s.0 == t(0, 0)),
        "and lands no bite of its own"
    );
    assert_eq!(
        e.state().spirit_at(t(2, 2)).unwrap().hp,
        60,
        "only the normal Stalker bit (90 − 30); the distant Stray did nothing",
    );
}

// --- interception: edge, the fading-arrival gate, and the no-bite gate ---
#[test]
fn interception_adds_the_resonance_edge_to_the_bite() {
    // L504-516 `icp.attack + edge`: a Pale Stalker (Fear) intercepting a Harmony arriver
    // gets the wheel edge (Fear edges Harmony, +10). Bite = 40 + 10 edge − 20 def = 30.
    // A `+ edge`→`- edge` flip gives 10; `+`→`*` explodes. Sproutling (Harmony 10/20/30)
    // bumped to 60 HP survives to pin the 30.
    let mut st = blank();
    put(&mut st, t(1, 1), 3, Seat::B, None); // Pale Stalker (Fear): Slant covers (2,0)
    put(&mut st, t(2, 0), 6, Seat::A, Some(60)); // Sproutling (Harmony) arriver
    let mut e = eng(st, 1);
    let evs = e.run_interception_for_test(t(2, 0), Seat::A);
    let s = strikes(&evs);
    assert_eq!(s.len(), 1, "one bite");
    assert_eq!(
        s[0].2, 30,
        "40 attack + 10 edge (Fear over Harmony) − 20 defense = 30"
    );
    assert_eq!(
        e.state().spirit_at(t(2, 0)).unwrap().hp,
        30,
        "the arriver survives at exactly 30"
    );
}

#[test]
fn interception_skips_a_fading_arrival() {
    // L450-453: the arrival is read as `Some(sp) if !sp.fading` (else `return`). A FADING
    // arrival (already gone) is not bitten. With the guard forced `true`, the dead arrival
    // would be struck anyway. The Pale Stalker covers the tile, so only the fading guard
    // stops the bite.
    let mut st = blank();
    put(&mut st, t(1, 1), 3, Seat::B, None); // Pale Stalker covers (2,0)
    put(&mut st, t(2, 0), 8, Seat::A, None); // arriver …
    st.board[t(2, 0) as usize].spirit.as_mut().unwrap().fading = true; // … but already fading
    let mut e = eng(st, 1);
    let evs = e.run_interception_for_test(t(2, 0), Seat::A);
    assert!(
        strikes(&evs).is_empty(),
        "a fading arrival draws no interception"
    );
}

#[test]
fn interception_does_not_bite_when_the_base_damage_is_zero() {
    // L516-519 `if base == 0 && !echo_possible { return }`: an interceptor whose attack is
    // fully blunted by the arriver's defense (base 0) and which is NOT at Echo deals no
    // bite at all — no Struck event. A dropped `!` would fall through and push a 0-damage
    // Struck. Pale Stalker attack lowered to 20 vs a 20-defense arriver (full HP, no Echo):
    // base = 20 − 20 = 0.
    let mut st = blank();
    put(&mut st, t(1, 1), 3, Seat::B, None); // Pale Stalker, full HP (not at Echo)
    st.board[t(1, 1) as usize].spirit.as_mut().unwrap().attack = 20;
    put(&mut st, t(2, 0), 8, Seat::A, None); // Cinderling (defense 20), full HP
    let mut e = eng(st, 1);
    let evs = e.run_interception_for_test(t(2, 0), Seat::A);
    assert!(
        strikes(&evs).is_empty(),
        "base 0 and no Echo ⇒ no bite, not even a 0-damage strike"
    );
    assert_eq!(
        e.state().spirit_at(t(2, 0)).unwrap().hp,
        20,
        "the arriver is untouched"
    );
}

// --- feral_stray_intercepts: the temperament/owner gates and the bite/counter boundaries ---
// (Each stages a normal Pale Stalker so the Feral path is reached; see the note above.)
#[test]
fn a_veiled_feral_stray_does_not_fight() {
    // L553 `stray.veiled || temperament != Feral`: a VEILED stray does not fight even if
    // Feral. An `||`→`&&` flip (veiled && non-Feral) would let the veiled Feral bite. The
    // normal Stalker still bites (so the Feral path WAS reached); the stray must not.
    let mut st = blank();
    put(&mut st, t(2, 2), 9, Seat::A, Some(90)); // arriver
    put(&mut st, t(1, 1), 3, Seat::B, None); // normal coverer
    st.stray = Some(Stray {
        card: CardId(40),
        tile: t(2, 1), // would reach (2,2) if it fought
        temperament: Temperament::Feral,
        veiled: true, // … but veiled
        courtship: 0,
        courted_by: None,
        hp: 40,
        hp_max: 40,
    });
    let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), feral_catalog());
    let evs = e.run_interception_for_test(t(2, 2), Seat::A);
    assert!(
        !evs.iter().any(|ev| matches!(ev, Event::StrayStruck { .. })),
        "a veiled Feral Stray does not fight"
    );
    assert!(
        !strikes(&evs).iter().any(|s| s.0 == t(2, 1)),
        "and lands no bite of its own"
    );
}

#[test]
fn a_feral_stray_does_not_fight_an_arrival_the_normal_bite_already_banished() {
    // L556-559 `Some(sp) if !sp.fading && sp.owner == actor`: by the time the Feral path
    // runs, the normal interceptor may have BANISHED the arrival (→ fading). A Feral Stray
    // does not fight a spirit that is already gone. Here the Pale Stalker fells the 20-HP
    // arrival (40 − 20 = 20), so when feral_stray_intercepts reads the arrival it is fading
    // ⇒ no fight. A `&&`→`||` flip (or the guard forced `true`) would have the Stray bite and
    // be countered by the dead arrival anyway — assert NO StrayStruck and no stray bite.
    let mut st = blank();
    put(&mut st, t(2, 2), 8, Seat::A, Some(20)); // arriver: Cinderling, felled by the normal bite
    put(&mut st, t(1, 1), 3, Seat::B, None); // Pale Stalker: Slant covers (2,2), bites 20 ⇒ banishes
    st.stray = Some(Stray {
        card: CardId(40),
        tile: t(2, 1), // Cross reaches (2,2): the Stray WOULD bite a standing arrival
        temperament: Temperament::Feral,
        veiled: false,
        courtship: 0,
        courted_by: None,
        hp: 40,
        hp_max: 40,
    });
    let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), feral_catalog());
    let evs = e.run_interception_for_test(t(2, 2), Seat::A);
    assert!(
        e.state()
            .spirit_at(t(2, 2))
            .map(|s| s.fading)
            .unwrap_or(true),
        "the normal Stalker bite banished the arrival"
    );
    assert!(
        !evs.iter().any(|ev| matches!(ev, Event::StrayStruck { .. })),
        "the Feral Stray does not fight an arrival that is already gone"
    );
    assert!(
        !strikes(&evs).iter().any(|s| s.0 == t(2, 1)),
        "and lands no bite of its own"
    );
}

#[test]
fn feral_stray_with_no_bite_neither_strikes_nor_is_countered() {
    // L566-567 `if bite > 0`: a Stray whose attack is fully blunted by the arriver's defense
    // deals NO bite (no Struck from the stray). A `>`→`>=` flip would push a 0-damage strike.
    // Bristleboar arriver bumped to defense 30 ⇒ Stray bite = 30 − 30 = 0. (The counter still
    // lands — that path is asserted elsewhere — so we check only the absence of the stray's bite.)
    let mut st = blank();
    put(&mut st, t(2, 2), 9, Seat::A, Some(90)); // arriver
    st.board[t(2, 2) as usize].spirit.as_mut().unwrap().defense = 30; // blunts the Stray's 30 attack to 0
    put(&mut st, t(1, 1), 3, Seat::B, None); // normal coverer
    st.stray = Some(Stray {
        card: CardId(40),
        tile: t(2, 1),
        temperament: Temperament::Feral,
        veiled: false,
        courtship: 0,
        courted_by: None,
        hp: 40,
        hp_max: 40,
    });
    let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), feral_catalog());
    let evs = e.run_interception_for_test(t(2, 2), Seat::A);
    assert!(
        !strikes(&evs).iter().any(|s| s.0 == t(2, 1)),
        "a 0 Stray bite emits no Struck"
    );
}

#[test]
fn feral_stray_bite_banishes_the_arrival_at_exactly_lethal() {
    // L579-584 `s.hp - bite <= 0`: the banish check. The Struck event is applied first (it
    // decrements HP), so when this check runs the arrival is ALREADY at `pre − bite`; the
    // condition fires iff `(pre − bite) − bite <= 0`, i.e. pre <= 2·bite. We set the HP going
    // into the Stray bite to EXACTLY 2·bite = 40 (arrival 70, normal Stalker bite 30 ⇒ 40):
    // post-bite HP is 20, and `20 − 20 = 0 <= 0` ⇒ banished. A `-`→`/` flip makes it
    // `20 / 20 = 1 > 0` (the arrival wrongly survives); `<=`→`>` likewise leaves it standing.
    let mut st = blank();
    put(&mut st, t(2, 2), 9, Seat::A, Some(70)); // arriver: 70 − 30 normal = 40 into the Stray bite
    put(&mut st, t(1, 1), 3, Seat::B, None); // normal coverer: bites 40 − 10 = 30
    st.stray = Some(Stray {
        card: CardId(40),
        tile: t(2, 1),
        temperament: Temperament::Feral,
        veiled: false,
        courtship: 0,
        courted_by: None,
        hp: 90, // the Stray survives the counter, so the arrival's banish is the only outcome under test
        hp_max: 90,
    });
    let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), feral_catalog());
    let evs = e.run_interception_for_test(t(2, 2), Seat::A);
    let stray_bite = strikes(&evs)
        .into_iter()
        .find(|s| s.0 == t(2, 1))
        .expect("the Stray bit");
    assert_eq!(stray_bite.2, 20, "Stray bite = 30 − 10 = 20");
    assert!(
        e.state().spirit_at(t(2, 2)).is_none()
            || e.state()
                .spirit_at(t(2, 2))
                .map(|s| s.fading)
                .unwrap_or(true),
        "the Stray's bite brought the arrival to exactly 0 ⇒ banished"
    );
}

#[test]
fn feral_stray_with_no_counter_is_not_struck_back() {
    // L588-589 `if counter > 0`: when the arrival's attack is fully blunted by the Stray's
    // defense, there is NO counter — no StrayStruck event. A `>`→`>=` flip would push a
    // 0-damage StrayStruck. Bristleboar arriver attack lowered to 10 = the Stray's defense ⇒
    // counter = 10 − 10 = 0.
    let mut st = blank();
    put(&mut st, t(2, 2), 9, Seat::A, Some(90)); // arriver
    st.board[t(2, 2) as usize].spirit.as_mut().unwrap().attack = 10; // == Stray defense ⇒ counter 0
    put(&mut st, t(1, 1), 3, Seat::B, None); // normal coverer
    st.stray = Some(Stray {
        card: CardId(40),
        tile: t(2, 1),
        temperament: Temperament::Feral,
        veiled: false,
        courtship: 0,
        courted_by: None,
        hp: 40,
        hp_max: 40,
    });
    let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), feral_catalog());
    let evs = e.run_interception_for_test(t(2, 2), Seat::A);
    assert!(
        !evs.iter().any(|ev| matches!(ev, Event::StrayStruck { .. })),
        "a 0 counter emits no StrayStruck"
    );
    assert_eq!(
        e.state().stray.as_ref().unwrap().hp,
        40,
        "the Stray is unwounded"
    );
}

// --- momentum_prefs: the banishing-first ESTIMATE arithmetic (L683-697) ---
// The estimate `e.hp <= attack + bonus + edge − eff_defense(e.def, …)` decides which foe
// the engine auto-targets. These pin the defense and edge terms by making them flip which
// foe is judged "banishable", so a mutated estimate picks the OTHER foe (observably).
fn burst_chainer(attack: i16, resonance: recollect_core::types::Resonance) -> Vec<CardDef> {
    use recollect_core::types::{CardDef, Reach};
    let mut catalog = recollect_core::cards::test_catalog();
    catalog.push(CardDef {
        id: CardId(97),
        name: "Test Chainer".into(),
        cost: 1,
        attack,
        defense: 10,
        hp: 90,
        reach: Reach::Burst,
        resonance,
        relentless: false, // exactly one momentum link ⇒ the pick is decisive
        ..Default::default()
    });
    catalog
}

#[test]
fn momentum_estimate_subtracts_the_targets_defense() {
    // L691 `- eff_defense(e.defense, …)` in the banishing estimate. Chainer attack 60,
    // link-1 bonus 10. Foe A = Pebbling (Resolve, defense 30) at 50 HP: estimate 60 + 10 − 30
    // = 40 < 50 ⇒ NOT banishable. Foe B = Cinderling (Fury, defense 20) at 40 HP: estimate
    // 60 + 10 − 20 = 50 ≥ 40 ⇒ banishable. So the engine picks B (the foe it can fell). A
    // `-`→`+` flip would judge A banishable too (60 + 10 + 30 = 100) and, tie-broken by the
    // higher HP, target A instead. We assert B is felled and A stands.
    let catalog = burst_chainer(60, recollect_core::types::Resonance::Fury);
    let mut st = blank();
    put(&mut st, t(2, 1), 8, Seat::B, Some(20)); // engage target (Fury Cinderling): felled by 60
    put(&mut st, t(1, 1), 10, Seat::B, Some(50)); // foe A: Pebbling, defense 30
    put(&mut st, t(3, 1), 8, Seat::B, Some(40)); // foe B: Cinderling, defense 20
    hand(&mut st, Seat::A, &[97]);
    let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), catalog);
    e.apply(
        Seat::A,
        Command::PlaySpirit {
            hand_index: 0,
            tile: t(2, 0),
            engage: Some(t(2, 1)),
            chain_prefs: Vec::new(), // no prefs ⇒ the estimate decides
        },
    )
    .unwrap();
    assert!(
        e.state()
            .spirit_at(t(3, 1))
            .map(|s| s.fading)
            .unwrap_or(true),
        "foe B (estimate ≥ its HP) is the banishing-first pick and is felled"
    );
    assert_eq!(
        e.state().spirit_at(t(1, 1)).map(|s| (s.hp, s.fading)),
        Some((50, false)),
        "foe A (defense subtracted ⇒ not banishable) is left untouched"
    );
}

#[test]
fn momentum_estimate_adds_the_resonance_edge() {
    // L686 `+ if edge { EDGE }` in the banishing estimate. Chainer is Fury (edges Sorrow,
    // not Resolve), attack 40, link-1 bonus 10. Foe A = Tearling (Sorrow, defense 20) at 40
    // HP: estimate 40 + 10 + 10 edge − 20 = 40 ≥ 40 ⇒ banishable; the actual blow (also 40)
    // fells it. Foe B = Pebbling (Resolve, defense 30) at 50 HP: estimate 40 + 10 + 0 − 30 =
    // 20 < 50 ⇒ NOT banishable. The engine picks A (the edge makes it fellable). A
    // `+ EDGE`→`- EDGE` flip drops A's estimate to 20 (< 40, not banishable); both foes are
    // then judged non-banishable and the tie-break picks the HIGHER-HP foe B (50 > 40) — whose
    // actual blow (40 + 10 − 30 = 20) does NOT fell its 50 HP. So under the mutant A is left
    // untouched. We assert A is felled (and B stands), which fails for the mutant.
    let catalog = burst_chainer(40, recollect_core::types::Resonance::Fury);
    let mut st = blank();
    put(&mut st, t(2, 1), 8, Seat::B, Some(20)); // engage target (Fury): felled by 40
    put(&mut st, t(1, 1), 4, Seat::B, Some(40)); // foe A: Tearling (Sorrow, def 20) — Fury edges it
    put(&mut st, t(3, 1), 10, Seat::B, Some(50)); // foe B: Pebbling (Resolve, def 30), higher HP — no edge
    hand(&mut st, Seat::A, &[97]);
    let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), catalog);
    e.apply(
        Seat::A,
        Command::PlaySpirit {
            hand_index: 0,
            tile: t(2, 0),
            engage: Some(t(2, 1)),
            chain_prefs: Vec::new(),
        },
    )
    .unwrap();
    assert!(
        e.state()
            .spirit_at(t(1, 1))
            .map(|s| s.fading)
            .unwrap_or(true),
        "foe A (edge lifts its estimate to lethal) is the banishing-first pick and is felled"
    );
    assert_eq!(
        e.state().spirit_at(t(3, 1)).map(|s| (s.hp, s.fading)),
        Some((50, false)),
        "foe B (no edge ⇒ not the pick) is left untouched at its full 50"
    );
}

// --- interception: the contracted-rim gate, Unforgiving's Warded bypass, and Echo math ---
#[test]
fn a_held_ground_rim_zone_still_bites_from_the_interior() {
    // L475 `!(sim.contracted && is_rim_w(i, …))`: on a CONTRACTED board, rim zones no longer
    // bite, but INTERIOR coverers still do. Interceptor at an interior tile (1,1) with the
    // board contracted must still intercept an interior arrival. An `&&`→`||` flip would read
    // `!(contracted || is_rim)` = false for ANY tile while contracted, silencing the interior
    // bite too.
    let mut st = blank();
    st.contracted = true;
    put(&mut st, t(1, 1), 3, Seat::B, None); // Pale Stalker at interior (1,1): Slant covers (2,2)
    put(&mut st, t(2, 2), 8, Seat::A, Some(50)); // arriver at interior (2,2)
    let mut e = eng(st, 1);
    let evs = e.run_interception_for_test(t(2, 2), Seat::A);
    let s = strikes(&evs);
    assert_eq!(
        s.len(),
        1,
        "the interior coverer still bites on a contracted board"
    );
    assert_eq!(s[0].2, 20, "40 − 20 = 20");
}

#[test]
fn the_unforgiving_interception_ignores_the_arrivers_warded() {
    // L509-516 `… && !card_carries_static_exception(icp.card, StrikesIgnoreWarded)`: The
    // Unforgiving's interception bite pierces a Warded arriver's defense (Warded would
    // otherwise keep it). A normal interceptor is blunted; The Unforgiving is not. Using a
    // custom catalog (the exception is keyed by NAME): a Warded, Arcane-defense-keeping
    // arriver takes MORE from The Unforgiving than from a plain interceptor of equal Attack.
    use recollect_core::types::{CardDef, Reach, Resonance};
    let mut catalog = recollect_core::cards::test_catalog();
    catalog.push(CardDef {
        id: CardId(90),
        name: "The Unforgiving".into(),
        cost: 1,
        attack: 50,
        defense: 30,
        hp: 60,
        reach: Reach::Lance,
        resonance: Resonance::Neutral,
        arcane: true, // pierces — but Warded would turn that aside unless the exception applies
        ..Default::default()
    });
    catalog.push(CardDef {
        id: CardId(91),
        name: "Plain Piercer".into(),
        cost: 1,
        attack: 50,
        defense: 30,
        hp: 60,
        reach: Reach::Lance,
        resonance: Resonance::Neutral,
        arcane: true,
        ..Default::default()
    });
    // Arriver: Hymnal Hart (Warded, 20/30/50) at (2,1). A Seat B Lance interceptor at (2,2)
    // looks "forward" (toward Seat A) onto (2,1). The custom interceptor cards (90/91) are not
    // in test_catalog, so place them by building the Spirit from the catalog CardDef directly
    // (rather than via `put`, which looks up test_catalog).
    let bite_from = |interceptor: u16| -> i16 {
        let mut st = blank();
        let ic = catalog
            .iter()
            .find(|c| c.id == CardId(interceptor))
            .unwrap();
        st.board[t(2, 2) as usize].spirit = Some(Spirit {
            card: ic.id,
            owner: Seat::B,
            attack: ic.attack,
            defense: ic.defense,
            hp: ic.hp,
            hp_max: ic.hp,
            fading: false,
            banished_by: None,
            intercepted_this_round: false,
            traits_stripped: false,
            traits_stripped_until: None,
            replacement_used: false,
            holding: false,
            face_down: false,
            is_token: false,
            placed_by: None,
            kw_grants: Vec::new(),
            no_engage_until: 0,
            throughline_done: false,
            copied_reach: None,
            fade_deadline: None,
        });
        put(&mut st, t(2, 1), 7, Seat::A, Some(50)); // Warded Hymnal Hart arriver (id 7 ∈ test_catalog)
        let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), catalog.clone());
        let evs = e.run_interception_for_test(t(2, 1), Seat::A);
        strikes(&evs).first().map(|s| s.2).unwrap_or(0)
    };
    // Plain Arcane vs Warded: defense kept ⇒ 50 − 30 = 20. The Unforgiving ignores Warded ⇒
    // it pierces 20 ⇒ 50 − (30 − 20) = 40.
    assert_eq!(
        bite_from(91),
        20,
        "a plain Arcane interceptor is turned aside by Warded: 50 − 30"
    );
    assert_eq!(
        bite_from(90),
        40,
        "The Unforgiving ignores the arriver's Warded and pierces: 50 − (30 − 20)"
    );
}

#[test]
fn interception_echo_adds_exactly_twenty() {
    // L503-521: an echo-eligible interceptor (below half HP) Echoes ~20% of the time; WHEN it
    // does, the bite is base + ECHO_BONUS (L521 `base + 20`). Pin the echo magnitude across
    // seeds: every echoed bite is base + 20, never base − 20 (the `+`→`-` flip). Pale Stalker
    // (40/20/40) at 20 HP ⇒ at Echo; base = 40 − 20 = 20 vs a 20-defense arriver.
    let mut echoes = 0;
    let trials = 200;
    for seed in 0..trials {
        let mut st = blank();
        put(&mut st, t(1, 1), 3, Seat::B, Some(20)); // Pale Stalker at half HP ⇒ Echo-eligible
        put(&mut st, t(2, 0), 8, Seat::A, Some(90)); // arriver (defense 20), survives
        let mut e = eng(st, seed);
        let evs = e.run_interception_for_test(t(2, 0), Seat::A);
        let s = strikes(&evs);
        assert_eq!(s.len(), 1, "one bite");
        let (dmg, echo) = (s[0].2, s[0].3);
        if echo {
            echoes += 1;
            assert_eq!(dmg, 40, "an echoed interception is base 20 + 20 = 40");
        } else {
            assert_eq!(dmg, 20, "a plain interception is the base 20");
        }
    }
    assert!(
        (18..=66).contains(&echoes),
        "~20% of {trials} bites echo, got {echoes}"
    );
}

#[test]
fn an_enemy_lullaby_suppresses_an_interceptors_echo() {
    // L503 `… && !echo_suppressed(it)`: an interceptor adjacent to an ENEMY Lullaby is too
    // calm to Echo. With the suppressor present, the at-Echo Pale Stalker NEVER echoes across
    // all seeds (the bite is always the base 20). A dropped `!` would invert suppression and
    // let it echo ~20% of the time. The Lullaby (Wide, SuppressesAdjacentEnemyEcho) is the
    // arriving seat's ally, adjacent to the interceptor.
    let cat = recollect_core::cards::canon_catalog();
    let stalker = cat.iter().find(|c| c.name == "Pale Stalker").map(|c| c.id);
    let stalker = match stalker {
        Some(id) => id,
        None => return, // canon naming guard
    };
    let lullaby = cat.iter().find(|c| c.name == "The Lullaby").unwrap().id;
    let cloud = cat.iter().find(|c| c.name == "Cloudling").unwrap().id;
    let mut any = false;
    for seed in 0..120u64 {
        let deck: Vec<CardId> = (0..20).map(|_| cloud).collect();
        let (mut e, _) = Engine::new(seed, cat.clone(), deck.clone(), deck);
        {
            let st = e.state_mut_for_test();
            recollect_core::test_support::put_spirit(st, 7, stalker, Seat::B); // interceptor (2,1)
            st.board[7].spirit.as_mut().unwrap().hp = 1; // deep at Echo
            recollect_core::test_support::put_spirit(st, 12, lullaby, Seat::A); // arriver's ally Lullaby, adjacent (2,2)
            recollect_core::test_support::put_spirit(st, 11, cloud, Seat::A); // the arriver (1,2): Pale Stalker Slant covers it
            st.board[11].spirit.as_mut().unwrap().hp = 90;
        }
        let evs = e.run_interception_for_test(11, Seat::A);
        for s in strikes(&evs) {
            any = true;
            assert!(
                !s.3,
                "an interceptor beside an enemy Lullaby never echoes (seed {seed})"
            );
        }
    }
    assert!(any, "the interceptor bit at least once across the seeds");
}

#[test]
fn momentum_stops_when_the_chaining_spirit_is_felled_by_a_chain_retaliation() {
    // momentum_prefs L646-648 `Some(sp) if !sp.fading => …, _ => return`: each loop iteration
    // re-reads the chaining spirit; if it has Faded (died to a chain link's retaliation) the
    // chain STOPS. We stage a mutual kill on link 1 — the attacker fells its target but dies
    // to the target's retaliation — while a third enemy waits in reach. Real: link 2 never
    // fires (the chaining spirit is gone). A `!sp.fading`→`true` flip would let the dead
    // attacker strike a second time (a Chain(2) event). We assert NO Chain(2) strike exists.
    use recollect_core::types::{CardDef, Reach, Resonance};
    let mut catalog = recollect_core::cards::test_catalog();
    catalog.push(CardDef {
        id: CardId(96),
        name: "Test Berserker".into(),
        cost: 1,
        attack: 40,
        defense: 0,
        hp: 25, // survives the engage retaliation (10), then dies to the chain-1 retaliation (40)
        reach: Reach::Burst,
        resonance: Resonance::Fury,
        relentless: true, // would chain again but for the death
        ..Default::default()
    });
    let mut st = blank();
    // engage target: Tearling (Sorrow 10/20/30) at 20 HP. Berserker 40 + 10 edge − 20 = 30 fells
    // it; its retaliation is only 10 (Sorrow does not edge Fury), leaving the Berserker at 15.
    put(&mut st, t(2, 1), 4, Seat::B, Some(20));
    // chain-1 target: Bristleboar (40 atk) at 30 HP. Berserker 40 + 10 bonus − 10 = 40 fells it;
    // its retaliation 40 − 0 = 40 fells the 15-HP Berserker (a MUTUAL kill on link 1).
    put(&mut st, t(1, 1), 9, Seat::B, Some(30));
    // chain-2 target waiting in reach (would be hit only if the dead attacker chained on).
    put(&mut st, t(3, 1), 10, Seat::B, Some(20));
    hand(&mut st, Seat::A, &[96]);
    let mut e = Engine::from_state(st, 1, recollect_core::DrawPos(0), catalog);
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: t(2, 0),
                engage: Some(t(2, 1)),
                chain_prefs: vec![t(1, 1), t(3, 1)],
            },
        )
        .unwrap();
    assert!(
        e.state()
            .spirit_at(t(2, 0))
            .map(|s| s.fading)
            .unwrap_or(true),
        "the chaining Berserker died to the chain-1 retaliation"
    );
    assert!(
        e.state()
            .spirit_at(t(1, 1))
            .map(|s| s.fading)
            .unwrap_or(true),
        "but it still felled its chain-1 target (mutual)"
    );
    assert!(
        !strikes(&evs).iter().any(|s| s.4 == StrikeKind::Chain(2)),
        "a Faded chaining spirit does not strike again — no Chain(2)"
    );
    assert_eq!(
        e.state().spirit_at(t(3, 1)).map(|s| (s.hp, s.fading)),
        Some((20, false)),
        "the third enemy is untouched — momentum stopped"
    );
}
