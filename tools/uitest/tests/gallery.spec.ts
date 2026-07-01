import { test, expect, Page } from "@playwright/test";
import { startLocalGame, handButtons, tileButtons } from "./helpers";
import { activate, firstPlayable } from "./gallery-helpers";

// ─────────────────────────────────────────────────────────────────────────────
// VISUAL GALLERY (stills) — a committed media RECORD of the canvas-native client
// (#100 Phases A+B): the local 1v1 match where the wgpu canvas owns the whole
// surface (HUD · opponent strip · hand tray · End-Turn/Study FABs) and the
// in-canvas affordances drive every action (a quiet green action dot on a playable
// piece/card, legal-target glows, the lifted hand card). See
// docs/decisions/web_client_ux.md for the moments. The video clips live in the
// sibling gallery-clips.spec.ts (split out because Playwright forbids a
// per-describe `test.use({ video })` — it must be top-level in a file).
//
// PURELY ADDITIVE (a parallel lane edits the web client + the other specs): this
// touches no web-client source and none of the existing specs — it imports only
// the read-only shared `helpers.ts` (+ the new gallery-helpers.ts). The committed
// canvas stills live under docs/gallery/web/ (the WEB-register gallery dir, parallel
// to docs/gallery/tui/), produced by the CPU rasterizer `gen_gallery.sh`; these specs
// capture into test-results/ for the report (see this repo's docs/gallery/README).
//
// HOW IT DRIVES THE CANVAS
//   The select / place / lift moments are driven through the virtual a11y tree's
//   buttons — the SAME engine commands the canvas affordances fire (helpers.ts):
//   picking a hand card up that way LIFTS it and lights its legal tiles ON THE
//   CANVAS, placing it draws the spirit — all real canvas pixels. The INSPECT moment
//   is now captured as the real in-canvas **floating inspect panel** (mouse hover over
//   a hand card — the pointer bridge routes hover/drag/tap correctly), anchored to the
//   card with full stats, the reach grid, and rules text.
//
// Captured at phone-portrait (primary) AND desktop — the two Playwright projects;
// filenames carry the project name so the breakpoints never collide. (The tablet
// project runs them harmlessly too; the committed set is the phone+desktop pair.)
// ─────────────────────────────────────────────────────────────────────────────

/** Settle the wgpu frame loop so a screenshot lands on a stable, fully-drawn frame
 *  (the renderer eases affordances in; a small beat lets the GL present catch up
 *  under headed parallel load). */
async function settle(page: Page, ms = 450): Promise<void> {
  await page.waitForTimeout(ms);
}

/** Capture the canvas region (the whole game surface) as a committed PNG + attach
 *  it to the report. `name` is the committed file's basename. */
async function shotCanvas(page: Page, name: string, testInfo: import("@playwright/test").TestInfo) {
  const file = testInfo.outputPath(name);
  await page.locator("#board").screenshot({ path: file });
  await testInfo.attach(name, { path: file, contentType: "image/png" });
  return file;
}

test.describe("gallery — canvas-native client (#100 A+B) — stills", () => {
  test("shell at rest — action-dot affordances visible", async ({ page }, testInfo) => {
    await startLocalGame(page);
    // At turn start the playable cards carry a quiet green action dot and the FABs
    // sit lower-right — the resting shell with every affordance shown.
    await expect(handButtons(page).first()).toBeVisible();
    await settle(page);
    await shotCanvas(page, `shell-at-rest-${testInfo.project.name}.png`, testInfo);
  });

  test("hand card lifted — legal tiles glowing", async ({ page }, testInfo) => {
    await startLocalGame(page);
    const playable = firstPlayable(page);
    test.skip(
      (await playable.count()) === 0,
      "the seed dealt no affordable play this run (2-anima opening) — lift needs one",
    );
    // Picking the card up (the a11y twin of tapping it) lifts it off the tray —
    // raised with a gild halo — and lights its legal destination tiles green on the
    // canvas. Both are real canvas pixels.
    await activate(page, playable);
    await settle(page);
    await shotCanvas(page, `hand-lifted-${testInfo.project.name}.png`, testInfo);
  });

  test("inspect — the in-canvas floating panel", async ({ page }, testInfo) => {
    await startLocalGame(page);
    await expect(handButtons(page).first()).toBeVisible();
    await settle(page);
    // Inspecting a card is the most common non-move action. MOUSE HOVER over a hand card
    // raises the in-canvas floating inspect panel (anchored to the card) — full stats
    // (the Atk/Def/HP shorthand), the reach grid, keywords, rules text. We hover over the
    // first hand card's real on-canvas rect (read from the shell regions) and capture the
    // canvas with the panel up.
    const cv = page.locator("#board");
    const box = (await cv.boundingBox())!;
    // The first card sits in the bottom tray; hover its centre. (The pointer bridge maps
    // the CSS point into the canvas backing space and routes hover → inspect.) Mouse hover
    // raises the canvas panel on a fine-pointer device; on a coarse-pointer (touch) build
    // the canvas panel is long-press-only, so we still drive the inspect DATA through the
    // a11y hand button (which calls the same inspect path) to prove + capture the moment.
    const coarse = await page.evaluate(() => matchMedia("(pointer: coarse)").matches);
    // Hover a card near the MIDDLE of the tray (not the outer 12% hover-scroll gutters), so
    // the pointer path raises the inspect panel rather than gliding the carousel.
    const hx = box.x + box.width * 0.5;
    const hy = box.y + box.height * 0.9;
    await page.mouse.move(hx - 4, hy - 4);
    await page.mouse.move(hx, hy);
    if (coarse) {
      // Touch build: drive inspect via the accessible mirror (same inspect(id) path).
      await handButtons(page).first().press("Enter").catch(() => {});
    }
    // The #detail screen-reader twin proves inspect fired (it shares the panel's data).
    await expect(page.locator("#detail .card")).toHaveCount(1, { timeout: 5_000 });
    await expect(page.locator("#detail .card .stats")).toContainText(/reach/);
    await settle(page);
    // Re-assert the hover right before the shot so the panel is up in the captured frame.
    await page.mouse.move(hx, hy);
    await page.waitForTimeout(150);
    await shotCanvas(page, `inspect-detail-${testInfo.project.name}.png`, testInfo);
  });

  test("placed spirit — a piece on the board", async ({ page }, testInfo) => {
    await startLocalGame(page);
    const playable = firstPlayable(page);
    test.skip((await playable.count()) === 0, "no affordable play this run to place a spirit");
    await activate(page, playable); // lift
    await settle(page, 200);
    const target = tileButtons(page).first();
    test.skip((await target.count()) === 0, "the lifted card lit no tile to place onto");
    await activate(page, target); // place it on the first legal tile
    // A placed piece is now mirrored as an occupied/actionable board tile button —
    // proof the placement resolved — and the canvas draws the spirit (gold-ringed,
    // its HP shown), the score ticks, and the anima is spent.
    await expect(tileButtons(page).first()).toBeVisible({ timeout: 5_000 });
    await settle(page);
    await shotCanvas(page, `placed-spirit-${testInfo.project.name}.png`, testInfo);
  });
});
