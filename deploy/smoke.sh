#!/usr/bin/env bash
# deploy/smoke.sh — black-box smoke test of the BUILT deploy artifact.
#
# Builds the deploy images and brings up the LOCAL deploy stack (the same single-origin
# compose production runs, minus the Cloudflare Tunnel + observability — server at
# http://localhost:8080, on-box Postgres), then proves the website + the game actually
# work FROM THE OUTSIDE before a real launch. The server is a black box: every assertion
# is plain HTTP/ws from the host, exactly what a browser or the `recollect` CLI sees.
#
# What it proves (each step exits non-zero on failure, dumping server logs):
#   1. BUILD + UP      build the deploy images, `up -d`, poll /healthz until healthy.
#   2. WEBSITE         GET / (title + nav), GET /client/ + its JS/wasm assets (the #96
#                      trunk-boot class where /client/ 404'd its own assets), GET /healthz
#                      — single-origin serving from the ONE server image.
#   3. THE GAME (ws)   a REAL PvP match driven through the server by two headless
#                      `recollect online --json` CLIENTS (host mints the match via the REST
#                      API, two clients join by token + autoplay). Asserts: the match is
#                      created, moves apply over the wire, the telling advances to a result
#                      (or a healthy move budget), and REDACTION holds (a client's view never
#                      carries the opponent's hand — only a count).
#   4. JOURNAL         a `journal_events` row exists in the on-box Postgres — the
#                      Postgres-authoritative append-before-ack path worked in the image.
#
# ALWAYS tears the stack down (a bash trap, even on failure / Ctrl-C). Idempotent +
# re-runnable. Invoked by `make deploy-smoke`, which passes the exact DEPLOY_COMPOSE
# plumbing + the local-only Postgres password the rest of the deploy targets use.
#
# Usage (prefer the make target):   make deploy-smoke
# Direct:  POSTGRES_PASSWORD=… deploy/smoke.sh
# Env knobs (all optional; the make target sets the first two):
#   COMPOSE_CMD        the full `docker compose -f … -f …` invocation (default: the local
#                      deploy overlay, resolved relative to this script's repo root).
#   POSTGRES_PASSWORD  on-box Postgres password (default: recollect-local-only — a throwaway
#                      name, matching DEPLOY_LOCAL_ENV; never a real secret).
#   BASE_URL           where the server is reached (default http://localhost:8080).
#   HEALTH_TIMEOUT     seconds to wait for /healthz to come up (default 180 — the first
#                      image build of the wasm client is the slow part).
#   MOVE_BUDGET        max applied moves before the game smoke gives up waiting for a result
#                      (default 400 — a full 12-round autoplay; a healthy handshake + several
#                      applied moves + the redaction check is already an accepted smoke).
#   KEEP_UP=1          skip teardown (debugging only — leaves containers running).
set -euo pipefail

# --- Resolve paths + config -------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

BASE_URL="${BASE_URL:-http://localhost:8080}"
HEALTH_TIMEOUT="${HEALTH_TIMEOUT:-180}"
MOVE_BUDGET="${MOVE_BUDGET:-400}"
export POSTGRES_PASSWORD="${POSTGRES_PASSWORD:-recollect-local-only}"

# The compose invocation. The make target passes COMPOSE_CMD (the canonical DEPLOY_COMPOSE); a
# direct run falls back to the same chain — the BUILD overlay (so the server image is compiled
# locally, since prod pulls it from ECR) + the local overlay — so the script stands alone too.
COMPOSE_CMD="${COMPOSE_CMD:-docker compose -f deploy/compose/docker-compose.deploy.yml -f deploy/compose/docker-compose.build.yml -f deploy/compose/docker-compose.local.yml}"
# shellcheck disable=SC2206  # deliberate word-split: COMPOSE_CMD is a command + flags.
COMPOSE=($COMPOSE_CMD)

# --- Pretty diagnostics -----------------------------------------------------------------
say()  { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
ok()   { printf '    \033[1;32mok\033[0m %s\n' "$*"; }
fail() { printf '\n\033[1;31mSMOKE FAILED: %s\033[0m\n' "$*" >&2; exit 1; }

# --- Teardown trap: ALWAYS, even on failure / interrupt ---------------------------------
# On a failed/interrupted run, dump the server logs first (the whole point of a smoke is to
# see WHY), then bring the stack down (taking the throwaway pg volume with -v so a re-run
# starts clean and idempotent).
cleanup() {
  local code=$?
  set +e
  if [ "$code" -ne 0 ]; then
    printf '\n\033[1;31m--- smoke failed (exit %s); dumping server logs ---\033[0m\n' "$code" >&2
    "${COMPOSE[@]}" logs --no-color --tail=120 server  >&2 2>/dev/null || true
    printf '\n\033[1;31m--- postgres logs (tail) ---\033[0m\n' >&2
    "${COMPOSE[@]}" logs --no-color --tail=40  postgres >&2 2>/dev/null || true
  fi
  if [ "${KEEP_UP:-0}" = "1" ]; then
    printf '\n\033[1;33m(KEEP_UP=1 — leaving the stack running; tear down with: %s down -v)\033[0m\n' "$COMPOSE_CMD" >&2
  else
    say "tearing down the local deploy stack"
    "${COMPOSE[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
  fi
  exit "$code"
}
trap cleanup EXIT INT TERM

# ========================================================================================
# 1. BUILD + UP
# ========================================================================================
say "building the deploy images + bringing up the local stack (no tunnel, no observability)"
# A clean slate first (idempotent re-runs): drop any stack + its throwaway volume.
"${COMPOSE[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
"${COMPOSE[@]}" up -d --build || fail "compose build/up failed"
ok "stack up"

say "polling ${BASE_URL}/healthz (timeout ${HEALTH_TIMEOUT}s)"
deadline=$(( $(date +%s) + HEALTH_TIMEOUT ))
healthy=0
while [ "$(date +%s)" -lt "$deadline" ]; do
  body="$(curl -fsS --max-time 5 "${BASE_URL}/healthz" 2>/dev/null || true)"
  if [ "$body" = "ok" ]; then healthy=1; break; fi
  sleep 2
done
[ "$healthy" -eq 1 ] || fail "/healthz did not return ok within ${HEALTH_TIMEOUT}s"
ok "server healthy (/healthz → ok)"

# ========================================================================================
# 2. WEBSITE — single-origin static serving from the server image
# ========================================================================================
# Assert an HTTP GET returns 200 AND the body contains an expected marker. `curl -w` gives
# the status; the body goes to a temp file we grep. A 200 alone isn't enough — the #96 bug
# was a 200 index whose /client/ assets 404'd, so we assert the assets themselves serve.
http_get_contains() {  # <path> <needle> <human label>
  local path="$1" needle="$2" label="$3" tmp status
  tmp="$(mktemp)"
  status="$(curl -fsS --max-time 10 -o "$tmp" -w '%{http_code}' "${BASE_URL}${path}" 2>/dev/null || echo 000)"
  if [ "$status" != "200" ]; then
    rm -f "$tmp"; fail "GET ${path} → HTTP ${status} (expected 200) [${label}]"
  fi
  if ! grep -qiF "$needle" "$tmp"; then
    rm -f "$tmp"; fail "GET ${path} 200 but missing marker '${needle}' [${label}]"
  fi
  rm -f "$tmp"
  ok "GET ${path} → 200 + '${needle}' [${label}]"
}

http_get_content_type() {  # <path> <expected content-type substring> <human label>
  local path="$1" want="$2" label="$3" ct status
  ct="$(curl -fsS --max-time 10 -o /dev/null -w '%{content_type}' "${BASE_URL}${path}" 2>/dev/null || echo '')"
  status="$(curl -fsS --max-time 10 -o /dev/null -w '%{http_code}' "${BASE_URL}${path}" 2>/dev/null || echo 000)"
  [ "$status" = "200" ] || fail "GET ${path} → HTTP ${status} (expected 200) [${label}]"
  if ! printf '%s' "$ct" | grep -qiF "$want"; then
    fail "GET ${path} 200 but Content-Type '${ct}' lacks '${want}' [${label}]"
  fi
  ok "GET ${path} → 200, Content-Type ${ct} [${label}]"
}

say "website: single-origin static serving (the site + the wasm play client)"
# The landing page: 200 + the brand/title marker AND the primary nav (semantic a11y chrome).
http_get_contains "/"            "Recollect"        "landing page title"
http_get_contains "/"            "site-nav"         "landing page nav"
# /healthz as plain HTTP (it is part of the same origin's surface).
http_get_contains "/healthz"     "ok"               "healthz"
# THE #96 CLASS: the wasm play client under /client/ and its assets must actually serve.
# The client index boots via `import init … '/client/recollect-web.js'` + the _bg.wasm —
# a 404 on either is exactly the trunk-boot regression #96 caught. Assert all three.
http_get_contains      "/client/"                       "recollect-web.js"   "play client index references its JS"
http_get_content_type  "/client/recollect-web.js"       "javascript"          "play client JS serves"
http_get_content_type  "/client/recollect-web_bg.wasm"  "application/wasm"    "play client wasm serves as application/wasm"
ok "single-origin serving proven (site + /client/ assets from the one server image)"

# ========================================================================================
# 3. THE GAME over ws — a real PvP match, two headless CLI clients
# ========================================================================================
# Mint a reproducible match via the REST API (a black-box curl), then drive BOTH seats with
# the `recollect online join --json` CLI (the exact client the website's network play uses).
# A Python driver (deploy/smoke_game.py) owns the lockstep: read each seat's view frames,
# and on a seat's own turn echo a legal move straight back (round-tripping `legal[i].cmd`,
# preferring EndTurn so the match marches to Nightfall). Every frame is checked for
# redaction. The CLI binary is a pure black box — we only spawn it and pipe JSON.

# Build the CLI FIRST (debug is fine — this is a client harness, not the artifact), so the
# slow compile happens BEFORE the match is minted — the match is then driven the instant it
# exists, with no idle gap. The SERVER under test is the Docker image; this binary is only
# the external client. The Rust workspace lives in app/ (the repo root has no Cargo.toml),
# so build from there.
say "building the recollect CLI (the external black-box client)"
( cd "$REPO_ROOT/app" && cargo build -q -p recollect-cli ) \
  || fail "failed to build the recollect CLI client"
CLI_BIN="$REPO_ROOT/app/target/debug/recollect"
[ -x "$CLI_BIN" ] || fail "recollect CLI binary not found at ${CLI_BIN}"
ok "CLI built"

# A deterministic Seat-A opener so the first frames are predictable (the toss is otherwise
# CSPRNG-seeded). 6 is the first seed that opens Seat A with no initiative bias (verified
# against decide_opener(seed, 0) == Seat::A). Either seat opening is fine for the smoke —
# the driver plays whichever seat is active — but a pinned opener keeps the run reproducible.
SEED=6

say "the game: creating a PvP match via POST /matches (seed=${SEED})"
CREATE_JSON="$(curl -fsS --max-time 10 -X POST "${BASE_URL}/matches?seed=${SEED}" 2>/dev/null || true)"
[ -n "$CREATE_JSON" ] || fail "POST /matches returned nothing"
MATCH_ID="$(printf '%s' "$CREATE_JSON"   | jq -r '.match_id // empty')"
TOKEN_A="$(printf '%s' "$CREATE_JSON"    | jq -r '.seat_a_token // empty')"
TOKEN_B="$(printf '%s' "$CREATE_JSON"    | jq -r '.seat_b_token // empty')"
COMMIT="$(printf '%s' "$CREATE_JSON"     | jq -r '.seed_commit // empty')"
[ -n "$MATCH_ID" ] && [ -n "$TOKEN_A" ] && [ -n "$TOKEN_B" ] \
  || fail "POST /matches missing match_id / seat tokens: ${CREATE_JSON}"
[ -n "$COMMIT" ] || fail "POST /matches did not publish a seed_commit (provably-fair shuffle): ${CREATE_JSON}"
ok "match ${MATCH_ID} created (both seat tokens minted, seed committed)"

say "driving the match: two headless 'recollect online join --json' clients autoplaying"
# The driver prints exactly one GAME_SMOKE_PASS / GAME_SMOKE_FAIL line; we relay its
# output and gate on the PASS marker.
GAME_OUT="$(
  CLI_BIN="$CLI_BIN" BASE_URL="$BASE_URL" MATCH_ID="$MATCH_ID" \
  TOKEN_A="$TOKEN_A" TOKEN_B="$TOKEN_B" MOVE_BUDGET="$MOVE_BUDGET" \
  python3 "$SCRIPT_DIR/smoke_game.py" 2>&1
)" || { printf '%s\n' "$GAME_OUT" >&2; fail "the game smoke driver errored"; }
printf '%s\n' "$GAME_OUT"
printf '%s' "$GAME_OUT" | grep -q '^GAME_SMOKE_PASS' \
  || fail "the game did not complete a healthy smoke (no PASS marker)"
ok "the game played over the wire (moves applied; redaction held)"

# ========================================================================================
# 4. JOURNAL — the Postgres-authoritative path recorded the match
# ========================================================================================
# With DATABASE_URL set (the deploy default), every applied command is appended to
# `journal_events` BEFORE the ack. After a real match there must be rows — proving the
# Postgres-authoritative path works inside the built stack. We query on-box (the server is
# the only thing with the DSN; psql runs in the postgres container).
say "journal: asserting the match was recorded in on-box Postgres (journal_events)"
ROWS="$(
  "${COMPOSE[@]}" exec -T postgres \
    psql -U recollect -d recollect -tAc 'SELECT COUNT(*) FROM journal_events;' 2>/dev/null \
    | tr -d '[:space:]'
)"
case "$ROWS" in
  ''|*[!0-9]*) fail "could not read journal_events row count (got '${ROWS}')" ;;
esac
[ "$ROWS" -gt 0 ] || fail "journal_events is empty — the Postgres-authoritative path did not record the match"
ok "journal_events has ${ROWS} row(s) — Postgres-authoritative append-before-ack confirmed"

# ========================================================================================
# Done — the trap tears the stack down.
# ========================================================================================
say "SMOKE PASSED — the built deploy artifact serves the site, plays a game, and journals it"
