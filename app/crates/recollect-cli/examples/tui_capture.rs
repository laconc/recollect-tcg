//! Deterministic **text** capture of the line-based terminal client — the TUI twin
//! of `recollect-web`'s GPU-free `shell_preview` rasterizer (`tools/gen_gallery.sh`),
//! but for the CLI, where the "render" is already plain text. It drives a SEEDED
//! `recollect-core` engine to each interesting moment (the logic lives in
//! [`recollect_cli::tui_gallery`]) and writes the exact screen a player reads to a
//! committed `.txt` snapshot under `docs/gallery/tui/`.
//!
//! No TTY, no GPU, no stdin: the same render functions the running client prints,
//! captured as strings. The gallery script sets `NO_COLOR` so the goldens carry no
//! ANSI escapes; the seed is fixed so every screen is byte-for-byte reproducible.
//!
//! Usage (the gallery script passes the committed path):
//!   cargo run -p recollect-cli --example tui_capture -- board docs/gallery/tui/tui-board.txt

use recollect_cli::tui_gallery;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let moment = args.get(1).cloned().unwrap_or_else(|| "board".into());
    let path = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| format!("/tmp/tui-{moment}.txt"));

    let Some(screen) = tui_gallery::screen(&moment) else {
        let names: Vec<&str> = tui_gallery::MOMENTS.iter().map(|(m, _)| *m).collect();
        eprintln!("unknown moment '{moment}' (one of: {})", names.join(", "));
        std::process::exit(2);
    };

    std::fs::write(&path, screen).expect("write tui snapshot");
    eprintln!("wrote {path} (moment '{moment}')");
}
