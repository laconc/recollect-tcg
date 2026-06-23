//! Golden-snapshot regression for the **TUI gallery** (`docs/gallery/tui/*.txt`) — the
//! committed text record of the line-based terminal client. Each moment is re-rendered
//! in-process through the SAME `recollect_cli::tui_gallery` the `tui_capture` example
//! uses, and asserted byte-for-byte against its committed `.txt`. A render change that
//! isn't reflected in the goldens fails here (run `make tui-gallery` and commit the diff).
//!
//! Two properties beyond "it still renders":
//!   • **Determinism** — the seeded screens are reproducible (this test IS the second
//!     run; the gallery script's `git diff --exit-code` is the on-disk twin).
//!   • **Redaction** (AGENTS.md invariant 2) — none of Seat B's private opening **hand**
//!     cards leak into any Seat-A snapshot. (Seat B's *placed* spirits are public board
//!     state and may be named — the result board names them; the hand never is.)
//!
//! Colour-agnostic: the in-process render may carry ANSI (per `NO_COLOR` in the env),
//! so we strip ANSI before comparing — `strip_ansi(coloured)` is byte-identical to the
//! `NO_COLOR` form the goldens are stored in. We also assert the goldens carry no ESC.
use recollect_cli::tui_gallery::{self, MOMENTS};
use recollect_core::Seat;
use std::path::PathBuf;

/// Path to `docs/gallery/tui/<base>.txt` from the crate root (CARGO_MANIFEST_DIR is
/// `app/crates/recollect-cli`; the docs tree is three levels up at the repo root).
fn golden_path(base: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/gallery/tui")
        .join(format!("{base}.txt"))
}

/// Remove ANSI SGR escapes (`ESC [ … m`) so the comparison is independent of whether
/// `NO_COLOR` is set in the test's environment. The goldens are the `NO_COLOR` form,
/// and stripping the escapes from a coloured render yields exactly that.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // Skip to the final byte of the CSI sequence ('m' for SGR).
            i += 2;
            while i < bytes.len() && bytes[i] != b'm' {
                i += 1;
            }
            i += 1; // consume the 'm'
        } else {
            // Copy this whole UTF-8 char (the board uses multi-byte glyphs like ░ · ⌂).
            let ch_len = utf8_len(bytes[i]);
            out.push_str(std::str::from_utf8(&bytes[i..i + ch_len]).unwrap());
            i += ch_len;
        }
    }
    out
}

/// Byte length of the UTF-8 sequence whose lead byte is `b`.
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

#[test]
fn tui_gallery_stills_match_their_committed_goldens() {
    for (moment, base) in MOMENTS {
        let rendered = strip_ansi(
            &tui_gallery::screen(moment).unwrap_or_else(|| panic!("moment '{moment}' renders")),
        );
        let path = golden_path(base);
        let golden = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "missing golden {} — run `make tui-gallery` to (re)generate it: {e}",
                path.display()
            )
        });
        // The committed goldens must be colour-free (the gallery sets NO_COLOR).
        assert!(
            !golden.contains('\u{1b}'),
            "golden {} carries an ANSI escape — regenerate with NO_COLOR",
            path.display()
        );
        assert_eq!(
            rendered,
            golden,
            "TUI snapshot drift for '{moment}' ({}). The render changed but the golden \
             wasn't updated — run `make tui-gallery` and commit docs/gallery/tui/{base}.txt.",
            path.display()
        );
    }
}

#[test]
fn tui_goldens_never_leak_the_opponents_hand() {
    // Seat B's private opening hand (the redaction target). At the opening, Seat B has
    // NOTHING on the board, so any of these names appearing in a Seat-A screen would be
    // a pure leak of the opponent's hand — exactly invariant 2.
    let engine = tui_gallery::new_engine();
    let b_hand: Vec<String> = engine
        .state()
        .player(Seat::B)
        .hand
        .iter()
        .map(|id| engine.card(*id).name.clone())
        .collect();
    assert!(!b_hand.is_empty(), "Seat B should hold an opening hand");

    // The opening-state snapshots — board, the Mulligan menu, and both Glimpse steps —
    // are rendered while Seat B has no board presence, so they are the clean redaction
    // surface: no B-hand name may appear. (`result` is excluded: by Nightfall B's
    // *placed* spirits are public board state and are legitimately named.)
    let opening_moments = [
        "board",
        "mulligan",
        "glimpse-burn",
        "glimpse-keep-bottom",
        "inspect",
    ];
    for moment in opening_moments {
        let screen = strip_ansi(&tui_gallery::screen(moment).expect("moment renders"));
        for name in &b_hand {
            assert!(
                !screen.contains(name.as_str()),
                "redaction breach: Seat B's hidden hand card '{name}' leaked into the \
                 Seat-A '{moment}' snapshot"
            );
        }
    }
}
