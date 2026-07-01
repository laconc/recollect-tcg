import { test, expect, Page, Locator } from "@playwright/test";
import {
  openPicker,
  startLocalGame,
  study,
  a11yTree,
  mulliganButton,
  keepHandButton,
  dismissOpeningMulligan,
} from "./helpers";

// The in-canvas GLIMPSE + MULLIGAN choice modals (the last big local-1v1 canvas
// interaction). The play client's board is a wgpu canvas (opaque to assistive tech), so
// the choice modals — like every other affordance — must be mirrored in the VIRTUAL ARIA
// TREE (#shell-a11y) as actionable buttons firing the SAME engine commands the canvas
// chips do, and each step announced in the #status live region (invariant 7). This suite
// drives both flows through the a11y tree BY KEYBOARD (focus + Enter — exactly how
// assistive tech reaches the visually-hidden mirror) and asserts the sr-only affordances +
// the redaction (the modal/announcement never names the opponent's cards).

/** Activate an a11y-tree button the way assistive tech does — focus + Enter (a clipped
 *  1px sr-only element can't take a synthetic pointer click). */
async function activate(page: Page, locator: Locator): Promise<void> {
  await locator.focus();
  await page.keyboard.press("Enter");
}

/** Open a LOCAL match and STOP at the opening Mulligan modal (don't auto-dismiss it) —
 *  so a spec can exercise the modal itself. Mirrors startLocalGame's start, minus the
 *  dismiss. */
async function startAtMulligan(page: Page): Promise<void> {
  await openPicker(page);
  await page.locator("#picker .style").first().click();
  // The Mulligan modal's button proves the opening modal is up (its a11y tree replaces the
  // live game tree). It's offered at the very start of a fresh 1v1 match.
  await expect(keepHandButton(page)).toBeVisible({ timeout: 15_000 });
}

/** Every actionable button currently in the choice a11y tree (the modal's options). */
function choiceButtons(page: Page): Locator {
  return a11yTree(page).locator("button[data-a11y-id^='choice-']");
}

test.describe("the opening Mulligan modal", () => {
  test("opens at the start and mirrors Mulligan / Keep as actionable buttons", async ({
    page,
  }) => {
    await startAtMulligan(page);
    // The choice section header names the opening decision (a labelled group, not a button).
    const headings = await a11yTree(page).locator("h2").allTextContents();
    expect(headings.join(" ")).toMatch(/mulligan/i);
    // Exactly the two options, each a real <button> with a descriptive label.
    await expect(mulliganButton(page)).toBeVisible();
    await expect(keepHandButton(page)).toBeVisible();
    await expect(mulliganButton(page)).toHaveText(/mulligan/i);
    await expect(keepHandButton(page)).toHaveText(/keep/i);
    // The status live region announces the opening decision (its cost spelled out), never
    // any card the opponent holds (redaction — the opening modal is about YOUR hand).
    const status = page.locator("#status");
    await expect(status).toContainText(/mulligan your hand/i);
    await expect(status).not.toContainText(/opponent/i);
  });

  test("Keep dismisses the modal and reaches the playable shell", async ({ page }) => {
    await startAtMulligan(page);
    await activate(page, keepHandButton(page));
    // The modal clears (the choice tree is gone) and the live game tree returns: End Turn
    // is an actionable button again.
    await expect(keepHandButton(page)).toHaveCount(0, { timeout: 10_000 });
    await expect(page.locator("#shell-a11y button[data-a11y-id='fab-end']")).toBeVisible();
    await expect(page.locator("#status")).toContainText(/keep your opening hand/i);
  });

  test("Mulligan redraws and states only the fact, never the cards (redaction)", async ({
    page,
  }) => {
    await startAtMulligan(page);
    await activate(page, mulliganButton(page));
    // The modal clears (a mulligan is once-only — the offer is spent) and play resumes.
    await expect(mulliganButton(page)).toHaveCount(0, { timeout: 10_000 });
    await expect(page.locator("#shell-a11y button[data-a11y-id='fab-end']")).toBeVisible();
    // The announcement states the FACT (a fresh hand, one to the bottom) — never which
    // cards were drawn or discarded (redaction holds in the live region).
    const status = page.locator("#status");
    await expect(status).toContainText(/mulligan/i);
    await expect(status).not.toContainText(/opponent/i);
  });
});

test.describe("the Glimpse choice modal", () => {
  test("Study opens the burn step, then the keep/bottom step, then returns to play", async ({
    page,
  }) => {
    await startLocalGame(page); // past the opening (keeps the hand)
    // Activating Study opens the Glimpse — STEP 1, the BURN cost: the choice tree replaces
    // the live tree with one "Burn <card>" chip per hand card (each framed as the cost).
    await activate(page, study(page));
    const burnChips = choiceButtons(page);
    await expect(burnChips.first()).toBeVisible({ timeout: 10_000 });
    const burnLabels = await burnChips.allTextContents();
    expect(burnLabels.length).toBeGreaterThan(0);
    for (const l of burnLabels) {
      // Each names a card to burn + the cost ("leaves play") — never an opaque index.
      expect(l, `burn chip "${l}" names the cost`).toMatch(/burn .+leaves play/i);
    }
    // The status live region narrates the burn step.
    await expect(page.locator("#status")).toContainText(/burn a card to peek/i);

    // Activate the first burn chip → STEP 2, keep/bottom: the peeked top card + two options.
    await activate(page, burnChips.first());
    const keepBottom = choiceButtons(page);
    await expect(keepBottom.first()).toBeVisible({ timeout: 10_000 });
    const kbLabels = (await keepBottom.allTextContents()).join(" ");
    expect(kbLabels).toMatch(/keep on top/i);
    expect(kbLabels).toMatch(/bottom it/i);
    // The keep step names the peeked card in the live region (it's YOUR top card — owner-only).
    await expect(page.locator("#status")).toContainText(/peek the top of the Memory/i);

    // Keep it → the modal clears and the playable shell returns (Study now spent this turn).
    const keep = a11yTree(page).locator("button[data-a11y-id='choice-choose-0']");
    await activate(page, keep);
    await expect(choiceButtons(page)).toHaveCount(0, { timeout: 10_000 });
    await expect(page.locator("#shell-a11y button[data-a11y-id='fab-end']")).toBeVisible();
  });

  test("the Glimpse modal never names the opponent (redaction)", async ({ page }) => {
    await startLocalGame(page);
    await activate(page, study(page));
    await expect(choiceButtons(page).first()).toBeVisible({ timeout: 10_000 });
    // The burn modal lists only YOUR hand (the burn cost) — the opponent's cards never
    // appear in the choice tree or the live region (invariant 2 / redaction).
    const treeText = await a11yTree(page).textContent();
    expect(treeText || "").not.toMatch(/opponent/i);
    await expect(page.locator("#status")).not.toContainText(/opponent/i);
  });

  test("while a choice modal is up, the board/hand tree is replaced by the modal", async ({
    page,
  }) => {
    await startLocalGame(page);
    await activate(page, study(page));
    await expect(choiceButtons(page).first()).toBeVisible({ timeout: 10_000 });
    // The modal is the ONLY actionable surface: the live tree's hand/board/FAB buttons are
    // gone (replaced by the choice options), so assistive tech can't fire a play that the
    // engine would reject mid-choice. (One modal, one tree — invariant 7 honesty.)
    await expect(page.locator("#shell-a11y button[data-a11y-id='fab-end']")).toHaveCount(0);
    await expect(page.locator("#shell-a11y button[data-a11y-id^='hand-']")).toHaveCount(0);
    // Every actionable button in the tree right now is a choice option.
    const allButtons = await a11yTree(page).locator("button").count();
    const choiceCount = await choiceButtons(page).count();
    expect(choiceCount).toBe(allButtons);
  });
});
