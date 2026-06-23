import { test, expect } from "@playwright/test";
import { startLocalGame, handButtons, tileButtons } from "./helpers";
import { activate, firstPlayable } from "./gallery-helpers";

// ─────────────────────────────────────────────────────────────────────────────
// VISUAL GALLERY (clips) — short per-test videos of the #100 A+B canvas-native
// client. Companion to gallery.spec.ts (stills); see that file's header +
// docs/decisions/web_client_ux.md for the moments and how the canvas is driven
// (and the current-state caveat: the canvas pointer bridge mis-routes its hit-test
// regions, so play is driven through the a11y tree — which lifts the card, lights
// the legal tiles, and places the spirit ON THE CANVAS all the same).
//
// `test.use({ video: 'on' })` is TOP-LEVEL in this file (Playwright forbids it in
// a describe group — it forces a new worker), so EVERY test here records its own
// video.webm under test-results/. It is NOT a global config change — the stills
// file and the existing suite keep the config default. Keeping the clips in their
// own file is what lets video be per-test without touching playwright.config.ts.
//
// Each clip is kept to a few seconds so the .webm stays small — large binaries
// don't belong in git (see docs/gallery/README for the size policy; the committed
// copies are renamed out of test-results/ into docs/gallery/web/, the WEB-register
// gallery dir alongside the canvas + site stills).
//
// PURELY ADDITIVE: imports only the read-only shared helpers + gallery-helpers.ts;
// touches no web-client source and none of the other specs.
// ─────────────────────────────────────────────────────────────────────────────

test.use({ video: "on" });

test("clip: select then target — lift a card, place it on a glowing tile", async ({ page }) => {
  await startLocalGame(page);
  const playable = firstPlayable(page);
  test.skip((await playable.count()) === 0, "no affordable play this run for the select-target clip");
  // The select-then-target play gesture (one of the two equally-valid gestures the
  // #100 design promises). Driven through the a11y tree so it's deterministic, with
  // brief holds so the lift → legal-tile glow → placement beats are legible: the
  // card lifts with its gild halo, the legal tiles light green, then a spirit lands
  // on the board — all real canvas pixels.
  await activate(page, playable);
  await page.waitForTimeout(900); // hold on the lifted card + the lit tiles
  const target = tileButtons(page).first();
  if (await target.count()) {
    await activate(page, target);
    await page.waitForTimeout(900); // the placed piece settles; score + anima update
  }
  await expect(page.locator("#status")).toBeVisible();
});

test("clip: inspect a card — the detail readout opens and updates", async ({ page }) => {
  await startLocalGame(page);
  await expect(handButtons(page).first()).toBeVisible();
  // The high-frequency inspect moment. The in-canvas floating panel rides the
  // (currently mis-routed) pointer path, so we record its WORKING twin — the
  // `#detail` readout the a11y activation populates — opening for one card and
  // re-populating for the next (name · kind · cost · the A/D/HP stat line · the
  // reach grid), captured as motion across the full page (canvas above, detail
  // below).
  // Cycle DISTINCT hand cards: activating a new card index re-inspects it (and
  // re-lifts), re-populating #detail — so the readout visibly changes card to card.
  // (We move across indices, never re-tapping the same lifted card, so no toggle.)
  const n = Math.min(3, await handButtons(page).count());
  for (let i = 0; i < n; i++) {
    await activate(page, handButtons(page).nth(i)); // pick up + inspect card i
    await expect(page.locator("#detail .card")).toBeVisible({ timeout: 5_000 });
    await page.waitForTimeout(700); // hold so the readout is legible before the next
  }
  await expect(page.locator("#detail .card")).toBeVisible();
});
