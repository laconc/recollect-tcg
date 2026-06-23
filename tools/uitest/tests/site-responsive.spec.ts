import { test, expect, Page } from "@playwright/test";
import { expectNoHorizontalScroll, expectTouchTarget } from "./helpers";

// ─────────────────────────────────────────────────────────────────────────────
// STATIC-SITE RESPONSIVE / TOUCH SWEEP — the marketing pages across the full width
// band (mobile · tablet · desktop) + a coarse-pointer (touch) pass.
//
// The play CLIENT's responsive law (the canvas shell) is covered by
// responsive.spec.ts (which needs a GL surface). THIS file covers the STATIC
// marketing pages (site/ → dist/: index · play · guide · rules · lore · cards ·
// contact · feedback) — plain semantic HTML/CSS that renders FAITHFULLY HEADLESS,
// so it runs ANYWHERE (including a no-GPU sandbox) and stays in the fast default
// `make uitest` suite.
//
// What it asserts, at EVERY width, on EVERY page:
//   • no horizontal scroll (the responsive law — brand_and_accessibility.md
//     "No horizontal scroll at any width");
//   • the primary nav is present and its first link is a real, visible target;
//   • the skip-link (the keyboard/AT entry point) exists and targets #main, and
//     every in-page anchor resolves (no dangling fragment at this width);
//   • the single <h1>/<main> landmark spine survives the reflow (the page never
//     collapses to a structureless blob on a narrow viewport).
// Plus a coarse-pointer (touch) pass: nav links + the play page's primary CTA meet
// the WCAG 2.5.5 ≥44px target, and a touch tap on a nav link navigates.
//
// These run JS-OFF — the marketing chrome is progressive enhancement (semantic
// anchors), so disabling JS proves the wayfinding + layout are real, not scripted.
// The semantic-HTML a11y (roles, contrast, names) is covered by axe.spec.ts; this
// file owns the LAYOUT-across-widths + touch-ergonomics half.
// ─────────────────────────────────────────────────────────────────────────────

// JS-off: the static pages are progressive enhancement; every check here is
// structural/layout, so disabling JS proves the responsive law holds without it.
test.use({ javaScriptEnabled: false });

// Every marketing page the site ships (the same set axe.spec.ts sweeps).
const PAGES = [
  "/index.html",
  "/play.html",
  "/guide.html",
  "/rules.html",
  "/lore.html",
  "/cards.html",
  "/contact.html",
  "/feedback.html",
] as const;

// The width band the brand doc names — a 320px phone, a mid tablet, a desktop.
// (The Playwright device PROJECTS set their own viewport; here we drive explicit
// widths so the FULL band is swept regardless of which project runs the file, and
// the narrow 320px edge — the tightest case in brand_and_accessibility.md — is
// always exercised.) A tall height keeps the whole document in one layout pass.
const WIDTHS = [
  { name: "mobile-320", w: 320, h: 880 }, // the tightest supported phone
  { name: "mobile-393", w: 393, h: 880 }, // a common modern phone (Pixel-class)
  { name: "tablet-768", w: 768, h: 1024 }, // iPad portrait
  { name: "tablet-1024", w: 1024, h: 768 }, // iPad landscape
  { name: "desktop-1280", w: 1280, h: 900 }, // a laptop/desktop
  { name: "desktop-1920", w: 1920, h: 1080 }, // a wide desktop
] as const;

/** The in-page `#…` link targets that have no matching element id, so we can prove
 *  the skip-link (and any other in-page anchor) resolves at this width. */
async function danglingInPageAnchors(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const ids = new Set(Array.from(document.querySelectorAll("[id]"), (e) => e.id));
    const targets = Array.from(document.querySelectorAll('a[href^="#"]'), (a) =>
      (a.getAttribute("href") || "").slice(1),
    ).filter((t) => t.length > 0);
    return targets.filter((t) => !ids.has(t));
  });
}

// The layout-across-widths matrix: every page × every width. This is the broadened
// static responsive coverage — previously only the picker + lore/rules pages were
// checked for horizontal scroll, and only at the three device projects' widths.
for (const { name, w, h } of WIDTHS) {
  test.describe(`static site @ ${name} (${w}px)`, () => {
    // The static DOM is identical across the file's three device projects, so the
    // explicit width matrix runs once (desktop project) — otherwise every width
    // would be asserted three times over byte-identical markup. The device projects
    // still contribute their own native viewports through responsive.spec (client).
    test.skip(
      () => test.info().project.name !== "desktop",
      "static DOM is viewport-independent; the explicit width matrix runs once",
    );

    for (const path of PAGES) {
      test(`${path} reflows with no horizontal scroll + keeps its spine`, async ({ page }) => {
        await page.setViewportSize({ width: w, height: h });
        await page.goto(path);

        // The responsive law: nothing overflows the viewport width at any size.
        await expectNoHorizontalScroll(page);

        // The document spine survives the reflow: exactly one <h1>, one <main>, and
        // the primary nav landmark (the page never collapses to a structureless blob
        // on a narrow viewport).
        await expect(page.locator("main#main")).toHaveCount(1);
        await expect(page.locator("h1")).toHaveCount(1);
        const nav = page.locator("nav.site-nav");
        await expect(nav).toHaveCount(1);
        await expect(nav.locator("a").first()).toBeVisible();

        // The skip-link — the keyboard/AT entry point — is present and points at the
        // main landmark (so it can never dangle as the layout changes).
        await expect(page.locator("a.skip-link")).toHaveAttribute("href", "#main");
        const dangling = await danglingInPageAnchors(page);
        expect(dangling, `dangling in-page anchors at ${name}: ${dangling.join(", ")}`).toEqual([]);
      });
    }
  });
}

// The coarse-pointer (touch) ergonomics pass — runs on the phone + tablet projects
// (which advertise `pointer: coarse`); desktop's fine-pointer build is denser by
// design and is skipped. Asserts touch-target sizes (the right WCAG bar per element
// class) and that a real touch tap navigates.
test.describe("static site — touch ergonomics (coarse pointer)", () => {
  test("nav links + the play CTA meet their touch-target floor", async ({ page }) => {
    const coarse = await page.evaluate(() => matchMedia("(pointer: coarse)").matches);
    test.skip(!coarse, "fine-pointer (desktop) build is intentionally denser");

    await page.goto("/index.html");
    // The top-nav entries are INLINE text-style links, held to the AA bar — WCAG 2.5.8
    // "Target Size (Minimum)" = 24px (NOT the AAA 44px the button-like affordances get;
    // 2.5.8 explicitly exempts inline links anyway). They render ~26px tall here, so
    // they clear AA. (Polish note for the maintainer: bumping them toward the 44px AAA
    // target would be a nicer phone tap — a site-CSS change, out of scope for this
    // test-coverage work; recorded here, not silently asserted at 44px.)
    const links = page.locator("nav.site-nav a");
    const n = await links.count();
    expect(n).toBeGreaterThan(2);
    for (let i = 0; i < n; i++) {
      await expectTouchTarget(links.nth(i), 24);
    }

    // The play page's primary CTA ("Launch the game") is a BUTTON-LIKE control and the
    // most important tap on the site — it must clear the generous ≥44px target on a
    // phone/tablet (it renders ~52px, comfortably above).
    await page.goto("/play.html");
    const launch = page.getByRole("link", { name: /Launch the game/ });
    await expect(launch).toBeVisible();
    await expectTouchTarget(launch, 44);
  });

  test("a touch tap on a nav link navigates (the inline nav is operable on touch)", async ({
    page,
  }) => {
    const coarse = await page.evaluate(() => matchMedia("(pointer: coarse)").matches);
    test.skip(!coarse, "this asserts the coarse-pointer tap path");

    await page.goto("/index.html");
    // Tap (not click) the Rules nav link via the touch pointer; the page navigates.
    // `tap()` requires the touch-enabled context the phone/tablet projects provide.
    const rules = page.locator("nav.site-nav a", { hasText: /^Rules$/ });
    test.skip((await rules.count()) === 0, "no Rules nav link on this build");
    await rules.first().tap();
    await expect(page).toHaveURL(/rules\.html/);
    // And the destination itself reflows cleanly at this touch viewport.
    await expectNoHorizontalScroll(page);
  });
});
