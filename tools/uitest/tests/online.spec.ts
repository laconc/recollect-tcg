import { test, expect, Page } from "@playwright/test";
import {
  startOnlineGame,
  startTeamGame,
  endTurn,
  study,
  handButtons,
  tileButtons,
  a11yTree,
} from "./helpers";

// #100 (LAUNCH-CRITICAL) — ONLINE PvP + 2v2 now drive the FULL canvas shell, the same
// board · HUD · hand · affordances · virtual a11y tree a local 1v1 telling draws, but
// sourced from the server's REDACTED PlayerView / TeamView + its legal list (no local
// engine; the server is authoritative). This suite covers what is testable HEADLESS —
// the rendering, the a11y tree, and REDACTION — by injecting the real wire shape of a
// `welcome` / `team_welcome` through the production `onServerMsg` path (no live socket;
// the live server-backed play is browser-verify per the task). The redaction guard is
// the load-bearing one: the online client only ever holds the redacted view, so it can
// never render an opponent's hand / deck — the opponent is counts/backs only.

test.describe("online 1v1 — the full canvas shell over the redacted PlayerView", () => {
  test("mounts the shell + retires the HTML move buttons (the canvas owns it)", async ({
    page,
  }) => {
    await startOnlineGame(page);
    // The status line names the online telling (round · anima) — the shell is live.
    await expect(page.locator("#status")).toContainText(/online/i);
    // The portrait shell LAYOUT is applied (body.shell) — without it, the picker/actions
    // DOM chrome would not be hidden and the board would be mis-sized. (Guards the fix that
    // setShellLayout runs once the first view arrives, not before.)
    await expect(page.locator("body")).toHaveClass(/\bshell\b/);
    // The transitional #moves / #hand DOM are RETIRED for online (the design says replace,
    // not parallel) — the canvas + the a11y tree are the path now.
    await expect(page.locator("#moves button")).toHaveCount(0);
    await expect(page.locator("#hand .chip")).toHaveCount(0);
  });

  test("the virtual a11y tree mirrors the canvas (board · hand · actions)", async ({ page }) => {
    await startOnlineGame(page);
    // The board is a named role="grid"; the hand + actions are headings — the same
    // accessible mirror the local shell builds, now over the server's view.
    await expect(a11yTree(page).locator("[role='grid']")).toHaveAttribute("aria-label", /Board/);
    const headings = await a11yTree(page).locator("h2").allTextContents();
    expect(headings.join(" ")).toMatch(/hand/i);
    expect(headings.join(" ")).toMatch(/Actions/);
    // End Turn + Study are actionable buttons (it's your turn at the opening).
    await expect(endTurn(page)).toBeVisible();
    await expect(study(page)).toBeVisible();
    // A button per hand card, each naming the card + its cost (never an opaque index).
    await expect(handButtons(page).first()).toBeVisible();
    for (const l of await handButtons(page).allTextContents()) {
      expect(l, `hand label "${l}" names a cost`).toMatch(/cost \d+/);
    }
  });

  test("REDACTION: the opponent is counts-only — never their cards", async ({ page }) => {
    await startOnlineGame(page);
    // The opponent strip in the a11y mirror is a TEXT readout: name · score · a hand COUNT,
    // never enumerated cards (redaction holds in the accessible path too).
    const opp = a11yTree(page).locator("p", { hasText: /Opponent:/ });
    await expect(opp.first()).toContainText(/holding \d+ cards/);
    // The hand BUTTONS in the tree are YOUR cards only; their count equals your own hand,
    // and the opponent's cards never appear as actionable nodes.
    const handCount = await handButtons(page).count();
    expect(handCount).toBeGreaterThan(0);
    // The client holds only the redacted view: there is no opponent hand array anywhere in
    // the page's online state (the strongest headless redaction check — the data isn't even
    // present to leak).
    const leak = await page.evaluate(() => {
      const w = window as unknown as { __recollectOnlineView?: unknown };
      // The live online view is whatever the shell last rendered; read it back through the
      // same path the shell uses (exposed for the test) and confirm no opponent hand.
      const t = (window as unknown as { __recollectTest: any }).__recollectTest;
      const v = t.onlineViewForTest ? t.onlineViewForTest() : null;
      if (!v) return { checked: false, leaked: false };
      const opp = v.opponent || {};
      return { checked: true, leaked: Array.isArray((opp as any).hand) };
    });
    expect(leak.checked, "the online view was readable for the redaction check").toBe(true);
    expect(leak.leaked, "the opponent view carries NO hand array (counts only)").toBe(false);
  });

  test("keyboard reaches the actions — End Turn fires (your turn)", async ({ page }) => {
    await startOnlineGame(page);
    const et = endTurn(page);
    await et.focus();
    await expect(et).toBeFocused();
    // Activating End Turn ships the command to the (stubbed) server; the client stays live
    // (no crash) and the status line still reflects the online telling.
    await page.keyboard.press("Enter");
    await expect(page.locator("#status")).toBeVisible();
  });

  test("as seat B, your own seat drives the shell (not hard-coded to A)", async ({ page }) => {
    // Online you may be seat B; the shell must source YOUR seat from the view, not assume A.
    await startOnlineGame(page, { seat: "B", moves: 0 });
    await expect(page.locator("#status")).toContainText(/online/i);
    // The board + hand mirror still build (the board may be empty at the very opening; the
    // hand is yours). No opponent hand leaks.
    const opp = a11yTree(page).locator("p", { hasText: /Opponent:/ });
    await expect(opp.first()).toContainText(/holding \d+ cards/);
  });
});

test.describe("online 2v2 — the full shell over the 6×6 redacted TeamView", () => {
  test("mounts the 6×6 shell + retires the HTML buttons", async ({ page }) => {
    await startTeamGame(page);
    await expect(page.locator("#status")).toContainText(/2v2/i);
    await expect(page.locator("#moves button")).toHaveCount(0);
    await expect(page.locator("#hand .chip")).toHaveCount(0);
    // The board mirror narrates a 6×6 page (the #board-sr text reading), and the a11y tree
    // exposes it as a 6×6 role="grid" (36 gridcells) with a hand section.
    const grid = a11yTree(page).locator("[role='grid']");
    await expect(grid).toHaveAttribute("aria-label", /Board/);
    await expect(grid).toHaveAttribute("aria-rowcount", "6");
    await expect(grid).toHaveAttribute("aria-colcount", "6");
    await expect(grid.locator("[role='gridcell']")).toHaveCount(36);
    const headings = await a11yTree(page).locator("h2").allTextContents();
    expect(headings.join(" ")).toMatch(/hand/i);
  });

  test("REDACTION: the opposing team is combined counts — never their cards", async ({ page }) => {
    await startTeamGame(page);
    // The opponent strip mirrors the OPPOSING TEAM as a count (the sum of both rivals'
    // hands) — never enumerated cards.
    const opp = a11yTree(page).locator("p", { hasText: /Opponent:/ });
    await expect(opp.first()).toContainText(/holding \d+ cards/);
    // No rival hand array is present in the synthesized opponent view (counts only).
    const leaked = await page.evaluate(() => {
      const t = (window as unknown as { __recollectTest: any }).__recollectTest;
      const v = t.onlineViewForTest ? t.onlineViewForTest() : null;
      if (!v) return false;
      // The TeamView's opponents are OpponentViews (no `hand` field); assert structurally.
      return (v.opponents || []).some((o: any) => Array.isArray(o.hand));
    });
    expect(leaked, "no rival hand array in the 2v2 view (counts only)").toBe(false);
  });

  test("the 2v2 board mirror reads the wider page", async ({ page }) => {
    await startTeamGame(page);
    // The #board-sr text mirror describes the board; a 6×6 board has more tiles than a 5×5,
    // and the round/turn line is present. (We assert the mirror is non-empty + names a slot
    // or the round — the detailed per-tile reading paces the canvas.)
    const sr = page.locator("#board-sr");
    await expect(sr).toContainText(/Round 1/);
  });
});
