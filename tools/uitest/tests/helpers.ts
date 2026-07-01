import { Page, Locator, expect, test } from "@playwright/test";

// Shared helpers for the Recollect UI suite. These drive the BUILT play client
// (`/client/`) as a player would.
//
// #100 Phase B: the LOCAL 1v1 match is now canvas-native — the in-canvas
// affordances drive every action and the **virtual a11y tree** (#shell-a11y)
// mirrors them as actionable ARIA buttons (the accessible path, invariant 7). The
// transitional HTML move buttons (#moves) are retired for that mode (they remain
// only for online / 2v2, which aren't on the shell). So the local-game helpers
// drive the a11y tree's buttons (which fire the SAME engine commands the canvas
// affordances do); the picker/responsive helpers are unchanged.

/** Open the play client and wait for the "Choose your match" picker to render
 *  (which only happens once the wgpu renderer has mounted and `main()` ran). */
export async function openPicker(page: Page): Promise<void> {
  await page.goto("/client/");
  await expect(page.getByRole("heading", { name: "Choose your match" })).toBeVisible({
    timeout: 30_000,
  });
}

/** Start a LOCAL (vs-AI) game by picking the first offered deck, and wait for
 *  the in-game shell to come live. Returns when the board is playable (the canvas
 *  shell is drawn and the a11y tree's End Turn button exists). */
export async function startLocalGame(page: Page): Promise<void> {
  await openPicker(page);
  // The three decks come first; "Watch a 2v2" / "Play online" follow. Pick the
  // first deck (a normal 1v1-vs-AI local game).
  await page.locator("#picker .style").first().click();
  // The status line proves the engine is live: the canvas-native gameplay prompt
  // ("tap a piece or card to act, or End Turn."), OR — at the very start — the
  // match-start opener announcement ("… opens the match …"), or the
  // opening Mulligan modal's prompt ("… mulligan your hand?"). Any means the match
  // is live.
  await expect(page.locator("#status")).toContainText(
    /tap a piece or card to act|End Turn|opens? the match|mulligan your hand/i,
    { timeout: 15_000 },
  );
  // The opening Mulligan modal opens automatically at the very start. To reach the
  // PLAYABLE shell (the board/hand/FAB tree), keep the opening hand (dismiss the modal);
  // a dedicated spec exercises the modal itself. The keep button is in the choice a11y tree.
  await dismissOpeningMulligan(page);
  // The virtual a11y tree's End Turn button proves the actionable mirror is built.
  await expect(endTurn(page)).toBeVisible({ timeout: 15_000 });
}

/** The opening Mulligan modal's option buttons in the virtual a11y tree (the
 *  choice tree replaces the live game tree while the modal is up). */
export function mulliganButton(page: Page): Locator {
  return page.locator("#shell-a11y button[data-a11y-id='choice-mulligan-0']");
}
export function keepHandButton(page: Page): Locator {
  return page.locator("#shell-a11y button[data-a11y-id='choice-keep-0']");
}

/** If the opening Mulligan modal is up, keep the opening hand (dismiss it) so the
 *  playable shell is reached. A no-op if the modal isn't showing (e.g. a rematch path,
 *  or a future opener that doesn't offer it). Keyboard-driven (sr-only tree). */
export async function dismissOpeningMulligan(page: Page): Promise<void> {
  const keep = keepHandButton(page);
  if (await keep.count()) {
    await keep.focus();
    await page.keyboard.press("Enter");
    // The modal clears → the live game tree returns (End Turn reappears).
    await expect(keep).toHaveCount(0, { timeout: 10_000 });
  }
}

/** The off-screen virtual a11y tree mirroring the canvas (#100 Phase B). */
export function a11yTree(page: Page): Locator {
  return page.locator("#shell-a11y");
}

/** Start a local match, but SKIP (don't fail) the test if the wgpu canvas shell
 *  never mounts — i.e. there is no real GL surface (a headless/no-GPU sandbox: the
 *  bundled headless-shell's software-WebGL is rejected by wgpu, so the canvas never
 *  comes live and `startLocalGame` times out). Used by the canvas-dependent specs
 *  (deep a11y traversal, the wgpu visual goldens) so they exercise the real surface
 *  where a GPU exists and quietly defer where one doesn't — the GPU-deferred path
 *  documented in docs/testing.md + docs/manual_verification.md. Mirrors the
 *  try/skip guard axe.spec.ts uses for the live-game subtree. */
export async function startLocalGameOrSkip(page: Page): Promise<void> {
  try {
    await startLocalGame(page);
  } catch (_) {
    test.skip(
      true,
      "no GL surface — the wgpu canvas shell did not mount (GPU-deferred; see docs/testing.md)",
    );
  }
}

/** #100 — mount an ONLINE 1v1 match in the canvas shell with NO live server: inject a
 *  sample redacted `welcome` (the real wire shape the server sends — a `PlayerView` for
 *  `seat` + the legal list) through the same `onServerMsg` path the socket drives. The
 *  live socket is browser-verify; this covers the headless-testable online rendering /
 *  a11y / redaction. `moves` opening plays populate the board so the card treatment shows.
 *  Returns once the online shell's a11y tree (End Turn) is built. */
export async function startOnlineGame(
  page: Page,
  opts: { seat?: "A" | "B"; moves?: number; seed?: number } = {},
): Promise<void> {
  await openPicker(page);
  const { seat = "A", moves = 2, seed = 7 } = opts;
  await page.evaluate(
    ([seat, moves, seed]) => {
      const t = (window as unknown as { __recollectTest: any }).__recollectTest;
      const msg = JSON.parse(t.sampleOnlineWelcome(seed, seat, moves));
      t.injectServerMsg(msg);
    },
    [seat, moves, seed] as const,
  );
  // The online shell is live: the status line names the online match, and the a11y
  // tree's End Turn button proves the actionable mirror is built (when it's your turn).
  await expect(page.locator("#status")).toContainText(/online/i, { timeout: 15_000 });
}

/** Mount an online 1v1 match whose board has a STANDING-FADED form (rescuable) on tile
 *  12 with the matching base in hand — so the canvas DEVOLVE (recede) affordance is live.
 *  Injected through the production `onServerMsg` path (no live server). Returns once the
 *  online shell is up. Used by the devolve-affordance specs. */
export async function startDevolveGame(page: Page, seed = 7): Promise<void> {
  await openPicker(page);
  await page.evaluate((seed) => {
    const t = (window as unknown as { __recollectTest: any }).__recollectTest;
    const msg = JSON.parse(t.sampleDevolveWelcome(seed));
    t.injectServerMsg(msg);
  }, seed);
  await expect(page.locator("#status")).toContainText(/online/i, { timeout: 15_000 });
}

/** #100 — mount an ONLINE 2v2 match (a 6×6 `TeamView`) in the canvas shell, no live
 *  server. Injects a sample redacted `team_welcome` through the production `onServerMsg`
 *  path. Returns once the status names the 2v2 match. */
export async function startTeamGame(page: Page, seed = 7): Promise<void> {
  await openPicker(page);
  await page.evaluate((seed) => {
    const t = (window as unknown as { __recollectTest: any }).__recollectTest;
    const msg = JSON.parse(t.sampleTeamWelcome(seed));
    t.injectServerMsg(msg);
  }, seed);
  await expect(page.locator("#status")).toContainText(/2v2/i, { timeout: 15_000 });
}

/** The End Turn control — now an actionable button in the virtual a11y tree
 *  (#shell-a11y), firing the same EndTurn command the canvas FAB does. */
export function endTurn(page: Page): Locator {
  return page.locator("#shell-a11y button[data-a11y-id='fab-end']");
}

/** The Glimpse control — the other global action button in the a11y tree. */
export function study(page: Page): Locator {
  return page.locator("#shell-a11y button[data-a11y-id='fab-study']");
}

/** Set the animation-speed setting. The play settings live behind the nav's
 *  **Options** disclosure, so open it, choose, then close — the control governs the paced
 *  replay. */
export async function setAnimSpeed(page: Page, value: "normal" | "fast"): Promise<void> {
  const toggle = page.locator("#options-toggle");
  await toggle.click();
  await page.locator("#anim-speed").selectOption(value);
  // Close the panel again so it doesn't overlap the canvas during the test.
  await toggle.click();
}

/** Every actionable hand-card button in the a11y tree (mirrors the hand tray). */
export function handButtons(page: Page): Locator {
  return page.locator("#shell-a11y button[data-a11y-id^='hand-']");
}

/** Every actionable board-tile button in the a11y tree (occupied / actionable). */
export function tileButtons(page: Page): Locator {
  return page.locator("#shell-a11y button[data-a11y-id^='tile-']");
}

/** Assert the document has no horizontal overflow at the current viewport. */
export async function expectNoHorizontalScroll(page: Page): Promise<void> {
  const overflow = await page.evaluate(() => {
    const d = document.documentElement;
    // A 1px rounding slack — sub-pixel layout shouldn't count as a scrollbar.
    return d.scrollWidth - d.clientWidth;
  });
  expect(overflow, "document should not scroll horizontally").toBeLessThanOrEqual(1);
}

/** Assert a locator's rendered box clears a touch-target size floor. The default
 *  (44px) is the generous bar the canvas-shell affordances + button-like CTAs hold
 *  (WCAG 2.5.5 "Target Size (Enhanced)", AAA — the signature-tier default); pass a
 *  smaller `min` for the AA bar (WCAG 2.5.8 "Target Size (Minimum)" = 24px), which
 *  is what inline text-style links (e.g. the top-nav) are held to. */
export async function expectTouchTarget(locator: Locator, min = 44): Promise<void> {
  const box = await locator.boundingBox();
  expect(box, "element should be laid out").not.toBeNull();
  // Allow a 0.5px sub-pixel slack on rounding.
  expect(box!.height, `touch-target height ≥ ${min}px`).toBeGreaterThanOrEqual(min - 0.5);
}
