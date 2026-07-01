# Testing — taxonomy, commands, tripwires

Run everything: `make test` (the fast suite — the workspace minus the slow
`recollect-verify` model-checker). CI runs this plus `make catalog-check` on
every push; `make test-verify` runs the model-checker.

`make test` runs the suite under [`cargo-nextest`](https://nexte.st) (it
schedules every test across all binaries in one parallel pool — faster than
cargo's per-binary runner — and prints a clean per-test report) **plus a
separate `cargo test … --doc` pass**, because nextest does not run doctests.
The two together cover exactly what plain `cargo test` did: same tests, same
doctests. The target installs `cargo-nextest` on demand if it is missing
(`cargo install --locked cargo-nextest`); CI installs the prebuilt binary.
The build's bottleneck is compile + link, not the test run; `app/.cargo/config.toml`
(opt-in, git-ignored — copy it from `config.toml.example`) switches the linker
to `lld`, which is much faster at the link step.

## The taxonomy

| Layer | Where | What it proves |
|---|---|---|
| Rules unit tests | `recollect-core/tests/suites/rules.rs` | Every law in the design doc: combat math, Arrival Law, interception caps, Momentum, Overwrite, Echo, the Dusk, Held Ground, clock. Each test name states the law it owns. |
| Keyword tests | `tests/suites/keywords.rs` | Each combat keyword's contract, asserted directly: Arcane pierce, Warded, Mobile steps, Steadfast, Relentless chains, Attune, Mourner, the Throughline. |
| Canon tests | `tests/canon.rs` | All 419 cards load; architecture math (114 spirits+callers / 60 evolutions — 48 Lorekeeper forms + 12 Solace Primal Deepenings / 120 spellbook / 92 Solace / 27 Foundlings / 6 Kindred); C/U budget-curve audit; Quick Play legality from canon; canon matches reach Nightfall. |
| Effects ratchet | `tests/suites/effects_coverage.rs` | The honesty meter: every deck-playable card is engine-implemented (the ratchet may only move toward implementation), AND **every effect-bearing NON-deck card** (Solace Unwritten/IllIntent creatures, Unwriting events, Foundlings, Kindred) is engine-backed too — `every_non_deck_effect_bearing_card_is_engine_backed` HARD-FAILs on any authored-but-dead effect, closing the card-red-team #78 blind spot where a non-deck spec could be authored, ratcheted, and silently never fire. |
| Determinism | `tests/suites/determinism.rs` (+ `src/rng.rs` unit tests) | Same seed + commands ⇒ identical state and events; counter-mode entropy; snapshot/replay identity. The `Rng` `EntropySource` impl is verified against ironstate's reusable `assert_entropy_contract` (in-range/coverage, seek round-trip + rewind, pure probe) plus golden-value pins for its exact modulo-zone stream in `rng.rs`. |
| Redaction | `tests/suites/redaction.rs` | `PlayerView` leaks no hidden information: opponent hands, deck order, seeds, Echo pre-knowledge. |
| Property-based (proptest) | `tests/suites/props.rs` | Proptest properties over a generated (seed + legal-command-index stream) that **shrink** any failure to the minimal command sequence: determinism (events+entropy+state identical on replay), legal-never-rejects, redaction (no view leaks the opponent's hand), and the shared invariant suite after every command (which includes the score≤board bound). Per-PR `PROPTEST_CASES=64`; crank for nightly. The broad/deep random playouts live in the full-catalog playthrough (`suites/fuzz.rs`, `make fuzz`); this is the minimal-counterexample half. |
| Gameplay fuzz / red-team — the FULL-CATALOG playthrough (**`make fuzz` / `make soak`**) | `tests/suites/fuzz.rs` (harness) + `tests/suites/redteam_playthrough.rs` + `tests/suites/redteam_rules_change.rs` (repros) | The heavy one, and the **all-cards** interaction/sequencing/illegal-state red-team — the class the per-card outcome tests miss because it only surfaces playing the FULL set through a game. The **harness** builds canon decks (all 419 cards) for 1v1 Lorekeeper, 1v1-vs-Solace, and 2v2, drives a board-shaping random legal playthrough to the finish, and after EVERY command asserts `invariants::check`, snapshot→restore parity, a redaction probe (the view serializes for both seats with no opponent-hand/peek/pending leak), and two lost-mutation guards (a fading spirit never outlives its standing-Faded window; a stray telegraph never goes stale). Three further arms over the same canon decks + every mode: `canon_replays_are_bit_identical` (DETERMINISM — same seed + policy ⇒ identical event stream + entropy-draw count on a re-run), `canon_rejected_commands_leave_no_trace` (REJECTION — a spray of out-of-range commands at each step leaves the snapshot byte-identical and entropy unmoved), and the SOAK mode (a `FUZZ_SECONDS` wall-clock budget). Per-PR runs a few hundred playouts; crank it via `RT_SEEDS=N` (count — `make fuzz SEEDS=N`), `RT_SEED_BASE=M` (shift the window so back-to-back runs cover **disjoint** ranges — base 0 then base N ⇒ 2N unique games, or a nightly job shards), or `FUZZ_SECONDS=N` (wall-clock — `make soak`). The **repros** are the minimal regressions distilled from it, each citing the design rule it restores: a spirit (and a Call) can't land on a terrain tile (`spirit AND terrain` illegal state), terrain can't be placed onto a Stray's tile (`terrain AND stray`, the §6-occupancy sibling of the spirit-onto-Stray guard), the Dusk sweeps bare rim terrain, a banished Kindred / a round-12 banished Unwritten leaves no impression and no illegal state. `redteam_rules_change.rs` carries the post-rules-change scenarios (the §5.4 Throughline lifecycle end-to-end, Overwrite-reaches-Stray win/lose/deny, and the §11 Unwritten-banishes-a-player path). |
| Model check | `app/crates/recollect-verify` | The stateright bridge: EXHAUSTIVE (not sampled) exploration of the bounded state space — BOTH Solace PvE and 1v1, via one parameterized `EngineModel`. Asserts SEVEN properties on every reachable state: the shared `invariants::check`, liveness, **determinism** (decide re-run is byte-identical), **no seed leak** (state/event/view), **redaction** (opponent crosses only as truthful counts), **abandonment** (`MatchAbandoned`⇒`Win(present_seat)`, which `legal_commands` can't reach so the BFS can't — checked as a property instead), and **mulligan** (§5: wherever the opening `Mulligan` is offered, it reshuffles cleanly — hand −1, page conserved, once-flag set — and never leaks the redrawn hand into the opponent's view). Each is mutation-tested: break the invariant and the named property goes red. ~6s gate (`make test-verify`, both modes); the `solace-modelcheck` bin runs the full frontier for both. |
| Card-effect execution | `tests/suites/card_effects_fire.rs` | Fires every card.s on-arrival/reveal spec and asserts it emits an event .no silent no-ops.. Found the AdjacentAlliesAll instant-heal gap (Rillsong Tadpole / Picnic Blanket). |
| Persistence/view fuzz | `tests/suites/fuzz.rs` (the `play` harness + `redaction_probe`) | After every command: snapshot→restore is byte-identical + offers the same legal set; the PlayerView serializes for both seats over the full canon catalog. Guards server restart + client deserialization. |
| Difficulty calibration | `bin/calibrate.rs`, `bin/strength.rs`, `bin/tier_sweep.rs`, `bin/char_sweep.rs`, `bin/char_sweep_2v2.rs` | Head-to-head tier win rates + PvE-faction win rates (Wilson CIs): `calibrate` verifies the difficulty ladder is monotone (the `the_difficulty_ladder_is_monotonic` gate pins the full adjacent ordering), `strength` that the agent beats random, `tier_sweep` is the `(temp, depth)` knob search behind a re-band (profile / candidate-ladder / pve probes), and `char_sweep`/`char_sweep_2v2` the per-character 1v1/2v2 Solace fairness (the Bal2 re-sweep evidence; see `docs/difficulty.md`). Found the PeekDeck cross-turn soft-lock. |
| Bot action-mix probe (data) | `bin/action_mix.rs` | **Not a `make test` gate — a data probe.** Where calibration answers *who wins*, this tallies *what the bot does*: the share of turns it spends on each activity (Play / Call / Evolve / Devolve / Glimpse / Move / EndTurn / Mulligan + the rest of the command vocabulary, summing to ~100%), broken out per **tier × faction × phase** (opening / mid / post-Dusk). Driven through the same public `legal_commands`/`apply` seam a client uses, so the mix is the real-game mix; the maintainer reads it to judge **human-sensibility** (does the bot evolve/devolve/glimpse/call/move at thoughtful-player rates?). Findings in [`docs/decisions/bot_action_mix.md`](decisions/bot_action_mix.md). |
| Security fuzz | `tests/suites/security.rs` | 8,000 garbage/hostile commands; engine rejects without state change or panic. Plus the bounds-rejection regression. |
| Red-test backlog | `tests/suites/m1_backlog.rs` | `#[ignore]`d tests that SPECIFY not-yet-built features — the ignore reason is the contract. Never delete one — implement it (as the `MatchAbandoned` forfeit just was). |
| Protocol | `recollect-protocol` | Wire round-trips, version tagging, postcard size sanity. |
| Server session | `recollect-server` | Seq replay rejection, rule rejection without state change. |
| Client parsing | `recollect-cli` | CLI verbs → `Command` mapping (`online` verb parser); the cursor TUI's pick-up→place → the right `Move`/`Play`/`Evolve` (`src/tui.rs` unit tests). |
| TUI snapshots | `recollect-cli/tests/{tui_gallery,cursor_tui}.rs` | Golden **text** stills (`docs/gallery/tui/*.txt`): the **line REPL** screens (`tui_gallery`) and the **cursor TUI** frames (`cursor_tui`, via `ratatui` `TestBackend`), each byte-equal + a **redaction** assertion (no Seat-B hand in a Seat-A frame). Seeded, GPU-free, TTY-free. These are the CI-gated record. |
| TUI image gallery (manual artifact) | `tools/tui_tapes/*.tape` → `docs/gallery/tui/{shot-*.png,cursor.gif}` (`make tui-shots`) | **Not a `make test` gate** — committed colour screenshots + a clip of the cursor TUI, the image twin of the `.txt` goldens (gold cursor, brass-gold theme, lifted-piece targets). Driven by [`charmbracelet/vhs`](https://github.com/charmbracelet/vhs) in a real headless terminal, so it needs **ttyd + ffmpeg** (+ a browser vhs fetches on first run) — `brew install vhs ttyd ffmpeg`; absent ⇒ `make tui-shots` prints the hint and **skips cleanly**. Same seed (`6`) as the goldens; captures gated on on-screen text (`Wait+Screen`), not sleeps, so they're reproducible. Reviewed by eye like the wgpu canvas stills. |
| DB integration | `recollect-journal-postgres` | `#[ignore = "requires postgres"]`; run `make dev-up && make db-test`. Accounts, token hashing, seq conflicts, draws_after. |
| wasm32 differential (D-26) | `recollect-determinism` | A seeded playout hashed native vs wasm32-wasip1 under `wasmtime`; the CI `wasm` job diffs them (`make wasm-diff` locally). Native-vs-wasm equality, not pinned values. |
| UI / end-to-end | `tools/uitest/` (Playwright) | Drives the **built** site (`make site` → `dist/`) in a real browser, automating the feasible `manual_verification.md` checks: the play page + picker render (the three decks, Easy/Normal/Hard/Expert); a local game mounts the board canvas, renders the labeled move buttons + the dedicated End Turn control, and shows the score + Anima HUD; the responsive law (no horizontal scroll, touch targets, board fits, End Turn stays reachable) — broadened to **every marketing page across the mobile→desktop width band** + a touch pass (`site-responsive.spec.ts`); a11y (`#board-sr` mirror present, the canvas a labeled keyboard-focusable input surface) — deepened with **keyboard traversal / deterministic focus order / live-region updates / reduced-motion** (`a11y-keyboard.spec.ts`); and `@visual` picker screenshots per breakpoint. `make uitest`. The **wgpu pixel/visual goldens** are a SEPARATE, decoupled lane — `make uitest-visual` (GPU-deferred; drop-if-flaky) — see "wgpu pixel/visual goldens" below. |

## Invariants registry — what must always hold, and what proves it

| Invariant | Statement | Guarded by | Run |
|---|---|---|---|
| Determinism | same seed + commands ⇒ identical state + events | `determinism.rs`, fuzz replay arm, wasm-diff, stateright "decide is deterministic" property | `make test` · `make wasm-diff` · `make test-verify` |
| Decide ≡ evolve | decide-time simulation reproduces from events alone | `determinism.rs` | `make test` |
| Redaction | `PlayerView` leaks no hidden info (hands, deck, seed, Echo) | `redaction.rs`, fuzz view arm, stateright "opponent crosses only as truthful counts" + "seed leaks into no…view" properties | `make test` · `make test-verify` |
| Abandonment | `MatchAbandoned`⇒match finishes as `Win(present_seat)` | `m1_backlog.rs` (green), stateright "abandonment forfeits to the present seat" property | `make test` · `make test-verify` |
| Score ≤ board | score never exceeds the tile count | fuzz `check_invariants`, stateright property | `make fuzz` · `make test-verify` |
| No overheal | `hp ≤ hp_max` always | fuzz `check_invariants`, stateright property | `make fuzz` · `make test-verify` |
| Liveness | every non-finished state offers ≥1 legal command | fuzz, stateright property | `make fuzz` · `make test-verify` |
| Termination | the Memory always ends within the clock | fuzz (step cap), `canon.rs` | `make fuzz` · `make test` |
| Snapshot identity | snapshot→restore is byte-identical + same legal set | fuzz persistence arm | `make test` |
| Hostile-input safety | garbage commands rejected, no state change/panic | `security.rs` | `make test` |
| Behavior stability | the rules don't drift unintentionally | `golden_replay.rs` (pinned baseline) | `make test` |
| Catalog ↔ source | `catalog.json` matches `cards.toml` | `canon.rs`, catalog-check | `make catalog-check` |

The **state-validity** rows (score≤board, no overheal, …) are **one shared suite** —
`recollect_core::invariants::check(state)` — consumed by all three runtime callers
(the fuzz harness, the stateright `properties()`, and `props.rs`), so they cannot
drift apart; `invariants.rs` is its single implementation. The stateright
`properties()` adds the engine-behaviour invariants that need more than a single
state to express — determinism (re-run), no-seed-leak, redaction, abandonment —
checked exhaustively over the bounded frontier (see the Model-check row above).

## Long-running verification (manual now; CI nightly schedules it)

Too slow for per-PR CI, runnable on demand — as `make` targets locally and via the
`nightly-verification` workflow (scheduled + the Actions "Run workflow" button):

| Command | What | Interval knob |
|---|---|---|
| `make fuzz SEEDS=20000 [BASE=0]` | a fixed number of seeded full-catalog playouts (`RT_SEEDS`/`RT_SEED_BASE`) | `SEEDS=` (count), `BASE=` (window) |
| `make fuzz SECONDS=60` · `make soak SECONDS=1800` | fuzz the full catalog for a wall-clock budget (`FUZZ_SECONDS`) | `SECONDS=` (time) |
| `make test-verify` | the stateright model-check | bounded depth |
| `make mutants FILE='**/engine/*.rs'` | mutation testing — do the tests catch injected bugs? | `TIMEOUT=` per mutant, `FILE=` scope |
| `make nightly` | all of the above in sequence | `SECONDS=`, `FILE=` |

`make mutants` needs `cargo install cargo-mutants`; kani proofs of the pure
combat/scoring math are a planned add.

**The full-engine sweep + the equivalent-mutant skip-list.** A periodic sweep runs
cargo-mutants across `recollect-core`'s rules logic (the highest-value reducers:
`combat`/`combat_stats`/`evolve`/`projection`/`clause`/`effects_exec`/`flow`/`decide`
and the redaction builder in `view.rs`). Each surviving mutant is triaged as a real
GAP — a missing or weak test, killed with an OUTCOME-asserting test that pins the exact
behaviour the mutant violated — or genuinely EQUIVALENT (no observable change, so no
test can ever catch it). The equivalents live in `app/.cargo/mutants.toml`'s `exclude_re`,
each with the reasoning inline, so they are never re-triaged and the survivor count stays
meaningful. **A survivor is a missing test until PROVEN equivalent** — never silence one
to make the number look good. The sweep's outcome-asserting kills landed in
`tests/suites/{bond_auras,effects_engine,rules,solace_effects,redaction}.rs` (bonded-combat
gates, the `effect_targets` adjacency geometry, the `finish` scoring/winner-determination,
the Unwritten inward-shift, and — HIGH, a security hole — three redaction leaks where a
hidden enemy lurker's Echo state or a face-down Fabrication's identity crossed the view).
Because in-place mutation needs the `docs/` tree the catalog-load test reads via
`include_str!`, the sweep runs `cargo mutants --in-place` (one file at a time) or, for
parallelism, across several git worktrees of the same branch.

## UI / end-to-end — Playwright over the built site

`make uitest` builds the site (`make site` → `dist/`) and drives it in a real
browser with **Playwright**, automating the feasible
`docs/manual_verification.md` checks. It lives in `tools/uitest/` — **quarantined
Node tooling OUTSIDE the cargo workspace** (its own `package.json` +
`node_modules`, gitignored), exactly like `tools/cardpipe`: the Node toolchain
never widens the engine's dependency graph or build/test time.

- **Layout.** `tools/uitest/{playwright.config.ts, serve.mjs, tests/}`. A
  dependency-free Node static server (`serve.mjs`) serves `dist/` over http
  (the wasm client can't `fetch`/instantiate over `file://`) and bridges two
  build quirks of the trunk artifact — the play client is emitted with
  root-absolute asset URLs and a hashed wasm/JS the hand-written inline bootstrap
  imports unhashed (see its header). One config, three device **projects** — the
  common web/mobile sizes: **phone** (Pixel 5), **tablet** (iPad), **desktop**
  (1280×900) — so the responsive law is asserted at each breakpoint.
- **What it covers.** The play page + picker (the three decks, the
  Easy/Normal/Hard/Expert difficulty); a local game (board canvas mounts, the
  labeled legal-move buttons + the dedicated **End Turn** control render, the HUD
  shows score + Anima); responsive (no horizontal scroll, touch targets on coarse
  pointers, the board fits, the move list scrolls in its own box without hiding
  End Turn); a11y (the `#board-sr` screen-reader mirror is present and live, the
  canvas is a labeled, keyboard-focusable input surface described by that mirror,
  moves are keyboard-focusable labeled buttons); and `@visual` picker screenshots —
  a committed baseline per breakpoint for future diffs.
- **Deepened responsive / touch / a11y (default suite).** Two specs broaden the
  fast, headless-safe coverage. `site-responsive.spec.ts` sweeps **every marketing
  page × the full width band** (mobile 320/393 · tablet 768/1024 · desktop
  1280/1920) — no horizontal scroll, the `<h1>`/`<main>`/nav spine survives the
  reflow, the skip-link + every in-page anchor resolves — plus a **coarse-pointer
  (touch)** pass: the button-like play CTA clears the **44px** (WCAG 2.5.5, AAA)
  target, the inline top-nav links clear the **24px** (WCAG 2.5.8, AA) floor, and a
  real touch tap navigates. (All static HTML, run JS-off — it renders faithfully
  headless, so it stays fast + stable anywhere.) `a11y-keyboard.spec.ts` deepens the
  canvas a11y mirror: every actionable node is a focusable Tab stop with a stable
  accessible name, the **reading order is deterministic**, sequential **Tab
  traversal** reaches the global controls with no keyboard trap, the `#status` live
  region is polite and its text **updates** as play advances, and
  `prefers-reduced-motion` collapses the paced replay. (These need the wgpu shell to
  mount, so each defers — skips, never fails — where there is no GL surface; see the
  visual-goldens note below and `manual_verification.md`.)
- **Headed Chromium under a virtual display.** The board is drawn by the **wgpu**
  ink renderer, which needs a real GL surface — Playwright's bundled
  headless-shell has none (no WebGPU; its software WebGL is rejected by wgpu), so
  the suite runs **headed** full Chromium. On a Linux runner `make uitest` wraps
  the run in `xvfb-run` (a virtual display) automatically; on a desktop it just
  runs headed.
- **Seed-agnostic.** The client seeds from `Date.now()`, so the opening hand
  varies between runs; the tests assert the UX contract (button shapes, labels
  reference real hand cards, the HUD/score/Anima format), never a specific hand.
- **CI lane.** A `uitest` job in `.github/workflows/ci.yml` runs on **every push to
  main + pull request**, like the rest of the suite. It builds the site, installs
  browsers, and runs the suite under xvfb. `@visual` baselines are OS-specific
  (committed for darwin so far), so the Linux gate skips them via
  `UITEST_GREP_INVERT='@visual'`; to add Linux baselines run `make uitest-update`
  on Linux and commit them, then drop the filter. (Not part of nightly.)
- **What stays manual (`manual_verification.md`).** Real input devices
  (genuine touch/keyboard ergonomics, the "stuck-hovered" look), multi-client
  online play, the observability stack, and screen-reader *announcement* (we assert
  the mirror's content + live regions, not what a real AT voices). The wgpu **pixel**
  output now also has an opt-in golden target (next) — still GPU-deferred, so the
  manual eyeball pass remains the backstop.

### wgpu pixel/visual goldens — a SEPARATE, decoupled target

`make uitest-visual` (a.k.a. `UITEST_VISUAL=1 npx playwright test
visual-canvas.spec.ts`) diffs the **real wgpu canvas render** of the live play shell
against committed golden PNGs — the highest-fidelity check of the renderer's output,
and the **flakiest** lane (GPU/driver/anti-aliasing variance). It is **decoupled from
`make uitest` on purpose** (the maintainer's decision): `playwright.config.ts` drops
`visual-canvas.spec.ts` from collection unless `UITEST_VISUAL` is set, so the default
suite stays fast + stable and these goldens are **ignorable if flaky**.

- **What it captures.** Three canvas set-pieces (per breakpoint project): the resting
  play shell, a hand card lifted with its legal tiles glowing, and the opening
  Mulligan overlay. The board content is made deterministic by **freezing the client
  clock** (the seed derives from `Date.now()`), so the only run-to-run delta is
  rendering noise.
- **Flakiness tuning (anti-aliasing).** The comparison uses a generous per-pixel
  `threshold` (0.3 — colour-distance tolerance, so AA fringing on the SDF rounded
  corners / glyph antialiasing isn't a diff) and a `maxDiffPixelRatio` (0.05 — a
  small fraction of differing pixels allowed). Animations are disabled. If a real GPU
  shows a golden flaking on noise, **tune these up first** (in `visual-canvas.spec.ts`'s
  `PIXEL`).
- **Refresh the goldens.** `make uitest-visual-update` (Playwright
  `--update-snapshots`). Run it **on the same OS that will assert them** (the PNGs are
  per-platform — the snapshot suffix is `-<project>-<platform>`), on a machine where
  the wgpu canvas actually **reads back** (see GPU-deferred). The PNGs land under
  `tools/uitest/tests/visual-canvas.spec.ts-snapshots/`. They are **binary** — the
  maintainer merges this binary-bearing lane via **cherry-pick**.
- **GPU-DEFERRED (why no PNGs are committed yet).** The board is a wgpu surface
  presented through the compositor with `preserveDrawingBuffer: false`, so
  Playwright's screenshot (which reads back the canvas **backing store**) returns an
  **all-black** frame wherever the surface contents aren't retained for readback —
  the headless/CI sandbox **and** headed Chromium on the current dev box (verified: a
  real ANGLE/Metal adapter is present, yet readback is 99.9% black). This is the same
  reason the canvas **gallery** stills come from the CPU rasterizer, not the canvas.
  So each golden is **guarded**: the spec proves the canvas actually painted (a
  non-black readback) and otherwise **SKIPS** (never fails, never writes an all-black
  baseline). The target + harness + the three golden *definitions* ship ready; their
  PNGs are captured + asserted only on an environment where wgpu readback works (a
  real GPU + a browser/driver that preserves the buffer for `toDataURL` — e.g. a
  configured Linux runner). Until then the live wgpu pixels stay covered by the manual
  eyeball pass in `manual_verification.md`.
- **DROP POLICY (drop-if-unfixably-flaky).** This lane is intentionally disposable. If
  the goldens prove **flaky on a real GPU and can't be tuned** (after raising the
  thresholds and stabilising the seed), **drop them**: delete
  `tools/uitest/tests/visual-canvas.spec.ts` and its `…-snapshots/` directory, and
  remove the `uitest-visual{,-update}` make targets — **nothing else depends on them**
  (the default `make uitest` already excludes the file, and the engine/site builds are
  untouched). Pixel goldens are a nice-to-have, never a gate.

## Probes (not tests — monitoring)

`make probes` runs the nightly fleet: seat win rates (law: A .469 / B .449 /
draw .082), rim-vs-inner texture (tripwire: rim > 30%), camper viability.
Probes detect drift experiments can't; results land in the design doc's
evidence sections.

## Test-binary layout

Cargo compiles and links a **separate test binary for every `.rs` file directly
under a crate's `tests/`**. On a constrained machine (few cores, a slow system
linker) that link step, not the test run, is the wall-clock bottleneck — so each
crate's integration suite is collapsed into **one** linked binary (`lld` attacks
the per-link cost; this attacks the *number* of links).

**The pattern.** Move each `tests/foo.rs` into `tests/suites/foo.rs` (a
*subdirectory*, so Cargo no longer auto-discovers it as its own target) and add a
`tests/main.rs` that pulls each in as a module:

```rust
// tests/main.rs — one linked binary for the whole crate's integration suite
mod suites {
    mod agent;
    mod fleet_tripwires;
    // …one `mod` per file in tests/suites/
}
```

Only a suite's module path gains a `suites::<file>::` prefix; the test bodies are
unchanged. A shared helper module (`recollect-core`'s `tests/common/mod.rs`) is
declared once at the top of `tests/main.rs` as `mod common;` and referenced from
the suites as `crate::common::*`. `recollect-core` and `recollect-bot` each carry
one such `tests/main.rs`.

**Kept as their own binaries (deliberately NOT consolidated)** — a Makefile target
invokes each by name with `--test <name>`, and a consolidated `main.rs` has no such
target, so merging them would break that target:

| File | Target | Why separate |
|---|---|---|
| `recollect-core/tests/golden_replay.rs` | `make nightly` | pinned behaviour baseline, run by name |
| `recollect-core/tests/canon.rs` | `make catalog` / `make catalog-check` | the catalog gate runs `--test canon` |
| `recollect-cli/tests/online_roundtrip.rs` | `make test-slow` | live-server integration (`--ignored`); 1 file anyway |
| `recollect-verify/tests/solace_bridge.rs` | `make test-verify` | the model-checker; 1 file anyway |

The gameplay fuzz (`make fuzz` / `make soak`) is the exception that does NOT need its own
binary: its harness (`suites/fuzz.rs`) stays a consolidated module, and the
targets select it in release by **test-name filter** against the crate's `main` binary
(`cargo test --release -- playthroughs_hold_every_invariant canon_replays_are_bit_identical
canon_rejected_commands_leave_no_trace`). A name filter works where `--test <name>` can't reach
into a consolidated binary.

A file that depends on process isolation (a `#[global_allocator]`, `set_var`,
`OnceCell`/`lazy_static`, or `static mut`) must stay its own binary too. Crates
with fewer than three integration files (`recollect-protocol`,
`recollect-journal-postgres`, `recollect-determinism`) are left as-is — nothing to
gain.

## Conventions

- A bug fix ships with the test that would have caught it.
- Every `MatchRules` variant gets a props arm.
- New features start as red tests in `m1_backlog.rs` with the design-doc
  section in the ignore reason.
- The catalog is generated — never hand-edit `catalog.json` (nor the side-data:
  `effects.json`, `evolution_{lines,split}.json`, `card_{keys,keywords}.json`); edit
  the `[[card]]` block in `cards.toml` and run `make catalog`.
