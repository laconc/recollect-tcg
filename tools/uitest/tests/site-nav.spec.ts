import { test, expect, Page } from "@playwright/test";
import { expectNoHorizontalScroll } from "./helpers";

// ─────────────────────────────────────────────────────────────────────────────
// SITE NAVIGATION & CROSS-LINKING — the marketing site's in-page wayfinding.
//
// The cards / lore / rules pages gained progressive-enhancement navigation that
// must work with JS OFF (plain `<a href="#…">` anchors + the generated markup):
//   • cards.html — base↔evolution cross-navigation (a base links DOWN to its
//     form(s), grouped by tier, menus shown as "A or B"; a form links UP to its
//     base) + a "Read its lore" link to the lore page, gated on authored prose.
//   • lore.html — a table of contents + the per-card lore sectioned by resonance
//     (from cards.toml), each card an anchored entry linking back to
//     its catalog tile.
//   • rules.html — a table of contents to each anchored section.
//
// These assert the wayfinding EXISTS and every anchor RESOLVES (no dangling
// in-page or cross-page link), and — because nothing here may depend on
// JavaScript — they run the link-integrity checks with `javaScriptEnabled: false`.
// The semantic-HTML a11y of the new structure (landmarks, heading order, names) is
// covered by the axe-core sweep in axe.spec.ts, which already scans these pages.
// ─────────────────────────────────────────────────────────────────────────────

/** The DOM structure is identical across the viewport projects, so the link-graph
 *  assertions run once (desktop). The responsive *layout* of the new long lore
 *  page is checked per-project below. */
function desktopOnly() {
  test.skip(
    test.info().project.name !== "desktop",
    "link-graph is viewport-independent; checked once on desktop",
  );
}

/** Collect the `#…` fragment every same-page anchor points at, and the set of ids
 *  present, so we can assert there are no dangling in-page links. */
async function inPageLinkIntegrity(page: Page) {
  return page.evaluate(() => {
    const ids = new Set(Array.from(document.querySelectorAll("[id]"), (e) => e.id));
    const targets = Array.from(document.querySelectorAll('a[href^="#"]'), (a) =>
      (a.getAttribute("href") || "").slice(1),
    ).filter((t) => t.length > 0);
    const dangling = targets.filter((t) => !ids.has(t));
    return { count: targets.length, dangling };
  });
}

// A JS-off context proves the nav is real progressive enhancement (anchors, not
// script). Every check in this file is structural, so disabling JS is safe.
test.use({ javaScriptEnabled: false });

test.describe("site navigation — rules page", () => {
  test("has a table of contents whose every link resolves to a section", async ({ page }) => {
    desktopOnly();
    await page.goto("/rules.html");
    const toc = page.locator("nav.page-toc");
    await expect(toc).toBeVisible();
    // Each major section is a TOC entry; assert the known ones are present + linked.
    for (const id of ["goal", "your-turn", "combat", "becoming", "dusk", "two-readings"]) {
      await expect(toc.locator(`a[href="#${id}"]`)).toHaveCount(1);
      await expect(page.locator(`h2#${id}`)).toHaveCount(1);
    }
    const { count, dangling } = await inPageLinkIntegrity(page);
    expect(count).toBeGreaterThan(5);
    expect(dangling, `dangling in-page links: ${dangling.join(", ")}`).toEqual([]);
  });
});

test.describe("site navigation — lore page", () => {
  test("has a TOC + a section per resonance, all anchors resolving", async ({ page }) => {
    desktopOnly();
    await page.goto("/lore.html");
    const toc = page.locator("nav.lore-toc");
    await expect(toc).toBeVisible();
    // The §9-mirroring sections: the six resonances, Remnants & Neutral, the Solace,
    // the Foundlings. Each is a TOC link AND a real <section> with that id.
    const sections = [
      "wonder",
      "fear",
      "sorrow",
      "harmony",
      "fury",
      "resolve",
      "remnants",
      "solace",
      "foundlings",
    ];
    for (const id of sections) {
      await expect(toc.locator(`a[href="#${id}"]`)).toHaveCount(1);
      await expect(page.locator(`section.lore-section#${id}`)).toHaveCount(1);
    }
    const { dangling } = await inPageLinkIntegrity(page);
    expect(dangling, `dangling in-page links: ${dangling.join(", ")}`).toEqual([]);
    // The lore is real prose: hundreds of anchored card entries to jump to.
    expect(await page.locator("article.lore-entry[id^='lore-']").count()).toBeGreaterThan(300);
  });

  test("each lore entry links back to its catalog tile", async ({ page }) => {
    desktopOnly();
    await page.goto("/lore.html");
    // A known card entry: its heading links to cards.html#card-<key>.
    const entry = page.locator("#lore-cloudling");
    await expect(entry).toHaveCount(1);
    await expect(entry.locator('a[href="cards.html#card-cloudling"]')).toHaveCount(1);
  });
});

test.describe("site navigation — cards evolution cross-navigation", () => {
  test("a base links to its form(s); a form links back; a menu shows both", async ({ page }) => {
    desktopOnly();
    await page.goto("/cards.html");

    // Simple line: Cloudling (base) → Stormswell (Primal).
    const cloudling = page.locator("#card-cloudling .evo");
    await expect(cloudling).toContainText(/Evolves into/);
    await expect(cloudling.locator('a[href="#card-stormswell"]')).toHaveCount(1);

    // The form links back UP to its base.
    const stormswell = page.locator("#card-stormswell .evo");
    await expect(stormswell).toContainText(/Evolves from/);
    await expect(stormswell.locator('a[href="#card-cloudling"]')).toHaveCount(1);

    // A gentle/malign MENU: one base, two Primal forms, both linked from the line.
    const erasure = page.locator("#card-the-kind-erasure .evo");
    await expect(erasure.locator('a[href="#card-the-kindest-erasure"]')).toHaveCount(1);
    await expect(erasure.locator('a[href="#card-the-unkindest-erasure"]')).toHaveCount(1);

    // A Primal+Fabled base shows both tier labels.
    const wisp = page.locator("#card-wisp-of-doubt .evo");
    await expect(wisp).toContainText("Primal");
    await expect(wisp).toContainText("Fabled");
  });

  test("every cards-page anchor resolves (evolution links + lore links are clean)", async ({
    page,
  }) => {
    desktopOnly();
    await page.goto("/cards.html");
    const { count, dangling } = await inPageLinkIntegrity(page);
    // Many evolution anchors across the catalog; none may dangle.
    expect(count).toBeGreaterThan(50);
    expect(dangling, `dangling evolution anchors: ${dangling.join(", ")}`).toEqual([]);

    // The lore cross-link is gated on authored prose: a procedural card has NO
    // link (so it can never dangle), a card with lore DOES.
    await expect(page.locator("#card-cloudling .card-lore-link")).toHaveCount(1);
    await expect(page.locator("#card-the-slammed-door .card-lore-link")).toHaveCount(0);
  });
});

test.describe("site navigation — cross-page links resolve both ways", () => {
  test("every cards→lore and lore→cards link points at a real anchor", async ({ page }) => {
    desktopOnly();
    // Gather the link sets from each page, and the id sets, then assert closure.
    await page.goto("/cards.html");
    const cards = await page.evaluate(() => ({
      ids: Array.from(document.querySelectorAll("article.card[id]"), (e) => e.id),
      toLore: Array.from(
        document.querySelectorAll('a[href^="lore.html#"]'),
        (a) => (a.getAttribute("href") || "").split("#")[1],
      ),
    }));
    await page.goto("/lore.html");
    const lore = await page.evaluate(() => ({
      ids: Array.from(document.querySelectorAll("article.lore-entry[id]"), (e) => e.id),
      toCards: Array.from(
        document.querySelectorAll('a[href^="cards.html#"]'),
        (a) => (a.getAttribute("href") || "").split("#")[1],
      ),
    }));

    const cardIds = new Set(cards.ids);
    const loreIds = new Set(lore.ids);
    const danglingCardsToLore = cards.toLore.filter((id) => !loreIds.has(id));
    const danglingLoreToCards = lore.toCards.filter((id) => !cardIds.has(id));

    expect(cards.toLore.length).toBeGreaterThan(300);
    expect(lore.toCards.length).toBeGreaterThan(300);
    expect(danglingCardsToLore, `cards→lore with no anchor: ${danglingCardsToLore.slice(0, 5)}`).toEqual([]);
    expect(danglingLoreToCards, `lore→cards with no tile: ${danglingLoreToCards.slice(0, 5)}`).toEqual([]);
  });
});

test.describe("site navigation — responsive layout holds with the new content", () => {
  // The long lore page must not introduce horizontal scroll at any breakpoint
  // (the responsive law; runs on every project, JS off).
  test("the lore page does not force horizontal scroll", async ({ page }) => {
    await page.goto("/lore.html");
    await expectNoHorizontalScroll(page);
  });

  test("the rules page does not force horizontal scroll", async ({ page }) => {
    await page.goto("/rules.html");
    await expectNoHorizontalScroll(page);
  });
});
