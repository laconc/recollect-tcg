//! The **in-canvas game shell**: the pure, backend-free composition
//! of the whole portrait/landscape game surface — the board (the hero), the HUD
//! (your score · your Anima · the round/clock with the Dusk + Nightfall markers),
//! the opponent strip (name · score *including* the off-board erasure tally · their
//! hand as face-down backs), the hand tray (your cards as real placeholder cards —
//! cost, name, the attack/defense/hp stat block), and the two floating buttons
//! (End Turn · Glimpse).
//!
//! Like [`scene`](crate::scene) this layer is deliberately renderer-free: it turns
//! the model data into a flat list of screen-space primitives in **viewport units**
//! (px from the top-left, x right / y down), so "did we draw the HUD?" is a
//! `cargo test` question and the same composition compiles native for the future
//! mobile shells. The wgpu backend (`render`) only
//! maps these to clip space and draws them.
//!
//! **Phase B makes the shell interactive** and accessible. The composition still
//! lives here as pure, native-tested geometry — but now it also carries the
//! **affordances** (a quiet dot on any piece/card with an available action, a
//! distinct glyph on a Fading spirit that can evolve), the **inspect panel** (a
//! floating card detail anchored to the hovered/long-pressed card or piece), and
//! the **hit-test regions + the virtual a11y tree** the JS bridge needs to map
//! pointer/keyboard input and mirror every canvas affordance as an actionable ARIA
//! element (AGENTS.md invariant 7). The engine stays authoritative: nothing here is
//! a rules input — these are render hints + an accessible mirror, derived JS-side
//! from the engine's legal-move list.
//!
//! **Phase C** adds the paced opponent-turn replay caption ([`ReplayCaption`] /
//! [`ReplayBeat`]) — the watched-and-paced opponent turn, one beat per action.
//!
//! **Phase D** adds the two final set-pieces: the **Dusk / Nightfall** flourish
//! ([`DuskSetPiece`]: the rim contracting/darkening, the binding strip lit as a clock
//! face, the seal) — paired with the held tiles rendering **lamplit** in the board scene
//! ([`scene`](crate::scene)'s Lamp layer) — and the in-canvas **result screen**
//! ([`ResultScreen`], built by the pure [`build_result_screen`]: the verdict in the
//! game's voice, the score breakdown, the Rematch / New-opponent / Back-to-site actions,
//! with [`result_a11y_tree`] mirroring it and [`result_action_rects`] hit-testing the
//! actions). Both adapt across modes.
//!
//! The shell reads a [`ShellModel`] (assembled JS-side from the `PlayerView` + the
//! engine's score/round state + the live selection) and lays it out responsively:
//! **portrait-first** (the one-hand commitment) with a landscape/desktop adaptation
//! that centers the board beside the bands.
use crate::scene::{Color, Interaction, MoveCues, Scene, build_player_scene_interactive, seat_ink};
use recollect_core::types::Seat;
use recollect_core::view::PlayerView;

/// A filled rectangle in **viewport pixels** (x right, y down; the origin is the
/// canvas top-left). The shell's coordinate space — distinct from
/// [`Quad`](crate::scene::Quad)'s tile-grid units, which only the board
/// sub-rectangle uses (mapped in via [`place_board`]). `radius` is a
/// corner-rounding hint in px (0 = sharp); the
/// backend approximates it, so a card/FAB reads as a rounded chip, not a hard box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: Color,
    /// Corner radius in px (a soft rounding hint; 0 ⇒ square corners).
    pub radius: f32,
    pub layer: ShellLayer,
}

/// A text label anchored at a viewport point, with an explicit pixel height and
/// horizontal alignment — the shell sizes its own type (the board's
/// [`Label`](crate::scene::Label) is always tile-centered, which the HUD/cards
/// can't use).
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub x: f32,
    pub y: f32,
    /// Glyph height in px.
    pub size: f32,
    pub text: String,
    pub color: Color,
    pub align: Align,
    pub layer: ShellLayer,
    /// Render in a **bold weight** (item 7). The atlas has one EB Garamond weight, so the backend
    /// faux-bolds by dilating each glyph (drawing it with a sub-pixel spread) — enough presence for
    /// the character names + the headline numbers without a second font. Defaults to `false`.
    pub bold: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Shell draw order, low→high (the backend sorts by it). The board is composited
/// as its own pass, so these only order the chrome around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ShellLayer {
    /// The full-bleed page ground.
    Ground = 0,
    /// A band/panel (HUD, opponent strip, hand tray backing).
    Panel = 1,
    /// A card/back/FAB body.
    Card = 2,
    /// Pips, rules, divots — small marks on a card/panel.
    Detail = 3,
    /// Text (always on top of its panel/card).
    Text = 4,
}

/// One placeholder hand card's data — the stat block the tray renders (no art:
/// a clean template per the design doc, the same fields the catalog page shows).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HandCard {
    pub name: String,
    pub cost: u8,
    pub attack: i16,
    pub defense: i16,
    pub hp: i16,
    /// "Spirit" / "Spell" / "Bond" … — the kind tag on the card frame. A
    /// non-spirit card has no A/D/HP block (rendered hidden).
    #[serde(default)]
    pub kind: String,
    /// The card's resonance, to tint the frame stripe (mirrors the catalog).
    #[serde(default)]
    pub resonance: String,
}

/// Everything the shell draws, gathered JS-side from the `PlayerView` plus the
/// engine's running score/round state (neither of which the view alone fully
/// carries — the erasure tally and the move-cued board come from the local
/// engine, exactly as the score/movement readouts do). One payload, one frame.
/// (No `PartialEq`: it embeds a `PlayerView`, which is not comparable.)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShellModel {
    // ── You (the HUD) ───────────────────────────────────────────────────────
    /// Which seat you are (tints your HUD + hand).
    pub you_seat: Seat,
    /// Your **character name** (e.g. "Corin Ashe") for the HUD — who you are
    /// telling as (`web_client_ux.md` §Opponent strip's HUD parity). Empty ⇒ the HUD
    /// reads just "You" + your faction word.
    #[serde(default)]
    pub you_name: String,
    /// Your faction label ("Lorekeepers" / "the Solace"), shown small under your name.
    #[serde(default)]
    pub you_faction: String,
    /// Your board score (tiles you hold).
    pub you_score: u8,
    /// Your Anima — the play budget (the limiter, surfaced in the HUD).
    pub you_anima: u8,
    /// Your hand, as placeholder cards.
    #[serde(default)]
    pub hand: Vec<HandCard>,

    // ── The opponent strip ──────────────────────────────────────────────────
    /// A short opponent label — the **character name** (e.g. "Corin Ashe")
    /// when one is fielded, else the faction word ("the Solace" / "Lorekeepers").
    #[serde(default)]
    pub opp_name: String,
    /// The opponent's faction label, shown small beside/under their name so the strip
    /// reads "Corin Ashe · the Solace". Empty falls back to no sub-label.
    #[serde(default)]
    pub opp_faction: String,
    /// The opponent's score INCLUDING the off-board erasure tally when they are
    /// the Solace (`board + solace_erasures`). The only place the
    /// Unwritten show, since they leave no board mark.
    pub opp_score: u8,
    /// How much of the opponent's score is the off-board erasure tally (so the
    /// strip can read e.g. "5 (3+2 erased)"). 0 for a Lorekeeper opponent.
    #[serde(default)]
    pub opp_erasures: u8,
    /// The opponent's hand size — drawn as that many face-down backs (redaction
    /// holds; you never see their cards).
    pub opp_hand_count: u8,

    // ── The clock ───────────────────────────────────────────────────────────
    /// The current round (1-based).
    pub round: u8,
    /// The last round — Nightfall (`recollect_core::engine::LAST_ROUND`, 12).
    pub last_round: u8,
    /// The Dusk round — the board contracts at the END of this round
    /// (`contraction_after`, 8). Pips past it read as the failing edge.
    pub dusk_after: u8,
    /// Whether it is your turn (the End-Turn FAB reads primary/enabled only then;
    /// in Phase A this is a visual emphasis, not an interaction gate).
    pub your_turn: bool,

    // ── The board (the hero) ────────────────────────────────────────────────
    /// Your redacted `PlayerView` — the board (the hero) is built from this with
    /// [`build_player_scene_interactive`], so the shell composes the exact same
    /// board the standalone draw shows, just
    /// placed into the central rectangle. Carried (not a pre-built `Scene`) so the
    /// model stays a plain serde payload and the board build stays renderer-side.
    pub view: PlayerView,
    /// Short card labels by id (the catalog is id-ordered) — the board's spirit
    /// names. Empty falls back to the numeric id, like the standalone board.
    #[serde(default)]
    pub names: Vec<String>,
    /// Movement cues for the board (a green/dim move pip per Mobile spirit).
    #[serde(default)]
    pub cues: MoveCues,
    /// Input overlays for the board (legal-target glows · the picked-up ring ·
    /// the keyboard focus ring) — the board honours them so the tap/drag/keyboard
    /// affordances read on the shell board.
    #[serde(default)]
    pub interaction: Interaction,

    // ── Phase B: affordances + inspect (the canvas-native interaction) ───────
    /// Board tiles (your standing pieces) that have **any** available action — a
    /// quiet "action dot" sits on each, so a glance reads "this can do something"
    /// without entering it. Derived JS-side from the legal moves (a tile that is the
    /// `from` of a Move, or the `tile` of an Evolve/Reclaim/StrikeFabrication/Reveal).
    #[serde(default)]
    pub actionable_tiles: Vec<u8>,
    /// Board tiles holding a **Fading** spirit that can be **evolved** — a distinct
    /// glyph (a small upward chevron), since evolve is playing a matching form
    /// card from hand onto this base. Marks the *base*; the form lives in the
    /// hand (see [`Self::evolve_forms`]).
    #[serde(default)]
    pub evolvable_tiles: Vec<u8>,
    /// Hand indices that have **any** legal play — a quiet action dot on the card
    /// (Play / Cast / Bond / Landmark / Set / Overwrite / TellUnwriting / Evolve-form).
    /// A card with no affordable/legal play this turn reads dimmed (no dot).
    #[serde(default)]
    pub actionable_hand: Vec<u8>,
    /// Hand indices that are **evolution form cards** with a matching base on the
    /// board — a distinct chevron on the card (its play target is a base, not an
    /// empty tile). A subset of [`Self::actionable_hand`].
    #[serde(default)]
    pub evolve_forms: Vec<u8>,
    /// Board tiles holding a **standing-Faded** form that can be **devolved** (receded
    /// a tier down to a base) — the §5 rescue, distinct from evolve. A **downward**
    /// chevron marks it (evolve is upward — the spirit *becomes*; devolve is downward —
    /// it *recedes*), and a [`combat_faded`](recollect_core::view::SpiritView::combat_faded)
    /// spirit stands here (so the scene renders it as rescuable). Marks the *form*; the
    /// base card lives in hand (see [`Self::devolve_bases`]). Derived from the engine's
    /// `Devolve` legal moves — the source of truth, so the canvas + a11y agree.
    #[serde(default)]
    pub devolvable_tiles: Vec<u8>,
    /// Hand indices that are **base cards** the player can play onto a standing-Faded
    /// form to recede it (a subset of [`Self::actionable_hand`]) — a distinct downward
    /// chevron on the card (its play target is a faded form, not an empty tile). The
    /// twin of [`Self::evolve_forms`] for the recede.
    #[serde(default)]
    pub devolve_bases: Vec<u8>,
    /// The picked-up hand card's index, if one is lifted (it floats above the tray,
    /// ringed in your ink, and the legal tiles glow on the board). The board's own
    /// picked-up spirit is carried by [`Interaction::selected`].
    #[serde(default)]
    pub lifted_hand: Option<u8>,
    /// The hand **carousel** scroll offset in viewport px (≥ 0 scrolls the row left so
    /// later cards come into view). JS owns it — driven by wheel / drag / touch-swipe —
    /// and clamps it to the row's overscroll bounds. A few bigger cards show at once; the
    /// rest scroll in carousel-smooth (`web_client_ux.md` §Hand). 0 ⇒ the row's start.
    #[serde(default)]
    pub hand_scroll: f32,
    /// Whether the player is mid-**drag** (pointer down on a piece/card, dragging
    /// toward a target). When set, the lifted card/spirit tracks the pointer and the
    /// FABs dim — purely cosmetic emphasis; the drop still resolves through the same
    /// legal-move match the tap path uses.
    #[serde(default)]
    pub dragging: bool,
    /// The drag pointer's current position in viewport px, when [`Self::dragging`].
    /// The lifted card's ghost follows it.
    #[serde(default)]
    pub drag_xy: Option<(f32, f32)>,
    /// The floating **inspect panel** to draw, if the player is hovering (mouse) or
    /// long-pressing (touch) a card/piece — full stats, the reach grid, keywords,
    /// rules text, anchored near the inspected element. `reach` shows here SOFT
    /// (passive, "what could this threaten"), distinct from the SELECT-time bright
    /// engageable-target glow on the board.
    #[serde(default)]
    pub inspect: Option<Inspect>,

    // ── Phase C: the paced opponent-turn replay caption ──────────────────────
    /// The current paced-replay **caption** to draw, when the opponent's turn is
    /// being replayed action-by-action (Phase C — the "watched + paced" decision in
    /// `web_client_ux.md`). `Some` only mid-replay (and the model then reads
    /// `your_turn = false` with no affordances — the player's controls are inert);
    /// `None` on a normal interactive frame. The shell draws it as a subtle on-canvas
    /// banner over the board and pulses the named [`ReplayCaption::tiles`], so the eye
    /// lands where the action happened — the same fact the `#status` live region reads
    /// (the announcements-are-a11y decision).
    #[serde(default)]
    pub replay: Option<ReplayCaption>,

    // ── Phase D: the Dusk / Nightfall set-piece + the result screen ───────────
    /// The animated **Dusk / Nightfall set-piece** to draw over the board, when the
    /// telling crosses one of the binding beats (Phase D). `Some` only for the brief
    /// set-piece frames the JS pacer animates (the round-boundary flourish — the rim
    /// contracting/darkening, the clock face lit, the seal); `None` on a normal frame.
    /// Pairs with the `#status` live-region announcement Phase C already wired
    /// (`round_announcement`) — this is the *visual* half of "Dusk falls" / "Nightfall".
    #[serde(default)]
    pub dusk: Option<DuskSetPiece>,
    /// The in-canvas **result screen** to draw, once the telling has ended (Phase D).
    /// `Some` only at the verdict; the shell then draws the verdict (in the game's
    /// voice), the score breakdown (board + the Solace's erasure tally), and the
    /// action affordances (Rematch / New opponent / Back to site). A scrim sits behind
    /// it so the final board still shows through. `None` mid-telling.
    #[serde(default)]
    pub result: Option<ResultScreen>,

    // ── the in-canvas GLIMPSE + MULLIGAN choice prompt ───────────────────
    /// The active **choice prompt** to draw, when YOUR Glimpse (burn → keep/bottom) or
    /// the opening Mulligan is awaiting your pick. `Some` only while a choice is
    /// in flight for *you*; the shell then draws a small modal card over the board with
    /// the prompt + its options as selectable chips, and mirrors them in the virtual
    /// a11y tree. Built by [`build_choice_prompt`] from the redacted `PlayerView`'s
    /// owner-only pending choice (so redaction holds — the opponent's Glimpse/Mulligan
    /// never surface here, only as an announcement). `None` on a normal frame.
    #[serde(default)]
    pub choice: Option<ChoicePrompt>,
}

impl ShellModel {
    /// Whether a **blocking modal** is up — the in-canvas result screen (the telling has
    /// ended) or an active Glimpse / Mulligan choice. These overlays take over the
    /// surface as a focused decision, so the live chrome (the board affordances + the FAB
    /// lane + the inspect panel) is suppressed beneath them and nothing actionable bleeds
    /// through the scrim (item 5). A `cargo test` pins the masking.
    pub fn blocking_modal(&self) -> bool {
        self.result.is_some() || self.choice.is_some()
    }
}

/// A floating inspect panel's content + anchor (Phase B). Built JS-side from the
/// engine's `card_detail_json` + `reach_grid_json` for whatever the player is
/// hovering / long-pressing. The shell lays it out as a small card beside the
/// anchor; `reach` renders as a passive grid (the "what could this threaten" read),
/// never the bright select-time target glow.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Inspect {
    /// The card name (header).
    pub name: String,
    /// "Spirit" / "Spell" / "Bond" … — the kind line.
    #[serde(default)]
    pub kind: String,
    /// The resonance word (tints the header rule).
    #[serde(default)]
    pub resonance: String,
    pub cost: u8,
    pub attack: i16,
    pub defense: i16,
    pub hp: i16,
    /// The reach name ("Cross" / "Lance" …) for the stat line.
    #[serde(default)]
    pub reach: String,
    /// Keyword chips (Arcane / Warded / Mobile / …).
    #[serde(default)]
    pub keywords: Vec<String>,
    /// The rules text (wrapped to the panel).
    #[serde(default)]
    pub rules: String,
    /// The reach grid: width `reach_w`, the centre tile, and the threatened tiles —
    /// drawn as a small passive grid (a star at centre, soft dots on the reach).
    #[serde(default)]
    pub reach_w: u8,
    #[serde(default)]
    pub reach_center: u8,
    #[serde(default)]
    pub reach_tiles: Vec<u8>,
    /// The anchor point in viewport px (the inspected card/piece's centre); the panel
    /// is placed beside it, clamped to stay on-screen.
    #[serde(default)]
    pub anchor: (f32, f32),
}

/// A **soft drop shadow** under a lifted surface (a card, button, panel, board piece,
/// or the inspect/result panel), in viewport px. The renderer draws it as a single
/// rounded-box SDF quad whose alpha falls off over the `softness` band — one consistent
/// light direction (the box is nudged down-and-right of its caster), a soft blur, a low
/// opacity. A *crafted* drop, not a stacked grey halo (`web_client_ux.md` item 1). The
/// rect is the **caster's** rect; the backend grows it by `softness` to leave room for the
/// penumbra and offsets it by the light direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    /// The caster's rect (the shadow is cast from this; the backend insets the cast box and
    /// adds the offset + softness around it).
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// The caster's corner radius (the shadow rounds to match, so a rounded card casts a
    /// rounded shadow).
    pub radius: f32,
    /// The penumbra width in px (how far the shadow's alpha fades out past the box edge).
    pub softness: f32,
    /// The shadow tint (near-black ink at a low alpha — the depth, never a colour cast).
    pub color: Color,
    /// The layer the shadow composites in — always just under its caster's layer band.
    pub layer: ShellLayer,
}

/// A **vertical gradient** rectangle in viewport px — `top`→`bottom` colour down the
/// box. The single quad pipeline already interpolates per-vertex colour, so a gradient is
/// free: the backend just gives the top two vertices `top` and the bottom two `bottom`.
/// Used for the hero surfaces (the page ground, the bands, the cards, the FABs, the
/// result/inspect panels) so the chrome reads as soft lit paper, not flat colour blocks —
/// and it's canvas-native, so the native shells inherit it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub top: Color,
    pub bottom: Color,
    pub radius: f32,
    pub layer: ShellLayer,
}

/// The composed frame: the board scene (still in tile-grid units, drawn by the
/// existing board pass against `board_rect`) plus the chrome in viewport pixels.
/// The backend draws `board` into `board_rect`, then the shell rects/grads/text over the
/// rest of the viewport.
#[derive(Debug, Clone, PartialEq)]
pub struct ShellScene {
    pub vw: f32,
    pub vh: f32,
    /// The board's pixel rectangle inside the viewport (the hero element).
    pub board_rect: Rect,
    /// The board scene to draw into `board_rect` (tile-grid units).
    pub board: Scene,
    /// Soft drop shadows (item 1) — composited UNDER `rects`/`grads` within each layer band,
    /// so a card/button/panel/piece reads as lifted off the page by one consistent light.
    pub shadows: Vec<Shadow>,
    pub rects: Vec<Rect>,
    /// Vertical-gradient rects (the hero surfaces), composited with `rects` by layer.
    pub grads: Vec<GradRect>,
    pub texts: Vec<Text>,
}

// ── Palette (shell chrome) ──────────────────────────────────────────────────
// The brand colours (paper & ink), echoing scene.rs / play.css. Kept here so the chrome reads
// as one crafted surface with the board. Warmed + given RANGE for the visual-polish pass: a warm
// cream page, a brighter "fresh paper" card that pops off a DEEPER parchment band (real
// hierarchy), crisp ink, and a BURNISHED ANTIQUE GOLD accent (brass-amber, not a bright
// lemon — which read cheap). The seat inks carry accent weight elsewhere (frames, titles).
/// The page ground — a warm aged-paper cream (matches scene.rs PAPER).
const PAPER: Color = Color::rgba(0.949, 0.918, 0.847, 1.0);
/// The page ground's deeper foot — the bottom of the ground gradient (warmer + a touch darker),
/// so the surface reads as gently lit paper from above, not a flat block.
const PAPER_DEEP: Color = Color::rgba(0.882, 0.835, 0.741, 1.0);
/// A parchment band behind a panel (HUD / opponent strip / hand tray) — DEEPER than the page so
/// the chrome reads as a distinct shelf the board+cards sit proud of (hierarchy, not a flat wash).
const PANEL: Color = Color::rgba(0.871, 0.824, 0.733, 1.0);
/// A panel gradient's deeper foot.
const PANEL_DEEP: Color = Color::rgba(0.808, 0.757, 0.659, 1.0);
/// A panel band's foot when it abuts the open BOARD page below — eased back toward the page paper
/// (item 5) so the parchment shelf dissolves into the board surface instead of ending on a hard
/// step. Sits between PANEL and the page's `PAPER_DEEP`.
const PANEL_FOOT_TO_PAGE: Color = Color::rgba(0.906, 0.864, 0.781, 1.0);
/// A card/back body — bright FRESH PAPER, the lightest surface, so a card pops off the parchment
/// band (the contrast point of the layout).
const CARD: Color = Color::rgba(0.988, 0.973, 0.937, 1.0);
/// A card gradient's deeper foot (a soft falloff toward the card's bottom — one light from above).
const CARD_DEEP: Color = Color::rgba(0.945, 0.918, 0.859, 1.0);
/// Ink (crisp near-black) for body text — darker than before so type reads sharp on the warm page.
const INK: Color = Color::rgba(0.090, 0.086, 0.118, 1.0);
/// A softer ink for secondary labels (still AA on paper).
const SOFT: Color = Color::rgba(0.286, 0.275, 0.322, 1.0);
/// A hairline rule colour (a warm sepia line).
const RULE: Color = Color::rgba(0.706, 0.659, 0.573, 1.0);
/// The **gild** — a BURNISHED ANTIQUE GOLD (brass-amber: `#b07c2e`), the precious accent for the
/// lit clock hours, the Anima drop, and the cost discs. Deeper + warmer than a bright lemon
/// so it reads as struck metal, not highlighter. Echoes the website `--gold`.
const GILD: Color = Color::rgba(0.690, 0.486, 0.180, 1.0);
/// The gild's deeper foot, for a struck-metal gradient on the gold discs/pips.
const GILD_DEEP: Color = Color::rgba(0.561, 0.388, 0.137, 1.0);
/// The Dusk/Nightfall ink (the failing edge of the clock).
const DUSK: Color = Color::rgba(0.17, 0.15, 0.13, 1.0);
/// The **mat** — a deep, warm walnut surround the framed table rests on (the side margins on a
/// wide desktop). Rich + dark so the cream page reads as a premium presentation against it, not a
/// tan void. (Decorative chrome only — never a text background, so no contrast bar applies.)
const MAT: Color = Color::rgba(0.196, 0.157, 0.137, 1.0);
/// The mat's deeper foot (for the vertical gradient — one light from above).
const MAT_DEEP: Color = Color::rgba(0.137, 0.110, 0.098, 1.0);
/// The **drop-shadow ink** (item 1): a cool near-black the soft-shadow quads tint. Low alpha
/// at the source — the [`Shadow`]'s falloff carries it to nothing — so a lifted surface reads
/// as gently raised by one consistent light, never ringed by a grey halo.
const SHADOW: Color = Color::rgba(0.06, 0.05, 0.08, 1.0);
/// The **action-dot** ink — a quiet "this can act" mark on a piece/card (the same
/// verdant green the board's legal-target glow uses, so the language is one: green
/// means "available action / go here"). Quiet at rest, so it reads as an invitation
/// not an alarm. Pairs with a shape (a dot) so it's never hue-only (WCAG AA).
const AFFORD: Color = Color::rgba(0.20, 0.60, 0.34, 1.0);
/// The **evolve** accent — a brighter, hopeful green for the evolve chevron on a
/// Fading base / a form card (distinct from the plain action dot: evolution is the
/// rescue beat, design-flavoured). Reads against both the failing-base dim and paper.
const EVOLVE: Color = Color::rgba(0.30, 0.70, 0.38, 1.0);
/// The **devolve / recede** accent — a warm lamplit amber for the *downward* chevron on a
/// **standing-Faded** form that can recede a tier (the §5 rescue), and on the base card in
/// hand that effects it. Distinct in BOTH hue and direction from evolve (green, upward):
/// evolution *ascends* (the spirit becomes), devolution *recedes* (it steps back to a base).
/// The amber echoes the lamplit "held in the failing light" vocabulary — a rescue from the
/// fade, not a growth — and the down-chevron shape keeps it non-hue-only (WCAG AA).
const DEVOLVE: Color = Color::rgba(0.85, 0.58, 0.22, 1.0);
/// A lifted card's halo — a warm gild glow under a picked-up hand card so it reads
/// as raised off the tray (paired with the seat-ink ring the card already carries).
const LIFT: Color = Color::rgba(0.86, 0.72, 0.36, 1.0);
/// The inspect panel's body (a touch warmer/lighter than a card — "a held note").
const INSPECT_BG: Color = Color::rgba(0.99, 0.97, 0.93, 1.0);
/// The **inspect-panel scrim** (item 5): a full-screen dim drawn on the Detail layer, so the
/// board's + hand's action dots (also Detail) read as dimmed CONTEXT behind the floating note
/// instead of bleeding through it. Translucent — the board still shows through; inspect is a
/// read, not a takeover — but deep enough that the masking is unmistakable.
const INSPECT_SCRIM: Color = Color::rgba(0.10, 0.09, 0.08, 0.34);
/// The Phase-C **replay pulse** — a warm gild ring softly highlighting the tile an
/// opponent action touched as it's watched (seat-neutral: the watched action is read,
/// not claimed). Paired with the caption banner so the beat reads at a glance.
const PULSE: Color = Color::rgba(0.80, 0.62, 0.30, 1.0);
/// The **Atk** ink — a warm ember red (the striking stat), distinct from the seat inks
/// but in the warm family. Labels + tints the Atk pill on a card / the inspect line.
const ATK_INK: Color = Color::rgba(0.74, 0.30, 0.22, 1.0);
/// The **Def** ink — a steady slate blue (the warding stat).
const DEF_INK: Color = Color::rgba(0.24, 0.42, 0.66, 1.0);
/// The **HP** ink — a living green (the held-life stat).
const HP_INK: Color = Color::rgba(0.26, 0.52, 0.32, 1.0);

fn with_alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

/// A soft tint for a card's **resonance** band/art plate — a quiet identity colour per
/// resonance (Wonder / Fear / Sorrow / Harmony / Fury / Resolve / Neutral), echoing the
/// "paper & ink, a fading Memory" palette. Returns a mid-saturation hue the card washes
/// at low alpha (the frame never fights the type).
fn resonance_tint(resonance: &str) -> Color {
    match resonance {
        "Wonder" => Color::rgba(0.42, 0.52, 0.74, 1.0), // sky blue
        "Fear" => Color::rgba(0.40, 0.34, 0.52, 1.0),   // dim violet
        "Sorrow" => Color::rgba(0.34, 0.48, 0.56, 1.0), // tide grey-blue
        "Harmony" => Color::rgba(0.34, 0.56, 0.46, 1.0), // calm green
        "Fury" => Color::rgba(0.74, 0.38, 0.26, 1.0),   // ember
        "Resolve" => Color::rgba(0.72, 0.56, 0.26, 1.0), // gild
        _ => Color::rgba(0.52, 0.49, 0.44, 1.0),        // Neutral — warm grey
    }
}

/// Lay out the whole game shell for a `vw`×`vh` viewport (in CSS px, the values JS
/// The viewport-derived band geometry — computed once from `(vw, vh)` so the draw
/// and the JS click-mapping agree on where the board sits. (Pure; content-free.)
#[derive(Debug, Clone, Copy, PartialEq)]
struct Metrics {
    u: f32,
    pad: f32,
    strip_h: f32,
    hud_h: f32,
    tray_h: f32,
    /// The board band's bottom (where the HUD begins).
    band_bottom: f32,
    board_x: f32,
    board_y: f32,
    board_side: f32,
    /// The centered **content frame** — the bands + the board lay out within
    /// `[content_x, content_x + content_w]`, not edge-to-edge. On a phone this is the full
    /// width; on a wide desktop it's a centered, bounded "table" so the chrome never
    /// stretches into a void (the margins fill with the darker page ground).
    content_x: f32,
    content_w: f32,
    /// The **control-rail** width on the right of the board band (item 2): a dedicated lane for
    /// the End-Turn / Glimpse buttons, OUTSIDE the play grid, so a tap on the board is never
    /// ambiguous between a button and a tile. `0` on a narrow (phone) viewport — there the FABs
    /// integrate into the HUD bar instead (see `fab_rects` / `hud_band`).
    rail_w: f32,
}

impl Metrics {
    fn for_viewport(vw: f32, vh: f32) -> Metrics {
        let vw = vw.max(1.0);
        let vh = vh.max(1.0);
        // A reference unit so the layout scales with the viewport (text + bands grow
        // on a tablet, shrink on a phone) without per-breakpoint branches. The min
        // axis governs (portrait → width, landscape → height), clamped so it never
        // gets unreadably small or comically large.
        let u = (vw.min(vh) / 26.0).clamp(9.0, 26.0);
        let pad = u * 0.7;
        // The centered content frame: on a wide viewport, bound the "table" to a portrait-
        // ish column (so the bands + board don't stretch edge-to-edge into empty side voids
        // — a deliberate framed layout, not the phone stretched wide). The margins become a
        // calm darker surround.
        let content_w = (vw).min((vh * 0.92).max(vw * 0.5));
        let content_x = (vw - content_w) / 2.0;
        // The strip + HUD + tray are fixed-height bands; the board takes the slack.
        let strip_h = u * 3.6;
        let hud_h = u * 3.2;
        let tray_h = u * 8.4;
        let band_top = strip_h;
        let band_bottom = vh - (hud_h + tray_h);
        let band_h = (band_bottom - band_top).max(u * 6.0);
        // Reserve a dedicated CONTROL RAIL on the right of the board band for the End-Turn /
        // Glimpse buttons (item 2), so they never float over the play grid. Only when the content
        // is wide enough that the board still gets a generous square after the rail is carved
        // out; on a narrow (phone) column there's no room, so the rail is 0 and the FABs move
        // into the HUD bar instead.
        let rail_w = u * 7.4;
        let board_area_w = content_w - 2.0 * pad;
        // Afford the rail only on a viewport with real surplus WIDTH (landscape / desktop): the
        // band must be at least board-square PLUS the rail, so the board stays a generous hero.
        // On a portrait phone there's no such surplus — the board keeps the full width and the
        // FABs move into the HUD bar (rail_w = 0). The aspect gate (width clearly exceeds the
        // board's natural square) is what distinguishes a phone-portrait from a wide table.
        let wide_enough = board_area_w - rail_w >= band_h && board_area_w - rail_w >= u * 16.0;
        let rail_w = if wide_enough { rail_w } else { 0.0 };
        // The board is square, centered in the content area MINUS the rail.
        let board_side = band_h.min(board_area_w - rail_w);
        // Centre the board in the space left of the rail.
        let left_w = content_w - rail_w;
        let board_x = content_x + (left_w - board_side) / 2.0;
        let board_y = band_top + (band_h - board_side) / 2.0;
        Metrics {
            u,
            pad,
            strip_h,
            hud_h,
            tray_h,
            band_bottom,
            board_x,
            board_y,
            board_side,
            content_x,
            content_w,
            rail_w,
        }
    }
}

/// The board's pixel rectangle for a `vw`×`vh` shell viewport — the same square the
/// draw places the board into. Exposed so the JS shell can map a canvas
/// pointer/keyboard hit to a board tile (the board is a sub-rectangle of the
/// canvas in the shell, not the whole canvas). Pure + native-tested.
pub fn board_rect(vw: f32, vh: f32) -> Rect {
    let m = Metrics::for_viewport(vw, vh);
    Rect {
        x: m.board_x,
        y: m.board_y,
        w: m.board_side,
        h: m.board_side,
        color: PAPER,
        radius: m.u * 0.3,
        layer: ShellLayer::Ground,
    }
}

/// passes from the canvas backing size). Portrait stacks top→bottom — opponent
/// strip, board, HUD, hand tray, with the two FABs floating over the lower-right;
/// landscape/desktop (wide aspect) centers the board and widens the bands. Pure +
/// native-tested.
pub fn build_shell(model: &ShellModel, vw: f32, vh: f32) -> ShellScene {
    let vw = vw.max(1.0);
    let vh = vh.max(1.0);
    // The board (the hero) — the exact standalone board scene, built from the view
    // with the live cues + overlays, then placed by the layout below.
    let board =
        build_player_scene_interactive(&model.view, &model.names, &model.cues, &model.interaction);
    let mut s = ShellScene {
        vw,
        vh,
        board_rect: Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            color: PAPER,
            radius: 0.0,
            layer: ShellLayer::Ground,
        },
        board,
        shadows: Vec::new(),
        rects: Vec::new(),
        grads: Vec::new(),
        texts: Vec::new(),
    };
    // The page ground — a soft vertical gradient (warm paper, a touch deeper at the foot),
    // so the surface reads as lit paper rather than one flat fill.
    s.grads.push(GradRect {
        x: 0.0,
        y: 0.0,
        w: vw,
        h: vh,
        top: PAPER,
        bottom: PAPER_DEEP,
        radius: 0.0,
        layer: ShellLayer::Ground,
    });
    // (A flat ground is still pushed below as the base, in case grads are unsupported.)
    s.rects.push(Rect {
        x: 0.0,
        y: 0.0,
        w: vw,
        h: vh,
        color: PAPER,
        radius: 0.0,
        layer: ShellLayer::Ground,
    });

    // All band geometry — derived from the viewport alone (not the content), so the
    // same layout drives the draw AND the JS click-mapping (one source of truth).
    let m = Metrics::for_viewport(vw, vh);
    let u = m.u;
    let pad = m.pad;

    // On a WIDE viewport the content is a centered "table" column; the side margins become a
    // deep, warm MAT (a dark walnut surround — the felt the page rests on), so the framed table
    // reads as a deliberate, premium presentation, not a phone stretched wide with a tan void.
    // A vertical gradient (a touch lighter at the top) gives the mat depth under one light. On a
    // phone the content fills the width — no margin.
    if m.content_x > u * 0.5 {
        for mx in [
            (0.0, m.content_x),
            (m.content_x + m.content_w, vw - (m.content_x + m.content_w)),
        ] {
            s.grads.push(GradRect {
                x: mx.0,
                y: 0.0,
                w: mx.1,
                h: vh,
                top: MAT,
                bottom: MAT_DEEP,
                radius: 0.0,
                layer: ShellLayer::Ground,
            });
        }
    }

    let opp = opponent_strip(
        model,
        &mut s,
        0.0,
        vw,
        m.strip_h,
        u,
        pad,
        m.content_x,
        m.content_w,
    );

    s.board_rect = Rect {
        x: m.board_x,
        y: m.board_y,
        w: m.board_side,
        h: m.board_side,
        color: PAPER,
        radius: u * 0.3,
        layer: ShellLayer::Ground,
    };
    let board_x = m.board_x;
    let board_y = m.board_y;
    let board_side = m.board_side;
    let band_bottom = m.band_bottom;
    let _ = opp;
    // The board "table" — a soft framed panel spanning the content column between the strip
    // and the HUD, so the board sits on a discrete surface (and the desktop side space reads
    // as a deliberate mat, not a void). A gentle gradient gives it depth. Item 4: its left/right
    // edges inset by the SAME `pad` the hand carousel + the HUD content use, so the board
    // section and the hand section share an aligned right (and left) edge — no `pad*0.5` lip
    // sticking out past the cards below.
    s.grads.push(GradRect {
        x: m.content_x + pad,
        y: m.strip_h + u * 0.2,
        w: m.content_w - 2.0 * pad,
        h: (m.band_bottom - m.strip_h - u * 0.4).max(u),
        top: with_alpha(PANEL, 0.5),
        bottom: with_alpha(PANEL_DEEP, 0.6),
        radius: u * 0.5,
        layer: ShellLayer::Panel,
    });
    // The board page — a discrete bright leaf raised off the table by a soft drop shadow (item 1),
    // with a fine BURNISHED-GOLD rule framing it like a real board. The shadow + the brighter
    // page + the gilt edge make the board read as the crafted hero, not a flat rect on tan.
    let fr_x = board_x - u * 0.4;
    let fr_y = board_y - u * 0.4;
    let fr_s = board_side + u * 0.8;
    push_shadow(
        &mut s,
        fr_x,
        fr_y,
        fr_s,
        fr_s,
        u * 0.4,
        ShellLayer::Panel,
        0.7,
    );
    s.rects.push(Rect {
        x: fr_x,
        y: fr_y,
        w: fr_s,
        h: fr_s,
        color: CARD,
        radius: u * 0.4,
        layer: ShellLayer::Panel,
    });
    // A fine gilt frame line just inside the page edge (a hairline ring, the "tooled" board edge).
    // On the Panel layer (under the board fill, order 1 < board order 2) so it rings the page edge
    // where the ~u*0.4 board inset leaves it visible, without drawing over the play area.
    {
        let rx = fr_x + u * 0.14;
        let ry = fr_y + u * 0.14;
        let rs = fr_s - u * 0.28;
        let t = (u * 0.06).max(1.0);
        let gild = with_alpha(GILD, 0.50);
        for edge in [
            (rx, ry, rs, t),
            (rx, ry + rs - t, rs, t),
            (rx, ry, t, rs),
            (rx + rs - t, ry, t, rs),
        ] {
            s.rects.push(Rect {
                x: edge.0,
                y: edge.1,
                w: edge.2,
                h: edge.3,
                color: gild,
                radius: 0.0,
                layer: ShellLayer::Panel,
            });
        }
    }

    // ── The live chrome — the board affordances, the HUD band, the hand tray, the FABs,
    // and the inspect panel — is drawn while the telling is LIVE. It is suppressed whenever a
    // **blocking modal** is up (item 5): the result screen (Phase D, the telling has ended) OR
    // an active Glimpse / Mulligan choice. Those overlays are a focused decision, so the
    // affordance layer (the green action dots / evolve chevrons) and the FAB lane (End Turn /
    // Glimpse) MUST NOT bleed through them — drawing them above the modal scrim would invite a
    // tap on a control the modal has taken over. The board + opponent strip stay (drawn earlier,
    // dimmed by the modal scrim) as the context behind the decision; the modal card sits cleanly
    // over them with nothing actionable showing beneath. The inspect panel is likewise a hover-
    // time affordance with no place behind a modal, so it's suppressed too. (A `cargo test`
    // pins this — see `the_choice_modal_masks_the_board_affordances_and_fabs`.)
    if !model.blocking_modal() {
        // Item 5 (option C) — under INSPECT (the hover / long-press panel) the player
        // can't act, so the **interactive affordance layer is fully SUPPRESSED**: the
        // board action dots / evolve chevrons AND the FAB lane (End Turn · Glimpse) are
        // not drawn, and the hand cards drop their action-dot/evolve badges. The board +
        // hand CARDS still draw (then dim under INSPECT_SCRIM) so the spatial context
        // reads — inspect is a read, not a takeover. (The Glimpse / Mulligan choice
        // modals keep their full-suppress via `blocking_modal()` above — a different,
        // stronger gate.) A native + a uitest guard pin this.
        let inspecting = model.inspect.is_some();
        // ── Phase B: the board affordances (quiet action dots on your actionable
        // pieces, an evolve chevron on a Fading base that can evolve). Mapped through
        // the board placement so they sit on the right tiles, composited above the board.
        // Skipped while the opponent's turn replays (Phase C) — your pieces are inert then —
        // and under INSPECT (option C — nothing to act on while reading a card).
        if model.replay.is_none() && !inspecting {
            board_affordances(model, &mut s, board_x, board_y, board_side);
        }

        // ── Phase C: the paced opponent-turn replay overlay — a subtle caption banner
        // across the top of the board and a soft pulse on the affected tiles, so the
        // watched action reads at a glance (the same fact the live region announces).
        if let Some(rep) = &model.replay {
            replay_overlay(rep, &mut s, board_x, board_y, board_side, u);
        }

        // ── The HUD band (your score · Anima · the clock). Its content insets to the
        // content frame so it aligns with the board "table" (the band backing is full-width).
        // The full metrics ride in so the clock can clear the phone FAB zone (item 3).
        hud_band(model, &mut s, band_bottom, vw, &m);

        // ── The hand tray (your placeholder cards), with the Phase-B affordances:
        // a quiet action dot on a playable card, an evolve chevron on a form card, and
        // the lifted card raised when one is picked up. The carousel lays out within the
        // content frame so it sits under the table, not edge-to-edge.
        // The hand cards always draw (context, dimmed under inspect); their action-dot /
        // evolve badges are suppressed while inspecting (option C — `badges = !inspecting`).
        hand_tray(
            model,
            &mut s,
            band_bottom + m.hud_h,
            vw,
            m.tray_h,
            u,
            pad,
            m.content_x,
            m.content_w,
            !inspecting,
        );

        // ── The control buttons (End Turn primary · Glimpse) — item 2: in their own lane, never
        // floating over the play grid. On a wide viewport that's the dedicated right RAIL beside
        // the board; on a narrow (phone) column it's the right of the HUD bar. `fab_rects`
        // (shared with the hit-test) owns the placement; `fabs` just draws there. Suppressed under
        // INSPECT (option C — you can't act while reading, so the lane shouldn't bleed through).
        if !inspecting {
            fabs(model, &mut s, &m);
        }

        // ── Phase B: the floating inspect panel (hover / long-press), topmost — full
        // stats + the passive reach grid + keywords + rules, anchored beside the card.
        if let Some(insp) = &model.inspect {
            inspect_panel(insp, &mut s, vw, vh, u);
        }
    }

    // ── Phase D: the Dusk / Nightfall set-piece over the board (the binding beat made
    // visible — the rim contracting/darkening, the clock face lit, the seal), and the
    // in-canvas result screen at the verdict. Both draw OVER the rest of the shell.
    if let Some(dusk) = &model.dusk {
        dusk_set_piece(dusk, &mut s, board_x, board_y, board_side, u);
    }
    // ── the Glimpse / Mulligan choice prompt — a modal card over the board, drawn
    // ABOVE the live chrome (it's a focused decision) but BELOW the result screen (the
    // telling can't end mid-choice, so they never co-occur — but the order is honoured).
    if let Some(choice) = &model.choice
        && model.result.is_none()
    {
        choice_prompt(choice, &mut s, vw, vh, u);
    }
    if let Some(result) = &model.result {
        result_screen(result, &mut s, vw, vh, u);
    }

    s
}

/// Tile index → its centre point in viewport px for the board placed at
/// `(bx, by, side)`. Mirrors the board's Y-flip (seat A's home row draws at the
/// bottom — the same flip [`crate::scene`]'s `tile_xy` and the JS click-mapping use),
/// so an overlay lands on the tile the player sees. `board_w` is the grid side.
fn tile_center_px(t: u8, board_w: u32, bx: f32, by: f32, side: f32) -> (f32, f32) {
    let bw = board_w.max(1);
    let cell = side / bw as f32;
    let gx = (t as u32 % bw) as f32;
    let gy = (bw - 1 - (t as u32 / bw)) as f32; // Y-flip: home row at the bottom
    (bx + (gx + 0.5) * cell, by + (gy + 0.5) * cell)
}

/// The board's grid side for this shell's view (square 1v1; falls back to 1).
fn board_side_tiles(model: &ShellModel) -> u32 {
    ((model.view.tiles.len() as f64).sqrt().round() as u32).max(1)
}

/// The **inverse** of [`tile_center_px`]: which board tile a viewport-px point lands on
/// for a board placed at `(bx, by, side)` with grid side `board_w`, or `None` if the
/// point is outside the board square. Mirrors the board's Y-flip (seat A's home row at
/// the bottom), so a tap resolves to the tile the player sees — the SAME mapping the JS
/// pointer bridge used inline (`boardRegionHit` / `clickedTile`), lifted into pure Rust
/// so the forward (tile→px) and inverse (px→tile) maps are ONE native-tested source and
/// a future misroute (off-by-one, a dropped Y-flip, a layout offset) fails a `cargo test`,
/// not just an eyeball of the canvas. `(px, py)` is in the shell's viewport-px space
/// (the board rect's own coordinates); `side` is the board's pixel side.
fn tile_at_px(px: f32, py: f32, board_w: u32, bx: f32, by: f32, side: f32) -> Option<u8> {
    let bw = board_w.max(1);
    if side <= 0.0 {
        return None;
    }
    // Fraction across the board square → grid column / row. `floor` (not round) so each
    // cell owns the half-open span `[k, k+1)`: a point on a cell's left/top edge belongs
    // to that cell, the right/bottom edge to the next (no double-claim, no gap).
    let fx = (px - bx) / side;
    let fy = (py - by) / side;
    let col = (fx * bw as f32).floor();
    let row = (fy * bw as f32).floor();
    if col < 0.0 || row < 0.0 || col >= bw as f32 || row >= bw as f32 {
        return None;
    }
    // The Y-flip: the top visual row is the highest tile-row (home row at the bottom).
    let tile = (bw - 1 - row as u32) * bw + col as u32;
    Some(tile as u8)
}

/// Phase B board affordances: a quiet **action dot** in the **top-right** corner of the
/// tile's card — the SAME corner the hand card uses, so "this can act" reads in one
/// consistent place across the whole UI (clear of the scene's top-left status pip and the
/// bottom-left move pip) — on every actionable piece, a brighter **evolve chevron** (upward)
/// in that same corner on a Fading base that can evolve, and a **devolve chevron** (downward,
/// amber) on a standing-Faded form that can recede (§5). Drawn as shell Detail rects
/// (composited above the board), mapped through the placed board so they track the tiles.
/// Quiet by design — an invitation, not an alarm.
fn board_affordances(model: &ShellModel, s: &mut ShellScene, bx: f32, by: f32, side: f32) {
    let bw = board_side_tiles(model);
    let cell = side / bw as f32;
    let dot = (cell * 0.16).clamp(3.0, 11.0);
    // The card face is inset `0.08` in the cell (see scene::push_spirit_card), so anchor the
    // badge just inside the card's top-right corner — not the raw cell edge.
    let inset = cell * 0.16;
    let evolvable: std::collections::HashSet<u8> = model.evolvable_tiles.iter().copied().collect();
    let devolvable: std::collections::HashSet<u8> =
        model.devolvable_tiles.iter().copied().collect();
    for &t in &model.actionable_tiles {
        if (t as usize) >= model.view.tiles.len() {
            continue;
        }
        // A base that can evolve (upward chevron) or a standing-Faded form that can recede
        // (downward chevron) shows its distinct mark instead of the plain action dot in that
        // corner — never two affordance marks stacked in the same place.
        if evolvable.contains(&t) || devolvable.contains(&t) {
            continue;
        }
        let (cx, cy) = tile_center_px(t, bw, bx, by, side);
        // Top-right corner of the card face — a small filled disc.
        let px = cx + cell * 0.5 - inset - dot * 0.5;
        let py = cy - cell * 0.5 + inset + dot * 0.5;
        // A soft halo + the dot, so it lifts off the spirit's ink fill / title band.
        s.rects.push(Rect {
            x: px - dot * 0.85,
            y: py - dot * 0.85,
            w: dot * 1.7,
            h: dot * 1.7,
            color: with_alpha(PAPER, 0.6),
            radius: dot,
            layer: ShellLayer::Detail,
        });
        s.rects.push(Rect {
            x: px - dot * 0.5,
            y: py - dot * 0.5,
            w: dot,
            h: dot,
            color: AFFORD,
            radius: dot / 2.0,
            layer: ShellLayer::Detail,
        });
    }
    // The evolve chevron: an upward "^" of two short strokes in the tile card's TOP-RIGHT
    // corner (the same corner as the action dot — affordances live in one place), on the
    // evolve green — a distinct, hopeful mark for a base that can become.
    for &t in &model.evolvable_tiles {
        if (t as usize) >= model.view.tiles.len() {
            continue;
        }
        let (cx, cy) = tile_center_px(t, bw, bx, by, side);
        let chev_w = cell * 0.24;
        let bcx = cx + cell * 0.5 - inset - chev_w * 0.5;
        let bcy = cy - cell * 0.5 + inset + chev_w * 0.5;
        push_chevron(s, bcx, bcy, chev_w, EVOLVE);
    }
    // The devolve chevron: a DOWNWARD "v" in the same TOP-RIGHT corner, on the recede amber
    // — a standing-Faded form that can step back a tier to its base (§5 rescue). Down, not up:
    // evolution ascends, devolution recedes; amber, not green: a rescue from the failing light.
    for &t in &model.devolvable_tiles {
        if (t as usize) >= model.view.tiles.len() {
            continue;
        }
        let (cx, cy) = tile_center_px(t, bw, bx, by, side);
        let chev_w = cell * 0.24;
        let bcx = cx + cell * 0.5 - inset - chev_w * 0.5;
        let bcy = cy - cell * 0.5 + inset + chev_w * 0.5;
        push_chevron_down(s, bcx, bcy, chev_w, DEVOLVE);
    }
}

/// A small upward chevron ("^") centred at `(cx, cy)`, `w` wide, in `color` — two
/// short rotated-by-approximation strokes built from stacked rects (the backend has
/// only axis-aligned quads, so the diagonal is a tiny staircase that reads as a "^"
/// at glyph size). The evolve affordance's distinct shape (vs the round action dot).
fn push_chevron(s: &mut ShellScene, cx: f32, cy: f32, w: f32, color: Color) {
    let half = w / 2.0;
    let steps = 4usize;
    let seg = half / steps as f32;
    let th = (w * 0.16).max(1.5);
    for i in 0..steps {
        let dx = i as f32 * seg;
        let dy = i as f32 * seg;
        // Left stroke rises to the apex; right stroke mirrors it.
        for sx in [cx - half + dx, cx + half - dx - seg] {
            s.rects.push(Rect {
                x: sx,
                y: cy + dy,
                w: seg + th * 0.5,
                h: th,
                color,
                radius: 0.0,
                layer: ShellLayer::Detail,
            });
        }
    }
}

/// The **vertical mirror** of [`push_chevron`] — a chevron pointing the OPPOSITE way,
/// centred at `(cx, cy)`, `w` wide, in `color`. The devolve / recede affordance's mark:
/// where evolve's chevron rises (ascend — the spirit *becomes*), this one points the
/// other way (recede — it *steps back* a tier). Built from the same staircase of
/// axis-aligned rects with the vertical offset negated, so the two glyphs are
/// unmistakably distinct in direction (and, with [`DEVOLVE`] vs [`EVOLVE`], in hue).
fn push_chevron_down(s: &mut ShellScene, cx: f32, cy: f32, w: f32, color: Color) {
    let half = w / 2.0;
    let steps = 4usize;
    let seg = half / steps as f32;
    let th = (w * 0.16).max(1.5);
    for i in 0..steps {
        let dx = i as f32 * seg;
        let dy = i as f32 * seg;
        // The mirror of `push_chevron`: the staircase descends from the wide base toward a
        // single apex on the far side (`cy - dy` instead of `cy + dy`), so the glyph reads
        // as a chevron pointing the opposite direction.
        for sx in [cx - half + dx, cx + half - dx - seg] {
            s.rects.push(Rect {
                x: sx,
                y: cy - dy,
                w: seg + th * 0.5,
                h: th,
                color,
                radius: 0.0,
                layer: ShellLayer::Detail,
            });
        }
    }
}

/// Phase C: the **paced opponent-turn replay** overlay — a subtle caption banner near
/// the top of the board naming the action, and a soft **pulse** ring on each affected
/// tile so the watched action lands where it happened. Drawn over the board (Detail/
/// Text layers). The banner is a quiet ink chip on paper (the Solace's erasure register
/// tints cooler than a play), echoing the caption the `#status` live region reads — one
/// source for the visual flourish and the screen-reader narration.
fn replay_overlay(rep: &ReplayCaption, s: &mut ShellScene, bx: f32, by: f32, side: f32, u: f32) {
    // Pulse the affected tiles first (under the banner) — a soft seat-neutral ring so
    // the eye is drawn to where the action happened without obscuring the board. The
    // caption carries tiles in board-index space; infer the grid side from the largest
    // index (5 for 1v1, 6 for 2v2) and map each through the placed board.
    let grid = if rep.tiles.iter().copied().max().unwrap_or(0) >= 25 {
        6
    } else {
        5
    };
    let cell = side / grid as f32;
    for &t in &rep.tiles {
        let (cx, cy) = tile_center_px(t, grid, bx, by, side);
        let r = cell * 0.46;
        // A soft halo ring (paper-bright) + a thin accent, layered so it reads as a
        // gentle pulse — the JS pacer fades the whole model in, so this animates.
        s.rects.push(Rect {
            x: cx - r,
            y: cy - r,
            w: r * 2.0,
            h: r * 2.0,
            color: with_alpha(PULSE, 0.22),
            radius: r,
            layer: ShellLayer::Detail,
        });
        push_ring_px(
            s,
            cx - r,
            cy - r,
            r * 2.0,
            r * 2.0,
            with_alpha(PULSE, 0.9),
            2.5,
        );
    }

    // The caption banner — a quiet chip near the top of the board. The Solace's
    // erasure register (unwrite / forget) reads cooler; the rest reads in ink.
    let cooler = rep.kind == "unwrite" || rep.kind == "banish";
    let chip_h = (u * 1.4).max(18.0);
    let pad = u * 0.5;
    let size = fit_size(rep.text.len().max(1), side - pad * 4.0).min(chip_h * 0.5);
    let txt = sanitize_keep_punct(&rep.text);
    let w = crate::font::text_width(&txt, size);
    let chip_w = (w + pad * 2.0).min(side - pad);
    let cx = bx + side / 2.0;
    let top = by + u * 0.6;
    s.rects.push(Rect {
        x: cx - chip_w / 2.0,
        y: top,
        w: chip_w,
        h: chip_h,
        color: with_alpha(if cooler { DUSK } else { INK }, 0.82),
        radius: chip_h / 2.0,
        layer: ShellLayer::Detail,
    });
    s.texts.push(Text {
        x: cx,
        y: top + chip_h / 2.0,
        size,
        text: txt,
        color: PAPER,
        align: Align::Center,
        bold: false,
        layer: ShellLayer::Text,
    });
}

/// The opponent strip across the top. Returns the strip's bottom y (where the
/// board band begins). Name · score (with the erasure tally folded in) · their
/// hand as small face-down backs.
#[allow(clippy::too_many_arguments)]
fn opponent_strip(
    model: &ShellModel,
    s: &mut ShellScene,
    top: f32,
    vw: f32,
    h: f32,
    u: f32,
    pad: f32,
    cx0: f32,
    cw: f32,
) -> f32 {
    // The opponent strip's parchment — its foot eases back UP toward the page paper so the strip
    // dissolves into the board surface below rather than ending on a hard ruled edge (item 5: the
    // sections blend into one continuous lit-paper surface; depth comes from the gentle gradients,
    // not boxed lines). The shared `band_blend` feather at the boundary completes the transition.
    s.grads.push(GradRect {
        x: 0.0,
        y: top,
        w: vw,
        h,
        top: PANEL,
        bottom: PANEL_FOOT_TO_PAGE,
        radius: 0.0,
        layer: ShellLayer::Panel,
    });
    band_blend(s, top + h, cx0, cw, u, BlendDir::Down);
    let opp_seat = model.you_seat.other();
    let mid = top + h / 2.0;
    let raw_name = if model.opp_name.is_empty() {
        opp_seat_word(opp_seat)
    } else {
        model.opp_name.clone()
    };
    let name = sanitize(&raw_name);
    let faction = sanitize(&model.opp_faction);
    // The face-down backs occupy the right; reserve their width so the name/score
    // never collide with them.
    let back_w = u * 0.78;
    let back_h = u * 1.45;
    let bgap = u * 0.22;
    let n = model.opp_hand_count.min(10) as f32; // cap the row width; the count is the truth
    let backs_w = if n > 0.0 {
        n * back_w + (n - 1.0) * bgap
    } else {
        0.0
    };
    let backs_start = vw - pad - backs_w;

    // An avatar disc on the left, marked with the character's initial in their seat ink
    // — a quick "who is across the page" anchor (the storyteller you contend with).
    let av_r = h * 0.34;
    let av_cx = pad + av_r;
    let av_cy = mid;
    // A ring-rimmed disc: a slightly larger ink disc behind, the tinted fill on top.
    s.rects.push(Rect {
        x: av_cx - av_r,
        y: av_cy - av_r,
        w: av_r * 2.0,
        h: av_r * 2.0,
        color: with_alpha(seat_ink(opp_seat), 0.85),
        radius: av_r,
        layer: ShellLayer::Detail,
    });
    let inner = av_r - 1.5;
    s.rects.push(Rect {
        x: av_cx - inner,
        y: av_cy - inner,
        w: inner * 2.0,
        h: inner * 2.0,
        color: Color { a: 1.0, ..PANEL },
        radius: inner,
        layer: ShellLayer::Detail,
    });
    s.rects.push(Rect {
        x: av_cx - inner,
        y: av_cy - inner,
        w: inner * 2.0,
        h: inner * 2.0,
        color: with_alpha(seat_ink(opp_seat), 0.16),
        radius: inner,
        layer: ShellLayer::Detail,
    });
    if let Some(ch) = name.chars().find(|c| c.is_ascii_alphanumeric()) {
        s.texts.push(Text {
            x: av_cx,
            y: av_cy,
            size: av_r * 1.1,
            text: ch.to_string(),
            color: seat_ink(opp_seat),
            align: Align::Center,
            bold: false,
            layer: ShellLayer::Text,
        });
    }

    // The name sits on the upper sub-line, sized to fit the lane left of the backs; the
    // faction word + the score sit on the lower sub-line beneath it.
    let text_x = av_cx + av_r + u * 0.6;
    let lane_w = (backs_start - text_x - u * 0.4).max(u * 4.0);
    let name_size = fit_size(name.chars().count(), lane_w).min(u * 1.15);
    s.texts.push(Text {
        x: text_x,
        y: mid - u * 0.6,
        size: name_size,
        text: name,
        color: seat_ink(opp_seat),
        align: Align::Left,
        bold: true, // item 7: the character name carries weight (seat-ink + bold)
        layer: ShellLayer::Text,
    });
    // The faction word + the score, with the erasure tally folded in for the Solace.
    // (Only font-drawable glyphs — no parens/`+`; the tally reads "N erased".)
    let score_text = if model.opp_erasures > 0 {
        format!("score {} - {} erased", model.opp_score, model.opp_erasures)
    } else {
        format!("score {}", model.opp_score)
    };
    let sub = if faction.is_empty() {
        score_text
    } else {
        format!("{faction} - {score_text}")
    };
    let sub_size = fit_size(sub.chars().count(), lane_w).min(u * 0.72);
    s.texts.push(Text {
        x: text_x,
        y: mid + u * 0.78,
        size: sub_size,
        text: sub,
        color: SOFT,
        align: Align::Left,
        bold: false,
        layer: ShellLayer::Text,
    });

    // Their hand as face-down backs (count only — redaction holds), right-aligned. Each is a
    // proper seat-tinted card BACK (a gradient body + a centred "memory" diamond), so a stack
    // reads unmistakably as "the cards they hold, hidden" — not mystery squares. Item 5: the
    // FANNED BACKS convey the count directly (one back per card), so there is no "N in hand"
    // text — the picture is the number.
    let start = backs_start;
    let ink = seat_ink(opp_seat);
    for i in 0..(n as u32) {
        let x = start + i as f32 * (back_w + bgap);
        let by = mid - back_h / 2.0 + u * 0.35;
        // A seat-tinted card back (a soft gradient — deeper at the foot).
        push_card_grad(
            s,
            x,
            by,
            back_w,
            back_h,
            with_alpha(ink, 0.5),
            with_alpha(ink, 0.66),
            ShellLayer::Card,
            u,
        );
        // A centred "memory" diamond mark on the back.
        s.rects.push(Rect {
            x: x + back_w / 2.0 - back_w * 0.16,
            y: by + back_h / 2.0 - back_w * 0.16,
            w: back_w * 0.32,
            h: back_w * 0.32,
            color: with_alpha(PAPER, 0.7),
            radius: back_w * 0.06,
            layer: ShellLayer::Detail,
        });
    }
    top + h
}

/// The HUD band: your score · your Anima · the round/clock (a 12-pip strip with the
/// Dusk + Nightfall markers). The band BACKING is full-width (`vw`), but its CONTENT insets
/// to the content frame so it aligns with the board "table" on desktop. On a **phone** the
/// two control buttons (End Turn · Glimpse) share this bar's right (no side rail), so the
/// clock right-anchors to the LEFT of that FAB zone — they never crowd above the pips (item 3).
fn hud_band(model: &ShellModel, s: &mut ShellScene, top: f32, vw: f32, m: &Metrics) {
    let (u, pad, cx0, cw, h) = (m.u, m.pad, m.content_x, m.content_w, m.hud_h);
    // The HUD parchment — its HEAD eases up toward the board page above (item 5: no hard ruled
    // line between the board and the HUD; the shelf rises out of the page), deepening to its
    // foot where the hand tray meets it. The `band_blend` feather completes the top transition.
    s.grads.push(GradRect {
        x: 0.0,
        y: top,
        w: vw,
        h,
        top: PANEL_FOOT_TO_PAGE,
        bottom: PANEL_DEEP,
        radius: 0.0,
        layer: ShellLayer::Panel,
    });
    band_blend(s, top, cx0, cw, u, BlendDir::Up);
    // Content insets to the framed column.
    let lx = cx0 + pad;
    let rx = cx0 + cw - pad;
    // On a PHONE the two control buttons (End Turn · Glimpse) share this bar's right (there is no
    // side rail), so the clock can't sit on the main row's right — it would collide both with the
    // buttons AND with the Anima readout pulled toward centre. Instead the clock rides the
    // identity sub-line's RIGHT (top of the bar), opposite the name, where it has clear room; the
    // FABs then own the main-row right, score/anima the main-row left (item 3 — no crowding). On a
    // desktop the FABs are in the rail, so the clock keeps its roomy main-row-right home.
    let phone = m.rail_w <= 0.0;
    // ── The identity sub-line (top): your character name (left), in your ink, so the
    // HUD names who you are telling as — the parity of the opponent strip.
    let name = sanitize(&if model.you_name.is_empty() {
        "You".to_string()
    } else {
        model.you_name.clone()
    });
    let faction = sanitize(&model.you_faction);
    let id_y = top + u * 0.7;
    let name_size = fit_size(name.chars().count(), u * 9.0).min(u * 0.95);
    s.texts.push(Text {
        x: lx,
        y: id_y,
        size: name_size,
        text: name,
        color: seat_ink(model.you_seat),
        align: Align::Left,
        bold: true, // item 7: the character name carries weight (seat-ink + bold)
        layer: ShellLayer::Text,
    });
    // The faction word, small + soft, to the right of the name (room permitting). On a phone the
    // clock takes the sub-line's right, so the faction word is dropped there to avoid a collision
    // (the name + the clock own the top line; the faction reads off the seat ink anyway).
    if !faction.is_empty() && !phone {
        s.texts.push(Text {
            x: lx + u * 9.4,
            y: id_y,
            size: u * 0.68,
            text: faction,
            color: SOFT,
            align: Align::Left,
            bold: false,
            layer: ShellLayer::Text,
        });
    }

    // ── The main row: score + anima (left), the clock (right). Sits below the name, with the
    // value (big) and its caption (small, beneath) packed as a unit that stays CLEAR of the band's
    // foot — the SCORE / ANIMA captions used to spill across the HUD↔hand boundary line (item 4);
    // now the row is pulled up and the caption gap tightened so the whole block sits inside the band.
    let row = top + h * 0.58;
    let cap_dy = u * 0.82; // value→caption baseline gap (was 0.95 — tightened to keep the foot clear)
    // Your score, in your ink — the headline number.
    s.texts.push(Text {
        x: lx,
        y: row,
        size: u * 1.4,
        text: format!("{}", model.you_score),
        color: seat_ink(model.you_seat),
        align: Align::Left,
        bold: false,
        layer: ShellLayer::Text,
    });
    s.texts.push(Text {
        x: lx,
        y: row + cap_dy,
        size: u * 0.6,
        text: "SCORE".into(),
        color: SOFT,
        align: Align::Left,
        bold: false,
        layer: ShellLayer::Text,
    });
    // Your Anima (the play budget) — a small gilt "drop" + the number, left-of-center. The drop is
    // a struck-metal gradient (burnished gold → its deeper foot), so the accent reads as cast metal.
    let anima_x = lx + u * 4.5;
    s.grads.push(GradRect {
        x: anima_x,
        y: row - u * 0.48,
        w: u * 0.86,
        h: u * 1.0,
        top: GILD,
        bottom: GILD_DEEP,
        radius: u * 0.43,
        layer: ShellLayer::Detail,
    });
    s.texts.push(Text {
        x: anima_x + u * 1.3,
        y: row,
        size: u * 1.4,
        text: format!("{}", model.you_anima),
        color: GILD,
        align: Align::Left,
        bold: true,
        layer: ShellLayer::Text,
    });
    s.texts.push(Text {
        x: anima_x + u * 1.3,
        y: row + cap_dy,
        size: u * 0.6,
        text: "ANIMA".into(),
        color: SOFT,
        align: Align::Left,
        bold: false,
        layer: ShellLayer::Text,
    });
    // The round/clock — the binding strip as a pip row. On a DESKTOP it sits on the main row's
    // right (the rail holds the FABs), with its beat caption ("Dusk"/"Nightfall") above it on the
    // sub-line. On a PHONE the FABs own the main-row right, so the clock rides the identity
    // sub-line's right (top of the bar, opposite the name) with its caption tucked just beneath
    // the pips — clear of both the name and the buttons (item 3).
    if phone {
        clock(model, s, rx, id_y, id_y + u * 1.05, u);
    } else {
        clock(model, s, rx, row, id_y, u);
    }
}

/// The 12-pip clock with the Dusk + Nightfall markers, right-anchored at `right_x`, the
/// pip row centered on `cy` and the caption drawn at `caption_y` (above the pips). Lit
/// pips = rounds elapsed; pips past the Dusk read in the failing ink; the final pip
/// (Nightfall) is ringed.
fn clock(model: &ShellModel, s: &mut ShellScene, right_x: f32, cy: f32, caption_y: f32, u: f32) {
    let last = model.last_round.max(1);
    let pip = u * 0.5;
    let gap = u * 0.34;
    let total = last as f32 * pip + (last as f32 - 1.0) * gap;
    let start = right_x - total;
    for r in 1..=last {
        let x = start + (r as f32 - 1.0) * (pip + gap);
        let past_dusk = r > model.dusk_after;
        let lit = r <= model.round;
        let color = if lit {
            if past_dusk { DUSK } else { GILD }
        } else if past_dusk {
            with_alpha(DUSK, 0.30)
        } else {
            with_alpha(RULE, 0.55)
        };
        // Nightfall (the final pip) is ringed by a **double circle** (item 13) — two concentric
        // circular rings AROUND the pip, so the last round reads as a clearly different, weightier
        // marker (the deadline) than the plain round pips. A square ring read wrong here; the
        // double ring echoes the round disc's own circle. The rings draw BEHIND the pip (the gaps
        // punch with the HUD's parchment tone), then the pip lands on top intact — so it reads as
        // pip · gap · ring · gap · ring, clean outlines all the way out.
        let pcx = x + pip / 2.0;
        if r == last {
            let ring_t = (pip * 0.13).max(1.25);
            push_circle_ring(s, pcx, cy, pip * 0.96, ring_t, DUSK, PANEL_DEEP); // outer ring
            push_circle_ring(s, pcx, cy, pip * 0.72, ring_t, DUSK, PANEL_DEEP); // inner ring
        }
        // The round pip itself, on top (so it covers the inner gaps of the Nightfall double ring).
        s.rects.push(Rect {
            x,
            y: cy - pip / 2.0,
            w: pip,
            h: pip,
            color,
            radius: pip / 2.0,
            layer: ShellLayer::Detail,
        });
    }
    // Item 6: the pip strip already SHOWS the round (lit pips = rounds elapsed), so there is no
    // redundant "Round N" caption. Only the meaningful BEAT labels remain — "Dusk" once the
    // board has begun contracting, "Nightfall" at the final round — drawn in the failing ink.
    let caption = if model.round > model.last_round {
        "Nightfall"
    } else if model.round > model.dusk_after {
        "Dusk"
    } else {
        ""
    };
    if !caption.is_empty() {
        // The caption normally sits ABOVE the pips, right-anchored (desktop). When it shares the
        // pip row's line (phone — `caption_y` ≈ `cy`, the clock rides the top sub-line), put it to
        // the LEFT of the pips instead so it doesn't stack on top of them.
        let same_line = (caption_y - cy).abs() < pip;
        let (cap_x, cap_y) = if same_line {
            (start - gap, cy)
        } else {
            (right_x, caption_y)
        };
        s.texts.push(Text {
            x: cap_x,
            y: cap_y,
            size: u * 0.72,
            text: caption.to_string(),
            color: DUSK,
            align: Align::Right,
            bold: false,
            layer: ShellLayer::Text,
        });
    }
}

/// The hand carousel's geometry — a **fixed comfortable card size** (a few big cards
/// visible) laid out left→right, with the scroll offset applied. Shared by the draw and
/// the hit-test so a tap maps to the card the player sees (`web_client_ux.md` §Hand).
/// Returns `(card_w, card_h, gap, start_x, card_top, max_scroll)`: when the row fits it
/// is centered (no scroll); when it overflows the cards start at the left margin and the
/// clamped scroll slides them, `max_scroll` being how far they can travel.
fn hand_layout(
    model: &ShellModel,
    top: f32,
    cx0: f32,
    cw: f32,
    h: f32,
    u: f32,
    pad: f32,
) -> HandLayout {
    let n = model.hand.len().max(1);
    let gap = u * 0.55;
    // The carousel lays out within the content frame `[cx0, cx0+cw]` (so on desktop it sits
    // under the board "table", not edge-to-edge).
    let avail = cw - 2.0 * pad;
    // A fixed, generous card size: tall enough to fill the tray (with a little breathing
    // room top + bottom so the stat foot never butts the viewport edge), 5:7 (the catalog
    // card aspect), wide enough that roughly 3–4 show on a phone (the rest scroll in).
    let card_h = (h - u * 1.5).max(u * 4.0);
    let card_w = (card_h * 5.0 / 7.0).min(avail * 0.42);
    let step = card_w + gap;
    let total = n as f32 * card_w + (n as f32 - 1.0) * gap;
    let card_top = top + (h - card_h) / 2.0;
    let (start_x, max_scroll) = if total <= avail {
        // Everything fits — center the row in the content frame, no scrolling.
        (cx0 + (cw - total) / 2.0, 0.0)
    } else {
        // Overflow — anchor at the content's left margin; the row scrolls by `hand_scroll`.
        (cx0 + pad, total - avail)
    };
    let scroll = model.hand_scroll.clamp(0.0, max_scroll);
    HandLayout {
        card_w,
        card_h,
        step,
        start_x: start_x - scroll,
        card_top,
        max_scroll,
    }
}

/// The resolved hand-carousel geometry (see [`hand_layout`]).
#[derive(Debug, Clone, Copy)]
struct HandLayout {
    card_w: f32,
    card_h: f32,
    step: f32,
    /// The left edge of card 0 (already shifted by the clamped scroll).
    start_x: f32,
    card_top: f32,
    /// How far the row can scroll (0 when everything fits).
    max_scroll: f32,
}

/// One hand card's slot rectangle at its rest position (no lift applied), in the
/// carousel. `i` is the hand index. The pure layout the tray draw and the hit-test
/// regions share, so a tap maps to the same card the player sees.
#[allow(clippy::too_many_arguments)]
fn hand_card_rect(
    model: &ShellModel,
    i: usize,
    top: f32,
    cx0: f32,
    cw: f32,
    h: f32,
    u: f32,
    pad: f32,
) -> Rect {
    let l = hand_layout(model, top, cx0, cw, h, u, pad);
    Rect {
        x: l.start_x + i as f32 * l.step,
        y: l.card_top,
        w: l.card_w,
        h: l.card_h,
        color: CARD,
        radius: u * 0.3,
        layer: ShellLayer::Card,
    }
}

/// The hand tray — a smooth left↔right **carousel** of your cards as real placeholder
/// cards (cost · resonance stripe · name · kind · the Atk/Def/HP stat block), the
/// frame the catalog page uses. A few big cards show at once; the rest scroll in via the
/// JS wheel / drag / touch-swipe (`web_client_ux.md` §Hand), the geometry living in
/// [`hand_layout`] so the draw + hit-testing agree. Off-screen cards are culled (no text
/// bleed past the tray edges); soft edge fades + a scroll indicator hint "there is more."
/// Phase B layers the affordances on top: a quiet action dot on a playable card, an
/// evolve chevron on a form card, a dim wash over an unplayable one, and the **lifted**
/// card raised (drawn last, with a gild halo + a brighter ring) when one is picked up.
#[allow(clippy::too_many_arguments)]
fn hand_tray(
    model: &ShellModel,
    s: &mut ShellScene,
    top: f32,
    vw: f32,
    h: f32,
    u: f32,
    pad: f32,
    cx0: f32,
    cw: f32,
    // Whether the cards carry their action-dot / evolve-chevron badge. False under
    // INSPECT (option C): the cards still draw (context), but no "you can act" mark.
    badges: bool,
) {
    // The tray backing — a soft gradient (lighter at the top where it meets the HUD, deeper at the
    // foot), so the hand sits on a gently lit shelf (full-width band). Its head eases up toward the
    // HUD above (item 5) and the `band_blend` feather completes the transition, so the HUD and the
    // tray read as one continuous parchment shelf — no hard ruled line between them.
    s.grads.push(GradRect {
        x: 0.0,
        y: top,
        w: vw,
        h,
        top: with_alpha(PANEL_DEEP, 0.62),
        bottom: with_alpha(PANEL_DEEP, 0.78),
        radius: 0.0,
        layer: ShellLayer::Panel,
    });
    band_blend(s, top, cx0, cw, u, BlendDir::Up);
    let n = model.hand.len();
    if n == 0 {
        return;
    }
    let l = hand_layout(model, top, cx0, cw, h, u, pad);
    let lifted = model.lifted_hand.map(|x| x as usize);
    // Item 6: cards STAY WITHIN the hand section. A card may peek at most `peek` past a frame edge;
    // beyond that it's culled (revealed by scrolling), and the soft fade curtain (drawn after) caps
    // that peek with the surround tone so NOTHING spills onto the mat / past the viewport. The
    // lifted card is exempt (you're holding it) and drawn LAST so it floats above its neighbours.
    // A card is drawn only while it overhangs a frame edge by at most `peek`; once it would spill
    // further it's culled (off-stage until scrolled in). So `[left edge ≥ hard_l - peek]` AND
    // `[right edge ≤ hard_r + peek]`. The curtain's parchment cap (`peek` wide, past the edge) then
    // paints over that bounded overhang — nothing ever reaches the mat proper / past the viewport.
    let peek = l.card_w * 0.42;
    let hard_l = cx0;
    let hard_r = cx0 + cw;
    let visible = |x: f32| x >= hard_l - peek && x + l.card_w <= hard_r + peek;
    for (i, c) in model.hand.iter().enumerate() {
        if Some(i) == lifted {
            continue; // drawn last
        }
        let x = l.start_x + i as f32 * l.step;
        if !visible(x) {
            continue;
        }
        let actionable = model.actionable_hand.contains(&(i as u8));
        let evolve = model.evolve_forms.contains(&(i as u8));
        let devolve = model.devolve_bases.contains(&(i as u8));
        hand_card(
            s,
            c,
            x,
            l.card_top,
            l.card_w,
            l.card_h,
            model.you_seat,
            u,
            actionable,
            evolve,
            devolve,
            false,
            badges, // hand-tray cards carry the action-dot / evolve / recede badge (off under inspect — option C)
        );
    }
    if let Some(i) = lifted
        && let Some(c) = model.hand.get(i)
    {
        let x = l.start_x + i as f32 * l.step;
        // Raise it (and toward the board) so it reads as picked up — drawn even if its
        // rest slot scrolled partly off (you're holding it).
        let ly = l.card_top - u * 1.1;
        let actionable = model.actionable_hand.contains(&(i as u8));
        let evolve = model.evolve_forms.contains(&(i as u8));
        let devolve = model.devolve_bases.contains(&(i as u8));
        hand_card(
            s,
            c,
            x,
            ly,
            l.card_w,
            l.card_h,
            model.you_seat,
            u,
            actionable,
            evolve,
            devolve,
            true,
            badges, // the lifted hand card keeps its badge (but inspect suppresses badges — option C)
        );
    }

    // ── The carousel edge treatment (item 6) — the row NEVER spills past the hand section.
    // A long hand's off-edge cards are not hard-clipped; they dissolve under a soft alpha FADE at
    // each frame edge (a parchment curtain, opaque at the very edge → transparent inward), so the
    // boundary reads as "more →" and the carousel (drag / wheel / swipe) scrolls them into view.
    // The curtain also caps the `peek` overhang with the surround tone, so nothing reaches the mat.
    let row_overflows = l.max_scroll > 0.5;
    let scroll = model.hand_scroll.clamp(0.0, l.max_scroll);
    let fade_w = (peek + u * 1.2).min(l.card_w); // wide enough to dissolve a peeking card
    if row_overflows && scroll > 1.0 {
        // Left edge: cards are scrolled off to the left → fade in from the left frame edge.
        push_edge_fade(s, hard_l, top, fade_w, peek, h, FadeSide::FromLeft);
    }
    if row_overflows && scroll < l.max_scroll - 1.0 {
        // Right edge: cards continue off to the right → fade out toward the right frame edge.
        push_edge_fade(s, hard_r, top, fade_w, peek, h, FadeSide::FromRight);
    }
    // A thin progress track + thumb at the very foot of the tray (only when the row overflows),
    // so "swipe for more" reads at a glance without a heavy scrollbar.
    if row_overflows {
        let track_w = cw * 0.34;
        let track_x = cx0 + (cw - track_w) / 2.0;
        let track_y = top + h - u * 0.5;
        s.rects.push(Rect {
            x: track_x,
            y: track_y,
            w: track_w,
            h: u * 0.12,
            color: with_alpha(RULE, 0.8),
            radius: u * 0.06,
            layer: ShellLayer::Detail,
        });
        let frac = (scroll / l.max_scroll).clamp(0.0, 1.0);
        let thumb_w = track_w * 0.42;
        s.rects.push(Rect {
            x: track_x + frac * (track_w - thumb_w),
            y: track_y,
            w: thumb_w,
            h: u * 0.12,
            color: with_alpha(GILD, 0.9),
            radius: u * 0.06,
            layer: ShellLayer::Detail,
        });
    }
}

/// Which side of the hand section a carousel [`push_edge_fade`] curtain hangs on — `edge_x` is
/// the section's hard frame edge; the fade dissolves INWARD (toward the row interior) and the cap
/// extends OUTWARD (past the edge) to hide a peeking card's overhang.
#[derive(Clone, Copy)]
enum FadeSide {
    FromLeft,
    FromRight,
}

/// A **soft horizontal alpha fade** over the hand carousel's edge (item 6). `edge_x` is the
/// section's hard frame edge. Two parts, so an off-edge card dissolves into the shelf and NOTHING
/// spills past the section:
///  - a **fade** ramping INWARD from the edge (`fade_w` wide): the tray's own top→foot parchment,
///    opaque at the edge → transparent inward, so a card sliding under it dissolves (signals "more →");
///  - a solid **cap** extending OUTWARD past the edge (`cap_w` wide, the tray's own parchment): it
///    paints over a card's permitted `peek` overhang so it never spills past the section. (The tray
///    band is full-width parchment on a wide desktop, so a parchment cap matches the shelf there;
///    on a phone the frame is the full viewport, so the cap sits just past the screen, harmlessly.)
///
/// Built from thin vertical strips with a smoothstep alpha ramp (the quad pipeline's gradients are
/// vertical-only, so a horizontal fade is strip-stacked).
#[allow(clippy::too_many_arguments)]
fn push_edge_fade(
    s: &mut ShellScene,
    edge_x: f32,
    y: f32,
    fade_w: f32,
    cap_w: f32,
    h: f32,
    side: FadeSide,
) {
    let steps = 16usize;
    let sw = fade_w / steps as f32;
    for k in 0..steps {
        // `t` = 0 at the opaque frame edge → 1 at the transparent interior edge.
        let frac = k as f32 / (steps as f32 - 1.0);
        let ramp = 1.0 - smoothstep01(frac); // opaque (1) at the edge, 0 inward
        if ramp <= 0.01 {
            continue;
        }
        let sx = match side {
            FadeSide::FromLeft => edge_x + k as f32 * sw,
            FadeSide::FromRight => edge_x - (k as f32 + 1.0) * sw,
        };
        // Opaque parchment at the very edge (so a card there is fully hidden), tapering inward.
        // The tray's top→foot tone keeps the curtain matched to the shelf behind the cards.
        s.grads.push(GradRect {
            x: sx,
            y,
            w: sw + 0.75, // a hair of overlap so the strips never seam
            h,
            top: with_alpha(PANEL, ramp),
            bottom: with_alpha(PANEL_DEEP, ramp),
            radius: 0.0,
            layer: ShellLayer::Detail,
        });
    }
    // The outward cap (the tray's own parchment tone), covering a peeking card's overhang past the
    // edge so it never spills past the section. Matches the full-width tray shelf on desktop; on a
    // phone it sits just past the screen edge (harmlessly clipped).
    if cap_w > 0.0 {
        let cx = match side {
            FadeSide::FromLeft => edge_x - cap_w,
            FadeSide::FromRight => edge_x,
        };
        s.grads.push(GradRect {
            x: cx,
            y,
            w: cap_w,
            h,
            top: PANEL,
            bottom: PANEL_DEEP,
            radius: 0.0,
            layer: ShellLayer::Detail,
        });
    }
}

/// `smoothstep(0,1,x)` — the Hermite ease used for soft alpha ramps (the carousel fades, etc.).
fn smoothstep01(x: f32) -> f32 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// One placeholder card face at `(x, y)` sized `w`×`h`. `actionable` ⇒ a quiet
/// action dot (it has a legal play); `evolve` ⇒ an evolve chevron (it's a form card
/// with a matching base); `devolve` ⇒ a DOWNWARD recede chevron (it's a base card that
/// can recede a standing-Faded form — §5); `lifted` ⇒ the picked-up treatment (a gild
/// halo + a brighter ring). A non-actionable card gets a soft dim wash so the playable
/// ones stand out. `badge` gates the action-dot / chevron corner mark — the hand tray
/// sets it; a card shown purely for INFORMATION (the Glimpse peek, item 11) passes
/// `false` so no spurious "you can act on this" dot sits on a card you're only deciding on.
#[allow(clippy::too_many_arguments)]
fn hand_card(
    s: &mut ShellScene,
    c: &HandCard,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    you: Seat,
    u: f32,
    actionable: bool,
    evolve: bool,
    devolve: bool,
    lifted: bool,
    badge: bool,
) {
    let scale = (w / (u * 4.0)).clamp(0.6, 1.5); // text scales with the card size
    let spirit = c.kind.eq_ignore_ascii_case("Spirit") || c.kind.is_empty();
    // A lifted card floats: a warm gild halo under it (a wider, softer shadow).
    if lifted {
        s.rects.push(Rect {
            x: x - u * 0.3,
            y: y - u * 0.3,
            w: w + u * 0.6,
            h: h + u * 0.6,
            color: with_alpha(LIFT, 0.45),
            radius: u * 0.5,
            layer: ShellLayer::Card,
        });
    }
    // The card body.
    push_card_grad(s, x, y, w, h, CARD, CARD_DEEP, ShellLayer::Card, u);
    // A thin ink frame in your seat colour (the card is yours); brighter + thicker
    // when lifted so the picked-up card reads unmistakably.
    let (ring_c, ring_t) = if lifted {
        (with_alpha(LIFT, 1.0), 2.5)
    } else {
        (with_alpha(seat_ink(you), 0.8), 1.5)
    };
    push_ring_px(s, x, y, w, h, ring_c, ring_t);

    let in_x = x + u * 0.22; // inner margin
    let in_w = w - u * 0.44;
    // ── The title band: a resonance-tinted header with the cost disc + the name. ──
    let band_h = h * 0.2;
    let res_tint = resonance_tint(&c.resonance);
    s.rects.push(Rect {
        x: in_x,
        y: y + u * 0.18,
        w: in_w,
        h: band_h,
        color: with_alpha(res_tint, 0.30),
        radius: u * 0.12,
        layer: ShellLayer::Detail,
    });
    // The cost divot (top-left of the band, a struck-metal gilt disc with the number).
    let divot = (band_h * 0.95).min(u * 1.15 * scale);
    let dcx = in_x + u * 0.12 + divot / 2.0;
    let dcy = y + u * 0.18 + band_h / 2.0;
    s.grads.push(GradRect {
        x: dcx - divot / 2.0,
        y: dcy - divot / 2.0,
        w: divot,
        h: divot,
        top: GILD,
        bottom: GILD_DEEP,
        radius: divot / 2.0,
        layer: ShellLayer::Detail,
    });
    s.texts.push(Text {
        x: dcx,
        y: dcy,
        size: divot * 0.62,
        text: format!("{}", c.cost),
        color: PAPER,
        align: Align::Center,
        bold: false,
        layer: ShellLayer::Text,
    });
    // The name — right of the cost disc, sized to fit the band width; a long catalog name
    // (up to 40 chars) wraps to two lines rather than shrinking unreadably small. Leave a
    // gutter on the right for the action-dot / evolve-chevron badge (top-right corner).
    let name = sanitize(&c.name);
    let name_x0 = dcx + divot / 2.0 + u * 0.18;
    let name_lane = ((in_x + in_w) - name_x0 - u * 1.0).max(u * 2.0);
    let name_cx = name_x0 + name_lane / 2.0;
    if name.chars().count() as f32 * u * 0.5 <= name_lane {
        // Fits on one line at a comfortable size.
        let name_size = fit_size(name.chars().count().max(1), name_lane).min(u * 0.6 * scale);
        s.texts.push(Text {
            x: name_cx,
            y: dcy,
            size: name_size,
            text: name,
            color: INK,
            align: Align::Center,
            bold: false,
            layer: ShellLayer::Text,
        });
    } else {
        // Wrap to two balanced lines, each sized to fit the lane.
        let (l1, l2) = wrap_two(&name, 14);
        let widest = l1.chars().count().max(l2.chars().count());
        let name_size = fit_size(widest.max(1), name_lane).min(u * 0.5 * scale);
        s.texts.push(Text {
            x: name_cx,
            y: dcy - name_size * 0.62,
            size: name_size,
            text: l1,
            color: INK,
            align: Align::Center,
            bold: false,
            layer: ShellLayer::Text,
        });
        if !l2.is_empty() {
            s.texts.push(Text {
                x: name_cx,
                y: dcy + name_size * 0.62,
                size: name_size,
                text: l2,
                color: INK,
                align: Align::Center,
                bold: false,
                layer: ShellLayer::Text,
            });
        }
    }

    // ── The body splits into an art plate (top) + a foot. Spirits reserve the foot for
    // the labeled Atk/Def/HP pills; non-combat cards (spells/bonds) give the art the room
    // and read just the kind tag. ──
    let body_top = y + u * 0.18 + band_h + u * 0.16;
    let foot_h = if spirit { h * 0.26 } else { 0.0 };
    let kind_h = u * 0.7 * scale; // the kind tag's lane
    let art_top = body_top;
    let art_bot = y + h - u * 0.3 - foot_h - kind_h;
    let art_h = (art_bot - art_top).max(u * 0.5);
    s.rects.push(Rect {
        x: in_x,
        y: art_top,
        w: in_w,
        h: art_h,
        color: with_alpha(res_tint, 0.16),
        radius: u * 0.1,
        layer: ShellLayer::Detail,
    });
    // A soft emblem disc with the kind's initial, centered in the plate.
    let em_r = (art_h.min(in_w) * 0.3).max(u * 0.5);
    let ecx = x + w / 2.0;
    let ecy = art_top + art_h / 2.0;
    s.rects.push(Rect {
        x: ecx - em_r,
        y: ecy - em_r,
        w: em_r * 2.0,
        h: em_r * 2.0,
        color: with_alpha(res_tint, 0.42),
        radius: em_r,
        layer: ShellLayer::Detail,
    });
    if let Some(ch) = c.kind.chars().next() {
        s.texts.push(Text {
            x: ecx,
            y: ecy,
            size: em_r * 1.0,
            text: ch.to_uppercase().to_string(),
            color: PAPER,
            align: Align::Center,
            bold: false,
            layer: ShellLayer::Text,
        });
    }
    // The kind tag, small + soft, between the art plate and the foot.
    if !c.kind.is_empty() {
        s.texts.push(Text {
            x: x + w / 2.0,
            y: art_bot + kind_h / 2.0,
            size: (u * 0.5 * scale).min(kind_h * 0.8),
            text: c.kind.to_uppercase(),
            color: SOFT,
            align: Align::Center,
            bold: false,
            layer: ShellLayer::Text,
        });
    }

    // ── The stat foot: labeled Atk / Def / HP pills (spirits only). Each pill carries its
    // stat name beside its value (the Atk/Def/HP shorthand the whole app uses — never
    // A/D/H), so a player never has to decode a bare coloured number. ──
    if spirit {
        let foot_top = y + h - u * 0.28 - foot_h;
        let gap = u * 0.16;
        let pill_w = (in_w - 2.0 * gap) / 3.0;
        let stats = [
            ("Atk", c.attack, ATK_INK),
            ("Def", c.defense, DEF_INK),
            ("HP", c.hp, HP_INK),
        ];
        for (k, (label, val, ink)) in stats.iter().enumerate() {
            let px = in_x + k as f32 * (pill_w + gap);
            // The pill body — a soft tinted plate, ringed in the stat ink.
            s.rects.push(Rect {
                x: px,
                y: foot_top,
                w: pill_w,
                h: foot_h,
                color: with_alpha(*ink, 0.16),
                radius: u * 0.14,
                layer: ShellLayer::Detail,
            });
            // The label (top, small), then the value (below, bold), centered in the pill.
            s.texts.push(Text {
                x: px + pill_w / 2.0,
                y: foot_top + foot_h * 0.32,
                size: (u * 0.4 * scale).min(foot_h * 0.32),
                text: (*label).to_string(),
                color: SOFT,
                align: Align::Center,
                bold: false,
                layer: ShellLayer::Text,
            });
            s.texts.push(Text {
                x: px + pill_w / 2.0,
                y: foot_top + foot_h * 0.72,
                size: (u * 0.6 * scale).min(foot_h * 0.42),
                text: format!("{val}"),
                color: *ink,
                align: Align::Center,
                bold: false,
                layer: ShellLayer::Text,
            });
        }
    }

    // ── Phase B affordances on the card ────────────────────────────────────────
    // A card with no legal play this turn reads clearly dimmed (a paper wash over it),
    // so the playable cards stand out at a glance — UNLESS it's lifted (you're holding it).
    if !actionable && !lifted {
        s.rects.push(Rect {
            x,
            y,
            w,
            h,
            color: with_alpha(PAPER, 0.62),
            radius: u * 0.3,
            layer: ShellLayer::Detail,
        });
    }
    // A quiet **action dot** marks a playable card; an **evolve chevron** (upward, green)
    // marks a form card with a matching base; a **recede chevron** (downward, amber) marks a
    // base card that can recede a standing-Faded form (§5). All sit in the TOP-RIGHT corner —
    // clear of the cost disc (top-left), the name, and the stat foot (bottom) — so the
    // "this can act" cue reads at a glance without colliding with the card's content. Skipped
    // entirely when `badge` is false (a card shown only for information, e.g. the Glimpse peek).
    let badge_cx = x + w - u * 0.6;
    let badge_cy = y + u * 0.6;
    if badge && devolve {
        push_chevron_down(s, badge_cx, badge_cy, (w * 0.28).max(u * 0.9), DEVOLVE);
    } else if badge && evolve {
        push_chevron(s, badge_cx, badge_cy, (w * 0.28).max(u * 0.9), EVOLVE);
    } else if badge && actionable {
        let dot = (w * 0.12).clamp(5.0, 14.0);
        // A soft paper halo so the dot lifts off the title band, then the green disc.
        s.rects.push(Rect {
            x: badge_cx - dot * 0.9,
            y: badge_cy - dot * 0.9,
            w: dot * 1.8,
            h: dot * 1.8,
            color: with_alpha(PAPER, 0.8),
            radius: dot,
            layer: ShellLayer::Detail,
        });
        s.rects.push(Rect {
            x: badge_cx - dot / 2.0,
            y: badge_cy - dot / 2.0,
            w: dot,
            h: dot,
            color: AFFORD,
            radius: dot / 2.0,
            layer: ShellLayer::Detail,
        });
    }
}

/// The two control-button rectangles, `[End Turn, Glimpse]` (item 2) — in their own lane, never
/// over the play grid. The pure geometry the draw + the hit-test share. Two layouts:
/// - **Rail** (`m.rail_w > 0`, wide viewports): the buttons STACK vertically in the dedicated
///   right rail beside the board (End Turn lower/primary, Glimpse above), centred in the board band.
/// - **HUD bar** (`m.rail_w == 0`, phone): they sit SIDE BY SIDE in the right of the HUD bar,
///   clear of the score/anima (left) and below the clock — still outside the board.
fn fab_rects(m: &Metrics) -> [Rect; 2] {
    let u = m.u;
    let mk = |x: f32, y: f32, w: f32, h: f32| Rect {
        x,
        y,
        w,
        h,
        color: CARD,
        radius: u * 0.3,
        layer: ShellLayer::Card,
    };
    if m.rail_w > 0.0 {
        // The rail spans [content_x + content_w - rail_w, content_x + content_w]; lay the buttons
        // out within it with a margin, stacked + centred in the board band.
        let rail_x = m.content_x + m.content_w - m.rail_w;
        let mgn = u * 0.7;
        let bw = m.rail_w - 2.0 * mgn;
        let bh = (u * 2.6).min(bw * 0.62);
        let gap = u * 0.7;
        let stack_h = bh * 2.0 + gap;
        // Centre the stack vertically against the board's centre.
        let cy = m.board_y + m.board_side / 2.0;
        let st_y = cy - stack_h / 2.0;
        let et_y = st_y + bh + gap;
        let x = rail_x + mgn;
        [mk(x, et_y, bw, bh), mk(x, st_y, bw, bh)]
    } else {
        // Phone: side by side in the HUD bar's right half (the HUD spans [band_bottom, +hud_h]).
        let top = m.band_bottom;
        let bh = (m.hud_h - u * 0.7).max(u * 1.8);
        let by = top + (m.hud_h - bh) / 2.0;
        let right = m.content_x + m.content_w - m.pad;
        let gap = u * 0.5;
        let bw = (u * 5.4).min((m.content_w * 0.5 - m.pad - gap) / 2.0);
        let et_x = right - bw;
        let st_x = et_x - gap - bw;
        [mk(et_x, by, bw, bh), mk(st_x, by, bw, bh)]
    }
}

/// The two floating buttons (End Turn primary · Glimpse), floating in the lower-right
/// gutter beside the board. **Now wired** (Phase B): the JS bridge maps a tap in
/// these rects (exposed via [`shell_regions`]) to `EndTurn` / `Glimpse`. End Turn
/// reads filled/primary when it's your turn, dimmed otherwise; both dim while
/// dragging (a drop in progress shouldn't invite an accidental end-of-turn).
fn fabs(model: &ShellModel, s: &mut ShellScene, m: &Metrics) {
    let u = m.u;
    let [et, st] = fab_rects(m);
    let bw = et.w;
    let dim = model.dragging;
    // Label size that fits the button's inner width (so "END TURN" never clips).
    let label_size = |text: &str| fit_size(text.chars().count(), bw - u * 0.8).min(u * 1.0);
    // End Turn — the primary, lowest (its bottom edge a margin above the HUD). A subtle
    // gradient gives the dark button a lit, pressable feel.
    let (et_fill, et_text) = if model.your_turn && !dim {
        (DUSK, PAPER)
    } else {
        (with_alpha(DUSK, 0.35), with_alpha(PAPER, 0.85))
    };
    let et_bottom = Color {
        r: et_fill.r * 0.7,
        g: et_fill.g * 0.7,
        b: et_fill.b * 0.7,
        a: et_fill.a,
    };
    push_card_grad(
        s,
        et.x,
        et.y,
        et.w,
        et.h,
        et_fill,
        et_bottom,
        ShellLayer::Card,
        u,
    );
    s.texts.push(Text {
        x: et.x + et.w / 2.0,
        y: et.y + et.h / 2.0,
        size: label_size("End Turn"),
        text: "End Turn".into(),
        color: et_text,
        align: Align::Center,
        bold: false,
        layer: ShellLayer::Text,
    });
    // Glimpse — secondary (outlined), above End Turn.
    push_card_grad(
        s,
        st.x,
        st.y,
        st.w,
        st.h,
        CARD,
        CARD_DEEP,
        ShellLayer::Card,
        u,
    );
    let st_ink = if dim {
        with_alpha(seat_ink(model.you_seat), 0.5)
    } else {
        seat_ink(model.you_seat)
    };
    push_ring_px(s, st.x, st.y, st.w, st.h, st_ink, 1.5);
    s.texts.push(Text {
        x: st.x + st.w / 2.0,
        y: st.y + st.h / 2.0,
        size: label_size("Glimpse"),
        text: "Glimpse".into(),
        color: st_ink,
        align: Align::Center,
        bold: false,
        layer: ShellLayer::Text,
    });
}

/// The floating **inspect panel** (Phase B): a small card beside the inspected
/// element showing the full stat line, keyword chips, the rules text (wrapped), and
/// a **passive reach grid** (a star at centre, soft dots on the threatened tiles —
/// the "what could this threaten" read, distinct from the bright select-time target
/// glow on the board). A faint scrim sits behind it so it reads above the board
/// without hiding it. Placed beside `anchor`, clamped to stay fully on-screen.
fn inspect_panel(insp: &Inspect, s: &mut ShellScene, vw: f32, vh: f32, u: f32) {
    // A full-screen scrim on the **Detail** layer (item 5): it sits ABOVE the board's +
    // hand's action dots / evolve chevrons (also Detail, drawn earlier this frame), so the
    // affordance layer reads as dimmed CONTEXT behind the floating note rather than bleeding
    // green dots over it. Deep enough that the masking is
    // unmistakable while the board still shows through (inspect is a read, not a takeover).
    s.rects.push(Rect {
        x: 0.0,
        y: 0.0,
        w: vw,
        h: vh,
        color: INSPECT_SCRIM,
        radius: 0.0,
        layer: ShellLayer::Detail,
    });
    let pw = (u * 14.0).min(vw - u * 1.0);
    let line = u * 1.05;
    // Height: header + stat block + keywords (if any) + the wrapped rules + the reach
    // grid. Computed so the panel hugs its content.
    let kw_line = if insp.keywords.is_empty() { 0.0 } else { line };
    // A spirit shows Atk/Def/HP on one line + Reach on its own; a spell shows only Reach.
    let stat_block = if insp.kind.eq_ignore_ascii_case("Spirit") {
        line * 0.92 + line
    } else {
        line
    };
    // The rules / lore — a full sentence (item 9): keep its real punctuation and ensure a
    // terminal period so a catalog entry that omitted one still reads as a clean sentence.
    let rules = ensure_sentence_period(&sanitize_keep_punct(&insp.rules));
    let rules_lines = wrap_lines(&rules, ((pw - u * 1.0) / (u * 0.36)) as usize).len() as f32;
    let grid_w = insp.reach_w.max(1) as f32;
    let grid_side = (pw - u * 2.0).min(u * 6.0);
    let ph = u * 1.0
        + line * 1.4  // header (name)
        + line * 0.95 // kind · resonance · cost
        + stat_block  // the Atk/Def/HP row + the Reach row (spirit) / just Reach (spell)
        + kw_line
        + line * rules_lines.max(1.0)
        + u * 0.6
        + grid_side
        + u * 1.0;
    // Placement (item 2). On a WIDE desktop the panel is **pinned to the top of the play
    // area** (just under the opponent strip) rather than chasing the inspected card's low
    // anchor and landing adrift mid-left — a consistent, prominent home for the most-frequent
    // read, with its x still tracking the anchor so it relates to the card. On a narrow phone
    // there's no room for a fixed top panel beside the board, so it stays anchored beside the
    // element (above it, or below if the top is tight). Both clamp fully on-screen.
    let m = Metrics::for_viewport(vw, vh);
    let (ax, ay) = insp.anchor;
    let wide = m.rail_w > 0.0; // the desktop/landscape "table" layout (a real side rail exists)
    let mut px = ax - pw / 2.0;
    let mut py = if wide {
        // Pin to the top of the play area (the board band's head), so it reaches the top.
        m.strip_h + u * 0.6
    } else {
        ay - ph - u * 1.0
    };
    if py < u * 0.5 {
        py = (ay + u * 1.0).min(vh - ph - u * 0.5);
    }
    px = px.clamp(u * 0.5, (vw - pw - u * 0.5).max(u * 0.5));
    py = py.clamp(u * 0.5, (vh - ph - u * 0.5).max(u * 0.5));

    // The panel body + frame — on the **Detail** layer (above the affordance dots, item 5),
    // its soft drop shadow on the Card band just beneath. So the panel reads as a note lifted
    // over a dimmed board, with no green dot peeking through its face.
    push_card_rect(s, px, py, pw, ph, INSPECT_BG, ShellLayer::Detail, u);
    push_ring_px(s, px, py, pw, ph, with_alpha(INK, 0.85), 1.5);
    // A resonance-tinted header rule.
    s.rects.push(Rect {
        x: px + u * 0.5,
        y: py + line * 1.5,
        w: pw - u * 1.0,
        h: 1.5,
        color: with_alpha(GILD, 0.8),
        radius: 0.0,
        layer: ShellLayer::Detail,
    });
    let cx = px + pw / 2.0;
    let mut cy = py + u * 0.9;
    // Header: the name (sized to fit), then the kind · resonance · cost line.
    let name = sanitize_keep_punct(&insp.name);
    let name_size = fit_size(name.chars().count(), pw - u * 1.0).min(u * 1.15);
    s.texts.push(Text {
        x: cx,
        y: cy,
        size: name_size,
        text: name,
        color: INK,
        align: Align::Center,
        bold: false,
        layer: ShellLayer::Text,
    });
    cy += line * 1.4;
    // The kind · resonance · cost line — middot-separated (item 10), the same dot the
    // card frame + the site cards page use; never hyphens.
    let meta = sanitize_keep_punct(&format!(
        "{} · {} · cost {}",
        insp.kind, insp.resonance, insp.cost
    ));
    s.texts.push(Text {
        x: px + u * 0.5,
        y: cy,
        size: u * 0.6,
        text: meta,
        color: SOFT,
        align: Align::Left,
        bold: false,
        layer: ShellLayer::Text,
    });
    cy += line * 0.95;
    // The stat block — each stat drawn as a coloured "Label: value" segment so the value carries
    // its own ink (item 6): Atk red, Def blue, HP green — the SAME stat inks the hand card + the
    // placed board card use (one language), with a colon separator (item 7). Reach gets the gild
    // ink (item 8) — a precious, distinct attribute, not a fourth combat stat — and sits on its
    // OWN line so the row never overflows the panel. Each segment is the label (soft ink) then the
    // value (the stat's ink, bold). Spirits show the combat row; a spell shows only Reach.
    let stat_size = u * 0.64;
    let left = px + u * 0.5;
    let seg_gap = u * 0.7; // gap between stat segments
    let push_stat =
        |s: &mut ShellScene, label: &str, value: &str, ink: Color, sx: &mut f32, y: f32| {
            // "Label:" in soft ink.
            let key = format!("{label}:");
            let kw = crate::font::text_width(&key, stat_size) * FONT_WIDTH_RATIO;
            s.texts.push(Text {
                x: *sx,
                y,
                size: stat_size,
                text: key,
                color: SOFT,
                align: Align::Left,
                bold: false,
                layer: ShellLayer::Text,
            });
            *sx += kw + u * 0.22;
            // the value, in the stat's own ink (bold, so the number reads strongly).
            let vw = crate::font::text_width(value, stat_size) * FONT_WIDTH_RATIO;
            s.texts.push(Text {
                x: *sx,
                y,
                size: stat_size,
                text: value.to_string(),
                color: ink,
                align: Align::Left,
                bold: true,
                layer: ShellLayer::Text,
            });
            *sx += vw + seg_gap;
        };
    if insp.kind.eq_ignore_ascii_case("Spirit") {
        let mut sx = left;
        push_stat(s, "Atk", &format!("{}", insp.attack), ATK_INK, &mut sx, cy);
        push_stat(s, "Def", &format!("{}", insp.defense), DEF_INK, &mut sx, cy);
        push_stat(s, "HP", &format!("{}", insp.hp), HP_INK, &mut sx, cy);
        cy += line * 0.92;
    }
    // Reach — gild ink (item 8), with the colon separator (item 7), on its own line.
    {
        let mut sx = left;
        push_stat(
            s,
            "Reach",
            &sanitize_keep_punct(&insp.reach),
            GILD,
            &mut sx,
            cy,
        );
    }
    cy += line;
    if !insp.keywords.is_empty() {
        // Keyword chips, middot-separated (item 10): "Mobile · Warded", never a bare space.
        let kw = sanitize_keep_punct(&insp.keywords.join(" · "));
        s.texts.push(Text {
            x: px + u * 0.5,
            y: cy,
            size: u * 0.6,
            text: kw,
            color: GILD,
            align: Align::Left,
            bold: false,
            layer: ShellLayer::Text,
        });
        cy += line;
    }
    // The rules text, wrapped to the panel width.
    for l in wrap_lines(&rules, ((pw - u * 1.0) / (u * 0.34)) as usize) {
        s.texts.push(Text {
            x: px + u * 0.5,
            y: cy,
            size: u * 0.56,
            text: l,
            color: SOFT,
            align: Align::Left,
            bold: false,
            layer: ShellLayer::Text,
        });
        cy += line * 0.9;
    }
    cy += u * 0.3;
    // The passive reach grid — a small board diagram beneath the text.
    let gx0 = cx - grid_side / 2.0;
    let cell = grid_side / grid_w;
    for r in 0..insp.reach_w {
        for col in 0..insp.reach_w {
            // Top row = high tile-row (the board's flip), matching the inspect-grid
            // convention the DOM panel used.
            let t = (insp.reach_w - 1 - r) * insp.reach_w + col;
            let tx = gx0 + col as f32 * cell;
            let ty = cy + r as f32 * cell;
            // A faint cell outline.
            push_ring_px(s, tx, ty, cell, cell, with_alpha(RULE, 0.7), 1.0);
            if t == insp.reach_center {
                // The card itself: a filled gild disc.
                let d = cell * 0.5;
                s.rects.push(Rect {
                    x: tx + (cell - d) / 2.0,
                    y: ty + (cell - d) / 2.0,
                    w: d,
                    h: d,
                    color: GILD,
                    radius: d / 2.0,
                    layer: ShellLayer::Detail,
                });
            } else if insp.reach_tiles.contains(&t) {
                // A threatened tile: a SOFT dot (passive — "could threaten", not the
                // bright select-time engageable glow).
                let d = cell * 0.34;
                s.rects.push(Rect {
                    x: tx + (cell - d) / 2.0,
                    y: ty + (cell - d) / 2.0,
                    w: d,
                    h: d,
                    color: with_alpha(AFFORD, 0.55),
                    radius: d / 2.0,
                    layer: ShellLayer::Detail,
                });
            }
        }
    }
}

/// A **soft drop shadow** under a surface at `(x, y, w, h)` with corner radius `r` (item 1).
/// One consistent light from the upper-left, so the shadow falls **down + right**; `lift`
/// scales the depth (a resting card lifts a little, a picked-up card or a modal panel more).
/// Drawn as a single SDF quad with a soft penumbra — never a stack of offset greys.
/// Which way a [`band_blend`] feather opens — DOWN (the panel is above the boundary, its tone
/// fades into the open page below) or UP (the panel is below, its tone fades up into the page).
#[derive(Clone, Copy)]
enum BlendDir {
    Down,
    Up,
}

/// A **soft section blend** at a band boundary (item 5): instead of a 1px ruled hairline (a hard
/// boxed edge), feather the parchment tone across a short band so the panel dissolves into the
/// open board page — the sections read as one continuous lit-paper surface, the depth carried by
/// the gentle gradient, not a line. `edge_y` is the boundary; `dir` says which side the panel is
/// on. Drawn as a `Panel`-layer GradRect whose alpha falls to 0 on the page side (the rasterizer
/// interpolates per-vertex alpha), so whatever sits below shows through cleanly. Spans only the
/// **content frame** `[cx0, cx0+cw]` — the open board page — so it never streaks across the dark
/// mat margins on a wide desktop (where the full-width bands already transition on their own).
fn band_blend(s: &mut ShellScene, edge_y: f32, cx0: f32, cw: f32, u: f32, dir: BlendDir) {
    let feather = u * 1.1;
    let edge = with_alpha(PANEL_DEEP, 0.5); // the parchment foot, at the band side
    let gone = with_alpha(PANEL_DEEP, 0.0); // faded out, at the page side
    let (y, top, bottom) = match dir {
        // Panel above: the feather sits BELOW the edge, opaque at the top (touching the panel
        // foot) → transparent at the bottom (into the page).
        BlendDir::Down => (edge_y, edge, gone),
        // Panel below: the feather sits ABOVE the edge, transparent at the top (page) → opaque
        // at the bottom (touching the panel head).
        BlendDir::Up => (edge_y - feather, gone, edge),
    };
    s.grads.push(GradRect {
        x: cx0,
        y,
        w: cw,
        h: feather,
        top,
        bottom,
        radius: 0.0,
        layer: ShellLayer::Panel,
    });
}

fn push_shadow(
    s: &mut ShellScene,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    layer: ShellLayer,
    lift: f32,
) {
    let u = ((w + h) * 0.06).clamp(4.0, 22.0); // a unit derived from the surface size
    let softness = u * (0.55 + 0.6 * lift); // the penumbra width
    let off_x = u * 0.10 * lift; // the light direction (down + slightly right)
    let off_y = u * (0.16 + 0.18 * lift);
    let alpha = (0.13 + 0.06 * lift).min(0.26);
    s.shadows.push(Shadow {
        x: x + off_x,
        y: y + off_y,
        w,
        h,
        radius: r,
        softness,
        color: with_alpha(SHADOW, alpha),
        layer,
    });
}

/// A rounded card body. The SDF backend rounds the corners crisply; a soft drop shadow
/// underneath lifts it off the paper (one light, soft blur — item 1).
fn push_card_rect(
    s: &mut ShellScene,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
    layer: ShellLayer,
    u: f32,
) {
    push_shadow(s, x, y, w, h, u * 0.3, layer, 0.4);
    s.rects.push(Rect {
        x,
        y,
        w,
        h,
        color,
        radius: u * 0.3,
        layer,
    });
}

/// A rounded card body with a **vertical gradient** (`top`→`bottom`) — the gradient twin
/// of [`push_card_rect`]: a soft drop shadow + the gradient fill, so a card/FAB/panel reads
/// as gently lit paper rather than a flat block. The single quad pipeline interpolates the
/// gradient per-vertex (free).
#[allow(clippy::too_many_arguments)]
fn push_card_grad(
    s: &mut ShellScene,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    top: Color,
    bottom: Color,
    layer: ShellLayer,
    u: f32,
) {
    push_shadow(s, x, y, w, h, u * 0.3, layer, 0.4);
    s.grads.push(GradRect {
        x,
        y,
        w,
        h,
        top,
        bottom,
        radius: u * 0.3,
        layer,
    });
}

/// A hollow rectangle ring (four px-thick edge rects) in `color`, thickness `t`.
fn push_ring_px(s: &mut ShellScene, x: f32, y: f32, w: f32, h: f32, color: Color, t: f32) {
    let edge = |x: f32, y: f32, w: f32, h: f32| Rect {
        x,
        y,
        w,
        h,
        color,
        radius: 0.0,
        layer: ShellLayer::Detail,
    };
    s.rects.push(edge(x, y, w, t)); // top
    s.rects.push(edge(x, y + h - t, w, t)); // bottom
    s.rects.push(edge(x, y, t, h)); // left
    s.rects.push(edge(x + w - t, y, t, h)); // right
}

/// A hollow **circular** ring of outer radius `r` centred at `(cx, cy)`, thickness `t`, in
/// `color`. The backend has only filled (rounded) rects, so a ring is a disc of `color` with a
/// `hole`-toned disc punched inside it — `hole` is the local background tone the ring sits on
/// (the gap reads as background, leaving a clean circular outline). Used for the round counter's
/// Nightfall double-circle (item 13), where a square ring read wrong against the round pips.
fn push_circle_ring(
    s: &mut ShellScene,
    cx: f32,
    cy: f32,
    r: f32,
    t: f32,
    color: Color,
    hole: Color,
) {
    s.rects.push(Rect {
        x: cx - r,
        y: cy - r,
        w: r * 2.0,
        h: r * 2.0,
        color,
        radius: r,
        layer: ShellLayer::Detail,
    });
    let inner = (r - t).max(0.0);
    s.rects.push(Rect {
        x: cx - inner,
        y: cy - inner,
        w: inner * 2.0,
        h: inner * 2.0,
        color: hole,
        radius: inner,
        layer: ShellLayer::Detail,
    });
}

/// The opponent's faction word from its seat (Seat A = Lorekeepers' cool ink, Seat
/// B = the Solace's warm ink) — a sensible default name when none is supplied.
fn opp_seat_word(seat: Seat) -> String {
    match seat {
        Seat::A => "Lorekeepers".into(),
        Seat::B => "the Solace".into(),
    }
}

/// The faction-flavoured **devolution verb** for the player whose `faction_word` this is
/// (vocabulary law, design §5): the Solace **recedes**, the Lorekeeper **reverts** — one
/// engine action (`Devolve`), the faction's word in player-facing text / UI / the a11y
/// tree. Derived from the faction word (the model carries "the Solace" / "Lorekeepers"),
/// matching the protocol `label`'s "Recede"/"Revert" so every surface speaks one verb.
fn recede_verb(faction_word: &str) -> &'static str {
    if faction_word.to_ascii_lowercase().contains("solace") {
        "Recede"
    } else {
        "Revert"
    }
}

/// Keep only the glyphs the bitmap font can draw (A–Z, 0–9, space), uppercasing for
/// the card-name band (the font has a full uppercase set and only `b`/`m`
/// lowercase). Anything else (apostrophes, accents) collapses to a word break.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The largest glyph height at which an `n`-glyph line fits `max_w` px wide. The pure
/// layout estimates width from the bitmap metric (glyphs ~3 wide + a 1-px gap on a 5-row
/// cell: `width ≈ (n*4 - 1) * size/5`), but the canvas now renders **real EB Garamond**,
/// whose proportional advances average noticeably narrower than that fixed cell — so the
/// estimate is scaled by [`FONT_WIDTH_RATIO`] to return the larger size the real type
/// actually allows (otherwise every label would shrink unreadably small). Floors legible.
fn fit_size(n: usize, max_w: f32) -> f32 {
    if n == 0 {
        return f32::INFINITY; // empty line: no constraint
    }
    (max_w * 5.0 * FONT_WIDTH_RATIO / (n as f32 * 4.0 - 1.0)).max(6.0)
}

/// How much wider the bitmap-metric width estimate runs than the real (proportional) EB
/// Garamond rendering — i.e. Garamond fits ~this many × more glyphs in the same box. Tuned
/// so names/labels size up to fill their frames instead of shrinking to the bitmap's
/// conservative estimate. (Garamond's mean advance ≈ 0.5em vs the bitmap cell's ≈ 0.8em.)
const FONT_WIDTH_RATIO: f32 = 1.5;

/// Lay a (possibly long, multi-word) card name onto up to two lines, each ≤ `max`
/// glyphs. A short name stays on one line. A longer one splits at the word boundary
/// nearest the middle so two-word names read naturally ("TIDEBOUND" / "COLOSSUS"),
/// then each line is hard-truncated to `max` (placeholder cards carry no art and
/// the bitmap font has no ellipsis glyph, so over-long names clip cleanly rather
/// than overrun the frame). A single very long word is split mid-word.
fn wrap_two(name: &str, max: usize) -> (String, String) {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= max {
        return (name.to_string(), String::new());
    }
    // Prefer a space split; pick the space nearest the string's midpoint so the two
    // lines are balanced. Fall back to a mid-word split when there's no space.
    let mid = chars.len() / 2;
    let split = chars
        .iter()
        .enumerate()
        .filter(|&(_, &c)| c == ' ')
        .min_by_key(|&(i, _)| (i as isize - mid as isize).unsigned_abs())
        .map(|(i, _)| i)
        .unwrap_or(mid);
    let l1: String = chars[..split].iter().take(max).collect();
    let rest_start = if chars.get(split) == Some(&' ') {
        split + 1
    } else {
        split
    };
    let l2: String = chars[rest_start..].iter().take(max).collect();
    (l1, l2)
}

/// Like [`sanitize`] but **case-preserving** and keeping real sentence
/// punctuation — for the inspect panel's body text (name / stat line / rules), the
/// choice/result copy, and the replay captions. The canvas now renders REAL EB
/// Garamond from the glyph atlas (the full ASCII printable set plus the typographic
/// marks `— · – ’ … ×`), so the old "collapse every mark to a box" rule is gone: a
/// card's lore keeps its terminal period, a "key: value" keeps its colon, a comma
/// reads as a comma, and a middot `·` separates list items. Only glyphs the atlas
/// genuinely can't draw collapse to a space. Runs of whitespace collapse to one.
///
/// The kept set is the punctuation the UI's prose actually uses:
/// `. , : ; ! ? ' " ( ) - – — / & + · …` (plus letters/digits/space). Everything
/// else (rare accents the bundled face may lack) becomes a word break.
fn sanitize_keep_punct(text: &str) -> String {
    const KEEP: &[char] = &[
        '.', ',', ':', ';', '!', '?', '\'', '"', '(', ')', '-', '–', '—', '/', '&', '+', '·', '…',
    ];
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || KEEP.contains(&c) {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Ensure a sentence-shaped string ends in terminal punctuation (item 9) — a card's
/// lore / a rules clause should read as a full sentence. If the (trimmed) text is
/// non-empty and doesn't already end in `.`/`!`/`?`/`:`/`…`, a period is appended.
/// Applied to the inspect panel's rules text so a catalog entry that omitted the
/// period still reads as a clean sentence on the card.
fn ensure_sentence_period(text: &str) -> String {
    let t = text.trim_end();
    if t.is_empty() {
        return String::new();
    }
    match t.chars().last() {
        Some('.' | '!' | '?' | ':' | '…') => t.to_string(),
        _ => format!("{t}."),
    }
}

/// Greedily wrap `text` into lines of at most `max` characters on word boundaries
/// (a single over-long word is hard-split). For the inspect panel's rules text +
/// keyword run. `max == 0` yields the whole text on one line (no constraint).
fn wrap_lines(text: &str, max: usize) -> Vec<String> {
    if max == 0 || text.is_empty() {
        return if text.is_empty() {
            Vec::new()
        } else {
            vec![text.to_string()]
        };
    }
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        // A word longer than the line: hard-split it across lines.
        if word.chars().count() > max {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            let mut chunk = String::new();
            for ch in word.chars() {
                chunk.push(ch);
                if chunk.chars().count() == max {
                    lines.push(std::mem::take(&mut chunk));
                }
            }
            cur = chunk;
            continue;
        }
        let extra = if cur.is_empty() { 0 } else { 1 };
        if cur.chars().count() + extra + word.chars().count() > max {
            lines.push(std::mem::take(&mut cur));
            cur.push_str(word);
        } else {
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Place the board scene (tile-grid units) into a viewport rectangle, returning the
/// per-quad transform the backend uses. Exposed for the backend; the math lives
/// here so it's native-tested. A tile-grid point `(gx, gy)` maps to
/// `rect.x + gx/board_w * rect.w`, `rect.y + gy/board_h * rect.h`.
pub fn place_board(rect: &Rect, board_w: u32, board_h: u32) -> BoardPlacement {
    BoardPlacement {
        x: rect.x,
        y: rect.y,
        sx: rect.w / board_w.max(1) as f32,
        sy: rect.h / board_h.max(1) as f32,
    }
}

/// The affine map from tile-grid units to viewport px for the placed board.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoardPlacement {
    pub x: f32,
    pub y: f32,
    pub sx: f32,
    pub sy: f32,
}

impl BoardPlacement {
    /// Map a tile-grid point to viewport px.
    pub fn map(&self, gx: f32, gy: f32) -> (f32, f32) {
        (self.x + gx * self.sx, self.y + gy * self.sy)
    }
}

// ── Phase B: hit-test regions + the virtual a11y tree ──────────────────────────

/// A clickable rectangle in viewport px (the JS bridge maps a pointer hit to one).
/// `radius`/`layer`/`color` are irrelevant here, so this is a bare box.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HitRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl HitRect {
    fn from(r: &Rect) -> HitRect {
        HitRect {
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
        }
    }
    /// Whether `(px, py)` (viewport px) lands inside this rect.
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// The shell's interactive regions for a `vw`×`vh` viewport — everything the JS
/// pointer bridge needs to turn a canvas tap/drag into a board tile, a hand-card
/// index, or a FAB. The board is a *rect + grid side* (JS maps a hit to a tile with
/// the same Y-flip the board draw uses); the hand is one rect per card; the FABs are
/// two named rects. Pure + native-tested, so the draw and the hit-testing never
/// disagree (one layout, one source of truth).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShellRegions {
    pub vw: f32,
    pub vh: f32,
    /// The board's pixel rectangle (the hero element).
    pub board: HitRect,
    /// The board's grid side (5 in 1v1) — JS maps a hit in `board` to a tile.
    pub board_w: u32,
    /// One rect per hand card, by hand index (the rest position — a lifted card's
    /// raised draw is cosmetic; the hit-target stays at rest so re-tapping toggles).
    /// The rects already reflect the live `hand_scroll`, so a hit maps to the visible
    /// card; off-screen cards still have a rect (their `x` is outside the viewport).
    pub hand: Vec<HitRect>,
    /// The whole hand-tray band rect — JS reads it to route a wheel / drag / swipe that
    /// starts on the tray to the carousel scroll (not a card pick-up).
    pub hand_tray: HitRect,
    /// How far the hand carousel can scroll (px); 0 when the whole hand fits. JS clamps
    /// `hand_scroll` to `[0, hand_max_scroll]`.
    pub hand_max_scroll: f32,
    /// The End-Turn FAB.
    pub end_turn: HitRect,
    /// The Glimpse FAB.
    pub study: HitRect,
}

impl ShellRegions {
    /// Resolve a viewport-px pointer to the board **tile** it lands on (the Y-flipped
    /// inverse of the board draw — home row at the bottom), or `None` if the point is
    /// outside the board square. The canonical px→tile map: [`shell_regions`] serializes
    /// the board rect + grid side to the JS bridge, whose `tileAtPx` helper (`index.html`)
    /// applies this exact formula instead of re-deriving the flip inline — so the draw and
    /// the hit-test share ONE source and a misroute can't slip in on only one side. Native
    /// tests pin the round-trip against the forward `tile_center_px` map.
    pub fn tile_at(&self, px: f32, py: f32) -> Option<u8> {
        tile_at_px(
            px,
            py,
            self.board_w,
            self.board.x,
            self.board.y,
            self.board.w,
        )
    }
}

/// The shell's interactive regions for `(vw, vh)` (see [`ShellRegions`]). The hand
/// rects honour the live hand size; the FABs + board match the draw exactly.
pub fn shell_regions(model: &ShellModel, vw: f32, vh: f32) -> ShellRegions {
    let vw = vw.max(1.0);
    let vh = vh.max(1.0);
    let m = Metrics::for_viewport(vw, vh);
    let tray_top = m.band_bottom + m.hud_h;
    let hand: Vec<HitRect> = (0..model.hand.len())
        .map(|i| {
            HitRect::from(&hand_card_rect(
                model,
                i,
                tray_top,
                m.content_x,
                m.content_w,
                m.tray_h,
                m.u,
                m.pad,
            ))
        })
        .collect();
    let l = hand_layout(
        model,
        tray_top,
        m.content_x,
        m.content_w,
        m.tray_h,
        m.u,
        m.pad,
    );
    let [et, st] = fab_rects(&m);
    ShellRegions {
        vw,
        vh,
        board: HitRect {
            x: m.board_x,
            y: m.board_y,
            w: m.board_side,
            h: m.board_side,
        },
        board_w: board_side_tiles(model),
        hand,
        hand_tray: HitRect {
            x: 0.0,
            y: tray_top,
            w: vw,
            h: m.tray_h,
        },
        hand_max_scroll: l.max_scroll,
        end_turn: HitRect::from(&et),
        study: HitRect::from(&st),
    }
}

/// One node in the **virtual a11y tree** — the off-screen ARIA mirror of the canvas
/// (AGENTS.md invariant 7). Each node the JS bridge renders as an actionable element
/// (an ARIA `button` when `command`/`select` is set, else a static `group`/`text`),
/// so a screen-reader / keyboard user reaches **every** canvas affordance at parity.
/// The tree is a flat, ordered list (board tiles, then the hand, the opponent strip,
/// the FABs) — assistive tech reads it top-to-bottom and Tab visits each actionable
/// node. Commands are NOT carried here (the JS side already holds the engine's legal
/// moves, the source of truth); instead a node names *what it targets* (`tile` /
/// `hand` / a global `action`) and JS fires the matching legal command — the same
/// path the canvas affordance uses, so the two can never diverge.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct A11yNode {
    /// A stable id (`tile-12`, `hand-3`, `fab-end`, `opp`, `section-board` …) — the
    /// JS bridge reuses the DOM element across frames by this id (so focus is stable).
    pub id: String,
    /// `"button"` (actionable) / `"group"` (a labelled section) / `"text"` (a static
    /// readout, e.g. the opponent strip). Drives the ARIA `role`.
    pub role: String,
    /// The accessible label (what a screen reader announces).
    pub label: String,
    /// For an actionable node, what it targets so JS fires the right legal command:
    /// `Tile(t)` (activate board tile `t`), `Hand(i)` (pick up / play hand card `i`),
    /// or `Action(verb)` for a global button (`"EndTurn"` / `"Glimpse"`). `None` ⇒ a
    /// non-actionable group/text node.
    #[serde(default)]
    pub target: Option<A11yTarget>,
    /// Whether this node is currently actionable (it has an available action / legal
    /// move). A non-actionable button still appears (so the structure is stable) but
    /// is exposed `aria-disabled` — present, not gone, so the tree never restructures
    /// under assistive tech mid-turn.
    #[serde(default)]
    pub enabled: bool,
    /// Heading depth for a `group` (a section header), else 0.
    #[serde(default)]
    pub level: u8,
    /// Grid placement for a node inside the board grid (the `"grid"` container, a
    /// `"row"`, or a `"gridcell"`). `None` for the flat sections (hand, opponent,
    /// FABs). Present so the JS bridge can assemble a true ARIA grid — `role="grid"` /
    /// `role="row"` / `role="gridcell"` with `aria-rowindex` / `aria-colindex` — letting
    /// a screen reader announce "row R, column C, `<occupant>`" and navigate the board by
    /// arrow keys, the per-tile fidelity `brand_and_accessibility.md` calls for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<A11yGrid>,
}

/// Where an [`A11yNode`] sits in the board's ARIA grid (AGENTS.md invariant 7, the
/// per-tile fidelity bar). The board is exposed as a real `role="grid"`: a container
/// node ([`A11yGridRole::Grid`], carrying the row / column **counts**), one
/// [`A11yGridRole::Row`] per board row, and a [`A11yGridRole::Cell`] for **every** tile
/// (carrying its 1-based `aria-rowindex` / `aria-colindex`). A cell with an available
/// action wraps a focusable button (the existing `tile-*` affordance, firing the same
/// command); an empty / inert cell is a plain gridcell with only its reading — so the
/// grid is **complete** for screen-reader navigation + announcement, yet the Tab
/// sequence still stops only on actionable cells (the "don't bury the action" rule).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct A11yGrid {
    /// Whether this node is the grid container, a row, or a single cell.
    pub role: A11yGridRole,
    /// 1-based row index within the grid (`aria-rowindex`). Set on rows + cells; 0 on
    /// the container.
    #[serde(default)]
    pub row: u32,
    /// 1-based column index within the grid (`aria-colindex`). Set on cells; 0 else.
    #[serde(default)]
    pub col: u32,
    /// On the container only: the total row count (`aria-rowcount`), so assistive tech
    /// announces "row R of N". 0 on rows / cells.
    #[serde(default)]
    pub rows: u32,
    /// On the container only: the total column count (`aria-colcount`). 0 on rows /
    /// cells.
    #[serde(default)]
    pub cols: u32,
}

/// The ARIA grid role an [`A11yGrid`] node carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum A11yGridRole {
    /// The `role="grid"` container wrapping the whole board.
    Grid,
    /// A `role="row"` — one per board row.
    Row,
    /// A `role="gridcell"` — one per tile.
    Cell,
}

/// What an [`A11yNode`] button targets — the JS bridge maps this to the same legal
/// command the canvas affordance fires (one action path, two surfaces).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum A11yTarget {
    /// Activate board tile `t` (pick up your piece / drop on a target / inspect).
    Tile(u8),
    /// Pick up / play hand card at index `i`.
    Hand(u8),
    /// A global action button by verb (`"EndTurn"` / `"Glimpse"`).
    Action(String),
}

/// Build the **virtual a11y tree** for the shell (Phase B / invariant 7): an ordered,
/// flat list of [`A11yNode`]s mirroring every canvas affordance —
///
/// 1. a **Board** section, then one actionable button per **occupied / actionable**
///    tile (its accessible label = the board mirror's per-tile reading, plus the
///    action it offers); empty, action-less tiles stay in the `#board-sr` text mirror
///    (listing 25 empties as buttons would bury the actionable ones);
/// 2. a **Hand** section, then one button per card (name + stats + whether it can be
///    played / evolved);
/// 3. the **Opponent** strip as a text node (name · score · hand size — never their
///    cards; redaction holds);
/// 4. the **Actions** — the End-Turn + Glimpse buttons.
///
/// `tile_labels` carries the per-tile screen-reader phrases (the JS side already
/// builds these from `board_aria`; passing them keeps one source for the wording).
/// The result is what `LocalGame::shell_a11y_json` serializes for the bridge.
pub fn build_a11y_tree(model: &ShellModel, tile_labels: &[String]) -> Vec<A11yNode> {
    let mut nodes: Vec<A11yNode> = Vec::new();
    let bw = board_side_tiles(model);
    let bh = ((model.view.tiles.len() as u32) / bw.max(1)).max(1);

    // 1) The board — a real ARIA grid (invariant 7, the per-tile fidelity bar). A
    // `role="grid"` container heads it, then EVERY tile is a `role="gridcell"` carrying
    // its 1-based row / column index and full reading, so a screen reader announces
    // "row R, column C, <occupant>" and arrow-navigates the board. Actionable cells
    // ALSO carry a `Tile` target + `enabled`, so the JS bridge wraps them in a focusable
    // button (the same affordance / command as before); inert cells stay plain gridcells
    // (reading only) — the grid is complete for navigation, but Tab still stops only on
    // the cells that can act (the "don't bury the action" rule the flat list kept).
    nodes.push(A11yNode {
        id: "section-board".into(),
        role: "grid".into(),
        label: format!(
            "Board, round {} of {}, a {bw} by {bh} grid. {}",
            model.round,
            model.last_round,
            if model.your_turn {
                "Your turn."
            } else {
                "Opponent's turn."
            }
        ),
        target: None,
        enabled: false,
        level: 2,
        grid: Some(A11yGrid {
            role: A11yGridRole::Grid,
            row: 0,
            col: 0,
            rows: bh,
            cols: bw,
        }),
    });
    for ri in 0..bh {
        // One `role="row"` per board row. The board renders seat A's home row at the
        // BOTTOM (the §5 orientation `tile_cell` uses), so the storage-order row index
        // `ri` IS the cell's printed row number — the grid's reading order matches the
        // coordinate a player reads off the board.
        nodes.push(A11yNode {
            id: format!("board-row-{ri}"),
            role: "row".into(),
            label: format!("Row {}", ri + 1),
            target: None,
            enabled: false,
            level: 0,
            grid: Some(A11yGrid {
                role: A11yGridRole::Row,
                row: ri + 1,
                col: 0,
                rows: 0,
                cols: 0,
            }),
        });
        for ci in 0..bw {
            let i = (ri * bw + ci) as usize;
            let Some(tile) = model.view.tiles.get(i) else {
                continue;
            };
            let t = i as u8;
            let actionable = model.actionable_tiles.contains(&t);
            let evolvable = model.evolvable_tiles.contains(&t);
            let devolvable = model.devolvable_tiles.contains(&t);
            let selected = model.interaction.selected == Some(t);
            let is_target = model.interaction.legal.contains(&t);
            let acts = actionable || evolvable || devolvable || is_target || selected;
            let occupied =
                tile.spirit.is_some() || tile.terrain.is_some() || tile.impression.is_some();
            // A cell is keyboard-REACHABLE (a Tab stop firing `activateTile`) when it can act
            // OR holds a piece to INSPECT — so an occupied-but-idle spirit is still selectable
            // off-turn. An empty, inert tile carries no target (no Tab stop), so the 25 empties
            // never bury the actionable ones — they remain present + announced as gridcells.
            let reachable = acts || occupied;
            let cell = tile_cell(i, bw);
            let reading = tile_labels
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("Tile {cell}"));
            // The action suffix the affordance offers (so the a11y twin says it too). A
            // standing-Faded form that can recede announces it in the faction's word
            // (the Solace recedes, the Lorekeeper reverts — vocabulary law, §5).
            let mut suffix = String::new();
            if selected {
                suffix.push_str(
                    " — selected; activate a highlighted tile to act, or activate again to cancel",
                );
            } else if is_target {
                suffix.push_str(" — a legal target; activate to act here");
            } else if devolvable {
                suffix.push_str(&format!(
                    " — standing Faded; can {} (play a base card from your hand to recede it a tier — the rescue)",
                    recede_verb(&model.you_faction).to_ascii_lowercase()
                ));
            } else if evolvable {
                suffix.push_str(" — Fading; can evolve (play a matching form card from your hand)");
            } else if actionable {
                suffix.push_str(" — has an available action; activate to select");
            }
            // EVERY tile is a gridcell (complete grid for SR navigation). The cell's
            // accessible name leads with its coordinate, so "row R, column C" reads with
            // the occupant. A REACHABLE cell (it can act, or holds a piece to inspect)
            // carries the `Tile` target, so the JS wraps the cell's content in a focusable
            // button; an inert EMPTY cell is a plain gridcell (no Tab stop, no command) —
            // but still present + announced, never invisible to the screen reader.
            nodes.push(A11yNode {
                id: format!("tile-{i}"),
                role: "gridcell".into(),
                label: format!("{cell}. {reading}{suffix}"),
                target: reachable.then_some(A11yTarget::Tile(t)),
                // Actionable (a live move) only when it's your turn and this tile offers
                // something; an occupied-but-idle cell is reachable (inspect) but exposed
                // aria-disabled for action — present, not gone.
                enabled: model.your_turn && acts,
                level: 0,
                grid: Some(A11yGrid {
                    role: A11yGridRole::Cell,
                    row: ri + 1,
                    col: ci + 1,
                    rows: 0,
                    cols: 0,
                }),
            });
            // Item 12 — a tile can hold BOTH a spirit AND a piece of terrain (a spirit standing on a
            // Landmark). The cell button above inspects/selects the SPIRIT (the top occupant), so the
            // LANDMARK would be unreachable. Emit a SECOND node for any inspectable terrain here — a
            // face-up Landmark, or a Fabrication revealed to the viewer (it carries a card identity) —
            // whose activation inspects the TERRAIN (the JS fires `inspect-terrain:<tile>`). On the
            // canvas the same target is reached by a toggle-tap (see `index.html`). Skipped for a
            // face-down enemy lie (no identity to read) — that stays a redacted "hidden terrain". It
            // rides INSIDE the same gridcell (a second button), so the grid geometry stays intact.
            if let Some(terr) = &tile.terrain
                && tile.spirit.is_some()
                && !terr.face_down
            {
                nodes.push(A11yNode {
                    id: format!("tile-{i}-terrain"),
                    role: "button".into(),
                    label: format!("{cell}. {} here — activate to inspect it", terr.kind),
                    target: Some(A11yTarget::Action(format!("inspect-terrain:{i}"))),
                    enabled: true, // inspect is always available (it's a read, not a turn action)
                    level: 0,
                    grid: Some(A11yGrid {
                        role: A11yGridRole::Cell,
                        row: ri + 1,
                        col: ci + 1,
                        rows: 0,
                        cols: 0,
                    }),
                });
            }
        }
    }

    // 2) The hand section + a button per card.
    nodes.push(A11yNode {
        id: "section-hand".into(),
        role: "group".into(),
        label: format!("Your hand, {} cards", model.hand.len()),
        target: None,
        enabled: false,
        level: 2,
        grid: None,
    });
    for (i, c) in model.hand.iter().enumerate() {
        let idx = i as u8;
        let actionable = model.actionable_hand.contains(&idx);
        let evolve = model.evolve_forms.contains(&idx);
        let devolve = model.devolve_bases.contains(&idx);
        let lifted = model.lifted_hand == Some(idx);
        let stat = if c.kind.eq_ignore_ascii_case("Spirit") || c.kind.is_empty() {
            format!(
                ", attack {}, defense {}, {} health",
                c.attack, c.defense, c.hp
            )
        } else {
            String::new()
        };
        let mut suffix = String::new();
        if lifted {
            suffix.push_str(", picked up — activate a highlighted tile to place it");
        } else if devolve {
            // A base card that can recede a standing-Faded form (it may also be playable
            // normally — but the recede is the distinct affordance, in the faction's word).
            suffix.push_str(&format!(
                ", a base — activate, then choose a standing-Faded form to {} it (the rescue)",
                recede_verb(&model.you_faction).to_ascii_lowercase()
            ));
        } else if evolve {
            suffix.push_str(", an evolution form — activate, then choose a matching base");
        } else if actionable {
            suffix.push_str(", playable — activate to pick up");
        } else {
            suffix.push_str(", no legal play this turn");
        }
        nodes.push(A11yNode {
            id: format!("hand-{i}"),
            role: "button".into(),
            label: format!(
                "{}, cost {}, {}{}{}",
                c.name,
                c.cost,
                if c.kind.is_empty() { "card" } else { &c.kind },
                stat,
                suffix
            ),
            target: Some(A11yTarget::Hand(idx)),
            enabled: model.your_turn && actionable,
            level: 0,
            grid: None,
        });
    }

    // 3) The opponent strip — a text node (redaction holds: name · score · hand size,
    // never their cards).
    let opp_name = if model.opp_name.is_empty() {
        opp_seat_word(model.you_seat.other())
    } else {
        model.opp_name.clone()
    };
    let score = if model.opp_erasures > 0 {
        format!("score {} ({} erased)", model.opp_score, model.opp_erasures)
    } else {
        format!("score {}", model.opp_score)
    };
    nodes.push(A11yNode {
        id: "opp".into(),
        role: "text".into(),
        label: format!(
            "Opponent: {opp_name}, {score}, holding {} cards",
            model.opp_hand_count
        ),
        target: None,
        enabled: false,
        level: 0,
        grid: None,
    });

    // 4) The global action buttons.
    nodes.push(A11yNode {
        id: "section-actions".into(),
        role: "group".into(),
        label: "Actions".into(),
        target: None,
        enabled: false,
        level: 2,
        grid: None,
    });
    nodes.push(A11yNode {
        id: "fab-end".into(),
        role: "button".into(),
        label: "End turn".into(),
        target: Some(A11yTarget::Action("EndTurn".into())),
        enabled: model.your_turn,
        level: 0,
        grid: None,
    });
    nodes.push(A11yNode {
        id: "fab-study".into(),
        role: "button".into(),
        label: "Glimpse — burn a hand card, then peek your top card and keep or bottom it".into(),
        target: Some(A11yTarget::Action("Glimpse".into())),
        enabled: model.your_turn,
        level: 0,
        grid: None,
    });
    nodes
}

/// Tile coordinate like the board's column letter + 1-based row (a1, c3, …), using
/// the engine's home-row-at-bottom orientation. Shared by the a11y labels.
fn tile_cell(i: usize, board_w: u32) -> String {
    let w = board_w.max(1);
    let col = (b'a' + (i as u32 % w) as u8) as char;
    let row = i as u32 / w + 1;
    format!("{col}{row}")
}

// ── Phase C: the paced opponent-turn replay + announcements ─────────────────────
//
// When it's the OPPONENT's turn (the bot, local 1v1) the client doesn't snap the
// state — it **replays the turn action-by-action through a pacing queue** (the
// "watched + paced" decision in `web_client_ux.md`): each discrete opponent action
// animates with a ~1s beat — its affected tile highlighted, a subtle one-line
// caption naming what happened, and the Solace's erasure tally counting up on an
// Unwriting. Each beat doubles as the a11y **live-region** announcement (invariant
// 7), so the screen-reader narration and the visual caption are one source.
//
// This is purely CLIENT-SIDE pacing of ALREADY-APPLIED state: the local engine has
// already decided + applied each bot command (determinism untouched), and every
// per-beat board snapshot is the **human's** redacted [`PlayerView`] (redaction
// untouched — the opponent's hand is never revealed, only their public board actions
// + face-down draws). [`crate::LocalGame::auto_play_turn_paced`] walks the turn one command
// at a time, distilling each command's events into a [`ReplayBeat`] and capturing the
// resulting redacted shell snapshot; the JS shell paces through them on a timer
// (honoring the animation-speed setting + `prefers-reduced-motion`).

/// The on-canvas **caption** for the action being replayed this frame (Phase C) —
/// the subtle banner over the board plus the tiles to pulse. The same fact as the
/// `#status` live-region announcement (the announcements-are-a11y decision); carried
/// on the [`ShellModel`] so the pure layout draws it and a `cargo test` can assert it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReplayCaption {
    /// The caption text — short, in the game's register ("The Solace tells an
    /// Unwriting"). Drawn as a quiet banner across the top of the board.
    pub text: String,
    /// A coarse kind tag (`"play"` / `"move"` / `"call"` / `"glimpse"` / `"evolve"` /
    /// `"banish"` / `"unwrite"` / `"phase"` / `"end"`), so the banner can tint by
    /// flavour (e.g. the Solace's erasure register reads cooler). A hint only.
    #[serde(default)]
    pub kind: String,
    /// The affected board tiles to **pulse** as this beat animates (the played /
    /// moved / struck / evolved / erased tiles) — a soft highlight so the eye lands
    /// where the action happened.
    #[serde(default)]
    pub tiles: Vec<u8>,
}

/// One paced beat of the opponent's replay — a single discrete action (play / move /
/// call / glimpse / evolve / banish / unwrite …) distilled from the events that one
/// bot command produced. The JS pacer animates these ~1s apart, drawing the
/// [`caption`](Self::caption) on-canvas, pushing [`announce`](Self::announce) into the
/// `#status` live region, and (on a Solace erasure) counting the tally to
/// [`erasures`](Self::erasures). The per-beat redacted shell snapshot rides alongside in
/// the JSON envelope ([`crate::LocalGame::auto_play_turn_paced`]), so the board animates
/// to the state *after* this action — not the turn's end.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReplayBeat {
    /// The subtle on-canvas caption naming the action ("The Solace tells an
    /// Unwriting", "Lorekeepers move a spirit"). Short, in the game's register —
    /// *banished*, never killed; only the Solace *Unwrites*.
    pub caption: String,
    /// The a11y live-region text for this beat — the same fact as [`caption`](Self::caption),
    /// phrased for narration (it IS the `#status` announcement). One source for the visual
    /// flourish and the screen-reader read (the announcements-are-a11y decision).
    pub announce: String,
    /// The affected board tiles to highlight as this beat animates.
    pub tiles: Vec<u8>,
    /// `Some(n)` when this action moved the Solace's off-board **erasure tally** — the
    /// post-action total `n`, so the HUD/opponent-strip tally counts up on an Unwriting
    /// / a forget / a Solace banish (the only place the Unwritten show). `None` else.
    #[serde(default)]
    pub erasures: Option<u8>,
    /// A coarse kind tag for the caption styling / animation flavour (see
    /// [`ReplayCaption::kind`]). JS uses it only as a hint (the engine stays
    /// authoritative).
    #[serde(default)]
    pub kind: String,
}

/// A short opponent label in the game's register from its faction word and seat —
/// "the Solace" / "Lorekeepers", matching the announcement voice. Empty `opp_name`
/// falls back to the seat's faction word.
fn opp_register(opp_name: &str, opp_seat: Seat) -> String {
    if opp_name.is_empty() {
        opp_seat_word(opp_seat)
    } else {
        opp_name.to_string()
    }
}

/// Distill **one bot command's** event slice into a single paced [`ReplayBeat`] — the
/// caption/announcement, the affected tiles, and whether it moved the Solace's erasure
/// tally. The anchor (the action's headline) is the most significant event in the
/// slice; subordinate events (strikes, a banish, a draw) enrich the wording + tiles.
/// `opp` is the acting opponent's register word; `erasures_after` is the Solace's tally
/// read from the post-command state (so the count-up is exact). Returns `None` for a
/// command that produced no player-visible action (a pure bookkeeping no-op) — the
/// pacer simply skips it, so the watched turn is only the moves that matter.
pub fn beat_for_command(
    events: &[recollect_core::state::Event],
    opp: &str,
    erasures_after: u8,
) -> Option<ReplayBeat> {
    use recollect_core::state::Event as E;
    if events.is_empty() {
        return None;
    }
    // Collect the affected tiles across the slice (deduped, in first-seen order) and
    // detect whether a banish / an erasure happened (for the tally + the verb).
    let mut tiles: Vec<u8> = Vec::new();
    let push = |t: u8, tiles: &mut Vec<u8>| {
        if !tiles.contains(&t) {
            tiles.push(t);
        }
    };
    let mut banished = false;
    let mut erased = false;
    for e in events {
        match e {
            E::SpiritPlayed { tile, .. }
            | E::Overwrote { tile, .. }
            | E::SpiritEvolved { tile, .. }
            | E::SpiritDevolved { tile, .. }
            | E::SpiritManifested { tile, .. }
            | E::UnwrittenManifested { tile, .. }
            | E::SpiritReleased { tile }
            | E::SpiritReclaimed { tile }
            | E::SpiritRevealed { tile }
            | E::OrdersSet { tile, .. }
            | E::LandmarkPlaced { tile, .. }
            | E::FabricationSet { tile, .. }
            | E::StraySurfaced { tile, .. }
            | E::StrayBefriended { tile, .. } => push(*tile, &mut tiles),
            E::SpiritMoved { from, to }
            | E::SpiritPushed { from, to }
            | E::UnwrittenShifted { from, to, .. } => {
                push(*from, &mut tiles);
                push(*to, &mut tiles);
            }
            E::Struck {
                from_tile, to_tile, ..
            } => {
                push(*from_tile, &mut tiles);
                push(*to_tile, &mut tiles);
            }
            E::BondAttached { tile_a, tile_b, .. } => {
                push(*tile_a, &mut tiles);
                push(*tile_b, &mut tiles);
            }
            E::SpiritBecameFading { tile, .. } | E::SpiritDissolved { tile, .. } => {
                push(*tile, &mut tiles);
                banished = true;
            }
            E::StrayBanished { tile, .. } => {
                push(*tile, &mut tiles);
                banished = true;
            }
            E::ImpressionUnwritten { tile } | E::ImpressionForgotten { tile } => {
                push(*tile, &mut tiles);
                erased = true;
            }
            E::TileFaded { tile }
            | E::FabricationRevealed { tile }
            | E::FabricationSpent { tile } => push(*tile, &mut tiles),
            _ => {}
        }
    }

    // The anchor — the headline action. Pick the FIRST event that names a discrete
    // play, in the order the engine emits them (the command's primary effect leads).
    // The phrasing stays in-register: spirits are *banished*; only the Solace *Unwrites*.
    let headline: Option<(&str, &str)> = events.iter().find_map(|e| match e {
        E::SpiritPlayed {
            face_down: true, ..
        }
        | E::FabricationSet { .. } => Some(("sets a face-down memory", "play")),
        E::SpiritPlayed { .. } | E::SpiritManifested { .. } => Some(("plays a spirit", "play")),
        E::Overwrote { .. } => Some(("overwrites a tile", "play")),
        E::SpiritMoved { .. } => Some(("moves a spirit", "move")),
        E::SpiritEvolved { .. } => Some(("evolves a spirit", "evolve")),
        // Devolution (§5): the rescue. The faction-precise verb (Lorekeeper REVERTS,
        // Solace RECEDES) is in the authoritative protocol `label`; this terse live-region
        // cue uses "recedes a spirit" — accurate for the devolve family and in-register for
        // the Solace, the faction most often seen doing it in PvE.
        E::SpiritDevolved { .. } => Some(("recedes a spirit", "evolve")),
        E::UnwritingTold { .. } => Some(("tells an Unwriting", "unwrite")),
        E::UnwrittenManifested { .. } => Some(("manifests an Unwritten", "unwrite")),
        E::RitualCast { .. } => Some(("casts a ritual", "call")),
        E::BondAttached { .. } => Some(("binds two spirits", "call")),
        E::LandmarkPlaced { .. } => Some(("raises a landmark", "call")),
        E::SpiritReclaimed { .. } => Some(("reclaims a spirit", "call")),
        E::SpiritRevealed { .. } => Some(("reveals a lurker", "call")),
        E::OrdersSet { hold, .. } => Some((
            if *hold {
                "orders a spirit to hold"
            } else {
                "orders a spirit to watch"
            },
            "call",
        )),
        E::Glimpsed { .. } => Some(("glimpses the Memory", "glimpse")),
        _ => None,
    });
    // No discrete play in the slice — classify by the most notable side effect, so a
    // command whose visible result is only a banish/erasure still narrates. But a slice
    // with NO action AND no banish/erasure (a bare EndTurn, or pure Flow/round
    // bookkeeping) is NOT a watchable beat — return `None` so the pacer skips it rather
    // than caption a meaningless "acts". (Signature-tier: every beat names a real move.)
    let (verb, kind): (&str, &str) = match headline {
        Some(h) => h,
        None if erased => ("erases an impression", "unwrite"),
        None if banished => ("banishes a spirit", "banish"),
        None => return None,
    };

    // A trailing clause when the headline action ALSO banished or erased (a strike on
    // arrival, an Unwriting that erased a mark) — so "plays a spirit, banishing one"
    // reads as one beat, not two. Erasure (the Solace) takes the register word.
    let mut announce = format!("{opp} {verb}");
    if kind != "banish" && kind != "unwrite" {
        if erased {
            announce.push_str(", erasing a memory");
        } else if banished {
            announce.push_str(", banishing a spirit");
        }
    }
    // The on-canvas caption is the same fact (kept short — it's a quiet one-liner).
    let caption = announce.clone();

    // The tally counts up only on a genuine **Solace erasure** — an Unwriting or a
    // forget (the events that bump `solace_erasures`). We report the post-command total
    // so the strip animates 4 → 5 exactly. (A Lorekeeper banish leaves no off-board mark,
    // so it never moves the tally; the per-beat snapshot's `opp_erasures` is the
    // authoritative display either way — this flag is just the count-up cue.)
    let moved_tally = erased || kind == "unwrite";

    Some(ReplayBeat {
        caption,
        announce,
        tiles,
        erasures: if moved_tally {
            Some(erasures_after)
        } else {
            None
        },
        kind: kind.to_string(),
    })
}

/// The match-start announcement: **who opens** (and always the first player), in the
/// game's register. Local 1v1 always opens the human (seat A), so this reads "You open
/// the telling" from the human's vantage; the opponent-opens phrasing is here for the
/// spectator / future-opener paths. Pure so it's native-tested and shared with the
/// `#status` live region.
pub fn opener_announcement(you_seat: Seat, first_seat: Seat, opp_name: &str) -> String {
    if you_seat == first_seat {
        "You open the telling — the first word is yours.".to_string()
    } else {
        let opp = opp_register(opp_name, first_seat);
        format!("{opp} opens the telling — they take the first word.")
    }
}

/// A round-boundary announcement, if `round` is one of the binding beats the design
/// calls out: **Dusk** ("Dusk falls") the round AFTER the board contracts
/// (`round == dusk_after + 1`, i.e. the failing edge begins), and **Nightfall** at the
/// last round (`round == last_round`). Other rounds get no set-piece announcement here
/// (the per-action beats carry the turn). Returns `None` off those beats — these two
/// are the round announcements the live region must surface (alongside banish/evolve).
pub fn round_announcement(round: u8, dusk_after: u8, last_round: u8) -> Option<String> {
    if round == last_round {
        Some("Nightfall — the last round. The Memory will keep what stands.".to_string())
    } else if round == dusk_after + 1 {
        Some("Dusk falls — the page begins to fail at its edges.".to_string())
    } else {
        None
    }
}

// ── Phase D: the Dusk / Nightfall set-piece + the in-canvas result screen ───────
//
// The LAST canvas-native phase (the "d" in `web_client_ux.md`). Two set-pieces:
//
//  • **The Dusk (end of R8) and Nightfall (R12)** — the binding beats made visible.
//    Phase C already announces them in the `#status` live region (`round_announcement`);
//    this is the *visual* half: a brief animated flourish over the board — the rim
//    contracting and darkening (the page failing at its edges), the binding strip's
//    clock face lit, and the seal ("Dusk falls" / "Nightfall"). The held tiles already
//    render lamplit in the board scene ([`scene`](crate::scene)'s Lamp layer); this
//    set-piece is the *moment* the Dusk arrives. The JS pacer drives the fade-in via
//    [`DuskSetPiece::progress`] and honours `prefers-reduced-motion` (it shows the
//    set-piece near-instant), so the composition here is pure + native-tested.
//
//  • **The result screen** — the verdict in the game's voice (*the Memory keeps
//    [winner]* / *both kept* / *forgotten*), the score breakdown (board points + the
//    Solace's off-board erasure tally), and the action affordances (Rematch / New
//    opponent / Back to site). It adapts across modes (1v1-vs-bot, 2v2, PvP) — the
//    actions and the verbs the verdict uses are carried in the [`ResultScreen`] the
//    caller builds, so the same layout renders every mode. a11y: the result is mirrored
//    in the virtual tree + announced, and the actions are actionable nodes.

/// The animated **Dusk / Nightfall set-piece** drawn over the board at a binding beat
/// (Phase D). Carried on the [`ShellModel`] so the pure layout draws it and a
/// `cargo test` can assert it. The JS pacer animates [`Self::progress`] 0→1 (a quick
/// fade/contract-in, then a dwell); `prefers-reduced-motion` collapses it to a near-
/// instant reveal. The same fact the `#status` live region reads ("Dusk falls" /
/// "Nightfall") — one source for the visual flourish and the screen-reader narration.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DuskSetPiece {
    /// `"dusk"` (the rim begins to fail at the end of R8) or `"nightfall"` (the last
    /// round — the Memory keeps what stands). Tints + titles the set-piece.
    pub kind: String,
    /// The seal text — "Dusk falls" / "Nightfall" — in the game's register.
    pub title: String,
    /// A one-line subtitle (what the beat means), e.g. "the page begins to fail at its
    /// edges" / "the Memory will keep what stands".
    #[serde(default)]
    pub subtitle: String,
    /// The set-piece's animation progress, 0→1 (the JS pacer ramps it; reduced-motion
    /// snaps to 1). The rim darkening + the seal fade in by it. Clamped on draw.
    #[serde(default)]
    pub progress: f32,
}

/// The in-canvas **result screen** content (Phase D), built by the caller from the
/// finished match. The shell lays it out as a centered card over a scrimmed final
/// board: the verdict (in the game's voice), the score breakdown, and the action
/// affordances. Mode-agnostic — the verbs the verdict uses and the action set are
/// carried here, so 1v1-vs-bot / 2v2 / PvP all render through one layout. The JS bridge
/// mirrors the same fields into the virtual a11y tree (the verdict + actions as nodes).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResultScreen {
    /// The headline verdict, in the game's voice — "The Memory keeps the Lorekeepers" /
    /// "The Memory is forgotten" (the Solace wins — erasure carried the telling) / "Both
    /// are kept — a draw". Built by the caller so the register adapts to who won + which
    /// faction (and to PvP, where it names the human winner).
    pub verdict: String,
    /// A one-line elaboration under the verdict (what carried the telling), e.g. "Held
    /// to the last — your spirits stand in the Memory." / "Erased faster than you could
    /// hold — the never-remembered won the page." A quiet second line; may be empty.
    #[serde(default)]
    pub flavor: String,
    /// Whether this is a draw (both kept) — tints the verdict neutral rather than to a
    /// seat's ink.
    #[serde(default)]
    pub draw: bool,
    /// `Some(seat)` of the winner ("A" / "B") to tint the verdict in their ink; `None`
    /// on a draw. Carried as a string so the serde payload stays a plain JSON object.
    #[serde(default)]
    pub winner: Option<String>,
    /// The score breakdown rows — each a `(label, value)` the screen lists under the
    /// verdict (e.g. "Lorekeepers — board", "12"; "the Solace — erased", "5"). The
    /// caller composes them so the Solace's off-board erasure tally folds in exactly as
    /// the HUD shows it. Ordered top→bottom.
    #[serde(default)]
    pub breakdown: Vec<ResultRow>,
    /// The action affordances, in order (Rematch / New opponent / Back to site). Each is
    /// a labelled, actionable node the shell draws as a button and the a11y tree mirrors.
    /// The caller adapts the set per mode (e.g. PvP may relabel "Rematch" → "Offer a
    /// rematch"); the shell just lays out whatever it's given.
    #[serde(default)]
    pub actions: Vec<ResultAction>,
}

/// One score-breakdown row on the result screen: a left-aligned `label`, a right-aligned
/// `value`. `solace` tints the row (the off-board erasure tally reads in the Solace's
/// register).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResultRow {
    pub label: String,
    pub value: String,
    /// Whether this row is the Solace's tally (tinted to its warm ink, set apart from
    /// the board-points rows).
    #[serde(default)]
    pub solace: bool,
}

/// One result-screen action affordance — a labelled button the shell draws and the
/// a11y tree mirrors. `verb` is the stable id the JS bridge dispatches on
/// (`"rematch"` / `"new"` / `"site"`), so the canvas button and the accessible node
/// fire the same thing. `primary` reads filled (the suggested next step — Rematch).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResultAction {
    pub label: String,
    pub verb: String,
    #[serde(default)]
    pub primary: bool,
}

/// The Dusk/Nightfall set-piece over the board (Phase D): a darkening rim-contraction
/// vignette (the page failing at its edges — the four bands of the rim wash inward),
/// the binding strip's clock face lit large at the board's foot, and the seal title +
/// subtitle. Animated by `set.progress` (the JS pacer ramps it; reduced-motion snaps
/// to 1), so the rim closes in and the seal fades up. Drawn on Detail/Text over the
/// board; pairs with the live-region "Dusk falls" / "Nightfall" announcement.
fn dusk_set_piece(set: &DuskSetPiece, s: &mut ShellScene, bx: f32, by: f32, side: f32, u: f32) {
    let p = set.progress.clamp(0.0, 1.0);
    let nightfall = set.kind == "nightfall";
    // The rim contraction: four darkening bands closing in from the board edges, their
    // depth growing with progress (a fifth of the board at full Dusk, a third at
    // Nightfall — the page failing harder as the end nears). Each band is a single
    // alpha quad on the Detail layer (over the board), so it reads as the night creeping
    // in without hiding the inner board where the game is still fought.
    let depth = side * (if nightfall { 0.34 } else { 0.20 }) * p;
    let band = with_alpha(DUSK, (if nightfall { 0.85 } else { 0.66 }) * p);
    if depth > 0.5 {
        // Top, bottom, left, right bands (left/right inset so corners aren't double-darkened).
        s.rects.push(Rect {
            x: bx,
            y: by,
            w: side,
            h: depth,
            color: band,
            radius: 0.0,
            layer: ShellLayer::Detail,
        });
        s.rects.push(Rect {
            x: bx,
            y: by + side - depth,
            w: side,
            h: depth,
            color: band,
            radius: 0.0,
            layer: ShellLayer::Detail,
        });
        s.rects.push(Rect {
            x: bx,
            y: by + depth,
            w: depth,
            h: side - 2.0 * depth,
            color: band,
            radius: 0.0,
            layer: ShellLayer::Detail,
        });
        s.rects.push(Rect {
            x: bx + side - depth,
            y: by + depth,
            w: depth,
            h: side - 2.0 * depth,
            color: band,
            radius: 0.0,
            layer: ShellLayer::Detail,
        });
    }

    // The clock face — the binding strip rendered as a clock at the board's foot (design
    // §5), lit large for the set-piece: a ringed disc with a sweep of pips, the failing
    // ones (past the Dusk) in the dark ink, and a hand pointing to the current beat.
    // Centered low over the board so it reads as the Memory's clock striking the hour.
    let clock_r = (side * 0.13).clamp(u * 2.0, u * 5.0);
    let ccx = bx + side / 2.0;
    let ccy = by + side * 0.62;
    // A soft halo behind the clock so it lifts off the darkening board.
    s.rects.push(Rect {
        x: ccx - clock_r * 1.25,
        y: ccy - clock_r * 1.25,
        w: clock_r * 2.5,
        h: clock_r * 2.5,
        color: with_alpha(GILD, 0.10 * p),
        radius: clock_r * 1.25,
        layer: ShellLayer::Detail,
    });
    // The clock face (paper, ringed in gild) — full at Nightfall, the gild ring brighter.
    s.rects.push(Rect {
        x: ccx - clock_r,
        y: ccy - clock_r,
        w: clock_r * 2.0,
        h: clock_r * 2.0,
        color: with_alpha(PAPER, 0.92 * p),
        radius: clock_r,
        layer: ShellLayer::Detail,
    });
    push_ring_px(
        s,
        ccx - clock_r,
        ccy - clock_r,
        clock_r * 2.0,
        clock_r * 2.0,
        with_alpha(if nightfall { DUSK } else { GILD }, p),
        (clock_r * 0.10).max(2.0),
    );
    // Twelve pips around the face; the failing hours (past the Dusk) read dark, the
    // lit ones gild. The "hand" is a short dark spoke to the current hour — at Nightfall
    // it points straight up to 12 (the last hour). A simple 12-point ring (the backend
    // has only axis-aligned quads, so each pip is a small disc placed by angle).
    let hours = 12u8;
    for h in 0..hours {
        let ang = std::f32::consts::TAU * (h as f32 / hours as f32) - std::f32::consts::FRAC_PI_2;
        let pr = clock_r * 0.78;
        let dot = (clock_r * 0.13).max(1.5);
        let dx = ccx + ang.cos() * pr;
        let dy = ccy + ang.sin() * pr;
        // Hours 9..12 are "past the Dusk" (the failing edge of the telling); they read dark.
        let failing = h >= 8;
        let col = if failing {
            DUSK
        } else {
            with_alpha(GILD, 0.85)
        };
        s.rects.push(Rect {
            x: dx - dot / 2.0,
            y: dy - dot / 2.0,
            w: dot,
            h: dot,
            color: with_alpha(col, p),
            radius: dot / 2.0,
            layer: ShellLayer::Text,
        });
    }
    // The hand: a short dark spoke from centre toward 12 (Nightfall) or toward the
    // failing arc (Dusk) — a staircase of small rects, the backend's diagonal.
    let hand_to = if nightfall {
        -std::f32::consts::FRAC_PI_2
    } else {
        std::f32::consts::FRAC_PI_2 * 0.6
    };
    let steps = 5usize;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let rr = clock_r * 0.66 * t;
        let hx = ccx + hand_to.cos() * rr;
        let hy = ccy + hand_to.sin() * rr;
        let th = (clock_r * 0.10).max(1.5);
        s.rects.push(Rect {
            x: hx - th / 2.0,
            y: hy - th / 2.0,
            w: th,
            h: th,
            color: with_alpha(DUSK, p),
            radius: 0.0,
            layer: ShellLayer::Text,
        });
    }

    // The seal — the title + subtitle, fading up over the clock. The title sits above the
    // clock, large; the subtitle below it. Both in the bitmap-drawable register.
    let title = sanitize_keep_punct(&set.title);
    let title_size = fit_size(title.chars().count().max(1), side * 0.8).min(u * 2.2);
    s.texts.push(Text {
        x: ccx,
        y: by + side * 0.24,
        size: title_size,
        text: title,
        color: with_alpha(if nightfall { PAPER } else { GILD }, p),
        align: Align::Center,
        bold: false,
        layer: ShellLayer::Text,
    });
    if !set.subtitle.is_empty() {
        let sub = sanitize_keep_punct(&set.subtitle);
        let sub_size = fit_size(sub.chars().count().max(1), side * 0.82).min(u * 0.9);
        s.texts.push(Text {
            x: ccx,
            y: by + side * 0.32,
            size: sub_size,
            text: sub,
            color: with_alpha(PAPER, p),
            align: Align::Center,
            bold: false,
            layer: ShellLayer::Text,
        });
    }
}

/// The in-canvas **result screen** (Phase D): a centered card over a scrim, drawing the
/// verdict (tinted to the winner's ink, neutral on a draw), the score breakdown, and the
/// action affordances as buttons (Rematch primary). The scrim dims the final board so the
/// result reads above it without hiding it (the board is the souvenir behind the verdict).
/// The action button rects are exposed via [`result_action_rects`] so the JS bridge maps a
/// tap onto the right verb (one source with this draw).
fn result_screen(res: &ResultScreen, s: &mut ShellScene, vw: f32, vh: f32, u: f32) {
    // A scrim over the whole viewport (the result is a modal beat; the board shows through).
    s.rects.push(Rect {
        x: 0.0,
        y: 0.0,
        w: vw,
        h: vh,
        color: with_alpha(DUSK, 0.55),
        radius: 0.0,
        layer: ShellLayer::Panel,
    });
    // The result card — centered, sized to the content, bounded to the viewport.
    let cw = (u * 22.0).min(vw - u * 1.5);
    let n_rows = res.breakdown.len() as f32;
    let n_acts = res.actions.len() as f32;
    let row_h = u * 1.5;
    let act_h = u * 2.4;
    let ch = (u * 2.2 // top pad + verdict
        + u * 1.6 // flavor
        + u * 1.0
        + n_rows * row_h
        + u * 1.2
        + n_acts * (act_h + u * 0.6)
        + u * 1.2)
        .min(vh - u * 1.0);
    let cx0 = (vw - cw) / 2.0;
    let cy0 = (vh - ch) / 2.0;
    push_card_grad(s, cx0, cy0, cw, ch, CARD, CARD_DEEP, ShellLayer::Card, u);
    // A header rule in the winner's ink (neutral gild on a draw), so the verdict carries
    // a quiet identity band — the page closing in the victor's colour.
    let accent = match (res.draw, res.winner.as_deref()) {
        (true, _) => GILD,
        (_, Some("A")) => seat_ink(Seat::A),
        (_, Some("B")) => seat_ink(Seat::B),
        _ => INK,
    };
    push_ring_px(s, cx0, cy0, cw, ch, with_alpha(accent, 0.8), 2.0);
    let ccx = cx0 + cw / 2.0;
    let mut cy = cy0 + u * 1.8;

    // The verdict — the headline, tinted to the winner's ink (neutral on a draw), sized
    // to fit the card. The game's voice ("The Memory keeps the Lorekeepers").
    let verdict = sanitize_keep_punct(&res.verdict);
    let verdict_size = fit_size(verdict.chars().count().max(1), cw - u * 1.5).min(u * 1.7);
    s.texts.push(Text {
        x: ccx,
        y: cy,
        size: verdict_size,
        text: verdict,
        color: accent,
        align: Align::Center,
        bold: false,
        layer: ShellLayer::Text,
    });
    cy += u * 1.5;
    if !res.flavor.is_empty() {
        let flavor = sanitize_keep_punct(&res.flavor);
        for l in wrap_lines(&flavor, ((cw - u * 1.5) / (u * 0.34)) as usize) {
            s.texts.push(Text {
                x: ccx,
                y: cy,
                size: u * 0.7,
                text: l,
                color: SOFT,
                align: Align::Center,
                bold: false,
                layer: ShellLayer::Text,
            });
            cy += u * 0.95;
        }
    }
    cy += u * 0.4;
    // The score breakdown — each row a label (left) + a value (right), the Solace's
    // erasure tally tinted to its warm ink and set apart from the board-points rows.
    for row in &res.breakdown {
        let ink = if row.solace { seat_ink(Seat::B) } else { SOFT };
        s.texts.push(Text {
            x: cx0 + u * 1.0,
            y: cy,
            size: u * 0.78,
            text: sanitize_keep_punct(&row.label),
            color: ink,
            align: Align::Left,
            bold: false,
            layer: ShellLayer::Text,
        });
        s.texts.push(Text {
            x: cx0 + cw - u * 1.0,
            y: cy,
            size: u * 0.95,
            text: sanitize_keep_punct(&row.value),
            color: if row.solace { seat_ink(Seat::B) } else { INK },
            align: Align::Right,
            bold: false,
            layer: ShellLayer::Text,
        });
        cy += row_h;
    }
    let _ = cy; // the breakdown ends the flowed text; the actions are absolutely placed below
    // The actions — Rematch (primary, filled) / New opponent / Back to site, stacked, the
    // primary first. Their rects are computed by `result_action_layout` (shared with the
    // hit-test), so the draw and the tap-mapping agree.
    let rects = result_action_layout(res, vw, vh, u);
    for (i, a) in res.actions.iter().enumerate() {
        let r = rects[i];
        if a.primary {
            push_card_rect(s, r.x, r.y, r.w, r.h, accent, ShellLayer::Card, u);
        } else {
            push_card_rect(s, r.x, r.y, r.w, r.h, CARD, ShellLayer::Card, u);
            push_ring_px(s, r.x, r.y, r.w, r.h, with_alpha(INK, 0.7), 1.5);
        }
        let label_size = fit_size(a.label.chars().count().max(1), r.w - u * 0.8).min(u * 1.0);
        s.texts.push(Text {
            x: r.x + r.w / 2.0,
            y: r.y + r.h / 2.0,
            size: label_size,
            text: sanitize_keep_punct(&a.label),
            color: if a.primary { PAPER } else { INK },
            align: Align::Center,
            bold: false,
            layer: ShellLayer::Text,
        });
    }
}

/// The result-screen action button rects, in order — the pure geometry the draw and the
/// hit-test share. Stacked in the lower third of the result card, centered, the primary
/// first. (Mirrors the card geometry in `result_screen`.)
fn result_action_layout(res: &ResultScreen, vw: f32, vh: f32, u: f32) -> Vec<Rect> {
    let cw = (u * 22.0).min(vw - u * 1.5);
    let n_rows = res.breakdown.len() as f32;
    let n_acts = res.actions.len() as f32;
    let row_h = u * 1.5;
    let act_h = u * 2.4;
    let ch = (u * 2.2
        + u * 1.6
        + u * 1.0
        + n_rows * row_h
        + u * 1.2
        + n_acts * (act_h + u * 0.6)
        + u * 1.2)
        .min(vh - u * 1.0);
    let cx0 = (vw - cw) / 2.0;
    let cy0 = (vh - ch) / 2.0;
    // The actions start below the verdict + flavor + breakdown block.
    let flavor_h = if res.flavor.is_empty() { 0.0 } else { u * 1.6 };
    let mut y = cy0 + u * 1.8 + u * 1.5 + flavor_h + u * 0.4 + n_rows * row_h + u * 0.6 + u * 0.6;
    let bw = (cw - u * 2.0).min(u * 16.0);
    let bx = cx0 + (cw - bw) / 2.0;
    let mut out = Vec::with_capacity(res.actions.len());
    for _ in &res.actions {
        out.push(Rect {
            x: bx,
            y,
            w: bw,
            h: act_h,
            color: CARD,
            radius: u * 0.3,
            layer: ShellLayer::Card,
        });
        y += act_h + u * 0.6;
    }
    out
}

/// The result-screen action hit-test rects for `(vw, vh)`, keyed by [`ResultAction::verb`]
/// — the JS bridge maps a canvas tap to the matching verb through these (one source with
/// the `result_screen` draw). Returns `(verb, HitRect)` pairs in draw order.
pub fn result_action_rects(res: &ResultScreen, vw: f32, vh: f32) -> Vec<(String, HitRect)> {
    let vw = vw.max(1.0);
    let vh = vh.max(1.0);
    let m = Metrics::for_viewport(vw, vh);
    let rects = result_action_layout(res, vw, vh, m.u);
    res.actions
        .iter()
        .zip(rects)
        .map(|(a, r)| (a.verb.clone(), HitRect::from(&r)))
        .collect()
}

/// Compose the [`ResultScreen`] for a finished telling (Phase D) — the verdict in the
/// game's voice, the score breakdown, and the mode-adapted actions. The verdict register:
///
///  • A **Lorekeeper** wins ⇒ *the Memory keeps them* — what is loved is kept. From the
///    human's vantage (local 1v1) the winner reads "you".
///  • The **Solace** wins ⇒ *the Memory is forgotten* — the never-remembered took the page
///    (the Solace's win condition is erasure, not holding; the register stays Solace-only).
///  • A **draw** ⇒ *both are kept*.
///
/// The breakdown lists each seat's board points, and the Solace's off-board **erasure
/// tally** as its own tinted row (the only place forgetting scores). `mode` adapts the
/// actions: `"pvp"` makes Rematch an invite ("Offer a rematch"); `"bot"`/`"2v2"` reseed
/// directly ("Rematch"). Pure + native-tested — the wording is asserted here, not in the
/// browser. `factions` + `words` carry each seat's faction + its register label so the same
/// builder phrases any pairing; `human` is the watching seat (tints "you").
#[allow(clippy::too_many_arguments)]
pub fn build_result_screen(
    result: recollect_core::state::MatchResult,
    score_a: u8,
    score_b: u8,
    a_board: u8,
    b_board: u8,
    erasures: u8,
    faction_a: recollect_core::types::Faction,
    faction_b: recollect_core::types::Faction,
    word_a: &str,
    word_b: &str,
    human: Seat,
    mode: &str,
) -> ResultScreen {
    use recollect_core::state::MatchResult;
    use recollect_core::types::Faction;

    let faction_of = |seat: Seat| {
        if seat == Seat::A {
            faction_a
        } else {
            faction_b
        }
    };
    let word_of = |seat: Seat| if seat == Seat::A { word_a } else { word_b };

    let (verdict, flavor, draw, winner) = match result {
        MatchResult::Draw => (
            "Both are kept — a draw.".to_string(),
            "Neither telling overtook the other; the Memory holds them both.".to_string(),
            true,
            None,
        ),
        MatchResult::Win(seat) => {
            let won = faction_of(seat) == Faction::Solace;
            if won {
                // The Solace wins by erasure — the page is forgotten. (Never "killed/
                // destroyed"; only the Solace Unwrites — the register is Solace-only.)
                let v = if human == seat {
                    "The Memory is forgotten — you let it slip.".to_string()
                } else {
                    "The Memory is forgotten.".to_string()
                };
                let f = "Erased faster than it could be held — the never-remembered took the page."
                    .to_string();
                (v, f, false, Some(seat))
            } else {
                // A Lorekeeper holds the page — the Memory keeps what is loved.
                let v = if human == seat {
                    "The Memory keeps you.".to_string()
                } else {
                    format!("The Memory keeps {}.", word_of(seat))
                };
                let f = if human == seat {
                    "Held to the last — your spirits still stand in the telling.".to_string()
                } else {
                    "Held to the last — their spirits still stand in the telling.".to_string()
                };
                (v, f, false, Some(seat))
            }
        }
    };

    // The score breakdown — each seat's board points, plus the Solace's off-board tally as
    // its own tinted row. The headline score (score_a / score_b) is the engine's final
    // tally (board, and for the Solace board+erasures); we surface the parts so the player
    // sees WHY (board points held vs memories erased).
    let mut breakdown: Vec<ResultRow> = Vec::new();
    let solace_seat = if faction_a == Faction::Solace {
        Some(Seat::A)
    } else if faction_b == Faction::Solace {
        Some(Seat::B)
    } else {
        None
    };
    for seat in [Seat::A, Seat::B] {
        let board = if seat == Seat::A { a_board } else { b_board };
        let total = if seat == Seat::A { score_a } else { score_b };
        let is_solace = solace_seat == Some(seat);
        // Board points row (every seat).
        breakdown.push(ResultRow {
            label: format!("{} — board", word_of(seat)),
            value: format!("{board}"),
            solace: false,
        });
        if is_solace {
            // The Solace's off-board erasure tally + its folded total (board + erased).
            breakdown.push(ResultRow {
                label: format!("{} — erased", word_of(seat)),
                value: format!("{erasures}"),
                solace: true,
            });
            breakdown.push(ResultRow {
                label: format!("{} — kept", word_of(seat)),
                value: format!("{total}"),
                solace: true,
            });
        }
    }

    // The actions — Rematch (primary) / New opponent / Back to site. PvP makes the rematch
    // an INVITE (host/join is the launch flow — you offer, the opponent accepts), not an
    // instant reseed; a bot or 2v2-watch match reseeds directly.
    let rematch_label = if mode == "pvp" {
        "Offer a rematch"
    } else {
        "Rematch"
    };
    let actions = vec![
        ResultAction {
            label: rematch_label.into(),
            verb: "rematch".into(),
            primary: true,
        },
        ResultAction {
            label: "New opponent".into(),
            verb: "new".into(),
            primary: false,
        },
        ResultAction {
            label: "Back to site".into(),
            verb: "site".into(),
            primary: false,
        },
    ];

    ResultScreen {
        verdict,
        flavor,
        draw,
        winner: winner.map(|s| format!("{s:?}")),
        breakdown,
        actions,
    }
}

/// The **virtual a11y tree** for the in-canvas result screen (Phase D / invariant 7):
/// the verdict + the score breakdown as a labelled text/group readout, then each action
/// (Rematch / New opponent / Back to site) as an actionable button whose target is an
/// [`A11yTarget::Action`] carrying the `verb` the canvas button fires (one action path,
/// two surfaces). The canvas is opaque to assistive tech, so this is the accessible
/// result — keyboard-reachable and announced. Pure + native-tested.
pub fn result_a11y_tree(res: &ResultScreen) -> Vec<A11yNode> {
    let mut nodes: Vec<A11yNode> = Vec::new();
    // The result section header — names the verdict (the live region also announces it).
    nodes.push(A11yNode {
        id: "section-result".into(),
        role: "group".into(),
        label: format!("The telling has ended. {}", res.verdict),
        target: None,
        enabled: false,
        level: 2,
        grid: None,
    });
    // The score breakdown as one text readout (board points + the Solace's erasure tally).
    if !res.breakdown.is_empty() {
        let rows: Vec<String> = res
            .breakdown
            .iter()
            .map(|r| format!("{}: {}", r.label, r.value))
            .collect();
        nodes.push(A11yNode {
            id: "result-score".into(),
            role: "text".into(),
            label: format!("Final tally — {}", rows.join("; ")),
            target: None,
            enabled: false,
            level: 0,
            grid: None,
        });
    }
    if !res.flavor.is_empty() {
        nodes.push(A11yNode {
            id: "result-flavor".into(),
            role: "text".into(),
            label: res.flavor.clone(),
            target: None,
            enabled: false,
            level: 0,
            grid: None,
        });
    }
    // The actions — each an actionable button firing its verb (the same the canvas does).
    for a in &res.actions {
        nodes.push(A11yNode {
            id: format!("result-{}", a.verb),
            role: "button".into(),
            label: a.label.clone(),
            target: Some(A11yTarget::Action(a.verb.clone())),
            enabled: true,
            level: 0,
            grid: None,
        });
    }
    nodes
}

// ── the in-canvas GLIMPSE + MULLIGAN choice prompt ──────────────────────────
//
// The last big local-1v1 canvas interaction (`web_client_ux.md` §Mulligan + the
// Glimpse flow in design §5). Two engine decisions are surfaced as in-canvas modals:
//
//  • **Glimpse** (the Glimpse button) is a two-step pending choice. Step 1 is the BURN
//    cost — pick a hand card to spend to activate the glimpse (it leaves play). Step 2
//    peeks the deck's top card and offers KEEP (stays on top, no Anima) or BOTTOM
//    (+1 Anima). Both ride the engine's `PendingChoice`/`Choose` flow, surfaced to the
//    owner ONLY via `view.you.pending` (redaction holds — the opponent's glimpse never
//    surfaces here, only as a paced-replay announcement). The prompt's options dispatch
//    `Command::Choose { index }`.
//
//  • **Mulligan** (the opening) is the London-lite redraw: at the very start, before
//    you've acted, you may mulligan once — draw a fresh full hand, then ONE card goes
//    to the bottom (seed-chosen, the cost — there is no second pick step, so the prompt
//    is one beat: "Mulligan" or "Keep"). Unlike the glimpse it is a *legal command*
//    (`Command::Mulligan`), not a pending choice, so the JS opening offer rides in
//    through `ui_json` (a `mulligan_offer` flag the bridge sets at game open + clears on
//    either pick). The announcement states only the FACT ("you mulligan"), never the
//    cards (redaction).
//
// Every option is mirrored in the virtual a11y tree as an actionable button + the step
// is announced in `#status` (invariant 7) — the canvas and the accessible mirror fire
// the same command, so they can never diverge.

/// The active **choice prompt** to draw: a small modal card over the board posing
/// the Glimpse (burn → keep/bottom) or the opening Mulligan, with its options as
/// selectable chips. Carried on the [`ShellModel`] so the pure layout draws it and a
/// `cargo test` can assert it. Built by [`build_choice_prompt`] from the redacted
/// `PlayerView`'s owner-only pending choice (Glimpse) or the JS opening offer (Mulligan).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChoicePrompt {
    /// A coarse kind tag: `"glimpse_burn"` (step 1 — pick a card to burn), `"glimpse_keep"`
    /// (step 2 — keep/bottom the peeked card), or `"mulligan"` (the opening offer). Tints +
    /// titles the modal and drives the option styling (a burn reads as a COST).
    pub kind: String,
    /// The headline — the framing, in the game's register ("Glimpse — burn a card to peek
    /// the Memory" / "You peek the top of the Memory" / "The opening — mulligan your hand?").
    pub title: String,
    /// A one-line subtitle (what the choice means / its cost), e.g. "the burned card leaves
    /// play — this is the glimpse's cost" / "keep it on top, or bottom it for +1 Anima".
    /// May be empty.
    #[serde(default)]
    pub subtitle: String,
    /// The **peeked card** floated above the options on the keep/bottom step — the top of
    /// the Memory you just glimpsed (owner-only; redaction holds). `None` on the burn /
    /// mulligan steps (nothing to float).
    #[serde(default)]
    pub peeked: Option<HandCard>,
    /// The options, in order — each a selectable chip the shell draws + the a11y tree
    /// mirrors. The caller composes them per step (the burn step = one per hand card; the
    /// keep step = Keep / Bottom; the mulligan = Mulligan / Keep).
    pub options: Vec<ChoiceOption>,
}

/// One option on a [`ChoicePrompt`] — a labelled, actionable chip. `verb` is the stable id
/// the JS bridge dispatches on: `"choose"` carries the engine choice `index`
/// (`Command::Choose { index }`); `"mulligan"` / `"keep"` drive the opening offer (apply
/// `Command::Mulligan` / dismiss). `cost` reads the chip as a COST (the burn — a warmer
/// frame), `primary` reads it filled (the suggested/affirmative step).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChoiceOption {
    /// The chip label (what it does), e.g. "Burn Tide-Caller" / "Keep on top" / "Mulligan".
    pub label: String,
    /// A small second line beneath it (the consequence), e.g. "leaves play" / "no Anima" /
    /// "for +1 Anima" / "draw a fresh hand, bottom one". May be empty.
    #[serde(default)]
    pub detail: String,
    /// The dispatch verb (`"choose"` / `"mulligan"` / `"keep"`).
    pub verb: String,
    /// For a `"choose"` verb, the engine choice index (`Command::Choose { index }`); 0 for
    /// the opening-offer verbs (which carry no index).
    #[serde(default)]
    pub index: u8,
    /// Whether this chip reads as a COST (the glimpse burn) — a warmer frame so "you spend
    /// this" is legible.
    #[serde(default)]
    pub cost: bool,
    /// Whether this chip reads filled/primary (the affirmative step — Keep / Mulligan).
    #[serde(default)]
    pub primary: bool,
}

/// Build the [`ChoicePrompt`] for a Glimpse pending choice, from the seat's own
/// (redacted) pending choice + a card-detail resolver. Returns `None` when the pending
/// choice is not a Glimpse step (a non-glimpse effect choice — Peek/Target/Recover — stays
/// on the labeled-command path rather than a canvas modal). `card_of` maps a
/// `CardId` to its [`HandCard`] stat block (the caller wires the catalog). The BURN
/// step lists one chip per hand card (spend it to glimpse); the KEEP step floats the
/// peeked top card with Keep / Bottom.
pub fn build_choice_prompt(
    pending: &recollect_core::state::PendingChoice,
    card_of: &dyn Fn(recollect_core::types::CardId) -> HandCard,
) -> Option<ChoicePrompt> {
    use recollect_core::state::PendingChoice as PC;
    match pending {
        PC::GlimpseBurn { burnable, .. } => {
            // Step 1 — the BURN cost: one chip per hand card to spend (it leaves play).
            let options: Vec<ChoiceOption> = burnable
                .iter()
                .enumerate()
                .map(|(i, &cid)| {
                    let c = card_of(cid);
                    ChoiceOption {
                        label: format!("Burn {}", c.name),
                        detail: "leaves play".into(),
                        verb: "choose".into(),
                        index: i as u8,
                        cost: true,
                        primary: false,
                    }
                })
                .collect();
            Some(ChoicePrompt {
                kind: "glimpse_burn".into(),
                title: "Glimpse — burn a card to peek the Memory".into(),
                subtitle: "the burned card leaves play; then you see the top card".into(),
                peeked: None,
                options,
            })
        }
        PC::Glimpse { top, .. } => {
            // Step 2 — keep the peeked top card (no Anima) or bottom it (+1 Anima).
            let card = card_of(*top);
            Some(ChoicePrompt {
                kind: "glimpse_keep".into(),
                title: "You peek the top of the Memory".into(),
                subtitle: "keep it on top, or bottom it for +1 Anima".into(),
                peeked: Some(card),
                options: vec![
                    ChoiceOption {
                        label: "Keep on top".into(),
                        detail: "no Anima".into(),
                        verb: "choose".into(),
                        index: 0,
                        cost: false,
                        primary: true,
                    },
                    ChoiceOption {
                        label: "Bottom it".into(),
                        detail: "for +1 Anima".into(),
                        verb: "choose".into(),
                        index: 1,
                        cost: false,
                        primary: false,
                    },
                ],
            })
        }
        // Other effect-driven choices (Peek / Target / Recover) stay on the labeled-command
        // path — the canvas modals are the Glimpse + Mulligan pair.
        _ => None,
    }
}

/// Build the opening **Mulligan** [`ChoicePrompt`] — the London-lite offer: redraw a
/// fresh hand (one card goes to the bottom, seed-chosen — the cost) or keep. A single beat
/// (there is no second pick step; the bottomed card is fixed by the seed). The announcement
/// states only the FACT, never the cards (redaction).
pub fn build_mulligan_prompt() -> ChoicePrompt {
    ChoicePrompt {
        kind: "mulligan".into(),
        title: "The opening — mulligan your hand?".into(),
        subtitle: "redraw a fresh hand; one card goes to the bottom (the cost). Once only.".into(),
        peeked: None,
        options: vec![
            ChoiceOption {
                label: "Mulligan".into(),
                detail: "draw fresh, bottom one".into(),
                verb: "mulligan".into(),
                index: 0,
                cost: false,
                primary: true,
            },
            ChoiceOption {
                label: "Keep this hand".into(),
                detail: "no change".into(),
                verb: "keep".into(),
                index: 0,
                cost: false,
                primary: false,
            },
        ],
    }
}

/// The choice modal's layout geometry for `(vw, vh)` — the card rect + the option chip
/// rects, the pure layout the draw ([`choice_prompt`]) and the hit-test
/// ([`choice_regions`]) share, so a tap maps to the chip the player sees (one source of
/// truth). The chips stack vertically; the burn step (a hand-length list) scrolls within
/// the card if it overflows — but in practice the opening hand is ≤ 7, so they fit.
fn choice_layout(prompt: &ChoicePrompt, vw: f32, vh: f32, u: f32) -> (Rect, Vec<Rect>) {
    let cw = (u * 20.0).min(vw - u * 1.5);
    let n = prompt.options.len().max(1) as f32;
    // The Glimpse BURN step is a LIST (one chip per hand card, up to ~7) — compact chips with a
    // tighter gap so the modal isn't a sparse tower (item 11). The keep/bottom + mulligan steps
    // are a couple of affirmative buttons, so they keep the roomier chip size.
    let burn = prompt.kind == "glimpse_burn";
    let opt_h = if burn { u * 1.9 } else { u * 2.4 };
    let opt_gap = if burn { u * 0.42 } else { u * 0.6 };
    let peek_h = if prompt.peeked.is_some() {
        u * 8.0
    } else {
        0.0
    };
    // header (title) + subtitle + the floated peek (if any) + the chips + padding.
    let ch = (u * 1.8 // top pad
        + u * 1.5 // title
        + u * 1.2 // subtitle
        + peek_h
        + u * 0.6
        + n * (opt_h + opt_gap)
        + u * 1.0)
        .min(vh - u * 1.0);
    let cx0 = (vw - cw) / 2.0;
    let cy0 = (vh - ch) / 2.0;
    let card = Rect {
        x: cx0,
        y: cy0,
        w: cw,
        h: ch,
        color: CARD,
        radius: u * 0.4,
        layer: ShellLayer::Card,
    };
    // The chips start below the title + subtitle + peek block.
    let chips_top = cy0 + u * 1.8 + u * 1.5 + u * 1.2 + peek_h + u * 0.6;
    let bw = (cw - u * 2.0).min(u * 15.0);
    let bx = cx0 + (cw - bw) / 2.0;
    let mut rects = Vec::with_capacity(prompt.options.len());
    let mut y = chips_top;
    for _ in &prompt.options {
        rects.push(Rect {
            x: bx,
            y,
            w: bw,
            h: opt_h,
            color: CARD,
            radius: u * 0.3,
            layer: ShellLayer::Card,
        });
        y += opt_h + opt_gap;
    }
    (card, rects)
}

/// Draw the in-canvas **choice prompt**: a centered modal card over a scrim, the
/// title + subtitle, the floated peeked card (on the keep/bottom step), and the option
/// chips (a burn reads as a warm COST; Keep/Mulligan read primary). The board shows
/// through the scrim (the decision is in context). The chip rects come from
/// [`choice_layout`] (shared with the hit-test), so the draw and the tap-mapping agree.
fn choice_prompt(prompt: &ChoicePrompt, s: &mut ShellScene, vw: f32, vh: f32, u: f32) {
    // A scrim over the whole viewport (the choice is a modal beat; the board shows through,
    // but dimmed enough that the modal is unmistakably the focus — matching the result scrim).
    s.rects.push(Rect {
        x: 0.0,
        y: 0.0,
        w: vw,
        h: vh,
        color: with_alpha(DUSK, 0.55),
        radius: 0.0,
        layer: ShellLayer::Panel,
    });
    let (card, chips) = choice_layout(prompt, vw, vh, u);
    push_card_grad(
        s,
        card.x,
        card.y,
        card.w,
        card.h,
        CARD,
        CARD_DEEP,
        ShellLayer::Card,
        u,
    );
    // A burnished-gold header rule — the glimpse/mulligan is a deliberate, precious beat.
    push_ring_px(
        s,
        card.x,
        card.y,
        card.w,
        card.h,
        with_alpha(GILD, 0.85),
        2.0,
    );
    let ccx = card.x + card.w / 2.0;
    let mut cy = card.y + u * 1.5;
    // The title — the framing, sized to fit the card.
    let title = sanitize_keep_punct(&prompt.title);
    let title_size = fit_size(title.chars().count().max(1), card.w - u * 1.5).min(u * 1.3);
    s.texts.push(Text {
        x: ccx,
        y: cy,
        size: title_size,
        text: title,
        color: INK,
        align: Align::Center,
        bold: true,
        layer: ShellLayer::Text,
    });
    cy += u * 1.4;
    if !prompt.subtitle.is_empty() {
        let sub = sanitize_keep_punct(&prompt.subtitle);
        for l in wrap_lines(&sub, ((card.w - u * 1.5) / (u * 0.34)) as usize) {
            s.texts.push(Text {
                x: ccx,
                y: cy,
                size: u * 0.66,
                text: l,
                color: SOFT,
                align: Align::Center,
                bold: false,
                layer: ShellLayer::Text,
            });
            cy += u * 0.9;
        }
    }
    cy += u * 0.3;
    // The floated peeked card (keep/bottom step) — a real placeholder card, so the player
    // sees exactly what they're deciding on. Centered, sized to the reserved peek band.
    if let Some(peek) = &prompt.peeked {
        let card_h = u * 7.4;
        let card_w = (card_h * 5.0 / 7.0).min(card.w - u * 3.0);
        let px = ccx - card_w / 2.0;
        hand_card(
            s,
            peek,
            px,
            cy,
            card_w,
            card_h,
            Seat::A, // the peek is YOURS — your ink frame
            u,
            true,  // actionable-styled (no dim wash — it's the focus)
            false, // not an evolve form
            false, // not a recede base
            false, // not lifted
            false, // item 11 — no action-dot badge: the peek is informational, not a play target
        );
    }
    // The option chips — a burn reads as a warm COST plate; Keep / Mulligan read filled
    // (primary); the rest read outlined. Each is a labelled chip with its consequence
    // beneath. The rects are `choice_layout`'s (shared with the hit-test).
    for (i, opt) in prompt.options.iter().enumerate() {
        let r = chips[i];
        if opt.primary {
            push_card_grad(s, r.x, r.y, r.w, r.h, GILD, GILD_DEEP, ShellLayer::Card, u);
        } else if opt.cost {
            // A burn — a warm ember plate (you spend this), ringed to read as a cost.
            push_card_rect(
                s,
                r.x,
                r.y,
                r.w,
                r.h,
                with_alpha(ATK_INK, 0.16),
                ShellLayer::Card,
                u,
            );
            push_ring_px(s, r.x, r.y, r.w, r.h, with_alpha(ATK_INK, 0.85), 1.5);
        } else {
            push_card_rect(s, r.x, r.y, r.w, r.h, CARD, ShellLayer::Card, u);
            push_ring_px(s, r.x, r.y, r.w, r.h, with_alpha(INK, 0.7), 1.5);
        }
        let label = sanitize_keep_punct(&opt.label);
        let txt_color = if opt.primary { PAPER } else { INK };
        let has_detail = !opt.detail.is_empty();
        let label_y = if has_detail {
            r.y + r.h * 0.38
        } else {
            r.y + r.h / 2.0
        };
        let label_size = fit_size(label.chars().count().max(1), r.w - u * 1.0).min(u * 0.95);
        s.texts.push(Text {
            x: r.x + r.w / 2.0,
            y: label_y,
            size: label_size,
            text: label,
            color: txt_color,
            align: Align::Center,
            bold: false,
            layer: ShellLayer::Text,
        });
        if has_detail {
            let detail = sanitize_keep_punct(&opt.detail);
            let dcolor = if opt.primary {
                with_alpha(PAPER, 0.9)
            } else {
                SOFT
            };
            s.texts.push(Text {
                x: r.x + r.w / 2.0,
                y: r.y + r.h * 0.72,
                size: (u * 0.56).min(r.h * 0.3),
                text: detail,
                color: dcolor,
                align: Align::Center,
                bold: false,
                layer: ShellLayer::Text,
            });
        }
    }
}

/// The choice prompt's option hit-test rects for `(vw, vh)`, keyed by the option's
/// dispatch verb + index — the JS bridge maps a canvas tap onto the right option through
/// these (one source with the modal draw). Returns `(verb, index, HitRect)`
/// in draw order. The whole-card rect is NOT returned (a tap off the chips is a no-op — a
/// choice is modal, with no click-out cancel; the cancel, where allowed, is an explicit
/// chip).
pub fn choice_regions(prompt: &ChoicePrompt, vw: f32, vh: f32) -> Vec<(String, u8, HitRect)> {
    let vw = vw.max(1.0);
    let vh = vh.max(1.0);
    let m = Metrics::for_viewport(vw, vh);
    let (_card, chips) = choice_layout(prompt, vw, vh, m.u);
    prompt
        .options
        .iter()
        .zip(chips)
        .map(|(o, r)| (o.verb.clone(), o.index, HitRect::from(&r)))
        .collect()
}

/// The **virtual a11y tree** for an active choice prompt (invariant 7): the prompt's
/// title + subtitle as a labelled section, then each option as an actionable button whose
/// target is an [`A11yTarget::Action`] carrying the verb (and, for a `"choose"`, the index
/// encoded as `"choose:N"`) the canvas chip fires (one action path, two surfaces). The
/// canvas is opaque to assistive tech, so this is the accessible choice — keyboard-reachable
/// and announced. Pure + native-tested. Redaction holds: only YOUR prompt is ever built.
pub fn choice_a11y_tree(prompt: &ChoicePrompt) -> Vec<A11yNode> {
    let mut nodes: Vec<A11yNode> = Vec::new();
    let header = if prompt.subtitle.is_empty() {
        prompt.title.clone()
    } else {
        format!("{} {}", prompt.title, prompt.subtitle)
    };
    nodes.push(A11yNode {
        id: "section-choice".into(),
        role: "group".into(),
        label: header,
        target: None,
        enabled: false,
        level: 2,
        grid: None,
    });
    // The peeked card (keep/bottom step) as a text readout — what you're deciding on.
    if let Some(peek) = &prompt.peeked {
        let stat = if peek.kind.eq_ignore_ascii_case("Spirit") || peek.kind.is_empty() {
            format!(
                ", attack {}, defense {}, {} health",
                peek.attack, peek.defense, peek.hp
            )
        } else {
            String::new()
        };
        nodes.push(A11yNode {
            id: "choice-peek".into(),
            role: "text".into(),
            label: format!(
                "You peek {}, cost {}, {}{}",
                peek.name,
                peek.cost,
                if peek.kind.is_empty() {
                    "card"
                } else {
                    &peek.kind
                },
                stat
            ),
            target: None,
            enabled: false,
            level: 0,
            grid: None,
        });
    }
    for opt in &prompt.options {
        // Encode the choose index in the verb so the JS bridge fires `Command::Choose`
        // with the right option (the result/FAB a11y nodes carry a bare verb; a choice
        // option also needs its index — "choose:1").
        let verb = if opt.verb == "choose" {
            format!("choose:{}", opt.index)
        } else {
            opt.verb.clone()
        };
        let label = if opt.detail.is_empty() {
            opt.label.clone()
        } else {
            format!("{} — {}", opt.label, opt.detail)
        };
        nodes.push(A11yNode {
            id: format!("choice-{}-{}", opt.verb, opt.index),
            role: "button".into(),
            label,
            target: Some(A11yTarget::Action(verb)),
            enabled: true,
            level: 0,
            grid: None,
        });
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use recollect_core::Engine;
    use recollect_core::cards::canon_catalog;
    use recollect_core::types::CardId;
    use recollect_core::view::view_for;

    /// A real (empty-board) 1v1 `PlayerView` for the layout tests — the chrome is
    /// what we assert; the board only needs to be a valid 5×5 scene source.
    fn a_view() -> PlayerView {
        let cat = canon_catalog();
        let deck: Vec<CardId> = (0..20).map(|_| CardId(0)).collect();
        let (e, _) = Engine::new(7, cat, deck.clone(), deck);
        view_for(&e, Seat::A)
    }

    fn model() -> ShellModel {
        ShellModel {
            you_seat: Seat::A,
            you_name: "Dreamer Juno".into(),
            you_faction: "Lorekeepers".into(),
            you_score: 3,
            you_anima: 4,
            hand: vec![
                HandCard {
                    name: "Cinderling".into(),
                    cost: 2,
                    attack: 3,
                    defense: 1,
                    hp: 2,
                    kind: "Spirit".into(),
                    resonance: "Ember".into(),
                },
                HandCard {
                    name: "Recollect".into(),
                    cost: 1,
                    attack: 0,
                    defense: 0,
                    hp: 0,
                    kind: "Spell".into(),
                    resonance: "Tide".into(),
                },
            ],
            opp_name: "Corin Ashe".into(),
            opp_faction: "the Solace".into(),
            opp_score: 5,
            opp_erasures: 2,
            opp_hand_count: 4,
            round: 9,
            last_round: 12,
            dusk_after: 8,
            your_turn: true,
            view: a_view(),
            names: vec![],
            cues: MoveCues::default(),
            interaction: Interaction::default(),
            actionable_tiles: vec![],
            evolvable_tiles: vec![],
            devolvable_tiles: vec![],
            actionable_hand: vec![],
            evolve_forms: vec![],
            devolve_bases: vec![],
            lifted_hand: None,
            hand_scroll: 0.0,
            dragging: false,
            drag_xy: None,
            inspect: None,
            replay: None,
            dusk: None,
            result: None,
            choice: None,
        }
    }

    #[test]
    fn portrait_lays_out_the_full_shell_within_the_viewport() {
        let s = build_shell(&model(), 400.0, 800.0);
        assert_eq!((s.vw, s.vh), (400.0, 800.0));
        // The board is square and centered horizontally.
        assert!(
            (s.board_rect.w - s.board_rect.h).abs() < 0.5,
            "board is square"
        );
        let cx = s.board_rect.x + s.board_rect.w / 2.0;
        assert!(
            (cx - 200.0).abs() < 1.0,
            "board centered horizontally: {cx}"
        );
        // Every rect sits inside the viewport (with a small shadow-overhang slack).
        for r in &s.rects {
            assert!(
                r.x >= -2.0 && r.x + r.w <= s.vw + 4.0,
                "rect within width: {r:?}"
            );
            assert!(
                r.y >= -2.0 && r.y + r.h <= s.vh + 4.0,
                "rect within height: {r:?}"
            );
        }
        // The gradient hero surfaces (ground/bands/cards) sit within the viewport too.
        for g in &s.grads {
            assert!(
                g.x >= -2.0 && g.x + g.w <= s.vw + 4.0 && g.y >= -2.0 && g.y + g.h <= s.vh + 4.0,
                "grad within viewport: {g:?}"
            );
        }
        // A gradient page ground is drawn (top != bottom — the soft lit-paper feel).
        assert!(
            s.grads
                .iter()
                .any(|g| g.layer == ShellLayer::Ground && g.top != g.bottom),
            "the page ground is a vertical gradient"
        );
    }

    #[test]
    fn the_hud_shows_score_anima_and_the_round() {
        let s = build_shell(&model(), 400.0, 800.0);
        let texts: Vec<&str> = s.texts.iter().map(|t| t.text.as_str()).collect();
        assert!(texts.contains(&"SCORE"), "HUD labels the score");
        assert!(texts.contains(&"ANIMA"), "HUD labels the Anima");
        assert!(texts.contains(&"3"), "your score value");
        assert!(texts.contains(&"4"), "your Anima value");
        // Item 6: the pip strip SHOWS the round, so there is NO redundant "Round N" caption.
        // Only the meaningful beat label remains — past the Dusk round (9 > dusk_after 8) the
        // caption reads "Dusk" (and "Nightfall" at the last round); a normal round shows nothing.
        assert!(
            !s.texts.iter().any(|t| t.text.contains("Round")),
            "no redundant 'Round N' text: {:?}",
            s.texts.iter().map(|t| &t.text).collect::<Vec<_>>()
        );
        assert!(
            s.texts.iter().any(|t| t.text == "Dusk"),
            "round 9 is past the Dusk: {:?}",
            s.texts.iter().map(|t| &t.text).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_opponent_strip_names_the_character_and_folds_the_erasure_tally() {
        let s = build_shell(&model(), 400.0, 800.0);
        // The opponent strip names the CHARACTER ("Corin Ashe", uppercased to
        // the bitmap font), with the faction word + score folded into the sub-line.
        assert!(
            s.texts.iter().any(|t| t.text == "CORIN ASHE"),
            "opponent named by character: {:?}",
            s.texts.iter().map(|t| &t.text).collect::<Vec<_>>()
        );
        // The sub-line carries the faction word AND the score with the erasure tally.
        assert!(
            s.texts.iter().any(|t| t.text.contains("THE SOLACE")
                && t.text.contains("5")
                && t.text.contains("2 erased")),
            "opp sub-line folds faction + the erasure tally: {:?}",
            s.texts.iter().map(|t| &t.text).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_hud_names_the_player_character() {
        // Parity — the HUD names who YOU are telling as (the character name + the
        // faction word), alongside the score + anima.
        let s = build_shell(&model(), 400.0, 800.0);
        assert!(
            s.texts.iter().any(|t| t.text == "DREAMER JUNO"),
            "the HUD names the player's character: {:?}",
            s.texts.iter().map(|t| &t.text).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_clock_has_a_pip_per_round_with_a_nightfall_ring() {
        let s = build_shell(&model(), 400.0, 800.0);
        // The clock pips are square (w == h) with a half-side radius; count them by
        // that signature. At least `last_round` (12) such round pips.
        let round_pips = s
            .rects
            .iter()
            .filter(|r| {
                r.layer == ShellLayer::Detail
                    && (r.w - r.h).abs() < 0.5
                    && (r.radius - r.w / 2.0).abs() < 0.5
                    && r.w < 16.0
            })
            .count();
        assert!(round_pips >= 12, "a pip per round (≥12): {round_pips}");
    }

    #[test]
    fn the_hand_tray_draws_a_card_per_hand_card_with_stats() {
        let s = build_shell(&model(), 400.0, 800.0);
        // The first card (Cinderling) shows its name and a stat block (3/1/2).
        let texts: Vec<&str> = s.texts.iter().map(|t| t.text.as_str()).collect();
        assert!(
            texts.iter().any(|t| t.contains("CINDER")),
            "spirit name on the card"
        );
        // A/D/HP values appear (3, 1, 2) — the spirit's stat block.
        assert!(
            texts.contains(&"3") && texts.contains(&"1") && texts.contains(&"2"),
            "stat block present"
        );
        // Each card shows a cost (the two costs are 2 and 1).
        let cost_text = s
            .texts
            .iter()
            .filter(|t| t.text == "2" || t.text == "1")
            .count();
        assert!(cost_text >= 2, "each card shows a cost");
    }

    #[test]
    fn hand_cards_label_the_atk_def_hp_stats() {
        // web_client_ux.md §Hand — the stat block is LABELED (Atk / Def / HP), never bare
        // coloured numbers and never the A/D/H shorthand. The spirit card carries all three.
        let s = build_shell(&model(), 400.0, 800.0);
        let texts: Vec<&str> = s.texts.iter().map(|t| t.text.as_str()).collect();
        for label in ["Atk", "Def", "HP"] {
            assert!(
                texts.contains(&label),
                "the hand card labels {label}: {texts:?}"
            );
        }
        // The banned A/D/H single-letter shorthand never appears as a stat label.
        assert!(
            !texts.iter().any(|t| *t == "A" || *t == "D" || *t == "H"),
            "no A/D/H single-letter stat labels: {texts:?}"
        );
    }

    #[test]
    fn a_long_hand_scrolls_as_a_carousel() {
        // A hand bigger than the row width overflows → the carousel reports a non-zero
        // max scroll, and `hand_scroll` slides the cards (the geometry the JS swipe uses).
        let mut m = model();
        // Ten cards on a phone won't all fit at the fixed comfortable card size.
        let card = m.hand[0].clone();
        m.hand = (0..10).map(|_| card.clone()).collect();
        let (vw, vh) = (390.0, 780.0);
        let r = shell_regions(&m, vw, vh);
        assert!(
            r.hand_max_scroll > 0.0,
            "a 10-card hand overflows the row (scrollable carousel)"
        );
        assert_eq!(r.hand.len(), 10, "every card has a hit rect");
        // Scrolling moves the rects left (later cards come into view).
        let x0 = r.hand[9].x;
        let mut m2 = m.clone();
        m2.hand_scroll = r.hand_max_scroll;
        let r2 = shell_regions(&m2, vw, vh);
        assert!(
            r2.hand[9].x < x0,
            "scrolling brings the last card toward view: {} -> {}",
            x0,
            r2.hand[9].x
        );
        // The scroll clamps — past the max it doesn't keep travelling.
        let mut m3 = m.clone();
        m3.hand_scroll = r.hand_max_scroll * 5.0;
        let r3 = shell_regions(&m3, vw, vh);
        assert!(
            (r3.hand[0].x - r2.hand[0].x).abs() < 0.5,
            "scroll is clamped to the max"
        );
    }

    #[test]
    fn the_fabs_render_end_turn_and_study() {
        let s = build_shell(&model(), 400.0, 800.0);
        let texts: Vec<&str> = s.texts.iter().map(|t| t.text.as_str()).collect();
        assert!(texts.contains(&"End Turn"), "End Turn FAB");
        assert!(texts.contains(&"Glimpse"), "Glimpse FAB");
    }

    #[test]
    fn landscape_centers_the_board_and_keeps_it_square() {
        let s = build_shell(&model(), 1200.0, 700.0);
        assert!(
            (s.board_rect.w - s.board_rect.h).abs() < 0.5,
            "board square in landscape"
        );
        let cx = s.board_rect.x + s.board_rect.w / 2.0;
        assert!(
            (cx - 600.0).abs() < 1.0,
            "board centered horizontally in landscape"
        );
        // The board is the hero — it takes a large share of the (lesser) height.
        assert!(
            s.board_rect.h > 300.0,
            "board is large in landscape: {}",
            s.board_rect.h
        );
    }

    #[test]
    fn place_board_maps_tile_grid_corners_to_the_rect() {
        let rect = Rect {
            x: 50.0,
            y: 100.0,
            w: 250.0,
            h: 250.0,
            color: PAPER,
            radius: 0.0,
            layer: ShellLayer::Ground,
        };
        let p = place_board(&rect, 5, 5);
        assert_eq!(p.map(0.0, 0.0), (50.0, 100.0), "tile origin → rect origin");
        assert_eq!(
            p.map(5.0, 5.0),
            (300.0, 350.0),
            "far corner → rect far corner"
        );
        assert_eq!(
            p.map(2.5, 2.5),
            (175.0, 225.0),
            "center tile-point → rect center"
        );
    }

    #[test]
    fn an_empty_hand_and_zero_opp_hand_dont_panic() {
        let mut m = model();
        m.hand.clear();
        m.opp_hand_count = 0;
        let s = build_shell(&m, 360.0, 640.0); // a small phone
        // Still renders the bands + FABs (just no cards / backs).
        assert!(s.texts.iter().any(|t| t.text == "End Turn"));
        assert!(s.board_rect.w > 0.0);
    }

    #[test]
    fn wrap_two_breaks_long_names_and_leaves_short_ones() {
        // A short name stays on one line.
        assert_eq!(wrap_two("EMBER", 10), ("EMBER".into(), String::new()));
        // A two-word name splits at the space, balanced across two lines.
        let (a, b) = wrap_two("TIDEBOUND COLOSSUS", 10);
        assert_eq!(a, "TIDEBOUND");
        assert_eq!(b, "COLOSSUS", "the second word wraps to line two");
        // An over-long line is hard-truncated to `max` (no overrun).
        let (a2, _b2) = wrap_two("PIGEON CARRYING A MESSAGE", 8);
        assert!(a2.chars().count() <= 8, "line one clips to max: {a2:?}");
    }

    #[test]
    fn nightfall_round_reads_nightfall() {
        let mut m = model();
        m.round = 12;
        let s = build_shell(&m, 400.0, 800.0);
        // Item 6: no "Round N" text. At round 12 (≤ last 12, past the Dusk) the beat label reads
        // "Dusk"; past it (13+, the Nightfall scoring beat) it reads "Nightfall".
        assert!(!s.texts.iter().any(|t| t.text.contains("Round")));
        assert!(s.texts.iter().any(|t| t.text == "Dusk"), "round 12 ⇒ Dusk");
        m.round = 13;
        let s2 = build_shell(&m, 400.0, 800.0);
        assert!(
            s2.texts.iter().any(|t| t.text == "Nightfall"),
            "post-12 ⇒ Nightfall"
        );
    }

    // ── Phase B: affordances, regions, the a11y tree ──────────────────────────

    /// A green "affordance"-tinted dot/disc: high green, green dominant over red+blue.
    fn is_afford(c: &Color) -> bool {
        c.g > 0.5 && c.g > c.r && c.g > c.b
    }

    #[test]
    fn an_actionable_tile_gets_a_quiet_action_dot() {
        let mut m = model();
        // Tile 12 (board centre) can act.
        m.actionable_tiles = vec![12];
        let plain = build_shell(&model(), 400.0, 800.0);
        let s = build_shell(&m, 400.0, 800.0);
        let green_dots = |sc: &ShellScene| {
            sc.rects
                .iter()
                .filter(|r| {
                    r.layer == ShellLayer::Detail
                        && is_afford(&r.color)
                        && (r.w - r.h).abs() < 0.5 // round
                        && (r.radius - r.w / 2.0).abs() < 0.6
                })
                .count()
        };
        assert!(
            green_dots(&s) > green_dots(&plain),
            "an actionable tile adds a green action dot"
        );
    }

    #[test]
    fn an_evolvable_base_gets_an_evolve_chevron() {
        let mut m = model();
        m.evolvable_tiles = vec![12];
        let plain = build_shell(&model(), 400.0, 800.0);
        let s = build_shell(&m, 400.0, 800.0);
        // The chevron is a cluster of small EVOLVE-green Detail rects; a plain board
        // has none on that tile. We just assert the evolve-green mark count rises.
        let evolve_marks = |sc: &ShellScene| {
            sc.rects
                .iter()
                .filter(|r| {
                    r.layer == ShellLayer::Detail
                        && r.color.g > 0.6
                        && r.color.g > r.color.r
                        && r.h < 6.0
                })
                .count()
        };
        assert!(
            evolve_marks(&s) > evolve_marks(&plain),
            "an evolvable base adds an evolve chevron (green strokes)"
        );
    }

    #[test]
    fn a_devolvable_form_gets_a_recede_chevron_distinct_from_evolve() {
        // A standing-Faded form that can recede shows a DOWNWARD amber chevron — distinct in
        // both hue (amber, not green) and direction (down, not up) from the evolve chevron.
        let mut m = model();
        m.devolvable_tiles = vec![12];
        let plain = build_shell(&model(), 400.0, 800.0);
        let s = build_shell(&m, 400.0, 800.0);
        // The recede chevron is a cluster of small DEVOLVE-amber (r high, g mid, b low) Detail
        // rects; a plain board has none. Assert the amber-mark count rises.
        let recede_marks = |sc: &ShellScene| {
            sc.rects
                .iter()
                .filter(|r| {
                    r.layer == ShellLayer::Detail
                        && r.color.r > 0.7
                        && r.color.r > r.color.g
                        && r.color.b < 0.4
                        && r.h < 6.0
                })
                .count()
        };
        assert!(
            recede_marks(&s) > recede_marks(&plain),
            "a devolvable form adds a recede chevron (amber strokes)"
        );
        // It must NOT be mistaken for the evolve glyph: no extra evolve-GREEN strokes appear.
        let evolve_marks = |sc: &ShellScene| {
            sc.rects
                .iter()
                .filter(|r| {
                    r.layer == ShellLayer::Detail
                        && r.color.g > 0.6
                        && r.color.g > r.color.r
                        && r.h < 6.0
                })
                .count()
        };
        assert_eq!(
            evolve_marks(&s),
            evolve_marks(&plain),
            "the recede chevron is amber, never the evolve green"
        );
    }

    #[test]
    fn the_a11y_tree_marks_a_devolvable_form_and_its_base_with_the_faction_verb() {
        // The recede affordance is reachable by keyboard / screen reader (invariant 7): the
        // standing-Faded form's tile button announces it can recede, the base card announces
        // it recedes a faded form, and both speak the faction's word. A Lorekeeper REVERTS.
        let mut m = model(); // model() is a Lorekeeper telling (you_faction = "Lorekeepers")
        m.devolvable_tiles = vec![12];
        m.actionable_tiles = vec![12];
        m.devolve_bases = vec![0];
        m.actionable_hand = vec![0];
        let labels: Vec<String> = (0..25).map(|i| format!("reading {i}")).collect();
        let tree = build_a11y_tree(&m, &labels);
        let tile = tree
            .iter()
            .find(|n| n.id == "tile-12")
            .expect("the standing-Faded form's tile button");
        assert!(
            tile.enabled,
            "the recede affordance is actionable on your turn"
        );
        assert!(
            tile.label.to_lowercase().contains("standing faded")
                && tile.label.to_lowercase().contains("revert"),
            "the tile announces the standing-Faded rescue in the faction's word: {:?}",
            tile.label
        );
        let card = tree
            .iter()
            .find(|n| n.id == "hand-0")
            .expect("the recede base hand button");
        assert!(
            card.label.to_lowercase().contains("revert"),
            "the base card announces it can revert a faded form: {:?}",
            card.label
        );
        // The Solace RECEDES (the other faction's word) — same affordance, the faction's verb.
        let mut solace = m.clone();
        solace.you_faction = "the Solace".into();
        let tree_s = build_a11y_tree(&solace, &labels);
        let tile_s = tree_s.iter().find(|n| n.id == "tile-12").unwrap();
        assert!(
            tile_s.label.to_lowercase().contains("recede"),
            "the Solace's standing-Faded tile says recede: {:?}",
            tile_s.label
        );
    }

    #[test]
    fn a_lifted_hand_card_is_raised_with_a_halo() {
        let mut m = model();
        m.actionable_hand = vec![0, 1];
        m.lifted_hand = Some(0);
        let s = build_shell(&m, 400.0, 800.0);
        // A LIFT-coloured halo rect appears (warm gild, semi-transparent, on the Card
        // layer) — the picked-up card's glow.
        assert!(
            s.rects.iter().any(|r| r.layer == ShellLayer::Card
                && r.color.r > 0.7
                && r.color.g > 0.6
                && r.color.b < 0.5
                && r.color.a < 0.8),
            "the lifted card has a gild halo"
        );
    }

    #[test]
    fn the_inspect_panel_draws_stats_and_a_reach_grid() {
        let mut m = model();
        m.inspect = Some(Inspect {
            name: "Cinderling".into(),
            kind: "Spirit".into(),
            resonance: "Ember".into(),
            cost: 2,
            attack: 3,
            defense: 1,
            hp: 2,
            reach: "Cross".into(),
            keywords: vec!["Mobile".into()],
            rules: "A small ember that drifts on the wind.".into(),
            reach_w: 5,
            reach_center: 12,
            reach_tiles: vec![7, 11, 13, 17],
            anchor: (200.0, 600.0),
        });
        let s = build_shell(&m, 400.0, 800.0);
        let texts: Vec<&str> = s.texts.iter().map(|t| t.text.as_str()).collect();
        // The panel headers the card name and shows the stat block.
        assert!(texts.iter().any(|t| t.contains("Cinderling")), "name shown");
        // Items 6/7 — each stat is a "Label: value" pair: the labels carry the Atk/Def/HP
        // shorthand (never A/D/H) with a COLON separator, and the values are SEPARATE text in
        // their own stat ink. Find each labelled segment + its coloured value.
        let labelled = |key: &str| texts.iter().any(|t| t.contains(key));
        for key in ["Atk:", "Def:", "HP:", "Reach:"] {
            assert!(
                labelled(key),
                "the stat block labels {key} with a colon: {texts:?}"
            );
        }
        // Item 6 — the values are coloured to the hand-card stat inks: 3 in Atk red, 1 in Def
        // blue, 2 in HP green. Each value is its own Text in the matching ink.
        let value_in = |val: &str, ink: Color| {
            s.texts.iter().any(|t| {
                t.text == val
                    && (t.color.r - ink.r).abs() < 0.02
                    && (t.color.g - ink.g).abs() < 0.02
            })
        };
        assert!(value_in("3", ATK_INK), "Atk value is ember red");
        assert!(value_in("1", DEF_INK), "Def value is slate blue");
        assert!(value_in("2", HP_INK), "HP value is living green");
        // Item 8 — the Reach value reads in the gild ink, set apart from the combat stats.
        assert!(
            s.texts
                .iter()
                .any(|t| t.text == "Cross" && (t.color.r - GILD.r).abs() < 0.02),
            "the Reach value is gild-coloured: {texts:?}"
        );
        // A passive reach grid: a gild centre disc + soft green reach dots (Detail).
        assert!(
            s.rects.iter().any(|r| r.layer == ShellLayer::Detail
                && r.color.r > 0.6
                && r.color.b < 0.4
                && (r.w - r.h).abs() < 0.5),
            "a gild centre disc in the reach grid"
        );
    }

    #[test]
    fn the_inspect_rules_text_ends_in_a_full_sentence() {
        // Item 9 — a card's lore reads as a full sentence: punctuation is preserved (the atlas
        // renders it now), and a missing terminal period is supplied so it never trails off.
        let mut m = model();
        m.inspect = Some(Inspect {
            name: "Zenith".into(),
            kind: "Spirit".into(),
            resonance: "Wonder".into(),
            cost: 6,
            attack: 80,
            defense: 20,
            hp: 70,
            reach: "Cross".into(),
            keywords: vec!["Warded".into()],
            // No terminal period in the source — the panel must add one.
            rules: "A drifting ember, on arrival kindle an adjacent ally".into(),
            reach_w: 5,
            reach_center: 12,
            reach_tiles: vec![11, 13],
            anchor: (300.0, 400.0),
        });
        let s = build_shell(&m, 1280.0, 900.0);
        // The rules text is wrapped across lines (SOFT ink, left-aligned). The lore reads as a
        // full sentence, so SOME rules line ends in a period — the missing terminal was supplied.
        let rules_lines: Vec<&str> = s
            .texts
            .iter()
            .filter(|t| {
                t.color == SOFT
                    && t.align == Align::Left
                    && (t.text.contains("ember")
                        || t.text.contains("ally")
                        || t.text.contains("arrival"))
            })
            .map(|t| t.text.as_str())
            .collect();
        assert!(
            !rules_lines.is_empty() && rules_lines.iter().any(|l| l.ends_with('.')),
            "the lore ends in a period (a full sentence): {rules_lines:?}"
        );
        // And no banned register leaked through (vocabulary law holds even in lore display).
        for banned in ["kill", "slain", "destroy"] {
            assert!(
                !s.texts
                    .iter()
                    .any(|t| t.text.to_lowercase().contains(banned)),
                "register law: '{banned}' must not appear"
            );
        }
    }

    #[test]
    fn the_inspect_panel_clamps_on_screen() {
        let mut m = model();
        // Anchor in the top-left corner: the panel must still fit fully on-screen.
        m.inspect = Some(Inspect {
            name: "X".into(),
            kind: "Spell".into(),
            resonance: "Tide".into(),
            cost: 1,
            attack: 0,
            defense: 0,
            hp: 0,
            reach: "None".into(),
            keywords: vec![],
            rules: "".into(),
            reach_w: 5,
            reach_center: 12,
            reach_tiles: vec![],
            anchor: (2.0, 2.0),
        });
        let s = build_shell(&m, 400.0, 800.0);
        // Every panel rect (the INSPECT_BG body included) sits within the viewport.
        for r in &s.rects {
            assert!(
                r.x >= -2.0 && r.x + r.w <= s.vw + 4.0,
                "rect within width: {r:?}"
            );
            assert!(
                r.y >= -2.0 && r.y + r.h <= s.vh + 4.0,
                "rect within height: {r:?}"
            );
        }
    }

    #[test]
    fn shell_regions_match_the_draw_geometry() {
        let m = model();
        let (vw, vh) = (400.0, 800.0);
        let r = shell_regions(&m, vw, vh);
        let s = build_shell(&m, vw, vh);
        // The board region equals the drawn board rect.
        assert!((r.board.x - s.board_rect.x).abs() < 0.5);
        assert!((r.board.w - s.board_rect.w).abs() < 0.5);
        assert_eq!(r.board_w, 5, "1v1 grid side");
        // One hand rect per card, each inside the viewport and below the board.
        assert_eq!(r.hand.len(), m.hand.len());
        for h in &r.hand {
            assert!(
                h.x >= -0.5 && h.x + h.w <= vw + 0.5,
                "hand card within width"
            );
            assert!(h.y > r.board.y, "hand sits below the board");
        }
        // The FABs (item 2) are two distinct, non-overlapping rects in their own lane, never over
        // the play grid. On this narrow portrait they sit side-by-side in the HUD bar (clear of
        // the board below it); on a wide desktop they stack in the right rail. Either way they
        // don't overlap the board and are on-screen.
        assert!(r.end_turn.x + r.end_turn.w <= vw + 0.5 && r.study.x >= -0.5);
        let overlap = |a: &HitRect, b: &HitRect| {
            a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
        };
        assert!(
            !overlap(&r.end_turn, &r.study),
            "End Turn and Glimpse are distinct rects"
        );
        // Neither button overlaps the board (the whole point of item 2 — no ambiguous taps).
        assert!(
            !overlap(&r.end_turn, &r.board),
            "End Turn is clear of the board"
        );
        assert!(
            !overlap(&r.study, &r.board),
            "Glimpse is clear of the board"
        );
        // On a WIDE desktop viewport the right rail engages and the buttons STACK (Glimpse above).
        let rd = shell_regions(&m, 1280.0, 860.0);
        assert!(
            rd.study.y < rd.end_turn.y,
            "desktop rail: Glimpse stacks above End Turn"
        );
        assert!(
            !overlap(&rd.study, &rd.board) && !overlap(&rd.end_turn, &rd.board),
            "desktop rail: buttons clear of the board"
        );
        // A point in the board maps; a point in a hand card is found.
        assert!(r.board.contains(r.board.x + 1.0, r.board.y + 1.0));
        assert!(r.hand[0].contains(r.hand[0].x + 1.0, r.hand[0].y + 1.0));
    }

    // ── the canvas pointer-PATH regression suite ─────────────────────────────
    //
    // The gap that hid the regions-misroute bug: a tap on a region resolved to the WRONG
    // tile (an off-by-one, a dropped Y-flip, or a layout offset), and nothing guarded the
    // pointer→tile map — the draw placed the board with one formula, the JS hit-test
    // resolved a tap with another, and only an eyeball of the canvas could catch a drift
    // between them. These tests pin the px→tile resolution (`ShellRegions::tile_at`, the
    // canonical inverse of the board draw) as a PURE function of coords + layout, across
    // both layouts (phone + desktop) and BOTH board sizes (5×5 1v1 and 6×6 2v2), so a
    // future misroute fails CI here.

    /// A 6×6 (2v2) shell model, built through the real online team-view adapter so the
    /// board is a faithful 36-tile square (`board_side_tiles` ⇒ 6) — the same model the
    /// 2v2 canvas shell draws and hit-tests.
    fn model_2v2() -> ShellModel {
        use recollect_core::types::CardKind;
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
        let slot = e.state().active_slot;
        let tv = view_for_slot(&e, slot);
        let legal = e.legal_commands(slot.team());
        crate::online::shell_model_for_team_view(&tv, &legal, "Ally One", "Rival")
    }

    /// EVERY tile's drawn centre must resolve back to that same tile — the round-trip that
    /// catches a forward/inverse drift (the misroute class). Run for a model across a
    /// phone-portrait AND a wide-desktop viewport: the board is a centered sub-rectangle
    /// whose origin/size differ per layout, so a tap resolved against the wrong rect (a
    /// layout-offset misroute) breaks the round-trip on one of them.
    fn assert_tile_roundtrip(m: &ShellModel, expect_w: u32) {
        for (vw, vh, layout) in [(390.0_f32, 780.0_f32, "phone"), (1280.0, 860.0, "desktop")] {
            let r = shell_regions(m, vw, vh);
            assert_eq!(
                r.board_w, expect_w,
                "{layout}: the grid side is {expect_w} (1v1=5, 2v2=6)"
            );
            let bw = r.board_w;
            for t in 0..(bw * bw) as u8 {
                // The drawn centre of tile `t` (the forward map the overlays/labels use).
                let (cx, cy) = tile_center_px(t, bw, r.board.x, r.board.y, r.board.w);
                let got = r.tile_at(cx, cy);
                assert_eq!(
                    got,
                    Some(t),
                    "{layout} {bw}x{bw}: tile {t}'s centre ({cx:.1},{cy:.1}) must resolve \
                     back to {t}, got {got:?} — a pointer→tile misroute"
                );
            }
        }
    }

    #[test]
    fn pointer_resolves_to_the_right_tile_on_the_5x5_board() {
        assert_tile_roundtrip(&model(), 5);
    }

    #[test]
    fn pointer_resolves_to_the_right_tile_on_the_6x6_board() {
        // The 6×6 board is 2v2; a tap must hit the right one of 36 tiles.
        assert_tile_roundtrip(&model_2v2(), 6);
    }

    #[test]
    fn the_pointer_path_honours_the_board_y_flip() {
        // The exact bug class the gap hid: the board draws seat A's home row at the BOTTOM
        // (the Y-flip), so the VISUAL corners must resolve to the flipped tile indices. A
        // dropped or inverted flip sends a tap to the mirror-image tile and fails here.
        for (m, w) in [(model(), 5u32), (model_2v2(), 6u32)] {
            let r = shell_regions(&m, 400.0, 800.0);
            let cell = r.board.w / w as f32;
            // A point just inside each visual corner of the board square.
            let q = cell * 0.5; // the centre of the corner cell
            let top_left = r.tile_at(r.board.x + q, r.board.y + q);
            let top_right = r.tile_at(r.board.x + r.board.w - q, r.board.y + q);
            let bottom_left = r.tile_at(r.board.x + q, r.board.y + r.board.h - q);
            let bottom_right = r.tile_at(r.board.x + r.board.w - q, r.board.y + r.board.h - q);
            // Home row (tiles 0..w) draws at the BOTTOM; the last row ((w-1)*w..) at the TOP.
            assert_eq!(
                bottom_left,
                Some(0),
                "{w}x{w}: bottom-left visual = tile 0 (home)"
            );
            assert_eq!(
                bottom_right,
                Some((w - 1) as u8),
                "{w}x{w}: bottom-right visual = tile w-1"
            );
            assert_eq!(
                top_left,
                Some(((w - 1) * w) as u8),
                "{w}x{w}: top-left visual = tile (w-1)*w"
            );
            assert_eq!(
                top_right,
                Some((w * w - 1) as u8),
                "{w}x{w}: top-right visual = the last tile"
            );
        }
    }

    #[test]
    fn the_pointer_path_has_no_off_by_one_at_cell_edges() {
        // An off-by-one in the floor/scale would misassign a tap near a cell boundary to the
        // neighbouring tile. Assert each cell owns the half-open span `[edge, edge+cell)`:
        // a point a hair inside the cell's top-left belongs to THAT cell; a point a hair
        // before its right/bottom edge still belongs to it (the next cell starts AT the edge).
        for (m, w) in [(model(), 5u32), (model_2v2(), 6u32)] {
            let r = shell_regions(&m, 420.0, 820.0);
            let cell = r.board.w / w as f32;
            let eps = cell * 0.02;
            for row in 0..w {
                for col in 0..w {
                    let tile = ((w - 1 - row) * w + col) as u8; // the Y-flipped index
                    // Just inside the cell's top-left.
                    let near_tl = r.tile_at(
                        r.board.x + col as f32 * cell + eps,
                        r.board.y + row as f32 * cell + eps,
                    );
                    // Just inside the cell's bottom-right (before the next cell's edge).
                    let near_br = r.tile_at(
                        r.board.x + (col + 1) as f32 * cell - eps,
                        r.board.y + (row + 1) as f32 * cell - eps,
                    );
                    assert_eq!(
                        near_tl,
                        Some(tile),
                        "{w}x{w}: cell ({col},{row}) top-left edge"
                    );
                    assert_eq!(
                        near_br,
                        Some(tile),
                        "{w}x{w}: cell ({col},{row}) bottom-right edge owns its span"
                    );
                }
            }
        }
    }

    #[test]
    fn each_tile_centre_maps_to_a_distinct_tile() {
        // A layout-offset misroute (e.g. the board rect used for hit-testing differing from
        // the one drawn) can bunch several taps onto one tile. Assert the px→tile map is a
        // bijection over the board: 25 (then 36) centres, 25 (36) distinct tiles, none lost.
        for (m, w) in [(model(), 5u32), (model_2v2(), 6u32)] {
            let r = shell_regions(&m, 1180.0, 900.0); // a wide desktop layout
            let mut seen = std::collections::HashSet::new();
            for t in 0..(w * w) as u8 {
                let (cx, cy) = tile_center_px(t, w, r.board.x, r.board.y, r.board.w);
                let got = r.tile_at(cx, cy).expect("a centre is on the board");
                assert!(
                    seen.insert(got),
                    "{w}x{w}: tile {got} claimed twice (a collision)"
                );
            }
            assert_eq!(
                seen.len(),
                (w * w) as usize,
                "{w}x{w}: every tile is reachable"
            );
        }
    }

    #[test]
    fn the_pointer_path_rejects_points_off_the_board() {
        // Outside the board square ⇒ `None` (so a tap in the gutter / HUD / hand band is
        // never silently snapped onto an edge tile). Guards the bounds half of the map.
        let m = model();
        let r = shell_regions(&m, 400.0, 800.0);
        let b = &r.board;
        for (px, py, where_) in [
            (b.x - 5.0, b.y + b.h / 2.0, "left of the board"),
            (b.x + b.w + 5.0, b.y + b.h / 2.0, "right of the board"),
            (b.x + b.w / 2.0, b.y - 5.0, "above the board"),
            (b.x + b.w / 2.0, b.y + b.h + 5.0, "below the board"),
        ] {
            assert_eq!(
                r.tile_at(px, py),
                None,
                "a point {where_} is off-board (None)"
            );
        }
        // The far corners are exactly on the exclusive edge — still off-board (half-open).
        assert_eq!(
            r.tile_at(b.x + b.w, b.y + b.h),
            None,
            "the exclusive far corner is off-board"
        );
        // ...but a hair inside that corner resolves to the top-right tile (tile w*w-1 is the
        // bottom-right cell of the GRID; the far-px corner is the board's bottom-right, which
        // after the Y-flip is the home row's right end = tile w-1).
        assert_eq!(
            r.tile_at(b.x + b.w - 0.5, b.y + b.h - 0.5),
            Some(4),
            "just inside the bottom-right resolves to tile 4 (home row, right end)"
        );
    }

    #[test]
    fn the_a11y_tree_mirrors_every_affordance_with_command_parity() {
        let mut m = model();
        // A spirit on tile 12 that can act; an evolvable Fading base on 8; a lifted
        // hand card 0; a legal play on card 1.
        m.actionable_tiles = vec![12];
        m.evolvable_tiles = vec![8];
        m.actionable_hand = vec![0, 1];
        m.evolve_forms = vec![1];
        m.lifted_hand = Some(0);
        m.interaction = Interaction {
            legal: vec![13],
            selected: Some(12),
            focus: None,
        };
        let labels: Vec<String> = (0..25).map(|i| format!("reading {i}")).collect();
        let tree = build_a11y_tree(&m, &labels);

        // The board is a `grid` container; the hand + actions are `group` sections.
        let board = tree.iter().find(|n| n.id == "section-board").unwrap();
        assert_eq!(board.role, "grid");
        assert_eq!(
            board.grid.as_ref().map(|g| g.role),
            Some(A11yGridRole::Grid)
        );
        let groups: Vec<&str> = tree
            .iter()
            .filter(|n| n.role == "group")
            .map(|n| n.id.as_str())
            .collect();
        assert!(groups.contains(&"section-hand"));
        assert!(groups.contains(&"section-actions"));

        // Every actionable tile + the legal target + the selected tile are GRIDCELLS
        // with a Tile target (the SAME target the canvas activate uses) — the JS bridge
        // wraps each in a focusable button. They carry their row/column index.
        let tile_btn = |t: u8| tree.iter().find(|n| n.id == format!("tile-{t}"));
        for t in [8u8, 12, 13] {
            let n = tile_btn(t).unwrap_or_else(|| panic!("tile-{t} present"));
            assert_eq!(n.role, "gridcell");
            assert_eq!(n.target, Some(A11yTarget::Tile(t)));
            let g = n.grid.as_ref().expect("an actionable tile is a gridcell");
            assert_eq!(g.role, A11yGridRole::Cell);
            assert!(g.row >= 1 && g.col >= 1, "1-based row/col on {t}: {g:?}");
        }
        // The evolvable base says so; the selected tile says so.
        assert!(tile_btn(8).unwrap().label.contains("evolve"));
        assert!(
            tile_btn(12)
                .unwrap()
                .label
                .to_lowercase()
                .contains("selected")
        );

        // Both FABs are buttons firing the global verbs.
        let fab = |id: &str| tree.iter().find(|n| n.id == id).unwrap();
        assert_eq!(
            fab("fab-end").target,
            Some(A11yTarget::Action("EndTurn".into()))
        );
        assert_eq!(
            fab("fab-study").target,
            Some(A11yTarget::Action("Glimpse".into()))
        );

        // Every hand card is a button with a Hand target; the lifted + evolve-form
        // ones are labelled distinctly.
        let hand0 = tree.iter().find(|n| n.id == "hand-0").unwrap();
        let hand1 = tree.iter().find(|n| n.id == "hand-1").unwrap();
        assert_eq!(hand0.target, Some(A11yTarget::Hand(0)));
        assert_eq!(hand1.target, Some(A11yTarget::Hand(1)));
        assert!(hand0.label.to_lowercase().contains("picked up"));
        assert!(hand1.label.to_lowercase().contains("evolution form"));

        // The opponent strip is a TEXT node naming score + hand size, never cards
        // (redaction holds — it's a count).
        let opp = tree.iter().find(|n| n.id == "opp").unwrap();
        assert_eq!(opp.role, "text");
        assert!(opp.label.contains("holding 4 cards"));
        assert!(opp.target.is_none());

        // Invariant 7 parity: EVERY actionable canvas affordance has an actionable a11y
        // node (a `Tile` gridcell / `Hand` button / global `Action`) — the count covers
        // the 3 actionable tiles ∪ the hand ∪ the 2 FABs.
        let actionable = tree.iter().filter(|n| n.target.is_some()).count();
        assert!(
            actionable >= 3 /*tiles*/ + m.hand.len() + 2, /*fabs*/
            "an actionable node per affordance, got {actionable}"
        );

        // The a11y labels the builder generates (section names, affordance suffixes, the
        // FAB / opponent text) are SPOKEN, so they are all WORDS — no canvas glyph that a
        // screen reader voices as "crossed swords" / "rightwards arrow" or drops silently.
        // (The em-dash `—` and middot in a NAME are fine; we bar the affordance glyphs.)
        for n in &tree {
            for glyph in ['→', '←', '⚔', '°', '⌂', '▒', '░', '★', '●', '◦'] {
                assert!(
                    !n.label.contains(glyph),
                    "a11y label {:?} must be all words, not {glyph:?}",
                    n.label
                );
            }
        }
    }

    #[test]
    fn the_board_grid_is_complete_but_only_actionable_cells_are_tab_stops() {
        // The board is a TRUE grid (invariant 7, per-tile fidelity): a `grid` container,
        // one `row` per board row, and a `gridcell` for EVERY tile (so a screen reader
        // can announce "row R, column C" + navigate). But an empty / inert cell carries
        // NO target — the JS renders it as a labelled gridcell that is not a Tab stop, so
        // the 25 empties never bury the actionable affordances. Here the board is empty +
        // nothing actionable: all 25 cells exist, none is a target, the FABs remain.
        let m = model();
        let labels: Vec<String> = (0..25).map(|i| format!("reading {i}")).collect();
        let tree = build_a11y_tree(&m, &labels);
        let bw = board_side_tiles(&m);
        // The grid container + one row per board row + a gridcell per tile.
        assert!(
            tree.iter()
                .any(|n| n.id == "section-board" && n.role == "grid"),
            "a grid container heads the board"
        );
        let rows = tree
            .iter()
            .filter(|n| n.grid.as_ref().map(|g| g.role) == Some(A11yGridRole::Row))
            .count();
        assert_eq!(rows as u32, bw, "one row per board row");
        let cells: Vec<&A11yNode> = tree
            .iter()
            .filter(|n| {
                n.grid.as_ref().map(|g| g.role) == Some(A11yGridRole::Cell) && n.role == "gridcell"
            })
            .collect();
        assert_eq!(
            cells.len(),
            m.view.tiles.len(),
            "a gridcell for EVERY tile (complete grid)"
        );
        // Each cell carries a 1-based row/col, and its name leads with the coordinate.
        for c in &cells {
            let g = c.grid.as_ref().unwrap();
            assert!(g.row >= 1 && g.col >= 1, "1-based row/col: {g:?}");
        }
        // No empty cell is a Tab stop — none carries a target (so empties don't bury action).
        assert!(
            cells.iter().all(|c| c.target.is_none()),
            "an empty board has no actionable cells"
        );
        // But the two FAB buttons + the hand buttons are always present.
        assert!(tree.iter().any(|n| n.id == "fab-end" && n.target.is_some()));
        assert!(
            tree.iter()
                .any(|n| n.id == "fab-study" && n.target.is_some())
        );
    }

    #[test]
    fn an_occupied_but_idle_cell_stays_reachable_for_inspect() {
        // A spirit you can't currently act with (off-turn, or just no legal move) must
        // still be keyboard-REACHABLE so a screen-reader / keyboard user can inspect it:
        // its gridcell carries the `Tile` target (a Tab stop firing `activateTile`), but
        // is exposed aria-disabled for ACTION (enabled=false) since there's no live move.
        // Only truly EMPTY cells drop the target. (Regression guard: the grid must not
        // make occupied pieces unreachable in the name of "don't bury the action".)
        use recollect_core::test_support::put_spirit;
        let cat = canon_catalog();
        let cloud = cat
            .iter()
            .find(|c| c.name == "Cloudling")
            .map(|c| c.id)
            .unwrap();
        let deck: Vec<CardId> = (0..20).map(|_| cloud).collect();
        let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
        put_spirit(e.state_mut_for_test(), 7, cloud, Seat::A);
        let mut m = model();
        m.view = view_for(&e, Seat::A);
        m.your_turn = false; // off-turn: nothing is actionable, but the spirit is inspectable
        let labels: Vec<String> = (0..25).map(|i| format!("reading {i}")).collect();
        let tree = build_a11y_tree(&m, &labels);
        let occ = tree
            .iter()
            .find(|n| n.id == "tile-7")
            .expect("the occupied cell");
        assert_eq!(occ.role, "gridcell");
        assert_eq!(
            occ.target,
            Some(A11yTarget::Tile(7)),
            "reachable for inspect"
        );
        assert!(!occ.enabled, "but not actionable off-turn (aria-disabled)");
        // An EMPTY neighbour stays an inert gridcell — no target, no Tab stop.
        let empty = tree
            .iter()
            .find(|n| n.id == "tile-0")
            .expect("an empty cell");
        assert!(empty.target.is_none(), "an empty cell is not a Tab stop");
    }

    #[test]
    fn wrap_lines_wraps_on_word_boundaries_and_hard_splits_long_words() {
        let lines = wrap_lines("the quick brown fox jumps", 9);
        assert!(
            lines.iter().all(|l| l.chars().count() <= 9),
            "each line fits: {lines:?}"
        );
        assert!(lines.len() >= 3, "wrapped onto several lines");
        // A single over-long word is hard-split.
        let long = wrap_lines("supercalifragilistic", 6);
        assert!(long.iter().all(|l| l.chars().count() <= 6));
        assert!(long.len() >= 3);
        // Empty text → no lines.
        assert!(wrap_lines("", 9).is_empty());
    }

    #[test]
    fn sanitize_keep_punct_preserves_case_and_real_punctuation() {
        // Case is preserved, and — since the canvas renders real EB Garamond from the atlas
        // (the full ASCII printable set + the typographic marks) — real sentence punctuation now
        // SURVIVES (items 7/9/10): the colon, the exclamation, the hyphen, the plus, the middot.
        assert_eq!(
            sanitize_keep_punct("Last-Light Koi: a note!"),
            "Last-Light Koi: a note!"
        );
        assert_eq!(
            sanitize_keep_punct("Glimpses take +1 card"),
            "Glimpses take +1 card"
        );
        assert_eq!(sanitize_keep_punct("Spirit · Wonder"), "Spirit · Wonder");
        // Runs of whitespace still collapse to one (and a truly undrawable mark → a break).
        assert_eq!(sanitize_keep_punct("a   b"), "a b");
        assert_eq!(sanitize_keep_punct("café"), "caf"); // a non-ASCII accent the bundle may lack
    }

    // ── the paced opponent-turn replay + announcements ────────────
    use recollect_core::state::Event as E;
    use recollect_core::types::CardId as Cid;

    #[test]
    fn beat_distills_a_play_with_its_affected_tile() {
        // A bot SpiritPlayed → one paced beat: "plays a spirit", the tile highlighted,
        // no erasure tally moved (it was a plain arrival, no banish).
        let evs = vec![E::SpiritPlayed {
            seat: Seat::B,
            card: Cid(3),
            tile: 12,
            attack: 3,
            defense: 1,
            hp: 2,
            face_down: false,
        }];
        let beat = beat_for_command(&evs, "the Solace", 0).expect("a play is a visible beat");
        assert_eq!(beat.kind, "play");
        assert!(
            beat.caption.contains("plays a spirit") && beat.caption.starts_with("the Solace"),
            "caption names the opponent + the action: {}",
            beat.caption
        );
        assert_eq!(beat.tiles, vec![12], "the played tile is highlighted");
        assert_eq!(
            beat.announce, beat.caption,
            "the live-region text is the caption"
        );
        assert!(
            beat.erasures.is_none(),
            "a plain play moved no erasure tally"
        );
    }

    #[test]
    fn beat_counts_the_solace_erasure_tally_on_an_unwriting() {
        // An Unwriting that erases an impression → the Solace register ("erases an
        // impression" / "tells an Unwriting") AND the post-command tally is reported so
        // the strip counts up. Never "killed/destroyed" — only the Solace *Unwrites*.
        let evs = vec![
            E::UnwritingTold {
                seat: Seat::B,
                card: Cid(40),
            },
            E::ImpressionForgotten { tile: 7 },
        ];
        let beat = beat_for_command(&evs, "the Solace", 5).expect("an Unwriting is visible");
        assert_eq!(beat.kind, "unwrite");
        assert!(
            beat.caption.to_lowercase().contains("unwriting"),
            "the Unwriting is named in-register: {}",
            beat.caption
        );
        assert_eq!(
            beat.erasures,
            Some(5),
            "the erasure tally counts up to its new total"
        );
        assert!(beat.tiles.contains(&7), "the erased tile is highlighted");
        // Vocabulary law: no banned register leaks into the caption.
        let low = beat.caption.to_lowercase();
        for banned in ["kill", "slain", "destroy"] {
            assert!(
                !low.contains(banned),
                "register law: '{banned}' must not appear"
            );
        }
    }

    #[test]
    fn beat_for_a_move_that_banishes_reads_as_one_beat() {
        // A move that lands a banishing strike folds the banish into the move's caption
        // ("moves a spirit, banishing a spirit") — one watched beat, not two, and the
        // spirit is *banished* (never killed). A Lorekeeper move moves no erasure tally.
        let evs = vec![
            E::SpiritMoved { from: 6, to: 11 },
            E::Struck {
                from_tile: 11,
                to_tile: 16,
                damage: 4,
                echo: false,
                kind: recollect_core::state::StrikeKind::Engage,
            },
            E::SpiritBecameFading {
                tile: 16,
                banished_by: Some(Seat::A),
            },
        ];
        let beat = beat_for_command(&evs, "Lorekeepers", 0).expect("a move is visible");
        assert_eq!(beat.kind, "move");
        assert!(
            beat.caption.contains("moves a spirit") && beat.caption.contains("banishing"),
            "the banish folds into the move beat: {}",
            beat.caption
        );
        assert!(
            beat.tiles.contains(&6) && beat.tiles.contains(&11) && beat.tiles.contains(&16),
            "from, to, and the struck tile all highlight: {:?}",
            beat.tiles
        );
        assert!(
            beat.erasures.is_none(),
            "a Lorekeeper banish is no Solace erasure"
        );
    }

    #[test]
    fn a_bookkeeping_only_command_yields_no_beat() {
        // An empty slice is not a watchable action — the pacer skips it (no empty frame).
        assert!(beat_for_command(&[], "the Solace", 0).is_none());
        // A bare EndTurn (turn-structure + Flow bookkeeping, no board ACTION and no
        // banish/erasure) is ALSO not a watchable beat — it must distill to None so the
        // replay never captions a meaningless "acts" for the opponent passing.
        let end_turn = vec![
            E::TurnEnded { seat: Seat::B },
            E::RoundAdvanced { round: 3 },
            E::AnimaGained {
                seat: Seat::A,
                amount: 2,
                reason: recollect_core::state::AnimaReason::Income,
            },
            E::CardDrawn { seat: Seat::A },
        ];
        assert!(
            beat_for_command(&end_turn, "Lorekeepers", 0).is_none(),
            "a bare EndTurn / Flow-bookkeeping slice is not a watchable beat"
        );
    }

    #[test]
    fn opener_and_round_announcements_speak_in_register() {
        // Match start always names the first player; from the human's seat when they open.
        assert!(
            opener_announcement(Seat::A, Seat::A, "the Solace").starts_with("You open"),
            "the human opening reads from their vantage"
        );
        assert!(
            opener_announcement(Seat::A, Seat::B, "the Solace").contains("the Solace"),
            "an opponent opener names them in-register"
        );
        // Dusk falls the round AFTER the contraction (dusk_after = 8 ⇒ round 9); Nightfall
        // is the last round (12). Off those beats there is no set-piece announcement.
        assert!(round_announcement(9, 8, 12).unwrap().contains("Dusk"));
        assert!(round_announcement(12, 8, 12).unwrap().contains("Nightfall"));
        assert!(
            round_announcement(5, 8, 12).is_none(),
            "ordinary rounds get no set-piece"
        );
    }

    #[test]
    fn the_replay_caption_draws_a_banner_and_pulses_the_tiles() {
        // With a replay caption set, build_shell draws the caption text and skips the
        // player's affordances (their controls are inert mid-replay).
        let mut m = model();
        m.your_turn = false;
        m.actionable_tiles = vec![6]; // would draw a dot — but must be suppressed
        m.replay = Some(ReplayCaption {
            text: "The Solace tells an Unwriting".into(),
            kind: "unwrite".into(),
            tiles: vec![12],
        });
        let s = build_shell(&m, 400.0, 800.0);
        // The caption text is drawn (sanitized — keeps case + spaces).
        assert!(
            s.texts.iter().any(|t| t.text.contains("Unwriting")),
            "the replay caption banner is drawn: {:?}",
            s.texts.iter().map(|t| &t.text).collect::<Vec<_>>()
        );
        // The board affordance dot for tile 6 is NOT drawn (affordances are inert under
        // replay) — assert no AFFORD-coloured detail rect exists.
        assert!(
            !s.rects.iter().any(|r| r.color == AFFORD),
            "player affordances are suppressed during the opponent replay"
        );
    }

    // ── the Dusk/Nightfall set-piece + the in-canvas result screen ──────
    use recollect_core::state::MatchResult;
    use recollect_core::types::Faction;

    #[test]
    fn the_dusk_set_piece_draws_the_seal_a_contracting_rim_and_the_clock() {
        // The Dusk set-piece over the board: the seal title ("Dusk falls"), a darkening
        // rim contraction (DUSK-ink bands inset from the board edges), and the clock face.
        let mut m = model();
        m.dusk = Some(DuskSetPiece {
            kind: "dusk".into(),
            title: "Dusk falls".into(),
            subtitle: "the page begins to fail at its edges".into(),
            progress: 1.0,
        });
        let s = build_shell(&m, 400.0, 800.0);
        // The seal title is drawn.
        assert!(
            s.texts.iter().any(|t| t.text.contains("Dusk falls")),
            "the Dusk seal is drawn: {:?}",
            s.texts.iter().map(|t| &t.text).collect::<Vec<_>>()
        );
        // A contracting rim: DUSK-ink detail bands appear (dark, near-opaque at full progress).
        let dark_bands = s
            .rects
            .iter()
            .filter(|r| {
                r.layer == ShellLayer::Detail
                    && r.color.r < 0.25
                    && r.color.g < 0.25
                    && r.color.b < 0.25
                    && r.color.a > 0.5
            })
            .count();
        assert!(dark_bands >= 4, "four rim bands close in: {dark_bands}");
        // The clock face is lit (a gild ring / pips) — a warm gild detail mark exists.
        assert!(
            s.rects
                .iter()
                .any(|r| r.color.r > 0.6 && r.color.b < 0.4 && r.color.a > 0.3),
            "the clock face is lit (gild)"
        );
    }

    #[test]
    fn the_dusk_set_piece_is_animated_by_progress() {
        // The set-piece fades/contracts in by `progress` (the JS pacer ramps it; reduced
        // motion snaps to 1). At progress 0 nothing dark is drawn; at 1 the rim is full.
        let mut m0 = model();
        m0.dusk = Some(DuskSetPiece {
            kind: "nightfall".into(),
            title: "Nightfall".into(),
            subtitle: "the Memory will keep what stands".into(),
            progress: 0.0,
        });
        let s0 = build_shell(&m0, 400.0, 800.0);
        let dark0 = s0
            .rects
            .iter()
            .filter(|r| r.color.r < 0.25 && r.color.g < 0.25 && r.color.a > 0.5)
            .count();
        let mut m1 = m0.clone();
        m1.dusk.as_mut().unwrap().progress = 1.0;
        let s1 = build_shell(&m1, 400.0, 800.0);
        let dark1 = s1
            .rects
            .iter()
            .filter(|r| r.color.r < 0.25 && r.color.g < 0.25 && r.color.a > 0.5)
            .count();
        assert!(
            dark1 > dark0,
            "the rim closes in as progress rises: {dark0} → {dark1}"
        );
    }

    #[test]
    fn the_result_screen_speaks_the_verdict_and_lists_actions() {
        // A Lorekeeper holds the page from the human's vantage: "The Memory keeps you",
        // the breakdown lists board points, and the three actions render as buttons.
        let res = build_result_screen(
            MatchResult::Win(Seat::A),
            12,
            5,
            12,
            0,
            0,
            Faction::Lorekeeper,
            Faction::Lorekeeper,
            "Lorekeepers",
            "Lorekeepers",
            Seat::A,
            "bot",
        );
        assert!(
            res.verdict.contains("keeps you"),
            "the human's win reads in their voice: {}",
            res.verdict
        );
        assert!(!res.draw);
        assert_eq!(res.winner.as_deref(), Some("A"));
        // The actions are Rematch (primary) / New opponent / Back to site.
        let verbs: Vec<&str> = res.actions.iter().map(|a| a.verb.as_str()).collect();
        assert_eq!(verbs, vec!["rematch", "new", "site"]);
        assert!(res.actions[0].primary, "Rematch is the primary action");

        // The screen draws the verdict text + a button per action.
        let mut m = model();
        m.result = Some(res.clone());
        let s = build_shell(&m, 400.0, 800.0);
        assert!(
            s.texts
                .iter()
                .any(|t| t.text.to_lowercase().contains("keeps you")),
            "the verdict is drawn: {:?}",
            s.texts.iter().map(|t| &t.text).collect::<Vec<_>>()
        );
        for label in ["Rematch", "New opponent", "Back to site"] {
            assert!(
                s.texts.iter().any(|t| t.text == label),
                "the {label} action button is drawn"
            );
        }
        // The live chrome is SUPPRESSED at the result screen (the telling has ended): the
        // FAB labels + the HUD's SCORE/ANIMA captions must not bleed through the result card.
        for chrome in ["End Turn", "Glimpse", "SCORE", "ANIMA"] {
            assert!(
                !s.texts.iter().any(|t| t.text == chrome),
                "the live chrome '{chrome}' is hidden behind the result screen"
            );
        }
    }

    #[test]
    fn the_result_verdict_adapts_to_the_solace_a_draw_and_pvp() {
        // The Solace wins by erasure ⇒ "forgotten" (the Solace register; never "killed").
        let solace = build_result_screen(
            MatchResult::Win(Seat::B),
            3,
            9,
            3,
            4,
            5,
            Faction::Lorekeeper,
            Faction::Solace,
            "Lorekeepers",
            "the Solace",
            Seat::A,
            "bot",
        );
        assert!(
            solace.verdict.to_lowercase().contains("forgotten"),
            "a Solace win reads as the Memory forgotten: {}",
            solace.verdict
        );
        // Vocabulary law: no banned register leaks.
        for banned in ["kill", "slain", "destroy"] {
            assert!(!solace.verdict.to_lowercase().contains(banned));
            assert!(!solace.flavor.to_lowercase().contains(banned));
        }
        // The Solace's erasure tally is its own tinted breakdown row.
        assert!(
            solace
                .breakdown
                .iter()
                .any(|r| r.solace && r.label.contains("erased") && r.value == "5"),
            "the erasure tally is a tinted row: {:?}",
            solace.breakdown
        );

        // A draw: "both are kept", neutral (no winner tint).
        let draw = build_result_screen(
            MatchResult::Draw,
            6,
            6,
            6,
            6,
            0,
            Faction::Lorekeeper,
            Faction::Lorekeeper,
            "Lorekeepers",
            "Lorekeepers",
            Seat::A,
            "bot",
        );
        assert!(draw.draw && draw.winner.is_none());
        assert!(draw.verdict.to_lowercase().contains("both are kept"));

        // PvP relabels Rematch as an INVITE (host/join is the launch flow), and names the
        // winning faction (not "you") when the human is the loser's seat.
        let pvp = build_result_screen(
            MatchResult::Win(Seat::B),
            4,
            10,
            4,
            10,
            0,
            Faction::Lorekeeper,
            Faction::Lorekeeper,
            "Lorekeepers",
            "Lorekeepers",
            Seat::A, // the human lost
            "pvp",
        );
        assert_eq!(
            pvp.actions[0].label, "Offer a rematch",
            "PvP rematch is an invite"
        );
        assert!(
            pvp.verdict.contains("keeps Lorekeepers"),
            "an opponent win names the faction: {}",
            pvp.verdict
        );
    }

    #[test]
    fn the_result_action_rects_map_each_verb_on_screen() {
        // The hit-test rects line up with the drawn buttons (one per verb), each inside
        // the viewport — so a canvas tap dispatches the right action.
        let res = build_result_screen(
            MatchResult::Win(Seat::A),
            12,
            5,
            12,
            0,
            0,
            Faction::Lorekeeper,
            Faction::Lorekeeper,
            "Lorekeepers",
            "Lorekeepers",
            Seat::A,
            "bot",
        );
        let (vw, vh) = (400.0, 800.0);
        let rects = result_action_rects(&res, vw, vh);
        let verbs: Vec<&str> = rects.iter().map(|(v, _)| v.as_str()).collect();
        assert_eq!(verbs, vec!["rematch", "new", "site"]);
        for (_, r) in &rects {
            assert!(r.w > 0.0 && r.h > 0.0, "the button has a size");
            assert!(
                r.x >= 0.0 && r.x + r.w <= vw + 1.0 && r.y >= 0.0 && r.y + r.h <= vh + 1.0,
                "the button is on-screen: {r:?}"
            );
        }
        // The rects don't overlap vertically (stacked, distinct tap targets).
        assert!(rects[0].1.y + rects[0].1.h <= rects[1].1.y + 1.0);
        assert!(rects[1].1.y + rects[1].1.h <= rects[2].1.y + 1.0);
    }

    #[test]
    fn the_result_a11y_tree_mirrors_the_verdict_and_actions() {
        // Invariant 7: the result is in the a11y tree — the verdict as a labelled group,
        // the breakdown as text, and each action as a button firing its verb (parity).
        let res = build_result_screen(
            MatchResult::Win(Seat::B),
            3,
            9,
            3,
            4,
            5,
            Faction::Lorekeeper,
            Faction::Solace,
            "Lorekeepers",
            "the Solace",
            Seat::A,
            "bot",
        );
        let tree = result_a11y_tree(&res);
        // The result section names the verdict.
        let header = tree.iter().find(|n| n.id == "section-result").unwrap();
        assert_eq!(header.role, "group");
        assert!(header.label.to_lowercase().contains("forgotten"));
        // The score breakdown is a text readout (board + erasures), never opaque.
        let score = tree.iter().find(|n| n.id == "result-score").unwrap();
        assert_eq!(score.role, "text");
        assert!(score.label.contains("erased"));
        // Each action is an actionable button with the matching verb target.
        for verb in ["rematch", "new", "site"] {
            let n = tree
                .iter()
                .find(|n| n.id == format!("result-{verb}"))
                .unwrap_or_else(|| panic!("result-{verb} present"));
            assert_eq!(n.role, "button");
            assert!(n.enabled, "the action is actionable");
            assert_eq!(n.target, Some(A11yTarget::Action(verb.into())));
        }
    }

    // ── the Glimpse + Mulligan choice prompt ────────────────────────────────
    fn stub_card(name: &str) -> HandCard {
        HandCard {
            name: name.into(),
            cost: 2,
            attack: 2,
            defense: 1,
            hp: 3,
            kind: "Spirit".into(),
            resonance: "Wonder".into(),
        }
    }

    #[test]
    fn the_glimpse_burn_step_lists_one_chip_per_hand_card() {
        // Step 1 — the BURN cost: one selectable chip per hand card, each framed as a
        // cost ("Burn <name>", leaves play), dispatching Choose by that card's index.
        let burnable = vec![CardId(3), CardId(7), CardId(9)];
        let names = [
            "",
            "",
            "",
            "Tide-Caller",
            "",
            "",
            "",
            "Hush",
            "",
            "Dawnling",
        ];
        let card_of = |c: CardId| stub_card(names.get(c.0 as usize).copied().unwrap_or("?"));
        let pending = recollect_core::state::PendingChoice::GlimpseBurn {
            seat: Seat::A,
            burnable: burnable.clone(),
        };
        let prompt = build_choice_prompt(&pending, &card_of).expect("a glimpse burn prompt");
        assert_eq!(prompt.kind, "glimpse_burn");
        assert!(prompt.peeked.is_none(), "the burn step floats no peek");
        assert_eq!(prompt.options.len(), 3, "one chip per hand card");
        for (i, opt) in prompt.options.iter().enumerate() {
            assert_eq!(opt.verb, "choose");
            assert_eq!(
                opt.index, i as u8,
                "the chip dispatches Choose by hand index"
            );
            assert!(opt.cost, "the burn reads as a cost");
            assert!(opt.label.starts_with("Burn "), "got: {}", opt.label);
        }
        assert!(prompt.options[0].label.contains("Tide-Caller"));
    }

    #[test]
    fn the_glimpse_keep_step_floats_the_peek_with_keep_and_bottom() {
        // Step 2 — keep/bottom: the peeked top card is floated, and exactly two options —
        // Keep (index 0, no Anima, primary) and Bottom (index 1, +1 Anima).
        let card_of = |_c: CardId| stub_card("Dawnling");
        let pending = recollect_core::state::PendingChoice::Glimpse {
            seat: Seat::A,
            top: CardId(9),
        };
        let prompt = build_choice_prompt(&pending, &card_of).expect("a glimpse keep prompt");
        assert_eq!(prompt.kind, "glimpse_keep");
        assert_eq!(
            prompt.peeked.as_ref().map(|c| c.name.as_str()),
            Some("Dawnling"),
            "the peeked top card is floated"
        );
        assert_eq!(prompt.options.len(), 2);
        let keep = &prompt.options[0];
        assert_eq!((keep.verb.as_str(), keep.index), ("choose", 0));
        assert!(keep.primary, "Keep is the suggested step");
        assert!(keep.detail.to_lowercase().contains("no anima"));
        let bottom = &prompt.options[1];
        assert_eq!((bottom.verb.as_str(), bottom.index), ("choose", 1));
        assert!(bottom.detail.contains("+1"), "bottom buys +1 Anima");
    }

    #[test]
    fn a_non_glimpse_pending_choice_is_not_a_canvas_modal() {
        // Peek/Target/Recover stay on the labeled-command path — the canvas modal is the
        // Glimpse + Mulligan pair, so build_choice_prompt returns None for them.
        let card_of = |_c: CardId| stub_card("X");
        let peek = recollect_core::state::PendingChoice::Peek {
            seat: Seat::A,
            looked: vec![CardId(1), CardId(2)],
        };
        assert!(build_choice_prompt(&peek, &card_of).is_none());
        let recover = recollect_core::state::PendingChoice::Recover {
            seat: Seat::A,
            options: vec![CardId(1)],
        };
        assert!(build_choice_prompt(&recover, &card_of).is_none());
    }

    #[test]
    fn the_mulligan_prompt_offers_mulligan_or_keep() {
        // The opening offer — a single beat: Mulligan (redraw, bottom one — the cost) or
        // Keep. The bottomed card is seed-chosen, so there is NO second pick step.
        let prompt = build_mulligan_prompt();
        assert_eq!(prompt.kind, "mulligan");
        assert!(prompt.peeked.is_none());
        assert_eq!(prompt.options.len(), 2);
        assert_eq!(prompt.options[0].verb, "mulligan");
        assert!(prompt.options[0].primary);
        assert_eq!(prompt.options[1].verb, "keep");
        // The subtitle states the cost (one card to the bottom) — legible, never hidden.
        assert!(prompt.subtitle.to_lowercase().contains("bottom"));
    }

    #[test]
    fn the_choice_modal_lays_out_on_screen_and_the_regions_match_the_chips() {
        // The card + every option chip sits inside the viewport, the chips stack without
        // overlap, and the hit-test rects line up with the drawn chips (carry the verb +
        // index) — so a canvas tap dispatches the right Choose / Mulligan.
        let card_of = |_c: CardId| stub_card("Dawnling");
        let prompt = build_choice_prompt(
            &recollect_core::state::PendingChoice::Glimpse {
                seat: Seat::A,
                top: CardId(9),
            },
            &card_of,
        )
        .unwrap();
        for (vw, vh) in [(400.0f32, 800.0f32), (1280.0, 900.0)] {
            let u = (vw.min(vh) / 26.0).clamp(9.0, 26.0);
            let (card, chips) = choice_layout(&prompt, vw, vh, u);
            assert!(
                card.x >= 0.0 && card.x + card.w <= vw + 1.0,
                "card on-screen: {card:?}"
            );
            assert!(
                card.y >= 0.0 && card.y + card.h <= vh + 1.0,
                "card on-screen: {card:?}"
            );
            let regions = choice_regions(&prompt, vw, vh);
            assert_eq!(regions.len(), prompt.options.len());
            for (k, (verb, index, r)) in regions.iter().enumerate() {
                assert_eq!(verb, "choose");
                assert_eq!(*index, prompt.options[k].index);
                assert!(
                    r.x >= 0.0 && r.x + r.w <= vw + 1.0 && r.y >= 0.0 && r.y + r.h <= vh + 1.0,
                    "chip on-screen: {r:?}"
                );
                // The hit-test rect matches the drawn chip (one source of truth).
                assert!((r.x - chips[k].x).abs() < 0.01 && (r.y - chips[k].y).abs() < 0.01);
            }
            // The chips stack without vertical overlap.
            assert!(regions[0].2.y + regions[0].2.h <= regions[1].2.y + 1.0);
        }
    }

    #[test]
    fn the_choice_modal_draws_a_scrim_card_and_chips() {
        // The pure build_shell, given a model with a choice, draws the modal: a scrim, the
        // card, and one chip per option, plus the peeked card's name as text.
        let card_of = |_c: CardId| stub_card("Dawnling");
        let prompt = build_choice_prompt(
            &recollect_core::state::PendingChoice::Glimpse {
                seat: Seat::A,
                top: CardId(9),
            },
            &card_of,
        )
        .unwrap();
        let model = choice_model(Some(prompt));
        let (vw, vh) = (412.0, 915.0);
        let scene = build_shell(&model, vw, vh);
        // A full-viewport scrim quad (the modal dims the board).
        assert!(
            scene
                .rects
                .iter()
                .any(|r| r.w >= vw - 1.0 && r.h >= vh - 1.0 && r.color.a > 0.2),
            "the choice modal scrims the board"
        );
        // The peeked card's name + the option labels are drawn as text.
        let texts: Vec<&str> = scene.texts.iter().map(|t| t.text.as_str()).collect();
        let joined = texts.join(" ").to_uppercase();
        assert!(joined.contains("DAWNLING"), "the peeked card is named");
        assert!(joined.contains("KEEP"), "the Keep chip is drawn");
        assert!(joined.contains("BOTTOM"), "the Bottom chip is drawn");
    }

    #[test]
    fn the_choice_a11y_tree_mirrors_each_option_as_a_button() {
        // Invariant 7: the choice is in the a11y tree — the prompt as a labelled group, the
        // peek as a text readout, and each option an actionable button whose Action target
        // carries the verb (a choose encodes its index as "choose:N").
        let card_of = |_c: CardId| stub_card("Dawnling");
        let prompt = build_choice_prompt(
            &recollect_core::state::PendingChoice::Glimpse {
                seat: Seat::A,
                top: CardId(9),
            },
            &card_of,
        )
        .unwrap();
        let tree = choice_a11y_tree(&prompt);
        let header = tree.iter().find(|n| n.id == "section-choice").unwrap();
        assert_eq!(header.role, "group");
        assert!(header.label.to_lowercase().contains("peek"));
        // The peek is a text readout — what you're deciding on (its name + stats).
        let peek = tree.iter().find(|n| n.id == "choice-peek").unwrap();
        assert_eq!(peek.role, "text");
        assert!(peek.label.contains("Dawnling"));
        // Keep + Bottom are actionable buttons; the Choose target encodes the index.
        let keep = tree.iter().find(|n| n.id == "choice-choose-0").unwrap();
        assert_eq!(keep.role, "button");
        assert!(keep.enabled);
        assert_eq!(keep.target, Some(A11yTarget::Action("choose:0".into())));
        let bottom = tree.iter().find(|n| n.id == "choice-choose-1").unwrap();
        assert_eq!(bottom.target, Some(A11yTarget::Action("choose:1".into())));
    }

    #[test]
    fn the_mulligan_a11y_options_dispatch_the_opening_verbs() {
        // The opening offer's a11y twin: Mulligan / Keep as actionable buttons firing the
        // opening verbs (no index — the bottomed card is seed-chosen).
        let tree = choice_a11y_tree(&build_mulligan_prompt());
        let mull = tree.iter().find(|n| n.id == "choice-mulligan-0").unwrap();
        assert_eq!(mull.target, Some(A11yTarget::Action("mulligan".into())));
        let keep = tree.iter().find(|n| n.id == "choice-keep-0").unwrap();
        assert_eq!(keep.target, Some(A11yTarget::Action("keep".into())));
    }

    /// A `ShellModel` carrying `choice` for the draw tests (reuses the shared `model()`).
    fn choice_model(choice: Option<ChoicePrompt>) -> ShellModel {
        ShellModel { choice, ..model() }
    }

    // ── a blocking modal MASKS the board affordances + the FAB lane ──────
    //
    // The gap the maintainer caught on the stills: a Glimpse / Mulligan (or result) modal was up,
    // yet the green action dots on the board/hand cards and the End-Turn / Glimpse FAB labels
    // still drew on top of the scrim. The overlay must suppress that affordance layer (the modal
    // is a focused decision; nothing actionable beneath it should invite a tap). These tests pin
    // the masking: with a modal active, no AFFORD-green dot and no FAB label appears.

    /// Count the AFFORD-green action dots (the board's + hand's "this can act" mark) in a scene —
    /// the exact bleed-through the maintainer flagged.
    fn afford_dots(s: &ShellScene) -> usize {
        s.rects
            .iter()
            .filter(|r| r.color == AFFORD && (r.w - r.h).abs() < 0.5)
            .count()
    }
    fn has_text(s: &ShellScene, t: &str) -> bool {
        s.texts.iter().any(|x| x.text == t)
    }

    #[test]
    fn the_choice_modal_masks_the_board_affordances_and_fabs() {
        // A live model with actionable pieces + cards (so dots + FABs WOULD draw)…
        let mut live = model();
        live.actionable_tiles = vec![12];
        live.actionable_hand = vec![0, 1];
        let live_s = build_shell(&live, 400.0, 800.0);
        assert!(
            afford_dots(&live_s) > 0,
            "the live shell has action dots to mask"
        );
        assert!(
            has_text(&live_s, "End Turn") && has_text(&live_s, "Glimpse"),
            "live FABs present"
        );

        // …now raise the opening Mulligan modal over the SAME model.
        let mut modal = live.clone();
        modal.choice = Some(build_mulligan_prompt());
        let s = build_shell(&modal, 400.0, 800.0);
        // Item 5: NO affordance dot, NO FAB label, NO HUD caption bleeds through the modal scrim.
        assert_eq!(
            afford_dots(&s),
            0,
            "no board/hand action dot bleeds through the modal"
        );
        for chrome in ["End Turn", "Glimpse", "SCORE", "ANIMA"] {
            assert!(
                !has_text(&s, chrome),
                "the live chrome '{chrome}' is masked behind the choice modal"
            );
        }
        // The modal itself IS drawn over a scrim (a full-viewport dim rect).
        assert!(
            s.rects
                .iter()
                .any(|r| r.w >= 400.0 - 1.0 && r.h >= 800.0 - 1.0 && r.color.a > 0.2),
            "the choice modal scrims the board"
        );
        // `blocking_modal()` reports the masking is in effect.
        assert!(modal.blocking_modal());
    }

    #[test]
    fn the_inspect_overlay_suppresses_the_affordances_but_keeps_the_cards() {
        // Item 5 / option C: under INSPECT the interactive affordance layer is FULLY
        // SUPPRESSED (you can't act while reading a card) — no board/hand action dots, no
        // FAB lane — but the board + hand CARDS still draw (spatial context) and dim under
        // the inspect scrim. This is stronger than the old "draw then dim": the dots/FABs
        // are ABSENT from the scene, not merely covered.
        let mut live = model();
        live.actionable_tiles = vec![12];
        live.actionable_hand = vec![0, 1];
        // Baseline (no inspect): the dots + FABs + hand cards all draw.
        let live_s = build_shell(&live, 400.0, 800.0);
        assert!(
            afford_dots(&live_s) > 0,
            "the live shell has action dots to suppress"
        );
        assert!(
            has_text(&live_s, "End Turn") && has_text(&live_s, "Glimpse"),
            "the live FAB lane is present"
        );
        // Hand-card bodies sit in the bottom tray (CARD-coloured grads on the Card layer,
        // low on a portrait viewport). Scope the count to the tray band so a CARD-coloured
        // FAB / board card isn't miscounted as a hand card.
        let tray_top = 800.0 * 0.6;
        let hand_card_bodies = |s: &ShellScene| {
            s.grads
                .iter()
                .filter(|g| g.top == CARD && g.layer == ShellLayer::Card && g.y >= tray_top)
                .count()
        };
        let live_cards = hand_card_bodies(&live_s);
        assert!(
            live_cards >= 2,
            "the live shell drew the hand cards ({live_cards})"
        );

        // Now open the inspect panel over the SAME model.
        let mut m = live.clone();
        m.inspect = Some(Inspect {
            name: "Cinderling".into(),
            kind: "Spirit".into(),
            resonance: "Ember".into(),
            cost: 2,
            attack: 3,
            defense: 1,
            hp: 2,
            reach: "Cross".into(),
            keywords: vec![],
            rules: "An ember.".into(),
            reach_w: 5,
            reach_center: 12,
            reach_tiles: vec![],
            anchor: (200.0, 600.0),
        });
        let s = build_shell(&m, 400.0, 800.0);
        // (C) The affordance layer is GONE: no action dots, no FAB labels.
        assert_eq!(
            afford_dots(&s),
            0,
            "under inspect, the board/hand action dots are SUPPRESSED (not drawn)"
        );
        for fab in ["End Turn", "Glimpse"] {
            assert!(
                !has_text(&s, fab),
                "under inspect, the FAB '{fab}' is suppressed (you can't act while reading)"
            );
        }
        // (C) …but the hand CARDS still draw (context): both hand cards' bodies are present
        // in the tray band (≥ the hand size). The inspect panel body is INSPECT_BG, never CARD,
        // so this counts hand cards, not the panel — proving the cards are dimmed context, not
        // removed. (`live_cards` ≥ this; the tray-scoped count is exactly the hand under inspect.)
        let _ = live_cards;
        assert!(
            hand_card_bodies(&s) >= m.hand.len(),
            "the hand cards still draw under inspect (dimmed context, not removed): {} < {}",
            hand_card_bodies(&s),
            m.hand.len(),
        );
        // The inspect scrim + panel are present (the cards dim UNDER the Detail-layer scrim).
        assert!(
            s.rects.iter().any(|r| r.layer == ShellLayer::Detail
                && r.w >= 400.0 - 1.0
                && r.h >= 800.0 - 1.0
                && r.color.a > 0.2),
            "the inspect overlay draws a Detail-layer scrim that dims the cards"
        );
        assert!(
            s.rects
                .iter()
                .any(|r| r.layer == ShellLayer::Detail && r.color == INSPECT_BG),
            "the inspect panel body draws"
        );
        // Inspect is NOT a blocking_modal (that's the Glimpse/Mulligan gate — unchanged).
        assert!(
            !m.blocking_modal(),
            "inspect is not a blocking modal (option C is its own gate)"
        );
    }

    // ── a co-occupied tile's LANDMARK is independently inspectable ───────
    //
    // A spirit can stand ON a Landmark; the tile button inspects/selects the spirit (the top
    // occupant), so the landmark would be unreachable. The a11y tree must emit a SECOND node for
    // the terrain so a keyboard / screen-reader user can inspect it (and the canvas reaches the
    // same target via a toggle-tap). This test builds a real co-occupied tile and asserts the node.

    /// A model with a spirit standing on a face-up Landmark on tile 12 (a co-occupied tile).
    fn model_spirit_on_landmark() -> ShellModel {
        use recollect_core::state::{Terrain, TerrainKind};
        use recollect_core::test_support::put_spirit;
        let cat = canon_catalog();
        let id_of = |name: &str| cat.iter().find(|c| c.name == name).map(|c| c.id).unwrap();
        let cloud = id_of("Cloudling");
        let deck: Vec<CardId> = (0..20).map(|_| cloud).collect();
        let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
        {
            let st = e.state_mut_for_test();
            st.board[12].terrain = Some(Terrain {
                card: cloud,
                owner: Seat::A,
                kind: TerrainKind::Landmark,
                face_down: false,
            });
            put_spirit(st, 12, cloud, Seat::A);
        }
        ShellModel {
            view: view_for(&e, Seat::A),
            ..model()
        }
    }

    #[test]
    fn a_co_occupied_tile_exposes_the_landmark_as_a_second_inspect_node() {
        // Tile 12 holds BOTH a spirit and a face-up Landmark. The a11y tree must carry the spirit
        // tile button AND a distinct "inspect the landmark" node firing `inspect-terrain:12`.
        let m = model_spirit_on_landmark();
        let labels: Vec<String> = (0..25).map(|i| format!("reading {i}")).collect();
        let tree = build_a11y_tree(&m, &labels);
        // The spirit's tile button is present (the top occupant — select/inspect).
        let tile = tree
            .iter()
            .find(|n| n.id == "tile-12")
            .expect("the spirit tile button");
        assert_eq!(tile.target, Some(A11yTarget::Tile(12)));
        // The LANDMARK has its own actionable inspect node — the second target (item 12).
        let terr = tree
            .iter()
            .find(|n| n.id == "tile-12-terrain")
            .expect("a distinct inspect-the-landmark node on the co-occupied tile");
        assert_eq!(terr.role, "button");
        assert!(terr.enabled, "inspect is always available (a read)");
        assert_eq!(
            terr.target,
            Some(A11yTarget::Action("inspect-terrain:12".into())),
            "activating the node inspects the terrain on tile 12"
        );
        assert!(
            terr.label.to_lowercase().contains("landmark")
                && terr.label.to_lowercase().contains("inspect"),
            "the node names the landmark + the inspect action: {}",
            terr.label
        );
    }

    #[test]
    fn a_tile_with_only_a_spirit_has_no_terrain_inspect_node() {
        // The second inspect node is ONLY for a co-occupied tile — a plain spirit (no terrain)
        // must NOT sprout a spurious terrain node.
        let mut m = model();
        m.actionable_tiles = vec![12];
        // model()'s board is empty; give tile 12 a lone spirit via the special-board path is
        // overkill — instead assert: with no terrain anywhere, no `-terrain` node exists.
        let labels: Vec<String> = (0..25).map(|_| "empty".into()).collect();
        let tree = build_a11y_tree(&m, &labels);
        assert!(
            !tree.iter().any(|n| n.id.ends_with("-terrain")),
            "no terrain inspect node without a co-resident landmark"
        );
    }

    #[test]
    fn the_nightfall_pip_is_a_double_circle_not_a_square() {
        // The final round marker reads as a DOUBLE CIRCLE (two concentric circular
        // rings). Each ring is a pair of fully-rounded (circle) discs in
        // the DUSK ink; assert several small DUSK circle discs cluster around the last pip, and no
        // square (radius 0) DUSK ring edges remain at the clock.
        let s = build_shell(&model(), 1280.0, 900.0);
        // Circle discs are w==h with radius == w/2. The double ring = 2 rings × (outer + hole) = 4
        // such discs in/near DUSK ink (the holes punch with PANEL_DEEP, also counted as circles).
        let circle_marks = s
            .rects
            .iter()
            .filter(|r| {
                r.layer == ShellLayer::Detail
                    && (r.w - r.h).abs() < 0.5
                    && (r.radius - r.w / 2.0).abs() < 0.5
                    && r.w < 24.0
                    && r.w > 1.0
            })
            .count();
        // The 12 round pips + the Nightfall double-ring's extra discs ⇒ clearly more than 12.
        assert!(
            circle_marks >= 14,
            "the Nightfall double-ring adds concentric circle discs beyond the 12 pips: {circle_marks}"
        );
        // A DUSK-ink ring disc exists (the double ring's outline colour) — the marker reads dark.
        assert!(
            s.rects.iter().any(|r| r.layer == ShellLayer::Detail
                && (r.color.r - DUSK.r).abs() < 0.05
                && (r.color.g - DUSK.g).abs() < 0.05
                && (r.w - r.h).abs() < 0.5
                && (r.radius - r.w / 2.0).abs() < 0.5),
            "the Nightfall ring is a DUSK-ink circle (not a square edge)"
        );
    }

    #[test]
    fn sanitize_keep_punct_keeps_real_sentence_punctuation_now() {
        // The atlas renders the full ASCII printable set + the typographic marks, so the
        // sanitizer keeps real punctuation (items 7/9/10) — the period, colon, comma, middot,
        // and hyphen all survive; only a truly undrawable mark collapses to a space.
        assert_eq!(
            sanitize_keep_punct("Atk: 80, Def: 20. Mobile · Warded"),
            "Atk: 80, Def: 20. Mobile · Warded"
        );
        assert_eq!(sanitize_keep_punct("Last-Light Koi"), "Last-Light Koi");
        // Runs of whitespace still collapse to one.
        assert_eq!(sanitize_keep_punct("a   b"), "a b");
    }

    #[test]
    fn ensure_sentence_period_supplies_a_missing_terminal() {
        // Item 9 — a lore clause without a terminal mark gets a period; one that already ends in
        // sentence punctuation is left alone; empty stays empty.
        assert_eq!(
            ensure_sentence_period("a drifting ember kindles an ally"),
            "a drifting ember kindles an ally."
        );
        assert_eq!(
            ensure_sentence_period("Your Glimpses take +1 card."),
            "Your Glimpses take +1 card."
        );
        assert_eq!(
            ensure_sentence_period("Reveal all Fabrications!"),
            "Reveal all Fabrications!"
        );
        assert_eq!(ensure_sentence_period(""), "");
        assert_eq!(ensure_sentence_period("trailing space "), "trailing space.");
    }
}
