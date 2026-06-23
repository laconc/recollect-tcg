import { test, expect } from "@playwright/test";

// #105 — the website/server security headers, asserted end-to-end against the served
// artifact. The static server (serve.mjs) MIRRORS the production server's
// `security_headers` middleware, so this guards the same policy the deploy ships:
//   • the security header set on every response (nosniff / frame-deny / referrer /
//     COOP / Permissions-Policy / HSTS);
//   • a PATH-AWARE Content-Security-Policy — strict `script-src 'self'` for the static
//     site (no inline-script XSS vector), and the wasm-permitting
//     `'wasm-unsafe-eval'` (NOT the broad `unsafe-eval`) for the `/client/` app, so the
//     trunk-boot keeps working under the CSP.
// The companion is the Rust test `the_deploy_host_sets_security_headers_with_a_path_aware_csp`
// (the actual production middleware); the OTHER uitest specs prove the client BOOTS
// under this CSP (they mount the canvas while serve.mjs sends these headers).

test.describe("security headers", () => {
  test.skip(
    () => test.info().project.name !== "desktop",
    "the headers are origin-wide; assert once on the desktop project",
  );

  test("the static site carries the full header set and a STRICT CSP", async ({ request }) => {
    const res = await request.get("/index.html");
    const h = res.headers();
    expect(h["x-content-type-options"]).toBe("nosniff");
    expect(h["x-frame-options"]).toBe("DENY");
    expect(h["referrer-policy"]).toBe("strict-origin-when-cross-origin");
    expect(h["cross-origin-opener-policy"]).toBe("same-origin");
    expect(h["permissions-policy"]).toContain("camera=()");
    expect(h["strict-transport-security"]).toContain("max-age=31536000");

    const csp = h["content-security-policy"] ?? "";
    expect(csp, "site CSP present").toContain("default-src 'self'");
    expect(csp, "site script-src is locked to self").toMatch(/script-src 'self'[;\s]/);
    expect(csp, "no clickjacking embed").toContain("frame-ancestors 'none'");
    expect(csp, "no plugins").toContain("object-src 'none'");
    expect(csp, "the static site must NOT permit any eval").not.toContain("unsafe-eval");
    expect(csp, "the static site must NOT permit inline script").not.toContain(
      "script-src 'self' 'unsafe-inline'",
    );
  });

  test("the wasm client CSP permits wasm-unsafe-eval but not the broad unsafe-eval", async ({
    request,
  }) => {
    const res = await request.get("/client/");
    const csp = res.headers()["content-security-policy"] ?? "";
    expect(csp, "client CSP present").toContain("default-src 'self'");
    expect(csp, "wasm-bindgen init needs wasm-unsafe-eval").toContain("'wasm-unsafe-eval'");
    // The narrow grant only — the space-delimited broad `unsafe-eval` must be absent.
    expect(csp, "never the broad unsafe-eval").not.toContain(" 'unsafe-eval'");
    expect(csp, "wgpu's worker").toContain("worker-src 'self' blob:");
  });

  test("the wasm module is served as application/wasm (the trunk-boot MIME)", async ({
    request,
  }) => {
    // The hand-written bootstrap fetches the unhashed name; serve.mjs aliases it to the
    // hashed file (and a CDN/origin does the real thing). Either way the MIME must be
    // application/wasm or instantiateStreaming rejects it.
    const res = await request.get("/client/recollect-web_bg.wasm");
    expect(res.ok()).toBeTruthy();
    expect(res.headers()["content-type"]).toContain("application/wasm");
  });

  test("the cards page has no inline script (strict script-src holds)", async ({ page }) => {
    // The card filter was externalised to cards.js so `script-src 'self'` works with no
    // inline allowance. Assert the served markup carries an external script and no
    // inline <script> body, and that the filter still runs under the CSP (the count
    // updates as the search input filters).
    await page.goto("/cards.html");
    await expect(page.locator('script[src="cards.js"]')).toHaveCount(1);
    const inlineScriptBodies = await page.$$eval("script:not([src])", (els) =>
      els.map((e) => (e.textContent ?? "").trim()).filter((t) => t.length > 0),
    );
    expect(inlineScriptBodies, "no inline <script> body on the cards page").toEqual([]);
    // The external filter executes under the strict CSP: typing narrows the count.
    const count = page.locator("#card-count");
    const before = await count.textContent();
    await page.locator("#card-search").fill("zzzznotacard");
    await expect(count).not.toHaveText(before ?? "");
    await expect(count).toHaveText(/^0 of \d+$/);
  });
});
