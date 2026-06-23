//! Core value types: cards (`CardDef`/`CardId`), board geometry (`Reach`, tiles),
//! `Resonance`, `Seat` — the shared vocabulary the engine and data layers speak.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Seat {
    A,
    B,
}

impl Seat {
    pub fn other(self) -> Seat {
        match self {
            Seat::A => Seat::B,
            Seat::B => Seat::A,
        }
    }
    /// +1 means "forward is +y" (Seat A, home rows y∈{0,1}); −1 for Seat B.
    pub fn forward(self) -> i8 {
        match self {
            Seat::A => 1,
            Seat::B => -1,
        }
    }
    pub fn home_rows(self) -> [i8; 2] {
        match self {
            Seat::A => [0, 1],
            Seat::B => [3, 4],
        }
    }
    /// Home rows for a given board width (6×6 pushes B's rows out).
    pub fn home_rows_w(self, w: i8) -> [i8; 2] {
        match self {
            Seat::A => [0, 1],
            Seat::B => [w - 2, w - 1],
        }
    }
}

/// The four physical players in 2v2 (A1, A2 on team A; B1, B2 on team B). Turn
/// order is A1 → B1 → A2 → B2. In 1v1 only A1/B1 are used and a slot's team IS
/// its Seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum SeatSlot {
    A1,
    B1,
    A2,
    B2,
}

impl SeatSlot {
    pub fn team(self) -> Seat {
        match self {
            SeatSlot::A1 | SeatSlot::A2 => Seat::A,
            SeatSlot::B1 | SeatSlot::B2 => Seat::B,
        }
    }
    /// Turn order A1 → B1 → A2 → B2 (then wrap).
    pub fn next_2v2(self) -> SeatSlot {
        match self {
            SeatSlot::A1 => SeatSlot::B1,
            SeatSlot::B1 => SeatSlot::A2,
            SeatSlot::A2 => SeatSlot::B2,
            SeatSlot::B2 => SeatSlot::A1,
        }
    }
    pub fn all_2v2() -> [SeatSlot; 4] {
        [SeatSlot::A1, SeatSlot::B1, SeatSlot::A2, SeatSlot::B2]
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Resonance {
    Neutral,
    Wonder,
    Fear,
    Sorrow,
    Harmony,
    Fury,
    Resolve,
}

impl Resonance {
    /// The display word for this resonance (matches the catalog's `{:?}` rendering:
    /// `"Wonder"`, `"Fury"`, …). A single source for the at-a-glance label so the
    /// deck-style picker and the card frames agree on wording.
    pub fn label(self) -> &'static str {
        use Resonance::*;
        match self {
            Neutral => "Neutral",
            Wonder => "Wonder",
            Fear => "Fear",
            Sorrow => "Sorrow",
            Harmony => "Harmony",
            Fury => "Fury",
            Resolve => "Resolve",
        }
    }

    /// The wheel: Wonder→Fury→Sorrow→Fear→Harmony→Resolve→Wonder.
    /// Opposing pairs (Wonder/Fear, Sorrow/Harmony, Fury/Resolve) are neutral.
    pub fn edge_over(self, other: Resonance) -> bool {
        if self == Resonance::Neutral || other == Resonance::Neutral {
            return false; // the wheel has no spoke for the unaligned
        }
        use Resonance::*;
        matches!(
            (self, other),
            (Wonder, Fury)
                | (Fury, Sorrow)
                | (Sorrow, Fear)
                | (Fear, Harmony)
                | (Harmony, Resolve)
                | (Resolve, Wonder)
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Reach {
    Cross,
    Slant,
    Lance,
    Wide,
    Spire,
    Veil,
    Burst,
}

impl Reach {
    /// Offsets as (dx, dy) where +dy is "forward" (toward the opponent).
    /// Orientation is applied per seat by the engine. Reach does triple duty:
    /// arrival targeting, standing interception, placement projection.
    pub fn offsets(self) -> &'static [(i8, i8)] {
        match self {
            Reach::Cross => &[(0, 1), (0, -1), (1, 0), (-1, 0)],
            Reach::Slant => &[(1, 1), (1, -1), (-1, 1), (-1, -1)],
            Reach::Lance => &[(0, 1), (0, 2)],
            Reach::Wide => &[(-1, 0), (1, 0), (0, 1)],
            Reach::Spire => &[(0, 1), (0, -1)],
            Reach::Veil => &[(-1, -1), (0, -1), (1, -1)],
            Reach::Burst => &[
                (0, 1),
                (0, -1),
                (1, 0),
                (-1, 0),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CardId(pub u16);

/// Stats: Attack / Defense / HP, tens-and-hundreds, multiples of 5.
/// (HP is called Form in lore; the frame — and the engine — say HP.)
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CardDef {
    pub id: CardId,
    pub name: String,
    /// Stable, name-independent identity (frozen slug). Effects + engine logic key off this,
    /// not the display `name` — so a rename never breaks an association. See card_keys.json.
    #[serde(default)]
    pub key: String,
    pub cost: u8,
    pub attack: i16,
    pub defense: i16,
    pub hp: i16,
    pub reach: Reach,
    pub resonance: Resonance,
    pub arcane: bool,
    pub warded: bool,
    pub mobile: bool,
    pub steadfast: bool,
    pub relentless: bool,
    /// Lurkers enter the telling face-down.
    #[serde(default)]
    pub lurk: bool,
    #[serde(default)]
    pub kind: CardKind,
    #[serde(default)]
    pub rarity: String,
    #[serde(default)]
    pub imprints: Vec<String>,
    #[serde(default)]
    pub rules: String,
    /// The forms this base may evolve into (Primal then Fabled).
    #[serde(default)]
    pub evolves_to: Vec<String>,
    /// The base/lower form this evolution grows from.
    #[serde(default)]
    pub evolves_from: Option<String>,
}

impl CardDef {
    /// Is this an antagonist creature (an Unwritten of any kind)?
    pub fn is_unwritten(&self) -> bool {
        self.kind.is_antagonist_creature()
    }
    /// An ordinary (envious) Unwritten — NOT the sinister ill-intent subset.
    /// Used to compose a "gentle" Solace deck.
    pub fn is_plain_unwritten(&self) -> bool {
        matches!(self.kind, CardKind::Unwritten)
    }
    /// One of the ill intent — the sinister Unwritten subset. Used to compose a
    /// "cruel" Solace deck.
    pub fn is_ill_intent(&self) -> bool {
        matches!(self.kind, CardKind::IllIntent)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CardKind {
    #[default]
    Spirit,
    Caller,
    Evolution,
    Ritual,
    Bond,
    Landmark,
    Fabrication,
    /// One of the six summoned creatures — a Kindred (design §10). Manifested by its
    /// caller onto an adjacent tile; occupies and scores while alive, dissolves to no
    /// impression, cannot evolve, may be Bonded, and fades if its caller leaves play.
    Kindred,
    /// An ordinary antagonist creature — an Unwritten (the envious, unfinished
    /// never-remembered). Leaves no impression when it falls.
    Unwritten,
    /// The sinister subset of the Unwritten — the ill intent (envy curdled into
    /// spite/hunger). Mechanically an Unwritten (leaves no impression); the distinct
    /// kind drives PvE deck composition (a cruel Solace fields these).
    IllIntent,
    /// An Unwriting event — the Solace plays these from hand (erase impressions,
    /// release fading spirits, encroach).
    Unwriting,
    Foundling,
}

impl CardKind {
    /// Both antagonist creature kinds (ordinary Unwritten + the sinister ill intent).
    /// Both leave no impression and are faced in PvE, never collected.
    pub fn is_antagonist_creature(self) -> bool {
        matches!(self, CardKind::Unwritten | CardKind::IllIntent)
    }
}

impl CardKind {
    /// What the engine can put in a deck and play: spirits, callers, the spellbook,
    /// and **evolution forms**. A Primal/Fabled form is a deck-playable card you draw
    /// to hand and *play onto its matching base* during Main (you must hold and pay
    /// for it). Deck construction pairs each form with a base that reaches it (no
    /// orphan evolutions); IllIntent, Kindred, and Foundlings remain non-deck.
    pub fn deck_playable(self) -> bool {
        // The spellbook joins spirits and callers in legal decks; evolution FORMS are
        // deck-playable (drawn, then played onto a base).
        matches!(
            self,
            CardKind::Spirit
                | CardKind::Caller
                | CardKind::Evolution
                | CardKind::Ritual
                | CardKind::Bond
                | CardKind::Landmark
                | CardKind::Fabrication
        )
    }

    /// Deck-eligible kinds for a faction's deck standard. Lorekeeper decks draw
    /// the spellbook above; the Solace draws its antagonist set (Unwritten
    /// creatures, IllIntent creatures, Unwriting events) **plus its Primal
    /// Deepenings** — the Solace deepens (Primal forms, never Fabled), so its
    /// `Evolution` forms are deck-playable too (drawn, then played onto an
    /// Unwritten base). The Primal-only constraint (a Solace deck never holds a
    /// Fabled, and a form's base must be a Solace creature) is enforced at the
    /// card level by [`crate::cards::validate_deck_for`], which sees the rarity +
    /// the no-orphan pairing; this kind-level gate just admits the FORM kind.
    /// The engine plays cards faction-agnostically — this only gates deck
    /// *construction*.
    pub fn deck_playable_for(self, faction: Faction) -> bool {
        match faction {
            Faction::Lorekeeper => self.deck_playable(),
            Faction::Solace => matches!(
                self,
                CardKind::Unwritten
                    | CardKind::IllIntent
                    | CardKind::Unwriting
                    | CardKind::Evolution
            ),
        }
    }
}

/// A player's faction — its card pool + deck ruleset. Every seat has one. The engine
/// plays cards faction-agnostically (rules key on `CardKind`, not faction), so faction
/// drives deck-building, the bot, and the UI — never the turn mechanics. The Solace is
/// bot-only and UI-gated from human selection; making it human-playable is a UI unlock,
/// not an engine change. (A seat is `{ faction, deck, controller }`.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Faction {
    #[default]
    Lorekeeper,
    Solace,
}

impl Default for CardDef {
    fn default() -> Self {
        CardDef {
            id: CardId(0),
            name: String::new(),
            key: String::new(),
            cost: 0,
            attack: 0,
            defense: 0,
            hp: 0,
            reach: Reach::Cross,
            resonance: Resonance::Wonder,
            arcane: false,
            warded: false,
            mobile: false,
            steadfast: false,
            relentless: false,
            lurk: false,
            kind: CardKind::Spirit,
            rarity: String::new(),
            imprints: vec![],
            rules: String::new(),
            evolves_to: vec![],
            evolves_from: None,
        }
    }
}

pub const BOARD_W: i8 = 5;
pub const BOARD_TILES: usize = 25;
/// 2v2 plays 6×6. Geometry is parameterized by width; the 5-wide wrappers below
/// are the 1v1 call sites.
pub const BOARD_W_2V2: i8 = 6;
pub const BOARD_TILES_2V2: usize = 36;

pub fn tile_xy_w(t: u8, w: i8) -> (i8, i8) {
    ((t as i8 % w), (t as i8 / w))
}
pub fn xy_tile_w(x: i8, y: i8, w: i8) -> Option<u8> {
    if (0..w).contains(&x) && (0..w).contains(&y) {
        Some((y * w + x) as u8)
    } else {
        None
    }
}
pub fn is_rim_w(t: u8, w: i8) -> bool {
    let (x, y) = tile_xy_w(t, w);
    x == 0 || x == w - 1 || y == 0 || y == w - 1
}
pub fn adjacent4_w(t: u8, w: i8) -> impl Iterator<Item = u8> {
    let (x, y) = tile_xy_w(t, w);
    [(1i8, 0i8), (-1, 0), (0, 1), (0, -1)]
        .into_iter()
        .filter_map(move |(dx, dy)| xy_tile_w(x + dx, y + dy, w))
}
// 5-wide wrappers: the 1v1 call sites.
pub fn tile_xy(t: u8) -> (i8, i8) {
    tile_xy_w(t, BOARD_W)
}
pub fn xy_tile(x: i8, y: i8) -> Option<u8> {
    xy_tile_w(x, y, BOARD_W)
}
pub fn is_rim(t: u8) -> bool {
    is_rim_w(t, BOARD_W)
}
pub fn adjacent4(t: u8) -> impl Iterator<Item = u8> {
    adjacent4_w(t, BOARD_W)
}

#[cfg(test)]
mod geometry_tests {
    //! The board coordinate primitives are foundational to every reach / move /
    //! adjacency computation, so they are pinned here exhaustively by their
    //! *invariants* -- tile<->(x,y) round-trip, the half-open bound, orthogonal-
    //! neighbour distance and count -- rather than by hand-listed coordinates.
    //! Invariants stay true across a board-size change where pinned literals would
    //! shatter, and they kill the arithmetic mutants (a `+1`/`-1`, a `%`/`/`, a
    //! `<`/`<=` bound, a dropped/flipped offset) that the broad reach / fuzz tests
    //! exercise only in aggregate.
    use super::*;

    // Every real board width: 1v1 (5x5) and 2v2 (6x6).
    const DIMS: [(i8, usize); 2] = [(BOARD_W, BOARD_TILES), (BOARD_W_2V2, BOARD_TILES_2V2)];

    #[test]
    fn tile_xy_round_trips_for_every_tile_at_both_widths() {
        for (w, tiles) in DIMS {
            for t in 0..tiles as u8 {
                let (x, y) = tile_xy_w(t, w);
                assert!(
                    (0..w).contains(&x) && (0..w).contains(&y),
                    "tile {t} (w={w}) mapped off-board to ({x},{y})"
                );
                assert_eq!(
                    xy_tile_w(x, y, w),
                    Some(t),
                    "tile {t} (w={w}): ({x},{y}) did not round-trip back to {t}"
                );
            }
        }
    }

    #[test]
    fn xy_tile_accepts_exactly_the_on_board_coordinates() {
        // One ring past every edge: the half-open [0, w) bound means x==w or y==w
        // (and -1) are off-board. A `<`->`<=` mutant would admit the first off-board
        // row/column.
        for (w, _) in DIMS {
            for y in -1..=w {
                for x in -1..=w {
                    let on_board = (0..w).contains(&x) && (0..w).contains(&y);
                    assert_eq!(
                        xy_tile_w(x, y, w).is_some(),
                        on_board,
                        "xy_tile_w({x},{y}, w={w}): on_board should be {on_board}"
                    );
                }
            }
        }
    }

    #[test]
    fn adjacent4_is_exactly_the_on_board_orthogonal_neighbours() {
        for (w, tiles) in DIMS {
            for t in 0..tiles as u8 {
                let (tx, ty) = tile_xy_w(t, w);
                let neighbours: Vec<u8> = adjacent4_w(t, w).collect();
                // Each neighbour is on-board and exactly one orthogonal step away --
                // kills an offset sign-flip or a diagonal leak.
                for &n in &neighbours {
                    let (nx, ny) = tile_xy_w(n, w);
                    assert_eq!(
                        (nx - tx).abs() + (ny - ty).abs(),
                        1,
                        "tile {t}->{n} (w={w}) is not a 1-step orthogonal neighbour"
                    );
                }
                // No duplicates.
                let mut uniq = neighbours.clone();
                uniq.sort_unstable();
                uniq.dedup();
                assert_eq!(
                    uniq.len(),
                    neighbours.len(),
                    "tile {t} (w={w}) has duplicate neighbours"
                );
                // Count matches the geometry independently of the offset table
                // (corner 2, edge 3, interior 4) -- kills a dropped/duplicated dir.
                let on_x_edge = tx == 0 || tx == w - 1;
                let on_y_edge = ty == 0 || ty == w - 1;
                let expect = match (on_x_edge, on_y_edge) {
                    (true, true) => 2,
                    (true, false) | (false, true) => 3,
                    (false, false) => 4,
                };
                assert_eq!(
                    neighbours.len(),
                    expect,
                    "tile {t} (w={w}) at ({tx},{ty}) should have {expect} neighbours"
                );
            }
        }
    }

    #[test]
    fn is_rim_matches_the_edge_coordinates() {
        for (w, tiles) in DIMS {
            for t in 0..tiles as u8 {
                let (x, y) = tile_xy_w(t, w);
                let on_edge = x == 0 || x == w - 1 || y == 0 || y == w - 1;
                assert_eq!(
                    is_rim_w(t, w),
                    on_edge,
                    "is_rim_w({t}, w={w}) disagrees with the edge test"
                );
            }
        }
    }

    #[test]
    fn the_1v1_wrappers_match_the_width_form_at_board_w() {
        for t in 0..BOARD_TILES as u8 {
            assert_eq!(tile_xy(t), tile_xy_w(t, BOARD_W));
            assert_eq!(is_rim(t), is_rim_w(t, BOARD_W));
            assert_eq!(
                adjacent4(t).collect::<Vec<_>>(),
                adjacent4_w(t, BOARD_W).collect::<Vec<_>>()
            );
        }
        for y in -1..=BOARD_W {
            for x in -1..=BOARD_W {
                assert_eq!(xy_tile(x, y), xy_tile_w(x, y, BOARD_W));
            }
        }
    }

    #[test]
    fn adjacent4_order_is_the_e_w_s_n_determinism_contract() {
        // The neighbour *order* is a determinism contract, not an implementation
        // detail: consumers take the FIRST match (`flow.rs` the first enemy-
        // impression tile, `clause.rs` the first open Kindred-landing tile) and
        // emit per-neighbour events in iteration order (`effects_fire.rs`), so a
        // reorder changes which tile is picked and the event stream. The offset
        // table [(1,0),(-1,0),(0,1),(0,-1)] fixes the sequence to East, West,
        // South, North -- pinned here so the `x±dx`/`y±dy` sign mutants (which keep
        // the same neighbour *set* but reorder it) are caught.
        for w in [BOARD_W, BOARD_W_2V2] {
            let t = xy_tile_w(2, 2, w).unwrap(); // interior: all four are on-board
            let got: Vec<u8> = adjacent4_w(t, w).collect();
            let expect = vec![
                xy_tile_w(3, 2, w).unwrap(), // East  (x+1)
                xy_tile_w(1, 2, w).unwrap(), // West  (x-1)
                xy_tile_w(2, 3, w).unwrap(), // South (y+1)
                xy_tile_w(2, 1, w).unwrap(), // North (y-1)
            ];
            assert_eq!(got, expect, "adjacent4_w({t}, w={w}) order must be E,W,S,N");
        }
    }
}
