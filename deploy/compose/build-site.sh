#!/usr/bin/env bash
# Build the deployable static site + wasm play client, then bake in the deploy-time origin and
# (optionally) the Cloudflare Web Analytics beacon. Mirrors the repo `make site` steps so a dev
# box and the deploy image produce byte-identical output, plus two deploy bakes:
#   origin           — rewrite the play client's default server to window.location.origin, so the
#                      deployed client targets its OWN origin (same-origin wss). SITE_ORIGIN is
#                      the human-facing label of that origin (used in logs); the bake itself is
#                      origin-agnostic, so one build works for prod and localhost alike.
#   CF_BEACON_TOKEN  — inject the cookieless Cloudflare Web Analytics beacon into every page <head>
#
# Usage: deploy/compose/build-site.sh <out-dir>     (run from the repo root)
# Env:   SITE_ORIGIN (default https://your-domain.com — a placeholder; the bake is origin-agnostic,
#        so this is only a log label), CF_BEACON_TOKEN (default empty)
set -euo pipefail

OUT="${1:?usage: build-site.sh <out-dir>}"
SITE_ORIGIN="${SITE_ORIGIN:-https://your-domain.com}"
CF_BEACON_TOKEN="${CF_BEACON_TOKEN:-}"
echo "==> building site for origin ${SITE_ORIGIN} (client targets window.location.origin)"
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO"

echo "==> generating the card catalog page"
python3 tools/gen_cards_page.py

echo "==> assembling the static site into ${OUT}"
rm -rf "$OUT"
mkdir -p "$OUT"
cp -R site/. "$OUT"/

# The wasm play client. trunk must be on PATH and the wasm target installed (trunk fetches the
# matching wasm-bindgen + wasm-opt itself — installing wasm-bindgen-cli by hand risks a skew with
# the locked wasm-bindgen). Dev box: `cargo install --locked trunk` + `rustup target add
# wasm32-unknown-unknown`. The image mirrors CI: the pinned trunk binary release.
if command -v trunk >/dev/null 2>&1; then
  echo "==> building the wasm play client (trunk --release)"
  ( cd app/crates/recollect-web && trunk build --release --public-url /client/ )
  mkdir -p "$OUT/client"
  cp -R app/crates/recollect-web/dist/. "$OUT/client"/
else
  echo "!! trunk not found — the play client is REQUIRED for a deploy. Install: cargo install --locked trunk" >&2
  exit 1
fi

# --- Bake 1: the play client's default server origin ------------------------------------------
# index.html ships a dev default of http://localhost:8080 in two spots (the #srv input value and
# the fetch fallback). Rewrite BOTH to window.location.origin so the deployed client talks to its
# own page origin (same-origin wss through Cloudflare). Done on the COPIED client, not the source.
CLIENT_INDEX="$OUT/client/index.html"
if [ -f "$CLIENT_INDEX" ]; then
  echo "==> baking play-client origin → window.location.origin (same-origin wss; was localhost:8080)"
  # Done in python (portable across GNU/BSD sed; the source strings contain $(), quotes and a
  # regex, which sed dialects escape differently). Two exact, asserted replacements: the #srv
  # input's visible default → empty, and the fetch fallback → window.location.origin. The script
  # FAILS if either anchor is missing, so a client refactor that moves the default can't silently
  # ship a localhost build to production.
  CLIENT_INDEX="$CLIENT_INDEX" python3 - <<'PY'
import os, sys
path = os.environ["CLIENT_INDEX"]
html = open(path, encoding="utf-8").read()
swaps = [
    ('value="http://localhost:8080"', 'value=""'),
    ('($("srv")?.value || "http://localhost:8080")',
     '($("srv")?.value || window.location.origin)'),
]
for old, new in swaps:
    if old not in html:
        sys.exit(f"build-site: origin anchor not found (client changed?): {old!r}")
    html = html.replace(old, new)
open(path, "w", encoding="utf-8").write(html)
print("    play-client origin baked (2 anchors)")
PY
fi

# --- Bake 2: the cookieless Cloudflare Web Analytics beacon -----------------------------------
# Inject the official beacon <script> before </head> on every page, ONLY when a token is given.
# Cookieless, no PII (usage_tracking.md). Skipped cleanly when CF_BEACON_TOKEN is empty.
if [ -n "$CF_BEACON_TOKEN" ]; then
  echo "==> injecting Cloudflare Web Analytics beacon into every page <head>"
  # The beacon <script> is built INSIDE python from the bare token (the shell carries only the
  # plain token — no embedded quotes/backslashes — so it stays an array-free, lint-clean string).
  # python does a robust HTML-safe insert before the first </head> (sed across </head> casings is
  # brittle); it is idempotent (skips a page whose token is already present).
  export CF_BEACON_TOKEN
  find "$OUT" -name '*.html' -type f -print0 | while IFS= read -r -d '' f; do
    python3 - "$f" <<'PY'
import os, sys
path = sys.argv[1]
token = os.environ["CF_BEACON_TOKEN"]
beacon = (
    '<script defer src="https://static.cloudflareinsights.com/beacon.min.js" '
    "data-cf-beacon='{\"token\": \"%s\"}'></script>" % token
)
html = open(path, encoding="utf-8").read()
if token in html:           # idempotent: never double-inject
    sys.exit(0)
idx = html.lower().find("</head>")
if idx == -1:
    sys.exit(0)             # no head (e.g. a fragment) — leave it
open(path, "w", encoding="utf-8").write(html[:idx] + beacon + "\n" + html[idx:])
PY
  done
else
  echo "==> no CF_BEACON_TOKEN — skipping the analytics beacon"
fi

echo "==> site built: ${OUT} ($(find "$OUT" -type f | wc -l | tr -d ' ') files)"
