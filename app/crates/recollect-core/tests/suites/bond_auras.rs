//! Phase-1 effects: the bonded-pair auras `combat_stats` applies. Held Hands /
//! High Ground are covered in spellbook.rs; this pins the one behavior changed in
//! this pass — Rivals' Pact's "+10 Attack each **while both damaged**" (the bond
//! `WhileDamaged` gate now requires BOTH ends hurt, not just the one being read).
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::effects::Keyword;
use recollect_core::engine::{combat_stats_for_test, keyword_active_for_test};
use recollect_core::state::{Bond, Command, Event};
use recollect_core::test_support::put_spirit;
use recollect_core::types::{CardId, Seat};

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

#[test]
fn rivals_pact_buffs_attack_only_while_both_damaged() {
    let cat = canon_catalog();
    // Attack of the bonded spirit at tile 11, with each end optionally wounded.
    let attack_with = |dmg_a: bool, dmg_b: bool| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Cloudling"), Seat::A);
            put_spirit(st, 12, id_of("Cloudling"), Seat::A);
            if dmg_a {
                st.board[11].spirit.as_mut().unwrap().hp -= 10;
            }
            if dmg_b {
                st.board[12].spirit.as_mut().unwrap().hp -= 10;
            }
            st.bonds.push(Bond {
                card: id_of("Rivals' Pact"),
                owner: Seat::A,
                tile_a: 11,
                tile_b: 12,
            });
        }
        combat_stats_for_test(e.state(), &cat, 11).attack
    };

    let base = attack_with(false, false);
    assert_eq!(attack_with(true, false), base, "only one wounded → no buff");
    assert_eq!(
        attack_with(false, true),
        base,
        "only the partner wounded → no buff"
    );
    assert_eq!(
        attack_with(true, true),
        base + 10,
        "both wounded → Rivals' Pact grants +10 Attack"
    );
}

#[test]
fn co_conspirators_debuffs_enemies_adjacent_to_the_pair() {
    let cat = canon_catalog();
    // Attack of an enemy spirit, given where it stands and whether the bonded
    // pair is adjacent (the PairAdjacent gate). Pair on 11 + (12|13).
    let enemy_attack = |enemy_tile: u8, pair_adjacent: bool| -> i16 {
        let partner = if pair_adjacent { 12 } else { 13 };
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Cloudling"), Seat::A);
            put_spirit(st, partner, id_of("Cloudling"), Seat::A);
            put_spirit(st, enemy_tile, id_of("Cloudling"), Seat::B);
            st.bonds.push(Bond {
                card: id_of("Co-Conspirators"),
                owner: Seat::A,
                tile_a: 11,
                tile_b: partner,
            });
        }
        combat_stats_for_test(e.state(), &cat, enemy_tile).attack
    };

    // On the 5×5 board tile 11=(1,2): tile 16=(1,3) is adjacent to endpoint 11;
    // tile 24=(4,4) is far from the pair.
    assert_eq!(enemy_attack(16, true), 0, "adjacent enemy: -10 Attack");
    assert_eq!(enemy_attack(24, true), 10, "distant enemy: unaffected");
    assert_eq!(
        enemy_attack(16, false),
        10,
        "pair not adjacent (PairAdjacent fails): no debuff"
    );
}

/// A bonded keyword grant reaches `tile` while the pair is present & adjacent.
fn pair_grants(bond: &str, kw: Keyword, pair_adjacent: bool) -> bool {
    let cat = canon_catalog();
    let partner = if pair_adjacent { 12 } else { 13 };
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        put_spirit(st, partner, id_of("Cloudling"), Seat::A);
        st.bonds.push(Bond {
            card: id_of(bond),
            owner: Seat::A,
            tile_a: 11,
            tile_b: partner,
        });
    }
    keyword_active_for_test(e.state(), &cat, 11, kw)
}

#[test]
fn fellow_travelers_grants_mobile_while_adjacent() {
    let cat = canon_catalog();
    let mut e = engine();
    // No bond: Cloudling is not intrinsically Mobile.
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
    }
    assert!(
        !keyword_active_for_test(e.state(), &cat, 11, Keyword::Mobile),
        "Cloudling is not intrinsically Mobile"
    );
    assert!(
        pair_grants("Fellow Travelers", Keyword::Mobile, true),
        "Fellow Travelers grants Mobile to the adjacent pair"
    );
    assert!(
        !pair_grants("Fellow Travelers", Keyword::Mobile, false),
        "the grant lapses when the pair is not adjacent"
    );
}

#[test]
fn shoulder_to_shoulder_grants_steadfast_and_defense() {
    let cat = canon_catalog();
    assert!(
        pair_grants("Shoulder to Shoulder", Keyword::Steadfast, true),
        "Shoulder to Shoulder grants Steadfast to the adjacent pair"
    );
    assert!(
        !pair_grants("Shoulder to Shoulder", Keyword::Steadfast, false),
        "Steadfast lapses when the pair is not adjacent"
    );
    // Its other clause (+10 Defense each) is a BondedPair StatDelta aura.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
        st.bonds.push(Bond {
            card: id_of("Shoulder to Shoulder"),
            owner: Seat::A,
            tile_a: 11,
            tile_b: 12,
        });
    }
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, 11).defense,
        10,
        "+10 Defense each"
    );
}

#[test]
fn race_you_buffs_both_partners_attack_while_adjacent() {
    // Static/PairAdjacent/BondedPair/StatDelta{atk:10}: combat_stats adds +10 Attack
    // to BOTH ends while the pair is adjacent — a documented simplification of the
    // reactive "whichever engages first grants the other +10" text.
    let cat = canon_catalog();
    let atk = |with_bond: bool| -> (i16, i16) {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Cloudling"), Seat::A);
            put_spirit(st, 12, id_of("Cloudling"), Seat::A);
            if with_bond {
                st.bonds.push(Bond {
                    card: id_of("Race You"),
                    owner: Seat::A,
                    tile_a: 11,
                    tile_b: 12,
                });
            }
        }
        (
            combat_stats_for_test(e.state(), &cat, 11).attack,
            combat_stats_for_test(e.state(), &cat, 12).attack,
        )
    };
    let (a0, b0) = atk(false);
    let (a1, b1) = atk(true);
    assert_eq!(a1 - a0, 10, "Race You: +10 Attack to one end");
    assert_eq!(b1 - b0, 10, "Race You: +10 Attack to the other end");
}

#[test]
fn pack_tactics_chips_the_target_before_the_engage() {
    // GrantEngage{pre_chip:10}: when a bonded spirit engages, its partner (in reach)
    // first chips 10. Attacker attack=0 isolates the chip from the main exchange.
    let dmg = |with_bond: bool| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Cloudling"), Seat::A); // attacker
            st.board[11].spirit.as_mut().unwrap().attack = 0;
            put_spirit(st, 12, id_of("Cloudling"), Seat::A); // partner (reaches 13)
            put_spirit(st, 13, id_of("Cloudling"), Seat::B); // target
            st.board[13].spirit.as_mut().unwrap().attack = 0;
            if with_bond {
                st.bonds.push(Bond {
                    card: id_of("Pack Tactics"),
                    owner: Seat::A,
                    tile_a: 11,
                    tile_b: 12,
                });
            }
        }
        let hp0 = e.state().board[13].spirit.as_ref().unwrap().hp;
        e.resolve_engage_for_test(11, 13);
        hp0 - e.state().board[13]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(40)
    };
    assert_eq!(
        dmg(true) - dmg(false),
        10,
        "Pack Tactics chips 10 to the target before the engage"
    );
}

#[test]
fn pack_tactics_does_not_chip_when_the_pair_is_split_apart() {
    // Mutation-killer (combat.rs `pack_tactics_chip`, the
    // `manhattan(a,b) != 1 || !present(a) || !present(b)` gate). The pre-chip is a
    // BONDED-PAIR power: when the two ends are not adjacent (manhattan > 1) the bond
    // grants nothing — even if the distant partner reaches the target. Geometry (5×5,
    // tile = y*5+x): attacker@7=(2,1) engages target@12=(2,2) [adjacent — the engage
    // lands]; partner@11=(1,2) reaches 12 (Cross of 11 = {6,10,12,16}) but bond(7,11)
    // spans manhattan 2. So there must be NO pre-chip and the only damage is the engage
    // itself (here 0). An `||`→`&&` flip would make the gate false and chip anyway.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 7, id_of("Cloudling"), Seat::A); // attacker (7~12 adjacent)
        st.board[7].spirit.as_mut().unwrap().attack = 0; // isolate any chip from the strike
        put_spirit(st, 12, id_of("Cloudling"), Seat::B); // target
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // partner: reaches 12, but split from 7
        st.bonds.push(Bond {
            card: id_of("Pack Tactics"),
            owner: Seat::A,
            tile_a: 7,
            tile_b: 11,
        });
    }
    let hp0 = e.state().board[12].spirit.as_ref().unwrap().hp;
    e.resolve_engage_for_test(7, 12);
    let dealt = hp0
        - e.state().board[12]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(hp0);
    assert_eq!(
        dealt, 0,
        "split Pack Tactics pair (manhattan>1): NO pre-chip — the bond grants nothing"
    );
}

#[test]
fn pack_tactics_does_not_chip_a_target_its_partner_cannot_reach() {
    // Mutation-killer (combat.rs `pack_tactics_chip`, the partner-reach gate
    // `!oriented_w(reach, partner, …).contains(&def_tile)`): the partner only chips a
    // target IN ITS OWN reach. attacker@12=(2,2) engages target@13=(3,2) [12 reaches 13];
    // partner@7=(2,1) is bonded & adjacent to 12, but 7's Cross = {2,6,8,12} does NOT
    // include 13 — so no pre-chip. Only the engage's own damage lands (here 30). A
    // dropped reach-gate would chip the unreachable target.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // attacker (reaches 13)
        st.board[12].spirit.as_mut().unwrap().attack = 30;
        put_spirit(st, 7, id_of("Cloudling"), Seat::A); // partner: 7~12 adjacent, but 7 ∌ 13
        put_spirit(st, 13, id_of("Cloudling"), Seat::B); // target — outside 7's reach
        st.board[13].spirit.as_mut().unwrap().attack = 0;
        st.bonds.push(Bond {
            card: id_of("Pack Tactics"),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 7,
        });
    }
    let hp0 = e.state().board[13].spirit.as_ref().unwrap().hp;
    e.resolve_engage_for_test(12, 13);
    let dealt = hp0
        - e.state().board[13]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(hp0);
    assert_eq!(
        dealt, 30,
        "partner cannot reach the target: exactly the engage's 30, no +10 pre-chip"
    );
}

#[test]
fn conspiracy_does_not_counter_when_the_pair_is_split_apart() {
    // Mutation-killer (combat.rs `conspiracy_counter`, the present-and-adjacent gate
    // mirroring Pack Tactics). The counter-engage is a BONDED-PAIR power: a non-adjacent
    // (manhattan > 1) partner never counters, even with the attacker in reach.
    // attacker@6=(1,1) engages defender@7=(2,1) [adjacent]; partner@11=(1,2) reaches the
    // attacker (Cross of 11 = {6,10,12,16} ∋ 6) but bond(7,11) is manhattan 2 — split. So
    // the attacker takes NO counter damage (its attack is 0, the defender's too). An
    // `||`→`&&` flip on the gate would let the split partner counter.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 6, id_of("Cloudling"), Seat::B); // attacker
        st.board[6].spirit.as_mut().unwrap().attack = 0; // no engage/retaliation noise
        put_spirit(st, 7, id_of("Cloudling"), Seat::A); // engaged defender
        st.board[7].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // partner: reaches 6, but split from 7
        st.board[11].spirit.as_mut().unwrap().attack = 50; // would bite hard IF it countered
        st.bonds.push(Bond {
            card: id_of("Conspiracy"),
            owner: Seat::A,
            tile_a: 7,
            tile_b: 11,
        });
    }
    let hp0 = e.state().board[6].spirit.as_ref().unwrap().hp;
    e.resolve_engage_for_test(6, 7);
    let dealt = hp0
        - e.state().board[6]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(hp0);
    assert_eq!(
        dealt, 0,
        "split Conspiracy pair (manhattan>1): the partner does NOT counter-engage the attacker"
    );
}

#[test]
fn promise_redirects_a_lethal_blow_to_the_partner() {
    // Replace(PartnerTakesIt), OncePerMatch: when a bonded spirit would Fade, the
    // partner takes the FULL blow instead and the saved spirit is untouched.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 150; // decisively lethal
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // bonded defender
        st.board[12].spirit.as_mut().unwrap().attack = 0; // no retaliation noise
        put_spirit(st, 13, id_of("Cloudling"), Seat::A); // bonded partner (12~13 adjacent)
        st.bonds.push(Bond {
            card: id_of("Promise"),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 13,
        });
    }
    let def_hp0 = e.state().board[12].spirit.as_ref().unwrap().hp;
    e.resolve_engage_for_test(11, 12);
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        def_hp0,
        "Promise: the saved spirit is untouched"
    );
    assert!(
        e.state().board[13]
            .spirit
            .as_ref()
            .map(|s| s.fading)
            .unwrap_or(true),
        "Promise: the partner took the lethal blow and Faded"
    );
}

#[test]
fn conspiracy_lets_the_partner_counter_engage_the_attacker() {
    // GrantEngage{immediate:true}: when a bonded spirit is engaged, its partner
    // (with the attacker in reach) immediately counter-engages. The bond is the
    // only thing that touches the attacker, so its HP drop isolates the counter.
    let atk_hp = |with_bond: bool| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 6, id_of("Cloudling"), Seat::B); // attacker
            st.board[6].spirit.as_mut().unwrap().attack = 0; // no engage/retaliation damage
            put_spirit(st, 12, id_of("Cloudling"), Seat::A); // bonded defender (engaged)
            st.board[12].spirit.as_mut().unwrap().attack = 0; // its retaliation does nothing
            put_spirit(st, 7, id_of("Cloudling"), Seat::A); // partner: 7~12 adjacent, reaches 6
            st.board[7].spirit.as_mut().unwrap().attack = 50; // the counter bites for 30
            if with_bond {
                st.bonds.push(Bond {
                    card: id_of("Conspiracy"),
                    owner: Seat::A,
                    tile_a: 12,
                    tile_b: 7,
                });
            }
        }
        e.resolve_engage_for_test(6, 12);
        e.state().board[6]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(0)
    };
    assert!(
        atk_hp(true) < atk_hp(false),
        "Conspiracy: the partner counter-engages the attacker (with-bond {} < no-bond {})",
        atk_hp(true),
        atk_hp(false)
    );
}

#[test]
fn grief_split_leaves_the_bonded_spirit_at_one_and_overflows_to_the_partner() {
    // RedirectDamageToPartner, PairAdjacent: a lethal blow leaves the bonded spirit
    // at 1 HP; the partner absorbs the overflow (here enough to Fade it).
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 150; // overflow Fades the partner
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // bonded defender
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 13, id_of("Cloudling"), Seat::A); // partner (12~13 adjacent)
        st.bonds.push(Bond {
            card: id_of("Grief Split"),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 13,
        });
    }
    e.resolve_engage_for_test(11, 12);
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        1,
        "Grief Split: the bonded spirit clings on at 1 HP"
    );
    assert!(
        e.state().board[13]
            .spirit
            .as_ref()
            .map(|s| s.fading)
            .unwrap_or(true),
        "Grief Split: the partner absorbed the overflow and Faded"
    );
}

#[test]
fn common_cause_buffs_the_pair_when_one_defeats_an_enemy() {
    // OnDefeat/PairAdjacent/BondedPair/StatDelta: when a bonded spirit defeats an
    // enemy, both get +10 Attack this round. Uses the engage test hook.
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // bonded victor
        put_spirit(st, 16, id_of("Cloudling"), Seat::A); // bonded partner (11~16 adjacent)
        st.board[11].spirit.as_mut().unwrap().attack = 90; // enough to defeat
        put_spirit(st, 12, id_of("Cloudling"), Seat::B); // the enemy
        st.board[12].spirit.as_mut().unwrap().hp = 5;
        st.board[12].spirit.as_mut().unwrap().attack = 0; // no retaliation
        st.bonds.push(Bond {
            card: id_of("Common Cause"),
            owner: Seat::A,
            tile_a: 11,
            tile_b: 16,
        });
    }
    let partner_before = combat_stats_for_test(e.state(), &cat, 16).attack;
    e.resolve_engage_for_test(11, 12);
    assert!(
        e.state().board[12]
            .spirit
            .as_ref()
            .map(|s| s.fading)
            .unwrap_or(true),
        "the enemy was defeated"
    );
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, 16).attack,
        partner_before + 10,
        "the bonded partner gained +10 Attack this round"
    );
}

#[test]
fn linnet_of_the_lea_draws_when_you_play_a_bond() {
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Linnet of the Lea"), Seat::A); // the watcher
        put_spirit(st, 16, id_of("Cloudling"), Seat::A); // a bondable pair
        put_spirit(st, 17, id_of("Cloudling"), Seat::A);
        st.player_a.hand = vec![id_of("Held Hands")];
        st.player_a.anima = 9;
    }
    let deck_before = e.state().player(Seat::A).deck.len();
    let attach = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::AttachBond { .. }))
        .expect("a bond is attachable");
    e.apply(Seat::A, attach).unwrap();
    assert_eq!(
        e.state().player(Seat::A).deck.len(),
        deck_before - 1,
        "Linnet drew a card when its teller played a Bond"
    );
}

#[test]
fn oathbound_warded_blunts_arcane_when_the_pair_defends() {
    // Oathbound grants the bonded pair Warded; Warded keeps a defender's Defense
    // against an Arcane attacker (which otherwise pierces it). HP lost with/without.
    let cat = canon_catalog();
    let _ = &cat;
    let dmg = |with_oathbound: bool| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 6, id_of("Stargazer Heron"), Seat::B); // Arcane attacker
            st.board[6].spirit.as_mut().unwrap().attack = 30;
            put_spirit(st, 11, id_of("Cloudling"), Seat::A); // bonded defender
            st.board[11].spirit.as_mut().unwrap().defense = 20;
            st.board[11].spirit.as_mut().unwrap().attack = 0;
            put_spirit(st, 16, id_of("Cloudling"), Seat::A); // bonded partner
            if with_oathbound {
                st.bonds.push(Bond {
                    card: id_of("Oathbound"),
                    owner: Seat::A,
                    tile_a: 11,
                    tile_b: 16,
                });
            }
        }
        let hp0 = e.state().board[11].spirit.as_ref().unwrap().hp;
        e.resolve_engage_for_test(6, 11);
        hp0 - e.state().board[11]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(0)
    };
    assert_eq!(
        dmg(false) - dmg(true),
        20,
        "Oathbound's aura-Warded keeps the defender's 20 Defense against Arcane"
    );
}

#[test]
fn oathbound_warded_grant_is_not_credited_to_movement_keywords() {
    // Oathbound grants Warded — a COMBAT keyword — so the grant must NOT be
    // credited as a movement keyword (it doesn't masquerade as Mobile/Steadfast).
    assert!(!pair_grants("Oathbound", Keyword::Mobile, true));
    assert!(!pair_grants("Oathbound", Keyword::Steadfast, true));
}

#[test]
fn rondel_shares_the_higher_defense_across_a_bonded_pair() {
    // Rondel, the Joining (Static · BondedPair · StatShareHigher{def}) is a SPIRIT, not
    // a bond card — while it stands, every one of the owner's bonded pairs takes the
    // higher Defense of the two. (Doc: "Bonded pairs share the higher Defense".)
    let cat = canon_catalog();
    let def_at_11 = |rondel: bool| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Cloudling"), Seat::A);
            put_spirit(st, 12, id_of("Cloudling"), Seat::A);
            st.board[11].spirit.as_mut().unwrap().defense = 10;
            st.board[12].spirit.as_mut().unwrap().defense = 50;
            st.bonds.push(Bond {
                card: id_of("Rivals' Pact"),
                owner: Seat::A,
                tile_a: 11,
                tile_b: 12,
            });
            if rondel {
                put_spirit(st, 6, id_of("Rondel, the Joining"), Seat::A);
            }
        }
        combat_stats_for_test(e.state(), &cat, 11).defense
    };
    assert_eq!(
        def_at_11(false),
        10,
        "without Rondel the low-Defense end keeps its own"
    );
    assert_eq!(
        def_at_11(true),
        50,
        "Rondel shares the pair's higher Defense to both ends"
    );
}

// ── Throughline subsystem ───────────────────────────────────────────────
fn play_cloudling_at(e: &mut Engine, tile: u8, card: &str) {
    e.state_mut_for_test().player_a.first_placement_done = true;
    e.state_mut_for_test().player_a.hand = vec![id_of(card)];
    e.state_mut_for_test().player_a.anima = 9;
    e.apply(
        Seat::A,
        Command::PlaySpirit {
            hand_index: 0,
            tile,
            engage: None,
            chain_prefs: Vec::new(),
        },
    )
    .unwrap();
}

#[test]
fn throughline_completes_for_a_straight_line_of_three() {
    // A straight orthogonal line of 3 allied spirits sharing an Imprint completes — the
    // completing spirit gains +10/+10 and a full restore.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 5, id_of("Cloudling"), Seat::A);
        put_spirit(st, 6, id_of("Cloudling"), Seat::A);
    }
    let base = e.card(id_of("Cloudling")).attack;
    play_cloudling_at(&mut e, 7, "Cloudling");
    let sp = e.state().board[7].spirit.as_ref().unwrap();
    assert!(sp.throughline_done, "the 3-line completed a Throughline");
    assert_eq!(
        sp.attack,
        base + 10,
        "the completing spirit gained +10 Attack"
    );
}

#[test]
fn errata_joins_a_throughline_as_any_imprint() {
    // Errata "counts as every Imprint" — it completes a line of two Storm spirits.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 5, id_of("Cloudling"), Seat::A);
        put_spirit(st, 6, id_of("Cloudling"), Seat::A);
    }
    play_cloudling_at(&mut e, 7, "Errata");
    assert!(
        e.state().board[7].spirit.as_ref().unwrap().throughline_done,
        "Errata (wildcard) completed the Throughline"
    );
}

#[test]
fn twin_telling_pools_imprints_to_complete_a_throughline() {
    // Twin Telling: the bonded middle spirit (Wanderer) pools its partner's Storm, so the
    // 5-6-7 line shares Storm and completes; without the bond it does not.
    let completed = |bonded: bool| -> bool {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 5, id_of("Cloudling"), Seat::A); // Storm
            put_spirit(st, 6, id_of("Moth of Small Hours"), Seat::A); // Wanderer
            if bonded {
                st.bonds.push(Bond {
                    card: id_of("Twin Telling"),
                    owner: Seat::A,
                    tile_a: 5,
                    tile_b: 6,
                });
            }
        }
        play_cloudling_at(&mut e, 7, "Cloudling"); // Storm
        e.state().board[7].spirit.as_ref().unwrap().throughline_done
    };
    assert!(!completed(false), "no shared Imprint without Twin Telling");
    assert!(
        completed(true),
        "Twin Telling pooled Storm — the line completed"
    );
}

#[test]
fn unbreakable_bridges_a_one_tile_gap_in_a_throughline() {
    // Unbreakable: a 1-tile gap (tile 6) is bridged by the 5–7 bond, so 5-[gap]-7-8 reads
    // as a 3-line and completes; without the bond it does not.
    let completed = |bonded: bool| -> bool {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 5, id_of("Cloudling"), Seat::A);
            put_spirit(st, 7, id_of("Cloudling"), Seat::A);
            if bonded {
                st.bonds.push(Bond {
                    card: id_of("Unbreakable"),
                    owner: Seat::A,
                    tile_a: 5,
                    tile_b: 7,
                });
            }
        }
        play_cloudling_at(&mut e, 8, "Cloudling");
        e.state().board[8].spirit.as_ref().unwrap().throughline_done
    };
    assert!(
        !completed(false),
        "the gap breaks the line without Unbreakable"
    );
    assert!(
        completed(true),
        "Unbreakable bridged the gap — the line completed"
    );
}

#[test]
fn queen_of_the_quiet_garden_amplifies_the_throughline_buff() {
    // Queen (Static/Owner/ThroughlineGrant{+10/+10}): your Throughline completion grants
    // +20/+20 instead of +10/+10.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 5, id_of("Cloudling"), Seat::A);
        put_spirit(st, 6, id_of("Cloudling"), Seat::A);
        put_spirit(st, 0, id_of("Queen of the Quiet Garden"), Seat::A); // a standing Queen
    }
    let base = e.card(id_of("Cloudling")).attack;
    play_cloudling_at(&mut e, 7, "Cloudling");
    assert_eq!(
        e.state().board[7].spirit.as_ref().unwrap().attack,
        base + 20,
        "Queen amplified the Throughline buff to +20"
    );
}

#[test]
fn vale_eternal_rewards_a_throughline_completion() {
    // Vale Eternal (OnThroughlineComplete/Owner/Draw{2}+AnimaDelta{2}): completing a
    // Throughline draws 2 and gains 2 Anima for its owner.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 5, id_of("Cloudling"), Seat::A);
        put_spirit(st, 6, id_of("Cloudling"), Seat::A);
        put_spirit(st, 0, id_of("Vale Eternal, the Standing Ovation"), Seat::A);
        st.player_a.first_placement_done = true;
        st.player_a.hand = vec![id_of("Cloudling")];
        st.player_a.anima = 9;
    }
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: 7,
                engage: None,
                chain_prefs: Vec::new(),
            },
        )
        .unwrap();
    let draws = evs
        .iter()
        .filter(|ev| matches!(ev, Event::CardDrawn { seat: Seat::A }))
        .count();
    assert_eq!(draws, 2, "Vale Eternal drew 2 on Throughline completion");
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::AnimaGained {
                seat: Seat::A,
                amount: 2,
                ..
            }
        )),
        "Vale Eternal gained 2 Anima on completion"
    );
}

#[test]
fn unbreakable_bond_survives_a_one_tile_separation() {
    // A normal Bond breaks once the pair is 1 tile apart; Unbreakable survives it (and so
    // keeps the Throughline gap-bridge alive in real play).
    let survives = |unbreakable: bool| -> bool {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 5, id_of("Cloudling"), Seat::A);
            put_spirit(st, 7, id_of("Cloudling"), Seat::A); // manhattan 2 — a 1-tile gap
            st.bonds.push(Bond {
                card: id_of(if unbreakable {
                    "Unbreakable"
                } else {
                    "Rivals' Pact"
                }),
                owner: Seat::A,
                tile_a: 5,
                tile_b: 7,
            });
        }
        e.apply(Seat::A, Command::EndTurn).unwrap(); // the Flow runs the bond-break sweep
        e.state()
            .bonds
            .iter()
            .any(|b| b.tile_a == 5 && b.tile_b == 7)
    };
    assert!(
        !survives(false),
        "a normal bond breaks at a 1-tile separation"
    );
    assert!(survives(true), "Unbreakable survives a 1-tile separation");
}

#[test]
fn unbreakable_makes_a_chorus_partner_count_as_adjacent() {
    // Chorus counts adjacent allies; an Unbreakable-bonded partner across a 1-tile gap counts.
    let cat = canon_catalog();
    let atk = |unbreakable: bool| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Chorus Vole"), Seat::A);
            put_spirit(st, 13, id_of("Cloudling"), Seat::A); // manhattan 2 from 11
            if unbreakable {
                st.bonds.push(Bond {
                    card: id_of("Unbreakable"),
                    owner: Seat::A,
                    tile_a: 11,
                    tile_b: 13,
                });
            }
        }
        combat_stats_for_test(e.state(), &cat, 11).attack
    };
    assert!(
        atk(true) > atk(false),
        "Unbreakable makes the gap-partner count for Chorus ({} vs {})",
        atk(false),
        atk(true)
    );
}

// ── Combat-redirect / counter / chip precision (mutation killers) ───────────
// These pin the EXACT redirect TARGET + amount, the precise counter damage, and the
// chip value for the bond-combat helpers in `engine/combat.rs`
// (`damage_redirect_to_partner`, `conspiracy_counter`, `pack_tactics_chip`). The
// existing tests above assert direction (differential / inequality); these assert the
// number, so a mutation of the arithmetic or the lethality/adjacency guards changes a
// value an assertion reads. Spirits are placed via `put_spirit` (attack overridden,
// defense 0, HP 40), so the arithmetic is clean: dmg = attacker-attack, no edge
// (Cloudling vs Cloudling is Wonder vs Wonder).

#[test]
fn grief_split_redirects_exactly_the_overflow_to_the_partner() {
    // RedirectDamageToPartner (L168 `dmg - (hp - 1)`): a 50-attack blow on a 40-HP bonded
    // Cloudling overflows by 50 − (40 − 1) = 11. The bonded spirit clings to 1; the
    // partner absorbs EXACTLY 11 (40 → 29). The DamageRedirected event carries amount 11.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 50; // dmg = 50 (def 0), lethal on 40
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // bonded defender (40 HP)
        st.board[12].spirit.as_mut().unwrap().attack = 0; // no retaliation noise
        put_spirit(st, 13, id_of("Cloudling"), Seat::A); // partner (12~13 adjacent)
        st.bonds.push(Bond {
            card: id_of("Grief Split"),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 13,
        });
    }
    let evs = e.resolve_engage_for_test(11, 12);
    let redirect = evs
        .iter()
        .find_map(|ev| match ev {
            Event::DamageRedirected {
                to_tile,
                amount,
                consume,
                ..
            } => Some((*to_tile, *amount, *consume)),
            _ => None,
        })
        .expect("Grief Split redirected the overflow");
    assert_eq!(
        redirect,
        (13, 11, false),
        "overflow 50 − (40 − 1) = 11 goes to the partner at tile 13; Grief Split is repeatable"
    );
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        1,
        "the bonded spirit clings on at exactly 1"
    );
    assert_eq!(
        e.state().board[13].spirit.as_ref().unwrap().hp,
        29,
        "the partner absorbed exactly 11 (40 → 29)"
    );
}

#[test]
fn promise_redirects_the_whole_blow_and_is_spent() {
    // Replace(PartnerTakesIt), OncePerMatch (L161 `Some((partner, dmg, true))`): the
    // partner takes the FULL 50, the saved spirit is untouched, the Promise is consumed.
    // The partner is given 60 HP so it survives to pin the exact post-blow HP (60 − 50 = 10).
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 50; // dmg = 50, lethal on the 40-HP saved spirit
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // bonded defender (saved)
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 13, id_of("Cloudling"), Seat::A); // bonded partner (12~13 adjacent)
        st.board[13].spirit.as_mut().unwrap().hp = 60; // survives the full 50 to a pinnable HP
        st.bonds.push(Bond {
            card: id_of("Promise"),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 13,
        });
    }
    let def_hp0 = e.state().board[12].spirit.as_ref().unwrap().hp;
    let evs = e.resolve_engage_for_test(11, 12);
    let redirect = evs
        .iter()
        .find_map(|ev| match ev {
            Event::DamageRedirected {
                to_tile,
                amount,
                consume,
                ..
            } => Some((*to_tile, *amount, *consume)),
            _ => None,
        })
        .expect("Promise redirected the blow");
    assert_eq!(
        redirect,
        (13, 50, true),
        "the WHOLE 50 goes to the partner at 13; the Promise is spent (consume = true)"
    );
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        def_hp0,
        "the saved spirit is untouched"
    );
    assert_eq!(
        e.state().board[13].spirit.as_ref().unwrap().hp,
        10,
        "the partner took the full 50 (60 → 10)"
    );
    assert!(
        e.state().board[12]
            .spirit
            .as_ref()
            .unwrap()
            .replacement_used,
        "the OncePerMatch Promise is now marked used on the saved spirit"
    );
}

#[test]
fn grief_split_redirects_even_an_exactly_lethal_blow() {
    // The lethality gate L129 `dmg < hp`: at EXACTLY lethal (dmg == hp) the redirect must
    // still fire (the blow IS lethal). A 40-attack blow on a 40-HP bonded Cloudling ⇒
    // dmg == hp; Grief Split overflow = 40 − (40 − 1) = 1. A `<`→`==` or `<`→`<=` flip
    // would treat the exactly-lethal blow as non-lethal and skip the redirect, banishing
    // the bonded spirit instead of leaving it at 1.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 40; // dmg == 40 == defender HP
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // bonded defender (40 HP)
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 13, id_of("Cloudling"), Seat::A); // partner
        st.bonds.push(Bond {
            card: id_of("Grief Split"),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 13,
        });
    }
    let evs = e.resolve_engage_for_test(11, 12);
    let redirect = evs.iter().find_map(|ev| match ev {
        Event::DamageRedirected { amount, .. } => Some(*amount),
        _ => None,
    });
    assert_eq!(
        redirect,
        Some(1),
        "an exactly-lethal blow still redirects: overflow 40 − (40 − 1) = 1"
    );
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        1,
        "the bonded spirit survives the exactly-lethal blow at 1"
    );
    assert!(
        !e.state().board[12].spirit.as_ref().unwrap().fading,
        "and is NOT banished"
    );
}

#[test]
fn a_non_lethal_blow_is_not_redirected_to_the_partner() {
    // The other side of L129 `dmg < hp`: a blow that is NOT lethal (dmg < hp) leaves the
    // ordinary strike standing — no redirect, the partner is untouched. A 30-attack blow
    // on a 40-HP bonded Cloudling ⇒ dmg 30 < 40: defender just takes 30 (→ 10), partner
    // keeps its 40, and there is NO DamageRedirected event.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 30; // dmg 30 < 40: not lethal
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // bonded defender
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 13, id_of("Cloudling"), Seat::A); // partner
        st.bonds.push(Bond {
            card: id_of("Grief Split"),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 13,
        });
    }
    let evs = e.resolve_engage_for_test(11, 12);
    assert!(
        !evs.iter()
            .any(|ev| matches!(ev, Event::DamageRedirected { .. })),
        "a non-lethal blow is not redirected"
    );
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        10,
        "the bonded spirit simply takes the 30 (40 → 10)"
    );
    assert_eq!(
        e.state().board[13].spirit.as_ref().unwrap().hp,
        40,
        "the partner is untouched"
    );
}

#[test]
fn pack_tactics_chips_exactly_ten_to_the_target() {
    // GrantEngage{pre_chip} (L36 guard `*pre_chip > 0 && selector == BondedPair`, L49
    // `Some(chip)`): the partner's pre-chip is EXACTLY 10. With both attacker and target
    // at 0 attack, the only damage is the chip: the 40-HP target drops to exactly 30, via
    // an EffectDamaged event of amount 10. The target SURVIVES the chip, so the engage
    // exchange still proceeds (a Struck event) and the target is not fading — this pins the
    // chip-fells guard L208-210 `s.hp <= 0 && !s.fading`: an `&&`→`||` flip would treat the
    // surviving 30-HP target as felled, banish it, and skip the strike.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 0; // isolate the chip from the strike
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // partner (reaches 13)
        put_spirit(st, 13, id_of("Cloudling"), Seat::B); // target
        st.board[13].spirit.as_mut().unwrap().attack = 0;
        st.bonds.push(Bond {
            card: id_of("Pack Tactics"),
            owner: Seat::A,
            tile_a: 11,
            tile_b: 12,
        });
    }
    let evs = e.resolve_engage_for_test(11, 13);
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::Struck {
                to_tile: 13,
                kind: recollect_core::state::StrikeKind::Engage,
                ..
            }
        )),
        "the target survived the chip, so the engage strike still resolves"
    );
    assert!(
        !e.state().board[13].spirit.as_ref().unwrap().fading,
        "the chip (10 on 40) did not fell the target"
    );
    let chip = evs.iter().find_map(|ev| match ev {
        Event::EffectDamaged { tile, amount } if *tile == 13 => Some(*amount),
        _ => None,
    });
    assert_eq!(
        chip,
        Some(10),
        "Pack Tactics chips exactly 10 to the target"
    );
    assert_eq!(
        e.state().board[13].spirit.as_ref().unwrap().hp,
        30,
        "the target drops to exactly 30 (40 − 10 chip, the engage adds 0)"
    );
}

#[test]
fn conspiracy_counter_engages_the_attacker_for_the_exact_amount() {
    // GrantEngage{immediate:true} (conspiracy_counter): the bonded partner immediately
    // counter-engages the attacker. The partner (attack 50, def 0) bites the attacker for
    // EXACTLY 50 — pinned via the Engage Struck event FROM the partner's tile. The blow is
    // lethal on the attacker's 40 HP, so it is banished on the counter itself (no momentum
    // link follows — there is no surviving target). Only the bond reaches the attacker.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 6, id_of("Cloudling"), Seat::B); // attacker (40 HP)
        st.board[6].spirit.as_mut().unwrap().attack = 0; // no engage/retaliation damage
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // bonded defender (engaged)
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 7, id_of("Cloudling"), Seat::A); // partner: 7~12 adjacent, reaches 6
        st.board[7].spirit.as_mut().unwrap().attack = 50; // counter = 50, lethal on 40
        st.bonds.push(Bond {
            card: id_of("Conspiracy"),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 7,
        });
    }
    let evs = e.resolve_engage_for_test(6, 12);
    // The counter is an Engage strike FROM the partner (tile 7) onto the attacker (tile 6).
    let counter = evs
        .iter()
        .find_map(|ev| match ev {
            Event::Struck {
                from_tile: 7,
                to_tile: 6,
                damage,
                kind: recollect_core::state::StrikeKind::Engage,
                ..
            } => Some(*damage),
            _ => None,
        })
        .expect("the partner counter-engaged the attacker");
    assert_eq!(
        counter, 50,
        "the counter-engage deals exactly 50 (attack 50 − defense 0)"
    );
    assert!(
        e.state().board[6]
            .spirit
            .as_ref()
            .map(|s| s.fading)
            .unwrap_or(true),
        "the 50 counter banished the 40-HP attacker"
    );
}

// ── Bond-helper guard precision (mutation killers) ──────────────────────────
// The exact-amount tests above leave the bond `find` predicate, the present-/fading-
// gates, and the clause-selection guards untested because they always place the keyed
// spirit on `tile_a` with both ends standing. These pin those guards directly: each
// asserts a real rule (the pre-chip / counter / redirect fires for the MIRRORED bond
// orientation, and does NOT fire when an end is gone or the bond is the wrong kind).

#[test]
fn pack_tactics_chips_when_the_engaging_spirit_is_the_bonds_second_end() {
    // The bond `find` predicate `b.tile_a == att || b.tile_b == att` (L16): the pre-chip
    // must fire whichever END of the bond engages. Here the attacker is `tile_b`, so a
    // `b.tile_b == att`→`!=` flip would fail to find the bond and skip the chip.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // attacker = the bond's tile_b
        st.board[11].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // partner (reaches 13)
        put_spirit(st, 13, id_of("Cloudling"), Seat::B); // target
        st.board[13].spirit.as_mut().unwrap().attack = 0;
        st.bonds.push(Bond {
            card: id_of("Pack Tactics"),
            owner: Seat::A,
            tile_a: 12, // partner is tile_a …
            tile_b: 11, // … and the attacker is tile_b
        });
    }
    let evs = e.resolve_engage_for_test(11, 13);
    assert_eq!(
        evs.iter().find_map(|ev| match ev {
            Event::EffectDamaged { tile, amount } if *tile == 13 => Some(*amount),
            _ => None,
        }),
        Some(10),
        "the pre-chip fires when the engaging spirit is the bond's second end"
    );
}

#[test]
fn pack_tactics_does_not_chip_when_the_partner_is_gone() {
    // The present-/fading-gate `!present(a) || !present(b)` (L18–19): if an end has Faded,
    // the bond grants nothing. With the partner marked fading, there must be NO pre-chip —
    // a `delete !` on the fading check would chip from the absent partner anyway.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // partner — but FADING (gone)
        st.board[12].spirit.as_mut().unwrap().fading = true;
        put_spirit(st, 13, id_of("Cloudling"), Seat::B); // target
        st.board[13].spirit.as_mut().unwrap().attack = 0;
        st.bonds.push(Bond {
            card: id_of("Pack Tactics"),
            owner: Seat::A,
            tile_a: 11,
            tile_b: 12,
        });
    }
    let hp0 = e.state().board[13].spirit.as_ref().unwrap().hp;
    let evs = e.resolve_engage_for_test(11, 13);
    assert!(
        !evs.iter()
            .any(|ev| matches!(ev, Event::EffectDamaged { tile: 13, .. })),
        "a faded partner grants no pre-chip"
    );
    assert_eq!(
        e.state().board[13].spirit.as_ref().unwrap().hp,
        hp0,
        "the target is untouched (attacker attack 0, no chip)"
    );
}

#[test]
fn pack_tactics_chip_does_not_fire_for_a_counter_only_bond() {
    // The clause-selection guard `*pre_chip > 0 && selector == BondedPair` (L36): a
    // Conspiracy bond is an IMMEDIATE counter-engage with `pre_chip: 0`, so it grants no
    // pre-chip. With the guard forced `true` (or `>`→`>=`, since 0 >= 0), the function
    // would match Conspiracy's GrantEngage and chip 0 — an EffectDamaged{0} that must NOT
    // appear. (The engaged target is NOT bonded, so no counter-engage fires here either.)
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // partner (reaches 13)
        put_spirit(st, 13, id_of("Cloudling"), Seat::B); // engaged target (unbonded)
        st.board[13].spirit.as_mut().unwrap().attack = 0;
        st.bonds.push(Bond {
            card: id_of("Conspiracy"), // immediate GrantEngage, pre_chip 0 — NOT a pre-chip bond
            owner: Seat::A,
            tile_a: 11,
            tile_b: 12,
        });
    }
    let evs = e.resolve_engage_for_test(11, 13);
    assert!(
        !evs.iter()
            .any(|ev| matches!(ev, Event::EffectDamaged { .. })),
        "a counter-only (pre_chip 0) bond produces no pre-chip, not even a 0 one"
    );
}

#[test]
fn conspiracy_counters_when_the_engaged_spirit_is_the_bonds_second_end() {
    // The bond `find` predicate `b.tile_a == def || b.tile_b == def` (L67): the counter
    // must fire whichever END is engaged. Here the engaged defender is `tile_b`, so a
    // `b.tile_b == def`→`!=` flip would fail to find the bond and skip the counter.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 6, id_of("Cloudling"), Seat::B); // attacker (40 HP)
        st.board[6].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // engaged defender = the bond's tile_b
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 7, id_of("Cloudling"), Seat::A); // partner: 7~12 adjacent, reaches 6
        st.board[7].spirit.as_mut().unwrap().attack = 50;
        st.bonds.push(Bond {
            card: id_of("Conspiracy"),
            owner: Seat::A,
            tile_a: 7,  // partner is tile_a …
            tile_b: 12, // … and the engaged defender is tile_b
        });
    }
    let evs = e.resolve_engage_for_test(6, 12);
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            Event::Struck {
                from_tile: 7,
                to_tile: 6,
                kind: recollect_core::state::StrikeKind::Engage,
                ..
            }
        )),
        "the counter fires when the engaged spirit is the bond's second end"
    );
    assert!(
        e.state().board[6]
            .spirit
            .as_ref()
            .map(|s| s.fading)
            .unwrap_or(true),
        "and the 50 counter banishes the attacker"
    );
}

#[test]
fn conspiracy_does_not_counter_when_the_partner_is_gone() {
    // The present-/fading-gate in conspiracy_counter (L69–70): a Faded partner cannot
    // counter. With the partner marked fading, the attacker takes NO counter damage — a
    // `delete !` on the fading check would let the absent partner strike anyway.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 6, id_of("Cloudling"), Seat::B); // attacker
        st.board[6].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // engaged defender
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 7, id_of("Cloudling"), Seat::A); // partner — but FADING
        st.board[7].spirit.as_mut().unwrap().attack = 50;
        st.board[7].spirit.as_mut().unwrap().fading = true;
        st.bonds.push(Bond {
            card: id_of("Conspiracy"),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 7,
        });
    }
    let hp0 = e.state().board[6].spirit.as_ref().unwrap().hp;
    e.resolve_engage_for_test(6, 12);
    assert_eq!(
        e.state().board[6].spirit.as_ref().unwrap().hp,
        hp0,
        "a faded partner deals no counter (the attacker is untouched)"
    );
}

#[test]
fn grief_split_redirects_when_the_struck_spirit_is_the_bonds_second_end() {
    // The bond `find` predicate `b.tile_a == def || b.tile_b == def` (L135): the redirect
    // must fire whichever END is struck. Here the bonded defender is `tile_b`, so a
    // `b.tile_b == def`→`!=` flip would fail to find the bond and let the lethal blow land.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 50; // lethal on 40
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // bonded defender = the bond's tile_b
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 13, id_of("Cloudling"), Seat::A); // partner (12~13 adjacent)
        st.bonds.push(Bond {
            card: id_of("Grief Split"),
            owner: Seat::A,
            tile_a: 13, // partner is tile_a …
            tile_b: 12, // … and the struck spirit is tile_b
        });
    }
    let evs = e.resolve_engage_for_test(11, 12);
    assert!(
        evs.iter()
            .any(|ev| matches!(ev, Event::DamageRedirected { to_tile: 13, .. })),
        "the redirect fires when the struck spirit is the bond's second end"
    );
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        1,
        "the struck spirit clings on at 1"
    );
}

#[test]
fn grief_split_does_not_redirect_when_the_partner_is_gone() {
    // The standing-partner gate `!present(partner)` (L141) plus the PairAdjacent guard
    // `present(a) && present(b)` (L165–166): with no standing partner to bear the blow,
    // the redirect is impossible and the lethal blow simply banishes the bonded spirit.
    // A dropped present-check would redirect into the absent partner.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 50; // lethal on 40
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // bonded defender
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 13, id_of("Cloudling"), Seat::A); // partner — but FADING (no bearer)
        st.board[13].spirit.as_mut().unwrap().fading = true;
        st.bonds.push(Bond {
            card: id_of("Grief Split"),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 13,
        });
    }
    let evs = e.resolve_engage_for_test(11, 12);
    assert!(
        !evs.iter()
            .any(|ev| matches!(ev, Event::DamageRedirected { .. })),
        "no standing partner ⇒ no redirect"
    );
    assert!(
        e.state().board[12]
            .spirit
            .as_ref()
            .map(|s| s.fading)
            .unwrap_or(true),
        "the bonded spirit takes the full lethal blow and is banished"
    );
}

#[test]
fn promise_does_not_redirect_once_already_spent() {
    // The Promise guard `OncePerMatch && !saved_used` (L159): a Promise already spent
    // (`replacement_used` set on the saved spirit) cannot fire again. With the flag set,
    // the lethal blow banishes the bonded spirit — a dropped `!saved_used` would let the
    // spent Promise redirect a second time. (No Grief Split here, so the only possible
    // redirect is the Promise.)
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 50; // lethal on 40
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // bonded defender — Promise already used
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        st.board[12].spirit.as_mut().unwrap().replacement_used = true;
        put_spirit(st, 13, id_of("Cloudling"), Seat::A); // partner
        st.bonds.push(Bond {
            card: id_of("Promise"),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 13,
        });
    }
    let evs = e.resolve_engage_for_test(11, 12);
    assert!(
        !evs.iter()
            .any(|ev| matches!(ev, Event::DamageRedirected { .. })),
        "a spent Promise does not redirect again"
    );
    assert!(
        e.state().board[12]
            .spirit
            .as_ref()
            .map(|s| s.fading)
            .unwrap_or(true),
        "the bonded spirit takes the full blow and is banished"
    );
}

#[test]
fn pack_tactics_chip_alone_can_fell_the_target_and_end_the_engage() {
    // full_exchange L208–216: if the pre-chip ALONE fells the target (`hp <= 0 && !fading`),
    // the target is banished by the chip and the engage is over (`return true`) — there is
    // NO strike exchange. A 10-chip on a 10-HP target ⇒ felled by the chip. We assert the
    // target is banished AND no Struck event fired (the early return). A `<=`→`>` or
    // dropped-`!` flip would fall through to the exchange and emit a Struck instead.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // partner (reaches 13)
        put_spirit(st, 13, id_of("Cloudling"), Seat::B); // target at exactly the chip's HP
        st.board[13].spirit.as_mut().unwrap().hp = 10;
        st.bonds.push(Bond {
            card: id_of("Pack Tactics"),
            owner: Seat::A,
            tile_a: 11,
            tile_b: 12,
        });
    }
    let evs = e.resolve_engage_for_test(11, 13);
    assert_eq!(
        evs.iter().find_map(|ev| match ev {
            Event::EffectDamaged { tile: 13, amount } => Some(*amount),
            _ => None,
        }),
        Some(10),
        "the chip lands for 10"
    );
    assert!(
        e.state().board[13]
            .spirit
            .as_ref()
            .map(|s| s.fading)
            .unwrap_or(true),
        "the chip alone felled the target"
    );
    assert!(
        !evs.iter().any(|ev| matches!(ev, Event::Struck { .. })),
        "no strike exchange follows a chip-kill — the engage ended on the chip"
    );
}

#[test]
fn pack_tactics_pre_chip_fires_only_on_the_initial_engage_not_chain_links() {
    // full_exchange L197 `matches!(kind, StrikeKind::Engage) && …`: the pre-chip is an
    // ENGAGE-only opener, not a per-link effect. Resolving a CHAIN strike (link 1) with a
    // Pack Tactics bond present must NOT chip the target. An `&&`→`||` flip would let the
    // pre-chip fire on chain links too — here that would deal an extra 10. Attacker attack
    // is 0, so without the (suppressed) chip the chain deals 0 and the target is untouched.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // chaining attacker
        st.board[11].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // partner (reaches 13)
        put_spirit(st, 13, id_of("Cloudling"), Seat::B); // target
        st.board[13].spirit.as_mut().unwrap().attack = 0;
        st.bonds.push(Bond {
            card: id_of("Pack Tactics"),
            owner: Seat::A,
            tile_a: 11,
            tile_b: 12,
        });
    }
    let hp0 = e.state().board[13].spirit.as_ref().unwrap().hp;
    let evs = e.resolve_chain_for_test(11, 13, 1);
    assert!(
        !evs.iter()
            .any(|ev| matches!(ev, Event::EffectDamaged { tile: 13, .. })),
        "no pre-chip on a chain link"
    );
    assert_eq!(
        e.state().board[13].spirit.as_ref().unwrap().hp,
        hp0,
        "the chain-link target is untouched (no Engage-only pre-chip)"
    );
}

#[test]
fn a_stat_buff_bond_does_not_trigger_a_counter_engage() {
    // conspiracy_counter's `is_counter_bond` test (L86-95): a clause counts only if it is
    // BOTH `selector == BondedPair` AND an immediate `GrantEngage` (`&&`). A plain stat-buff
    // bond (Race You: Static/BondedPair/StatDelta — adjacent, in reach, partner could bite)
    // is NOT a counter-bond, so engaging its bonded end triggers NO counter. An `&&`→`||`
    // flip would accept the BondedPair stat clause as a counter and have the partner strike.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 6, id_of("Cloudling"), Seat::B); // attacker
        st.board[6].spirit.as_mut().unwrap().attack = 0; // no engage/retaliation damage
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // engaged defender
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 7, id_of("Cloudling"), Seat::A); // would-be counter partner: 7~12 adj, reaches 6
        st.board[7].spirit.as_mut().unwrap().attack = 50; // would bite hard IF it counted
        st.bonds.push(Bond {
            card: id_of("Race You"), // BondedPair StatDelta — NOT a counter-engage bond
            owner: Seat::A,
            tile_a: 12,
            tile_b: 7,
        });
    }
    let evs = e.resolve_engage_for_test(6, 12);
    assert!(
        !evs.iter().any(|ev| matches!(
            ev,
            Event::Struck {
                from_tile: 7,
                to_tile: 6,
                ..
            }
        )),
        "a stat-buff bond does not make the partner counter-engage"
    );
    // The 50-attack partner would BANISH the 40-HP attacker if it counter-engaged; without
    // a counter the attacker only takes the defender's own retaliation and survives.
    // (Race You grants +10 Attack to the bonded defender, so its retaliation is a legitimate
    // 10 — not a counter; the attacker stands at 30, decisively not banished.)
    assert!(
        !e.state().board[6].spirit.as_ref().unwrap().fading,
        "the attacker is NOT banished — no 50-damage counter from a non-counter bond"
    );
}

#[test]
fn grief_split_does_not_redirect_when_the_pair_is_not_adjacent() {
    // Grief Split's RedirectDamageToPartner is PairAdjacent-gated (L163-166:
    // `manhattan(a, b) == 1 && present(a) && present(b)`). A bonded pair pulled APART
    // (manhattan > 1) grants no redirect even though both ends still stand. Geometry (5×5):
    // defender@12=(2,2), partner@14=(4,2) ⇒ manhattan 2; both present. A lethal blow on the
    // defender must banish it (no overflow to the distant partner). Forcing the guard `true`
    // (L164) or any `&&`→`||` on the present-checks (L165-166, both already true) would
    // redirect across the gap.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // attacker (reaches 12)
        st.board[11].spirit.as_mut().unwrap().attack = 50; // lethal on 40
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // bonded defender
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 14, id_of("Cloudling"), Seat::A); // partner — bonded but 2 tiles away
        st.bonds.push(Bond {
            card: id_of("Grief Split"),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 14, // manhattan(12, 14) = 2 on the 5×5: the pair is split
        });
    }
    let evs = e.resolve_engage_for_test(11, 12);
    assert!(
        !evs.iter()
            .any(|ev| matches!(ev, Event::DamageRedirected { .. })),
        "a non-adjacent pair grants no Grief Split redirect"
    );
    assert!(
        e.state().board[12]
            .spirit
            .as_ref()
            .map(|s| s.fading)
            .unwrap_or(true),
        "the bonded spirit takes the full lethal blow and is banished"
    );
    assert_eq!(
        e.state().board[14].spirit.as_ref().unwrap().hp,
        40,
        "the distant partner is untouched"
    );
}

// ── full_exchange arithmetic that needs non-zero damage-reduction / retaliation ──
// The vanilla-stat tests in rules.rs leave the `- dmg_reduction` and `+ retaliation`
// terms (zero for plain spirits) unpinned. These give them real values via an allied
// Adamant (DamageReduction aura to allies-in-reach) and a temp_retaliation entry.

#[test]
fn full_exchange_subtracts_the_defenders_damage_reduction() {
    // L272 `- dcs.dmg_reduction`: the defender stands beside an allied Adamant (Static
    // DamageReduction 10 to allies in reach), so it takes 10 LESS. Attacker 30 − 0 def −
    // 10 reduction = 20 ⇒ a 40-HP defender ends at 20. A `-`→`+` flip would ADD 10 (dmg 40)
    // and fell it; `-`→`/` would divide. The attacker's attack is the only strike term.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 30;
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // defender (def 0 via put_spirit)
        st.board[12].spirit.as_mut().unwrap().attack = 0; // no retaliation noise
        put_spirit(st, 13, id_of("Adamant, the Kept Word"), Seat::A); // ally: Cross of 13 covers 12
    }
    let evs = e.resolve_engage_for_test(11, 12);
    let to_def = evs.iter().find_map(|ev| match ev {
        Event::Struck {
            to_tile: 12,
            damage,
            ..
        } => Some(*damage),
        _ => None,
    });
    assert_eq!(
        to_def,
        Some(20),
        "30 attack − 0 defense − 10 Adamant reduction = 20"
    );
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        20,
        "the defender survives at exactly 20 (reduction subtracted, not added)"
    );
}

#[test]
fn full_exchange_adds_the_defenders_retaliation_bonus() {
    // L279 `dcs.atk + dcs.retaliation`: a temp_retaliation entry gives the defender +10
    // retaliation. Its retaliation = 20 attack + 10 retaliation − 0 def = 30 back to the
    // attacker. A `+`→`-` flip on the retaliation term would deal only 10. The attacker
    // (90 HP) survives to pin the exact 30.
    let mut e = engine();
    let round = e.state().round;
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 0; // no strike noise
        st.board[11].spirit.as_mut().unwrap().hp = 90; // survives the retaliation
        st.board[11].spirit.as_mut().unwrap().hp_max = 90;
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // defender
        st.board[12].spirit.as_mut().unwrap().attack = 20;
        st.temp_retaliation = vec![(12, 10, round)]; // +10 retaliation on the defender this round
    }
    let evs = e.resolve_engage_for_test(11, 12);
    let to_att = evs.iter().find_map(|ev| match ev {
        Event::Struck {
            to_tile: 11,
            damage,
            kind: recollect_core::state::StrikeKind::Retaliation,
            ..
        } => Some(*damage),
        _ => None,
    });
    assert_eq!(
        to_att,
        Some(30),
        "20 attack + 10 retaliation − 0 defense = 30"
    );
    assert_eq!(
        e.state().board[11].spirit.as_ref().unwrap().hp,
        60,
        "the attacker drops to exactly 60 (90 − 30)"
    );
}

#[test]
fn full_exchange_subtracts_the_attackers_damage_reduction_from_retaliation() {
    // L281 `- acs.dmg_reduction`: the ATTACKER stands beside an allied Adamant, so the
    // retaliation it takes is reduced by 10. Defender retaliation = 30 attack − 0 def − 10
    // attacker-reduction = 20. A `-`→`+` flip would deal 40 instead. (The attacker's own
    // strike is 0; only the reduced retaliation moves its HP.)
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 6, id_of("Adamant, the Kept Word"), Seat::B); // attacker's ally: Cross of 6 covers 11
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // defender
        st.board[12].spirit.as_mut().unwrap().attack = 30;
    }
    let hp0 = e.state().board[11].spirit.as_ref().unwrap().hp;
    let evs = e.resolve_engage_for_test(11, 12);
    let to_att = evs.iter().find_map(|ev| match ev {
        Event::Struck {
            to_tile: 11,
            damage,
            kind: recollect_core::state::StrikeKind::Retaliation,
            ..
        } => Some(*damage),
        _ => None,
    });
    assert_eq!(
        to_att,
        Some(20),
        "30 retaliation − 0 def − 10 attacker-reduction = 20"
    );
    assert_eq!(
        e.state().board[11].spirit.as_ref().unwrap().hp,
        hp0 - 20,
        "the attacker takes exactly 20 (reduction subtracted from the retaliation)"
    );
}

#[test]
fn grief_split_overflow_banishes_the_partner_at_exactly_lethal() {
    // full_exchange L303-316: after the redirect, the partner is banished iff
    // `p_pre - redirect <= 0`. Tune the overflow to EXACTLY the partner's HP. Attacker 50
    // on a 40-HP defender ⇒ redirect = 50 − (40 − 1) = 11; partner at 11 HP ⇒ 11 − 11 = 0
    // ≤ 0 ⇒ banished. A `-`→`/` flip makes it 11 / 11 = 1 > 0 (partner wrongly survives).
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::B); // attacker
        st.board[11].spirit.as_mut().unwrap().attack = 50; // dmg 50 on 40 ⇒ redirect 11
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // bonded defender
        st.board[12].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 13, id_of("Cloudling"), Seat::A); // partner at exactly the overflow HP
        st.board[13].spirit.as_mut().unwrap().hp = 11;
        st.bonds.push(Bond {
            card: id_of("Grief Split"),
            owner: Seat::A,
            tile_a: 12,
            tile_b: 13,
        });
    }
    let evs = e.resolve_engage_for_test(11, 12);
    assert_eq!(
        evs.iter().find_map(|ev| match ev {
            Event::DamageRedirected { amount, .. } => Some(*amount),
            _ => None,
        }),
        Some(11),
        "the overflow is 11"
    );
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        1,
        "the bonded spirit clings on at 1"
    );
    assert!(
        e.state().board[13]
            .spirit
            .as_ref()
            .map(|s| s.fading)
            .unwrap_or(true),
        "the partner's 11 HP is exactly the overflow ⇒ banished"
    );
}

#[test]
fn common_cause_on_defeat_does_not_fire_when_the_victor_dies_to_retaliation() {
    // full_exchange L336 `if banished && att.hp - dmg_att > 0`: a victor that defeats its
    // target but DIES to the retaliation does NOT fire OnDefeat. Set the victor's HP to
    // exactly the retaliation it takes (40), so `att.hp - dmg_att = 0`, not > 0. With the
    // real `>`, Common Cause's OnDefeat is skipped (no +10 to the partner). A `>`→`>=` flip
    // (or `-`→`+`) would fire it. The Common Cause victor is the bond's tile_b (so this also
    // guards the L361 `tile_b == att_tile` find: `==`→`!=` would miss the bond).
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 16, id_of("Cloudling"), Seat::A); // bonded partner (gets the buff IF it fires)
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // bonded victor = the bond's tile_b
        st.board[11].spirit.as_mut().unwrap().attack = 90; // fells the enemy
        st.board[11].spirit.as_mut().unwrap().hp = 40; // exactly the retaliation it will take
        put_spirit(st, 12, id_of("Cloudling"), Seat::B); // the enemy
        st.board[12].spirit.as_mut().unwrap().hp = 5; // felled by the 90
        st.board[12].spirit.as_mut().unwrap().attack = 40; // retaliation = 40 = the victor's HP
        st.bonds.push(Bond {
            card: id_of("Common Cause"),
            owner: Seat::A,
            tile_a: 16, // partner is tile_a …
            tile_b: 11, // … and the victor is tile_b
        });
    }
    let partner_before = combat_stats_for_test(e.state(), &cat, 16).attack;
    e.resolve_engage_for_test(11, 12);
    assert!(
        e.state().board[12]
            .spirit
            .as_ref()
            .map(|s| s.fading)
            .unwrap_or(true),
        "the enemy was defeated"
    );
    assert!(
        e.state().board[11]
            .spirit
            .as_ref()
            .map(|s| s.fading)
            .unwrap_or(true),
        "but the victor died to the exactly-lethal retaliation"
    );
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, 16).attack,
        partner_before,
        "a dead victor fires no OnDefeat — the partner gets no Common Cause buff"
    );
}

// ── momentum_prefs bonus sub-terms that need a MomentumMod card ──────────────
// The vanilla 2-link chain test in rules.rs pins `MOMENTUM_PER_LINK * link`, but the
// per-link and first-engage MomentumMod bonuses are zero for plain spirits. Sparkfather
// (first_engage_bonus) and Embermane (per_link_bonus 10) give them real values.

#[test]
fn momentum_first_engage_bonus_doubles_the_first_links_multiplier() {
    // momentum_prefs L655-661: with `momentum_first_bonus` set (Sparkfather Vermilion), the
    // FIRST link's multiplier is `link + 1 = 2`, so its bonus is MOMENTUM_PER_LINK * 2 = 20.
    // Chain link 1 = 80 attack + 20 bonus − 0 defense (put_spirit targets have defense 0) = 100.
    // Kills L657:54 `link == 1`→`!=` (would drop the +1 ⇒ bonus 10 ⇒ 90) and L657:17
    // `link + …`→`link - …` (⇒ bonus 0 ⇒ 80).
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.player_a.first_placement_done = false; // first placement ⇒ home row (y=0) is legal
        st.player_a.hand = vec![id_of("Sparkfather Vermilion")];
        st.player_a.anima = 20;
        put_spirit(st, 7, id_of("Cloudling"), Seat::B); // engage target (2,1)=tile 7: felled by the 80
        st.board[7].spirit.as_mut().unwrap().hp = 20;
        st.board[7].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 1, id_of("Cinderling"), Seat::B); // chain target (1,0)=tile 1, Fury (no edge): Cross of (2,0) reaches it
        st.board[1].spirit.as_mut().unwrap().hp = 90;
        st.board[1].spirit.as_mut().unwrap().attack = 0;
    }
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: 2, // (2,0): home row
                engage: Some(7),
                chain_prefs: vec![1],
            },
        )
        .unwrap();
    let link1 = evs.iter().find_map(|ev| match ev {
        Event::Struck {
            to_tile: 1,
            damage,
            kind: recollect_core::state::StrikeKind::Chain(1),
            ..
        } => Some(*damage),
        _ => None,
    });
    assert_eq!(
        link1,
        Some(100),
        "first-engage bonus: 80 attack + (10 * 2) bonus − 0 defense = 100"
    );
}

#[test]
fn momentum_per_link_bonus_raises_every_links_increment() {
    // momentum_prefs L655 `MOMENTUM_PER_LINK + cs.momentum_per_link_bonus`: Embermane adds
    // +10 per link, so link 1's bonus is (10 + 10) * 1 = 20. Chain link 1 (Lance reaches the
    // forward tile) = 60 attack + 20 bonus − 0 defense (put_spirit targets have defense 0) = 80.
    // A `+`→`-` flip makes the increment (10 − 10) * 1 = 0 ⇒ damage 60. Embermane's
    // chain_while_defeating lets the non-Relentless cat chain at all.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.player_a.first_placement_done = false; // first placement ⇒ home row (y=0) is legal
        st.player_a.hand = vec![id_of("Embermane, First of the Pride")];
        st.player_a.anima = 20;
        put_spirit(st, 7, id_of("Cloudling"), Seat::B); // engage target (2,1)=tile 7: felled by the 60
        st.board[7].spirit.as_mut().unwrap().hp = 20;
        st.board[7].spirit.as_mut().unwrap().attack = 0;
        put_spirit(st, 12, id_of("Cinderling"), Seat::B); // chain target (2,2)=tile 12, Fury (no edge): Lance of (2,0) reaches it
        st.board[12].spirit.as_mut().unwrap().hp = 90;
        st.board[12].spirit.as_mut().unwrap().attack = 0;
    }
    let evs = e
        .apply(
            Seat::A,
            Command::PlaySpirit {
                hand_index: 0,
                tile: 2, // (2,0): home row
                engage: Some(7),
                chain_prefs: vec![12],
            },
        )
        .unwrap();
    let link1 = evs.iter().find_map(|ev| match ev {
        Event::Struck {
            to_tile: 12,
            damage,
            kind: recollect_core::state::StrikeKind::Chain(1),
            ..
        } => Some(*damage),
        _ => None,
    });
    assert_eq!(
        link1,
        Some(80),
        "per-link bonus: 60 attack + (10 + 10) bonus − 0 defense = 80"
    );
}

#[test]
fn an_enemy_lullaby_suppresses_an_attackers_strike_echo() {
    // full_exchange L230-232 `att.echo_eligible() && !echo_suppressed(att_tile) && draw_below`:
    // an attacker at Echo (below half HP) adjacent to an ENEMY Lullaby is too calm to Echo, so
    // its STRIKE never gets the +20 across all seeds. A dropped `!` would invert suppression
    // and let it echo ~20% of the time. The attacker (Cloudling, HP forced to 1 ⇒ deep Echo)
    // strikes a defender; an adjacent enemy Lullaby denies the variance.
    let mut struck = 0;
    for seed in 0..150u64 {
        let cat = canon_catalog();
        let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
        let (mut e, _) = Engine::new(seed, cat, deck.clone(), deck);
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Cloudling"), Seat::A); // attacker
            st.board[11].spirit.as_mut().unwrap().hp = 1; // deep at Echo (echo-eligible)
            put_spirit(st, 12, id_of("Cloudling"), Seat::B); // defender (engaged)
            st.board[12].spirit.as_mut().unwrap().attack = 0; // no retaliation noise
            st.board[12].spirit.as_mut().unwrap().hp = 90; // survives so the strike value is readable
            put_spirit(st, 6, id_of("The Lullaby"), Seat::B); // enemy Lullaby adjacent to (1,1)~(1,2)
        }
        let evs = e.resolve_engage_for_test(11, 12);
        for s in evs.iter() {
            if let Event::Struck {
                from_tile: 11,
                echo,
                ..
            } = s
            {
                struck += 1;
                assert!(
                    !echo,
                    "a suppressed attacker never echoes its strike (seed {seed})"
                );
            }
        }
    }
    assert!(
        struck > 0,
        "the attacker struck at least once across the seeds"
    );
}

#[test]
fn common_cause_bond_on_defeat_fires_when_the_victor_is_the_bonds_second_end() {
    // full_exchange L358-374: a SURVIVING bonded victor also fires its BOND's OnDefeat. The
    // bond is found by `b.tile_a == att_tile || b.tile_b == att_tile` (L361); here the victor
    // is the bond's tile_b, so a `b.tile_b == att`→`!=` flip would miss the bond and skip the
    // +10 Common Cause buff. The victor stays at full HP (the enemy has 0 attack), so the
    // OnDefeat branch runs.
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 16, id_of("Cloudling"), Seat::A); // bonded partner = tile_a
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // bonded victor = tile_b (full HP)
        st.board[11].spirit.as_mut().unwrap().attack = 90; // fells the enemy
        put_spirit(st, 12, id_of("Cloudling"), Seat::B); // the enemy
        st.board[12].spirit.as_mut().unwrap().hp = 5;
        st.board[12].spirit.as_mut().unwrap().attack = 0; // no retaliation ⇒ the victor survives
        st.bonds.push(Bond {
            card: id_of("Common Cause"),
            owner: Seat::A,
            tile_a: 16, // partner is tile_a …
            tile_b: 11, // … and the victor is tile_b
        });
    }
    let partner_before = combat_stats_for_test(e.state(), &cat, 16).attack;
    e.resolve_engage_for_test(11, 12);
    assert!(
        e.state().board[12]
            .spirit
            .as_ref()
            .map(|s| s.fading)
            .unwrap_or(true),
        "the enemy was defeated"
    );
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, 16).attack,
        partner_before + 10,
        "the bond's OnDefeat fired (found via the victor's tile_b end): +10 to the partner"
    );
}
