# tools/uitest — Playwright UI / end-to-end tests

Quarantined Node tooling that drives the **built** Recollect site
(`make site` → repo-root `dist/`) in a real browser, automating the feasible
checks from `docs/manual_verification.md`. It lives **outside** the cargo
workspace (`app/`) on purpose — exactly like `tools/cardpipe` — so its Node
dependencies never touch the engine's build graph. `node_modules/` is gitignored.

## Run it

From the repo root:

```
make uitest                 # build the site, then run the Playwright suite (default; fast + stable)
make uitest-update          # refresh the committed @visual picker baselines
make uitest-visual          # SEPARATE wgpu pixel/visual goldens (opt-in; GPU-deferred — see below)
make uitest-visual-update   # refresh the wgpu canvas goldens (run on a real GPU, same OS)
```

…which is just, from this directory:

```
npm ci
npx playwright install chromium   # one-time browser download
npx playwright test
```

The suite **builds nothing** — it serves the already-built `dist/` (via the
dependency-free `serve.mjs`) and tests it. `make uitest` does the `make site`
build first.

## What's here

- `playwright.config.ts` — three device **projects** (the common sizes: phone /
  tablet / desktop), the headed-Chromium launch (the wgpu canvas needs a real GL
  surface), and the `serve.mjs` web server.
- `serve.mjs` — a tiny static server for `dist/` that bridges two trunk
  build-output quirks (root-absolute asset URLs + an unhashed inline import); a
  production CDN/nginx config would do the same. See its header.
- `tests/` — `play-page`, `local-game`, `responsive`, `a11y`, `visual`, plus the
  deepened lanes: `site-responsive` (every marketing page × the mobile→desktop
  width band + a touch pass — static HTML, headless-safe) and `a11y-keyboard`
  (canvas a11y mirror: Tab traversal · deterministic focus order · live-region
  updates · reduced-motion) (+ `helpers.ts`). `visual-canvas.spec.ts` is the SEPARATE
  wgpu pixel-golden lane (below).
- `tests/visual.spec.ts-snapshots/` — committed visual baselines, **per OS**
  (`-darwin` so far). Regenerate with `make uitest-update` on the matching OS.

## wgpu pixel/visual goldens (separate target — `make uitest-visual`)

`visual-canvas.spec.ts` diffs the **real wgpu canvas render** against committed
golden PNGs. It is **decoupled** from `make uitest` (the config drops it unless
`UITEST_VISUAL=1`), so the default suite stays fast + stable and the goldens are
**ignorable / droppable** if flaky. It is **GPU-deferred**: the wgpu surface doesn't
preserve its buffer for Playwright's screenshot readback in the sandbox/CI (or headed
on the current dev box) — the frame reads back all-black — so each golden is guarded
and **skips** rather than committing a blank baseline. Capture + assert on a real GPU
where readback works; refresh with `make uitest-visual-update` on the same OS. The
golden PNGs are **binary** (merged via cherry-pick). Full rationale + the drop policy:
`docs/testing.md` → "wgpu pixel/visual goldens".

## Headed Chromium / xvfb

The board is drawn by **wgpu**, which needs a real GL surface; Playwright's
bundled headless-shell has none, so the suite runs **headed** full Chromium. On
Linux (CI) `make uitest` wraps the run in `xvfb-run` automatically; on a desktop
it just runs headed.

## CI

A `uitest` job in `.github/workflows/ci.yml` runs on **every push to main + pull
request**, like the rest of the suite (builds the site, installs browsers, runs
under xvfb). The Linux gate skips the OS-specific `@visual` tests
(`UITEST_GREP_INVERT='@visual'`). Not part of nightly.

See `docs/testing.md` → "UI / end-to-end" for the full rationale and what stays
manual.
