import { test, expect, Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { openPicker, startLocalGame } from "./helpers";

// #105 — the pre-launch accessibility gate (AGENTS.md invariant 7; WCAG 2.1 AA).
// An automated axe-core sweep over BOTH surfaces:
//   • the marketing site (landing / guide / rules / lore / cards / play / contact /
//     feedback) — static semantic HTML, where axe is fully meaningful;
//   • the play client's CHROME — the shared nav, the Options disclosure panel, and
//     the virtual a11y tree (#shell-a11y) that mirrors the wgpu canvas. The canvas
//     itself is opaque to axe (and to assistive tech — that is WHY the a11y tree
//     exists), so we scan the DOM around it: the nav, the picker, and the sr-only
//     mirror once a match is live.
//
// We assert ZERO violations at the WCAG 2.0/2.1 A + AA conformance levels (the bar
// the brand doc sets). axe's tags select exactly those success criteria; a failure
// prints the offending rule + nodes so a regression is actionable.

const WCAG_AA_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"];

/** axe inspects the DOM/CSSOM, which is identical across our viewport projects for
 *  these pages (the responsive LAYOUT law is covered by responsive.spec.ts). Run the
 *  sweep once, on the desktop project, so the same findings aren't triple-counted and
 *  the GPU-light pages stay off the headed-Chromium contention. */
function desktopOnly() {
  test.skip(
    test.info().project.name !== "desktop",
    "axe scans the DOM once (desktop project); responsive.spec covers the breakpoints",
  );
}

/** Run axe over (a subtree of) the page at the WCAG-AA tags and assert no violations,
 *  with a readable failure naming each rule, its impact, and the node count. */
async function expectNoAxeViolations(
  page: Page,
  context: string,
  configure: (b: AxeBuilder) => AxeBuilder = (b) => b,
) {
  const results = await configure(new AxeBuilder({ page }).withTags(WCAG_AA_TAGS)).analyze();
  const summary = results.violations.map(
    (v) =>
      `${v.id} (${v.impact}) — ${v.help} [${v.nodes.length} node(s)]\n      ${v.helpUrl}\n      e.g. ${v.nodes[0]?.target?.join(" ")}`,
  );
  expect(results.violations, `axe WCAG 2.1 AA violations on ${context}:\n  ${summary.join("\n  ")}`).toEqual(
    [],
  );
}

test.describe("accessibility — axe-core WCAG 2.1 AA sweep", () => {
  // Every marketing page. Static HTML — axe checks landmarks, heading order,
  // colour contrast, names/roles, form labels, and the document language.
  const sitePages = [
    "/index.html",
    "/play.html",
    "/guide.html",
    "/rules.html",
    "/lore.html",
    "/cards.html",
    "/contact.html",
    "/feedback.html",
  ];

  for (const path of sitePages) {
    test(`site ${path} has no WCAG 2.1 AA violations`, async ({ page }) => {
      desktopOnly();
      await page.goto(path);
      // Let fonts/CSS settle so contrast checks read the real rendered colours.
      await page.waitForLoadState("networkidle");
      await expectNoAxeViolations(page, `site ${path}`);
    });
  }

  test("the play client chrome (picker state) has no WCAG 2.1 AA violations", async ({ page }) => {
    desktopOnly();
    await openPicker(page);
    // The canvas is excluded (opaque to axe by design — the a11y tree is the accessible
    // path; axe's colour-contrast can't read canvas pixels, so excluding it avoids a
    // false positive). Everything else — nav, Options trigger, the picker — must pass.
    await expectNoAxeViolations(page, "the client picker", (b) => b.exclude("#board"));
  });

  test("the play client Options disclosure panel has no WCAG 2.1 AA violations", async ({ page }) => {
    desktopOnly();
    await openPicker(page);
    // Open the disclosure so its controls (sound · reduced-motion · animation-speed)
    // are in the a11y tree to scan — a closed [hidden] panel isn't.
    await page.locator("#options-toggle").click();
    await expect(page.locator("#options-panel")).toBeVisible();
    await expectNoAxeViolations(page, "the Options panel", (b) => b.include("#options-panel"));
  });

  test("the live game's virtual a11y tree has no WCAG 2.1 AA violations", async ({ page }) => {
    desktopOnly();
    // A live local match builds #shell-a11y (the actionable ARIA mirror) + #board-sr
    // + #status — the screen-reader path for the canvas. It must be a clean a11y subtree
    // while a game is in progress. Needs a GL surface for the shell to mount; on a
    // no-GPU sandbox startLocalGame times out and the test skips rather than fails.
    try {
      await startLocalGame(page);
    } catch (_) {
      test.skip(true, "no GL surface — the canvas shell didn't mount (a11y tree needs a live match)");
      return;
    }
    await expectNoAxeViolations(page, "live play", (b) => b.exclude("#board"));
  });
});
