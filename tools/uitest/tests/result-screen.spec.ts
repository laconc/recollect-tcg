import { test, expect, Page, Locator } from "@playwright/test";
import {
  startLocalGame,
  endTurn,
  handButtons,
  a11yTree,
  setAnimSpeed,
  dismissOpeningMulligan,
} from "./helpers";

// #100 Phase D — the in-canvas result screen + the Dusk/Nightfall set-pieces (the LAST
// canvas-native phase). When a local 1v1 match ENDS, the canvas draws the result
// screen — the verdict in the game's voice (the Memory keeps [winner] / both kept /
// forgotten), the score breakdown (board + the Solace's erasure tally), and the actions
// (Rematch / New opponent / Back to site). The canvas is opaque to assistive tech, so the
// result is also mirrored in the virtual a11y tree (#shell-a11y) + announced in the
// #status live region, and the actions are actionable nodes (invariant 7).
//
// Reaching the verdict means playing the match to its end (Nightfall, R12). We drive it
// with the accessible mirror at fast/reduced-motion pacing: end each of the human's turns
// (and resolve a hand-cap Release by activating a hand card when End Turn isn't offered),
// until the result section appears.

// The a11y-tree buttons are screen-reader-only; a player drives them by KEYBOARD.
// `locator.press` is one auto-retrying, actionable step (re-resolve → focus → key), so it
// never races the frame loop rebuilding the tree between a separate focus() and press().
async function activate(page: Page, locator: Locator, timeout = 10_000): Promise<void> {
  await locator.press("Enter", { timeout });
}

const resultSection = (page: Page): Locator =>
  page.locator("#shell-a11y h2", { hasText: /match has ended/i });

const resultAction = (page: Page, verb: string): Locator =>
  page.locator(`#shell-a11y button[data-a11y-id='result-${verb}']`);

// The current round (1-based) from the #status line, or 0 if not shown.
async function currentRound(page: Page): Promise<number> {
  const st = (await page.locator("#status").textContent()) ?? "";
  const m = st.match(/Round (\d+)/);
  return m ? Number(m[1]) : 0;
}

// Play the local match to its end (the verdict). Returns once the result section shows.
// Each of the human's turns we End Turn; if that doesn't advance the round (the human is at
// the hand cap, so End Turn is illegal until a Release), we activate a hand card — which
// fires the pending Release (or plays a card) — then End Turn. Bounded by a turn budget (a
// match is 12 rounds) so a stuck state fails fast rather than hanging.
async function playToTheEnd(page: Page): Promise<void> {
  const result = resultSection(page);
  // Wait for the bot's paced replay (fast / reduced-motion ⇒ near-instant) to hand back, or
  // for the verdict — i.e. an actionable in-game tree, or the result section.
  const handBack = () =>
    page
      .waitForFunction(
        () => {
          const tree = document.getElementById("shell-a11y");
          const ended = !!tree && /match has ended/i.test(tree.textContent || "");
          const fab = !!tree && !!tree.querySelector("button[data-a11y-id='fab-end']");
          return ended || fab;
        },
        { timeout: 20_000 },
      )
      .catch(() => {});

  for (let i = 0; i < 60; i++) {
    if (await result.count()) return;
    const before = await currentRound(page);
    const et = endTurn(page);
    // The match can END on this very End Turn — the result screen replaces the FAB
    // mid-press. Bound + swallow the press so the loop falls through to the result check
    // below instead of hanging on a button that just vanished.
    if (await et.count()) await activate(page, et).catch(() => {});
    await handBack();
    if (await result.count()) return;
    // If the round didn't advance, the human is over the hand cap (End Turn is illegal until
    // a Release): activate a hand card to clear it, then continue. (Playing also drives the
    // human's score up, so the verdict isn't always a shutout.)
    if ((await currentRound(page)) === before && before > 0) {
      const hb = handButtons(page).first();
      if (await hb.count()) {
        await activate(page, hb).catch(() => {});
        // A placed spirit then needs a target tile; activate one if offered (harmless if not).
        const tile = page.locator("#shell-a11y button[data-a11y-id^='tile-']").first();
        if (await tile.count()) await activate(page, tile).catch(() => {});
        await handBack();
      }
    }
  }
  await expect(result, "the match reached its verdict within the turn budget").toHaveCount(1);
}

test.describe("the in-canvas result screen (#100 Phase D)", () => {
  // Reaching the verdict means playing a full 12-round match — expensive. The result
  // screen's logic is breakpoint-independent (the responsive law is covered by the layout
  // suites), so run these once (the desktop project) rather than ×3, to keep CI sane.
  // Reduced motion so the paced bot replays collapse to a near-instant tick — a full
  // 12-round match then completes well inside the test timeout.
  test.beforeEach(async ({ page }, testInfo) => {
    test.skip(
      testInfo.project.name !== "desktop",
      "result-screen playthrough runs once (desktop) — breakpoint-independent",
    );
    await page.emulateMedia({ reducedMotion: "reduce" });
  });

  test("ending the match shows the verdict + score breakdown + actions, announced", async ({
    page,
  }) => {
    test.setTimeout(180_000);
    await startLocalGame(page);
    await setAnimSpeed(page, "fast");

    await playToTheEnd(page);

    // The Dusk (after R8) and Nightfall (R12) set-pieces fired as the rounds crossed them —
    // the rim-contraction/clock flourish over the board, each announced in the live region
    // (the seal text is transient on screen, so we assert via the test-observability hook
    // the page records each shown set-piece into). A full match crosses R12 (Nightfall),
    // and a 12-round match always reaches the Dusk too.
    const duskShown: string[] = await page.evaluate(
      () => (window as any).__recollectTest?.duskShown ?? [],
    );
    expect(duskShown, `a Dusk / Nightfall set-piece should have fired; shown: ${duskShown}`)
      .toContain("nightfall");
    expect(duskShown).toContain("dusk");

    // The verdict speaks in the game's voice — the Memory keeps someone, or is forgotten,
    // or both are kept (a draw). It heads the result section AND lands in the live region.
    const verdictRe = /(the memory keeps|is forgotten|both are kept)/i;
    await expect(resultSection(page)).toContainText(verdictRe);
    await expect(page.locator("#status")).toContainText(verdictRe);

    // The score breakdown is mirrored as text (board points; the Solace's erasure tally
    // folds in as its own row when the opponent is the Solace) — never opaque.
    await expect(
      a11yTree(page).locator("p").filter({ hasText: /Final tally/i }),
    ).toHaveCount(1);

    // The three actions are actionable buttons (the accessible twins of the canvas buttons).
    await expect(resultAction(page, "rematch")).toBeVisible();
    await expect(resultAction(page, "rematch")).toHaveText(/rematch/i);
    await expect(resultAction(page, "new")).toHaveText(/new opponent/i);
    await expect(resultAction(page, "site")).toHaveText(/back to site/i);

    // Focus management on this SCREEN CHANGE: the board (and its tile / FAB buttons) is gone,
    // so focus moved to the verdict heading — a `tabindex="-1"` landing — rather than being
    // stranded on a removed control. A keyboard / screen-reader user lands on the outcome and
    // can Tab straight into Rematch / New opponent / Back. (Polled: the move is deferred a
    // frame past the result draw.)
    await expect
      .poll(
        async () =>
          page.evaluate(() => {
            const el = document.activeElement as HTMLElement | null;
            return !!el && el.tagName === "H2" && /match has ended/i.test(el.textContent || "");
          }),
        { timeout: 10_000 },
      )
      .toBe(true);
    // From the verdict, a single Tab reaches the first result action (no keyboard trap).
    await page.keyboard.press("Tab");
    await expect
      .poll(
        async () =>
          page.evaluate(() => {
            const el = document.activeElement as HTMLElement | null;
            return (
              !!el &&
              !!el.closest("#shell-a11y") &&
              (el.getAttribute("data-a11y-id") || "").startsWith("result-")
            );
          }),
        { timeout: 5_000 },
      )
      .toBe(true);
  });

  test("Rematch starts a fresh match (the result screen is dismissed)", async ({ page }) => {
    test.setTimeout(180_000);
    await startLocalGame(page);
    await setAnimSpeed(page, "fast");
    await playToTheEnd(page);

    // Rematch reseeds an equivalent local match: the result section is gone. A fresh
    // match re-offers the opening Mulligan — keep the hand to reach the playable
    // shell — then the actionable in-game tree (the End Turn FAB) returns.
    await activate(page, resultAction(page, "rematch"));
    await expect(resultSection(page)).toHaveCount(0, { timeout: 15_000 });
    await dismissOpeningMulligan(page);
    await expect(endTurn(page)).toBeVisible({ timeout: 15_000 });
  });

  test("New opponent returns to the picker", async ({ page }) => {
    test.setTimeout(180_000);
    await startLocalGame(page);
    await setAnimSpeed(page, "fast");
    await playToTheEnd(page);

    await activate(page, resultAction(page, "new"));
    // Back at the "Choose your match" picker (a fresh match can be started).
    await expect(page.getByRole("heading", { name: "Choose your match" })).toBeVisible({
      timeout: 15_000,
    });
  });

  test("Back to site links home", async ({ page }) => {
    test.setTimeout(180_000);
    await startLocalGame(page);
    await setAnimSpeed(page, "fast");
    await playToTheEnd(page);

    await activate(page, resultAction(page, "site"));
    // Navigates to the site root (the play header's "Back to site" target).
    await page.waitForURL(/index\.html$|\/$/, { timeout: 15_000 });
  });
});
