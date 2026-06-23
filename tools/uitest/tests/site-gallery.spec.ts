import { test, expect } from "@playwright/test";
import { join } from "node:path";

// ─────────────────────────────────────────────────────────────────────────────
// WEBSITE GALLERY (stills) — a committed media record of the STATIC marketing site
// (the pages in site/ → dist/: index, rules, cards, lore, guide, play), captured at
// BOTH desktop and phone widths into docs/gallery/web/ as `site-<page>-<width>.png`,
// matching the canvas gallery's naming + PNG format (they share the WEB-register
// gallery dir docs/gallery/web/, parallel to the terminal register's docs/gallery/tui/).
//
// Unlike the canvas gallery (a wgpu surface that presents all-black headless, so its
// committed stills come from the deterministic CPU rasterizer `gen_gallery.sh`), these
// pages are plain HTML/CSS and render FAITHFULLY HEADLESS — so Playwright captures them
// directly, reproducibly, in the uitest harness. They're DECOUPLED from `make uitest`
// (gated behind UITEST_SITE_GALLERY, like the canvas gallery is behind UITEST_GALLERY),
// so the default assertion run never rewrites docs/gallery/web/. Refresh with:
//
//   make site && UITEST_SITE_GALLERY=1 npx playwright test site-gallery.spec.ts --project=desktop
//
// (One project is enough — the spec sets its own viewport per width, so the device
// project is irrelevant; --project=desktop just picks a single worker lane.)
// ─────────────────────────────────────────────────────────────────────────────

// docs/gallery/web/ at the repo root (this file lives at tools/uitest/tests/, transpiled
// to CommonJS by Playwright — so `__dirname` resolves here). The web-register gallery dir
// holds ONLY generated files; gen_gallery.sh writes the canvas stills + clips alongside.
const GALLERY = join(__dirname, "..", "..", "..", "docs", "gallery", "web");

// page slug → URL path (the dist/ root the marketing site serves from).
const PAGES: ReadonlyArray<readonly [string, string]> = [
  ["index", "/index.html"],
  ["rules", "/rules.html"],
  ["guide", "/guide.html"],
  ["lore", "/lore.html"],
  ["cards", "/cards.html"],
  ["play", "/play.html"],
];

// The two widths the canvas gallery uses (desktop 1280, phone 412); a tall viewport so
// a full-page shot needs little stitching, and `fullPage` captures the whole document.
const WIDTHS: ReadonlyArray<readonly [string, number, number]> = [
  ["desktop", 1280, 900],
  ["phone", 412, 915],
];

// Run only when explicitly capturing (writes committed files) — never in the default
// assertion suite. Mirrors the canvas gallery's UITEST_GALLERY decoupling.
const capturing = !!process.env.UITEST_SITE_GALLERY;

test.describe("site gallery — the static marketing pages — stills", () => {
  test.skip(!capturing, "set UITEST_SITE_GALLERY=1 to (re)capture the committed site stills");

  for (const [slug, path] of PAGES) {
    for (const [width, vw, vh] of WIDTHS) {
      test(`${slug} @ ${width}`, async ({ page }, testInfo) => {
        await page.setViewportSize({ width: vw, height: vh });
        await page.goto(path);
        // Let the web fonts + CSS settle so the captured type/colour is the real
        // rendered surface (the cards page also fetches its catalog data).
        await page.waitForLoadState("networkidle");
        // A beat for late layout (font swap, the cards grid) before the shot.
        await page.waitForTimeout(300);
        const file = join(GALLERY, `site-${slug}-${width}.png`);
        await page.screenshot({ path: file, fullPage: true });
        await testInfo.attach(`site-${slug}-${width}.png`, {
          path: file,
          contentType: "image/png",
        });
        // The committed file exists + is non-trivial (a real render, not a blank frame).
        expect(testInfo.attachments.length).toBeGreaterThan(0);
      });
    }
  }
});
