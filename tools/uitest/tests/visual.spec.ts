import { test, expect } from "@playwright/test";
import { openPicker } from "./helpers";

// Visual-regression baselines at the key breakpoints (one per Playwright project,
// so phone / tablet / desktop each get their own committed image). Future diffs
// catch unintended layout/style drift in the play client's chrome.
//
// We screenshot the PICKER, masking the wgpu canvas: the canvas is software/GPU-
// rendered and varies machine to machine, so it's excluded from the pixel
// comparison (its presence + sizing is asserted in local-game.spec /
// responsive.spec instead). Animations are disabled by the config's
// `toHaveScreenshot` default, matching the client's reduced-motion path.
//
// The picker's deck previews are **seed-derived** (the client seeds from
// `Date.now()`), so the offered styles + their card lists vary run-to-run — which
// changes the wrapped height and makes a full-page baseline non-deterministic. We
// **freeze `Date.now()` to a constant** before the page boots so the seed (and thus
// the previews + the page height) are stable; the baseline then catches real chrome
// drift, not the seed lottery. (Before this freeze the tablet baseline flaked
// whenever a preview wrapped to an extra line — #100 Phase B finding.)

// Tagged @visual: pixel baselines are OS-specific (font rendering differs across
// macOS / Linux), so they're committed per-platform and asserted only where a
// matching baseline exists. `make uitest` runs them on a dev machine against the
// committed `-darwin` baselines; the CI lane runs the functional suite and skips
// @visual unless Linux baselines are committed (see .github/workflows/ci.yml).
test.describe("visual regression @visual", () => {
  test("the picker renders consistently @visual", async ({ page }) => {
    // Freeze the clock so the seed-derived deck previews are deterministic (a stable
    // page height ⇒ a stable full-page baseline). Must run before any page script.
    await page.addInitScript(() => {
      const FIXED = 1_700_000_000_000; // a fixed epoch ms — any constant works
      const _Now = Date.now.bind(Date);
      Date.now = () => FIXED;
      // new Date() with no args also reads the clock — pin it too, for completeness.
      const RealDate = Date;
      // @ts-expect-error - override the constructor's no-arg form only
      // eslint-disable-next-line no-global-assign
      Date = class extends RealDate {
        constructor(...args: unknown[]) {
          // @ts-expect-error - spread into the Date constructor
          super(...(args.length ? args : [FIXED]));
        }
        static now() {
          return FIXED;
        }
      } as DateConstructor;
      void _Now;
    });
    await openPicker(page);
    // Let layout + fonts settle.
    await expect(page.locator("#picker .style").first()).toBeVisible();
    await page.waitForTimeout(300);
    await expect(page).toHaveScreenshot("picker.png", {
      fullPage: true,
      mask: [page.locator("#board")],
    });
  });
});
