//! The **online / 2v2 shell adapters** (the launch-critical online PvP + 2v2
//! shells): build the same [`ShellModel`] the local 1v1 shell draws, but from the
//! server's **redacted** [`PlayerView`] / [`TeamView`] and its supplied legal-move list
//! — with **no local engine** (the server is authoritative online).
//!
//! The local shell ([`LocalGame::shell_model_json`](crate::LocalGame::shell_model_json))
//! reads the running score, the hand stat-blocks, and the affordances from its
//! in-process engine. An online / 2v2 client has none of that: it holds only the
//! redacted view it was pushed and the `legal: [LegalMove]` the server computed. So
//! these builders re-derive the same `ShellModel` purely:
//! - the **board** is the view's `tiles` (already redacted — an opponent's hidden
//!   lurker is a face-down back, never their card);
//! - the **running score** is read from the view's tiles (board points per seat) plus
//!   the public `solace_erasures` tally — exactly as the score readout does — never from
//!   any hidden state;
//! - the **hand stat-blocks** come from the canon catalog (id-ordered, engine-free) for
//!   the cards in *your own* `you.hand` — your cards only;
//! - the **affordances** (the action dots / evolve glyphs + the a11y action list) are
//!   derived from the server's **legal commands**, the single source of truth, so the
//!   canvas + the a11y tree can never offer a move the server would reject.
//!
//! **Redaction is the whole point.** The client only ever holds the redacted view, so
//! by construction it cannot render an opponent's hand or deck — the opponent is
//! **counts only** (face-down backs + a number). The 2v2 adapter folds the active
//! slot's `TeamView` (your hand, the teammate + both rivals as counts) into the same
//! `PlayerView`-shaped `ShellModel`, honouring the same redaction.
//!
//! These are pure + native-tested; the wasm seam ([`crate::OnlineShell`]) is a thin
//! JSON wrapper, mirroring how [`crate::LocalGame`] exposes the local shell.
use crate::shell::{HandCard, ShellModel};
use recollect_core::state::Command;
use recollect_core::types::{CardId, Faction, Seat};
use recollect_core::view::{OpponentView, PlayerView, TeamView, TileView, YouView};

/// The shell affordances derived from a legal-move list (the online/2v2 equivalent of
/// `LocalGame`'s private `Affordances`): which board tiles can act, which Fading bases
/// can evolve, which standing-Faded forms can devolve (recede), which hand cards have a
/// legal play, and which of those are evolution form cards / recede bases. Folded
/// straight into the [`ShellModel`].
#[derive(Debug, Clone, Default)]
struct Affordances {
    actionable_tiles: Vec<u8>,
    evolvable_tiles: Vec<u8>,
    devolvable_tiles: Vec<u8>,
    actionable_hand: Vec<u8>,
    evolve_forms: Vec<u8>,
    devolve_bases: Vec<u8>,
}

/// A short, player-facing faction label ("the Solace" / "Lorekeepers") — kept here so
/// the online module is self-contained (the local shell has its own copy).
fn faction_word(f: Faction) -> String {
    match f {
        Faction::Solace => "the Solace".into(),
        Faction::Lorekeeper => "Lorekeepers".into(),
    }
}

/// One card's full stat block from the **canon catalog** (engine-free, id-ordered) —
/// the online client has no engine, so it looks card stats up here exactly as
/// [`crate::card_name`] looks names up. An out-of-range id yields an empty placeholder
/// (it is never your own card, so it never renders a real hand chip).
fn hand_card_of(id: CardId) -> HandCard {
    match recollect_core::cards::canon_catalog().get(id.0 as usize) {
        Some(c) => HandCard {
            name: c.name.clone(),
            cost: c.cost,
            attack: c.attack,
            defense: c.defense,
            hp: c.hp,
            kind: format!("{:?}", c.kind),
            resonance: format!("{:?}", c.resonance),
        },
        None => HandCard {
            name: String::new(),
            cost: 0,
            attack: 0,
            defense: 0,
            hp: 0,
            kind: String::new(),
            resonance: String::new(),
        },
    }
}

/// Your hand (your cards ONLY — redaction holds) as placeholder cards, from the canon
/// catalog. `you` is the redacted view's own-seat private state.
fn hand_from_you(you: &YouView) -> Vec<HandCard> {
    you.hand.iter().map(|id| hand_card_of(*id)).collect()
}

/// Board points per seat from a **redacted** tile list (the same split the score readout
/// and the local shell use): a held tile scores for its standing spirit's owner (a
/// fading spirit still stands and still scores), or — if empty — for its impression's
/// owner. Reads only the redacted view, so it never touches hidden state.
fn board_points(tiles: &[TileView]) -> (u8, u8) {
    let (mut a, mut b) = (0u8, 0u8);
    for t in tiles {
        let owner = match &t.spirit {
            Some(sp) => Some(sp.owner),
            None => t.impression,
        };
        match owner {
            Some(Seat::A) => a = a.saturating_add(1),
            Some(Seat::B) => b = b.saturating_add(1),
            None => {}
        }
    }
    (a, b)
}

/// Derive the shell [`Affordances`] from the server's **legal commands** — the single
/// source of truth, so the canvas dots/glyphs and the a11y tree agree with what the
/// server will actually accept. This mirrors `LocalGame::affordances`, but works off
/// the supplied `legal` list (no engine) — and reads each tile's `fading` flag from the
/// redacted view to decide the evolve glyph (a Fading base earns the rescue chevron).
fn affordances_from_legal(legal: &[Command], tiles: &[TileView]) -> Affordances {
    use std::collections::BTreeSet;
    let mut t: BTreeSet<u8> = BTreeSet::new();
    let mut evolvable: BTreeSet<u8> = BTreeSet::new();
    let mut devolvable: BTreeSet<u8> = BTreeSet::new();
    let mut hand: BTreeSet<u8> = BTreeSet::new();
    let mut forms: BTreeSet<u8> = BTreeSet::new();
    let mut bases: BTreeSet<u8> = BTreeSet::new();
    let is_fading = |tile: u8| {
        tiles
            .get(tile as usize)
            .and_then(|tv| tv.spirit.as_ref())
            .is_some_and(|sp| sp.fading)
    };
    for cmd in legal {
        match cmd {
            Command::MoveSpirit { from, .. } => {
                t.insert(*from);
            }
            Command::Reclaim { tile } | Command::Reveal { tile, .. } => {
                t.insert(*tile);
            }
            Command::StrikeFabrication { from, .. } => {
                t.insert(*from);
            }
            Command::SetOrders { tile, .. } => {
                t.insert(*tile);
            }
            Command::Evolve {
                tile, form_hand, ..
            } => {
                t.insert(*tile);
                if is_fading(*tile) {
                    evolvable.insert(*tile);
                }
                hand.insert(*form_hand);
                forms.insert(*form_hand);
            }
            // Devolution (§5): the standing-Faded form earns the DEVOLVE glyph (a downward
            // chevron — recede, vs evolve's upward become), and the base card in hand is a
            // recede base. The server only offers Devolve on a `combat_faded` form (the §0.5
            // window), so the redacted view already renders it rescuable.
            Command::Devolve { tile, base_hand } => {
                t.insert(*tile);
                devolvable.insert(*tile);
                hand.insert(*base_hand);
                bases.insert(*base_hand);
            }
            Command::PlaySpirit { hand_index, .. }
            | Command::Overwrite { hand_index, .. }
            | Command::CastRitual { hand_index }
            | Command::PlaceLandmark { hand_index, .. }
            | Command::SetFabrication { hand_index, .. }
            | Command::TellUnwriting { hand_index }
            | Command::Release { hand_index } => {
                hand.insert(*hand_index);
            }
            Command::AttachBond {
                hand_index,
                tile_a,
                tile_b,
            } => {
                hand.insert(*hand_index);
                t.insert(*tile_a);
                t.insert(*tile_b);
            }
            Command::Glimpse
            | Command::EndTurn
            | Command::Choose { .. }
            | Command::BanishStray
            | Command::Mulligan { .. }
            | Command::MatchAbandoned { .. } => {}
        }
    }
    Affordances {
        actionable_tiles: t.into_iter().collect(),
        evolvable_tiles: evolvable.into_iter().collect(),
        devolvable_tiles: devolvable.into_iter().collect(),
        actionable_hand: hand.into_iter().collect(),
        evolve_forms: forms.into_iter().collect(),
        devolve_bases: bases.into_iter().collect(),
    }
}

/// The short board labels (card id → short name), id-ordered like the catalog — the
/// board's spirit names. Engine-free; shared by every online/2v2 frame.
fn board_names() -> Vec<String> {
    recollect_core::cards::canon_catalog()
        .iter()
        .map(|c| crate::scene::short_board_name(&c.name))
        .collect()
}

/// The movement cues for the live online/2v2 frame, derived from the **redacted
/// view + the server's legal moves** (no engine): `movable` is every tile the server
/// offers a `MoveSpirit` from (a Mobile spirit with its one step left); `sick` is your
/// Mobile, non-fading spirits whose tile sits in the view's public `moved_this_turn`
/// (spent their move or just arrived). Both read only public state — the `mobile` flag
/// (a public keyword on the view), `moved_this_turn` (public per-turn state), and the
/// legal list — so they leak nothing.
fn move_cues(view: &PlayerView, legal: &[Command]) -> crate::scene::MoveCues {
    use std::collections::BTreeSet;
    let movable: Vec<u8> = {
        let set: BTreeSet<u8> = legal
            .iter()
            .filter_map(|c| match c {
                Command::MoveSpirit { from, .. } => Some(*from),
                _ => None,
            })
            .collect();
        set.into_iter().collect()
    };
    let you = view.seat;
    let sick: Vec<u8> = view
        .tiles
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let sp = t.spirit.as_ref()?;
            (sp.owner == you
                && sp.mobile
                && !sp.fading
                && view.moved_this_turn.contains(&(i as u8)))
            .then_some(i as u8)
        })
        .collect();
    crate::scene::MoveCues { movable, sick }
}

/// Build the **online 1v1** [`ShellModel`] from a redacted [`PlayerView`] + the server's
/// legal commands + the seat display names. The same shell the local 1v1 match draws —
/// board, HUD, hand, affordances, the a11y inputs — but sourced purely from the view (you
/// see only YOUR seat; the opponent is counts/backs). `you_name` / `opp_name` are the
/// session display names (empty falls back to the faction word).
///
/// **Redaction by construction:** `view` is already `view_for(your_seat)`, so it carries
/// only your hand; the opponent rides as `opponent.hand_count` (face-down backs). The
/// erasure tally folds into the opponent's score only when the public `solace_erasures`
/// is nonzero AND the opponent is the conventional Solace seat (B) — in a Lorekeeper PvP
/// match it stays 0, so the score is pure board points.
pub fn shell_model_for_player_view(
    view: PlayerView,
    legal: &[Command],
    you_name: &str,
    opp_name: &str,
) -> ShellModel {
    let you = view.seat;
    let opp = you.other();
    let (a_board, b_board) = board_points(&view.tiles);
    let you_board = if you == Seat::A { a_board } else { b_board };
    let opp_board = if opp == Seat::A { a_board } else { b_board };

    // The off-board erasure tally is public (`solace_erasures`). Without faction data in
    // the view, attribute it to the opponent when they are the conventional Solace seat
    // (B) — i.e. when YOU are seat A. In Lorekeeper PvP it is 0, so this is a no-op there;
    // and it is a public score, never hidden information, so attributing it never leaks.
    let erasures = view.solace_erasures;
    let opp_erasures = if erasures > 0 && opp == Seat::B {
        erasures
    } else {
        0
    };
    let you_erasures = if erasures > 0 && you == Seat::B {
        erasures
    } else {
        0
    };
    let you_score = you_board.saturating_add(you_erasures);
    let opp_score = opp_board.saturating_add(opp_erasures);

    let hand = hand_from_you(&view.you);
    let aff = affordances_from_legal(legal, &view.tiles);
    let cues = move_cues(&view, legal);

    let your_turn = view.active == view.seat;
    let you_anima = view.you.anima;
    let opp_hand_count = view.opponent.hand_count;
    let round = view.round;

    // The opponent strip / HUD name by the session display name when present, else the
    // faction word. Online PvP is Lorekeeper-vs-Lorekeeper by default; the erasure tally
    // (if any) is what distinguishes a Solace opponent, so label the faction by that.
    let you_faction = faction_word(if you_erasures > 0 {
        Faction::Solace
    } else {
        Faction::Lorekeeper
    });
    let opp_faction = faction_word(if opp_erasures > 0 {
        Faction::Solace
    } else {
        Faction::Lorekeeper
    });
    let you_label = if you_name.is_empty() {
        "You".to_string()
    } else {
        you_name.to_string()
    };
    let opp_label = opp_name.to_string();

    // The §5 Glimpse modal is an engine pending choice surfaced to its owner via the
    // redacted view (`view.you.pending`); online it is built the same way as local (so a
    // Glimpse you own raises the modal). The opponent's choice never surfaces (redaction).
    // Computed before `view` is moved into the model below.
    let choice = view
        .you
        .pending
        .as_ref()
        .and_then(|pending| crate::shell::build_choice_prompt(pending, &hand_card_of));

    ShellModel {
        you_seat: you,
        you_name: you_label,
        you_faction,
        you_score,
        you_anima,
        hand,
        opp_name: opp_label,
        opp_faction,
        opp_score,
        opp_erasures,
        opp_hand_count,
        round,
        last_round: recollect_core::engine::LAST_ROUND,
        // Contraction is the default rule (8); the view does not carry it. The clock
        // strip reads the same default the engine uses, so the Dusk/Nightfall pips land.
        dusk_after: recollect_core::state::MatchRules::default().contraction_after,
        your_turn,
        view,
        names: board_names(),
        cues,
        interaction: crate::scene::Interaction::default(),
        actionable_tiles: aff.actionable_tiles,
        evolvable_tiles: aff.evolvable_tiles,
        devolvable_tiles: aff.devolvable_tiles,
        actionable_hand: aff.actionable_hand,
        evolve_forms: aff.evolve_forms,
        devolve_bases: aff.devolve_bases,
        lifted_hand: None,
        hand_scroll: 0.0,
        dragging: false,
        drag_xy: None,
        inspect: None,
        replay: None,
        dusk: None,
        result: None,
        choice,
    }
}

/// Synthesize a `PlayerView` from a 2v2 `TeamView` so the shell (which draws a
/// `PlayerView`-shaped board + your hand) can render the active slot's vantage. The
/// board tiles, your hand, your team's projection, the round, the phase, and the public
/// per-turn state all carry over verbatim; the single "opponent" is the OPPOSING TEAM as
/// **combined counts** (the sum of both rival slots' hand sizes — redaction holds, never
/// their cards). `active`/`seat` are mapped to the team `Seat` so `your_turn` reads right.
fn player_view_from_team(tv: &TeamView) -> (PlayerView, u8) {
    let sum = |f: fn(&OpponentView) -> u8| tv.opponents.iter().map(f).fold(0u8, u8::saturating_add);
    // The opposing team's combined hand size — face-down backs only (redaction).
    let opp_hand_count = sum(|o| o.hand_count);
    let opp_deck_count = sum(|o| o.deck_count);
    // The opposing team's Anima isn't a single value in 2v2; sum it for the (unused-on-
    // strip) field so the shape is whole. The strip shows counts, not the rival Anima.
    let opp_anima = sum(|o| o.anima);

    let pv = PlayerView {
        seat: tv.team,
        round: tv.round,
        active: tv.active_slot.team(),
        phase: tv.phase.clone(),
        tiles: tv.tiles.clone(),
        you: YouView {
            hand: tv.you.hand.clone(),
            deck_count: tv.you.deck_count,
            anima: tv.you.anima,
            peeked_top: tv.you.peeked_top,
            pending: tv.you.pending.clone(),
        },
        opponent: OpponentView {
            hand_count: opp_hand_count,
            deck_count: opp_deck_count,
            anima: opp_anima,
        },
        solace_erasures: tv.solace_erasures,
        moved_this_turn: tv.moved_this_turn.clone(),
        mulliganed: tv.mulliganed,
    };
    (pv, opp_hand_count)
}

/// Build the **2v2** [`ShellModel`] from the active slot's redacted [`TeamView`] + the
/// server's legal commands + the display names. The full shell over the **6×6** board and
/// the four-seat structure: your slot's HUD / hand / affordances, the opposing team as
/// combined counts (face-down backs — redaction holds). `you_name` is your slot's display
/// name; `opp_name` labels the opposing team (empty ⇒ "the opposing team").
///
/// **Redaction by construction:** the `TeamView` already shows your hand only; the
/// teammate and both rivals are counts. The synthesized `PlayerView` folds the opposing
/// team into one counts-only opponent — never any seat's cards.
pub fn shell_model_for_team_view(
    tv: &TeamView,
    legal: &[Command],
    you_name: &str,
    opp_name: &str,
) -> ShellModel {
    let (pv, _opp_hand_count) = player_view_from_team(tv);
    // The 2v2 board is 6×6; the model's affordances + score read off the same redacted
    // tiles. Reuse the 1v1 builder over the synthesized view, then fix the 2v2-only fields.
    let mut model = shell_model_for_player_view(pv, legal, you_name, opp_name);
    if opp_name.is_empty() {
        model.opp_name = "the opposing team".into();
    }
    // `your_turn` must be SLOT-level, not team-level: it is your turn only when YOUR slot is
    // the active one (not your teammate's). The synthesized view's `active`/`seat` are the
    // team Seat, so the 1v1 builder would read true on a teammate's turn too — override it.
    model.your_turn = tv.active_slot == tv.slot;
    model
}

/// The in-canvas **result screen** content for an online / 2v2 match, built purely
/// from the view's `Finished` phase + the redacted tiles + the public erasure tally —
/// no engine. `team` selects the 2v2 `TeamView` (the human is the team seat); `mode` is
/// `"pvp"` / `"2v2"`. Returns `"null"` mid-match or on a malformed view.
///
/// The verdict speaks in the game's voice (the Memory keeps / is forgotten / both kept),
/// the breakdown lists each seat's board points (the Solace's erasure tally folded in),
/// and the actions adapt to the mode — exactly the [`crate::shell::build_result_screen`]
/// the local shell uses, fed from the view instead of the engine. Factions default to
/// Lorekeeper PvP; a nonzero public `solace_erasures` marks the eraser as the Solace.
pub fn result_screen_json(view_json: &str, team: bool, mode: &str) -> String {
    // The few scalars the result screen needs: the result + final scores, the per-seat
    // board points, the erasure tally, and which seat is the human. Read from whichever
    // view shape this is, so 1v1 and 2v2 share one builder.
    let (phase, tiles, erasures, human) = if team {
        match serde_json::from_str::<TeamView>(view_json) {
            Ok(tv) => (tv.phase, tv.tiles, tv.solace_erasures, tv.team),
            Err(_) => return "null".into(),
        }
    } else {
        match serde_json::from_str::<PlayerView>(view_json) {
            Ok(pv) => (pv.phase, pv.tiles, pv.solace_erasures, pv.seat),
            Err(_) => return "null".into(),
        }
    };
    let (result, score_a, score_b) = match phase {
        recollect_core::state::Phase::Finished {
            result,
            score_a,
            score_b,
        } => (result, score_a, score_b),
        _ => return "null".into(),
    };
    let (a_board, b_board) = board_points(&tiles);
    // Factions: Lorekeeper PvP by default; a nonzero public erasure tally marks the
    // conventional Solace seat (B) as the Solace, so the verdict phrases right.
    let faction = |seat: Seat| {
        if erasures > 0 && seat == Seat::B {
            Faction::Solace
        } else {
            Faction::Lorekeeper
        }
    };
    let word = |seat: Seat| {
        if seat == human {
            "you".to_string()
        } else if faction(seat) == Faction::Solace {
            "the Solace".to_string()
        } else {
            "your opponent".to_string()
        }
    };
    let screen = crate::shell::build_result_screen(
        result,
        score_a,
        score_b,
        a_board,
        b_board,
        erasures,
        faction(Seat::A),
        faction(Seat::B),
        &word(Seat::A),
        &word(Seat::B),
        human,
        mode,
    );
    serde_json::to_string(&screen).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use recollect_core::cards::canon_catalog;
    use recollect_core::types::CardKind;
    use recollect_core::view::{view_for, view_for_slot};
    use recollect_core::{Engine, state::Command as C};

    fn spirit_deck() -> Vec<CardId> {
        canon_catalog()
            .iter()
            .filter(|c| c.kind == CardKind::Spirit)
            .take(20)
            .map(|c| c.id)
            .collect()
    }

    fn engine_1v1() -> Engine {
        let cat = canon_catalog();
        let d = spirit_deck();
        Engine::new(7, cat, d.clone(), d).0
    }

    /// The online 1v1 builder produces a ShellModel that draws the board + your hand and
    /// whose affordances match the server's legal moves — with no engine.
    #[test]
    fn online_model_renders_board_hand_and_legal_affordances() {
        let e = engine_1v1();
        let you = Seat::A;
        let view = view_for(&e, you);
        let legal = e.legal_commands(you);
        let model = shell_model_for_player_view(view, &legal, "Warden Ames", "Corin Ashe");
        // Your hand is real cards (catalog stat-blocks), the opponent a count only.
        assert!(!model.hand.is_empty(), "your hand is dealt");
        assert!(
            model.hand.iter().all(|c| !c.name.is_empty()),
            "every hand card names a real catalog card"
        );
        assert_eq!(model.you_name, "Warden Ames");
        assert_eq!(model.opp_name, "Corin Ashe");
        assert!(model.your_turn, "seat A opens");
        // Every actionable hand index corresponds to a legal play in the supplied list.
        for &i in &model.actionable_hand {
            assert!(
                legal.iter().any(|c| matches!(c,
                    C::PlaySpirit { hand_index, .. }
                    | C::Overwrite { hand_index, .. }
                    | C::CastRitual { hand_index }
                    | C::PlaceLandmark { hand_index, .. }
                    | C::SetFabrication { hand_index, .. }
                    | C::TellUnwriting { hand_index }
                    | C::AttachBond { hand_index, .. }
                    | C::Release { hand_index }
                    | C::Evolve { form_hand: hand_index, .. } if *hand_index == i)),
                "actionable hand {i} matches a legal play"
            );
        }
        assert!(
            !model.actionable_hand.is_empty() || !model.actionable_tiles.is_empty(),
            "the opening surfaces an affordance"
        );
    }

    /// REDACTION (invariant 2): the online model holds only the redacted view — there is
    /// no opponent hand list anywhere, only a count. This is the headless guard the task
    /// asks for: online never renders an opponent's hand/deck.
    #[test]
    fn online_model_never_carries_the_opponents_hand() {
        let e = engine_1v1();
        let view = view_for(&e, Seat::A);
        let legal = e.legal_commands(Seat::A);
        let model = shell_model_for_player_view(view, &legal, "You", "Them");
        let json = serde_json::to_string(&model).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        // The opponent rides as a count, never an enumerated hand.
        assert!(
            v["opp_hand_count"].as_u64().is_some(),
            "the opponent is a count"
        );
        assert!(
            v["view"]["opponent"].get("hand").is_none(),
            "no opponent hand array leaks into the model"
        );
        // The only `hand` array in the model is YOUR own (view.you.hand + the HUD hand).
        assert!(
            v["view"]["you"]["hand"].is_array(),
            "your own hand is present (your cards only)"
        );
    }

    fn engine_2v2() -> Engine {
        let cat = canon_catalog();
        let d = spirit_deck();
        let decks = [d.clone(), d.clone(), d.clone(), d];
        Engine::new_2v2(7, cat, decks).0
    }

    /// The 2v2 builder renders the 6×6 board from the active slot's TeamView, with the
    /// opposing team as combined counts — and never their cards.
    #[test]
    fn team_model_renders_six_by_six_and_counts_only_opponents() {
        let e = engine_2v2();
        let slot = e.state().active_slot;
        let tv = view_for_slot(&e, slot);
        // The server computes legal moves for the active slot via `legal_commands(team)`
        // (it reads `active_slot` internally for the per-slot hand/anima).
        let legal = e.legal_commands(slot.team());
        let model = shell_model_for_team_view(&tv, &legal, "Ally One", "");
        // The board is 6×6 (36 tiles).
        assert_eq!(model.view.tiles.len(), 36, "the 2v2 board is 6×6");
        // Your hand is your slot's cards (real catalog entries).
        assert!(!model.hand.is_empty(), "your slot's hand is dealt");
        // The opposing team is the SUM of both rivals' hand sizes — a count, never cards.
        let expected: u8 = tv
            .opponents
            .iter()
            .map(|o| o.hand_count)
            .fold(0u8, u8::saturating_add);
        assert_eq!(
            model.opp_hand_count, expected,
            "the opposing team is combined counts"
        );
        assert_eq!(model.opp_name, "the opposing team", "empty name falls back");
        // No opponent hand list leaks.
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&model).unwrap()).unwrap();
        assert!(v["view"]["opponent"].get("hand").is_none());
    }

    /// 2v2 `your_turn` is SLOT-level, not team-level: it is true only when YOUR slot is the
    /// active one, false on your teammate's turn (else the a11y tree / FABs would advertise
    /// "enabled" on a turn you cannot act). Guards the slot-level override.
    #[test]
    fn team_model_your_turn_is_slot_level_not_team_level() {
        use recollect_core::types::SeatSlot;
        let e = engine_2v2();
        let active = e.state().active_slot; // A1 opens
        let tv_active = view_for_slot(&e, active);
        let legal = e.legal_commands(active.team());
        let m_active = shell_model_for_team_view(&tv_active, &legal, "", "");
        assert!(m_active.your_turn, "the active slot reads your_turn = true");
        // The TEAMMATE slot (same team, not active) must read your_turn = false even though
        // the team Seat IS the active team — the bug the slot-level override fixes.
        let mate = match active {
            SeatSlot::A1 => SeatSlot::A2,
            SeatSlot::A2 => SeatSlot::A1,
            SeatSlot::B1 => SeatSlot::B2,
            SeatSlot::B2 => SeatSlot::B1,
        };
        let tv_mate = view_for_slot(&e, mate);
        let m_mate = shell_model_for_team_view(&tv_mate, &[], "", "");
        assert_eq!(
            tv_mate.team, tv_active.team,
            "the teammate shares the active team"
        );
        assert!(
            !m_mate.your_turn,
            "the teammate slot reads your_turn = false (not its turn)"
        );
    }

    /// REDACTION for 2v2: the synthesized PlayerView carries no rival hand — only the
    /// combined count. (The TeamView's `opponents` are `OpponentView`s, which have no
    /// `hand` field by construction, so this is structurally guaranteed; we assert it.)
    #[test]
    fn team_model_opponent_is_counts_only() {
        let e = engine_2v2();
        let slot = e.state().active_slot;
        let tv = view_for_slot(&e, slot);
        let (pv, count) = player_view_from_team(&tv);
        // The synthesized opponent is counts only.
        let oj: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&pv.opponent).unwrap()).unwrap();
        assert!(oj.get("hand").is_none(), "no rival hand array");
        assert!(oj["hand_count"].as_u64().is_some(), "a rival hand count");
        assert_eq!(oj["hand_count"].as_u64().unwrap() as u8, count);
    }
}
