import { test, expect, Page, Locator } from "@playwright/test";
import { startLocalGameOrSkip, handButtons, a11yTree } from "./helpers";

// ─────────────────────────────────────────────────────────────────────────────
// WGPU PIXEL / VISUAL GOLDENS — a SEPARATE, opt-in target (NOT part of `make uitest`).
//
// Unlike visual.spec.ts (which screenshots the DOM picker and MASKS the canvas
// because the wgpu surface varies machine-to-machine), THIS file diffs the REAL
// wgpu CANVAS render of the live play shell against committed golden PNGs. That is
// the highest-fidelity check we have of the renderer's output — and the FLAKIEST
// lane in the harness (GPU/driver/anti-aliasing variance, font hinting, sub-pixel
// layout). So, per the maintainer's decision, it is DECOUPLED:
//
//   • Run it only via `make uitest-visual` (or `UITEST_VISUAL=1 npx playwright test`).
//     The config (playwright.config.ts → testIgnore) drops this file unless
//     UITEST_VISUAL is set, so the default `make uitest` never runs it. The goldens
//     are therefore IGNORABLE if flaky — and DROPPABLE if they can't be tuned (just
//     delete this spec + its -snapshots/ dir; nothing else depends on it).
//   • Refresh the goldens with `make uitest-visual-update` (Playwright
//     `--update-snapshots`), on the SAME OS that will assert them (PNGs are
//     per-platform — the suffix is `-<project>-<platform>`), on a machine where the
//     wgpu canvas actually reads back (see GPU-DEFERRED below).
//
// FLAKINESS TUNING (anti-aliasing): screenshots compare with a generous per-pixel
// `threshold` (colour-distance tolerance, so AA edge fringing on the SDF rounded
// corners / glyph antialiasing does not count as a diff) AND a `maxDiffPixelRatio`
// (a small fraction of differing pixels is allowed). Animations are disabled and
// the seed is FROZEN (Date.now pinned) so the board CONTENT is deterministic — the
// only expected variance is rendering noise, which the thresholds absorb.
//
// GPU-DEFERRED (the load-bearing caveat). The board is a wgpu surface presented via
// the compositor with `preserveDrawingBuffer: false`, so Playwright's screenshot —
// which reads back the canvas BACKING STORE — comes back BLANK (all-black) on
// environments where the surface contents aren't retained for readback. That is the
// case in the headless/CI sandbox AND in headed Chromium on this dev box (verified:
// a real ANGLE/Metal adapter is present, yet the readback is 99.9% black). This is
// the SAME reason the canvas GALLERY stills come from the CPU rasterizer, not the
// canvas (docs/testing.md). So every golden here is GUARDED: `captureCanvasGolden`
// first proves the canvas actually PAINTED (a non-black readback); if it didn't, the
// test SKIPS (never fails, never writes an all-black baseline) — the goldens are
// captured + asserted ONLY where wgpu readback works (a real GPU + a browser/driver
// that preserves the buffer for `toDataURL`, e.g. a configured Linux runner). The
// remaining manual coverage of the live wgpu pixels stays in docs/manual_verification.md.
// ─────────────────────────────────────────────────────────────────────────────

// Freeze the client's clock BEFORE any page script so the seed-derived opening hand
// + board are deterministic (the client seeds from Date.now()). A stable seed ⇒ a
// stable render, so the only run-to-run delta is GPU/AA noise (which the thresholds
// below absorb). Same technique visual.spec.ts uses for the DOM picker baseline.
async function freezeSeed(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const FIXED = 1_700_000_000_000; // a fixed epoch ms — any constant works
    const RealDate = Date;
    // @ts-expect-error - override the no-arg constructor form + now()
    // eslint-disable-next-line no-global-assign
    Date = class extends RealDate {
      constructor(...args: unknown[]) {
        // @ts-expect-error - spread into the Date constructor
        super(...(args.length ? args : [FIXED]));
      }
      static now() {
        return FIXED;
      }
    } as DateConstructor;
  });
}

/** Settle the wgpu frame loop so the screenshot lands on a stable, fully-drawn frame
 *  (the renderer eases affordances in; a beat lets the GL present catch up under
 *  headed parallel load). Mirrors the gallery specs' `settle`. */
async function settle(page: Page, ms = 600): Promise<void> {
  await page.waitForTimeout(ms);
}

/** Has the wgpu canvas actually PAINTED to a readable backing store? Reads the
 *  canvas back and measures the fraction of non-near-black pixels. On a surface that
 *  doesn't preserve its buffer for readback this is ~0 (all-black) even when the
 *  board is visibly drawn on screen — the signal we use to DEFER (skip) rather than
 *  diff a blank frame. Returns the non-black ratio (0…1); a tiny epsilon (border
 *  antialiasing) is well below the threshold we gate on. */
async function canvasNonBlackRatio(board: Locator): Promise<number> {
  return board.evaluate((c: HTMLCanvasElement) => {
    try {
      const off = document.createElement("canvas");
      off.width = c.width;
      off.height = c.height;
      const ctx = off.getContext("2d");
      if (!ctx) return 0;
      // drawImage of the live wgpu canvas pulls its current backing store (blank if
      // the surface doesn't retain contents for readback).
      ctx.drawImage(c, 0, 0);
      const d = ctx.getImageData(0, 0, off.width, off.height).data;
      let nonBlack = 0;
      const total = d.length / 4;
      for (let i = 0; i < d.length; i += 4) {
        if (d[i] > 20 || d[i + 1] > 20 || d[i + 2] > 20) nonBlack++;
      }
      return total ? nonBlack / total : 0;
    } catch {
      return 0; // a tainted/again-unreadable canvas ⇒ treat as not-readable ⇒ defer
    }
  });
}

// The anti-aliasing-tolerant comparison knobs. `threshold` is the per-pixel colour
// distance (0–1) below which a pixel is considered unchanged — generous enough that
// AA fringing on edges/glyphs is ignored. `maxDiffPixelRatio` tolerates a small
// fraction of genuinely-differing pixels (GPU/driver noise). Tune these UP if a real
// GPU shows the goldens flaking on noise; if they still can't be stabilised, DROP the
// spec (the drop-if-flaky policy in docs/testing.md).
const PIXEL = { threshold: 0.3, maxDiffPixelRatio: 0.05, animations: "disabled" as const };

/** Capture the canvas region as a committed golden — but ONLY if the wgpu surface
 *  actually read back (non-black). Otherwise SKIP with the GPU-deferred reason, so
 *  this opt-in lane stays clean wherever readback doesn't work (the sandbox, CI, and
 *  this dev box) instead of writing an all-black baseline or failing. */
async function captureCanvasGolden(
  page: Page,
  name: string,
): Promise<void> {
  const board = page.locator("#board");
  await settle(page);
  const ratio = await canvasNonBlackRatio(board);
  test.skip(
    ratio < 0.01,
    `wgpu canvas reads back blank (non-black ${(ratio * 100).toFixed(2)}%) — GPU-deferred ` +
      `(the surface doesn't preserve its buffer for readback here; capture on a real GPU. ` +
      `See docs/testing.md → "wgpu pixel/visual goldens").`,
  );
  await expect(board).toHaveScreenshot(name, PIXEL);
}

test.describe("wgpu canvas visual goldens @visual-canvas", () => {
  test("the resting play shell renders to its golden", async ({ page }, testInfo) => {
    await freezeSeed(page);
    await startLocalGameOrSkip(page);
    // The resting shell: HUD · opponent strip · board · hand tray · the End-Turn /
    // Glimpse control lane, every affordance eased in. Capture the canvas region.
    await expect(handButtons(page).first()).toBeVisible();
    await captureCanvasGolden(page, `canvas-shell-at-rest-${testInfo.project.name}.png`);
  });

  test("a hand card lifted renders its lit-tiles golden", async ({ page }, testInfo) => {
    await freezeSeed(page);
    await startLocalGameOrSkip(page);
    // Pick up the first PLAYABLE hand card (the accessible twin of tapping it): it
    // lifts off the tray with its gild halo and lights its legal destination tiles
    // green ON THE CANVAS. If the frozen seed dealt no affordable opening play, skip
    // (the resting golden above still covers the static surface).
    const playable = handButtons(page).filter({ hasText: /playable|evolution form/ }).first();
    test.skip(
      (await playable.count()) === 0,
      "the frozen seed dealt no affordable opening play — the lift golden needs one",
    );
    await playable.focus();
    await page.keyboard.press("Enter");
    // Wait for the lift to register (the canvas re-renders; #detail populates as the
    // pick-up also inspects), then capture.
    await expect(page.locator("#detail .card")).toHaveCount(1, { timeout: 5_000 });
    await captureCanvasGolden(page, `canvas-hand-lifted-${testInfo.project.name}.png`);
  });

  test("the opening Mulligan modal renders its golden", async ({ page }, testInfo) => {
    await freezeSeed(page);
    // Reach the opening Mulligan WITHOUT dismissing it (startLocalGame would keep the
    // hand): open the picker, pick a telling, and capture the canvas with the modal up
    // — the blocking-overlay set-piece (scrim + Mulligan/Keep). Guard on a GL surface.
    await page.goto("/client/");
    const heading = page.getByRole("heading", { name: "Choose your telling" });
    try {
      await expect(heading).toBeVisible({ timeout: 30_000 });
    } catch (_) {
      test.skip(true, "no GL surface — picker never mounted (GPU-deferred)");
      return;
    }
    await page.locator("#picker .style").first().click();
    // The modal's Keep button proves the Mulligan overlay is up (its choice tree
    // replaces the game tree). If it never appears, the shell didn't mount → skip.
    const keep = a11yTree(page).locator("button[data-a11y-id='choice-keep-0']");
    try {
      await expect(keep).toBeVisible({ timeout: 15_000 });
    } catch (_) {
      test.skip(true, "no GL surface — the canvas shell did not mount (GPU-deferred)");
      return;
    }
    await captureCanvasGolden(page, `canvas-mulligan-${testInfo.project.name}.png`);
  });
});
