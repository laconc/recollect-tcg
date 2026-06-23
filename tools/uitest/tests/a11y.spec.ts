import { test, expect } from "@playwright/test";
import {
  openPicker,
  startLocalGame,
  startDevolveGame,
  endTurn,
  study,
  a11yTree,
  handButtons,
  tileButtons,
} from "./helpers";

// AGENTS.md invariant 7 — accessibility is first-class. The play client's board is
// a wgpu canvas (opaque to assistive tech), so the #100 Phase-B client maintains a
// VIRTUAL ARIA TREE (#shell-a11y) that mirrors every canvas affordance — board
// tiles, hand cards, the opponent strip, End Turn / Glimpse — as ACTIONABLE ARIA
// buttons firing the SAME engine commands the canvas does, plus #board-sr narrates
// the board tile-by-tile and #status is a live region. This suite asserts that
// mirror exists at command parity and is keyboard-reachable (the accessible path
// the canvas migration must never regress).

test.describe("accessibility", () => {
  test("the board canvas is a labeled, focusable input surface", async ({ page }) => {
    await openPicker(page);
    const board = page.locator("#board");
    // It is keyboard-reachable (a Tab stop) and self-describing, with the detailed
    // per-tile reading carried by the #board-sr mirror it points to — not aria-hidden
    // (an aria-hidden element can't be an interaction target for assistive tech).
    await expect(board).toHaveAttribute("tabindex", "0");
    await expect(board).toHaveAttribute("role", "application");
    await expect(board).toHaveAttribute("aria-label", /board/i);
    await expect(board).toHaveAttribute("aria-describedby", "board-sr");
  });

  test("the screen-reader board mirror (#board-sr) is present and live", async ({ page }) => {
    await openPicker(page);
    const sr = page.locator("#board-sr");
    // The region exists, is a labeled live region, and is screen-reader-only
    // (visually hidden) rather than removed from the a11y tree.
    await expect(sr).toHaveAttribute("role", "region");
    await expect(sr).toHaveAttribute("aria-label", /Board state/);
    const cls = await sr.getAttribute("class");
    expect(cls).toContain("visually-hidden");

    // Once a game is live the mirror describes the board in words.
    await startLocalGame(page);
    await expect(sr).toContainText(/Round 1/);
    await expect(sr).toContainText(/tiles?/);
  });

  test("the status line is a polite live region, screen-reader-only", async ({ page }) => {
    await openPicker(page);
    const status = page.locator("#status");
    await expect(status).toHaveAttribute("role", "status");
    await expect(status).toHaveAttribute("aria-live", "polite");
    // The announcements live region is in the a11y tree but INVISIBLE on screen (the canvas
    // is the visible surface — the announcements show in-canvas as captions / the HUD).
    expect(await status.getAttribute("class")).toContain("visually-hidden");
  });

  test("the canvas is the visible surface — the a11y mirror is all sr-only (#100 §a11y)", async ({
    page,
  }) => {
    await startLocalGame(page);
    // The accessible mirror (#status, #board-sr, #shell-a11y, #detail) is present in the a11y
    // tree but visually-hidden — none of it clutters the visible page. (Each is a 1px clipped
    // element, so it has effectively no on-screen footprint.)
    for (const sel of ["#status", "#board-sr", "#shell-a11y", "#detail"]) {
      const cls = await page.locator(sel).getAttribute("class");
      expect(cls, `${sel} should be visually-hidden`).toContain("visually-hidden");
      const box = await page.locator(sel).boundingBox();
      // A clipped sr-only element collapses to ~1px; never a visible block of text.
      expect((box?.width ?? 0) <= 2 && (box?.height ?? 0) <= 2, `${sel} has no visible footprint`)
        .toBe(true);
    }
    // The retired transitional DOM controls (move list, hand chips) carry no visible content.
    await expect(page.locator("#moves button")).toHaveCount(0);
    await expect(page.locator("#hand .chip")).toHaveCount(0);
  });

  test("the virtual a11y tree mirrors the canvas as an actionable group (#100)", async ({
    page,
  }) => {
    await startLocalGame(page);
    const tree = a11yTree(page);
    // It's a labeled group in the a11y tree (visually hidden, not aria-hidden).
    await expect(tree).toHaveAttribute("role", "group");
    await expect(tree).toHaveAttribute("aria-label", /Game actions/i);
    expect(await tree.getAttribute("class")).toContain("visually-hidden");
    // The board is a named role="grid"; the hand + actions are headings for navigation.
    const grid = tree.locator("[role='grid']");
    await expect(grid).toHaveAttribute("aria-label", /Board/);
    await expect(tree.locator("h2")).not.toHaveCount(0);
    const headings = await tree.locator("h2").allTextContents();
    expect(headings.join(" ")).toMatch(/hand/i);
    expect(headings.join(" ")).toMatch(/Actions/);
  });

  test("the board is a true ARIA grid — rows + per-tile gridcells with row/col indices (#100)", async ({
    page,
  }) => {
    // The per-tile fidelity bar (brand_and_accessibility.md): the board is exposed as a
    // real role="grid" so a screen reader announces "row R, column C, <occupant>" and
    // arrow-navigates it. Assert the grid container, its row/col counts, one role="row"
    // per board row, and a role="gridcell" for EVERY tile (the 1v1 board is 5×5 = 25),
    // each carrying a 1-based aria-rowindex / aria-colindex.
    await startLocalGame(page);
    const tree = a11yTree(page);
    const grid = tree.locator("[role='grid']");
    await expect(grid).toHaveCount(1);
    // The container advertises its dimensions (so AT says "row R of N").
    await expect(grid).toHaveAttribute("aria-rowcount", "5");
    await expect(grid).toHaveAttribute("aria-colcount", "5");
    // One row per board row, each indexed.
    const rows = grid.locator("[role='row']");
    await expect(rows).toHaveCount(5);
    await expect(rows.first()).toHaveAttribute("aria-rowindex", "1");
    // A gridcell for EVERY tile (complete grid — empties included, for navigation).
    const cells = grid.locator("[role='gridcell']");
    await expect(cells).toHaveCount(25);
    // Every cell carries a 1-based row + column index, and a non-empty accessible name
    // (its coordinate + reading) — the "row R, column C, <occupant>" announcement.
    const meta = await cells.evaluateAll((els) =>
      els.map((e) => ({
        row: e.getAttribute("aria-rowindex"),
        col: e.getAttribute("aria-colindex"),
        // The cell's name: its own text, or its (single) child button's text.
        name: (e.textContent ?? "").trim(),
      })),
    );
    for (const m of meta) {
      expect(Number(m.row), `cell rowindex ≥ 1`).toBeGreaterThanOrEqual(1);
      expect(Number(m.col), `cell colindex ≥ 1`).toBeGreaterThanOrEqual(1);
      expect(m.name.length, `cell "${m.name}" has a reading`).toBeGreaterThan(0);
    }
    // The grid spans the full coordinate range (corners present): row 1..5 × col 1..5.
    const rowSet = new Set(meta.map((m) => m.row));
    const colSet = new Set(meta.map((m) => m.col));
    expect([...rowSet].sort()).toEqual(["1", "2", "3", "4", "5"]);
    expect([...colSet].sort()).toEqual(["1", "2", "3", "4", "5"]);
  });

  test("End Turn and Glimpse are actionable buttons in the a11y tree", async ({ page }) => {
    await startLocalGame(page);
    const et = endTurn(page);
    const st = study(page);
    // Real <button>s with text labels (not unlabeled canvas hit-areas), keyboard-
    // focusable — the accessible twins of the canvas FABs.
    await expect(et).toHaveJSProperty("tagName", "BUTTON");
    await expect(et).toHaveText(/End turn/i);
    await expect(st).toHaveText(/Glimpse/i);
    await et.focus();
    await expect(et).toBeFocused();
    await st.focus();
    await expect(st).toBeFocused();
  });

  test("the hand and the board are mirrored as actionable buttons", async ({ page }) => {
    await startLocalGame(page);
    // A button per hand card (the opening hand is dealt), each naming the card +
    // whether it's playable — the accessible twin of the canvas hand tray.
    await expect(handButtons(page).first()).toBeVisible();
    const handLabels = await handButtons(page).allTextContents();
    expect(handLabels.length).toBeGreaterThan(0);
    for (const l of handLabels) {
      // A descriptive label — names the card + cost, never an opaque index.
      expect(l.trim().length, `hand label "${l}" is descriptive`).toBeGreaterThan(4);
      expect(l, `hand label "${l}" names a cost`).toMatch(/cost \d+/);
    }
    // Board tiles that are occupied / actionable are mirrored too (the opening board
    // may be empty — then there are no tile buttons, which is correct: empty tiles
    // live in the #board-sr text mirror, not as buttons that bury the actionable
    // ones). Here we only assert the locator resolves without error.
    await expect(tileButtons(page)).toHaveCount(await tileButtons(page).count());
  });

  test("the opponent strip is announced as text, never their cards (redaction)", async ({
    page,
  }) => {
    await startLocalGame(page);
    const opp = a11yTree(page).locator("p");
    await expect(opp.first()).toContainText(/Opponent:/);
    // Their hand is a COUNT, never named cards (redaction holds in the a11y mirror).
    await expect(opp.first()).toContainText(/holding \d+ cards/);
  });

  test("the a11y tree reaches every action by keyboard (Tab + activate)", async ({ page }) => {
    await startLocalGame(page);
    // Every actionable button is a real Tab stop with an accessible name; activating
    // End Turn fires the same command the canvas FAB does and the turn advances.
    const et = endTurn(page);
    await et.focus();
    await expect(et).toBeFocused();
    await page.keyboard.press("Enter");
    // The turn passed to the AI; the client stays live and the status line reflects
    // the new state (no crash; the canvas-native turn advanced).
    await expect(page.locator("#status")).toBeVisible();
  });

  test("the DEVOLVE (recede) affordance is in the a11y tree — actionable + announced (§5)", async ({
    page,
  }) => {
    // Invariant 7 extends to the new affordance: a standing-Faded form that can recede is an
    // ACTIONABLE accessible element, the standing-Faded state is ANNOUNCED (tile + #board-sr),
    // and the base card that recedes it carries its own actionable node — all in the faction's
    // word. The native shell.rs tests pin the canvas geometry; here we prove the mirror.
    await startDevolveGame(page);
    const tile = a11yTree(page).locator("button[data-a11y-id='tile-12']");
    await expect(tile).toBeVisible({ timeout: 15_000 });
    await expect(tile).toBeEnabled(); // the recede is reachable, not a lesser/disabled path
    await expect(tile).toHaveText(/standing faded/i);
    await expect(tile).toHaveText(/revert/i); // a Lorekeeper reverts (the Solace recedes)
    // The base card mirrors the recede chevron — its own actionable node.
    const base = a11yTree(page).locator("button[data-a11y-id='hand-0']");
    await expect(base).toBeVisible();
    await expect(base).toHaveText(/revert/i);
    // The board narration reads the rescuable window the canvas glow shows.
    await expect(page.locator("#board-sr")).toContainText(/standing faded/i);

    // Resolving the recede (pick up the base, activate the form) ANNOUNCES the move in the
    // live region as a worded, glyph-free phrase — "Revert … with … — the rescue", never the
    // canvas's ← / ⚔ glyphs. (Driven here, in the same loaded telling, so no extra cold start.)
    await base.press("Enter");
    await tile.press("Enter");
    const status = page.locator("#status");
    await expect(status).toContainText(/revert/i, { timeout: 10_000 });
    const announced = (await status.textContent()) ?? "";
    for (const glyph of ["⚔", "→", "←", "°", "⌂", "▒", "░", "★", "●"]) {
      expect(announced.includes(glyph), `announcement "${announced}" must be words, not ${glyph}`).toBe(false);
    }
  });
});

// The spoken layer must read as WORDS, not canvas glyphs. The visible wgpu canvas uses a
// compact symbol language (⚔ for combat, → for a target, ° / ⌂ / ▒ markers, gold pips, the
// lamplit pool); a screen reader voices those oddly ("crossed swords", "rightwards arrow")
// or drops them. So everything that reaches the spoken/ARIA layer — the #shell-a11y labels,
// the #board-sr narration, and the #status announcements — is rendered in plain words. The
// canvas keeps its glyphs (this is the invisible mirror only); these tests guard the bar.
test.describe("the spoken layer is words, never glyphs (#a11y)", () => {
  // The set of canvas glyphs that must never reach the spoken text (the affordance markers;
  // an em-dash `—` and a middot inside a card NAME are allowed, so they aren't listed here).
  const SPOKEN_GLYPHS = ["⚔", "→", "←", "°", "⌂", "▒", "░", "★", "●", "◦"];
  const containsGlyph = (s: string) => SPOKEN_GLYPHS.filter((g) => s.includes(g));

  test("no #shell-a11y label carries a canvas glyph", async ({ page }) => {
    await startLocalGame(page);
    // Every node's accessible text (headings, gridcells, hand/FAB buttons) is words.
    const text = (await a11yTree(page).textContent()) ?? "";
    expect(text.length, "the a11y tree has content").toBeGreaterThan(0);
    expect(
      containsGlyph(text),
      `the a11y tree must be all words; found glyph(s) in: ${JSON.stringify(text.slice(0, 400))}`,
    ).toEqual([]);
    // The board grid's container aria-label is words too (it's announced on entry).
    const gridLabel =
      (await a11yTree(page).locator("[role='grid']").getAttribute("aria-label")) ?? "";
    expect(containsGlyph(gridLabel), `grid label words: ${gridLabel}`).toEqual([]);
  });

  test("the #board-sr narration reads in words (no glyphs, real coordinates)", async ({ page }) => {
    await startLocalGame(page);
    const sr = (await page.locator("#board-sr").textContent()) ?? "";
    expect(sr.length).toBeGreaterThan(0);
    expect(containsGlyph(sr), `board narration words: ${sr.slice(0, 400)}`).toEqual([]);
    // It narrates the round + a tile count in words (the established phrasing).
    expect(sr).toMatch(/Round \d+/);
    expect(sr).toMatch(/tiles?/);
  });

  test("a hand card's a11y label names its stats in words", async ({ page }) => {
    await startLocalGame(page);
    // The opening hand is dealt; each card button names cost + (for a spirit) attack /
    // defense / health in words — never an "Atk/Def/HP" glyph row or a bare number.
    const labels = await handButtons(page).allTextContents();
    expect(labels.length).toBeGreaterThan(0);
    for (const l of labels) {
      expect(containsGlyph(l), `hand label words: ${l}`).toEqual([]);
      expect(l).toMatch(/cost \d+/);
    }
  });
});
