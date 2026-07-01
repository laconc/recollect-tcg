#!/usr/bin/env bash
# Regenerate the docs/gallery/tui/ TEXT stills from the line-based terminal client
# (recollect-cli's `tui_capture` example). It drives a SEEDED `recollect-core` engine
# to each moment and writes the exact screen a player reads — the board render, the
# "Legal plays" menu, the inspect panel — as a committed `.txt` snapshot. This is
# the line-based twin of tools/gen_gallery.sh (the wgpu shell's PNG gallery): no GPU,
# no TTY, no seed lottery, so the goldens are DETERMINISTIC and reproducible anywhere.
# NO_COLOR is set so the snapshots carry no ANSI escapes (stable bytes). See
# docs/gallery/tui/README.md.
#
# These `.txt` goldens are the CI-gated record. For the COLOUR image twin — real
# terminal screenshots + a clip of the cursor TUI (gold cursor, brass-gold theme) — see
# tools/gen_tui_shots.sh (`make tui-shots`); those are committed artifacts, not a gate.
#
# Usage:  tools/gen_tui_gallery.sh                 # writes .txt into docs/gallery/tui/
#         OUT=/tmp/tui tools/gen_tui_gallery.sh    # writes elsewhere
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${OUT:-docs/gallery/tui}"
mkdir -p "$OUT"
ABS="$(cd "$OUT" && pwd)"
# NO_COLOR ⇒ stable goldens (no ANSI). The example is GPU-free + reads no stdin.
run() { (cd app && NO_COLOR=1 cargo run --quiet -p recollect-cli --example tui_capture -- "$1" "$2"); }

# moment -> committed basename. The opening (board + the Mulligan menu), the two
# Glimpse choice prompts (burn, then keep/bottom), the inspect panel, and Nightfall.
declare -a MOMENTS=(
  "board:tui-board"
  "glimpse-burn:tui-glimpse-burn"
  "glimpse-keep-bottom:tui-glimpse-keep-bottom"
  "mulligan:tui-mulligan"
  "inspect:tui-inspect"
  "result:tui-result"
)
for entry in "${MOMENTS[@]}"; do
  moment="${entry%%:*}"; base="${entry##*:}"
  run "$moment" "$ABS/${base}.txt"
done

echo "tui gallery stills -> $OUT/"
