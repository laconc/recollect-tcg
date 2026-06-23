#!/usr/bin/env bash
# Regenerate the docs/gallery/tui/ IMAGE gallery — real terminal screenshots (PNG) +
# a short motion clip (GIF) of the cursor TUI — with charmbracelet/vhs. This is the
# IMAGE twin of tools/gen_tui_gallery.sh: that script writes the deterministic `.txt`
# goldens (TestBackend frames, asserted byte-for-byte in `make test`); this one drives
# the REAL ratatui cursor TUI in a headless terminal and photographs the same beats in
# colour — the gold cursor, the lifted-piece gold targets, the brass-gold overlays.
#
# Unlike the `.txt` goldens, the PNG/GIF are COMMITTED ARTIFACTS, not a CI gate (the
# same status as the wgpu canvas stills): they need a real pty + a browser to render,
# so they're regenerated on demand and reviewed by eye. The `.txt` goldens remain the
# deterministic, CI-enforced record.
#
# vhs drives a real headless terminal from a `.tape` script (tools/tui_tapes/*.tape)
# and emits PNG/GIF. It needs ttyd (the pty) + ffmpeg (the encoder), and downloads a
# headless Chromium on first run. On macOS: `brew install vhs ttyd ffmpeg`.
#
# Determinism: every scene drives `recollect --seed 6` (the gallery seed; Seat A opens)
# and gates each capture on a `Wait+Screen /…/` of on-screen text — never a wall-clock
# sleep — so the same beat is captured regardless of machine speed. The freshly-built
# debug binary is put on PATH so the tapes can call a bare `recollect` (no absolute
# paths baked into the committed `.tape` files).
#
# Usage:  tools/gen_tui_shots.sh            # build + render every scene into docs/gallery/tui/
#         tools/gen_tui_shots.sh board      # render only the named scene(s)
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$(pwd)"
OUT="docs/gallery/tui"
TAPES="tools/tui_tapes"

# --- Dependency gate: skip CLEANLY if the toolchain is absent (the wasm-diff/trunk
# pattern — print the install hint and exit 0, never hard-fail a `make`). ---
missing=""
for tool in vhs ttyd ffmpeg; do
  command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
done
if [ -n "$missing" ]; then
  echo "(tui-shots skipped — missing:$missing)"
  echo "  The IMAGE gallery needs charmbracelet/vhs + ttyd + ffmpeg (a real pty + encoder)."
  echo "  Install (macOS):  brew install vhs ttyd ffmpeg"
  echo "  Install (Linux):  see https://github.com/charmbracelet/vhs#installation"
  echo "  The committed PNG/GIF stand in until then; the .txt goldens (make tui-gallery) are unaffected."
  exit 0
fi

# --- Build the cursor-TUI binary once, then expose it on PATH for the tapes. ---
echo "building recollect-cli (debug) for the image gallery…"
( cd app && cargo build -q -p recollect-cli )
BIN_DIR="$ROOT/app/target/debug"
if [ ! -x "$BIN_DIR/recollect" ]; then
  echo "error: built binary not found at $BIN_DIR/recollect" >&2
  exit 1
fi
export PATH="$BIN_DIR:$PATH"

mkdir -p "$OUT"
# The PNG scenes still emit a throwaway VHS video (its `Output`) alongside the committed
# `Screenshot` still; sink those videos here so only the GIF scene writes to docs/.
SINK="/tmp/vhs-recollect-tui"
mkdir -p "$SINK"

# The scenes (each a committed tools/tui_tapes/<name>.tape). PNG stills, then the GIF —
# the same beats as the .txt gallery, plus the motion clip.
ALL=(board pickup inspect glimpse mulligan result cursor)
scenes=("${ALL[@]}")
if [ "$#" -gt 0 ]; then
  scenes=("$@")
fi

failed=""
for name in "${scenes[@]}"; do
  tape="$TAPES/$name.tape"
  if [ ! -f "$tape" ]; then
    echo "skip: no tape $tape" >&2
    continue
  fi
  echo "vhs → $name"
  # Run from the repo root so the tapes' repo-root-relative Source/Output/Screenshot
  # paths resolve. `--quiet` keeps the rendered-command echo out of the log. One scene
  # failing must not abort the rest (it needs a browser + pty — environment-sensitive),
  # so collect failures and report them at the end rather than `set -e`-ing out.
  if ! vhs --quiet "$tape"; then
    echo "  ! $name failed to render" >&2
    failed="$failed $name"
  fi
done

if [ -n "$failed" ]; then
  echo "tui image gallery → $OUT/  (some scenes FAILED:$failed — see above; rerun: tools/gen_tui_shots.sh$failed)" >&2
  exit 1
fi
echo "tui image gallery → $OUT/  (PNG stills + cursor.gif)"
