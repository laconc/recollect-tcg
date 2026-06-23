import { defineConfig, devices } from "@playwright/test";

// Recollect UI/end-to-end harness — quarantined Node tooling (see package.json).
//
// These tests drive the BUILT site (`make site` -> repo-root `dist/`) in a real
// browser, automating the feasible checks from docs/manual_verification.md. We
// serve `dist/` over a static HTTP server (the wasm client is fetched via
// `fetch`/instantiateStreaming, which needs http:// not file://), and run the
// suite at phone / tablet / desktop projects so the responsive law is
// exercised at every breakpoint. (Headed full Chromium — the wgpu canvas needs a
// real GL surface; see GL_ARGS below.)

const PORT = Number(process.env.UITEST_PORT ?? 4417); // clear of OTLP/pg/server
const BASE_URL = `http://127.0.0.1:${PORT}`;

// The board is drawn by the wgpu ink renderer (BROWSER_WEBGPU | GL backends, with
// a WebGL2 downlevel fallback — see recollect-web/src/render.rs). wgpu needs a
// REAL graphics surface: Playwright's bundled `chromium-headless-shell` ships no
// WebGPU and its software-WebGL (SwiftShader) surface is rejected by wgpu
// ("create_surface_unsafe"), so the canvas never mounts there. The full Chromium
// build running HEADED gives a real GL/WebGPU surface wgpu accepts, so we launch
// headed (`headless: false`). On a Linux CI runner that means wrapping the test
// command in `xvfb-run` (a virtual display) — see `make uitest` / docs/testing.md;
// on a desktop (macOS/Windows) headed just works. `--ignore-gpu-blocklist` lets
// software/virtual GPUs through.
const GL_ARGS = ["--ignore-gpu-blocklist"];

export default defineConfig({
  testDir: "./tests",
  // The visual GALLERY specs (gallery.spec.ts = stills, gallery-clips.spec.ts = videos) are
  // DECOUPLED from the default run: `make uitest` runs ONLY the assertion suite
  // (a11y / responsive / local-game / visual-regression / replay / result), never the gallery,
  // so it's safe to run ANYWHERE — including a headless/no-GPU sandbox where the canvas presents
  // all-black. The committed gallery STILLS are produced solely by the deterministic CPU
  // rasterizer (`tools/gen_gallery.sh`); the video CLIPS are captured by hand on a real GPU. This
  // keeps a no-GPU run from ever writing all-black frames over docs/gallery/web/ or fighting the
  // rasterizer. To capture the clips on a real Mac/GPU, set UITEST_GALLERY=1 (which lifts this
  // ignore), then run `UITEST_GALLERY=1 npx playwright test gallery-clips.spec.ts`
  // (see docs/gallery/README.md).
  // The canvas gallery (CPU-rasterized stills + GPU clips) is ignored unless UITEST_GALLERY
  // is set; the SITE gallery (static-HTML stills, headless-safe) is ignored unless
  // UITEST_SITE_GALLERY is set. Both write committed files under docs/gallery/web/ (the
  // WEB-register gallery dir, parallel to docs/gallery/tui/), so neither runs in the default
  // assertion suite (`make uitest`).
  //
  // The WGPU PIXEL/VISUAL GOLDENS (visual-canvas.spec.ts) are a SEPARATE target — `make
  // uitest-visual` (or `UITEST_VISUAL=1 npx playwright test`). They diff the REAL wgpu canvas
  // render against committed golden PNGs, so they need a true GL surface AND are the flakiest
  // lane (GPU/driver/AA variance). Per the maintainer's decision they are DECOUPLED from
  // `make uitest` — ignored unless UITEST_VISUAL is set — so the default suite stays fast +
  // stable and the goldens are ignorable (and droppable) if they can't be tuned. See
  // docs/testing.md → "wgpu pixel/visual goldens" for the refresh + drop-if-flaky policy.
  testIgnore: [
    ...(process.env.UITEST_GALLERY ? [] : ["**/gallery.spec.ts", "**/gallery-clips.spec.ts"]),
    ...(process.env.UITEST_SITE_GALLERY ? [] : ["**/site-gallery.spec.ts"]),
    ...(process.env.UITEST_VISUAL ? [] : ["**/visual-canvas.spec.ts"]),
  ],
  // The wgpu canvas boot can be slow under parallel load on a software GL stack.
  timeout: 60_000,
  // CI can skip OS-specific suites (the @visual pixel baselines are committed
  // per-platform — darwin so far), e.g. `UITEST_GREP_INVERT='@visual'`.
  grepInvert: process.env.UITEST_GREP_INVERT ? new RegExp(process.env.UITEST_GREP_INVERT) : undefined,
  // The built site is the contract; never hit the network or a live server.
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  // The headed Chromium windows each spin up a wgpu/GL surface — heavy. Too many
  // in parallel starve the GPU/CPU and flake on actionability timeouts, so cap
  // the worker count (a couple of retries absorb the rare GL hiccup).
  retries: 2,
  workers: 2,
  reporter: process.env.CI ? [["github"], ["html", { open: "never" }]] : [["list"], ["html", { open: "never" }]],
  // Visual-regression baselines committed under tests/visual.spec.ts-snapshots/;
  // small anti-aliasing/font deltas are expected across machines, so allow a little.
  expect: {
    toHaveScreenshot: { maxDiffPixelRatio: 0.02, animations: "disabled" },
  },
  use: {
    baseURL: BASE_URL,
    trace: "on-first-retry",
    screenshot: "only-on-failure",
    // Headed full Chromium (see GL_ARGS) so the wgpu canvas gets a real surface.
    headless: false,
    launchOptions: { args: GL_ARGS },
  },

  // Build is done by `make uitest` before this runs; here we only SERVE dist/.
  // (Kept separate so a dev can `npx playwright test` against an already-built
  // dist/ without rebuilding.) serve.mjs is a dependency-free Node static server
  // that bridges two deploy-path quirks of the built client — see its header.
  webServer: {
    command: "node serve.mjs",
    url: BASE_URL,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
    env: { UITEST_PORT: String(PORT) },
  },

  // One device class per project so the responsive law is asserted at each
  // breakpoint. Screenshots are stored per-project (the suffix keeps baselines
  // distinct), so a phone baseline never diffs against a desktop one.
  projects: [
    {
      // Phone portrait, ~393px — the narrow-viewport case.
      name: "phone",
      use: { ...devices["Pixel 5"], browserName: "chromium" },
    },
    {
      // Tablet — forced to chromium (the iPad descriptor defaults to WebKit, but
      // we need the GL flags + one engine for stable screenshots).
      name: "tablet",
      use: { ...devices["iPad (gen 7)"], browserName: "chromium" },
    },
    {
      name: "desktop",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 900 } },
    },
  ],
});
