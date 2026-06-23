//! AtFlow: persistent sources tithe/heal at their owner's Flow. Wellspring (a
//! Landmark anima tithe — previously credited but NEVER fired, an honesty gap now
//! closed), Hearth (Landmark occupant heal), Shared Umbrella (Bond pair heal).
//! Each test forces it to be Seat B's turn, then ends it so Seat A's Flow fires.
use recollect_core::Engine;
use recollect_core::cards::canon_catalog;
use recollect_core::state::{Bond, Command, Phase, Terrain, TerrainKind};
use recollect_core::test_support::put_spirit;
use recollect_core::types::{CardId, Seat, SeatSlot};

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

/// Hand Seat B the turn (mid-Act) so that ending it runs Seat A's Flow.
fn hand_turn_to_b(e: &mut Engine) {
    let st = e.state_mut_for_test();
    st.active = Seat::B;
    st.active_slot = SeatSlot::B1;
    st.phase = Phase::Acting;
}

#[test]
fn remember_them_draws_per_ally_dissolved_this_turn() {
    // DrawPerBanishedThisTurn: an ally dissolves at A's Fade (its turn-END), then A casts
    // Remember Them. We stage on A's turn and force the Fade dissolution of A's fading
    // ally (the Fade phase is at turn-END now; the hook drives the same dissolution body).
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        st.active = Seat::A;
        st.active_slot = SeatSlot::A1;
        put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        st.board[11].spirit.as_mut().unwrap().fading = true;
        st.player_a.hand = vec![id_of("Remember Them")];
        st.player_a.anima = 9;
    }
    e.force_fade_step_for_test(Seat::A); // A's Fade dissolves the Cloudling (ticks the count)
    let deck_before = e.state().player(Seat::A).deck.len();
    let cast = e
        .legal_commands(Seat::A)
        .into_iter()
        .find(|c| matches!(c, Command::CastRitual { .. }))
        .expect("Remember Them is castable");
    e.apply(Seat::A, cast).unwrap();
    assert_eq!(
        e.state().player(Seat::A).deck.len(),
        deck_before - 1,
        "drew 1 for the one ally that dissolved this turn"
    );
}

#[test]
fn the_long_walk_heals_the_survivor_when_its_partner_parts() {
    // Parting/BondedPartner/RestoreForm: when a bonded spirit fully dissolves, the
    // survivor restores 30. Tile 11 (fading) dissolves at A's turn-END Fade; partner 12
    // heals. Staged on A's turn, the Fade forced via the hook (Fade is at turn-END now).
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        st.active = Seat::A;
        st.active_slot = SeatSlot::A1;
        put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
        st.board[11].spirit.as_mut().unwrap().fading = true;
        st.board[12].spirit.as_mut().unwrap().hp = 5;
        st.bonds.push(Bond {
            card: id_of("The Long Walk"),
            owner: Seat::A,
            tile_a: 11,
            tile_b: 12,
        });
    }
    e.force_fade_step_for_test(Seat::A);
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        35,
        "the survivor restored 30 HP (5 → 35) when its partner Parted"
    );
}

#[test]
fn wellspring_tithes_one_anima_at_your_flow() {
    let anima_after_flow = |with_wellspring: bool| -> u8 {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            if with_wellspring {
                st.board[12].terrain = Some(Terrain {
                    card: id_of("Wellspring"),
                    owner: Seat::A,
                    kind: TerrainKind::Landmark,
                    face_down: false,
                });
            }
            st.player_a.anima = 0;
        }
        hand_turn_to_b(&mut e);
        e.apply(Seat::B, Command::EndTurn).unwrap();
        e.state().player(Seat::A).anima
    };
    assert_eq!(
        anima_after_flow(true),
        anima_after_flow(false) + 1,
        "Wellspring grants its owner +1 Anima at their Flow"
    );
}

#[test]
fn hearth_restores_its_occupant_at_your_flow() {
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        st.board[12].terrain = Some(Terrain {
            card: id_of("Hearth"),
            owner: Seat::A,
            kind: TerrainKind::Landmark,
            face_down: false,
        });
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
        st.board[12].spirit.as_mut().unwrap().hp = 5; // wounded (hp_max 40)
    }
    hand_turn_to_b(&mut e);
    e.apply(Seat::B, Command::EndTurn).unwrap();
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        15,
        "Hearth restores its occupant 10 HP at Flow (5 → 15)"
    );
}

#[test]
fn shared_umbrella_restores_the_bonded_pair_at_your_flow() {
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 11, id_of("Cloudling"), Seat::A);
        put_spirit(st, 12, id_of("Cloudling"), Seat::A);
        st.board[11].spirit.as_mut().unwrap().hp = 5;
        st.board[12].spirit.as_mut().unwrap().hp = 5;
        st.bonds.push(Bond {
            card: id_of("Shared Umbrella"),
            owner: Seat::A,
            tile_a: 11,
            tile_b: 12,
        });
    }
    hand_turn_to_b(&mut e);
    e.apply(Seat::B, Command::EndTurn).unwrap();
    assert_eq!(e.state().board[11].spirit.as_ref().unwrap().hp, 15);
    assert_eq!(
        e.state().board[12].spirit.as_ref().unwrap().hp,
        15,
        "Shared Umbrella restores each bonded spirit 10 HP at Flow"
    );
}

#[test]
fn the_unfiled_draws_at_flow_while_attuned() {
    // OnAttuned/Owner/Draw{1}: while attuned (adjacent to 2+ allies sharing a Resonance),
    // The Unfiled draws an extra card at its owner's Flow.
    let deck_drop = |attuned: bool| -> usize {
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 12, id_of("The Unfiled"), Seat::A);
            put_spirit(st, 11, id_of("Cloudling"), Seat::A);
            if attuned {
                put_spirit(st, 13, id_of("Cloudling"), Seat::A); // 2nd same-Resonance ally
            }
        }
        let before = e.state().player(Seat::A).deck.len();
        hand_turn_to_b(&mut e);
        e.apply(Seat::B, Command::EndTurn).unwrap(); // runs A's Flow
        before - e.state().player(Seat::A).deck.len()
    };
    assert_eq!(
        deck_drop(false),
        1,
        "not attuned: only the normal Flow draw"
    );
    assert_eq!(
        deck_drop(true),
        2,
        "attuned: the normal Flow draw + The Unfiled's extra"
    );
}

#[test]
fn elder_of_the_unbroken_watch_heals_adjacent_allies_at_flow() {
    // AtFlow/AdjacentAlliesAll/RestoreForm{10}: a standing Elder restores its adjacent
    // allies at the owner's Flow.
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Elder of the Unbroken Watch"), Seat::A);
        put_spirit(st, 11, id_of("Cloudling"), Seat::A); // adjacent, wounded
        st.board[11].spirit.as_mut().unwrap().hp = 10;
        st.board[11].spirit.as_mut().unwrap().hp_max = 50;
    }
    hand_turn_to_b(&mut e);
    e.apply(Seat::B, Command::EndTurn).unwrap(); // runs A's Flow
    assert!(
        e.state().board[11].spirit.as_ref().unwrap().hp > 10,
        "Elder restored its adjacent ally at Flow"
    );
}

#[test]
fn sage_of_and_then_opens_a_glimpse_at_flow() {
    // AtFlow/Owner/PeekDeck{look:2, take:1}: a standing Sage opens a Glimpse at its owner's
    // Flow (the owner is active then, so the interactive peek is safe).
    let mut e = engine();
    {
        let st = e.state_mut_for_test();
        put_spirit(st, 12, id_of("Sage of And-Then"), Seat::A);
    }
    hand_turn_to_b(&mut e);
    e.apply(Seat::B, Command::EndTurn).unwrap(); // runs A's Flow
    assert!(
        e.state().pending_choice.is_some(),
        "Sage opened a Glimpse at A's Flow"
    );
}

#[test]
fn foal_grows_five_each_flow_capped_at_twenty() {
    // AtFlow/SelfSpirit/GrowEachFlow{step:5,max:20}: "+5/+5 (max +20/+20) — it grows up with
    // you." This was DEAD — GrowEachFlow was an IR variant executed NOWHERE, so the Foal
    // never grew. Now its own Flow grows it +5/+5, banking only the room up to the cap.
    let mut e = engine();
    let foal = 12u8;
    let (printed_a, printed_d) = {
        let f = id_of("Foal Born During the Storm");
        let d = canon_catalog().iter().find(|c| c.id == f).cloned().unwrap();
        (d.attack, d.defense)
    };
    {
        let st = e.state_mut_for_test();
        put_spirit(st, foal, id_of("Foal Born During the Storm"), Seat::A);
        let sp = st.board[foal as usize].spirit.as_mut().unwrap();
        sp.attack = printed_a; // start at printed (put_spirit uses fixed stats)
        sp.defense = printed_d;
        sp.hp_max = 40;
        sp.hp = 40;
    }
    // One flow: +5/+5.
    hand_turn_to_b(&mut e);
    e.apply(Seat::B, Command::EndTurn).unwrap(); // → A's Flow
    {
        let sp = e.state().board[foal as usize].spirit.as_ref().unwrap();
        assert_eq!(
            sp.attack - printed_a,
            5,
            "Foal grew +5 Attack at its Flow (was dead)"
        );
        assert_eq!(sp.defense - printed_d, 5, "and +5 Defense");
    }
    // Drive it to the +20 cap, then prove it stops (never overshoots "max +20/+20").
    for _ in 0..6 {
        hand_turn_to_b(&mut e);
        e.apply(Seat::B, Command::EndTurn).unwrap();
    }
    let sp = e.state().board[foal as usize].spirit.as_ref().unwrap();
    assert_eq!(
        sp.attack - printed_a,
        20,
        "Foal's growth caps at +20 Attack — it does not grow past the cap"
    );
    assert_eq!(sp.defense - printed_d, 20, "and caps at +20 Defense");
}
