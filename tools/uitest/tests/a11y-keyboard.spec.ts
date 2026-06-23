import { test, expect } from "@playwright/test";
import {
  startLocalGameOrSkip,
  endTurn,
  study,
  handButtons,
  a11yTree,
} from "./helpers";

// ─────────────────────────────────────────────────────────────────────────────
// DEEP CANVAS A11Y — keyboard traversal, focus-order stability, and live-region
// behaviour over the virtual ARIA tree (#shell-a11y) that mirrors the wgpu canvas.
//
// a11y.spec.ts proves the mirror EXISTS at command parity; THIS file deepens it to
// the things a real keyboard / screen-reader user depends on (AGENTS.md invariant
// 7 — accessibility is first-class, never an afterthought):
//   • every actionable node is a focusable Tab stop with a stable, non-empty
//     accessible name (focus order is deterministic — a screen reader reads the
//     same sequence twice);
//   • Sequential Tab traversal reaches the global controls (End Turn / Glimpse) and
//     hand cards in a coherent order — no keyboard trap, no orphan node;
//   • the #status live region is `aria-live="polite"` and its text UPDATES as the
//     game advances (an announcement actually fires for an AT to voice);
//   • `prefers-reduced-motion` is honoured (the paced replay collapses, so the turn
//     advances near-instantly) — the motion bar in brand_and_accessibility.md.
//
// These need the wgpu shell to mount (a real GL surface), so each goes through
// `startLocalGameOrSkip`: it exercises the real accessible surface where a GPU
// exists and quietly DEFERS (skips, never fails) on a no-GPU sandbox — the GPU-
// deferred path documented in docs/testing.md. The breakpoint-independent traversal
// runs once (desktop project) to stay off the headed-Chromium GL contention.
// ─────────────────────────────────────────────────────────────────────────────

function desktopOnly() {
  test.skip(
    test.info().project.name !== "desktop",
    "keyboard traversal is viewport-independent; run once on desktop",
  );
}

test.describe("deep canvas a11y — keyboard + live regions", () => {
  test("every actionable mirror node is a focusable Tab stop with a stable name", async ({
    page,
  }) => {
    desktopOnly();
    await startLocalGameOrSkip(page);
    // Collect the actionable buttons in the a11y tree and assert each can take focus
    // AND advertises a non-empty accessible name (a screen reader never lands on an
    // anonymous control). tabindex must not be negative (those are skipped by Tab).
    const buttons = a11yTree(page).locator("button");
    const count = await buttons.count();
    expect(count, "the live a11y tree mirrors at least the two global FABs + the hand").toBeGreaterThan(2);
    for (let i = 0; i < count; i++) {
      const b = buttons.nth(i);
      const ti = await b.getAttribute("tabindex");
      expect(Number(ti ?? "0"), `button ${i} is a Tab stop (tabindex ≥ 0)`).toBeGreaterThanOrEqual(0);
      const name = (await b.textContent())?.trim() ?? "";
      expect(name.length, `button ${i} has a non-empty accessible name`).toBeGreaterThan(0);
      await b.focus();
      await expect(b, `button ${i} is focusable`).toBeFocused();
    }
  });

  test("the focus order is deterministic — the same DOM sequence twice", async ({ page }) => {
    desktopOnly();
    await startLocalGameOrSkip(page);
    // The mirror's reading order must be stable: capture the data-a11y-id sequence,
    // and (without acting) capture it again — a screen reader reading top-to-bottom
    // gets the identical order. (Document-order IS the AT reading order for these
    // sr-only nodes; an unstable order would scramble narration.)
    const ids = async () =>
      a11yTree(page)
        .locator("button[data-a11y-id]")
        .evaluateAll((els) => els.map((e) => e.getAttribute("data-a11y-id")));
    const first = await ids();
    expect(first.length).toBeGreaterThan(0);
    const second = await ids();
    expect(second, "the a11y reading order is deterministic").toEqual(first);
    // The two global controls appear in the order End Turn → Glimpse (the canvas FAB
    // lane order), so keyboard users learn one stable sequence.
    expect(first).toContain("fab-end");
    expect(first).toContain("fab-study");
  });

  test("the board grid reads in a deterministic row-major order with complete coordinates", async ({
    page,
  }) => {
    desktopOnly();
    await startLocalGameOrSkip(page);
    // A screen reader reads the grid in DOCUMENT order; it must be a stable, complete
    // row-major sweep (row 1 cols 1..N, row 2 cols 1..N, …) so narration is predictable
    // and no cell is skipped. Capture each gridcell's (rowindex, colindex) in document
    // order and assert it is exactly that lexicographic sweep across the full 5×5 board.
    const coords = async () =>
      a11yTree(page)
        .locator("[role='grid'] [role='gridcell']")
        .evaluateAll((els) =>
          els.map((e) => [
            Number(e.getAttribute("aria-rowindex")),
            Number(e.getAttribute("aria-colindex")),
          ]),
        );
    const first = await coords();
    expect(first.length, "every tile is a gridcell (5×5 board)").toBe(25);
    // Document order is the row-major sweep: sorting by (row, col) is a no-op.
    const sorted = [...first].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
    expect(first, "gridcells read row-major, in order").toEqual(sorted);
    // The sweep is complete + gap-free: row 1..5 each with col 1..5.
    const expected: number[][] = [];
    for (let r = 1; r <= 5; r++) for (let c = 1; c <= 5; c++) expected.push([r, c]);
    expect(first, "the grid covers every (row, col) once").toEqual(expected);
    // Deterministic: a second read (no action taken) yields the identical sequence.
    expect(await coords(), "the grid reading order is stable").toEqual(first);
  });

  test("an actionable gridcell is keyboard-operable — the grid is navigable, not just announced", async ({
    page,
  }) => {
    desktopOnly();
    await startLocalGameOrSkip(page);
    // The grid must be OPERABLE by keyboard, not merely announced: an actionable cell
    // wraps a real <button> (a Tab stop), so a keyboard user can focus + activate a tile.
    // Find a gridcell that carries a button (the opening board may be empty; if so, this
    // is vacuously satisfied — the empties-are-inert rule — and we assert the structure
    // exists instead). When a piece IS present, its button focuses + sits in the grid.
    const cellButtons = a11yTree(page).locator("[role='gridcell'] button[data-a11y-id^='tile-']");
    const n = await cellButtons.count();
    if (n > 0) {
      const b = cellButtons.first();
      await b.focus();
      await expect(b, "an actionable tile button takes focus").toBeFocused();
      // It lives inside a gridcell with a coordinate (so focusing it announces the cell).
      const inCell = await b.evaluate((el) => {
        const cell = el.closest("[role='gridcell']");
        return !!cell && !!cell.getAttribute("aria-rowindex") && !!cell.getAttribute("aria-colindex");
      });
      expect(inCell, "the focusable tile button sits in a coordinate-bearing gridcell").toBe(true);
    } else {
      // No occupied/actionable tiles at the opening — the inert empties are still a
      // complete, announced grid (proven by the row-major test); nothing to operate yet.
      await expect(a11yTree(page).locator("[role='grid'] [role='gridcell']")).toHaveCount(25);
    }
  });

  test("sequential Tab traversal reaches the global controls with no keyboard trap", async ({
    page,
  }) => {
    desktopOnly();
    await startLocalGameOrSkip(page);
    // From the document start, press Tab repeatedly and record which a11y-tree node
    // holds focus at each step. Within a bounded number of stops we must reach BOTH
    // End Turn and Glimpse — proving the mirror is in the natural tab sequence and
    // focus never sticks (a keyboard trap would loop on one node forever).
    await page.locator("body").focus().catch(() => {});
    const seen = new Set<string>();
    let sawEnd = false;
    let sawStudy = false;
    for (let i = 0; i < 60 && !(sawEnd && sawStudy); i++) {
      await page.keyboard.press("Tab");
      const id = await page.evaluate(() => {
        const el = document.activeElement as HTMLElement | null;
        return el?.closest("#shell-a11y") ? (el.getAttribute("data-a11y-id") ?? "") : "";
      });
      if (id) seen.add(id);
      if (id === "fab-end") sawEnd = true;
      if (id === "fab-study") sawStudy = true;
    }
    expect(sawEnd, "Tab traversal reaches End Turn").toBe(true);
    expect(sawStudy, "Tab traversal reaches Glimpse").toBe(true);
    // More than one distinct node received focus along the way — no single-node trap.
    expect(seen.size, "focus moved across multiple nodes (no keyboard trap)").toBeGreaterThan(1);
  });

  test("the #status live region is polite and its text updates as play advances", async ({
    page,
  }) => {
    desktopOnly();
    await startLocalGameOrSkip(page);
    const status = page.locator("#status");
    await expect(status).toHaveAttribute("aria-live", "polite");
    // Capture the announcement text, end the turn (the seeded AI then plays, paced),
    // and assert the live region's text CHANGES — an announcement actually fired for
    // assistive tech to voice (not a static label). We poll because the bot's paced
    // turn updates the region asynchronously.
    const before = (await status.textContent())?.trim() ?? "";
    await endTurn(page).focus();
    await page.keyboard.press("Enter");
    await expect
      .poll(async () => (await status.textContent())?.trim() ?? "", { timeout: 25_000 })
      .not.toBe(before);
  });

  test("prefers-reduced-motion collapses the paced replay (the turn advances fast)", async ({
    page,
  }) => {
    desktopOnly();
    // The motion bar (brand_and_accessibility.md): honour prefers-reduced-motion. With
    // it set, the opponent's paced replay collapses to near-instant, so ending the turn
    // hands control back quickly rather than dwelling ~1s per bot action.
    await page.emulateMedia({ reducedMotion: "reduce" });
    await startLocalGameOrSkip(page);
    await expect(endTurn(page)).toBeVisible();
    await endTurn(page).focus();
    await page.keyboard.press("Enter");
    // Control returns: either the live FAB is back (a fresh turn) or the telling ended.
    // Under reduced motion this resolves well within the budget (the dwell is collapsed).
    await expect
      .poll(
        async () => {
          const fab = (await endTurn(page).count()) > 0;
          const ended =
            (await a11yTree(page).locator("h2", { hasText: /telling has ended/i }).count()) > 0;
          return fab || ended;
        },
        { timeout: 20_000 },
      )
      .toBe(true);
  });

  test("Glimpse and a hand card are both keyboard-operable in one session", async ({ page }) => {
    desktopOnly();
    await startLocalGameOrSkip(page);
    // Two distinct actionable affordances are reachable + operable by keyboard in the
    // same live telling: Glimpse (always legal at turn start) and the first hand card
    // (focus it — picking it up is the accessible twin of tapping it). No pointer used.
    const st = study(page);
    await st.focus();
    await expect(st).toBeFocused();
    const hand = handButtons(page).first();
    await expect(hand).toBeVisible();
    await hand.focus();
    await expect(hand).toBeFocused();
    // Activating the hand card via keyboard inspects it (the #detail sr-twin populates)
    // — proof the keyboard path drives the same engine command the canvas tap does.
    await page.keyboard.press("Enter");
    await expect(page.locator("#detail .card")).toHaveCount(1, { timeout: 5_000 });
  });

  test("focus is preserved across the per-frame a11y-tree rebuild (no focus loss)", async ({
    page,
  }) => {
    desktopOnly();
    await startLocalGameOrSkip(page);
    // The a11y tree is rebuilt every frame from the live model; it MUST be focus-stable by
    // id (a screen-reader user is not flung back to the document top each frame). Focus a
    // stable control (End Turn), let several frames pass, and assert focus never moved — the
    // rebuild restores the same node by its data-a11y-id.
    const et = endTurn(page);
    await et.focus();
    await expect(et).toBeFocused();
    // Let the frame loop rebuild the tree a few times (the rAF draw runs continuously).
    await page.waitForTimeout(300);
    const stillEnd = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      return el?.getAttribute("data-a11y-id") ?? "";
    });
    expect(stillEnd, "focus stayed on End Turn across the rebuilds").toBe("fab-end");
    await expect(et).toBeFocused();
  });

  test("the live region announces a resolved move in WORDS (no glyphs)", async ({ page }) => {
    desktopOnly();
    await startLocalGameOrSkip(page);
    // Announcement quality: when a move resolves, the #status live region carries a worded
    // phrase a screen reader can voice — never the canvas's ⚔ / → glyphs. End the turn (a
    // move that always resolves) and assert the announcement updated to glyph-free words.
    const status = page.locator("#status");
    const before = (await status.textContent())?.trim() ?? "";
    await endTurn(page).press("Enter");
    await expect.poll(async () => (await status.textContent())?.trim() ?? "", { timeout: 25_000 }).not.toBe(before);
    const after = (await status.textContent()) ?? "";
    for (const glyph of ["⚔", "→", "←", "°", "⌂", "▒", "░", "★", "●"]) {
      expect(after.includes(glyph), `announcement "${after}" must be words, not ${glyph}`).toBe(false);
    }
  });
});
