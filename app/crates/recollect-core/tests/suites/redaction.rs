//! Hidden information stays hidden — by construction, pending the family's
//! `#[redact]` derive and `leak_test!` (sampled).
use crate::common::*;
use recollect_core::state::PendingChoice;
use recollect_core::types::*;
use recollect_core::view::view_for;
use recollect_core::{Command, Seat};

#[test]
fn opponent_hand_deck_and_peek_are_counts_only() {
    // Glimpse (§5) redaction across the WHOLE flow: the opponent learns A glimpsed
    // and that A burned A card (hand count falls), but NEVER which card was burned,
    // the burnable hand, the peeked top, or the keep/bottom outcome — counts/beats
    // only. Asserted at each step: the burn choice, the keep-or-bottom choice, and
    // after resolution.
    let mut st = blank();
    st.player_mut(Seat::A).hand = vec![CardId(17), CardId(16)];
    st.player_mut(Seat::A).deck = vec![CardId(13), CardId(12)];
    let mut e = eng(st, 1);

    // --- Step 1: A glimpses — the BURN choice opens. ---
    e.apply(Seat::A, Command::Glimpse).unwrap();
    // A sees her own burnable hand through her own `pending`; B sees nothing.
    let va = view_for(&e, Seat::A);
    assert!(
        matches!(&va.you.pending, Some(PendingChoice::GlimpseBurn { burnable, .. })
            if *burnable == vec![CardId(17), CardId(16)]),
        "the glimpser sees her own burnable hand"
    );
    let vb = view_for(&e, Seat::B);
    assert_eq!(vb.opponent.hand_count, 2, "B sees only A's hand COUNT");
    assert_eq!(vb.opponent.deck_count, 2);
    assert!(
        vb.you.pending.is_none(),
        "B never sees A's burn choice (nor the burnable hand)"
    );
    let json = serde_json::to_string(&vb).unwrap();
    assert_eq!(
        json.matches("\"hand\":").count(),
        1,
        "only B's own hand serializes — the GlimpseBurn list never leaks"
    );
    assert!(
        !json.contains("burnable"),
        "the burnable hand never appears in the opponent's serialized view"
    );

    // --- Step 2: A burns CardId(17) — the keep-or-bottom choice opens. ---
    e.apply(Seat::A, Command::Choose { index: 0 }).unwrap();
    let va = view_for(&e, Seat::A);
    assert!(
        matches!(
            va.you.pending,
            Some(PendingChoice::Glimpse {
                top: CardId(13),
                ..
            })
        ),
        "the glimpser now sees her own peeked top"
    );
    let vb = view_for(&e, Seat::B);
    assert_eq!(
        vb.opponent.hand_count, 1,
        "B sees A's hand count FALL by one (a card was burned) — but not which"
    );
    assert_eq!(vb.you.peeked_top, None);
    assert!(
        vb.you.pending.is_none(),
        "B never sees A's keep-or-bottom choice (nor the peeked card)"
    );
    let json = serde_json::to_string(&vb).unwrap();
    assert!(
        !json.contains("\"top\":"),
        "the peeked card never appears in the opponent's serialized view"
    );
    // The burned card (17) is nowhere in B's view.
    assert!(
        !json.contains(&format!("{}", CardId(17).0)),
        "the burned card never appears in the opponent's serialized view"
    );
    assert_eq!(
        json.matches("\"hand\":").count(),
        1,
        "only your own hand serializes"
    );
    assert_eq!(
        json.matches("peeked_top").count(),
        1,
        "only your own peek serializes"
    );
    assert_eq!(
        json.matches("\"pending\":").count(),
        1,
        "only your own pending serializes"
    );

    // --- After A KEEPS: peeked_top is A's private deck knowledge; B learns the beat
    // (hand fell) but not the card or the outcome. ---
    e.apply(Seat::A, Command::Choose { index: 0 }).unwrap();
    assert_eq!(view_for(&e, Seat::A).you.peeked_top, Some(CardId(13)));
    let vb = view_for(&e, Seat::B);
    assert_eq!(vb.you.peeked_top, None, "B never learns A's kept top");
    assert_eq!(vb.opponent.hand_count, 1, "B sees the post-burn hand count");
    assert!(vb.you.pending.is_none());
}

/// The three view fields that drive the rules-change cues + the Solace's live
/// score (`solace_erasures`, per-spirit `mobile`, `moved_this_turn`) carry ONLY
/// public information. They expose a public score and public board state — never the
/// opponent's hand, deck order, or Echo pre-knowledge — and a Mobile keyword is
/// redacted on a hidden enemy lurker exactly like its stat block. This pins the
/// redaction contract for the new fields so a future change can't quietly widen them.
#[test]
fn the_new_cue_fields_carry_only_public_info() {
    let mut st = blank();
    // B (the opponent, here the Solace) holds private state A must never see.
    st.player_mut(Seat::B).hand = vec![CardId(11), CardId(13)];
    st.player_mut(Seat::B).deck = vec![CardId(12), CardId(17), CardId(16)];
    st.player_mut(Seat::B).peeked_top = Some(CardId(12)); // B's private Glimpse peek
    // Public board + score state the cues read.
    st.solace_erasures = 3; // the Solace's running off-board tally
    // A owns a Mobile spirit (Spark Shrew, card 14) that has already stepped, and a
    // non-Mobile one (Kilnhorn Rhino, card 17). B owns a hidden lurker that is ALSO a
    // Mobile card — but face-down, so its keyword must redact to false for A.
    put(&mut st, t(2, 2), 14, Seat::A, None);
    put(&mut st, t(3, 2), 17, Seat::A, None);
    put(&mut st, t(1, 1), 14, Seat::B, None);
    {
        let lurker = st.board[t(1, 1) as usize].spirit.as_mut().unwrap();
        lurker.face_down = true;
        // …and it is ALSO standing-Faded (banished in combat, in its §0.5 window):
        // `combat_faded` must STILL redact to false for A, since the lurker reveals
        // nothing. A bug that leaked combat_faded would expose a hidden enemy's fade.
        lurker.fading = true;
        lurker.banished_by = Some(Seat::A);
        lurker.fade_deadline = Some(st.round);
        // …and wounded to at-or-below half HP, so it IS Echo-eligible. A hidden
        // lurker's Echo state must STILL redact to false — leaking `echo: true` would
        // reveal that the unseen enemy is below half (a real tell). (Mutation-killer
        // for view.rs `tiles_for`: `echo: !hidden && sp.echo_eligible()` — an `&&`→`||`
        // flip leaks the live echo flag through the hidden lurker.)
        lurker.hp = lurker.hp_max / 2;
    }
    st.moved_this_turn = vec![t(2, 2)]; // A's Shrew has spent its step
    let e = eng(st, 1);
    let va = view_for(&e, Seat::A);

    // 1) solace_erasures is the public score, reported truthfully.
    assert_eq!(va.solace_erasures, 3, "the Solace's tally shows mid-game");
    // 2) moved_this_turn is public board state, reported truthfully.
    assert_eq!(va.moved_this_turn, vec![t(2, 2)]);
    // 3) mobile is the public keyword for visible spirits…
    assert!(
        va.tiles[t(2, 2) as usize].spirit.as_ref().unwrap().mobile,
        "A's Spark Shrew is Mobile"
    );
    assert!(
        !va.tiles[t(3, 2) as usize].spirit.as_ref().unwrap().mobile,
        "A's Kilnhorn Rhino is not Mobile"
    );
    // A VISIBLE own spirit at full HP is NOT Echo-eligible, so its `echo` cue reads
    // false — the flag tracks real below-half state, it is not hardcoded. (Kills the
    // view.rs `echo: !hidden && …` mutant that drops the eligibility term, which would
    // report every visible spirit as Echo-ready.)
    assert!(
        !va.tiles[t(2, 2) as usize].spirit.as_ref().unwrap().echo,
        "a full-HP visible spirit is not Echo-eligible → echo cue is false"
    );
    // …but REDACTED to false on the enemy's hidden lurker, like its stat block: a
    // face-down spirit's keywords are not yet known, even if the card is Mobile.
    let lurker = va.tiles[t(1, 1) as usize].spirit.as_ref().unwrap();
    assert!(lurker.face_down, "the enemy lurker is hidden");
    assert!(
        !lurker.mobile,
        "a hidden enemy lurker's Mobile keyword is redacted to false"
    );
    // combat_faded is public board state for a VISIBLE spirit, but a hidden lurker
    // reveals nothing — so even though this lurker IS standing-Faded, A sees false
    // (the §0.5 window / banished_by / fade_deadline never leak through the cue).
    assert!(
        !lurker.combat_faded,
        "a hidden enemy lurker's standing-Faded state is redacted to false"
    );
    assert!(
        !lurker.echo,
        "a hidden enemy lurker's Echo-eligibility (it is below half HP) is redacted to false"
    );
    assert_eq!(
        lurker.card,
        CardId(u16::MAX),
        "and its identity stays hidden"
    );

    // 4) Structural redaction holds: the new fields add no second hand/peek/pending
    // key, and B's private cards never appear in A's serialized view.
    let json = serde_json::to_string(&va).unwrap();
    assert_eq!(json.matches("\"hand\":").count(), 1, "only A's own hand");
    assert_eq!(
        json.matches("\"peeked_top\":").count(),
        1,
        "only A's own peek"
    );
    assert_eq!(
        json.matches("\"pending\":").count(),
        1,
        "only A's own pending"
    );
    assert_eq!(
        va.opponent.hand_count, 2,
        "B's hand crosses as a truthful count, not contents"
    );
    assert_eq!(va.opponent.deck_count, 3);
    // B's deck holds CardId(16) and hand holds CardId(13); neither may surface as a
    // bare card value through any field (the lurker's identity is the MAX sentinel).
    assert!(
        !json.contains("\"card\":16") && !json.contains("\"card\":13"),
        "no opponent deck/hand identity leaks through a cue field: {json}"
    );
}

#[test]
fn an_enemy_face_down_fabrication_is_masked_but_your_own_is_not() {
    // Mutation-killer (view.rs `tiles_for`, the terrain `enemy_fab = tr.face_down &&
    // tr.owner != seat` gate). An enemy's face-down Fabrication crosses as a hidden lie —
    // identity withheld (the MAX sentinel), only "a face-down Fabrication is here". But
    // YOUR OWN face-down Fabrication is open to you. The `&&`→`||` flip would either
    // un-hide the enemy's lie or HIDE your own — the own-Fab control catches the latter.
    use recollect_core::state::{Terrain, TerrainKind};
    let mut st = blank();
    // B's secret Fabrication at (1,1); A's own face-down Fabrication at (3,3).
    st.board[t(1, 1) as usize].terrain = Some(Terrain {
        card: CardId(41),
        owner: Seat::B,
        kind: TerrainKind::Fabrication,
        face_down: true,
    });
    st.board[t(3, 3) as usize].terrain = Some(Terrain {
        card: CardId(42),
        owner: Seat::A,
        kind: TerrainKind::Fabrication,
        face_down: true,
    });
    let e = eng(st, 1);
    let va = view_for(&e, Seat::A);
    let enemy = va.tiles[t(1, 1) as usize].terrain.as_ref().unwrap();
    assert_eq!(
        enemy.card,
        CardId(u16::MAX),
        "an enemy face-down Fabrication's identity is hidden"
    );
    assert!(enemy.face_down, "and it shows as face-down");
    assert_eq!(
        enemy.kind, "Fabrication",
        "its kind crosses (a lie is here), not its name"
    );
    let own = va.tiles[t(3, 3) as usize].terrain.as_ref().unwrap();
    assert_eq!(
        own.card,
        CardId(42),
        "your OWN face-down Fabrication stays open to you (the owner gate)"
    );
    // And B sees the mirror image: B's own Fab open, A's masked.
    let vb = view_for(&e, Seat::B);
    assert_eq!(
        vb.tiles[t(1, 1) as usize].terrain.as_ref().unwrap().card,
        CardId(41),
        "B sees its own Fabrication"
    );
    assert_eq!(
        vb.tiles[t(3, 3) as usize].terrain.as_ref().unwrap().card,
        CardId(u16::MAX),
        "but A's Fabrication is hidden from B"
    );
}

#[test]
fn the_ink_wash_is_per_seat() {
    let mut st = blank();
    put(&mut st, t(2, 2), 0, Seat::A, None);
    let e = eng(st, 1);
    let va = view_for(&e, Seat::A);
    let vb = view_for(&e, Seat::B);
    assert!(
        va.tiles[t(2, 3) as usize].in_your_projection,
        "Dawnling reaches forward for A"
    );
    let a_wash: Vec<bool> = va.tiles.iter().map(|t| t.in_your_projection).collect();
    let b_wash: Vec<bool> = vb.tiles.iter().map(|t| t.in_your_projection).collect();
    assert_ne!(a_wash, b_wash, "each Narrator sees her own play's edge");
}
