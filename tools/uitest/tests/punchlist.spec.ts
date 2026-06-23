import { test, expect, Page, Locator } from "@playwright/test";
import {
  openPicker,
  startLocalGame,
  a11yTree,
  endTurn,
  study,
  keepHandButton,
} from "./helpers";

// The canvas-UI polish punch-list. The play client's board is a wgpu canvas
// (opaque to assistive tech + pixel-unstable across machines), so the canvas pixels
// are verified by the NATIVE shell.rs tests (the pure ShellScene geometry). What this
// browser suite guards is the two items whose CORRECTNESS is observable through the
// accessible mirror (#shell-a11y / #detail) — the only honest seam into the canvas:
//
//   • Item 5 — a blocking modal (Glimpse / Mulligan / result) MASKS the board
//     affordances: while it's up, the actionable game tree is REPLACED by the modal's
//     own option tree, so no board/hand/FAB affordance is reachable beneath it (the
//     visual masking the native test `the_choice_modal_masks_the_board_affordances_*`
//     pins; here we prove the accessible affordances vanish too — one source).
//   • Item 12 — a tile holding BOTH a spirit and a Landmark exposes the landmark as a
//     SECOND, independently-activatable inspect node, and activating it inspects the
//     landmark (the #detail readout names it) — so the landmark a spirit covers is
//     never unreachable.

/** Activate an a11y-tree button the way assistive tech does — focus + Enter (a clipped
 *  1px sr-only element can't take a synthetic pointer click). */
async function activate(page: Page, locator: Locator): Promise<void> {
  await locator.focus();
  await page.keyboard.press("Enter");
}

test.describe("item 5 — a blocking modal masks the board affordances", () => {
  test("the opening Mulligan replaces the game affordances with its own options", async ({
    page,
  }) => {
    await openPicker(page);
    await page.locator("#picker .style").first().click();
    // The opening Mulligan modal is up: its Keep button proves the choice tree replaced the
    // live game tree (the modal is the only live target — the masking the maintainer wanted).
    await expect(keepHandButton(page)).toBeVisible({ timeout: 15_000 });
    // While the modal is up, NONE of the live game affordances are present in the a11y tree —
    // no End Turn / Glimpse FAB, no board-tile or hand-card buttons. They are MASKED.
    await expect(endTurn(page)).toHaveCount(0);
    await expect(study(page)).toHaveCount(0);
    await expect(a11yTree(page).locator("button[data-a11y-id^='tile-']")).toHaveCount(0);
    await expect(a11yTree(page).locator("button[data-a11y-id^='hand-']")).toHaveCount(0);
    // Only the modal's option buttons are reachable (Mulligan / Keep).
    const opts = a11yTree(page).locator("button[data-a11y-id^='choice-']");
    await expect(opts).toHaveCount(2);
  });

  test("the live affordances return once the modal is dismissed", async ({ page }) => {
    // startLocalGame keeps the opening hand (dismisses the modal) and waits for the playable
    // shell — so reaching it proves the masking lifts and the game tree comes back.
    await startLocalGame(page);
    await expect(endTurn(page)).toBeVisible();
    // The choice tree is gone; the live game affordances are back.
    await expect(a11yTree(page).locator("button[data-a11y-id^='choice-']")).toHaveCount(0);
  });
});

test.describe("item 12 — a co-occupied tile's landmark is independently inspectable", () => {
  /** Mount an online telling whose board has a spirit standing ON a face-up Landmark
   *  (tile 12), via the crafted welcome injected through the production onServerMsg path. */
  async function startWithLandmark(page: Page): Promise<void> {
    await openPicker(page);
    await page.evaluate(() => {
      const t = (window as unknown as { __recollectTest: any }).__recollectTest;
      const msg = JSON.parse(t.sampleLandmarkWelcome(7));
      t.injectServerMsg(msg);
    });
    await expect(page.locator("#status")).toContainText(/online/i, { timeout: 15_000 });
  }

  test("the landmark gets its own inspect node in the a11y tree", async ({ page }) => {
    await startWithLandmark(page);
    // The spirit's tile button exists (the top occupant — select / inspect).
    await expect(
      a11yTree(page).locator("button[data-a11y-id='tile-12']"),
    ).toBeVisible({ timeout: 10_000 });
    // The LANDMARK has its OWN actionable inspect node — the second target (item 12).
    const terrain = a11yTree(page).locator("button[data-a11y-id='tile-12-terrain']");
    await expect(terrain).toBeVisible();
    await expect(terrain).toHaveText(/landmark/i);
    await expect(terrain).toHaveText(/inspect/i);
  });

  test("activating the landmark node inspects the landmark (the #detail readout names it)", async ({
    page,
  }) => {
    await startWithLandmark(page);
    const terrain = a11yTree(page).locator("button[data-a11y-id='tile-12-terrain']");
    await expect(terrain).toBeVisible({ timeout: 10_000 });
    await activate(page, terrain);
    // The inspect readout (#detail, the sr-only twin of the in-canvas inspect panel) now
    // describes the LANDMARK's card (Cloudling), reachable even though a spirit stands on it.
    const detail = page.locator("#detail");
    await expect(detail).toContainText(/Cloudling/i, { timeout: 10_000 });
    // The status line confirms the landmark is what was inspected.
    await expect(page.locator("#status")).toContainText(/landmark/i);
  });
});
