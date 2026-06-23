//! The `recollect` client as a **library** — the transports (local / online /
//! headless), the shared verb grammar, and the seat's-eye [`render`]. The
//! `recollect` binary (`main.rs`) is a thin CLI shell over this; exposing the same
//! modules as a lib lets the **`tui_capture` example** drive a seeded engine and
//! snapshot the exact text screens the running client prints (the deterministic,
//! GPU-free TUI gallery — the line-based twin of `recollect-web`'s `shell_preview`).
//!
//! Only what an out-of-crate consumer (the example, future tests) needs is surfaced
//! `pub`; the render functions build strings (see [`render`]), so a snapshot is just
//! the returned `String` with `NO_COLOR` set.
#![forbid(unsafe_code)]

pub mod headless;
pub mod local;
pub mod online;
pub mod render;
pub mod tui;
pub mod tui_gallery;
pub mod verbs;

use recollect_core::{Engine, Seat};

/// The numbered "Legal tellings" menu for `seat`, as a `String` — the same block
/// [`local`]'s interactive prompt prints (every legal command through the canonical
/// labeler, plus the input hint). Exposed for the `tui_capture` example so it can
/// snapshot the Glimpse / Mulligan menus without a TTY; the underlying builder stays
/// crate-private (it's an internal of the prompt loop).
pub fn tui_menu_string(engine: &Engine, seat: Seat) -> String {
    local::menu_string(engine, seat)
}
