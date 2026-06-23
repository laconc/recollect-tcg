//! Per-seat views — redaction by construction. The opponent's hand, deck
//! order, and Glimpse peek never appear in your view; only counts do.
use crate::engine::{Engine, projection};
use crate::state::{GameState, PendingChoice, Phase, PlayerState};
use crate::types::{CardId, Seat};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpiritView {
    pub card: CardId,
    pub owner: Seat,
    pub attack: i16,
    pub defense: i16,
    pub hp: i16,
    pub hp_max: i16,
    pub fading: bool,
    /// The **standing-Faded** (rescuable) window: this spirit was **banished in
    /// combat** and is lingering through its owner's Main — `fading` AND it has a
    /// `fade_deadline` (the §0.5 window, not an uncontested Dusk fade). This is the
    /// state in which the owner may **evolve** (Primal-rescue) or **devolve** (recede)
    /// it; an ordinary `fading` base with no deadline (the Dusk sweep) is NOT rescuable.
    /// PUBLIC board state — the opponent can see a banished form linger too — so it is
    /// redaction-safe (it carries no hidden identity). The renderer reads it to draw a
    /// rescuable spirit DISTINCTLY from both a live spirit and an unrecoverable fade
    /// (`web_client_ux.md` §standing-Faded). `#[serde(default)]` for wire compat.
    #[serde(default)]
    pub combat_faded: bool,
    pub echo: bool,
    /// The Mobile keyword (a public card property): whether this spirit *may* take
    /// its one step a turn at all. Drives the can-move cue — a networked
    /// client has no catalog to look this up, so the view carries it. Redacted to
    /// `false` for a hidden enemy lurker, exactly like the stat block (its keywords
    /// are not yet known). Whether it has *already* moved this turn is the separate,
    /// per-turn [`PlayerView::moved_this_turn`].
    #[serde(default)]
    pub mobile: bool,
    /// Face-down to the viewer. When true and `card`/`attack`/… are
    /// the redaction sentinels, only "a lurker stands here" is known.
    pub face_down: bool,
    /// Evolution transparency: the forms this base can become, shown to BOTH
    /// players (name + tier). Empty for non-evolvers and hidden lurkers.
    #[serde(default)]
    pub evolutions: Vec<EvolutionOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionOption {
    pub form: String,
    /// "Primal" or "Fabled" — tells the player which fuel it needs.
    pub tier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileView {
    pub spirit: Option<SpiritView>,
    /// The single impression on this tile, if any — the banisher's mark (one per tile,
    /// last-wins; a new banish overwrites the old). The engine stores `Vec<Seat>` internally but
    /// the rendered contract is one mark per tile (we tried stacking and opted against it).
    pub impression: Option<Seat>,
    pub faded: bool,
    /// Your ink wash: a tile your telling can reach.
    pub in_your_projection: bool,
    /// The terrain on this tile, if any — a Landmark (always shown)
    /// or a Fabrication (shown openly to its owner; an enemy's face-down lie
    /// shows only as hidden terrain). `None` for an empty tile. Renderers MUST
    /// draw this; before this field, Landmarks and Fabrications were invisible.
    pub terrain: Option<TerrainView>,
}

/// What a renderer needs to draw a piece of terrain. A face-down enemy
/// Fabrication is redacted to `kind = Fabrication, face_down = true` with no
/// card identity — the lie is visible as a lie, not as its contents.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TerrainView {
    pub card: CardId,
    pub owner: Seat,
    /// "Landmark" or "Fabrication".
    pub kind: String,
    pub face_down: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YouView {
    pub hand: Vec<CardId>,
    pub deck_count: u8,
    pub anima: u8,
    pub peeked_top: Option<CardId>,
    /// YOUR pending choice (peeked cards, target options). The
    /// opponent's view never carries this — redaction test enforced.
    #[serde(default)]
    pub pending: Option<crate::state::PendingChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpponentView {
    pub hand_count: u8,
    pub deck_count: u8,
    pub anima: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerView {
    pub seat: Seat,
    pub round: u8,
    pub active: Seat,
    pub phase: Phase,
    pub tiles: Vec<TileView>,
    pub you: YouView,
    pub opponent: OpponentView,
    /// The Solace's running off-board erasure tally: +1 each time the Solace
    /// erases a scoring presence (banishing a spirit, Unwriting an impression). It
    /// is public — a score, not hidden information — and lets a client show the
    /// Solace's standing MID-game, since the Unwritten leave no board mark and the
    /// tally is otherwise only folded into seat B's total at Nightfall.
    #[serde(default)]
    pub solace_erasures: u8,
    /// Tiles whose spirit has spent its one Move this turn OR just arrived this turn
    /// (summoning sickness). Public board state — the same per-turn transient
    /// the engine tracks; a client cross-references it with each tile's [`SpiritView::mobile`]
    /// to paint the can-still-step / rested cue without re-deriving move legality.
    #[serde(default)]
    pub moved_this_turn: Vec<u8>,
    /// Mulligan (§5): whether each seat `[A, B]` has spent its opening mulligan.
    /// PUBLIC — the opponent learns THAT you mulliganed (the redraw + bottomed card
    /// stay hidden), so a client can render the public beat ("your opponent
    /// mulliganed"). Carries no card identity: a boolean per seat, never the hand.
    #[serde(default)]
    pub mulliganed: [bool; 2],
}

/// The four-seat view. `you` is the specific SLOT's private state;
/// `teammate` is your ally's public counts (you share a team but not a hand);
/// `opponents` are both rival slots. Hands are redacted exactly as in 1v1:
/// you see your own cards, everyone else's are counts only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamView {
    pub slot: crate::types::SeatSlot,
    pub team: Seat,
    pub round: u8,
    pub active_slot: crate::types::SeatSlot,
    pub board_w: i8,
    pub phase: Phase,
    pub tiles: Vec<TileView>,
    pub you: YouView,
    pub teammate: OpponentView,
    pub opponents: Vec<OpponentView>,
    /// The Solace team's running off-board erasure tally — see
    /// [`PlayerView::solace_erasures`]. Public; surfaced for the same mid-game
    /// score readout in 2v2.
    #[serde(default)]
    pub solace_erasures: u8,
    /// Tiles whose spirit has moved or just arrived this turn — see
    /// [`PlayerView::moved_this_turn`]. Public per-turn board state.
    #[serde(default)]
    pub moved_this_turn: Vec<u8>,
    /// Mulligan (§5): the public per-seat mulligan beat — see
    /// [`PlayerView::mulliganed`]. The mulligan is a 1v1 opening mechanic, so in a
    /// 2v2 telling this stays `[false, false]`; the field rides the view for one
    /// uniform shape across modes.
    #[serde(default)]
    pub mulliganed: [bool; 2],
}

/// Build the redacted tile list from `seat`'s vantage (team-shared projection).
fn tiles_for(engine: &Engine, seat: Seat) -> Vec<TileView> {
    let st = engine.state();
    let proj = projection(st, seat, engine.catalog_ref());
    st.board
        .iter()
        .enumerate()
        .map(|(i, t)| TileView {
            spirit: t.spirit.as_ref().map(|sp| {
                // Redaction: an enemy lurker shows only as face-down —
                // identity and numbers withheld until it steps into light.
                let hidden = sp.face_down && sp.owner != seat;
                SpiritView {
                    card: if hidden { CardId(u16::MAX) } else { sp.card },
                    owner: sp.owner,
                    attack: if hidden { 0 } else { sp.attack },
                    defense: if hidden { 0 } else { sp.defense },
                    hp: if hidden { 0 } else { sp.hp },
                    hp_max: if hidden { 0 } else { sp.hp_max },
                    fading: sp.fading,
                    // Standing-Faded (rescuable): banished in combat, in its §0.5 window
                    // (`fading` with a `fade_deadline`). Public board state — but a hidden
                    // lurker reveals nothing, so report false alongside its redacted stats.
                    combat_faded: !hidden && sp.fading && sp.fade_deadline.is_some(),
                    echo: !hidden && sp.echo_eligible(),
                    // Mobile is a public keyword, but a hidden lurker's keywords are
                    // not yet known — redact to false alongside its stats.
                    mobile: !hidden && engine.card(sp.card).mobile,
                    face_down: sp.face_down,
                    evolutions: if hidden {
                        Vec::new()
                    } else {
                        // Evolution transparency: both players see what a base
                        // can become and which fuel (tier) it needs.
                        let def = engine.card(sp.card);
                        def.evolves_to
                            .iter()
                            .filter_map(|name| {
                                engine
                                    .catalog_ref()
                                    .iter()
                                    .find(|c| &c.name == name)
                                    .map(|f| EvolutionOption {
                                        form: f.name.clone(),
                                        tier: f.rarity.clone(),
                                    })
                            })
                            .collect()
                    },
                }
            }),
            impression: t.impressions.first().copied(),
            faded: t.faded,
            in_your_projection: proj[i],
            terrain: t.terrain.as_ref().map(|tr| {
                // Redaction: an enemy's face-down Fabrication shows as a hidden
                // lie (kind + face_down), not its identity. Landmarks are open.
                // Curio Fox: a Fabrication this seat privately peeked (still the same
                // card at this tile) is un-redacted for THIS seat only. Beacon: an enemy
                // Fabrication adjacent to this seat's face-up Beacon is revealed to it.
                let enemy_fab = tr.face_down && tr.owner != seat;
                let peeked = st.peeked_fabs[seat as usize]
                    .iter()
                    .any(|&(pt, pc)| pt == i as u8 && pc == tr.card);
                let by_beacon = enemy_fab
                    && tr.kind == crate::state::TerrainKind::Fabrication
                    && crate::engine::beacon_reveals_fab(st, engine.catalog_ref(), seat, i as u8);
                let hidden = enemy_fab && !peeked && !by_beacon;
                TerrainView {
                    card: if hidden { CardId(u16::MAX) } else { tr.card },
                    owner: tr.owner,
                    kind: format!("{:?}", tr.kind),
                    face_down: tr.face_down,
                }
            }),
        })
        .collect()
}

/// The redaction filter for a pending choice: a choice is surfaced ONLY to the
/// seat it belongs to (a 2v2 slot inherits its team's `seat`). The opponent sees
/// only THAT a choice is in flight (carried by `phase`) — never the burnable
/// hand, the burned/peeked card, or which option was taken. THE single seam both
/// view builders use, so the redaction can't drift between them (invariant #2).
fn pending_for(st: &GameState, seat: Seat) -> Option<PendingChoice> {
    st.pending_choice.clone().filter(|pc| pc.seat() == seat)
}

/// Assemble the private [`YouView`] — your own hand, counts, peek, and your
/// (redacted) pending choice. `player` is the acting seat/slot's `PlayerState`;
/// `seat` is whose pending choice to surface (the slot's team in 2v2). Shared by
/// [`view_for`] and [`view_for_slot`] so the you/pending block lives once.
fn you_view(player: &PlayerState, st: &GameState, seat: Seat) -> YouView {
    YouView {
        hand: player.hand.clone(),
        deck_count: player.deck.len() as u8,
        anima: player.anima,
        peeked_top: player.peeked_top,
        pending: pending_for(st, seat),
    }
}

/// Build the redacted [`PlayerView`] for `seat` — the *only* state a client ever
/// sees. Opponent hands, deck order, and Echo pre-knowledge are never included
/// (the redaction invariant; see AGENTS.md). The seed appears in no view.
pub fn view_for(engine: &Engine, seat: Seat) -> PlayerView {
    let st = engine.state();
    let tiles = tiles_for(engine, seat);
    let you = st.player(seat);
    let opp = st.player(seat.other());
    PlayerView {
        seat,
        round: st.round,
        active: st.active,
        phase: st.phase.clone(),
        tiles,
        you: you_view(you, st, seat),
        opponent: OpponentView {
            hand_count: opp.hand.len() as u8,
            deck_count: opp.deck.len() as u8,
            anima: opp.anima,
        },
        solace_erasures: st.solace_erasures,
        moved_this_turn: st.moved_this_turn.clone(),
        mulliganed: st.mulliganed,
    }
}

/// The four-seat view for one physical player (`slot`). You see your own
/// hand; your teammate and both opponents are public counts only. Projection
/// is team-shared, so the highlighted tiles are your team's.
pub fn view_for_slot(engine: &Engine, slot: crate::types::SeatSlot) -> TeamView {
    use crate::types::SeatSlot;
    let st = engine.state();
    let team = slot.team();
    let tiles = tiles_for(engine, team);
    let you = st.player_slot(slot);
    // The teammate is the OTHER slot on your team.
    let mate_slot = match slot {
        SeatSlot::A1 => SeatSlot::A2,
        SeatSlot::A2 => SeatSlot::A1,
        SeatSlot::B1 => SeatSlot::B2,
        SeatSlot::B2 => SeatSlot::B1,
    };
    let pub_view = |s: SeatSlot| {
        let p = st.player_slot(s);
        OpponentView {
            hand_count: p.hand.len() as u8,
            deck_count: p.deck.len() as u8,
            anima: p.anima,
        }
    };
    let opponents = if team == Seat::A {
        vec![pub_view(SeatSlot::B1), pub_view(SeatSlot::B2)]
    } else {
        vec![pub_view(SeatSlot::A1), pub_view(SeatSlot::A2)]
    };
    TeamView {
        slot,
        team,
        round: st.round,
        active_slot: st.active_slot,
        board_w: st.board_w,
        phase: st.phase.clone(),
        tiles,
        you: you_view(you, st, team),
        teammate: pub_view(mate_slot),
        opponents,
        solace_erasures: st.solace_erasures,
        moved_this_turn: st.moved_this_turn.clone(),
        mulliganed: st.mulliganed,
    }
}
