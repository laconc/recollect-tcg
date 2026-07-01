//! The **cursor TUI** — a `ratatui` + `crossterm` arrow-key board for local 1v1.
//!
//! This is the terminal analogue of the web canvas's gold-cursor interaction
//! (`docs/decisions/web_client_ux.md`, how_to_play.md): a **gold cursor** sits on
//! the board, the **arrows** move it, **Enter/Space** picks up the spirit/hand-card
//! under it and places on the next press, **Esc** cancels, **Tab** toggles board↔hand
//! focus. Pick-up → place resolves to exactly the [`Command`] the engine offers — a
//! `MoveSpirit` / `PlaySpirit` / `Evolve` / `Overwrite` — so the cursor can never
//! produce a move `legal_commands` would reject (the same guarantee the canvas keeps).
//!
//! ## Parity with the canvas, not a lesser path
//! - The **legal-move list stays visible** beside the board (it mirrors the web keeping
//!   the a11y button list — invariant 7): the cursor is the fast path, the numbered list
//!   is the complete one. A bare **number** still picks from it, and the **verb
//!   mini-buffer** (`:` opens it) speaks the same shared [`crate::verbs`] grammar both
//!   line modes use.
//! - **Select shows targets; inspect shows reach.** Picking a piece highlights its legal
//!   *targets* (the engine's commands from/with it) in bright gold; `i` opens a passive
//!   inspect overlay (full stats + the reach grid) — the same two-reads split the canvas
//!   draws.
//! - **Redaction holds**: the board + panes are drawn from `seat`'s eye only
//!   ([`render_engine`] / `view_for`), so an opponent's hand never reaches the frame.
//!
//! ## TTY gate
//! [`run_local`] is the cursor mode; the caller ([`crate::local::run`]) uses it **only
//! when stdout is a real terminal** and the match is a local 1v1. Piped / `--json` /
//! headless / CI runs (and online / 2v2) keep the existing line REPL, byte-for-byte —
//! so the `online_roundtrip` golden path is untouched.
//!
//! ## Pure core, testable without a TTY
//! Everything that decides *what the cursor does* — the actionable sources, a source's
//! legal targets, the pick-up→place resolution, and the board-cell glyphs — is pure
//! (`&Engine` in, value out), so [`TestBackend`](ratatui::backend::TestBackend) snapshots
//! and the resolution unit tests run with no terminal at all. Only [`run_local`] touches
//! crossterm.
use crate::render::{describe, render_engine};
use crate::verbs;
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::crossterm::{ExecutableCommand, execute};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use recollect_core::state::{Command, PendingChoice, Phase};
use recollect_core::types::{tile_xy_w, xy_tile_w};
use recollect_core::{Engine, Seat};
use std::io;

/// What the cursor has **picked up** and is about to place. Mirrors the canvas's
/// "lifted" piece / "lifted" hand card: a board spirit to move (or evolve), or a hand
/// card to play. The next Enter on a legal target tile resolves it to a [`Command`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A standing spirit on this board tile — its next placement is a `MoveSpirit`
    /// (or an `Evolve`, when a held form card targets it).
    Board(u8),
    /// The hand card at this index — its next placement is a `PlaySpirit` / `Overwrite`
    /// (or, for a form card, an `Evolve` onto the base it lands on).
    Hand(u8),
}

/// Which half of the screen the cursor steers — the board grid or the hand tray. `Tab`
/// toggles it (the terminal analogue of moving between the canvas board and hand row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Board,
    Hand,
}

/// The cursor's live state: where it is, what it holds, and which transient overlay is
/// open. One per match; the event loop mutates it, the draw pass reads it. Pure data —
/// no engine, no terminal — so it is trivially constructable in a test.
#[derive(Debug, Clone)]
pub struct BoardCursor {
    /// Board tile the cursor sits on (0..w*w), used while `focus == Board`.
    pub tile: u8,
    /// Hand index the cursor sits on, used while `focus == Hand`.
    pub hand_index: u8,
    /// Board vs hand steering.
    pub focus: Focus,
    /// The lifted piece/card, if any. `Some` ⇒ the next Enter on a legal target places it.
    pub picked_up: Option<Source>,
    /// The inspect overlay (`i`) — a passive full-card panel; closed by any key.
    pub inspecting: bool,
    /// The help overlay (`?`) — the key legend; closed by any key.
    pub show_help: bool,
    /// The verb mini-buffer (`:` opens it); `None` ⇒ closed. Speaks [`crate::verbs`].
    pub buffer: Option<String>,
    /// A transient status line (last action / hint / rejection), shown under the board.
    pub status: String,
}

impl BoardCursor {
    /// A fresh cursor centred on the board, focused on the board, holding nothing.
    pub fn new(w: i8) -> Self {
        let mid = (w * w / 2) as u8;
        Self {
            tile: mid,
            hand_index: 0,
            focus: Focus::Board,
            picked_up: None,
            inspecting: false,
            show_help: false,
            buffer: None,
            status: "arrows move · Enter pick up / place · Tab board↔hand · i inspect · : verb · ? help · q quit".into(),
        }
    }
}

/// The tiles a **picked-up source** can legally be placed onto, paired with the concrete
/// [`Command`] each placement resolves to. Pure: derived entirely from
/// `engine.legal_commands(seat)`, so the highlighted targets are exactly what the engine
/// will accept — never a superset. Mirrors the canvas lighting a lifted piece's targets.
///
/// - A `Board(from)` source yields its `MoveSpirit { from, to }` destinations **and** any
///   `Evolve { tile: from }` (a held form card landing on this base — the target IS the
///   base, like the canvas's evolve chevron) **and** any `Devolve { tile: from }` (the
///   standing-Faded form receding to a held base — the target IS the form, the canvas's
///   downward devolve chevron).
/// - A `Hand(hi)` source yields the tiles of every command that plays/overwrites/evolves/
///   devolves with hand index `hi`: `PlaySpirit`, `Overwrite`, `Evolve { form_hand: hi }`,
///   `Devolve { base_hand: hi }` (a base card landing on a faded form), plus the spellbook
///   placements (`PlaceLandmark` / `SetFabrication`).
pub fn targets_for_source(engine: &Engine, seat: Seat, src: Source) -> Vec<(u8, Command)> {
    let mut out: Vec<(u8, Command)> = Vec::new();
    for c in engine.legal_commands(seat) {
        let hit = match (src, &c) {
            // Board piece → move / evolve-in-place / devolve-in-place (recede).
            (Source::Board(from), Command::MoveSpirit { from: f, to, .. }) if *f == from => {
                Some(*to)
            }
            (Source::Board(from), Command::Evolve { tile, .. }) if *tile == from => Some(*tile),
            (Source::Board(from), Command::Devolve { tile, .. }) if *tile == from => Some(*tile),
            (Source::Board(from), Command::Reveal { tile, .. }) if *tile == from => Some(*tile),
            // Hand card → play / overwrite / evolve / devolve / terrain.
            (
                Source::Hand(hi),
                Command::PlaySpirit {
                    hand_index, tile, ..
                },
            ) if *hand_index == hi => Some(*tile),
            (Source::Hand(hi), Command::Overwrite { hand_index, tile }) if *hand_index == hi => {
                Some(*tile)
            }
            (
                Source::Hand(hi),
                Command::Evolve {
                    form_hand, tile, ..
                },
            ) if *form_hand == hi => Some(*tile),
            // A base card from hand receding a standing-Faded form: it lands on the form's tile.
            (Source::Hand(hi), Command::Devolve { tile, base_hand }) if *base_hand == hi => {
                Some(*tile)
            }
            (Source::Hand(hi), Command::PlaceLandmark { hand_index, tile })
                if *hand_index == hi =>
            {
                Some(*tile)
            }
            (Source::Hand(hi), Command::SetFabrication { hand_index, tile })
                if *hand_index == hi =>
            {
                Some(*tile)
            }
            _ => None,
        };
        if let Some(t) = hit {
            // Keep the FIRST command per target tile (a stable, deterministic pick when a
            // tile is reachable several ways — e.g. plain move vs engaging move; the
            // numbered list still offers every variant).
            if !out.iter().any(|(tt, _)| *tt == t) {
                out.push((t, c));
            }
        }
    }
    out
}

/// Resolve a **pick-up → place** to the concrete [`Command`], given the cursor's held
/// `src` and the `target` tile it was placed on. `None` ⇒ that target is not legal for
/// the source (the caller leaves the pickup live and flashes a hint). Pure — the heart of
/// the cursor, and the unit-tested seam (arrow-to-tile + Enter → the right command).
pub fn resolve_place(engine: &Engine, seat: Seat, src: Source, target: u8) -> Option<Command> {
    targets_for_source(engine, seat, src)
        .into_iter()
        .find(|(t, _)| *t == target)
        .map(|(_, c)| c)
}

/// The board tiles that carry an **available action** for `seat` — a quiet cue the cursor
/// can render (the terminal twin of the canvas's "quiet dot on any actionable piece").
/// Pure; derived from the same `legal_commands` the targets are.
pub fn actionable_tiles(engine: &Engine, seat: Seat) -> std::collections::HashSet<u8> {
    engine
        .legal_commands(seat)
        .iter()
        .filter_map(|c| match c {
            Command::MoveSpirit { from, .. } => Some(*from),
            Command::Evolve { tile, .. } => Some(*tile),
            Command::Devolve { tile, .. } => Some(*tile),
            Command::Reveal { tile, .. } => Some(*tile),
            Command::StrikeFabrication { from, .. } => Some(*from),
            Command::Overwrite { tile, .. } => Some(*tile),
            Command::Reclaim { tile } => Some(*tile),
            _ => None,
        })
        .collect()
}

/// A board cell's two-character glyph + its base colour, from `seat`'s eye. Single-width
/// ASCII only (no wide Unicode), so the grid stays aligned and the [`TestBackend`]
/// snapshots are clean. `Aa`/`Bb` = a seat's spirit (upper = healthy, lower = fading);
/// `··`/`,,` = an impression; `##` = faded (dark) ground; spaces = empty.
///
/// [`TestBackend`]: ratatui::backend::TestBackend
fn cell_glyph(engine: &Engine, tile: u8) -> (String, Color) {
    let st = engine.state();
    let t = &st.board[tile as usize];
    if t.faded && t.spirit.is_none() {
        return ("##".into(), Color::DarkGray);
    }
    if let Some(sp) = &t.spirit {
        let c = seat_color(sp.owner);
        let ch = match sp.owner {
            Seat::A => 'A',
            Seat::B => 'B',
        };
        // Healthy = upper tag + a space; fading = lower tag + '~'.
        if sp.fading {
            (format!("{}~", ch.to_ascii_lowercase()), Color::DarkGray)
        } else {
            (format!("{ch} "), c)
        }
    } else if let Some(terr) = &t.terrain {
        use recollect_core::state::TerrainKind;
        let c = seat_color(terr.owner);
        let tag = match (&terr.kind, terr.face_down) {
            (TerrainKind::Landmark, _) => "Lm",
            (TerrainKind::Fabrication, true) => "??",
            (TerrainKind::Fabrication, false) => "Fb",
        };
        (tag.into(), c)
    } else if let Some(&s) = t.impressions.first() {
        let g = match s {
            Seat::A => "..",
            Seat::B => ",,",
        };
        (g.into(), seat_color(s))
    } else {
        ("  ".into(), Color::Reset)
    }
}

fn seat_color(s: Seat) -> Color {
    match s {
        Seat::A => Color::Cyan,
        Seat::B => Color::Magenta,
    }
}

/// The 5×5 (or w×w) interactive board as ratatui [`Line`]s, from `seat`'s eye. Draws the
/// cursor highlight, the picked-up source, and — when something is picked up — its legal
/// **target** tiles in bright gold (select-shows-targets). Pure (no terminal), so the
/// snapshot tests render it directly.
fn board_lines(engine: &Engine, seat: Seat, cur: &BoardCursor) -> Text<'static> {
    let st = engine.state();
    let w = st.board_w;
    let gold = Color::Yellow;
    let targets: std::collections::HashSet<u8> = match cur.picked_up {
        Some(src) => targets_for_source(engine, seat, src)
            .into_iter()
            .map(|(t, _)| t)
            .collect(),
        None => std::collections::HashSet::new(),
    };
    let actionable = actionable_tiles(engine, seat);
    let mut lines: Vec<Line> = Vec::new();
    for y in (0..w).rev() {
        let mut spans: Vec<Span> = vec![Span::styled(
            format!("{} ", y + 1),
            Style::default().fg(Color::DarkGray),
        )];
        for x in 0..w {
            let tile = xy_tile_w(x, y, w).unwrap();
            let (glyph, base) = cell_glyph(engine, tile);
            let is_cursor = cur.focus == Focus::Board && tile == cur.tile;
            let is_picked = cur.picked_up == Some(Source::Board(tile));
            let is_target = targets.contains(&tile);
            // Bracket the cell; the bracket colour carries the cue (cursor > picked >
            // target > actionable > plain).
            let (lb, rb, cue) = if is_cursor {
                ("[", "]", Some(gold))
            } else if is_picked {
                ("{", "}", Some(gold))
            } else if is_target {
                ("<", ">", Some(gold))
            } else if actionable.contains(&tile) {
                ("(", ")", Some(Color::Green))
            } else {
                (" ", " ", None)
            };
            let bracket_style = cue
                .map(|c| Style::default().fg(c).add_modifier(Modifier::BOLD))
                .unwrap_or_default();
            let glyph_style = if is_cursor || is_picked {
                Style::default()
                    .fg(base)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else if is_target {
                Style::default().fg(gold).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(base)
            };
            spans.push(Span::styled(lb.to_string(), bracket_style));
            spans.push(Span::styled(glyph, glyph_style));
            spans.push(Span::styled(rb.to_string(), bracket_style));
        }
        lines.push(Line::from(spans));
    }
    // Column legend (a..e / a..f).
    let mut legend: Vec<Span> = vec![Span::raw("  ")];
    for x in 0..w {
        let col = (b'a' + x as u8) as char;
        legend.push(Span::styled(
            format!(" {col} "),
            Style::default().fg(Color::DarkGray),
        ));
    }
    lines.push(Line::from(legend));
    Text::from(lines)
}

/// The hand tray as ratatui [`Line`]s — one card per row (cost · name · Atk/Def/HP), the
/// cursor row highlighted, the picked-up card marked. From `seat`'s own hand only
/// (redaction). A green dot leads a card that has a legal play this turn.
fn hand_lines(engine: &Engine, seat: Seat, cur: &BoardCursor) -> Text<'static> {
    let st = engine.state();
    let hand = &st.player(seat).hand;
    // Which hand indices can be played at all (any target) — the green-dot cue.
    let playable: std::collections::HashSet<u8> = engine
        .legal_commands(seat)
        .iter()
        .filter_map(|c| match c {
            Command::PlaySpirit { hand_index, .. }
            | Command::Overwrite { hand_index, .. }
            | Command::CastRitual { hand_index }
            | Command::TellUnwriting { hand_index }
            | Command::PlaceLandmark { hand_index, .. }
            | Command::SetFabrication { hand_index, .. }
            | Command::AttachBond { hand_index, .. }
            | Command::Release { hand_index } => Some(*hand_index),
            Command::Evolve { form_hand, .. } => Some(*form_hand),
            Command::Devolve { base_hand, .. } => Some(*base_hand),
            _ => None,
        })
        .collect();
    let mut lines: Vec<Line> = Vec::new();
    if hand.is_empty() {
        lines.push(Line::styled(
            "  (your hand is empty)",
            Style::default().fg(Color::DarkGray),
        ));
    }
    for (i, cid) in hand.iter().enumerate() {
        let d = engine.card(*cid);
        let is_cursor = cur.focus == Focus::Hand && i as u8 == cur.hand_index;
        let is_picked = cur.picked_up == Some(Source::Hand(i as u8));
        let dot = if playable.contains(&(i as u8)) {
            Span::styled("• ", Style::default().fg(Color::Green))
        } else {
            Span::raw("  ")
        };
        let marker = if is_picked {
            "{"
        } else if is_cursor {
            "["
        } else {
            " "
        };
        let body = format!(
            "{marker}{i}. {:<20} {}c  {}/{}/{}",
            d.name, d.cost, d.attack, d.defense, d.hp
        );
        let style = if is_cursor || is_picked {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![dot, Span::styled(body, style)]));
    }
    Text::from(lines)
}

/// The numbered "Legal plays" list (the always-visible move set, mirroring the web's
/// a11y button list) as ratatui [`Line`]s — each legal command through the canonical
/// [`describe`] labeler. Pure.
fn legal_lines(engine: &Engine, seat: Seat) -> Text<'static> {
    let legal = engine.legal_commands(seat);
    let mut lines: Vec<Line> = Vec::new();
    for (i, c) in legal.iter().enumerate() {
        lines.push(Line::from(format!("{i:>3}. {}", describe(engine, seat, c))));
    }
    if legal.is_empty() {
        lines.push(Line::styled(
            "(no legal plays — the turn has passed)",
            Style::default().fg(Color::DarkGray),
        ));
    }
    Text::from(lines)
}

/// A `PendingChoice` (Glimpse burn / keep-bottom, a target/recover pick) as plain
/// selectable blocks — the copy kept consistent with the canvas modal ("burn a card to
/// Glimpse", "Keep / Bottom for +1 Anima"). The modal *aesthetic* is a later polish; for
/// now these are the labeled options, cursor-pickable by number (and the legal list shows
/// the same `Choose { index }` entries). Returns `None` when no choice is pending.
fn pending_choice_lines(engine: &Engine, seat: Seat) -> Option<(String, Text<'static>)> {
    let st = engine.state();
    if !matches!(st.phase, Phase::PendingChoice { seat: s, .. } if s == seat) {
        return None;
    }
    let pc = st.pending_choice.as_ref()?;
    let (title, opts): (String, Vec<String>) = match pc {
        PendingChoice::GlimpseBurn { burnable, .. } => (
            "Glimpse — burn a card to Glimpse (it leaves play):".into(),
            burnable
                .iter()
                .enumerate()
                .map(|(i, c)| format!("{i}. Burn {} to Glimpse", engine.card(*c).name))
                .collect(),
        ),
        PendingChoice::Glimpse { top, .. } => {
            let n = engine.card(*top).name.clone();
            (
                "Glimpse — keep it, or bottom it for +1 Anima:".into(),
                vec![
                    format!("0. Keep {n} on top"),
                    format!("1. Bottom {n} for +1 Anima"),
                ],
            )
        }
        PendingChoice::Peek { looked, .. } => (
            "Take one to hand (the rest bottom in order):".into(),
            looked
                .iter()
                .enumerate()
                .map(|(i, c)| format!("{i}. Take {}", engine.card(*c).name))
                .collect(),
        ),
        PendingChoice::Recover { options, .. } => (
            "Recover one dissolved spirit to hand:".into(),
            options
                .iter()
                .enumerate()
                .map(|(i, c)| format!("{i}. Recover {}", engine.card(*c).name))
                .collect(),
        ),
        PendingChoice::Target { options, .. } => (
            "Choose a target tile:".into(),
            options
                .iter()
                .enumerate()
                .map(|(i, t)| format!("{i}. {}", crate::render::tn(*t)))
                .collect(),
        ),
    };
    let lines: Vec<Line> = opts
        .into_iter()
        .map(|s| Line::styled(s, Style::default().fg(Color::Yellow)))
        .collect();
    Some((title, Text::from(lines)))
}

/// Draw one full frame: the board (top-left), the hand (bottom-left), the engine's
/// seat's-eye render (the stats/score/anima pane — REUSED from [`render_engine`]) and the
/// legal-move list (right), plus the status line and any open overlay. Generic over the
/// backend so the real terminal and the [`TestBackend`] draw the identical layout.
///
/// [`TestBackend`]: ratatui::backend::TestBackend
pub fn draw<B: Backend>(
    terminal: &mut Terminal<B>,
    engine: &Engine,
    seat: Seat,
    cur: &BoardCursor,
) -> io::Result<()>
where
    B::Error: std::fmt::Display,
{
    terminal
        .draw(|f| {
            let area = f.area();
            // Left = board + hand + status; Right = the reused engine render + legal list.
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
                .split(area);
            let left = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(9), // board
                    Constraint::Min(6),    // hand
                    Constraint::Length(4), // status
                ])
                .split(cols[0]);
            let right = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
                .split(cols[1]);

            let phase = match engine.state().phase {
                Phase::PendingChoice { .. } => " · a choice awaits",
                Phase::PendingRelease { .. } => " · RELEASE a card",
                _ => "",
            };
            let lifted = match cur.picked_up {
                Some(Source::Board(t)) => format!("(lifted {}) ", crate::render::tn(t)),
                Some(Source::Hand(h)) => format!("(playing hand {h}) "),
                None => String::new(),
            };
            let board_title = format!(" Board — Seat {seat:?}{phase} {lifted}");
            f.render_widget(
                Paragraph::new(board_lines(engine, seat, cur))
                    .block(Block::default().borders(Borders::ALL).title(board_title)),
                left[0],
            );
            f.render_widget(
                Paragraph::new(hand_lines(engine, seat, cur)).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(if cur.focus == Focus::Hand {
                            " Hand ◀ "
                        } else {
                            " Hand "
                        }),
                ),
                left[1],
            );
            // Status / verb buffer.
            let status_text = match &cur.buffer {
                Some(b) => format!("verb> {b}"),
                None => cur.status.clone(),
            };
            f.render_widget(
                Paragraph::new(status_text)
                    .wrap(Wrap { trim: true })
                    .block(Block::default().borders(Borders::ALL).title(" Status ")),
                left[2],
            );
            // The reused engine render (stats / projections / score / hand) — the rich
            // seat's-eye String, drawn verbatim into a scrollable paragraph.
            f.render_widget(
                Paragraph::new(strip_ansi(&render_engine(engine, seat)))
                    .wrap(Wrap { trim: false })
                    .block(Block::default().borders(Borders::ALL).title(" Match ")),
                right[0],
            );
            f.render_widget(
                Paragraph::new(legal_lines(engine, seat))
                    .wrap(Wrap { trim: false })
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(" Legal plays (number / : verb / cursor) "),
                    ),
                right[1],
            );

            // Overlays draw last, over a cleared centred rect.
            if cur.show_help {
                overlay(f, area, " Keys ", help_text());
            } else if cur.inspecting {
                if let Some((title, body)) = inspect_overlay(engine, seat, cur) {
                    overlay(f, area, &title, body);
                }
            } else if let Some((title, body)) = pending_choice_lines(engine, seat) {
                overlay(f, area, &format!(" {title} "), body);
            }
        })
        .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(())
}

/// The inspect overlay body for whatever the cursor points at (a board spirit, or the
/// hand card) — the full card via [`crate::render::inspect_card`] (stats + keywords +
/// rules + the reach grid), the passive "what could this threaten" read. `None` when the
/// cursor points at empty ground.
fn inspect_overlay(
    engine: &Engine,
    seat: Seat,
    cur: &BoardCursor,
) -> Option<(String, Text<'static>)> {
    let st = engine.state();
    let (def, at, owner) = match cur.focus {
        Focus::Board => {
            let sp = st.spirit_at(cur.tile)?;
            (engine.card(sp.card).clone(), Some(cur.tile), sp.owner)
        }
        Focus::Hand => {
            let cid = *st.player(seat).hand.get(cur.hand_index as usize)?;
            (engine.card(cid).clone(), None, seat)
        }
    };
    let title = format!(" Inspect — {} ", def.name);
    let body = strip_ansi(&crate::render::inspect_card(engine, &def, at, owner));
    Some((title, Text::from(body)))
}

/// Render `body` in a bordered, cleared box centred in `area` (an overlay panel). One
/// helper for the inspect / help / choice overlays.
fn overlay(f: &mut ratatui::Frame, area: Rect, title: &str, body: Text<'static>) {
    let w = (area.width as f32 * 0.7) as u16;
    let h = (area.height as f32 * 0.7) as u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w.min(area.width), h.min(area.height));
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title.to_string()),
        ),
        rect,
    );
}

/// The help overlay text — the full key legend.
fn help_text() -> Text<'static> {
    Text::from(vec![
        Line::from("Cursor TUI — local 1v1"),
        Line::from(""),
        Line::from("  ← ↑ ↓ →   move the cursor (board), up/down (hand)"),
        Line::from("  Enter/Space  pick up the piece/card, then place on a target"),
        Line::from("  Esc       cancel a pick-up (or close this/an overlay)"),
        Line::from("  Tab       toggle board ↔ hand focus"),
        Line::from("  i         inspect (full card + reach grid)"),
        Line::from("  :         open the verb mini-buffer (p/o/m/v/dv/rc/g/r/end)"),
        Line::from("  0-9 …     pick a numbered legal play, then Enter"),
        Line::from("  ?         this help · q  quit"),
        Line::from(""),
        Line::from("Picking a piece highlights its legal targets in gold."),
    ])
}

/// Strip ANSI SGR escapes from a render string so it lays out as plain text in a ratatui
/// [`Paragraph`] (ratatui styles cells itself; raw ANSI bytes would corrupt the buffer).
/// The reused [`render_engine`] / [`crate::render::inspect_card`] strings may carry colour
/// (per `NO_COLOR`); this yields the same bytes the `NO_COLOR` form would.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x1b && i + 1 < b.len() && b[i + 1] == b'[' {
            i += 2;
            while i < b.len() && b[i] != b'm' {
                i += 1;
            }
            i += 1;
        } else {
            let n = utf8_len(b[i]);
            out.push_str(std::str::from_utf8(&b[i..(i + n).min(b.len())]).unwrap_or(""));
            i += n;
        }
    }
    out
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

/// The outcome of one key press handled against the cursor state — drives the loop.
enum Step {
    /// Stay in the cursor loop, redraw.
    Continue,
    /// Apply this engine command (a resolved move), then redraw.
    Apply(Command),
    /// Quit the cursor loop (pause the match).
    Quit,
}

/// Handle one key against the cursor + engine, returning what the loop should do. **Pure**
/// over (engine, seat, cursor) → it mutates `cur` and yields a [`Step`]; it never touches
/// the terminal, so the resolution path is unit-testable. The verb buffer and the
/// number-buffer reuse the shared [`crate::verbs`] grammar + the numbered legal list.
fn handle_key(engine: &Engine, seat: Seat, cur: &mut BoardCursor, code: KeyCode) -> Step {
    let w = engine.state().board_w;
    let hand_len = engine.state().player(seat).hand.len() as u8;
    let legal = engine.legal_commands(seat);

    // An open overlay (help / inspect): any key closes it.
    if cur.show_help || cur.inspecting {
        cur.show_help = false;
        cur.inspecting = false;
        return Step::Continue;
    }

    // The verb mini-buffer captures keys until Enter (resolve) or Esc (cancel).
    if let Some(buf) = cur.buffer.as_mut() {
        match code {
            KeyCode::Esc => {
                cur.buffer = None;
                cur.status = "verb cancelled".into();
            }
            KeyCode::Enter => {
                let line = buf.trim().to_string();
                cur.buffer = None;
                if line.is_empty() {
                    return Step::Continue;
                }
                // A bare number picks from the numbered legal list; else parse a verb.
                if let Ok(n) = line.parse::<usize>() {
                    match legal.get(n) {
                        Some(c) => return Step::Apply(c.clone()),
                        None => cur.status = format!("no play {n}"),
                    }
                } else if let Some(intent) = verbs::parse(&line, w as u8) {
                    match verbs::resolve(intent, &legal) {
                        Some(c) => return Step::Apply(c),
                        None => cur.status = "no legal move on that tile".into(),
                    }
                } else {
                    cur.status = verbs::USAGE.into();
                }
            }
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char(ch) => buf.push(ch),
            _ => {}
        }
        return Step::Continue;
    }

    match code {
        KeyCode::Char('q') => return Step::Quit,
        KeyCode::Char('?') => cur.show_help = true,
        KeyCode::Char('i') => cur.inspecting = true,
        KeyCode::Char(':') => cur.buffer = Some(String::new()),
        KeyCode::Tab | KeyCode::BackTab => {
            cur.focus = match cur.focus {
                Focus::Board => Focus::Hand,
                Focus::Hand => Focus::Board,
            };
            cur.picked_up = None;
        }
        KeyCode::Esc => {
            if cur.picked_up.take().is_some() {
                cur.status = "pick-up cancelled".into();
            }
        }
        // A digit opens the verb buffer pre-seeded — so `3` then more digits then Enter
        // picks play 3+ (the numbered list is always available, like the web a11y list).
        KeyCode::Char(c @ '0'..='9') => cur.buffer = Some(c.to_string()),
        KeyCode::Left | KeyCode::Char('h') => move_cursor(cur, w, -1, 0),
        KeyCode::Right | KeyCode::Char('l') => move_cursor(cur, w, 1, 0),
        KeyCode::Up | KeyCode::Char('k') => match cur.focus {
            Focus::Board => move_cursor(cur, w, 0, 1),
            Focus::Hand => {
                cur.hand_index = cur.hand_index.saturating_sub(1);
            }
        },
        KeyCode::Down | KeyCode::Char('j') => match cur.focus {
            Focus::Board => move_cursor(cur, w, 0, -1),
            Focus::Hand => {
                if hand_len > 0 {
                    cur.hand_index = (cur.hand_index + 1).min(hand_len - 1);
                }
            }
        },
        KeyCode::Enter | KeyCode::Char(' ') => return activate(engine, seat, cur),
        _ => {}
    }
    Step::Continue
}

/// Move the board cursor by (dx, dy), clamped to the board. Hand focus ignores horizontal
/// movement (the hand is a vertical list). `dy` is in board coordinates (up = +1).
fn move_cursor(cur: &mut BoardCursor, w: i8, dx: i8, dy: i8) {
    if cur.focus != Focus::Board {
        return;
    }
    let (x, y) = tile_xy_w(cur.tile, w);
    let nx = (x + dx).clamp(0, w - 1);
    let ny = (y + dy).clamp(0, w - 1);
    if let Some(t) = xy_tile_w(nx, ny, w) {
        cur.tile = t;
    }
}

/// Enter/Space: if nothing is held, **pick up** the spirit/card under the cursor (only
/// when it actually has a legal action — empty hands flash a hint). If something is held,
/// **place** it on the cursor tile, resolving to the concrete [`Command`] (or flash that
/// the target is illegal and keep the pickup live).
fn activate(engine: &Engine, seat: Seat, cur: &mut BoardCursor) -> Step {
    match cur.picked_up {
        None => {
            let src = match cur.focus {
                Focus::Board => Source::Board(cur.tile),
                Focus::Hand => {
                    let hand_len = engine.state().player(seat).hand.len();
                    if cur.hand_index as usize >= hand_len {
                        cur.status = "no card there".into();
                        return Step::Continue;
                    }
                    Source::Hand(cur.hand_index)
                }
            };
            // Only pick up something with at least one legal target (mirrors the canvas:
            // a piece with no action doesn't lift).
            if targets_for_source(engine, seat, src).is_empty() {
                cur.status = match src {
                    Source::Board(t) => {
                        format!("{} has no move — read the plays", crate::render::tn(t))
                    }
                    Source::Hand(_) => "that card has no placement now".into(),
                };
                // Spellbook one-shots (Ritual / Unwriting) have no tile target — resolve
                // straight from the hand pick (the canvas resolves these on pick-up too).
                if let Source::Hand(hi) = src
                    && let Some(c) = engine.legal_commands(seat).into_iter().find(|c| {
                        matches!(c, Command::CastRitual { hand_index } | Command::TellUnwriting { hand_index } if *hand_index == hi)
                    })
                {
                    return Step::Apply(c);
                }
                return Step::Continue;
            }
            cur.picked_up = Some(src);
            // Jump the cursor onto the board so the player can aim a target immediately.
            cur.focus = Focus::Board;
            cur.status = "placing — arrow to a gold target, Enter to commit, Esc to cancel".into();
            Step::Continue
        }
        Some(src) => match resolve_place(engine, seat, src, cur.tile) {
            Some(c) => {
                cur.picked_up = None;
                Step::Apply(c)
            }
            None => {
                cur.status = format!("{} is not a legal target", crate::render::tn(cur.tile));
                Step::Continue
            }
        },
    }
}

/// The cursor TUI entry point — local 1v1 only, called when stdout is a real terminal.
/// Owns crossterm raw mode + the alternate screen for its lifetime (restored on every
/// exit path, including a panic-free error), runs the draw/key loop against the in-process
/// `engine`, applying each resolved [`Command`] through `engine.apply`. The opponent
/// (Seat B) is the AI: after the human's move the loop drives B's turns via `bot` until
/// it is the human's turn again or the match finishes. Returns when the match ends or the
/// player quits (`q`) — the caller then prints the final board / closing line on the
/// normal screen, so Nightfall reads identically to the line mode.
pub fn run_local(
    engine: &mut Engine,
    human: Seat,
    bot_seat: Seat,
    difficulty: recollect_bot::Difficulty,
    ai: &mut recollect_core::rng::Rng,
) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = cursor_loop(&mut terminal, engine, human, bot_seat, difficulty, ai);

    // Always restore the terminal, whatever the loop returned.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

/// The draw/key loop, factored out so [`run_local`] can restore the terminal around it on
/// every path. Advances the AI's turns between the human's, and exits on quit or finish.
fn cursor_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    engine: &mut Engine,
    human: Seat,
    bot_seat: Seat,
    difficulty: recollect_bot::Difficulty,
    ai: &mut recollect_core::rng::Rng,
) -> io::Result<()> {
    let mut cur = BoardCursor::new(engine.state().board_w);
    loop {
        // Let the bot take any turns it owns, so control returns on the human's turn.
        while engine.state().active == bot_seat
            && !matches!(engine.state().phase, Phase::Finished { .. })
        {
            let cmd = recollect_bot::choose(engine, bot_seat, difficulty, ai);
            let label = describe(engine, bot_seat, &cmd);
            let _ = engine.apply(bot_seat, cmd);
            cur.status = format!("Seat {bot_seat:?} ({} bot): {label}", difficulty.name());
        }
        if matches!(engine.state().phase, Phase::Finished { .. }) {
            return Ok(());
        }
        draw(terminal, engine, human, &cur)?;
        // Block for a key (only key-press events drive the loop).
        match event::read()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                match handle_key(engine, human, &mut cur, k.code) {
                    Step::Quit => return Ok(()),
                    Step::Continue => {}
                    Step::Apply(cmd) => {
                        let label = describe(engine, human, &cmd);
                        match engine.apply(human, cmd) {
                            Ok(_) => cur.status = format!("you: {label}"),
                            Err(r) => cur.status = format!("the Memory declines: {r:?}"),
                        }
                    }
                }
            }
            // Resize / other events: just redraw.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recollect_core::cards::canon_catalog;
    use recollect_core::quickplay::{decide_opener, generate_deck, offer};
    use recollect_core::state::MatchRules;

    /// A deterministic opening engine — the SAME seeded local 1v1 the gallery builds
    /// (Seat A opens), so the cursor tests run against a real, reproducible state.
    fn engine() -> Engine {
        let seed = crate::tui_gallery::SEED;
        let catalog = canon_catalog();
        let style_a = offer(seed)[0].id;
        let style_b = offer(seed ^ 0xB)[0].id;
        let deck_a = generate_deck(style_a, seed, &catalog);
        let deck_b = generate_deck(style_b, seed.wrapping_add(1), &catalog);
        let opener = decide_opener(seed, 0);
        let (e, _) =
            Engine::new_with_rules(seed, catalog, deck_a, deck_b, MatchRules::default(), opener);
        e
    }

    #[test]
    fn cursor_starts_centred_and_holds_nothing() {
        let e = engine();
        let cur = BoardCursor::new(e.state().board_w);
        assert_eq!(cur.tile, 12, "5x5 board centres on tile 12 (c3)");
        assert!(cur.picked_up.is_none());
        assert_eq!(cur.focus, Focus::Board);
    }

    #[test]
    fn arrows_clamp_to_the_board() {
        let e = engine();
        let mut cur = BoardCursor::new(e.state().board_w);
        // Walk hard left + down off the corner — must clamp at a1 (tile 0), never wrap.
        for _ in 0..9 {
            move_cursor(&mut cur, 5, -1, 0);
            move_cursor(&mut cur, 5, 0, -1);
        }
        assert_eq!(cur.tile, 0, "clamps to a1");
        // Walk hard right + up — must clamp at e5 (tile 24).
        for _ in 0..9 {
            move_cursor(&mut cur, 5, 1, 0);
            move_cursor(&mut cur, 5, 0, 1);
        }
        assert_eq!(cur.tile, 24, "clamps to e5");
    }

    #[test]
    fn picking_up_a_hand_card_then_a_legal_tile_resolves_to_playspirit() {
        let e = engine();
        // Find a hand card with a legal PlaySpirit, and the tile it lands on.
        let (hi, tile) = e
            .legal_commands(Seat::A)
            .into_iter()
            .find_map(|c| match c {
                Command::PlaySpirit {
                    hand_index, tile, ..
                } => Some((hand_index, tile)),
                _ => None,
            })
            .expect("the opening offers at least one PlaySpirit");
        let src = Source::Hand(hi);
        // The tile must be among the source's highlighted targets…
        assert!(
            targets_for_source(&e, Seat::A, src)
                .iter()
                .any(|(t, _)| *t == tile)
        );
        // …and placing there resolves to a PlaySpirit of that card onto that tile.
        let cmd = resolve_place(&e, Seat::A, src, tile).expect("legal placement resolves");
        assert!(
            matches!(cmd, Command::PlaySpirit { hand_index, tile: t, .. } if hand_index == hi && t == tile),
            "resolved to {cmd:?}"
        );
        // An off-target tile resolves to nothing (the cursor keeps the pickup live).
        let bad = (0..25u8).find(|t| {
            !targets_for_source(&e, Seat::A, src)
                .iter()
                .any(|(x, _)| x == t)
        });
        if let Some(bad) = bad {
            assert!(resolve_place(&e, Seat::A, src, bad).is_none());
        }
    }

    #[test]
    fn activate_picks_up_then_places_via_the_handle_key_seam() {
        let mut e = engine();
        let (hi, tile) = e
            .legal_commands(Seat::A)
            .into_iter()
            .find_map(|c| match c {
                Command::PlaySpirit {
                    hand_index, tile, ..
                } => Some((hand_index, tile)),
                _ => None,
            })
            .unwrap();
        let mut cur = BoardCursor::new(e.state().board_w);
        // Focus the hand, point at the card, Enter to lift it.
        cur.focus = Focus::Hand;
        cur.hand_index = hi;
        match handle_key(&e, Seat::A, &mut cur, KeyCode::Enter) {
            Step::Continue => {}
            other => panic!("pick-up should Continue, got {other:?}"),
        }
        assert_eq!(cur.picked_up, Some(Source::Hand(hi)), "the card lifted");
        assert_eq!(cur.focus, Focus::Board, "focus jumped to the board to aim");
        // Aim the cursor at the landing tile, Enter to place → the engine command.
        cur.tile = tile;
        match handle_key(&e, Seat::A, &mut cur, KeyCode::Enter) {
            Step::Apply(cmd) => {
                assert!(
                    matches!(cmd, Command::PlaySpirit { hand_index, tile: t, .. } if hand_index == hi && t == tile)
                );
                e.apply(Seat::A, cmd)
                    .expect("the resolved command is legal");
            }
            other => panic!("placement should Apply, got {other:?}"),
        }
        assert!(cur.picked_up.is_none(), "the pickup cleared after placing");
    }

    #[test]
    fn esc_cancels_a_pickup() {
        let e = engine();
        let mut cur = BoardCursor::new(e.state().board_w);
        cur.picked_up = Some(Source::Hand(0));
        let _ = handle_key(&e, Seat::A, &mut cur, KeyCode::Esc);
        assert!(cur.picked_up.is_none(), "Esc drops the pickup");
    }

    #[test]
    fn tab_toggles_focus_and_drops_any_pickup() {
        let e = engine();
        let mut cur = BoardCursor::new(e.state().board_w);
        cur.picked_up = Some(Source::Board(12));
        let _ = handle_key(&e, Seat::A, &mut cur, KeyCode::Tab);
        assert_eq!(cur.focus, Focus::Hand);
        assert!(cur.picked_up.is_none(), "switching focus cancels a pickup");
    }

    // Step is debug-printed in test panics.
    impl std::fmt::Debug for Step {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Step::Continue => write!(f, "Continue"),
                Step::Apply(c) => write!(f, "Apply({c:?})"),
                Step::Quit => write!(f, "Quit"),
            }
        }
    }
}
