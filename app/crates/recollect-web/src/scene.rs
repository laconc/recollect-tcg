//! The renderer-agnostic **scene**: a `PlayerView` (or `TeamView`) turned into a
//! flat list of draw primitives — quads and text labels in tile-grid coordinates.
//!
//! This is the renderer's brain, and deliberately backend-free: it is pure Rust,
//! native-unit-tested, and is where the data-completeness lives (terrain,
//! evolutions, impressions, and projection washes all become primitives here). The
//! wgpu backend (`render.rs`) only scales these to the viewport and draws them, so
//! "did we draw the Landmark?" is a `cargo test` question, not an eyeball-the-canvas
//! one. Rendering may use floats (it is presentation, not rules).
use recollect_core::types::Seat;
use recollect_core::view::{PlayerView, TeamView, TileView};

/// Linear RGBA, 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
}

/// Draw order, low to high — the backend sorts by this so a spirit sits over its
/// wash and a marker over its spirit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    Wash = 0,
    /// The **board grid** — the thin ink rules between cells (and the outer frame)
    /// that make the page read as a distinct 5×5 (6×6 in 2v2) lattice of tiles, not a
    /// blank rectangle (the design's "the board is a page" — `web_client_ux.md`
    /// §In-canvas layout). Drawn over the projection wash but under everything an
    /// occupant or overlay puts in a cell, so the lattice always shows through.
    Grid = 1,
    Faded = 2,
    /// The **lamplit** pool of light under a *held* occupant on a faded (Dusk-dark)
    /// tile — drawn over [`Layer::Faded`] so the held spirit reads visibly different
    /// from both the live board and the dark rim (design §5's readability law: "a held
    /// tile renders lamplit… the light goes out the moment the spirit leaves"). No
    /// reach overlay accompanies it (no zone, no overlay).
    Lamp = 3,
    Impression = 4,
    Terrain = 5,
    Spirit = 6,
    Marker = 7,
}

/// A filled rectangle in tile-grid units (one cell is 1.0 × 1.0; the origin is the
/// top-left of tile 0, x increasing right, y increasing down).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: Color,
    pub layer: Layer,
}

/// A short text label anchored at a tile-grid point (the backend picks a font). `size` is the
/// glyph height in **tile-grid units** (1.0 = one cell tall); the default [`Label::DEFAULT_SIZE`]
/// is the standard board label height. A compacted board card uses a smaller size for its
/// stat foot.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub color: Color,
    /// Glyph height in tile-grid units (defaults to [`Label::DEFAULT_SIZE`]).
    pub size: f32,
}

impl Label {
    /// The standard board label height in tile-grid units (a spirit's short name / terrain glyph).
    pub const DEFAULT_SIZE: f32 = 0.2;
}

/// Everything to draw for one frame, plus the board width so the backend knows
/// the aspect ratio (5×5 in 1v1, 6×6 in 2v2 TeamView).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    pub board_w: u32,
    pub board_h: u32,
    pub quads: Vec<Quad>,
    pub labels: Vec<Label>,
}

/// Movement cues, by tile index: which of the active seat's Mobile spirits can
/// still take their one Move this turn (`movable`), and which are Mobile but rested
/// — they spent their move or just arrived and are summoning-sick (`sick`). The
/// `PlayerView` does not carry the per-turn move state, so the local shell computes
/// these from its in-process engine and passes them in; an online client (no engine)
/// passes the default empty set until the view carries the data. Pure render hints —
/// never a rules input.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MoveCues {
    pub movable: Vec<u8>,
    pub sick: Vec<u8>,
}

/// Pointer + keyboard input overlays, by tile index. `legal` is the set of
/// tiles a legal-target highlight should glow on — the destinations of the
/// currently-selected spirit or hand card (a spirit's Move tiles, a card's
/// Play/Overwrite tiles, or — for a selected evolution form card — the
/// matching base it can be played onto to evolve). `selected` is
/// the one tile the player has picked up (a chosen spirit, drawn ringed). `focus`
/// is the keyboard cursor's current tile — a focus ring so arrow-key navigation
/// is visible, the canvas counterpart of a DOM `:focus-visible` outline. All are
/// pure render hints derived in JS from the engine's legal-move list; never a
/// rules input. The default (no selection, no focus) draws exactly the plain board.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Interaction {
    /// Tiles a legal-target highlight glows on (derived from the legal moves).
    pub legal: Vec<u8>,
    /// The picked-up spirit's tile, if one is selected (drawn ringed).
    pub selected: Option<u8>,
    /// The keyboard focus cursor's tile, if the cursor is on the board.
    pub focus: Option<u8>,
}

impl Scene {
    /// A frame between `prev` and `next` at progress `t` (0→1), for animating state
    /// changes: spirit quads that LEAVE (a banish) fade out and ones that ENTER (a
    /// play) fade in, matched by tile cell (`layer == Spirit`); the board itself snaps
    /// to `next`. A move reads as a quick fade across — a true slide would want a
    /// spirit-instance id in the view (a later refinement). At `t >= 1` this equals `next`.
    pub fn interpolate(prev: &Scene, next: &Scene, t: f32) -> Scene {
        let t = t.clamp(0.0, 1.0);
        let mut quads: Vec<Quad> = Vec::with_capacity(next.quads.len() + 4);
        for q in &next.quads {
            let entering = q.layer == Layer::Spirit
                && !prev
                    .quads
                    .iter()
                    .any(|p| p.layer == Layer::Spirit && same_cell(p, q));
            quads.push(if entering { faded(q, t) } else { *q });
        }
        if 1.0 - t > 0.001 {
            for p in &prev.quads {
                if p.layer == Layer::Spirit
                    && !next
                        .quads
                        .iter()
                        .any(|q| q.layer == Layer::Spirit && same_cell(q, p))
                {
                    quads.push(faded(p, 1.0 - t)); // leaving → fade out
                }
            }
        }
        Scene {
            board_w: next.board_w,
            board_h: next.board_h,
            quads,
            labels: next.labels.clone(),
        }
    }
}

fn same_cell(a: &Quad, b: &Quad) -> bool {
    (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01
}

fn faded(q: &Quad, a: f32) -> Quad {
    Quad {
        color: Color {
            a: q.color.a * a,
            ..q.color
        },
        ..*q
    }
}

#[cfg(test)]
mod anim_tests {
    use super::*;
    fn spirit(x: f32, y: f32) -> Quad {
        Quad {
            x,
            y,
            w: 1.0,
            h: 1.0,
            color: Color::rgba(0.2, 0.4, 0.6, 1.0),
            layer: Layer::Spirit,
        }
    }
    #[test]
    fn interpolate_fades_leavers_enterers_and_lands_on_next() {
        let prev = Scene {
            board_w: 5,
            board_h: 5,
            quads: vec![spirit(1.0, 1.0)],
            labels: vec![],
        };
        let next = Scene {
            board_w: 5,
            board_h: 5,
            quads: vec![spirit(3.0, 2.0)],
            labels: vec![],
        };
        let mid = Scene::interpolate(&prev, &next, 0.5);
        let entering = mid
            .quads
            .iter()
            .find(|q| same_cell(q, &spirit(3.0, 2.0)))
            .expect("entrant present");
        let leaving = mid
            .quads
            .iter()
            .find(|q| same_cell(q, &spirit(1.0, 1.0)))
            .expect("leaver present");
        assert!((entering.color.a - 0.5).abs() < 0.01, "entrant fades in");
        assert!((leaving.color.a - 0.5).abs() < 0.01, "leaver fades out");
        let end = Scene::interpolate(&prev, &next, 1.0);
        assert_eq!(end.quads.len(), 1, "at t=1 the leaver is gone");
        assert!(
            (end.quads[0].color.a - 1.0).abs() < 0.01
                && same_cell(&end.quads[0], &spirit(3.0, 2.0))
        );
    }
}

// Palette — "paper & ink, a fading Memory" (the canonical brand colours; the website
// mirrors these as CSS custom properties — see docs/decisions/brand_and_accessibility.md).
// Anchored to the brand tokens, but warmed + given RANGE: the board reads as aged letterpress
// stock (a warm cream, not a flat grey-beige), the ink is crisper/darker, and the seat inks +
// gild carry real accent weight (the visual-polish brief). Contrast still meets WCAG AA (the
// `palette_meets_wcag_aa_contrast` test gates it).
/// The page / board ground — a warm aged-paper cream (richer + a touch more saturated than the
/// old flat off-white, so the page reads as tactile printed stock).
const PAPER: Color = Color::rgba(0.949, 0.918, 0.847, 1.0);
/// The board ground's deeper, warmer foot — the bottom of the board's subtle vertical wash, so
/// the page reads as gently lit stock with a single light from above (never a flat slab).
const PAPER_FOOT: Color = Color::rgba(0.902, 0.859, 0.769, 1.0);
/// Ink text + the Dusk/Nightfall overlay — a crisp, slightly cool near-black (darker than the old
/// ink so type reads sharp and high-contrast on the warm page).
const NIGHT: Color = Color::rgba(0.067, 0.063, 0.094, 0.94);
/// Lorekeepers (Seat A): cool ink-blue (the brand `#2E5C9E`).
const SEAT_A: Color = Color::rgba(0.18, 0.36, 0.62, 1.0);
/// The Solace (Seat B): warm ink-red (the brand `#A83D33`).
const SEAT_B: Color = Color::rgba(0.66, 0.24, 0.20, 1.0);
/// Legal-target glow (a verdant "you may go here" green — neither seat's ink,
/// so a highlight never reads as a spirit; paired with a ring so it isn't hue-only).
const TARGET: Color = Color::rgba(0.20, 0.60, 0.34, 1.0);
/// Keyboard focus ring — a burnished-gold cursor (the canvas's `:focus-visible`), distinct
/// from both seat inks and the green target glow. A warm antique gold (brass-amber), not the old
/// bright lemon, so it reads as struck metal and matches the website/HUD gild.
const FOCUS: Color = Color::rgba(0.831, 0.612, 0.196, 1.0);
/// The **lamplit** pool of light under a *held* occupant on a faded
/// (Dusk-dark) tile (design §5's readability law). A warm, soft lantern glow that lifts
/// the held spirit out of the night so one glance answers "why is that spirit out there"
/// — visibly different from the live board AND the dark rim. Layered over [`Layer::Faded`]
/// (the night) and under the occupant, alpha-blended so the rim reads as night around it.
const LAMP: Color = Color::rgba(0.97, 0.82, 0.46, 1.0);
/// The **standing-Faded rescue glow** — a warm amber pool under a spirit **banished in
/// combat** that still stands in its §0.5 window (`combat_faded`), rescuable THIS turn by
/// evolve or devolve (design §5). It reads "held in the failing light, savable" — distinct
/// from a LIVE spirit (no glow) and from an unrecoverable Dusk fade (a dim grey pip, no
/// glow): the fading card stays *brighter* than a hopeless fade and gains this lamplit amber
/// halo, so a glance answers "this one can be saved before it dissolves." The amber matches
/// the shell's recede chevron + pip (`shell::DEVOLVE`), so the canvas speaks one rescue hue.
const RESCUE: Color = Color::rgba(0.85, 0.58, 0.22, 1.0);
/// The **board grid** ink — a soft sepia rule between cells (and a slightly stronger
/// outer frame), so the page reads as a ruled 5×5 lattice (`web_client_ux.md`
/// §In-canvas layout). Quiet enough not to fight the spirits, present enough that an
/// empty board is never a blank rectangle.
const GRID: Color = Color::rgba(0.55, 0.49, 0.39, 1.0);
/// A placed spirit's **card face** — fresh paper, a touch brighter than the board stock, so the
/// piece reads as a card laid on the page (item 3). Mirrors the shell hand card's `CARD`.
const CARD_FACE: Color = Color::rgba(0.984, 0.965, 0.925, 1.0);
/// Ink for the small stat numerals on a placed spirit's card foot (crisp on the paper face).
const CARD_INK: Color = Color::rgba(0.12, 0.12, 0.16, 1.0);
/// The **Atk** stat ink (a warm ember red) — labels the strike value on a placed card (and the
/// hand card / inspect mirror it). Distinct from the seat inks, in the warm family.
const ATK_INK: Color = Color::rgba(0.74, 0.30, 0.22, 1.0);
/// The **Def** stat ink (a steady slate blue) — the warding value.
const DEF_INK: Color = Color::rgba(0.24, 0.42, 0.66, 1.0);
/// The **HP** stat ink (a living green) — the held-life value.
const HP_INK: Color = Color::rgba(0.24, 0.50, 0.30, 1.0);

/// The two contesting inks. Seat A is cool, Seat B is warm — the same split the
/// projection washes and impressions read by.
pub fn seat_ink(seat: Seat) -> Color {
    match seat {
        Seat::A => SEAT_A,
        Seat::B => SEAT_B,
    }
}

fn with_alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

/// Linear interpolation between two opaque colours (`t` 0→1). Used for the board's gentle
/// top→foot warmth (one light from above).
fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// Alpha-composite `fg` (with its own alpha) over an opaque `bg`, returning an opaque colour —
/// so a faint seat tint over warmed paper keeps the page's warmth instead of greying it.
fn over(fg: Color, bg: Color) -> Color {
    let a = fg.a;
    Color {
        r: fg.r * a + bg.r * (1.0 - a),
        g: fg.g * a + bg.g * (1.0 - a),
        b: fg.b * a + bg.b * (1.0 - a),
        a: 1.0,
    }
}

/// A board-cell-sized label: the first four alphanumerics of a card's name,
/// uppercased — what fits legibly on a small tile (and on the shell's placed
/// board). Shared by the wgpu backend's id-indexed name table and the shell model
/// so the board reads identically standalone and in the shell.
pub fn short_board_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(4)
        .collect::<String>()
        .to_uppercase()
}

/// Build the scene for a 1v1 `PlayerView` (board derived as a square). `names`
/// maps card id → short label (indexed by id, as the catalog is); empty falls
/// back to the numeric id. No movement cues (the animation/blend path); use
/// [`build_player_scene_cued`] for the live frame.
pub fn build_player_scene(view: &PlayerView, names: &[String]) -> Scene {
    build_player_scene_cued(view, names, &MoveCues::default())
}

/// As [`build_player_scene`], plus the movement cues for the live frame: a green
/// pip on a Mobile spirit that can still step, a dim one on a rested / summoning-sick
/// Mobile spirit. The cues are tile-indexed and computed by the caller's engine.
pub fn build_player_scene_cued(view: &PlayerView, names: &[String], cues: &MoveCues) -> Scene {
    build_player_scene_interactive(view, names, cues, &Interaction::default())
}

/// As [`build_player_scene_cued`], plus the input overlays (legal-target
/// highlights, the picked-up spirit's ring, and the keyboard focus ring) for the
/// live frame. The overlays are derived in JS from the engine's legal moves.
pub fn build_player_scene_interactive(
    view: &PlayerView,
    names: &[String],
    cues: &MoveCues,
    inter: &Interaction,
) -> Scene {
    let board_w = (view.tiles.len() as f64).sqrt().round() as u32;
    tiles_scene(&view.tiles, board_w, board_w, view.seat, names, cues, inter)
}

/// Build the scene for a 2v2 `TeamView` (board width is carried explicitly).
pub fn build_team_scene(view: &TeamView, names: &[String]) -> Scene {
    build_team_scene_cued(view, names, &MoveCues::default())
}

/// As [`build_team_scene`], plus the movement cues for the live frame.
pub fn build_team_scene_cued(view: &TeamView, names: &[String], cues: &MoveCues) -> Scene {
    build_team_scene_interactive(view, names, cues, &Interaction::default())
}

/// As [`build_team_scene_cued`], plus the input overlays for the live 2v2 frame.
pub fn build_team_scene_interactive(
    view: &TeamView,
    names: &[String],
    cues: &MoveCues,
    inter: &Interaction,
) -> Scene {
    let board_w = view.board_w.max(1) as u32;
    let board_h = (view.tiles.len() as u32) / board_w;
    tiles_scene(&view.tiles, board_w, board_h, view.team, names, cues, inter)
}

/// A screen-reader description of a 1v1 board: the round + whose turn, then each
/// occupied tile (cell, occupant, strength), terrain, impressions, the Dusk, and a
/// count of empties. Pure + native-tested; the wasm shell drops it into a
/// visually-hidden region so the wgpu canvas isn't a black box to screen-reader users.
pub fn board_description(view: &PlayerView, names: &[String]) -> String {
    let board_w = ((view.tiles.len() as f64).sqrt().round() as u32).max(1);
    let turn = if view.active == view.seat {
        "your turn"
    } else {
        "opponent's turn"
    };
    format!(
        "Round {}, {}. {}",
        view.round,
        turn,
        describe_board(&view.tiles, board_w, view.seat, names)
    )
}

/// The 2v2 counterpart, from one slot's `TeamView`.
pub fn board_description_team(view: &TeamView, names: &[String]) -> String {
    let turn = if view.active_slot.team() == view.team {
        "your team to act"
    } else {
        "opponents to act"
    };
    format!(
        "Round {}, {}, slot {:?}. {}",
        view.round,
        turn,
        view.active_slot,
        describe_board(&view.tiles, (view.board_w.max(1)) as u32, view.team, names)
    )
}

/// A per-tile screen-reader reading for a 1v1 board, indexed by tile — the same
/// phrasing `describe_board` joins, but kept per-tile so the a11y
/// tree can label each actionable tile-button identically to the text mirror (one
/// source for the wording). An empty tile reads "empty". Pure + native-tested.
pub fn tile_readings(view: &PlayerView, names: &[String]) -> Vec<String> {
    view.tiles
        .iter()
        .map(|tile| describe_tile(tile, view.seat, names))
        .collect()
}

/// One tile's reading (occupant + strength / terrain / impression / "empty"),
/// owner-relative to `you`. The cell coordinate is prepended by the caller.
fn describe_tile(tile: &TileView, you: Seat, names: &[String]) -> String {
    let owner_word = |seat: Seat| if seat == you { "your" } else { "opponent's" };
    if let Some(sp) = &tile.spirit {
        if sp.face_down {
            return format!("a face-down {} spirit", owner_word(sp.owner));
        }
        let name = names
            .get(sp.card.0 as usize)
            .cloned()
            .unwrap_or_else(|| sp.card.0.to_string());
        // A standing-Faded form (banished in combat, in its §0.5 window) reads as rescuable
        // — distinct from an ordinary, unrecoverable fade — so the board mirror narrates the
        // window ("standing Faded — rescuable this turn") that the canvas glow shows.
        let fading = if sp.combat_faded {
            ", standing Faded — rescuable this turn"
        } else if sp.fading {
            ", fading"
        } else {
            ""
        };
        // An Echo carries a gold pip on the canvas — the spirit returns once if banished. A
        // screen reader can't see the pip, so the reading names it (the canvas glyph → words).
        let echo = if sp.echo {
            ", an Echo (it returns once if banished)"
        } else {
            ""
        };
        // "Held": an occupant standing on a Dusk-darkened tile is held in the lamplight
        // (the canvas's `Lamp` layer) — the only thing keeping that tile lit. The board
        // mirror narrates the held state so the lamplit cue isn't sight-only.
        let held = if tile.faded {
            ", held in the lamplight"
        } else {
            ""
        };
        return format!(
            "{} {name}, attack {}, defense {}, {} of {} health{echo}{fading}{held}",
            owner_word(sp.owner),
            sp.attack,
            sp.defense,
            sp.hp,
            sp.hp_max
        );
    }
    if let Some(terr) = &tile.terrain {
        return if terr.face_down {
            "hidden terrain".to_string()
        } else {
            format!("{} {}", owner_word(terr.owner), terr.kind)
        };
    }
    if let Some(imp) = tile.impression {
        return format!("{} impression", owner_word(imp));
    }
    "empty".to_string()
}

fn describe_board(tiles: &[TileView], board_w: u32, you: Seat, names: &[String]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut empty = 0u32;
    let mut faded = 0u32;
    for (i, tile) in tiles.iter().enumerate() {
        let col = (b'A' + (i as u32 % board_w) as u8) as char;
        let row = i as u32 / board_w + 1;
        let cell = format!("{col}{row}");
        if tile.faded {
            faded += 1;
        }
        let occupied = tile.spirit.is_some() || tile.terrain.is_some() || tile.impression.is_some();
        if occupied {
            // Reuse the per-tile reading so the joined description and the a11y tree
            // word a tile identically (one source).
            parts.push(format!("{cell}: {}", describe_tile(tile, you, names)));
        } else {
            empty += 1;
        }
    }
    let mut s = if parts.is_empty() {
        "The board is empty.".to_string()
    } else {
        format!("{}.", parts.join(". "))
    };
    s.push_str(&format!(" {empty} empty tiles."));
    if faded > 0 {
        s.push_str(&format!(" {faded} tiles have faded into the Dusk."));
    }
    s
}

#[cfg(test)]
mod aria_tests {
    use super::*;
    use recollect_core::types::CardId;
    use recollect_core::view::SpiritView;

    fn empty_tile() -> TileView {
        TileView {
            spirit: None,
            impression: None,
            faded: false,
            in_your_projection: false,
            terrain: None,
        }
    }
    fn spirit_tile(card: u16, owner: Seat) -> TileView {
        TileView {
            spirit: Some(SpiritView {
                card: CardId(card),
                owner,
                attack: 2,
                defense: 1,
                hp: 3,
                hp_max: 3,
                fading: false,
                combat_faded: false,
                echo: false,
                mobile: false,
                face_down: false,
                evolutions: vec![],
            }),
            ..empty_tile()
        }
    }
    #[test]
    fn describes_occupants_owner_perspective_and_empties() {
        let names: Vec<String> = vec!["Zero".into(), "One".into(), "Ember".into()];
        // 2×2 board: A1 = card 2 owned by A; the rest empty.
        let tiles = vec![
            spirit_tile(2, Seat::A),
            empty_tile(),
            empty_tile(),
            empty_tile(),
        ];
        let from_a = describe_board(&tiles, 2, Seat::A, &names);
        assert!(
            from_a.contains("A1: your Ember, attack 2, defense 1, 3 of 3 health"),
            "got: {from_a}"
        );
        assert!(from_a.contains("3 empty tiles"), "got: {from_a}");
        // The same spirit reads as the opponent's from seat B.
        let from_b = describe_board(&tiles, 2, Seat::B, &names);
        assert!(from_b.contains("A1: opponent's Ember"), "got: {from_b}");
    }

    #[test]
    fn the_reading_names_echo_and_the_held_lamplight_in_words() {
        // The canvas marks an Echo with a gold pip and a held occupant with a lamplit pool;
        // a screen reader can't see either, so the per-tile reading must say them in WORDS
        // (the glyph → words bar). An Echo spirit reads as an Echo; a spirit standing on a
        // Dusk-darkened (faded) tile reads as held in the lamplight.
        let names: Vec<String> = vec!["Zero".into(), "One".into(), "Ember".into()];
        let mut echoer = spirit_tile(2, Seat::A);
        echoer.spirit.as_mut().unwrap().echo = true;
        let reading = describe_tile(&echoer, Seat::A, &names);
        assert!(
            reading.contains("an Echo (it returns once if banished)"),
            "the Echo is named in words, got: {reading}"
        );
        assert!(
            !reading.contains("held in the lamplight"),
            "a live (un-faded) tile is not held, got: {reading}"
        );
        // A spirit on a faded tile is held in the lamplight (the only thing keeping it lit).
        let mut held = spirit_tile(2, Seat::A);
        held.faded = true;
        let held_reading = describe_tile(&held, Seat::A, &names);
        assert!(
            held_reading.contains("held in the lamplight"),
            "the held lamplight is named in words, got: {held_reading}"
        );
        // The reading is glyph-free (no pip / lamp symbol leaks into the spoken text).
        for glyph in ['★', '●', '◦', '°', '·', '→', '⚔'] {
            assert!(
                !held_reading.contains(glyph) && !reading.contains(glyph),
                "the spoken reading must be all words, not {glyph:?}"
            );
        }
    }
}

/// Tile-grid position of tile `i` (column gx, Y-flipped row gy so seat A's home
/// row draws at the bottom — the same flip the click mapping in index.html uses).
fn tile_xy(i: usize, board_w: u32, board_h: u32) -> (f32, f32) {
    let gx = (i as u32 % board_w) as f32;
    let gy = (board_h - 1 - (i as u32 / board_w)) as f32;
    (gx, gy)
}

fn tiles_scene(
    tiles: &[TileView],
    board_w: u32,
    board_h: u32,
    you: Seat,
    names: &[String],
    cues: &MoveCues,
    inter: &Interaction,
) -> Scene {
    let mut scene = Scene {
        board_w,
        board_h,
        ..Default::default()
    };
    for (i, tile) in tiles.iter().enumerate() {
        let (gx, gy) = tile_xy(i, board_w, board_h);
        let move_cue = if cues.movable.contains(&(i as u8)) {
            Some(true)
        } else if cues.sick.contains(&(i as u8)) {
            Some(false)
        } else {
            None
        };
        // A gentle top→foot warmth across the board (0 at the top row, 1 at the bottom),
        // so the page reads as printed stock lit from above — not a flat rectangle.
        let depth = if board_h > 1 {
            gy / (board_h - 1).max(1) as f32
        } else {
            0.0
        };
        push_tile(&mut scene, gx, gy, tile, you, names, move_cue, depth);
    }
    // The ruled lattice — drawn after the washes (so it overlays them) but on the low
    // Grid layer (so any occupant/overlay still sits above it). This is what turns the
    // board from a blank rectangle into a visible 5×5 (6×6) page of tiles.
    push_grid(&mut scene, board_w, board_h);
    // Input overlays, drawn over the tiles: legal-target glows, the picked-up
    // spirit's ring, then the keyboard focus ring (highest, so it reads on top of
    // a selected/highlighted tile too).
    for &t in &inter.legal {
        if (t as usize) < tiles.len() {
            let (gx, gy) = tile_xy(t as usize, board_w, board_h);
            push_target_glow(&mut scene, gx, gy);
        }
    }
    if let Some(t) = inter.selected
        && (t as usize) < tiles.len()
    {
        let (gx, gy) = tile_xy(t as usize, board_w, board_h);
        push_ring(&mut scene, gx, gy, seat_ink(you), 0.10);
    }
    if let Some(t) = inter.focus
        && (t as usize) < tiles.len()
    {
        let (gx, gy) = tile_xy(t as usize, board_w, board_h);
        // A bright, neutral focus ring (the canvas counterpart of :focus-visible).
        push_ring(&mut scene, gx, gy, FOCUS, 0.04);
    }
    scene
}

/// The ruled board lattice: the interior cell-separator lines plus a slightly
/// stronger outer frame, in tile-grid units (the board spans `[0,board_w]×[0,board_h]`).
/// Thin sepia rules on the [`Layer::Grid`] layer, so the page reads as a 5×5 (6×6)
/// grid of tiles rather than one flat rectangle (`web_client_ux.md` §In-canvas layout).
fn push_grid(scene: &mut Scene, board_w: u32, board_h: u32) {
    let w = board_w.max(1) as f32;
    let h = board_h.max(1) as f32;
    // Quiet hairline rules: thin + low-alpha so the lattice reads as a faint ruling on the
    // page (it organises the eye without drawing a hard cage). A slightly stronger but
    // still soft frame gives the page a clean edge.
    let t = 0.018; // interior rule thickness, tile-grid units
    let line = with_alpha(GRID, 0.20);
    let frame = with_alpha(GRID, 0.40);
    let push = |scene: &mut Scene, x: f32, y: f32, qw: f32, qh: f32, color: Color| {
        scene.quads.push(Quad {
            x,
            y,
            w: qw,
            h: qh,
            color,
            layer: Layer::Grid,
        });
    };
    // Interior verticals (skip the outer edges — the frame covers them).
    for c in 1..board_w {
        push(scene, c as f32 - t / 2.0, 0.0, t, h, line);
    }
    // Interior horizontals.
    for r in 1..board_h {
        push(scene, 0.0, r as f32 - t / 2.0, w, t, line);
    }
    // The outer frame — a soft, thin edge (not a heavy border).
    let ft = 0.03;
    push(scene, 0.0, 0.0, w, ft, frame); // top
    push(scene, 0.0, h - ft, w, ft, frame); // bottom
    push(scene, 0.0, 0.0, ft, h, frame); // left
    push(scene, w - ft, 0.0, ft, h, frame); // right
}

/// A soft legal-target wash + a thin border ring, so a legal destination glows
/// (color-and-shape, not hue alone — see brand_and_accessibility.md). Drawn on the
/// Marker layer so it sits over the tile's spirit/terrain.
fn push_target_glow(scene: &mut Scene, gx: f32, gy: f32) {
    scene
        .quads
        .push(cell(gx, gy, 0.0, with_alpha(TARGET, 0.22), Layer::Marker));
    push_ring(scene, gx, gy, TARGET, 0.06);
}

/// A hollow ring (four edge quads) inset by `inset`, in `color`. A ring rather than
/// a fill so the cell's occupant stays legible inside it.
fn push_ring(scene: &mut Scene, gx: f32, gy: f32, color: Color, inset: f32) {
    let t = 0.06; // ring thickness, tile-grid units
    let x0 = gx + inset;
    let y0 = gy + inset;
    let side = 1.0 - 2.0 * inset;
    let edge = |x: f32, y: f32, w: f32, h: f32| Quad {
        x,
        y,
        w,
        h,
        color,
        layer: Layer::Marker,
    };
    scene.quads.push(edge(x0, y0, side, t)); // top
    scene.quads.push(edge(x0, y0 + side - t, side, t)); // bottom
    scene.quads.push(edge(x0, y0, t, side)); // left
    scene.quads.push(edge(x0 + side - t, y0, t, side)); // right
}

fn cell(gx: f32, gy: f32, inset: f32, color: Color, layer: Layer) -> Quad {
    Quad {
        x: gx + inset,
        y: gy + inset,
        w: 1.0 - 2.0 * inset,
        h: 1.0 - 2.0 * inset,
        color,
        layer,
    }
}

/// `move_cue`: `Some(true)` ⇒ a Mobile spirit that can still step this turn (green
/// corner pip); `Some(false)` ⇒ Mobile but rested / summoning-sick (dim pip); `None`
/// ⇒ no movement marker (not the active seat's, not Mobile, or fading). `depth` (0 at the
/// top row → 1 at the bottom) warms the paper ground toward its foot, so the board reads as
/// stock lit from above.
#[allow(clippy::too_many_arguments)]
fn push_tile(
    scene: &mut Scene,
    gx: f32,
    gy: f32,
    tile: &TileView,
    you: Seat,
    names: &[String],
    move_cue: Option<bool>,
    depth: f32,
) {
    // The paper ground, warmed toward the foot of the board (a gentle top→bottom wash — one
    // light from above), so an empty page reads as tactile printed stock, not a flat slab.
    let paper = lerp_color(PAPER, PAPER_FOOT, depth);
    // The projection wash: your reach, theirs, both, or bare paper. The view only
    // carries YOUR projection (`in_your_projection`); the opponent's overlay is
    // server-pushed elsewhere, so here a tile is "yours" or "paper". A VERY light seat tint
    // — a barely-there hint of "your ground" against the ruled grid, never a grey slab (the
    // earlier heavier wash read as an artifact mid-board).
    let wash = if tile.in_your_projection {
        // Composite the faint seat tint OVER the warmed paper, so a projected tile keeps the
        // page's warmth (not a cool grey wash that read as an artifact before).
        over(with_alpha(seat_ink(you), 0.06), paper)
    } else {
        paper
    };
    scene.quads.push(cell(gx, gy, 0.0, wash, Layer::Wash));

    if tile.faded {
        scene.quads.push(cell(gx, gy, 0.0, NIGHT, Layer::Faded));
        // A **held** tile renders lamplit: a soft pool of light over the
        // night when something stands (or is impressed/terraformed) here, so the held
        // occupant reads visibly different from both the live board and the dark rim
        // (design §5). An empty faded tile stays pure night; the light is the occupant's.
        // (The Solace's Unwritten leave no board mark, so they are never held here — the
        // Dusk simply sweeps them; an empty tile shows no lamp.)
        let held = tile.spirit.is_some() || tile.terrain.is_some() || tile.impression.is_some();
        if held {
            // Two stacked, inset pools — a wider faint halo + a brighter core — so the
            // light reads as a lantern glow that falls off toward the cell edge (which
            // stays night), not a flat fill. Soft alphas keep the night legible around it.
            scene
                .quads
                .push(cell(gx, gy, 0.06, with_alpha(LAMP, 0.30), Layer::Lamp));
            scene
                .quads
                .push(cell(gx, gy, 0.18, with_alpha(LAMP, 0.34), Layer::Lamp));
        }
    }

    if let Some(impression) = tile.impression {
        scene.quads.push(cell(
            gx,
            gy,
            0.06,
            with_alpha(seat_ink(impression), 0.34),
            Layer::Impression,
        ));
    }

    // Terrain — invisible before this layer existed. A Landmark shows openly; a
    // face-down Fabrication shows as a veiled lie (neutral hatch); a revealed one
    // shows in its owner's ink.
    if let Some(terr) = &tile.terrain {
        let (color, glyph) = match (terr.kind.as_str(), terr.face_down) {
            ("Landmark", _) => (with_alpha(seat_ink(terr.owner), 0.85), "Lm"),
            ("Fabrication", true) => (Color::rgba(0.45, 0.45, 0.48, 0.8), "?"),
            ("Fabrication", false) => (with_alpha(seat_ink(terr.owner), 0.7), "Fb"),
            _ => (Color::rgba(0.5, 0.5, 0.5, 0.7), "?"),
        };
        scene.quads.push(cell(gx, gy, 0.22, color, Layer::Terrain));
        scene.labels.push(Label {
            x: gx + 0.5,
            y: gy + 0.5,
            text: glyph.into(),
            color: PAPER,
            size: Label::DEFAULT_SIZE,
        });
    }

    if let Some(sp) = &tile.spirit {
        // A placed spirit gets the SAME card treatment as a hand card, compacted to
        // the tile: a paper card face inset in the cell, a seat-tinted title band carrying the
        // short name, and a compact Atk / Def / HP foot — so a piece reads as the same object it
        // was in hand, not a flat coloured square. The seat ink is the FRAME (band + ring), the
        // identity colour; the body is paper, like the catalog card.
        let ink = if sp.face_down {
            Color::rgba(0.40, 0.40, 0.43, 1.0) // a hidden lurker: identity withheld
        } else {
            seat_ink(sp.owner)
        };
        // A standing-Faded form (banished in combat, in its §0.5 window) is RESCUABLE this
        // turn — so it reads brighter (it is still here, savable) and gains a warm amber
        // lamplit glow pool, visibly distinct from a LIVE spirit (no glow) and from an
        // unrecoverable Dusk fade (dimmer, grey pip, no glow). An ordinary `fading` base
        // with no rescue window dims further (it is leaving the page).
        let fade = if sp.combat_faded {
            0.78 // held in the failing light — savable, so still vivid
        } else if sp.fading {
            0.60 // an unrecoverable fade — dimming out
        } else {
            1.0
        };
        // The rescue glow: a soft amber pool UNDER the card (the Lamp layer, like the Dusk's
        // held-light), so the rescuable spirit is lifted out of the fade. Two stacked discs
        // (a broad soft halo + a brighter inner pool), the same warm-light grammar the Dusk
        // lamplit tile uses — but in the recede amber, the "this can be saved" hue.
        if sp.combat_faded {
            scene
                .quads
                .push(cell(gx, gy, 0.16, with_alpha(RESCUE, 0.26), Layer::Lamp));
            scene
                .quads
                .push(cell(gx, gy, 0.30, with_alpha(RESCUE, 0.30), Layer::Lamp));
        }
        push_spirit_card(scene, gx, gy, sp, ink, fade, names);

        // Corner pips: Echo (burnished gold), can-still-evolve (green — the Primal rescue),
        // a standing-Faded form rescuable by recede (amber — devolution), a plain unrecoverable
        // fade (dim grey). The amber pip pairs the rescue glow + the shell's recede chevron, so
        // "this one can be saved (by receding)" reads at a glance and in one hue.
        if sp.echo {
            scene
                .quads
                .push(pip(gx, gy, Color::rgba(0.831, 0.612, 0.196, 1.0)));
        } else if sp.fading && !sp.evolutions.is_empty() {
            scene
                .quads
                .push(pip(gx, gy, Color::rgba(0.30, 0.70, 0.35, 1.0)));
        } else if sp.combat_faded {
            scene.quads.push(pip(gx, gy, RESCUE));
        } else if sp.fading {
            scene
                .quads
                .push(pip(gx, gy, Color::rgba(0.5, 0.5, 0.5, 1.0)));
        }

        // Movement cue (bottom-left, so it never collides with the top-left
        // status pip or the top-right affordance dot): a bright green dot means this Mobile spirit can still take its
        // one step this turn; a dim dot means it is rested — it already moved or just
        // arrived (summoning-sick) and cannot step again until next turn.
        match move_cue {
            Some(true) => scene
                .quads
                .push(move_pip(gx, gy, Color::rgba(0.30, 0.70, 0.35, 1.0))),
            Some(false) => scene
                .quads
                .push(move_pip(gx, gy, Color::rgba(0.45, 0.45, 0.48, 0.85))),
            None => {}
        }
    }
}

/// Draw a placed spirit as a **compact card face** in its tile (item 3): a paper card body
/// inset in the cell, a seat-tinted title band with the short name, an emblem initial, and a
/// compact Atk / Def / HP foot — the same anatomy as the hand card, scaled to the tile. `ink`
/// is the seat (or hidden-grey) identity colour for the band + frame; `fade` dims a Fading
/// spirit's whole card. A face-down spirit shows just the tinted back (identity withheld).
fn push_spirit_card(
    scene: &mut Scene,
    gx: f32,
    gy: f32,
    sp: &recollect_core::view::SpiritView,
    ink: Color,
    fade: f32,
    names: &[String],
) {
    let inset = 0.08;
    let x0 = gx + inset;
    let y0 = gy + inset;
    let side = 1.0 - 2.0 * inset;
    // The seat-ink frame plate (reads as the card's coloured edge / identity), then the paper
    // face inset within it — so the seat colour rings the card and the body is fresh paper.
    scene.quads.push(Quad {
        x: x0,
        y: y0,
        w: side,
        h: side,
        color: with_alpha(ink, fade),
        layer: Layer::Spirit,
    });
    if sp.face_down {
        // A hidden lurker: just the tinted back + a faint "memory" diamond, no stats/name.
        let d = side * 0.30;
        scene.quads.push(Quad {
            x: gx + 0.5 - d / 2.0,
            y: gy + 0.5 - d / 2.0,
            w: d,
            h: d,
            color: with_alpha(PAPER, 0.55 * fade),
            layer: Layer::Spirit,
        });
        return;
    }
    let fr = 0.045; // frame thickness (seat ink shows around the paper face)
    let fx = x0 + fr;
    let fy = y0 + fr;
    let fw = side - 2.0 * fr;
    // The paper card body.
    scene.quads.push(Quad {
        x: fx,
        y: fy,
        w: fw,
        h: fw,
        color: with_alpha(CARD_FACE, fade),
        layer: Layer::Spirit,
    });
    // The title band (seat ink) across the top of the face, carrying the short name in paper.
    let band_h = fw * 0.30;
    scene.quads.push(Quad {
        x: fx,
        y: fy,
        w: fw,
        h: band_h,
        color: with_alpha(ink, fade),
        layer: Layer::Spirit,
    });
    let name = names
        .get(sp.card.0 as usize)
        .cloned()
        .unwrap_or_else(|| sp.card.0.to_string());
    scene.labels.push(Label {
        x: gx + 0.5,
        y: fy + band_h * 0.52,
        text: name,
        color: with_alpha(PAPER, fade),
        size: band_h * 0.62,
    });
    // The card face splits into the title band (top), an ART PLATE (middle), and a clear STAT
    // FOOT (bottom) — the same three-part anatomy as the hand card. Reserving an explicit foot
    // band keeps the Atk/Def/HP numerals clear of the emblem (they overlapped the art plate
    // before): the emblem is centred in the plate ABOVE the foot, never reaching down into it.
    let body_top = fy + band_h;
    let body_h = fw - band_h;
    let foot_h = body_h * 0.30; // the reserved stat lane at the card's foot
    let plate_top = body_top;
    let plate_h = body_h - foot_h;
    // A soft emblem initial centred in the ART PLATE (above the foot) — the card's "art plate" at
    // tile scale: a faint seat-tinted disc with the spirit's leading glyph. Sized to the plate so
    // it never bleeds into the foot lane.
    let body_cy = plate_top + plate_h * 0.5;
    let em_r = (plate_h * 0.42).min(fw * 0.20);
    scene.quads.push(Quad {
        x: gx + 0.5 - em_r,
        y: body_cy - em_r,
        w: em_r * 2.0,
        h: em_r * 2.0,
        color: with_alpha(ink, 0.16 * fade),
        layer: Layer::Spirit,
    });
    // The compact Atk / Def / HP foot — three small ink numerals in their stat colours, centred in
    // the reserved foot lane, so the placed card reads its strength the same way the hand card does
    // (never a bare HP number) and the numbers never overlap the emblem above.
    let foot_y = plate_top + plate_h + foot_h * 0.5;
    let stat_size = (foot_h * 0.62).min(fw * 0.18);
    let third = fw / 3.0;
    for (k, (val, col)) in [(sp.attack, ATK_INK), (sp.defense, DEF_INK), (sp.hp, HP_INK)]
        .iter()
        .enumerate()
    {
        scene.labels.push(Label {
            x: fx + third * (k as f32 + 0.5),
            y: foot_y,
            text: format!("{val}"),
            color: with_alpha(*col, fade),
            size: stat_size,
        });
    }
    let _ = CARD_INK;
}

/// A small status marker in the **top-left** corner of a cell (Echo gold / fading green-or-grey).
/// Top-left keeps it clear of the **affordance** dot, which the shell overlay draws in the
/// top-right of every card consistently (hand + board) — see `shell::board_affordances`.
fn pip(gx: f32, gy: f32, color: Color) -> Quad {
    Quad {
        x: gx + 0.08,
        y: gy + 0.08,
        w: 0.18,
        h: 0.18,
        color,
        layer: Layer::Marker,
    }
}

/// The movement-cue marker, in the bottom-left corner of a cell (kept clear of
/// the top-left status [`pip`] and the top-right affordance dot).
fn move_pip(gx: f32, gy: f32, color: Color) -> Quad {
    Quad {
        x: gx + 0.08,
        y: gy + 0.74,
        w: 0.18,
        h: 0.18,
        color,
        layer: Layer::Marker,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recollect_core::Engine;
    use recollect_core::cards::canon_catalog;
    use recollect_core::state::{Terrain, TerrainKind};

    /// The brand palette must meet WCAG 2.1 AA contrast (a11y — see
    /// docs/decisions/brand_and_accessibility.md). Body text (ink on the page) needs
    /// 4.5:1; large / UI elements (seat-ink spirit fills + borders) need 3:1. Computed
    /// on opaque RGB — NIGHT's slight alpha only raises the real on-paper ratio.
    #[test]
    fn palette_meets_wcag_aa_contrast() {
        fn channel(u: f32) -> f32 {
            if u <= 0.03928 {
                u / 12.92
            } else {
                ((u + 0.055) / 1.055).powf(2.4)
            }
        }
        fn luminance(c: Color) -> f32 {
            0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
        }
        fn ratio(a: Color, b: Color) -> f32 {
            let (la, lb) = (luminance(a), luminance(b));
            let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
            (hi + 0.05) / (lo + 0.05)
        }
        assert!(
            ratio(NIGHT, PAPER) >= 4.5,
            "ink-on-paper body text must be AA 4.5:1, got {:.2}",
            ratio(NIGHT, PAPER)
        );
        for (seat, ink) in [("A", SEAT_A), ("B", SEAT_B)] {
            assert!(
                ratio(ink, PAPER) >= 3.0,
                "seat {seat} ink on paper must be AA 3:1 (UI), got {:.2}",
                ratio(ink, PAPER)
            );
        }
    }
    use recollect_core::test_support::put_spirit;
    use recollect_core::types::CardId;
    use recollect_core::view::view_for;

    fn engine() -> Engine {
        let cat = canon_catalog();
        let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
        Engine::new(7, cat.clone(), deck.clone(), deck).0
    }

    fn id_of(name: &str) -> CardId {
        canon_catalog()
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.id)
            .unwrap()
    }

    #[test]
    fn scene_has_a_wash_for_every_tile_and_the_right_board_size() {
        let e = engine();
        let scene = build_player_scene(&view_for(&e, Seat::A), &[]);
        assert_eq!(scene.board_w, 5);
        let washes = scene
            .quads
            .iter()
            .filter(|q| q.layer == Layer::Wash)
            .count();
        assert_eq!(washes, 25, "one wash per tile");
    }

    #[test]
    fn the_board_draws_a_visible_ruled_grid() {
        // web_client_ux.md §In-canvas layout: the board is a DISTINCT 5×5 lattice, not a
        // blank rectangle. A 5×5 board has 4 interior verticals + 4 interior horizontals
        // + 4 frame edges = 12 Grid-layer rules; each is a thin sepia line.
        let e = engine();
        let scene = build_player_scene(&view_for(&e, Seat::A), &[]);
        let grid: Vec<&Quad> = scene
            .quads
            .iter()
            .filter(|q| q.layer == Layer::Grid)
            .collect();
        assert_eq!(
            grid.len(),
            4 + 4 + 4,
            "4 interior verticals + 4 interior horizontals + 4 frame edges"
        );
        // Each rule is thin (a line, not a fill) and sepia (the grid ink, not a seat).
        for q in &grid {
            assert!(q.w.min(q.h) < 0.1, "a grid rule is thin: {q:?}");
            assert!(
                q.color.r > q.color.b && q.color.g > q.color.b,
                "the grid is warm sepia, not a cool seat ink: {:?}",
                q.color
            );
        }
        // The grid sits UNDER any occupant (Grid < Spirit), so a piece reads on top of it.
        assert!((Layer::Grid as u8) < (Layer::Spirit as u8));
    }

    #[test]
    fn a_2v2_team_view_renders_a_six_by_six_board() {
        use recollect_core::types::{CardKind, SeatSlot};
        use recollect_core::view::view_for_slot;
        let cat = canon_catalog();
        let deck: Vec<CardId> = cat
            .iter()
            .filter(|c| c.kind == CardKind::Spirit)
            .take(20)
            .map(|c| c.id)
            .collect();
        let decks = [deck.clone(), deck.clone(), deck.clone(), deck];
        let (e, _) = Engine::new_2v2(7, cat, decks);
        let scene = build_team_scene(&view_for_slot(&e, SeatSlot::A1), &[]);
        assert_eq!(scene.board_w, 6);
        assert_eq!(scene.board_h, 6);
        let washes = scene
            .quads
            .iter()
            .filter(|q| q.layer == Layer::Wash)
            .count();
        assert_eq!(washes, 36, "one wash per tile on the 6×6 board");
    }

    #[test]
    fn terrain_becomes_a_drawn_primitive() {
        // Terrain is a drawn primitive: a Landmark and a Fabrication each yield
        // a Terrain quad + a glyph label.
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            st.board[12].terrain = Some(Terrain {
                card: id_of("Cloudling"),
                owner: Seat::A,
                kind: TerrainKind::Landmark,
                face_down: false,
            });
            st.board[7].terrain = Some(Terrain {
                card: id_of("Cloudling"),
                owner: Seat::B,
                kind: TerrainKind::Fabrication,
                face_down: true,
            });
        }
        let scene = build_player_scene(&view_for(&e, Seat::A), &[]);
        let terrain_quads = scene
            .quads
            .iter()
            .filter(|q| q.layer == Layer::Terrain)
            .count();
        assert_eq!(terrain_quads, 2, "a Landmark and a Fabrication both drawn");
        assert!(
            scene.labels.iter().any(|l| l.text == "Lm"),
            "Landmark glyph"
        );
        assert!(
            scene.labels.iter().any(|l| l.text == "?"),
            "the enemy face-down Fabrication shows as a veiled lie"
        );
    }

    #[test]
    fn a_held_spirit_on_a_faded_tile_renders_lamplit() {
        // The Dusk's readability law: an EMPTY faded rim tile is pure
        // night (no lamp); a faded tile that still HOLDS a spirit renders lamplit — a
        // warm pool of light over the night, so one glance answers "why is that spirit
        // out there." We assert: no Lamp quad on an empty faded tile, a Lamp quad once
        // a spirit stands there, and the lamp is warm (a lantern glow, not a cool wash).
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            st.board[12].faded = true; // a faded, EMPTY tile
        }
        let empty_dark = build_player_scene(&view_for(&e, Seat::A), &[]);
        assert_eq!(
            empty_dark
                .quads
                .iter()
                .filter(|q| q.layer == Layer::Lamp)
                .count(),
            0,
            "an empty faded tile shows no lamp — pure night"
        );
        // Now a spirit holds that faded tile (the Held Ground law).
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 12, CardId(0), Seat::A);
            st.board[12].faded = true;
        }
        let held = build_player_scene(&view_for(&e, Seat::A), &[]);
        let lamps: Vec<&Quad> = held
            .quads
            .iter()
            .filter(|q| q.layer == Layer::Lamp)
            .collect();
        assert!(
            !lamps.is_empty(),
            "a held spirit on a faded tile is lamplit"
        );
        // The lamp is a WARM pool of light (red+green high, blue lower) — a lantern, not
        // a cool target glow; and it sits UNDER the spirit (the Lamp layer < Spirit).
        for q in &lamps {
            assert!(
                q.color.r > 0.6 && q.color.g > 0.5 && q.color.b < q.color.r,
                "the lamp is a warm glow: {:?}",
                q.color
            );
        }
        assert!(
            (Layer::Lamp as u8) < (Layer::Spirit as u8),
            "the lamp pools under the held spirit"
        );
        // The night layer is still drawn (the rim is dark; the lamp is a pool atop it).
        assert!(
            held.quads.iter().any(|q| q.layer == Layer::Faded),
            "the faded night still renders under the lamp"
        );
    }

    #[test]
    fn a_fading_evolvable_base_gets_the_evolution_pip() {
        // A Fading base that can still evolve gets the green pip, not the dim one.
        let base = canon_catalog()
            .iter()
            .find(|c| !c.evolves_to.is_empty())
            .map(|c| c.id)
            .expect("some canon base evolves");
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 12, base, Seat::A);
            let sp = st.board[12].spirit.as_mut().unwrap();
            sp.fading = true;
        }
        let view = view_for(&e, Seat::A);
        let evolvable = view.tiles[12]
            .spirit
            .as_ref()
            .map(|s| !s.evolutions.is_empty())
            .unwrap_or(false);
        let scene = build_player_scene(&view, &[]);
        let pips = scene
            .quads
            .iter()
            .filter(|q| q.layer == Layer::Marker)
            .count();
        assert_eq!(pips, 1, "exactly one corner pip on the fading base");
        if evolvable {
            let pip = scene
                .quads
                .iter()
                .find(|q| q.layer == Layer::Marker)
                .unwrap();
            assert!(pip.color.g > pip.color.r, "evolvable ⇒ green pip");
        }
    }

    #[test]
    fn a_standing_faded_form_renders_distinctly_rescuable() {
        // A spirit BANISHED IN COMBAT lingers standing-Faded (rescuable this turn) — it must
        // render DISTINCTLY from a live spirit AND from an unrecoverable Dusk fade: a warm
        // amber rescue glow under it (Lamp layer), an amber corner pip, and a brighter card
        // (savable, not dimming out). The board mirror narrates the window too.
        // Pick a NON-evolving spirit so the green evolvable-pip branch never fires (isolating
        // the recede amber).
        let plain_id = canon_catalog()
            .iter()
            .find(|c| {
                matches!(c.kind, recollect_core::types::CardKind::Spirit) && c.evolves_to.is_empty()
            })
            .map(|c| c.id)
            .expect("some canon spirit doesn't evolve");

        // (a) An ORDINARY, unrecoverable fade (fading, NO fade_deadline) — the baseline.
        let mut e = engine();
        {
            let st = e.state_mut_for_test();
            put_spirit(st, 12, plain_id, Seat::A);
            let sp = st.board[12].spirit.as_mut().unwrap();
            sp.fading = true;
            sp.banished_by = None;
            sp.fade_deadline = None;
        }
        let plain_fade = build_player_scene(&view_for(&e, Seat::A), &[]);
        assert_eq!(
            plain_fade
                .quads
                .iter()
                .filter(|q| q.layer == Layer::Lamp)
                .count(),
            0,
            "an unrecoverable fade gets NO rescue glow"
        );
        let plain_pip = plain_fade
            .quads
            .iter()
            .find(|q| q.layer == Layer::Marker)
            .expect("the unrecoverable fade still gets a (dim grey) pip");
        // The dim fade pip is grey (no channel dominates) — explicitly NOT the recede amber.
        assert!(
            !(plain_pip.color.r > 0.7 && plain_pip.color.r > plain_pip.color.g),
            "the unrecoverable fade's pip is dim grey, not amber: {:?}",
            plain_pip.color
        );

        // (b) The SAME spirit, now standing-Faded (banished in combat, in its window).
        let mut e2 = engine();
        {
            let st = e2.state_mut_for_test();
            put_spirit(st, 12, plain_id, Seat::A);
            let sp = st.board[12].spirit.as_mut().unwrap();
            sp.fading = true;
            sp.banished_by = Some(Seat::B);
            sp.fade_deadline = Some(st.round + 1); // the §0.5 rescue window
        }
        let view = view_for(&e2, Seat::A);
        assert!(
            view.tiles[12].spirit.as_ref().unwrap().combat_faded,
            "the view marks the standing-Faded spirit rescuable (combat_faded)"
        );
        let rescued = build_player_scene(&view, &[]);
        // A warm AMBER rescue glow pools under it (Lamp layer) — absent on the plain fade.
        let glows: Vec<&Quad> = rescued
            .quads
            .iter()
            .filter(|q| q.layer == Layer::Lamp)
            .collect();
        assert!(
            !glows.is_empty(),
            "a standing-Faded form gets the amber rescue glow"
        );
        for q in &glows {
            assert!(
                q.color.r > 0.7 && q.color.r > q.color.g && q.color.b < 0.4,
                "the rescue glow is warm amber (recede hue): {:?}",
                q.color
            );
        }
        // Its corner pip is the recede amber — distinct from the grey unrecoverable-fade pip.
        let pip = rescued
            .quads
            .iter()
            .find(|q| q.layer == Layer::Marker)
            .expect("a standing-Faded pip");
        assert!(
            pip.color.r > 0.7 && pip.color.r > pip.color.g && pip.color.b < 0.4,
            "the standing-Faded pip is amber: {:?}",
            pip.color
        );
        // The board mirror narrates the rescuable window (the screen-reader read).
        let names: Vec<String> = canon_catalog().iter().map(|c| c.name.clone()).collect();
        let reading = describe_tile(&view.tiles[12], Seat::A, &names);
        assert!(
            reading.to_lowercase().contains("standing faded")
                && reading.to_lowercase().contains("rescuable"),
            "the board mirror narrates the standing-Faded rescue window: {reading:?}"
        );
    }

    #[test]
    fn a_face_up_spirit_renders_its_short_name() {
        // Names are id-indexed (as the catalog is); a face-up spirit shows its
        // short label, not the numeric id.
        let cloud = id_of("Cloudling");
        let mut e = engine();
        put_spirit(e.state_mut_for_test(), 12, cloud, Seat::A);
        let mut names = vec![String::new(); canon_catalog().len()];
        names[cloud.0 as usize] = "CLOU".into();
        let scene = build_player_scene(&view_for(&e, Seat::A), &names);
        assert!(
            scene.labels.iter().any(|l| l.text == "CLOU"),
            "the spirit shows its short name, not its id"
        );
    }

    #[test]
    fn movement_cues_pip_movable_green_and_sick_dim() {
        // A "can still step" tile gets a green bottom-left pip; a rested /
        // summoning-sick one gets a dim pip; an uncued board gets neither.
        let cloud = id_of("Cloudling");
        let mut e = engine();
        put_spirit(e.state_mut_for_test(), 12, cloud, Seat::A);
        let view = view_for(&e, Seat::A);

        let plain = build_player_scene_cued(&view, &[], &MoveCues::default());
        let move_pips = |scene: &Scene| {
            // The movement pip is the bottom-left Marker (y offset 0.74); the status
            // pip is top-left (y offset 0.08).
            scene
                .quads
                .iter()
                .filter(|q| q.layer == Layer::Marker && q.y.fract() > 0.7)
                .cloned()
                .collect::<Vec<_>>()
        };
        assert!(move_pips(&plain).is_empty(), "no cue ⇒ no movement pip");

        let movable = build_player_scene_cued(
            &view,
            &[],
            &MoveCues {
                movable: vec![12],
                sick: vec![],
            },
        );
        let mp = move_pips(&movable);
        assert_eq!(mp.len(), 1, "the movable spirit gets one movement pip");
        assert!(mp[0].color.g > mp[0].color.r, "movable ⇒ green pip");

        let sick = build_player_scene_cued(
            &view,
            &[],
            &MoveCues {
                movable: vec![],
                sick: vec![12],
            },
        );
        let sp = move_pips(&sick);
        assert_eq!(sp.len(), 1, "the rested spirit gets one movement pip");
        assert!(
            (sp[0].color.r - sp[0].color.g).abs() < 0.1,
            "rested ⇒ a neutral/dim pip, not green"
        );
    }

    #[test]
    fn interaction_overlays_glow_targets_ring_selection_and_focus() {
        // A default Interaction adds nothing (the plain board is unchanged); a
        // legal-target tile glows green, a selected tile gets the seat's ink ring,
        // and the keyboard cursor gets a gold focus ring.
        let e = engine();
        let view = view_for(&e, Seat::A);

        let plain = build_player_scene_interactive(
            &view,
            &[],
            &MoveCues::default(),
            &Interaction::default(),
        );
        let bare = build_player_scene(&view, &[]);
        assert_eq!(
            plain.quads.len(),
            bare.quads.len(),
            "an empty interaction draws exactly the plain board"
        );

        let inter = Interaction {
            legal: vec![13, 17],
            selected: Some(12),
            focus: Some(7),
        };
        let scene = build_player_scene_interactive(&view, &[], &MoveCues::default(), &inter);
        // Two legal targets ⇒ two green wash quads (Marker layer, a full-cell green
        // fill — both w AND h large, which excludes the thin ring edges).
        let target_fills = scene
            .quads
            .iter()
            .filter(|q| {
                q.layer == Layer::Marker
                    && q.w > 0.5
                    && q.h > 0.5
                    && q.color.g > q.color.r
                    && q.color.g > q.color.b
            })
            .count();
        assert_eq!(target_fills, 2, "each legal target gets a green glow");
        // A gold focus ring somewhere (FOCUS: high red+green, low blue, thin quads).
        assert!(
            scene.quads.iter().any(|q| q.layer == Layer::Marker
                && q.color.r > 0.8
                && q.color.g > 0.6
                && q.color.b < 0.3
                && q.w < 0.5),
            "the keyboard focus tile gets a gold ring"
        );
        // Out-of-range overlay indices are ignored (no panic, no stray quad).
        let oob = build_player_scene_interactive(
            &view,
            &[],
            &MoveCues::default(),
            &Interaction {
                legal: vec![250],
                selected: Some(200),
                focus: Some(199),
            },
        );
        assert_eq!(
            oob.quads.len(),
            bare.quads.len(),
            "out-of-range overlays are dropped"
        );
    }
}
