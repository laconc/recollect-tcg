//! Golden-snapshot regression for the **cursor TUI** (`src/tui.rs`) — the ratatui
//! arrow-key board for local 1v1. Each "moment" drives a SEEDED `recollect-core` engine
//! to a point of interest, sets the cursor state, draws one frame into a fixed-size
//! [`TestBackend`], and asserts the frame byte-for-byte against a committed golden under
//! `docs/gallery/tui/cursor-*.txt` (the same discipline as the text gallery).
//!
//! Three moments mirror the deliverable's acceptance shots:
//!   • `cursor-board`   — the opening board, the gold cursor on the centre tile.
//!   • `cursor-pickup`  — a hand card lifted, its legal targets highlighted in gold.
//!   • `cursor-glimpse` — the Glimpse burn prompt as the selectable choice overlay.
//!
//! Beyond "it still draws", two properties:
//!   • **Determinism** — the seeded frames are reproducible (this test IS the second run;
//!     `BLESS=1 cargo test -p recollect-cli --test cursor_tui` rewrites the goldens).
//!   • **Redaction** (AGENTS.md invariant 2) — none of Seat B's private opening **hand**
//!     cards appear in any Seat-A cursor frame. (At the opening B has no board presence,
//!     so any B-hand name in the frame would be a pure leak.)
//!
//! The `TestBackend` buffer is single-width ASCII by construction (the cursor board uses
//! no wide glyphs), so the frames carry no "Hidden by multi-width symbols" noise and the
//! goldens are stable bytes.
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use recollect_cli::tui::{self, BoardCursor, Source};
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::{decide_opener, generate_deck, offer};
use recollect_core::state::{Command, MatchRules};
use recollect_core::{Engine, Seat};
use std::path::PathBuf;

/// A fresh deterministic opening engine — the SAME seeded local 1v1 the text gallery
/// builds (Seat A opens), so the cursor frames are reproducible and share the gallery's
/// fixtures. NO_COLOR is irrelevant here: `TestBackend`'s `Display` renders symbols only
/// (no ANSI), so the styling never reaches the golden bytes.
fn engine() -> Engine {
    let seed = recollect_cli::tui_gallery::SEED;
    let catalog = canon_catalog();
    let style_a = offer(seed)[0].id;
    let style_b = offer(seed ^ 0xB)[0].id;
    let deck_a = generate_deck(style_a, seed, &catalog);
    let deck_b = generate_deck(style_b, seed.wrapping_add(1), &catalog);
    let opener = decide_opener(seed, 0);
    let (e, _) =
        Engine::new_with_rules(seed, catalog, deck_a, deck_b, MatchRules::default(), opener);
    assert_eq!(e.state().active, Seat::A, "SEED must open Seat A");
    e
}

/// Render one frame to the `TestBackend` `Display` string at a fixed 100×34 size.
fn frame(engine: &Engine, cur: &BoardCursor) -> String {
    let backend = TestBackend::new(100, 34);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    tui::draw(&mut terminal, engine, Seat::A, cur).expect("draw");
    format!("{}", terminal.backend())
}

/// The three moments, each (name, the rendered frame).
fn moments() -> Vec<(&'static str, String)> {
    let mut out = Vec::new();

    // 1) The opening board — the cursor on the centre tile, nothing held.
    let e = engine();
    let cur = BoardCursor::new(e.state().board_w);
    out.push(("cursor-board", frame(&e, &cur)));

    // 2) A hand card lifted — its legal targets highlighted in gold.
    let e = engine();
    let hi = e
        .legal_commands(Seat::A)
        .into_iter()
        .find_map(|c| match c {
            Command::PlaySpirit { hand_index, .. } => Some(hand_index),
            _ => None,
        })
        .expect("the opening offers a PlaySpirit");
    let mut cur = BoardCursor::new(e.state().board_w);
    cur.picked_up = Some(Source::Hand(hi));
    out.push(("cursor-pickup", frame(&e, &cur)));

    // 3) The Glimpse burn prompt — Glimpse opens the burn step; the choice overlay shows.
    let mut e = engine();
    e.apply(Seat::A, Command::Glimpse)
        .expect("Glimpse is legal at the opening");
    let cur = BoardCursor::new(e.state().board_w);
    out.push(("cursor-glimpse", frame(&e, &cur)));

    out
}

fn golden_path(base: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/gallery/tui")
        .join(format!("{base}.txt"))
}

#[test]
fn cursor_frames_match_their_committed_goldens() {
    let bless = std::env::var_os("BLESS").is_some();
    for (name, rendered) in moments() {
        let path = golden_path(name);
        if bless {
            std::fs::write(&path, &rendered).expect("write golden");
            continue;
        }
        let golden = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "missing golden {} — run `BLESS=1 cargo test -p recollect-cli --test cursor_tui`: {e}",
                path.display()
            )
        });
        assert_eq!(
            rendered,
            golden,
            "cursor TUI snapshot drift for '{name}' ({}). The frame changed but the golden \
             wasn't updated — re-bless with `BLESS=1 cargo test -p recollect-cli --test cursor_tui` \
             and commit docs/gallery/tui/{name}.txt.",
            path.display()
        );
    }
}

#[test]
fn cursor_frames_never_leak_the_opponents_hand() {
    // Seat B's private opening hand — the redaction target. At the opening B has nothing
    // on the board, so any of these names in a Seat-A frame is a pure leak (invariant 2).
    let e = engine();
    let b_hand: Vec<String> = e
        .state()
        .player(Seat::B)
        .hand
        .iter()
        .map(|id| e.card(*id).name.clone())
        .collect();
    assert!(!b_hand.is_empty(), "Seat B should hold an opening hand");
    for (name, rendered) in moments() {
        for card in &b_hand {
            assert!(
                !rendered.contains(card.as_str()),
                "redaction breach: Seat B's hidden hand card '{card}' leaked into the \
                 Seat-A cursor frame '{name}'"
            );
        }
    }
}
