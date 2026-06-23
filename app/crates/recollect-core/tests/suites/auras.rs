//! Static auras / standing-spirit grants (split from bond_auras.rs — bonds+throughline stay there).
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
fn oathkeeper_adamant_grants_adjacent_allies_steadfast() {
    // Static/AdjacentAlliesAll/GrantKeyword{Steadfast}: a standing Oathkeeper makes its
    // adjacent allies Steadfast.
    let cat = canon_catalog();
    let steadfast = |oathkeeper: bool| -> bool {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Cloudling"), Seat::A);
            if oathkeeper {
                put_spirit(st, 12, id_of("Oathkeeper Adamant"), Seat::A); // adjacent to 11
            }
        }
        keyword_active_for_test(e.state(), &cat, 11, Keyword::Steadfast)
    };
    assert!(!steadfast(false), "no Steadfast without Oathkeeper");
    assert!(
        steadfast(true),
        "Oathkeeper grants adjacent allies Steadfast"
    );
}

#[test]
fn warden_breaker_grows_from_defeating_a_warded_enemy() {
    // OnDefeatWarded/SelfSpirit/StatDelta{+20/+20}: defeating a WARDED enemy buffs the victor;
    // defeating a non-Warded enemy does not.
    let grew = |warded: bool| -> i16 {
        let mut e = engine();
        let enemy = if warded { "Warded Ram" } else { "Cloudling" };
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Warden-Breaker, Crowned in Smoke"), Seat::A);
            put_spirit(st, 12, id_of(enemy), Seat::B);
            let e12 = st.board[12].spirit.as_mut().unwrap();
            e12.hp = 1;
            e12.attack = 0;
            e12.defense = 0;
        }
        let base = e.state().board[11].spirit.as_ref().unwrap().attack;
        e.resolve_engage_for_test(11, 12);
        e.state().board[11]
            .spirit
            .as_ref()
            .map(|s| s.attack)
            .unwrap_or(base)
            - base
    };
    assert_eq!(grew(false), 0, "no buff from defeating a non-Warded enemy");
    assert_eq!(
        grew(true),
        20,
        "Warden-Breaker grew +20 from defeating a Warded enemy"
    );
}

#[test]
fn herald_of_the_ill_intent_burns_enemies_pushed_onto_impressions() {
    // Static/Owner/ImpressionPushDamage{10}: while a Herald stands, an enemy shoved onto a
    // Impression-bearing tile takes 10 (here Vertigo pushes the engage survivor onto an impression).
    let dmg = |herald: bool| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Vertigo, Who Loves the Long Fall"), Seat::A);
            put_spirit(st, 12, id_of("Cloudling"), Seat::B);
            let d = st.board[12].spirit.as_mut().unwrap();
            d.hp = 500;
            d.attack = 0;
            st.board[13].impressions = vec![Seat::A]; // the push destination is Impression-bearing
            if herald {
                put_spirit(st, 0, id_of("Herald of the Ill Intent"), Seat::A);
            }
        }
        e.resolve_engage_for_test(11, 12); // Vertigo shoves the survivor (12) to 13
        500 - e.state().board[13]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(500)
    };
    assert_eq!(
        dmg(true) - dmg(false),
        10,
        "Herald adds 10 to an enemy pushed onto an impression"
    );
}

#[test]
fn whisperer_at_the_door_weakens_its_attacker() {
    // Static/SelfSpirit/EngagerAttackDelta{-10}: an enemy engaging the Whisperer strikes
    // for 10 less than it would a plain target of the same Defense.
    let dmg = |target: &str| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Cloudling"), Seat::A); // attacker
            put_spirit(st, 12, id_of(target), Seat::B);
            let d = st.board[12].spirit.as_mut().unwrap();
            d.hp = 500;
            d.defense = 0;
            d.attack = 0;
        }
        e.resolve_engage_for_test(11, 12);
        500 - e.state().board[12]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(500)
    };
    assert_eq!(
        dmg("Cloudling") - dmg("Whisperer at the Door"),
        10,
        "the Whisperer reduces its attacker's Attack by 10"
    );
}

#[test]
fn elegist_wren_grows_when_an_ally_parts_in_reach() {
    // OnAllyPartsInReach/SelfSpirit/StatDelta{+10/+10}: an ally Parting within the Wren's
    // reach grows the Wren.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Elegist Wren"), Seat::A);
        put_spirit(st, 6, id_of("Cloudling"), Seat::A); // ally in the Wren's Veil reach, fading
        st.board[6].spirit.as_mut().unwrap().fading = true;
    }
    let base = e.state().board[11].spirit.as_ref().unwrap().attack;
    e.force_fade_step_for_test(Seat::A); // the ally dissolves → its Parting fires
    assert_eq!(
        e.state().board[11].spirit.as_ref().unwrap().attack,
        base + 10,
        "Elegist Wren grew when an ally Parted in reach"
    );
}

#[test]
fn duet_ascendant_grants_bonded_spirits_extra_defense() {
    // Static/Owner/BondStatGrant{def:10}: while Duet stands, the owner's bonded spirits
    // each gain +10 Defense (a flat seat-wide bond amplifier).
    let cat = canon_catalog();
    let def_at_11 = |duet: bool| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Cloudling"), Seat::A);
            put_spirit(st, 12, id_of("Cloudling"), Seat::A);
            st.bonds.push(Bond {
                card: id_of("Rivals' Pact"),
                owner: Seat::A,
                tile_a: 11,
                tile_b: 12,
            });
            if duet {
                put_spirit(st, 6, id_of("Duet Ascendant, Both Halves"), Seat::A);
            }
        }
        combat_stats_for_test(e.state(), &cat, 11).defense
    };
    assert_eq!(
        def_at_11(true) - def_at_11(false),
        10,
        "Duet grants each bonded spirit +10 Defense"
    );
}

#[test]
fn maestra_vole_grants_chorus_to_adjacent_allies() {
    // Static/AdjacentAlliesAll/CounterAura{Chorus,1}: an ally adjacent to Maestra gains
    // Chorus (+10, counting its own adjacent allies, here the Maestra itself).
    let cat = canon_catalog();
    let atk = |maestra: bool| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Cloudling"), Seat::A);
            let neighbour = if maestra {
                "Maestra Vole, Conductor of Small Things"
            } else {
                "Cloudling" // a plain adjacent ally — same adjacency, no grant
            };
            put_spirit(st, 12, id_of(neighbour), Seat::A);
        }
        combat_stats_for_test(e.state(), &cat, 11).attack
    };
    assert_eq!(
        atk(true) - atk(false),
        10,
        "Maestra grants its adjacent ally Chorus (+10)"
    );
}

#[test]
fn momentum_mod_auras_set_the_chain_flags() {
    // Previously authored-but-unwired: MomentumMod now sets the derived flags momentum_prefs
    // reads. Sparkfather (AlliesAll/first_engage_bonus) and The Long Coronation (SelfSpirit/
    // chain_while_defeating).
    let cat = canon_catalog();
    let mut e = engine();
    put_spirit(e.state_mut_for_test(), 11, id_of("Cloudling"), Seat::A);
    assert!(
        !combat_stats_for_test(e.state(), &cat, 11).momentum_first_bonus,
        "no first-engage bonus without Sparkfather"
    );
    put_spirit(
        e.state_mut_for_test(),
        6,
        id_of("Sparkfather Vermilion"),
        Seat::A,
    );
    assert!(
        combat_stats_for_test(e.state(), &cat, 11).momentum_first_bonus,
        "Sparkfather grants allies the first-engage Momentum bonus"
    );
    let mut e2 = engine();
    put_spirit(
        e2.state_mut_for_test(),
        12,
        id_of("The Long Coronation"),
        Seat::A,
    );
    assert!(
        combat_stats_for_test(e2.state(), &cat, 12).chain_while_defeating,
        "The Long Coronation chains while defeating"
    );
}

#[test]
fn embermane_doubles_the_momentum_per_link() {
    // Static/SelfSpirit/MomentumMod{per_link_bonus:10}: each Momentum chain link gives +20
    // Attack (MOMENTUM_PER_LINK 10 + 10), and it chains while defeating.
    let cat = canon_catalog();
    let mut e = engine();
    put_spirit(
        e.state_mut_for_test(),
        12,
        id_of("Embermane, First of the Pride"),
        Seat::A,
    );
    let cs = combat_stats_for_test(e.state(), &cat, 12);
    assert_eq!(
        cs.momentum_per_link_bonus, 10,
        "Embermane adds +10 to each Momentum link (10 → 20)"
    );
    assert!(cs.chain_while_defeating, "Embermane chains while defeating");
}

#[test]
fn pyrrhic_takes_no_retaliation_on_its_second_chain() {
    // MomentumMod{chain_no_retaliation:true}: Pyrrhic's chain links from the 2nd on take no
    // retaliation; its 1st chain still does.
    let hp_loss = |link: u8| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Pyrrhic, the Laughing Brand"), Seat::A);
            let a = st.board[11].spirit.as_mut().unwrap();
            a.defense = 0;
            a.hp = 100;
            a.hp_max = 100;
            put_spirit(st, 12, id_of("Cloudling"), Seat::B);
            let d = st.board[12].spirit.as_mut().unwrap();
            d.attack = 50;
            d.defense = 0;
            d.hp = 100;
        }
        e.resolve_chain_for_test(11, 12, link);
        100 - e.state().board[11]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(0)
    };
    assert!(hp_loss(1) > 0, "Pyrrhic takes retaliation on its 1st chain");
    assert_eq!(
        hp_loss(2),
        0,
        "Pyrrhic takes no retaliation on its 2nd chain"
    );
}

#[test]
fn badgermarshal_softens_momentum_chains_for_adjacent_allies() {
    // AdjacentAlliesAll/ChainDamageReductionAura{10}: with Badgermarshal adjacent, a CHAIN
    // strike hits its ally for 10 less than an ENGAGE strike (isolating the chain reduction
    // from Badgermarshal's Bulwark +10 Defense, which applies to both).
    let dmg = |chain: bool| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 12, id_of("Cloudling"), Seat::B); // attacker
            st.board[12].spirit.as_mut().unwrap().attack = 40;
            put_spirit(st, 11, id_of("Cloudling"), Seat::A); // defender (ally)
            let d = st.board[11].spirit.as_mut().unwrap();
            d.defense = 0;
            d.hp = 200;
            put_spirit(st, 6, id_of("Badgermarshal of the Last Line"), Seat::A); // adjacent to 11
        }
        if chain {
            e.resolve_chain_for_test(12, 11, 1);
        } else {
            e.resolve_engage_for_test(12, 11);
        }
        200 - e.state().board[11]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(0)
    };
    assert_eq!(
        dmg(false) - dmg(true),
        10,
        "Badgermarshal softens a chain strike on its adjacent ally by 10 (vs an engage)"
    );
}

#[test]
fn grudge_kept_grows_with_enemy_impressions() {
    // Static/SelfSpirit/AttackPerEnemyImpression{10}: +10 Attack per enemy impression on the board.
    let cat = canon_catalog();
    let atk = |impressions: usize| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Grudge-Kept"), Seat::A);
            for t in 0..impressions {
                st.board[t].impressions = vec![Seat::B];
            }
        }
        combat_stats_for_test(e.state(), &cat, 11).attack
    };
    assert_eq!(
        atk(2) - atk(0),
        20,
        "Grudge-Kept gains +10 Attack per enemy impression"
    );
}

#[test]
fn the_worst_version_copies_the_highest_enemy_attack() {
    // Static/SelfSpirit/CopyHighestEnemyAttack: its Attack becomes the strongest enemy's.
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("The Worst Version"), Seat::A);
        put_spirit(st, 12, id_of("Cloudling"), Seat::B);
        st.board[12].spirit.as_mut().unwrap().attack = 70;
        put_spirit(st, 13, id_of("Cloudling"), Seat::B);
        st.board[13].spirit.as_mut().unwrap().attack = 40;
    }
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, 11).attack,
        70,
        "The Worst Version copies the highest enemy Attack (70)"
    );
}

#[test]
fn the_long_rest_denies_impressions_on_adjacent_dissolve() {
    // Static/SelfSpirit/Exception(NoImpressionOnAdjacentDissolve): a spirit dissolving adjacent
    // to The Long Rest leaves no impression.
    let impression_left = |long_rest: bool| -> bool {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 12, id_of("Cloudling"), Seat::A);
            st.board[12].spirit.as_mut().unwrap().fading = true;
            if long_rest {
                put_spirit(st, 11, id_of("The Long Rest"), Seat::A); // adjacent to 12
            }
        }
        e.force_fade_step_for_test(Seat::A);
        e.state().board[12].impressions.first().copied().is_some()
    };
    assert!(
        impression_left(false),
        "a normal dissolve leaves an Impression"
    );
    assert!(
        !impression_left(true),
        "The Long Rest denies the Impression on an adjacent dissolve"
    );
}

#[test]
fn the_closing_book_is_immune_to_interception() {
    // Static/SelfSpirit/Exception(ImmuneToInterception): an arriving Closing Book is not bitten.
    let bitten = |arriver: &str| -> bool {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 7, id_of(arriver), Seat::A); // the arriver
            let a = st.board[7].spirit.as_mut().unwrap();
            a.hp = 100;
            a.hp_max = 100;
            put_spirit(st, 12, id_of("Cloudling"), Seat::B); // interceptor (reaches 7)
            st.board[12].spirit.as_mut().unwrap().attack = 50;
        }
        e.run_interception_for_test(7, Seat::A);
        e.state().board[7]
            .spirit
            .as_ref()
            .map(|s| s.hp < 100)
            .unwrap_or(true)
    };
    assert!(bitten("Cloudling"), "a normal arriver is intercepted");
    assert!(
        !bitten("The Closing Book"),
        "The Closing Book is immune to interception"
    );
}

#[test]
fn teethmarks_is_immune_to_interception() {
    // Static/SelfSpirit/Exception(ImmuneToInterception) (the rim-only entry is a Solace
    // director rule, handled at manifestation): an arriving Teethmarks is not bitten.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 7, id_of("Teethmarks"), Seat::A);
        let a = st.board[7].spirit.as_mut().unwrap();
        a.hp = 100;
        a.hp_max = 100;
        put_spirit(st, 12, id_of("Cloudling"), Seat::B);
        st.board[12].spirit.as_mut().unwrap().attack = 50;
    }
    e.run_interception_for_test(7, Seat::A);
    assert_eq!(
        e.state().board[7].spirit.as_ref().unwrap().hp,
        100,
        "Teethmarks is immune to interception"
    );
}

#[test]
fn the_forgiven_debt_cannot_be_banished_by_an_echo_attacker() {
    // Static/SelfSpirit/Exception(UnbanishableByEcho): a lethal blow from an attacker at Echo
    // (below half HP) is capped to leave it at 1 HP. A healthy attacker still banishes.
    let survives_hp = |attacker_at_echo: bool| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 12, id_of("Cloudling"), Seat::B); // attacker
            let a = st.board[12].spirit.as_mut().unwrap();
            a.attack = 90;
            a.hp_max = 100;
            a.hp = if attacker_at_echo { 30 } else { 100 }; // below half ⇒ at Echo
            put_spirit(st, 11, id_of("The Forgiven Debt"), Seat::A); // defender
            let d = st.board[11].spirit.as_mut().unwrap();
            d.defense = 0;
            d.hp = 40;
            d.hp_max = 40;
        }
        e.resolve_engage_for_test(12, 11);
        e.state().board[11]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(0)
    };
    assert_eq!(
        survives_hp(true),
        1,
        "an Echo attacker cannot banish it (left at 1 HP)"
    );
    assert!(
        survives_hp(false) <= 0,
        "a healthy attacker lands the lethal blow normally"
    );
}

#[test]
fn ferrier_adds_one_anima_to_fade_reclaims() {
    // Static/Owner/Exception(FadeReclaimsExtraAnima): the owner's Fade reclaim regains +1.
    let gained = |ferrier: bool| -> u8 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 11, id_of("Cloudling"), Seat::A);
            if ferrier {
                put_spirit(st, 12, id_of("Ferrier of the Salt Road"), Seat::A);
            }
            st.player_a.anima = 0;
            st.player_a.first_placement_done = true;
        }
        let before = e.state().player_a.anima;
        e.apply(Seat::A, Command::Reclaim { tile: 11 }).unwrap();
        e.state().player_a.anima - before
    };
    assert_eq!(
        gained(true) - gained(false),
        1,
        "Ferrier adds +1 Anima to a Fade reclaim"
    );
}

#[test]
fn hush_suppresses_an_adjacent_spirits_parting() {
    // Static/SelfSpirit/Exception(SuppressesAdjacentParting): a spirit dissolving adjacent to
    // Hush cannot fire its Parting (Wisp of Doubt's adjacent-enemy debuff never lands).
    let parted = |hush: bool| -> bool {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 12, id_of("Wisp of Doubt"), Seat::A);
            st.board[12].spirit.as_mut().unwrap().fading = true;
            put_spirit(st, 13, id_of("Cloudling"), Seat::B); // adjacent enemy Wisp debuffs
            if hush {
                put_spirit(st, 11, id_of("Hush"), Seat::B); // adjacent to Wisp
            }
        }
        e.force_fade_step_for_test(Seat::A)
            .iter()
            .any(|ev| matches!(ev, Event::EffectTempStat { .. }))
    };
    assert!(parted(false), "Wisp's Parting debuffs the adjacent enemy");
    assert!(!parted(true), "Hush suppresses the adjacent Wisp's Parting");
}

#[test]
fn the_lullaby_suppresses_adjacent_enemy_echo() {
    // Static/SelfSpirit/Exception(SuppressesAdjacentEnemyEcho): an enemy adjacent to The Lullaby
    // is too calm to Echo (echo_suppressed denies its variance bonus).
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Cloudling"), Seat::A); // an enemy of the Lullaby
        let s = st.board[12].spirit.as_mut().unwrap();
        s.hp = 10;
        s.hp_max = 100; // below half ⇒ Echo-eligible
        put_spirit(st, 11, id_of("The Lullaby"), Seat::B); // adjacent enemy Lullaby
    }
    assert!(
        e.state().board[12].spirit.as_ref().unwrap().echo_eligible(),
        "the wounded spirit would normally be Echo-eligible"
    );
    assert!(
        e.echo_suppressed_for_test(12),
        "The Lullaby suppresses the adjacent enemy's Echo"
    );
    assert!(
        !e.echo_suppressed_for_test(11),
        "the Lullaby itself is not suppressed"
    );
}

#[test]
fn the_almost_said_copies_its_engagers_reach() {
    // OnEngageResolved/SelfSpirit/CopyEngagerReach: when an enemy engages The Almost-Said (base
    // Cross), it adopts that enemy's Reach (Bristleboar's Lance).
    use recollect_core::types::Reach;
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("The Almost-Said"), Seat::B);
        let d = st.board[11].spirit.as_mut().unwrap();
        d.attack = 0; // no retaliation, so the engager survives to be copied
        d.hp = 500;
        put_spirit(st, 12, id_of("Bristleboar"), Seat::A); // Lance reach
        let a = st.board[12].spirit.as_mut().unwrap();
        a.attack = 0; // The Almost-Said survives the engage
        a.hp = 500;
    }
    assert!(
        e.state().board[11]
            .spirit
            .as_ref()
            .unwrap()
            .copied_reach
            .is_none(),
        "no reach copied before any engage"
    );
    e.resolve_engage_for_test(12, 11);
    assert_eq!(
        e.state().board[11].spirit.as_ref().unwrap().copied_reach,
        Some(Reach::Lance),
        "The Almost-Said copies the engager's Lance reach"
    );
}

#[test]
fn the_unforgiving_arcane_strikes_pierce_warded_defenders() {
    // Now Arcane: its arcane pierce (−20 Defense) is normally negated by Warded, but
    // StrikesIgnoreWarded makes it land even on a Warded defender.
    let dmg = |attacker: &str| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 12, id_of(attacker), Seat::B);
            st.board[12].spirit.as_mut().unwrap().attack = 40;
            put_spirit(st, 11, id_of("Forgotten Name"), Seat::A); // intrinsically Warded
            let d = st.board[11].spirit.as_mut().unwrap();
            d.defense = 30; // so the arcane pierce matters
            d.hp = 500;
        }
        e.resolve_engage_for_test(12, 11);
        500 - e.state().board[11]
            .spirit
            .as_ref()
            .map(|s| s.hp)
            .unwrap_or(500)
    };
    // The Unforgiving (Arcane + ignore Warded) pierces the Warded defender's Defense;
    // a plain non-arcane striker does not.
    assert!(
        dmg("The Unforgiving") > dmg("Cloudling"),
        "The Unforgiving pierces a Warded defender that stops a normal striker"
    );
}
