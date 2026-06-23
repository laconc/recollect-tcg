import { test, expect } from "@playwright/test";
import { startLocalGame, endTurn, a11yTree } from "./helpers";

// The INSPECT overlay's masking + a11y. When the player opens
// inspect (hover / long-press a card), the interactive affordance layer is fully
// SUPPRESSED on the canvas — the green action dots AND the FAB lane (End Turn · Glimpse)
// — while the board + hand CARDS still draw (dimmed context). The pixel-level scene
// composition is pinned by the native test
// `shell::tests::the_inspect_overlay_suppresses_the_affordances_but_keeps_the_cards`;
// this is the browser half: the INTERACTION contract (a tap on the suppressed FAB lane
// while inspecting only DISMISSES the read — it never fires) and the a11y invariant
// (inspect is a pointer affordance, so the screen-reader/keyboard tree keeps full access
// to End Turn — option C is canvas-visual, never an a11y regression).

const hook = (page: import("@playwright/test").Page) =>
  page.evaluate(() => (window as unknown as { __recollectTest: any }).__recollectTest);

test.describe("inspect overlay — masking + a11y (item 5 / option C)", () => {
  test("opening inspect populates the inspect mirror and sets the inspecting state", async ({
    page,
  }) => {
    await startLocalGame(page);
    const opened = await page.evaluate(
      () => (window as unknown as { __recollectTest: any }).__recollectTest.openInspectForTest(),
    );
    expect(opened, "inspect opened on the first hand card").toBe(true);
    // The #detail screen-reader twin of the in-canvas inspect panel is populated.
    await expect(page.locator("#detail .card")).toHaveCount(1);
    const inspecting = await page.evaluate(
      () => (window as unknown as { __recollectTest: any }).__recollectTest.inspectingForTest(),
    );
    expect(inspecting, "the canvas is in the inspect read-state").toBe(true);
  });

  test("the screen-reader path keeps End Turn while inspecting (a11y not regressed)", async ({
    page,
  }) => {
    await startLocalGame(page);
    // Inspect is a pointer (hover/long-press) affordance; the a11y tree never enters it,
    // so the FAB buttons stay reachable for keyboard / screen-reader users under inspect.
    await page.evaluate(
      () => (window as unknown as { __recollectTest: any }).__recollectTest.openInspectForTest(),
    );
    await expect(endTurn(page)).toBeVisible();
    await endTurn(page).focus();
    await expect(endTurn(page)).toBeFocused();
    // The a11y tree still carries the Actions section (no collapse under inspect).
    await expect(a11yTree(page).locator("h2")).not.toHaveCount(0);
  });

  test("a tap on the suppressed FAB lane while inspecting only DISMISSES inspect (never fires)", async ({
    page,
  }) => {
    await startLocalGame(page);
    // The board narration before any turn passes (Round 1).
    await expect(page.locator("#board-sr")).toContainText(/Round 1/);

    // Open inspect, then invoke the PRODUCTION tap handler on the End-Turn FAB region —
    // exactly what a tap where the (now-invisible) FAB sits would do.
    const dismissedWithoutFiring = await page.evaluate(() => {
      const t = (window as unknown as { __recollectTest: any }).__recollectTest;
      t.openInspectForTest();
      if (!t.inspectingForTest()) return { ok: false, why: "inspect did not open" };
      t.tapRegionForTest({ kind: "fab", which: "end" });
      // After the tap: inspect must be cleared (the read dismissed) …
      return { ok: true, stillInspecting: t.inspectingForTest() };
    });
    expect(dismissedWithoutFiring.ok, dismissedWithoutFiring.why ?? "").toBe(true);
    expect(dismissedWithoutFiring.stillInspecting, "the tap dismissed the inspect read").toBe(false);

    // … and the turn did NOT end — the FAB was inert under inspect. Still Round 1, still
    // the player's move (End Turn still offered). A real End Turn would advance the round /
    // hand the turn to the AI; neither happened.
    await expect(page.locator("#board-sr")).toContainText(/Round 1/);
    await expect(endTurn(page)).toBeVisible();
  });
});
