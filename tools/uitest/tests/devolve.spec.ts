import { test, expect, Page, Locator } from "@playwright/test";
import { a11yTree, startDevolveGame } from "./helpers";

// Devolution (design §5) — the **recede** rescue — on the canvas client. A Primal/Fabled
// form banished in combat lingers STANDING-FADED for a turn; on your Main you may play a
// BASE card from hand onto it to recede it a tier (the Lorekeeper REVERTS, the Solace
// RECEDES). The board is a wgpu canvas (opaque to assistive tech + pixel-unstable across
// machines), so the canvas pixels — the standing-Faded rendering and the recede chevron —
// are verified by the NATIVE shell.rs / scene.rs tests (the pure ShellScene geometry +
// the SpiritView treatment). What THIS browser suite guards is the affordance's correctness
// through the only honest seam into the canvas — the accessible mirror (#shell-a11y) and the
// board narration (#board-sr): the recede is an ACTIONABLE accessible element, the
// standing-Faded state is ANNOUNCED, and a select-then-target recede DISPATCHES the engine
// command (invariant 7 — the accessible path is at parity with the canvas, never a lesser one).
//
// The scenario is a crafted online welcome (the real wire shape) injected through the
// production onServerMsg path: seat A holds the base "Cloudling" and a standing-Faded
// "Stormswell" (its Primal) stands on tile 12, on A's turn — so the server's legal list
// carries a Devolve and the recede affordance is live (`sample_devolve_welcome_json`).

/** Activate an a11y-tree button the way assistive tech does — focus + Enter (a clipped
 *  1px sr-only element can't take a synthetic pointer click). */
async function activate(page: Page, locator: Locator): Promise<void> {
  await locator.focus();
  await page.keyboard.press("Enter");
}

const fadedTile = (page: Page): Locator =>
  a11yTree(page).locator("button[data-a11y-id='tile-12']");
const baseCard = (page: Page): Locator =>
  a11yTree(page).locator("button[data-a11y-id='hand-0']");

test.describe("devolution — the recede affordance (canvas a11y mirror)", () => {
  test("the standing-Faded form is an actionable node that announces the recede in the faction's word", async ({
    page,
  }) => {
    await startDevolveGame(page);
    const tile = fadedTile(page);
    await expect(tile).toBeVisible({ timeout: 15_000 });
    // It is ACTIONABLE (not a disabled / inspect-only node) — the recede is reachable on your turn.
    await expect(tile).toBeEnabled();
    // It ANNOUNCES the standing-Faded rescue state, in the faction's word. Cloudling is a
    // Lorekeeper (Wonder) line, so the verb is REVERT (the Solace would RECEDE) — vocabulary law.
    await expect(tile).toHaveText(/standing faded/i);
    await expect(tile).toHaveText(/revert/i);
  });

  test("the base card announces it can recede a faded form", async ({ page }) => {
    await startDevolveGame(page);
    const card = baseCard(page);
    await expect(card).toBeVisible({ timeout: 15_000 });
    // The hand card mirrors the recede chevron: it can be played onto the standing-Faded form.
    await expect(card).toHaveText(/revert/i);
    await expect(card).toBeEnabled();
  });

  test("the board narration (#board-sr) reads the standing-Faded form as rescuable", async ({
    page,
  }) => {
    await startDevolveGame(page);
    // The screen-reader board mirror narrates the rescuable window the canvas glow shows —
    // distinct from an ordinary, unrecoverable fade.
    await expect(page.locator("#board-sr")).toContainText(/standing faded/i, { timeout: 15_000 });
    await expect(page.locator("#board-sr")).toContainText(/rescuable/i);
  });

  test("select-then-target recede dispatches the engine command (keyboard, at parity with the canvas)", async ({
    page,
  }) => {
    await startDevolveGame(page);
    // Pick up the base card → the recede prompt names the flow (its targets light on the canvas).
    await activate(page, baseCard(page));
    await expect(page.locator("#status")).toContainText(/base card picked up/i, { timeout: 10_000 });
    await expect(page.locator("#status")).toContainText(/recede/i);
    // Activate the standing-Faded form → the recede is dispatched; the live region narrates the
    // move in the faction's word (the legal-move label) — the same two-activation gesture as a tap.
    await activate(page, fadedTile(page));
    await expect(page.locator("#status")).toContainText(/revert/i, { timeout: 10_000 });
  });
});
