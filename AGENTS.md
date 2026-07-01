# AGENTS.md — working on Recollect (humans and AI agents alike)

Recollect is a board card game (Rust workspace) where two storytellers contend (Lorekeepers vs. the Solace)
over a fading Memory. **The documents are the law and the code follows them**
— never the reverse.

## The bar: signature-tier quality
Everything we ship is held to **signature tier** — Apple-grade: polished, high-quality,
user-focused; never a "cheap tier that just does the job." It governs **code, visual elements,
tests, gameplay, UX, UI, and ergonomics** alike. When a change offers a quick-but-adequate path
and a polished-but-more-work path, take the polished one. A fantastic, ergonomic user experience
is the default acceptance criterion — on top of the invariants below, never instead of them.

## Read these before changing anything
- `docs/design.md` — the rules reference (the design law); the source for
  mechanics. Trace rules → tests via `rules_coverage.md`.
- `app/crates/recollect-core/data/cards.toml` — all 419 cards (stats, rules, keywords,
  effects, evolution lines, lore, physical). THE source of card truth; `make catalog`
  regenerates `catalog.json` + the runtime side-data from it. See `docs/cards_design.md`
  for the template/architecture/exemplars + the naming law (the design prose).
- `docs/how_to_play.md` — the player-facing HOW-TO: the CLI + web interfaces,
  their controls, and the turn phases (links design §5 for the rules).
- `docs/difficulty.md` — the difficulty map: 1v1/2v2 × faction × tier win rates for a Hard-level player.
- `architecture.md` — engine shape, invariants table, verification routing.
- `docs/testing.md` — test taxonomy and conventions.
- `docs/operations.md` — make targets, compose stack, teardown.
- `docs/observability.md` — the instrumentation catalog: every metric (name, type,
  labels, emission point), the `#[instrument]` spans, the Grafana dashboards, the OTLP
  path, and the convention for adding a metric.
- `docs/best-practices.md` — the standard this repo holds itself to.
- `docs/decisions/` — the design rationale behind the current shapes (the web/UX
  design-of-record, the brand + a11y bars, the bot/ML and launch plans).
- `docs/roadmap.md` — **the single prioritized backlog (the source of truth for what to do next)**;
  the numbered deliverables (D-series) and the sequencing law.
- `docs/rules_coverage.md` — every rule/behaviour → the tests that guard it.
- `docs/engine.md` — a guided tour of `recollect-core` (decide/evolve, the
  event vocabulary, where each mechanic lives) — START HERE to change rules.
- `docs/decisions/playtest_launch_plan.md` — the website/accounts/infra plan.

## Non-negotiable invariants (tests enforce all of these)
1. **Determinism**: same seed + same commands ⇒ identical state and events.
   Entropy is counter-mode; the seed appears in NO event and NO view.
2. **Redaction**: `PlayerView` is the only thing a client sees. Never leak
   opponent hands, deck order, or Echo pre-knowledge.
3. **The catalog is generated.** Never hand-edit `catalog.json` (nor the runtime
   side-data: `effects.json`, `evolution_{lines,split}.json`, `card_{keys,keywords}.json`).
   Edit a `[[card]]` in `cards.toml`, run `make catalog`. CI diffs via `make catalog-check`.
4. **Vocabulary is law**: spirits are *banished* (never killed/slain/destroyed);
   only the Solace *unwrites*; "forgetting" is Solace-register only.
5. **Red tests are contracts**: `tests/suites/m1_backlog.rs` holds `#[ignore]`d
   specs for unbuilt features. Implement them; never delete them.
6. **The effects ratchet** (`tests/suites/effects_coverage.rs`): the implemented/data-only
   counts (every deck-playable card is engine-backed) may only move toward
   implementation — never regress.
7. **Accessibility is first-class** (alongside the vocabulary law): every
   user-facing interface change **maintains the accessibility tree**. On the web
   client that means the **virtual ARIA tree mirroring the canvas** — board, hand,
   and actions as actionable accessible elements, plus a **live region** for the
   announcements (see `docs/decisions/web_client_ux.md`); on the website,
   **semantic HTML a11y**. The bar is WCAG 2.1 AA (`brand_and_accessibility.md`).
   a11y is in scope, never an afterthought — a change that regresses the accessible
   path is not done.

## Before you call work done
The Rust workspace lives in `app/`; `make` runs `cargo` there for you. Run
these from the repo root:
```
make test            # fast suite; excludes the slow model-checker
make catalog-check   # doc/code drift gate
make test-verify     # the stateright model-check (~12s, run separately)
```
`make test` runs the fast suite (the workspace minus the slow
`recollect-verify` model-checker) under `cargo-nextest` plus a separate
`cargo test --doc` pass (nextest doesn't run doctests; the two together match
plain `cargo test`). It installs `cargo-nextest` on demand if missing. Run the
model-checker with `make test-verify`, or both with `make test-all`.
If you changed rules: update the design doc FIRST, then code, then tests.
If you changed cards: edit `cards.toml` → `make catalog` → tests.
If you added a feature the docs don't describe: stop, write the doc change,
get it agreed, then build.
**Keep the living docs current.** Any code change that makes a living doc inaccurate must fix that
doc in the SAME change — `architecture.md`, `engine.md`, `AGENTS.md`, `README.md`, the design/cards
docs, and `docs/{testing,operations,observability}.md` are **sources of truth, not notes**. Stale or deprecated
content (a retired module, a renamed flag, an old count) only confuses — **delete or correct it,
never leave it.** When you retire a concept, grep the docs for it and purge every reference.
Before a **release** (not every change), walk `docs/manual_verification.md` — the
sandbox-deferred checks CI can't do: the browser UI, real mouse/touch/keyboard input,
multi-client play, and the observability stack.

## Layout
The Rust workspace is under `app/` (root holds docs, ops, Docker files). The
**deploy** lives in `deploy/`: `deploy/pulumi/` (the §10.1 launch host as Pulumi IaC, split into TWO
projects — `foundation/` (run once: ECR repo + GitHub-OIDC CI push role) and `platform/` (run per
release: EC2 + Cloudflare Tunnel; the box PULLS the server image CI pushed to ECR); see
`deploy/README.md`), `deploy/compose/` (the deploy + local-run compose stacks; `docker-compose.build.yml`
is the local/smoke build overlay since prod pulls), and `deploy/helm/` (the §10.2 K8s scale target).
`app/crates/recollect-core` (engine: `decide`/`evolve`, family-shaped; the rules
live in `src/engine/` — 18 focused modules, none over ~1000 lines, see engine.md) ·
`-protocol` (versioned wire) · `-server` (axum ws + accounts + pg record) ·
`-cli` (the `recollect` binary: local/online play, TUI or headless) ·
`-bot` (AI + probes) · `-web` (wasm shell) ·
`-journal-postgres` (accounts + event record) ·
`-determinism` (D-26 wasm32 differential: a seeded playout hashed identically
native and under `wasmtime` — `make wasm-diff`) ·
`-ffi` (D-25 UniFFI native bindings over core — `make ffi-bindings`).
The `recollect` CLI is one binary over two orthogonal axes — transport
(local engine vs `--server` WebSocket) and interface (interactive TUI vs
headless JSON/autoplay).

## Code shape conventions
- **Split by concern; ~1000 lines is a tripwire, not a hard cap.** The real test is editability —
  one file, one concern, so a human or an AI can find a thing and change it without holding the
  whole file in their head (and the Edit tool gets a unique anchor). `~1000 lines` is a cheap signal
  to *go look at cohesion*, not a law: split when a file outgrows its concern, leave a long-but-
  coherent file alone. Exempt generated/data files (the catalog, big tables) — they aren't
  hand-edited. Split into sibling modules (`use super::*` shares the crate's helpers + types;
  re-export with `pub(crate) use <mod>::*`). The engine, `state` (→ `events`), `effects`
  (→ `support`), and `server` (→ `actor`/`matchmaking`/`ws`) are split this way; big test files by theme.
- **Every module + crate carries a `//!` doc; intra-doc links must resolve.** Each lib
  sets `#![deny(rustdoc::broken_intra_doc_links)]`; CI runs `RUSTDOCFLAGS=-D warnings`.
  (`missing_docs` is deliberately NOT enforced — most undocumented items are
  self-evident enum variants; document the API that isn't.)
- **Scripted multi-file refactors:** back up to `/tmp` first, capture a file's contents
  into a variable *before* rewriting (never read-after-truncate — it silently clobbers),
  and gate with the golden-replay corpus (`tests/golden_replay.rs`) + `make test`.

## Toolchain
Rust **1.96.0** is pinned in `rust-toolchain.toml` (CI, Docker, and dev machines
all build on it). Manifests use normal semver ranges; `Cargo.lock` is just the
current resolution — run `cargo update` freely and let CI gate it (a weekly
freshness job does this automatically). OpenTelemetry is **always compiled in**;
OTLP export is gated at runtime on `OTEL_EXPORTER_OTLP_ENDPOINT` (unset ⇒ JSON
logs only; set ⇒ also export traces over gRPC). There is no `otel` cargo feature —
one always-built path the test suite covers. **Logging convention:** diagnostics
go through `tracing` (the server command path carries `#[instrument]` spans +
events); raw `println`/`eprintln` is reserved for genuine UI/output — the CLI's
TUI board + JSON frames, the bot sims' reports, `recollect-determinism`'s hashes.
Don't log diagnostics with `println`.

## The Ironstate journal/lifecycle framework
`recollect-core`'s `GameState` implements `ironstate_aggregate::AggregateRules`
(crates.io), and the core re-exports `{AggregateRules, DrawPos, EntropySource}`
at its root. `Rng` (counter-mode) implements `EntropySource` with modulo-zone
overrides of the derived draws (do NOT remove them — ironstate's bit-mask default
would change every seeded outcome); the override is verified against ironstate's
reusable `assert_entropy_contract` plus golden-value pins in `rng.rs`.
- Postgres is **authoritative** when `DATABASE_URL` is set: the server routes each
  command through `Session::apply_journaled` (the engine's `decide_journaled`/
  `Decided` seam + `store::AsyncStore` append-before-ack); `resume_async` rebuilds
  a match from the journal. No `DATABASE_URL` ⇒ in-memory `Engine::apply` (graceful
  degrade).
- The seven-property `journal_contract_test!` bar is met by a synchronous `Journal`
  twin over the real schema (`make db-test`).

**Abandonment.** `Command::MatchAbandoned { seat }` is a system-issued forfeit:
resolvable on either seat's turn (it precedes the turn-ownership check in `decide`),
journaled as a distinct `Event::MatchAbandoned` (not a scored `MatchEnded`), and
`evolve` finishes the match as `Win(present_seat)`. It is excluded from
`legal_commands` (the transport gates who may issue it). **The SERVER issues it** on a
grace-expired disconnect: `socket_loop` signals the actor (`SeatVacated`) the instant a
socket tears down, the actor arms a per-principal grace timer, a reconnect disarms it, and
on expiry it applies `MatchAbandoned` against the absent seat's team through the normal
journaled apply funnel. Only HUMAN seats arm (a bot never forfeits); grace defaults to
**120s** (`?abandon_grace_secs=N`, `0` disables it); in 2v2 any one absent slot forfeits its
whole team.
