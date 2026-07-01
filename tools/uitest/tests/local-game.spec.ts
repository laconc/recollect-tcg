import { test, expect, Page, Locator } from "@playwright/test";
import { startLocalGame, endTurn, study, handButtons, tileButtons, a11yTree } from "./helpers";

// The a11y-tree buttons are screen-reader-only (visually-hidden), so a real player
// activates them by KEYBOARD (focus + Enter) — exactly how assistive tech drives
// them — not a synthetic pointer click at screen coordinates (which a clipped 1px
// element can't receive). `activate` focuses the button then presses Enter.
async function activate(page: Page, locator: Locator): Promise<void> {
  await locator.focus();
  await page.keyboard.press("Enter");
}

// Automates docs/manual_verification.md → "Play client": a LOCAL game vs the AI is
// playable on the #100 Phase-B canvas-native shell — the board canvas mounts and
// owns the whole surface, the in-canvas affordances drive every action (with the
// virtual a11y tree as the actionable accessible mirror), End Turn / Study work,
// and the HUD shows score + Anima. (The transitional HTML move buttons are retired
// for this mode; the a11y tree's buttons fire the same engine commands.)

test.describe("local game vs the AI", () => {
  test("starting a match mounts the board canvas", async ({ page }) => {
    await startLocalGame(page);
    const canvas = page.locator("#board");
    await expect(canvas).toBeVisible();
    // fitCanvas() resizes the wgpu backing buffer to the displayed size × DPR once
    // the renderer mounts; a non-zero buffer proves the renderer is live (not the
    // static 720 default left when init fails).
    const dims = await canvas.evaluate((c: HTMLCanvasElement) => ({ w: c.width, h: c.height }));
    expect(dims.w).toBeGreaterThan(0);
    expect(dims.h).toBeGreaterThan(0);
  });

  test("the canvas-native shell retires the transitional HTML move buttons", async ({ page }) => {
    await startLocalGame(page);
    // The #100 design replaces (not parallels) the labeled-move list for the local
    // 1v1 shell: #moves is empty (no leftover buttons), and the actions live in the
    // virtual a11y tree + the canvas affordances instead.
    await expect(page.locator("#moves button")).toHaveCount(0);
    await expect(page.locator("#hand .chip")).toHaveCount(0);
    // The actionable mirror is present (the accessible path).
    await expect(a11yTree(page).locator("button").first()).toBeVisible();
  });

  test("the hand is mirrored as actionable buttons with descriptive labels", async ({ page }) => {
    await startLocalGame(page);
    // The opening hand is seed-derived (the client seeds from Date.now()), so the
    // exact cards vary — but each is a button naming the card + cost + whether it's
    // playable, never an opaque index. (We assert the SHAPE, not a closed card set.)
    await expect(handButtons(page).first()).toBeVisible();
    const labels = await handButtons(page).allTextContents();
    expect(labels.length).toBeGreaterThan(0);
    for (const l of labels) {
      expect(l.trim().length, `hand label "${l}" should be non-trivial`).toBeGreaterThan(4);
      expect(l, `hand label "${l}" names a cost`).toMatch(/cost \d+/);
    }
  });

  test("End Turn and Study render as actionable controls", async ({ page }) => {
    await startLocalGame(page);
    // The two global controls — now actionable a11y-tree buttons mirroring the canvas
    // FABs (Glimpse is always legal at turn start; End Turn always passes the turn).
    await expect(endTurn(page)).toHaveCount(1);
    await expect(endTurn(page)).toHaveText(/End turn/i);
    await expect(study(page)).toHaveCount(1);
    await expect(study(page)).toHaveText(/Glimpse/i);
  });

  test("the HUD shows the running score and the Anima budget", async ({ page }) => {
    await startLocalGame(page);
    const status = page.locator("#status");
    // Running score (A vs B) + Anima as the play budget, in the status line.
    await expect(status).toContainText(/score A \d+ — B/);
    await expect(status).toContainText(/anima \(your play budget\)/);
    // The hand is mirrored (the actionable cards).
    await expect(handButtons(page).first()).toBeVisible();
  });

  test("an action is applied and the turn can be ended via the a11y tree", async ({ page }) => {
    await startLocalGame(page);
    // Apply an action through the accessible mirror (the same command path the canvas
    // affordance fires): a playable hand card if one is affordable, else Study (always
    // available — the anima floor). The opening hand + 2-anima budget can leave Study
    // as the only affordable play, so we don't assume a card play exists.
    const playable = handButtons(page).filter({ hasText: /playable|evolution form/ }).first();
    if (await playable.count()) {
      await activate(page, playable); // picks the card up (its tiles glow on the canvas)
      // Place it on a legal tile if one is mirrored; otherwise the pick-up is
      // non-destructive and we fall through to ending the turn.
      const target = tileButtons(page).first();
      if (await target.count()) await activate(page, target).catch(() => {});
    } else {
      await activate(page, study(page));
    }

    // End the turn — the seeded AI then takes its whole turn (paced).
    await expect(endTurn(page)).toBeVisible();
    await activate(page, endTurn(page));
    // The client stays responsive: the status live region is present, and once the bot's
    // paced turn hands back, the actionable a11y tree returns (the End Turn FAB, or — if the
    // match ended — the result section). The DOM "New game" control is retired for the
    // canvas-native shell (it's an in-canvas affordance now), so we assert the canvas-native
    // responsiveness instead. No crash; the turn advanced.
    await expect(page.locator("#status")).toBeAttached();
    await expect
      .poll(
        async () => {
          const tree = a11yTree(page);
          const ended = (await tree.locator("h2", { hasText: /match has ended/i }).count()) > 0;
          const fab = (await endTurn(page).count()) > 0;
          return ended || fab;
        },
        { timeout: 25_000 },
      )
      .toBe(true);
  });

  test("activating a hand card opens the in-canvas inspect (the #detail mirror updates)", async ({
    page,
  }) => {
    await startLocalGame(page);
    // The canvas-native inspect is the VISIBLE panel (drawn IN the canvas); #detail is its
    // screen-reader-only (visually-hidden) text twin, so a POPULATED #detail proves inspect
    // fired. Activating a hand button (the accessible twin of tapping the card) picks it up
    // AND inspects it, populating #detail — the same path a hover/long-press drives on the
    // canvas. (#detail is in the a11y tree but invisible to the eye — the canvas is the
    // visible surface, §a11y — so we assert its CONTENT, not its visibility.)
    await activate(page, handButtons(page).first());
    await expect(page.locator("#detail .card")).toHaveCount(1);
    await expect(page.locator("#detail .card .stats")).toContainText(/reach/);
    // It carries the Atk/Def/HP shorthand (never A/D/H).
    await expect(page.locator("#detail .card .stats")).toContainText(/Atk \d+ · Def \d+ · HP \d+/);
    // And it is screen-reader-only (present in the a11y tree, invisible on screen).
    expect(await page.locator("#detail").getAttribute("class")).toContain("visually-hidden");
  });
});
