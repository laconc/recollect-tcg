//! CARD red-team: outcome coverage for the stat-aura cards that the
//! `combat_stats` fold applies, pinned per-card on top of the generic-mechanism
//! tests + the ratchet credit. The ratchet proves a card is *authored* and the
//! mechanism is wired; these table sweeps prove each individual card's authored
//! MAGNITUDE and TARGETING land — catching the per-card data-drift class (a +20
//! that pays +10, a buff that hits the wrong side).
//!
//! The exotic cards this sweep guards against silent breakage (Quiet Tide, The Last
//! Warm Page, Faultline, The Unsaid Cruelty, the Foal) are pinned in
//! solace_effects.rs / flow_effects.rs next to their kin.
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::effects::Keyword;
use recollect_core::engine::{combat_stats_for_test, keyword_active_for_test};
use recollect_core::state::{Bond, Terrain, TerrainKind};
use recollect_core::test_support::put_spirit;
use recollect_core::types::{CardId, Faction, Seat};

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

/// Δ(attack, defense) the landmark's OCCUPANT gains vs. standing on bare ground.
fn occupant_delta(landmark: &str) -> (i16, i16) {
    let cat = canon_catalog();
    // Baseline: a lone Cloudling at tile 12 on bare ground.
    let mut base = engine();
    put_spirit(base.state_mut_for_test(), 12, id_of("Cloudling"), Seat::A);
    let b = combat_stats_for_test(base.state(), &cat, 12);
    // With the landmark under it.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        st.board[12].terrain = Some(Terrain {
            card: id_of(landmark),
            owner: Seat::A,
            kind: TerrainKind::Landmark,
            face_down: false,
        });
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
    }
    let w = combat_stats_for_test(e.state(), &cat, 12);
    (w.attack - b.attack, w.defense - b.defense)
}

#[test]
fn landmark_occupant_stat_auras_match_their_card_text() {
    // Each (landmark, +Attack, +Defense) read straight off the §4 index / IR. High Ground
    // (+10/0) is covered in spellbook.rs; this sweeps the rest of the flat occupant auras.
    // (Ashfield / Mirror Pool are RetaliationDelta — combat-side, covered by their own
    // engage tests; Waystone is a movement exception with a 0/0 stat clause.)
    let cases: &[(&str, i16, i16)] = &[
        ("Old Tree", 0, 10),
        ("Mire", -10, 0),
        ("The Bellows", 10, 0), // base +10 Attack (Flame +20 is the unmodeled half)
        ("The Confluence", 10, 10),
        ("The Dimming", 0, 10),     // base +10 Defense (Fear half folded in)
        ("The Still Shore", 0, 10), // base +10 Defense (Sorrow half folded in)
        ("The Old Wall", -10, 20),
    ];
    for &(name, da, dd) in cases {
        let (ga, gd) = occupant_delta(name);
        assert_eq!(
            (ga, gd),
            (da, dd),
            "{name}: occupant should gain {da:+}/{dd:+}, got {ga:+}/{gd:+}"
        );
    }
}

/// Δ defense an ADJACENT ally gains from a standing aura-spirit at tile 12; the ally
/// is at 11 and a control ally sits far away at 24 (must be unbuffed).
fn adjacent_ally_defense_delta(aura_spirit: &str) -> (i16, i16) {
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of(aura_spirit), Seat::A); // the aura source
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // adjacent ally
        put_spirit(st, 24, id_of("Cloudling"), Seat::A); // distant ally (control)
    }
    let adj = combat_stats_for_test(e.state(), &cat, 11);
    let far = combat_stats_for_test(e.state(), &cat, 24);
    (adj.attack - far.attack, adj.defense - far.defense)
}

#[test]
fn defensive_aura_spirits_buff_adjacent_allies_per_card_text() {
    // Bulwark / Warden auras: "adjacent allies +10 Defense" — assert the adjacent ally is
    // buffed and a distant ally is not (targeting), at the card's magnitude.
    for name in ["Bulwark Badger", "Warden of the Glade", "The Graven Elder"] {
        let (da, dd) = adjacent_ally_defense_delta(name);
        assert_eq!(
            (da, dd),
            (0, 10),
            "{name}: an adjacent ally should gain +0/+10 (and a distant ally nothing), got {da:+}/{dd:+}"
        );
    }
}

#[test]
fn the_graven_elder_gains_twenty_defense_only_while_undamaged() {
    // Static/WhileUndamaged/SelfSpirit/StatDelta{+0/+20}: the +20 holds only at full HP.
    let cat = canon_catalog();
    let measure = |wounded: bool| -> i16 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 12, id_of("The Graven Elder"), Seat::A);
            if wounded {
                st.board[12].spirit.as_mut().unwrap().hp -= 10;
            }
        }
        combat_stats_for_test(e.state(), &cat, 12).defense
    };
    assert_eq!(
        measure(false) - measure(true),
        20,
        "The Graven Elder's +20 Defense applies only while undamaged"
    );
}

#[test]
fn choir_of_the_vale_buffs_only_allied_song_spirits() {
    // Static/AlliesWithImprint{Song}/StatDelta{+10/+10}: a tribal buff that must hit a Song
    // ally and spare a non-Song ally (correct imprint targeting).
    let cat = canon_catalog();
    // Find a Song-imprinted spirit and a non-Song one to stand beside the Choir.
    let song = cat
        .iter()
        .find(|c| {
            matches!(c.kind, recollect_core::types::CardKind::Spirit)
                && c.imprints.iter().any(|i| i == "Song")
                && c.name != "Choir of the Vale"
        })
        .map(|c| c.id)
        .expect("a Song spirit exists");
    let non_song = cat
        .iter()
        .find(|c| {
            matches!(c.kind, recollect_core::types::CardKind::Spirit)
                && !c.imprints.iter().any(|i| i == "Song")
        })
        .map(|c| c.id)
        .expect("a non-Song spirit exists");
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Choir of the Vale"), Seat::A);
        st.board[11].spirit = Some(recollect_core::state::Spirit {
            ..mk(song, Seat::A)
        });
        st.board[13].spirit = Some(recollect_core::state::Spirit {
            ..mk(non_song, Seat::A)
        });
    }
    let song_stats = combat_stats_for_test(e.state(), &cat, 11);
    let non_song_stats = combat_stats_for_test(e.state(), &cat, 13);
    let song_printed = cat.iter().find(|c| c.id == song).unwrap();
    let non_printed = cat.iter().find(|c| c.id == non_song).unwrap();
    // "+10/+10" — both halves must land (the second was authored as HP/`form`, which a
    // Static aura cannot grant — combat_stats folds only attack+defense — so it was dead;
    // fixed to defense:10 like its sister auras).
    assert_eq!(
        (
            song_stats.attack - song_printed.attack,
            song_stats.defense - song_printed.defense
        ),
        (10, 10),
        "the Song ally gains +10 Attack / +10 Defense from Choir of the Vale"
    );
    assert_eq!(
        (
            non_song_stats.attack - non_printed.attack,
            non_song_stats.defense - non_printed.defense
        ),
        (0, 0),
        "a non-Song ally gains nothing (tribal targeting)"
    );
}

/// A Spirit at full printed stats (helper for the Choir test — the synthetic catalog's
/// put_spirit uses fixed stats, so build from the real CardDef here).
fn mk(card: CardId, owner: Seat) -> recollect_core::state::Spirit {
    let d = canon_catalog().into_iter().find(|c| c.id == card).unwrap();
    recollect_core::state::Spirit {
        replacement_used: false,
        holding: false,
        face_down: false,
        is_token: false,
        placed_by: None,
        card: d.id,
        owner,
        attack: d.attack,
        defense: d.defense,
        hp: d.hp,
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
    }
}

/// Δ(attack, defense) BOTH ends of a bond gain (read at tile 11) vs. printed, while the
/// pair stands adjacent at 11/12.
fn bonded_pair_delta(bond: &str) -> (i16, i16) {
    let cat = canon_catalog();
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
        st.bonds.push(Bond {
            card: id_of(bond),
            owner: Seat::A,
            tile_a: 11,
            tile_b: 12,
        });
    }
    // Baseline: same Cloudling, unbonded.
    let mut base = engine();
    put_spirit(base.state_mut_for_test(), 11, id_of("Cloudling"), Seat::A);
    let b = combat_stats_for_test(base.state(), &cat, 11);
    let w = combat_stats_for_test(e.state(), &cat, 11);
    (w.attack - b.attack, w.defense - b.defense)
}

#[test]
fn strangers_no_more_grants_ten_defense_to_each_bonded_end() {
    // Static/BondedPair/StatDelta{+0/+10}: a flat +10 Defense to both ends while adjacent.
    let (ga, gd) = bonded_pair_delta("Strangers No More");
    assert_eq!(
        (ga, gd),
        (0, 10),
        "Strangers No More: each bonded end gains +0/+10, got {ga:+}/{gd:+}"
    );
}

#[test]
fn the_old_friendship_grants_ten_attack_and_ten_defense_to_each_bonded_end() {
    // The Old Friendship's "+10/+10 each" — a STATIC bonded-pair aura. It was authored as
    // StatDelta{attack:10, defense:0, form:10}, but a Static aura is realized by `combat_stats`,
    // which folds ONLY attack + defense — never HP (`form`). So its second "+10" (encoded as
    // HP) was STRUCTURALLY DEAD: combat_stats dropped it, leaving only +10 Attack. Fixed to
    // {attack:10, defense:10, form:0}, matching The Confluence (the sister Static "+10/+10"
    // aura) — now both numbers land, as the card text reads.
    let (ga, gd) = bonded_pair_delta("The Old Friendship");
    assert_eq!(
        (ga, gd),
        (10, 10),
        "The Old Friendship lifts each bonded end +10 Attack / +10 Defense, got {ga:+}/{gd:+}"
    );
}

#[test]
fn share_higher_bonds_lift_the_weaker_end_to_the_stronger() {
    // Borrowed Courage (share higher ATTACK) / Sworn Shields (share higher DEFENSE): the
    // lower-stat end is lifted to the higher; the strong end is unchanged.
    let cat = canon_catalog();
    // Two spirits of different Attack/Defense, bonded and adjacent.
    let strong_atk = cat
        .iter()
        .find(|c| matches!(c.kind, recollect_core::types::CardKind::Spirit) && c.attack >= 40)
        .map(|c| c.id)
        .expect("a high-Attack spirit");
    let weak_atk = cat
        .iter()
        .find(|c| matches!(c.kind, recollect_core::types::CardKind::Spirit) && c.attack <= 20)
        .map(|c| c.id)
        .expect("a low-Attack spirit");
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        st.board[11].spirit = Some(mk(weak_atk, Seat::A));
        st.board[12].spirit = Some(mk(strong_atk, Seat::A));
        st.bonds.push(Bond {
            card: id_of("Borrowed Courage"),
            owner: Seat::A,
            tile_a: 11,
            tile_b: 12,
        });
    }
    let weak_printed = cat.iter().find(|c| c.id == weak_atk).unwrap().attack;
    let strong_printed = cat.iter().find(|c| c.id == strong_atk).unwrap().attack;
    let weak_now = combat_stats_for_test(e.state(), &cat, 11).attack;
    let strong_now = combat_stats_for_test(e.state(), &cat, 12).attack;
    assert_eq!(
        weak_now, strong_printed,
        "Borrowed Courage lifts the weaker end's Attack to the higher ({weak_printed} → {strong_printed})"
    );
    assert_eq!(strong_now, strong_printed, "the stronger end is unchanged");
}

// ── NON-DECK red-team: the dead/mis-scoped effects that hide in the non-deck set
// (Solace Unwritten/IllIntent/Unwriting, Foundlings, tokens) — authored but invisible
// to the deck-playable ratchet, so an unfired one would never show there.
// Now engine-backed and outcome-pinned, so the dead state can't recur. Guarded for keeps by
// `every_non_deck_effect_bearing_card_is_engine_backed` (effects_coverage.rs).

/// A Song-imprinted spirit carrying Chorus (its Attack scales with adjacent allies) for the
/// TraitSilence tests — Choir of the Vale is the canonical Song/Chorus aura source. Returns
/// its id; panics if the catalog shape changed.
fn a_chorus_spirit() -> CardId {
    id_of("Hum") // the Kindred: SelfSpirit/CounterAura{Chorus} — pure Chorus, +10/adjacent
}

#[test]
fn the_smudge_silences_an_adjacent_enemys_chorus_bonus() {
    // Static/AdjacentEnemiesAll/TraitSilence — DEAD (no executor read TraitSilence). Now it
    // blanks the adjacent enemy's Chorus tribal bonus. Measure a Chorus spirit's Attack with a
    // friendly neighbour (so Chorus is +10) WITHOUT vs WITH The Smudge adjacent.
    let cat = canon_catalog();
    let chorus = a_chorus_spirit();
    let measure = |smudge_adjacent: bool| -> i16 {
        let mut e = engine();
        let st = e.state_mut_for_test();
        // Chorus spirit at 12, a friendly ally at 11 → Chorus would grant +10 Attack.
        st.board[12].spirit = Some(recollect_core::state::Spirit {
            ..mk(chorus, Seat::B)
        });
        put_spirit(st, 11, id_of("Cloudling"), Seat::B);
        if smudge_adjacent {
            // The Smudge (seat A — the enemy of the Chorus spirit) adjacent at 13.
            put_spirit(st, 13, id_of("The Smudge"), Seat::A);
        }
        combat_stats_for_test(e.state(), &cat, 12).attack
    };
    let free = measure(false);
    let silenced = measure(true);
    assert_eq!(
        free - silenced,
        10,
        "The Smudge silences the adjacent enemy's +10 Chorus bonus (free={free}, silenced={silenced})"
    );
}

#[test]
fn null_choir_silences_an_adjacent_enemys_chorus_bonus() {
    // Same TraitSilence mechanic as The Smudge (the choir that stands and does not sing).
    let cat = canon_catalog();
    let chorus = a_chorus_spirit();
    let measure = |choir_adjacent: bool| -> i16 {
        let mut e = engine();
        let st = e.state_mut_for_test();
        st.board[12].spirit = Some(recollect_core::state::Spirit {
            ..mk(chorus, Seat::B)
        });
        put_spirit(st, 11, id_of("Cloudling"), Seat::B);
        if choir_adjacent {
            put_spirit(st, 13, id_of("Null Choir"), Seat::A);
        }
        combat_stats_for_test(e.state(), &cat, 12).attack
    };
    assert_eq!(
        measure(false) - measure(true),
        10,
        "Null Choir silences the adjacent enemy's +10 Chorus bonus"
    );
}

#[test]
fn whats_its_name_cannot_be_chosen_as_a_ritual_target() {
    // Static/SelfSpirit/Restrict(BeTargetedByRituals) — DEAD (consulted nowhere). Now the
    // unnameable thing is excluded from a target-choosing Ritual's options. Cast Hold Fast
    // ("a spirit +20 Defense") with What's-Its-Name and an ordinary spirit both on the board:
    // the options must offer the ordinary spirit and NEVER What's-Its-Name.
    use recollect_core::state::{Command, PendingChoice};
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| id_of("Cloudling")).collect();
    let (mut e, _) = recollect_core::Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // an ordinary, targetable spirit
        put_spirit(st, 13, id_of("What's-Its-Name"), Seat::B); // the unnameable
        st.player_a.hand = vec![id_of("Hold Fast")];
        st.player_a.anima = 9;
    }
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Hold Fast is castable");
    e.apply(Seat::A, cast).unwrap();
    let Some(PendingChoice::Target { options, .. }) = e.state().pending_choice.clone() else {
        panic!("a target choice is pending");
    };
    assert!(
        options.contains(&11),
        "the ordinary spirit is a legal target ({options:?})"
    );
    assert!(
        !options.contains(&13),
        "What's-Its-Name is unnameable — never a Ritual target ({options:?})"
    );
}

#[test]
fn the_misremembered_copies_the_printed_stats_of_the_spirit_it_fought() {
    // OnEngageResolved/SelfSpirit/CopyPrintedStats — DEAD (fire_survivor handled only
    // Survivor/Push; no CopyPrintedStats arm). Now, after it engages, it takes on the PRINTED
    // Attack/Defense of the spirit it fought. The Misremembered (A40/D20) attacks a sturdier
    // foe and should END at that foe's printed A/D.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = recollect_core::Engine::new(7, cat.clone(), deck.clone(), deck);
    // A foe whose printed A/D differ from The Misremembered's (A40/D20), to prove the copy.
    let foe = cat
        .iter()
        .find(|c| {
            matches!(c.kind, recollect_core::types::CardKind::Spirit)
                && c.defense >= 30
                && (c.attack, c.defense) != (40, 20)
        })
        .expect("a foe with distinct printed stats exists");
    let (mtile, ftile) = (12u8, 11u8);
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Lorekeeper, Faction::Solace];
        put_spirit(st, mtile, id_of("The Misremembered"), Seat::B);
        // Make BOTH endpoints survive the exchange so its OnEngageResolved fires: The
        // Misremembered tanky and toothless (it shouldn't banish the foe), the foe likewise.
        {
            let m = st.board[mtile as usize].spirit.as_mut().unwrap();
            m.attack = 0;
            m.hp = 500;
            m.hp_max = 500;
        }
        st.board[ftile as usize].spirit = Some(recollect_core::state::Spirit {
            ..mk(foe.id, Seat::A)
        });
        let f = st.board[ftile as usize].spirit.as_mut().unwrap();
        f.attack = 0;
        f.hp = 500;
        f.hp_max = 500;
    }
    let evs = e.resolve_engage_for_test(mtile, ftile);
    // resolve_engage_for_test returns the events without applying them — apply here.
    for ev in evs {
        e.apply_event_for_test(ev);
    }
    let after = e
        .state()
        .spirit_at(mtile)
        .expect("The Misremembered still stands");
    assert_eq!(
        (after.attack, after.defense),
        (foe.attack, foe.defense),
        "it copied the printed A/D of the spirit it fought ({}: {}/{})",
        foe.name,
        foe.attack,
        foe.defense
    );
}

#[test]
fn ink_runs_dry_taxes_both_narrators_next_card_by_one() {
    // OnPlay/BothNarrators/CostDelta{+1}/ThisRound — DEAD (the CostDelta arm handled only
    // Owner discounts). Now BOTH seats' next card costs 1 more this round, via the per-seat
    // card_tax read in cost_aura. Play the Unwriting, then assert seat A's next spirit pays +1.
    use recollect_core::state::{Command, Event};
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = recollect_core::Engine::new(7, cat.clone(), deck.clone(), deck);
    e.apply(Seat::A, Command::EndTurn)
        .expect("A ends → the Solace acts");
    // A cost-1 spirit seat A will play next, to read its effective cost.
    let cheap = cat
        .iter()
        .find(|c| c.kind == recollect_core::types::CardKind::Spirit && c.cost == 1)
        .expect("a cost-1 spirit exists");
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Lorekeeper, Faction::Solace];
        st.player_b.hand = vec![id_of("Ink Runs Dry")];
        st.player_b.anima = 9;
        st.player_a.hand = vec![cheap.id];
        st.player_a.anima = 9;
    }
    // The Solace plays Ink Runs Dry (an Unwriting event) from hand.
    e.apply(Seat::B, Command::TellUnwriting { hand_index: 0 })
        .expect("the Solace tells Ink Runs Dry");
    // Back to A: play the cost-1 spirit and read the AnimaSpent — it should be 2 (1 + tax).
    e.apply(Seat::B, Command::EndTurn).unwrap();
    let play = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::PlaySpirit { .. }))
        .expect("the cost-1 spirit is playable");
    let evs = e.apply(Seat::A, play).unwrap();
    let paid = evs.iter().find_map(|ev| match ev {
        Event::AnimaSpent {
            seat: Seat::A,
            amount,
        } => Some(*amount),
        _ => None,
    });
    assert_eq!(
        paid,
        Some(2),
        "Ink Runs Dry taxed A's next card +1 (cost-1 spirit paid 2): {evs:?}"
    );
    // And the surcharge is one-shot: a SECOND play this round is back to base cost.
    let st = e.state_mut_for_test();
    assert_eq!(
        st.card_tax[Seat::A as usize],
        (0, 0),
        "the tax is spent after one card"
    );
}

#[test]
fn smear_blanks_the_engaged_enemys_traits_for_the_round() {
    // OnPlay/EngagedEnemy/TraitStrip/ThisRound — DEAD (exec_trait_strip handled only Engager).
    // Now on arrival Smear blanks an adjacent enemy's printed Keywords/Traits for the round —
    // observable as the loss of a printed keyword. Put a Steadfast enemy beside Smear's arrival.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = recollect_core::Engine::new(7, cat.clone(), deck.clone(), deck);
    let steadfast = cat
        .iter()
        .find(|c| c.kind == recollect_core::types::CardKind::Spirit && c.steadfast)
        .expect("a Steadfast spirit exists");
    let (smear_tile, foe_tile) = (12u8, 11u8);
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Lorekeeper, Faction::Solace];
        put_spirit(st, smear_tile, id_of("Smear"), Seat::B);
        st.board[foe_tile as usize].spirit = Some(recollect_core::state::Spirit {
            ..mk(steadfast.id, Seat::A)
        });
    }
    assert!(
        keyword_active_for_test(e.state(), &cat, foe_tile, Keyword::Steadfast),
        "precondition: the foe is Steadfast before Smear arrives"
    );
    e.fire_arrival_effects_for_test(smear_tile, Seat::B);
    assert!(
        !keyword_active_for_test(e.state(), &cat, foe_tile, Keyword::Steadfast),
        "Smear blanked the engaged enemy's printed Steadfast for the round — was dead"
    );
}

#[test]
fn tooth_in_the_margin_strikes_plus_twenty_on_arrival() {
    // OnPlay/OncePerMatch/SelfSpirit/StatDelta{+20}/ThisRound — DEAD (fire_mode dropped
    // OncePerMatch → cond_ok=false, so it never fired). Now its arrival grants +20 Attack this
    // round (the once-per-match instance IS the single play).
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = recollect_core::Engine::new(7, cat.clone(), deck.clone(), deck);
    let tile = 13u8; // a rim-ward inner tile with no adjacent enemy (isolate the self-buff)
    {
        let st = e.state_mut_for_test();
        put_spirit(st, tile, id_of("Tooth in the Margin"), Seat::B);
    }
    let base = combat_stats_for_test(e.state(), &cat, tile).attack;
    e.fire_arrival_effects_for_test(tile, Seat::B);
    let buffed = combat_stats_for_test(e.state(), &cat, tile).attack;
    assert_eq!(
        buffed - base,
        20,
        "Tooth in the Margin's first strike deals +20 (base={base}, buffed={buffed}) — was dead"
    );
}

#[test]
fn wolverine_wearing_a_trap_retaliates_plus_ten() {
    // Static/SelfSpirit/{GrantKeyword(Steadfast) + RetaliationDelta{+10}} — the GrantKeyword
    // half restates its intrinsic Steadfast (engine-honored); pin the RetaliationDelta rider.
    let cat = canon_catalog();
    let tile = 12u8;
    let mut e = engine();
    put_spirit(
        e.state_mut_for_test(),
        tile,
        id_of("Wolverine Wearing a Trap"),
        Seat::A,
    );
    assert_eq!(
        combat_stats_for_test(e.state(), &cat, tile).retaliation,
        10,
        "Wolverine retaliates +10 (its Static RetaliationDelta rider)"
    );
    assert!(
        keyword_active_for_test(e.state(), &cat, tile, Keyword::Steadfast),
        "and is Steadfast (intrinsic, restated by its SelfSpirit GrantKeyword)"
    );
}

#[test]
fn the_devouring_margin_heals_ten_when_it_eats() {
    // OnMove: ImpressionEat + RestoreForm{10} — the eat was wired (via the inward shift) but
    // the paired heal ("heals 10 when it does") was DEAD: UnwrittenShifted never restored form.
    // Now an eat on the inward shift heals the eater by 10.
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = recollect_core::Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Lorekeeper, Faction::Solace];
        put_spirit(st, 0, id_of("The Devouring Margin"), Seat::B);
        st.board[0].spirit.as_mut().unwrap().hp = 10; // wounded, hp_max is higher
        st.board[1].impressions = vec![Seat::A]; // the mark it shifts onto (0 → 1) and eats
    }
    let before = e.state().board[0].spirit.as_ref().unwrap().hp;
    // The Page Turns shifts every Unwritten inward — the eater steps 0 → 1, eats, and heals.
    // `fire_unwriting_for_test` already APPLIES the events it returns (see the sibling
    // the_devouring_margin_forgets_the_mark_it_lands_on), so we read state directly.
    let evs = e.fire_unwriting_for_test("The Page Turns", Seat::B);
    assert!(
        evs.iter().any(|ev| matches!(
            ev,
            recollect_core::state::Event::EffectRestored {
                tile: 1,
                amount: 10
            }
        )),
        "the eat emitted a +10 heal on the landing tile ({evs:?})"
    );
    let healed = e.state().board[1].spirit.as_ref().unwrap(); // it now stands at tile 1
    assert!(
        healed.hp == before + 10 || healed.hp == healed.hp_max,
        "the eater healed 10 on the eat (hp {before} → {})",
        healed.hp
    );
    assert!(
        e.state().board[1].impressions.is_empty(),
        "and it ate the mark it landed on"
    );
}

#[test]
fn erasures_patience_stops_adjacent_impressions_from_scoring() {
    // Static/SelfSpirit/Exception(AdjacentImpressionsDontScore) — re-authored from the dead,
    // mis-scoped Restrict(GainImpressions). While it stands, the impressions on tiles adjacent
    // to it do not score at Nightfall (the marks near it cool). Drive a match to the last round
    // with a seat-A mark adjacent to Erasure's Patience and another mark far away: only the far
    // mark scores for A.
    use recollect_core::state::{Command, Event};
    let cat = canon_catalog();
    let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
    let (mut e, _) = recollect_core::Engine::new(7, cat.clone(), deck.clone(), deck);
    {
        let st = e.state_mut_for_test();
        st.rules.factions = [Faction::Lorekeeper, Faction::Solace];
        st.round = st.rules.last_round;
        st.active = Seat::B;
        // Erasure's Patience (seat B) at 12; a seat-A mark adjacent at 11 (cooled) and one far
        // away at 0 (scores). No standing spirits on the marked tiles.
        put_spirit(st, 12, id_of("Erasure's Patience"), Seat::B);
        st.board[11].impressions = vec![Seat::A];
        st.board[0].impressions = vec![Seat::A];
    }
    let evs = e.apply(Seat::B, Command::EndTurn).unwrap();
    let (sa, sb) = evs
        .iter()
        .find_map(|ev| match ev {
            Event::MatchEnded {
                score_a, score_b, ..
            } => Some((*score_a, *score_b)),
            _ => None,
        })
        .expect("the match ended at Nightfall");
    // A's far mark (tile 0) scores 1; the adjacent mark (tile 11) is cooled → does NOT score.
    // B scores its standing Erasure's Patience (tile 12) = 1.
    assert_eq!(
        sa, 1,
        "only A's NON-adjacent mark scored (the adjacent one cooled): score_a={sa}"
    );
    assert_eq!(sb, 1, "B scores its standing spirit: score_b={sb}");
}
