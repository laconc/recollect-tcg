import { Page, Locator } from "@playwright/test";
import { handButtons } from "./helpers";

// Shared helpers for the VISUAL GALLERY specs (gallery.spec.ts = stills,
// gallery-clips.spec.ts = videos). Purely additive — a NEW module, not the shared
// `helpers.ts` the other lanes also edit. See gallery.spec.ts's header for the
// gallery's purpose, how it drives the #100 A+B canvas-native client through the
// a11y tree, and the current-state caveat about the canvas pointer bridge.

/** Activate an a11y-tree button the way assistive tech (and the existing specs) do
 *  — focus + Enter — since the mirror is visually-hidden (a synthetic click at
 *  screen coordinates can't reach a clipped 1px element). Fires the same engine
 *  command the canvas affordance does, so the canvas re-renders the right scene
 *  (the lifted card, the lit tiles, the placed spirit). */
export async function activate(page: Page, locator: Locator): Promise<void> {
  await locator.focus();
  await page.keyboard.press("Enter");
}

/** The first hand card the engine reports as a legal play (so picking it up lights
 *  legal tiles + a place is possible). The opening 2-anima budget can leave none
 *  affordable — callers handle a zero count. */
export function firstPlayable(page: Page): Locator {
  return handButtons(page).filter({ hasText: /playable|evolution form/ }).first();
}
