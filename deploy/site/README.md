# Deploying the static WEBSITE — Pulumi (Pages project + DNS) + GitHub CI (the upload)

This is the deploy path for the **static website** (`site/` → `dist/`): the marketing landing page,
the rules-in-brief, the browsable **card catalog**, the **lore**, the player **guide**, and the
contact/feedback pages. It is built by `make site` and published to **Cloudflare Pages** by CI on
every push to `main` that touches the site sources.

The Cloudflare resources are **IaC in the PLATFORM Pulumi project** (`deploy/pulumi/platform/`,
alongside the game-server tunnel) — the Pages **project**, its **custom-domain bindings** (apex +
`www`), and the apex/`www` **DNS records**. The only by-hand steps left are the two a token can't
mint for itself: a scoped **Pages:Edit API token** and the **GitHub secrets/variable** CI deploys
with ([§3](#3-the-residual-manual-steps-what-pulumi-cant-do)).

It is **deliberately separate from the game-server deploy** in
[`deploy/README.md`](../README.md). That deploy is the Pulumi **FOUNDATION + PLATFORM** stack — an
EC2 box behind a **Cloudflare Tunnel** that serves the WebSocket + REST API (and the live origin the
wasm play client dials). This one is a **pure static bundle** with no server, no state, and no
release cadence tied to the engine — so it belongs on **Cloudflare Pages**, the natural fit for a
static site (global edge CDN, atomic deploys, instant rollback, free tier). The two share **one
Cloudflare zone**: the **website on the apex/`www`**, the **game on `play.<domain>`** (see
[Custom domain + DNS](#2-custom-domain--dns-all-iac--how-the-two-deploys-share-one-zone)).

> **Why Pages, not the origin box.** The tech design's §10.1 single-origin note ("the axum server
> serves the static site itself") is the *minimal* launch posture — one box serving everything. The
> design also names Cloudflare Pages as the alternative for the static site, and it is the better one
> here: the marketing/rules/lore pages change far more often than the engine, want a global CDN, and
> should never be coupled to a server redeploy. Splitting them means a copy edit ships in ~30s via
> Pages without touching the game server, and the box carries one fewer concern. The wasm **play
> client** is also served by the **game server** (`play.<domain>`), where it connects same-origin to
> the WebSocket — so the API still lives only on the box.

---

## TL;DR — the deploy path

```
   push to main (paths: site/**, tools/gen_cards_page.py, the catalog, the cards source, recollect-web/**)
       │
       ▼
   CI (.github/workflows/site-deploy.yml)  ──▶  make site  ──▶  dist/
       │                                          (gen cards page + copy site/ + trunk-bundle the wasm client)
       ▼
   wrangler pages deploy dist/ --project-name=<CF_PAGES_PROJECT>   (DIRECT upload, authed by a Cloudflare API token)
       │
       ▼
   Cloudflare Pages  ──▶  served at the apex / www  (the Pages project + domains + DNS are Pulumi IaC)
```

**Pulumi owns the Cloudflare setup; CI owns the build + upload.** `pulumi up` (the PLATFORM stack)
creates the Pages project, its apex/`www` custom-domain bindings, and the apex/`www` DNS records.
CI then **builds** `dist/` on a real runner (with cargo/trunk caching) and **direct-uploads** it;
nothing is built on Cloudflare's side. The job **skips** on any clone where the Pages project
variable isn't set, so a public fork never fails red.

---

## 1. The Pulumi IaC — the Pages project, domains, and DNS

Everything Cloudflare-side is **declarative in `deploy/pulumi/platform/index.ts`** (the same stack
that runs the game-server tunnel), created on `make platform-up`:

| Resource (in `platform/index.ts`) | What it is |
|---|---|
| `cloudflare.PagesProject` **`recollect-site`** | The Pages project, **`productionBranch: "main"`**, **direct-upload** — it has **no `source` block, so no connected Git repo** (the intended state: CI uploads `dist/`, Cloudflare just receives it). Its name is the `pagesProjectName` config (default `recollect-site`). |
| `cloudflare.PagesDomain` **apex + www** | The two custom-domain **bindings** that tell Pages to serve `<domain>` and `www.<domain>` and provision their edge certs. (A binding is separate from a DNS record — Cloudflare needs both; same-zone domains validate automatically.) |
| `cloudflare.DnsRecord` **apex + www** | Two proxied `CNAME`s → the project's `*.pages.dev` (`pagesProject.subdomain`). `proxied: true` for edge TLS + CDN; at the apex this is CNAME flattening, handled transparently. |

Two **config** knobs (both have generic, non-deployment-unique defaults — override freely):

```bash
pulumi config set pagesProjectName recollect-site   # the Pages project name (== CI's CF_PAGES_PROJECT)
pulumi config set gameSubdomain    play             # the game server's sub-route ⇒ play.<domain>
```

The project name is surfaced as the **`pagesProjectName`** stack output, so CI's `CF_PAGES_PROJECT`
variable can name the **same** project Pulumi created:

```bash
gh variable set CF_PAGES_PROJECT \
  --body "$(cd deploy/pulumi/platform && pulumi stack output pagesProjectName)"
```

> **Direct-upload, never connect-repo.** Keeping **one build pipeline** (ours) means the site CI
> produces byte-identical output to a local `make site`, and the wasm client is built with the repo's
> pinned toolchain (Rust 1.96.0 + trunk) rather than a re-created Cloudflare build config. Omitting
> the `source` block is exactly what makes the Pages project a direct-upload one. Do **not** also
> connect the GitHub repo, or you'd get two competing deploys. (A private repo is fine: with
> direct-upload Cloudflare never reads the repo — CI pushes the built `dist/`.)

---

## 2. Custom domain + DNS (all IaC) — how the two deploys share one zone

Both the website and the game server live under **one Cloudflare zone** (your domain — supplied as
the PLATFORM `domain` config, never committed). Pulumi creates every record below; the split:

| Hostname | Served by | DNS record (in the zone) |
|---|---|---|
| **apex** (`<domain>`) and **`www`** | **Cloudflare Pages** (this deploy) | the two proxied `CNAME`s → `<project>.pages.dev`, **created by PLATFORM** ([§1](#1-the-pulumi-iac--the-pages-project-domains-and-dns)), backed by the Pages custom-domain bindings. |
| **`play.<domain>`** (the game) | the **EC2 box via the Cloudflare Tunnel** (PLATFORM) | a **proxied `CNAME`** to the tunnel — also PLATFORM (`gameSubdomain`, default `play`). |
| **`grafana.<domain>`** (ops) | the box's Grafana, behind **Cloudflare Access** | a proxied `CNAME` to the tunnel — also PLATFORM. |

So the **website is the apex/`www`** and the **game server is a sub-route** (`play.…`). They never
collide — Pulumi owns all of it: Pages on the apex/`www`, the tunnel on `play.`/`grafana.`. The
marketing site's **"Play" button** points at `https://play.<domain>` (the game origin), and the wasm
client served there connects to the game server's WebSocket **same-origin**.

> **If the game server is NOT yet deployed:** the website still stands up fine — the apex/`www` Pages
> records + bindings don't depend on the box. (`play.<domain>` and `grafana.<domain>` simply won't
> resolve until you've stood up the rest of PLATFORM.) The "Play" link points at `play.<domain>`,
> which goes live when the box does.

---

## 3. The residual manual steps (what Pulumi can't do)

A token can't mint itself, and Pulumi doesn't manage your GitHub repo's secrets — so **two** by-hand
steps remain (the project, the domains, and the DNS are all IaC above):

**A. Mint a scoped Cloudflare API token** (Pages-only, revocable — **never the Global API Key**).
Dashboard → **My Profile → API Tokens → Create Token → Create Custom Token**:
- **Name:** e.g. `recollect-pages-deploy`.
- **Permissions:** add exactly **Account · Cloudflare Pages · Edit** (the only permission a Pages
  direct-upload needs).
- **Account Resources:** *Include →* **your account** (the one that owns the zone).
- Optionally set a **TTL** so it self-rotates. **Create**, copy the value **once** (shown only at
  creation). Revoke anytime from this page.

**B. Set the GitHub config** (Settings → Secrets and variables → Actions, or the `gh` CLI). Three
values — the **variable is the ON switch** (the job's `if:` reads it; a job-level `if:` can read
`vars` but not `secrets`):

```bash
gh variable set CF_PAGES_PROJECT      --body "$(cd deploy/pulumi/platform && pulumi stack output pagesProjectName)"
gh secret   set CLOUDFLARE_API_TOKEN  --body "<the scoped Pages token from step A>"
gh secret   set CLOUDFLARE_ACCOUNT_ID --body "<your Cloudflare account id>"
```

| Kind | Name | Required? | What it is |
|---|---|---|---|
| **variable** | `CF_PAGES_PROJECT` | **yes — the ON switch** | The Pages **project name** Pulumi created (the `pagesProjectName` stack output). The job's `if:` gate reads this, so **setting it is what turns the deploy on**. |
| **secret** | `CLOUDFLARE_API_TOKEN` | **yes** | The scoped **Pages:Edit** token from step A. The deploy step's only credential. |
| **secret** | `CLOUDFLARE_ACCOUNT_ID` | **yes** | Your Cloudflare **account id** (the account that owns the zone + the Pages project). Find it on any zone's **Overview → right rail → Account ID**. |

Then **trigger the first deploy:** push a change under `site/**`, or **Actions → site-deploy → Run
workflow**. Confirm the run is green and the site loads at your apex/`www`. Everything after is
automatic on push to `main`.

> **Account ID — also Pulumi's.** The same account id is already the PLATFORM `cloudflareAccountId`
> config. It is non-secret, but the workflow consumes it as a secret for symmetry with the token.

---

## 4. The GitHub Actions workflow — `.github/workflows/site-deploy.yml`

Already scaffolded. On a **push to `main`** that touches the site sources (or a manual **Actions →
site-deploy → Run workflow**), it:

1. **Builds** `make site` → `dist/` (after installing the pinned Rust toolchain + the `wasm32`
   target + `trunk`, with a cargo/trunk cache; trunk bundles the wasm play client into `dist/client/`).
2. **Deploys** with `cloudflare/wrangler-action` running
   `wrangler pages deploy dist/ --project-name=<CF_PAGES_PROJECT> --branch=main` — a **direct upload**.

It is wired to the **same public-repo-safe config-hygiene pattern** as `deploy-image.yml`: every
deployment-unique value is a **GitHub secret/variable**, never committed, and the job **SKIPS** unless
configured (so a public clone never fails red).

> **Trigger paths.** The workflow runs on changes to `site/**`, the cards/lore page generators
> (`tools/gen_cards_page.py`, `tools/gen_lore_page.py`, `tools/lore_extract.py`), the generated
> catalog (`app/crates/recollect-core/data/catalog.json`), the **card source** upstream of it
> (`app/crates/recollect-core/data/cards.toml`), the wasm client (`app/crates/recollect-web/**`), and
> the workflow file itself. If you add a new site generator or a new content source to `make site`,
> **add its path here** so a content change actually triggers a publish.

### What `make site` produces (`dist/`)

The workflow publishes exactly what `make site` writes to **`dist/`** at the repo root — the live tree:

```
dist/
├── index.html        the landing page          ├── lore.html        the lore (hand-written page)
├── rules.html        rules in brief            ├── guide.html       the player guide
├── cards.html        the card catalog (GENERATED from catalog.json by tools/gen_cards_page.py)
├── cards.js          the catalog filter/search ├── contact.html  ├── feedback.html  ├── play.html
├── css/  (brand.css)        fonts/  (EBGaramond.ttf, OFL.txt)
└── client/           the wasm play client (bundled by trunk, --public-url /client/):
    ├── index.html  ├── recollect-web.js  ├── recollect-web_bg.wasm  └── play.css
```

`dist/` is **git-ignored** (built output, never committed). `cards.html` is **generated** from the
catalog, so a catalog change (`make catalog`) reflows the cards page on the next `make site`. The
`client/` subdir appears only when **trunk** is installed (CI installs it); without trunk, `make site`
assembles the static pages and skips the client.

---

## Config hygiene — nothing deployment-unique in git

Same rule as the rest of `deploy/`: **the repo stays generic.** The domain, the account id, the
project name, and the API token are **Pulumi config (gitignored per-stack) + GitHub secrets/variables
+ Cloudflare dashboard state only** — never committed. The workflow reads its three values at run time
and **skips** when they're absent, so a public clone carries zero specifics and never fails red. The
built `dist/` is git-ignored.

| Value | Lives in | Created by | Rotate by |
|---|---|---|---|
| Pages **project** + apex/`www` **bindings** + **DNS** | Pulumi state (PLATFORM) | **Pulumi IaC** (`platform/index.ts`) | edit `index.ts` / the `pagesProjectName`,`gameSubdomain` config + `make platform-up` |
| `CLOUDFLARE_API_TOKEN` (Pages:Edit) | GitHub **secret** | you ([§3 step A](#3-the-residual-manual-steps-what-pulumi-cant-do)) | roll it on the API Tokens page; re-set the secret |
| `CLOUDFLARE_ACCOUNT_ID` | GitHub **secret** | you ([§3 step B](#3-the-residual-manual-steps-what-pulumi-cant-do)) | n/a (stable) — update the secret if you change accounts |
| `CF_PAGES_PROJECT` | GitHub **variable** (non-secret) | you, from the `pagesProjectName` output ([§3](#3-the-residual-manual-steps-what-pulumi-cant-do)) | change the `pagesProjectName` config + the variable together |

See also: [`deploy/README.md`](../README.md) (the game-server FOUNDATION/PLATFORM deploy + the
Cloudflare token recipe for the **Tunnel/Access/DNS/Pages** provider),
`docs/decisions/playtest_launch_plan.md` (§0 branding/domain, §4 Cloudflare-in-front), and the root
`make site` target.
