import { test, expect } from "@playwright/test";
import { openPicker } from "./helpers";

// Automates docs/manual_verification.md → "Static site" (the play page) and the
// first half of "Play client" (the picker renders, the styles + difficulty show).

test.describe("play page + picker", () => {
  test("the static play page renders and links to the client", async ({ page }) => {
    await page.goto("/play.html");
    await expect(page).toHaveTitle(/Recollect/);
    await expect(page.getByRole("heading", { name: "Play", level: 1 })).toBeVisible();
    // "Launch the game" opens the live client (manual_verification "Static site").
    const launch = page.getByRole("link", { name: /Launch the game/ });
    await expect(launch).toBeVisible();
    await expect(launch).toHaveAttribute("href", "client/");
  });

  test("the client loads and the picker offers the three tellings", async ({ page }) => {
    await openPicker(page);
    // F-29: the three offered styles (decks) the seed derives.
    const styles = page.locator("#picker .style b");
    await expect(styles.nth(0)).toBeVisible();
    // First three are the tellings; "Watch a 2v2" + "Play online" follow.
    const labels = await styles.allTextContents();
    expect(labels.length).toBeGreaterThanOrEqual(5);
    expect(labels).toContain("Watch a 2v2");
    expect(labels).toContain("Play online");
    // The three telling cards each preview a deck (spirits / spellbook / curve).
    await expect(page.locator("#picker .style").first()).toContainText(/spirits/);
  });

  test("each telling surfaces the OBJECTIVE selection-info chips", async ({ page }) => {
    await openPicker(page);
    // The picker must show the deck's SHAPE, not just the subjective blurb: the four
    // SelectionInfo facets (resonance lean · tempo · aggression · body-mix), computed
    // in core over many deck-gen seeds, render as chips on each telling card.
    const first = page.locator("#picker .style.telling").first();
    const chips = first.locator(".facet");
    await expect(chips).toHaveCount(4);
    // Each chip carries its dimension heading and a value word.
    const chipText = (await chips.allTextContents()).join(" ");
    for (const dim of ["Resonance", "Tempo", "Aggression", "Body mix"]) {
      expect(chipText, `the chips name the ${dim} dimension`).toContain(dim);
    }
    // The values are the authored vocabulary — a tempo word and an aggression word
    // appear across the three offered tellings (the seed always offers a mix).
    const allChips = await page.locator("#picker .style.telling .facet").allTextContents();
    const joined = allChips.join(" ");
    expect(joined).toMatch(/Fast|Even|Grindy/); // a tempo word
    expect(joined).toMatch(/Defensive|Measured|Aggressive/); // an aggression word
    // Each chip's detail gloss is exposed (the title attribute) so a value is never bare.
    await expect(first.locator(".facet").first()).toHaveAttribute("title", /.+/);
  });

  test("the telling cards are keyboard-operable buttons that read the shape as words", async ({
    page,
  }) => {
    // Invariant 7: the picker's actionable cards are real <button>s (Tab stops with a
    // visible focus ring), and the objective selection-info rides the button's
    // accessible NAME — so a screen reader hears the shape as words, not a row of
    // unlabelled colour. (The chips themselves are aria-hidden; the label is the
    // curated roll-up.)
    await openPicker(page);
    const first = page.locator("#picker .style.telling").first();
    await expect(first).toHaveJSProperty("tagName", "BUTTON");
    // The accessible name names the style, the tempo/aggression shape, and the deck stats.
    const label = (await first.getAttribute("aria-label")) ?? "";
    expect(label).toMatch(/Play .+/);
    expect(label).toMatch(/Fast|Even|Grindy/);
    expect(label).toMatch(/Defensive|Measured|Aggressive/);
    expect(label).toMatch(/spirits/);
    // The visual content (chips included) is hidden from the a11y tree (the label
    // carries the substance, so the screen reader doesn't fragment it chip-by-chip).
    await expect(first.locator("> span[aria-hidden='true']")).toHaveCount(1);
    await expect(first.locator("> span[aria-hidden='true'] .facets")).toHaveCount(1);
    // It is keyboard-focusable (a real Tab stop).
    await first.focus();
    await expect(first).toBeFocused();
  });

  test("the difficulty picker offers Easy / Normal / Hard / Expert", async ({ page }) => {
    await openPicker(page);
    const opts = page.locator("#diff option");
    await expect(opts).toHaveText(["Easy", "Normal", "Hard", "Expert"]);
    // Normal is the seeded default.
    await expect(page.locator("#diff")).toHaveValue("1");
  });
});
