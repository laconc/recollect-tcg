#!/usr/bin/env bash
# Regenerate the docs/gallery/web/ canvas stills from the NATIVE, GPU-free preview rasterizer
# (recollect-web's `shell_preview` example). It builds the exact `ShellScene`/`Scene` the wgpu
# client draws and rasterizes it on the CPU — the same SDF rounded corners, soft drop shadows,
# vertical gradients, palette, and EB Garamond atlas glyphs — so the gallery is DETERMINISTIC and
# reproducible anywhere (no GPU, no headed browser, no seed lottery). See docs/gallery/README.md.
#
# These canvas stills land in docs/gallery/web/ — the WEB-register gallery dir, parallel to the
# terminal register's docs/gallery/tui/. The website-page stills (`site-*.png`) and the motion
# clips (`clip-*.webm`) share that web/ dir; each gallery dir holds ONLY generated files.
#
# Usage:  tools/gen_gallery.sh                # writes PNGs into docs/gallery/web/
#         OUT=/tmp/g tools/gen_gallery.sh     # writes elsewhere
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${OUT:-docs/gallery/web}"
mkdir -p "$OUT"
ABS="$(cd "$OUT" && pwd)"
run() { (cd app && cargo run --quiet -p recollect-web --example shell_preview -- "$1" "$2" "$3" "$4"); }

# moment -> committed basename (the 1v1 shell set + the 2v2 board + the dedicated inspect still +
# the special situations + the ONLINE/2v2 full-shell stills). Desktop (1280×900) and phone-portrait
# (412×915) for each.
declare -a MOMENTS=(
  "rest:shell-at-rest"
  "lifted:hand-lifted"
  "placed:placed-spirit"
  "inspect:inspect-detail"
  "special:special-situations"
  "2v2:board-2v2"
  # The FULL canvas shell for ONLINE play (launch-critical) + 2v2: built from the server's
  # REDACTED PlayerView / TeamView (no engine), the opponent counts/backs only.
  "online-1v1:shell-online-1v1"
  "online-2v2:shell-online-2v2"
  # The in-canvas Glimpse + Mulligan choice modals (the last big local-1v1 interaction).
  "glimpse-burn:glimpse-burn-prompt"
  "glimpse-keep:glimpse-keep-bottom"
  "mulligan:mulligan-prompt"
)
for entry in "${MOMENTS[@]}"; do
  scenario="${entry%%:*}"; base="${entry##*:}"
  run "$scenario" "$ABS/${base}-desktop.png" 1280 900
  run "$scenario" "$ABS/${base}-phone.png"   412 915
done

echo "canvas gallery stills -> $OUT/"
