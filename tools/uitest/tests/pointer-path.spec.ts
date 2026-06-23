import { test, expect } from "@playwright/test";
import { startLocalGame } from "./helpers";

// The canvas pointer-PATH regression spec: the in-browser half of the guard the
// native `recollect-web` suite owns (`shell::tests::pointer_resolves_to_the_right_tile_*`,
// `the_pointer_path_*`). It pins the PRODUCTION JS px→tile resolver (`tileAtPx` in
// index.html — the one every pointer/keyboard path funnels through) as the faithful
// twin of Rust's `ShellRegions::tile_at`, so a future regions-misroute (a tap resolving to
// the WRONG tile: an off-by-one, a dropped Y-flip, or a layout offset) fails CI here too.
//
// Why exercise the resolver directly (via the `__recollectTest` hook), not a canvas pixel
// tap? The wgpu canvas hit-test over real mouse pixels is sandbox-deferred to
// docs/manual_verification.md (a headed-GL pixel tap is flaky in CI). The MAP itself is a
// pure function of (coords, board rect, grid side) — so we test that function deterministically
// over the LIVE regions the shell is drawing, plus a synthesized 6×6 (the 2v2 board) that
// drives the identical code path.

// The forward map (tile → its drawn centre, the Y-flip): the exact inverse of `tileAtPx`,
// matching Rust's `tile_center_px`. Seat A's home row draws at the BOTTOM.
function tileCenterPx(t: number, w: number, b: { x: number; y: number; w: number; h: number }) {
  const cell = b.w / w;
  const gx = t % w;
  const gy = w - 1 - Math.floor(t / w); // Y-flip: home row at the bottom
  return { x: b.x + (gx + 0.5) * cell, y: b.y + (gy + 0.5) * cell };
}

type Regions = { board: { x: number; y: number; w: number; h: number }; board_w: number };

async function tileAt(page: import("@playwright/test").Page, R: Regions, x: number, y: number) {
  return page.evaluate(
    ([r, px, py]) => (window as any).__recollectTest.tileAtPx(r, px, py),
    [R, x, y] as const,
  );
}

test.describe("canvas pointer path → tile", () => {
  test("every tile centre resolves back to that tile on the live 5×5 board", async ({ page }) => {
    await startLocalGame(page);
    // The live regions the shell is drawing this frame (real board rect + grid side).
    const R = (await page.evaluate(() => (window as any).__recollectTest.regionsForTest())) as Regions;
    expect(R, "the shell exposes live hit-test regions").toBeTruthy();
    expect(R.board_w).toBe(5);
    const w = R.board_w;
    for (let t = 0; t < w * w; t++) {
      const c = tileCenterPx(t, w, R.board);
      const got = await tileAt(page, R, c.x, c.y);
      expect(got, `tile ${t}'s centre must resolve back to ${t} (no misroute)`).toBe(t);
    }
  });

  test("the resolver honours the board Y-flip (home row at the bottom)", async ({ page }) => {
    await startLocalGame(page);
    const R = (await page.evaluate(() => (window as any).__recollectTest.regionsForTest())) as Regions;
    const w = R.board_w,
      b = R.board,
      q = (b.w / w) * 0.5;
    // The VISUAL corners map to the flipped tile indices the player sees.
    expect(await tileAt(page, R, b.x + q, b.y + b.h - q), "bottom-left visual = tile 0 (home)").toBe(0);
    expect(await tileAt(page, R, b.x + b.w - q, b.y + b.h - q), "bottom-right = tile w-1").toBe(w - 1);
    expect(await tileAt(page, R, b.x + q, b.y + q), "top-left = tile (w-1)*w").toBe((w - 1) * w);
    expect(await tileAt(page, R, b.x + b.w - q, b.y + q), "top-right = the last tile").toBe(w * w - 1);
  });

  test("a point off the board square resolves to nothing (null)", async ({ page }) => {
    await startLocalGame(page);
    const R = (await page.evaluate(() => (window as any).__recollectTest.regionsForTest())) as Regions;
    const b = R.board;
    expect(await tileAt(page, R, b.x - 5, b.y + b.h / 2), "left of board").toBeNull();
    expect(await tileAt(page, R, b.x + b.w + 5, b.y + b.h / 2), "right of board").toBeNull();
    expect(await tileAt(page, R, b.x + b.w / 2, b.y - 5), "above board").toBeNull();
    expect(await tileAt(page, R, b.x + b.w / 2, b.y + b.h + 5), "below board").toBeNull();
  });

  test("the same resolver maps the 6×6 (2v2) board with no misroute", async ({ page }) => {
    // The resolver is pure over `{board, board_w}` — the same code the 2v2 shell feeds a
    // 6×6 region. Drive it over a synthesized 6×6 board to guard the wider grid + its flip.
    await startLocalGame(page); // any page with the hook loaded
    const R: Regions = { board: { x: 120, y: 80, w: 600, h: 600 }, board_w: 6 };
    const w = R.board_w;
    const seen = new Set<number>();
    for (let t = 0; t < w * w; t++) {
      const c = tileCenterPx(t, w, R.board);
      const got = await tileAt(page, R, c.x, c.y);
      expect(got, `6×6 tile ${t}'s centre resolves back to ${t}`).toBe(t);
      seen.add(got as number);
    }
    expect(seen.size, "all 36 tiles distinct (no collision/bunching)").toBe(36);
  });
});
