// Static server for the Recollect UI tests — serves the BUILT site (repo-root
// `dist/`, produced by `make site`) over http:// so the wasm client can fetch +
// instantiate its module (file:// can't).
//
// Two deploy-path quirks of the built artifact are bridged here, in the SERVING
// layer only (we never touch the built files — `make uitest` re-copies dist/ each
// run): a production CDN/nginx config would do the same.
//
//  1. The play client (`dist/client/`) is emitted by trunk with ROOT-ABSOLUTE
//     asset URLs (`/recollect-web-<hash>.js`), so it must be served as if it were
//     the origin root. We mount `dist/client/` at BOTH `/` of a dedicated client
//     base AND under `/client/` (the marketing site links to `/client/`), serving
//     its assets from the same directory either way.
//  2. The hand-written inline bootstrap in the client imports the UNHASHED
//     `./recollect-web.js`, while trunk's `filehash` renames the file to
//     `recollect-web-<hash>.js`. We alias the unhashed name to the hashed file so
//     the real app logic runs. (Flagged in the report as a build bug to fix at
//     the source; the harness must not depend on this alias once it's fixed.)
//
// No dependencies — Node's built-in http/fs only, so the quarantined tooling
// stays a single dev-dependency (Playwright).

import { createServer } from "node:http";
import { readFile, stat, readdir } from "node:fs/promises";
import { join, extname, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = fileURLToPath(new URL(".", import.meta.url));
const DIST = process.env.UITEST_DIST ?? join(HERE, "..", "..", "dist");
const PORT = Number(process.env.UITEST_PORT ?? 4417);

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".wasm": "application/wasm",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".webp": "image/webp",
  ".woff2": "font/woff2",
  ".ico": "image/x-icon",
};

// The hashed JS + wasm trunk emitted, so we can alias the unhashed names the
// hand-written inline bootstrap (and wasm-bindgen's default `init()`) ask for.
let hashed = null;
async function resolveHashed() {
  if (hashed !== null) return hashed;
  hashed = { js: "", wasm: "" };
  try {
    const files = await readdir(join(DIST, "client"));
    hashed.js = files.find((f) => /^recollect-web-[0-9a-f]+\.js$/.test(f)) ?? "";
    hashed.wasm = files.find((f) => /^recollect-web-[0-9a-f]+_bg\.wasm$/.test(f)) ?? "";
  } catch {
    /* dist/client missing — the marketing pages still serve */
  }
  return hashed;
}

// MIRROR the production server's security response headers
// (recollect-server `security_headers`) so the headless suite boots the real wasm
// client UNDER the deploy CSP: if a tightened policy ever broke the trunk-boot
// (the wasm MIME + `wasm-unsafe-eval`), the client-mount tests would fail here. The
// CSP is path-aware exactly as the server's is — the `/client/` wasm app gets
// `wasm-unsafe-eval`; the static pages stay strict `script-src 'self'`.
const CSP_SITE =
  "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; " +
  "img-src 'self' data:; font-src 'self'; connect-src 'self'; base-uri 'self'; " +
  "form-action 'self'; frame-ancestors 'none'; object-src 'none'";
const CSP_CLIENT =
  "default-src 'self'; script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'; " +
  "style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; " +
  "connect-src 'self'; worker-src 'self' blob:; child-src blob:; base-uri 'self'; " +
  "form-action 'self'; frame-ancestors 'none'; object-src 'none'";

function securityHeaders(pathname) {
  return {
    "content-security-policy": pathname.startsWith("/client/") ? CSP_CLIENT : CSP_SITE,
    "x-content-type-options": "nosniff",
    "x-frame-options": "DENY",
    "referrer-policy": "strict-origin-when-cross-origin",
    "cross-origin-opener-policy": "same-origin",
    "permissions-policy": "geolocation=(), microphone=(), camera=(), payment=(), usb=()",
    "strict-transport-security": "max-age=31536000; includeSubDomains",
  };
}

async function send(res, status, body, type, pathname = "/") {
  res.writeHead(status, {
    "content-type": type ?? "text/plain; charset=utf-8",
    "cache-control": "no-store",
    ...securityHeaders(pathname),
  });
  res.end(body);
}

async function tryServe(res, filePath, pathname) {
  try {
    const s = await stat(filePath);
    const target = s.isDirectory() ? join(filePath, "index.html") : filePath;
    const buf = await readFile(target);
    await send(res, 200, buf, MIME[extname(target)] ?? "application/octet-stream", pathname);
    return true;
  } catch {
    return false;
  }
}

const server = createServer(async (req, res) => {
  // Strip query/hash, decode, and block path traversal.
  let pathname = decodeURIComponent((req.url ?? "/").split("?")[0].split("#")[0]);
  pathname = normalize(pathname).replace(/^(\.\.[/\\])+/, "");
  if (pathname === "/") pathname = "/index.html";

  // Alias the unhashed client import + wasm-bindgen's default wasm fetch to the
  // hashed files trunk emitted (the inline bootstrap imports `./recollect-web.js`
  // and `init()` then fetches `recollect-web_bg.wasm`, both unhashed).
  const h = await resolveHashed();
  if (h.js && (pathname === "/client/recollect-web.js" || pathname === "/recollect-web.js")) {
    pathname = `/client/${h.js}`;
  } else if (h.wasm && (pathname === "/client/recollect-web_bg.wasm" || pathname === "/recollect-web_bg.wasm")) {
    pathname = `/client/${h.wasm}`;
  }
  // Trunk's own bootstrap + the modulepreload/CSS links use root-absolute
  // `/recollect-web-<hash>.{js,wasm}` and `/play-<hash>.css`; when the client is
  // reached via `/client/`, those resolve at the root — map them into dist/client/.
  if (
    /^\/recollect-web-[0-9a-f]+(_bg)?\.(js|wasm)$/.test(pathname) ||
    /^\/play-[0-9a-f]+\.css$/.test(pathname)
  ) {
    pathname = `/client${pathname}`;
  }

  const filePath = join(DIST, pathname);
  if (await tryServe(res, filePath, pathname)) return;
  await send(res, 404, `not found: ${pathname}`, undefined, pathname);
});

server.listen(PORT, "127.0.0.1", () => {
  // eslint-disable-next-line no-console
  console.log(`uitest static server: http://127.0.0.1:${PORT}  (dist=${DIST})`);
});
