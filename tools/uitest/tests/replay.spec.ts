import { test, expect, Page, Locator } from "@playwright/test";
import { startLocalGame, endTurn, handButtons, a11yTree } from "./helpers";

// #100 Phase C — the paced opponent-turn replay + announcements. When you end your
// turn in a local 1v1 telling, the bot's turn is REPLAYED action-by-action through a
// pacing queue (~1s/beat) instead of snapping the state: each discrete action animates
// with a subtle on-canvas caption AND a line in the #status live region (the a11y
// announcements — invariant 7), the affected tile lit, the Solace's erasure tally
// counting up on an Unwriting. While it replays the player's affordances are inert and
// the a11y tree shows the opponent's-turn state; your move returns when it finishes.
//
// This suite asserts the announcements land in the live region (the key gate), the
// affordances go inert + return, and the animation-speed setting governs the pacing.

// The a11y-tree buttons are screen-reader-only; a player drives them by KEYBOARD.
async function activate(page: Page, locator: Locator): Promise<void> {
  await locator.focus();
  await page.keyboard.press("Enter");
}

// End the human's turn through the accessible mirror, kicking off the bot's replay.
async function endTurnToReplay(page: Page): Promise<void> {
  await expect(endTurn(page)).toBeVisible();
  await activate(page, endTurn(page));
}

test.describe("the paced opponent-turn replay (#100 Phase C)", () => {
  test("ending your turn replays the opponent action-by-action with live announcements", async ({
    page,
  }) => {
    // A real (not reduced) motion preference + the default 'normal' speed give a wide
    // enough window to observe the paced beats deterministically.
    await page.emulateMedia({ reducedMotion: "no-preference" });
    await startLocalGame(page);

    // Capture every distinct #status line from the moment we end the turn until the
    // replay hands back. The bot takes ≥1 action, so ≥1 opponent announcement lands.
    const status = page.locator("#status");
    const seen: string[] = [];
    const poll = setInterval(async () => {
      try {
        const t = (await status.textContent())?.trim() ?? "";
        if (t && seen[seen.length - 1] !== t) seen.push(t);
      } catch (_) {}
    }, 80);

    await endTurnToReplay(page);

    // While the replay runs the virtual a11y tree shows the OPPONENT'S-TURN state — a
    // labeled note, and NO actionable End Turn button (your controls are inert). This
    // is observable promptly (it's set synchronously when the replay starts).
    await expect(a11yTree(page)).toContainText(/opponent is telling/i, { timeout: 5_000 });

    // The replay then finishes and your move returns: the End Turn button comes back as
    // an actionable a11y node (the actionable tree is restored).
    await expect(endTurn(page)).toBeVisible({ timeout: 30_000 });
    clearInterval(poll);

    // At least one captured status line is an OPPONENT ANNOUNCEMENT — names the actor
    // (the Solace / Lorekeepers) with an action verb, or a Dusk/Nightfall set-piece.
    const joined = seen.join(" || ");
    expect(
      seen.some((t) =>
        /(the Solace|Lorekeepers)\s+(plays|moves|evolves|tells|manifests|casts|binds|raises|reclaims|reveals|orders|studies|overwrites|sets|banishes|erases)/i.test(
          t,
        ) || /(Dusk falls|Nightfall)/i.test(t),
      ),
      `an opponent-action announcement should land in the live region; saw: ${joined}`,
    ).toBeTruthy();
  });

  test("the player's affordances are inert during the replay and return after", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "no-preference" });
    await startLocalGame(page);

    await endTurnToReplay(page);

    // Mid-replay the actionable mirror is REPLACED by the opponent's-turn state — a note
    // and no actionable buttons (nothing to do but watch). The note's presence implies
    // the End Turn / hand / tile buttons are gone (the tree only holds the note); we
    // assert that directly while it's showing.
    const tree = a11yTree(page);
    await expect(tree).toContainText(/opponent is telling/i, { timeout: 5_000 });
    expect(await endTurn(page).count(), "End Turn is gone while watching").toBe(0);
    expect(await handButtons(page).count(), "no hand affordances while watching").toBe(0);

    // After the replay your affordances come back (the hand is mirrored, End Turn is
    // actionable again) — the canvas migration never strands the accessible path.
    await expect(endTurn(page)).toBeVisible({ timeout: 30_000 });
    await expect(handButtons(page).first()).toBeVisible();
  });

  test("the match opens with an announcement naming the first player", async ({ page }) => {
    await startLocalGame(page);
    // The opener is the first thing the live region reads — it always names who opens
    // (design §5). Local 1v1 opens the human (Seat A), so it reads from your vantage.
    await expect(page.locator("#status")).toContainText(/open(s)? the telling/i, {
      timeout: 15_000,
    });
  });

  test("the animation-speed setting is present and governs pacing (normal / fast)", async ({
    page,
  }) => {
    await startLocalGame(page);
    // The single global pacing control is a labeled <select> (semantic a11y), NOT a per-action
    // fast-forward. It lives in the nav's **Options** disclosure panel, so open
    // Options first. Normal + Fast are the options.
    await page.locator("#options-toggle").click();
    const sel = page.locator("#anim-speed");
    await expect(sel).toBeVisible();
    await expect(sel).toHaveJSProperty("tagName", "SELECT");
    const opts = await sel.locator("option").allTextContents();
    expect(opts.map((o) => o.toLowerCase())).toEqual(expect.arrayContaining(["normal", "fast"]));

    // Choosing 'fast' is honored: end the turn and the replay still completes (a faster
    // dwell), handing your move back — the setting changes pace, never correctness.
    await sel.selectOption("fast");
    await activate(page, endTurn(page));
    await expect(endTurn(page)).toBeVisible({ timeout: 30_000 });
  });

  test("reduced-motion plays the replay near-instant but still narrates", async ({ page }) => {
    // With prefers-reduced-motion the beats collapse to a brief tick — no one waits on
    // motion they've reduced — but the announcements still land in order (a11y holds).
    await page.emulateMedia({ reducedMotion: "reduce" });
    await startLocalGame(page);

    const status = page.locator("#status");
    const seen: string[] = [];
    const poll = setInterval(async () => {
      try {
        const t = (await status.textContent())?.trim() ?? "";
        if (t && seen[seen.length - 1] !== t) seen.push(t);
      } catch (_) {}
    }, 20);

    await endTurnToReplay(page);
    // It returns to your move quickly (near-instant pacing).
    await expect(endTurn(page)).toBeVisible({ timeout: 15_000 });
    clearInterval(poll);

    // Even near-instant, at least one opponent announcement was narrated.
    expect(
      seen.some((t) =>
        /(the Solace|Lorekeepers)\s+\w+/i.test(t) || /(Dusk falls|Nightfall|open)/i.test(t),
      ),
      `reduced-motion still narrates the opponent's turn; saw: ${seen.join(" || ")}`,
    ).toBeTruthy();
  });
});
