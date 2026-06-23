import { test, expect } from "@playwright/test";
import {
  openPicker,
  startLocalGame,
  endTurn,
  handButtons,
  expectNoHorizontalScroll,
  expectTouchTarget,
} from "./helpers";

// Automates docs/manual_verification.md → the responsive law at phone /
// tablet / desktop widths (the three Playwright projects). The base layout is
// fluid; these tests assert it holds: no horizontal scroll, the board fits, and
// the canvas-native shell (#100) keeps the action controls reachable.
//
// #100 Phase B: the local 1v1 telling is canvas-native — the HUD, hand, FABs +
// affordances are drawn IN the canvas, and the actionable accessible mirror is the
// off-screen a11y tree (not the old #moves button list). So the responsive checks
// assert the canvas fits + the a11y controls are reachable, not the (retired) move
// list's scroll containment.

test.describe("responsive layout", () => {
  test("the picker page has no horizontal scroll", async ({ page }) => {
    await openPicker(page);
    await expectNoHorizontalScroll(page);
  });

  test("the in-game board fits without forcing horizontal scroll", async ({ page }) => {
    await startLocalGame(page);
    await expectNoHorizontalScroll(page);
    // The board canvas never overruns the viewport width.
    const board = await page.locator("#board").boundingBox();
    const vw = page.viewportSize()!.width;
    expect(board!.x).toBeGreaterThanOrEqual(-0.5);
    expect(board!.x + board!.width).toBeLessThanOrEqual(vw + 0.5);
  });

  test("the actionable controls (End Turn, hand) are reachable on every viewport", async ({
    page,
  }) => {
    await startLocalGame(page);
    // The canvas-native shell draws End Turn / Glimpse / the hand IN the canvas; the
    // accessible mirror (the a11y tree) is the keyboard/screen-reader path. Both the
    // End Turn button and at least one hand button exist + are operable at this
    // breakpoint — the action surface never gets squeezed off-screen.
    await expect(endTurn(page)).toBeVisible();
    await expect(handButtons(page).first()).toBeVisible();
    await expectNoHorizontalScroll(page);
  });

  test("the canvas-native shell does not leave a scrolling move list", async ({ page }) => {
    await startLocalGame(page);
    // #moves is empty for the local 1v1 shell, so it can't grow the page or push
    // controls off-screen — the canvas + a11y tree carry the board.
    await expect(page.locator("#moves button")).toHaveCount(0);
    await expectNoHorizontalScroll(page);
  });

  test("the picker's DOM controls meet the ≥44px touch target on coarse pointers", async ({
    page,
  }, testInfo) => {
    // The in-game action surface is now in-canvas (the affordances are sized
    // generously there); the remaining DOM controls are the picker (style cards,
    // the difficulty select). The ≥44px rule (WCAG 2.5.5) applies on phone/tablet
    // (coarse-pointer) projects; desktop (fine pointer) keeps its denser build.
    const coarse = await page.evaluate(() => matchMedia("(pointer: coarse)").matches);
    test.skip(!coarse, "fine-pointer (desktop) build is intentionally denser");
    await openPicker(page);
    await expectTouchTarget(page.locator("#picker .style").first());
    await expectTouchTarget(page.locator("#diff"));
  });
});
