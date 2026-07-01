# Operations — local stack, deployment, teardown

`make help` lists everything. The essentials:

## Local development
- `make server` — server on :8080, in-memory (set DATABASE_URL for persistence)
- `make client` — the `recollect` CLI in online mode: creates a match, joins seat A,
  prints seat B's token to hand to an opponent. `make client-join ID=… TOKEN=…` for seat B.
- `make tui` — the same `recollect` CLI, local/offline vs the bot (hotseat and watch modes too)
- `make site` / `make site-serve` — build the static site + wasm play client into `dist/` (and serve it)
- `make uitest` — build the site, then drive it in a real browser with Playwright (UI/e2e
  over `dist/`; the quarantined Node tooling in `tools/uitest/`). `make uitest-update` refreshes
  the visual-regression baselines. Needs Node (+ a one-time `npx playwright install chromium`);
  runs headed Chromium, wrapped in `xvfb-run` on Linux. See docs/testing.md → "UI / end-to-end".
- `make test` — the fast suite under `cargo-nextest` + a separate doctest pass
  (installs `cargo-nextest` on demand; see docs/testing.md). `make doc`,
  `make probes`, `make catalog`, `make catalog-check`

## The local stacks: `make up` (full experience) vs `make dev-up` (fast loop)

Two local stacks, for two jobs:

- **`make up` — the FULL local experience** (the production mirror). The real **website + game at
  :8080** (the axum server serving the built site + wasm client from one origin) **and Grafana + the
  Recollect dashboards at :3000**, backed by on-box Postgres — the closest thing to production you can
  run on a laptop. It is the deploy stack run locally (minus the Cloudflare Tunnel), so it validates
  the real artifacts. `make down` stops it, `make nuke` deletes its volumes, `make logs` tails the server.
- **`make dev-up` — the FAST inner loop** (no site build). postgres:18.4 + grafana/otel-lgtm (Grafana
  :3000; OTLP :4317/:4318) + the API server built from the root `Dockerfile` (static musl,
  distroless/static nonroot runtime). You iterate the engine/server here and run `make server` against
  it; the site isn't built, so it stays quick. `make dev-down` stops it.

(A leaner third option, **`make deploy-local`**, is the full single-origin **site** at :8080 *without*
the observability stack — see Deployment below.) OpenTelemetry is always compiled in; the compose
`server` sets `OTEL_EXPORTER_OTLP_ENDPOINT` so it exports to LGTM — unset it (e.g. plain `make server`)
and you get JSON logs only. `make seed` mints two demo accounts and a match (tokens print ONCE);
`make db-test` runs the postgres integration suite against the running stack.

**Signals in Grafana** (:3000 → Explore): traces are the server's `#[instrument]`
command spans (Tempo); logs are the `tracing` JSON events (Loki); metrics
(Prometheus) include `recollect.commands.applied` (by `outcome=ok|reject`),
`recollect.command.duration_ms`, `recollect.matches.created`,
`recollect.ws.connections.opened`, and `recollect.ws.reconnections` (a seat
re-subscribed over a fresh socket, superseding a still-live one), plus the
§16 game-design cuts (`recollect.evolutions`, `recollect.throughline_completed`,
`recollect.match_length_turns`, and the `led_at_contraction`/`won` labels on
`recollect.matches.finished`). **The full instrumentation catalog — every metric's
name, type, labels, emission point, and meaning, the `#[instrument]` spans, all
three provisioned dashboards panel-by-panel, the OTLP export path, and the
convention for adding a metric — is `docs/observability.md`** (the living source of
truth; this section is just the quick list). The three dashboards land
pre-provisioned (read-only): **Recollect — service (RED)**, **Recollect —
game-design (§16)**, and the **host/box** view.

## Teardown, carefully
These act on the **`make up`** (full) stack; the fast loop's teardown is **`make dev-down`**.
1. `make db-backup` — pg_dump to `backups/`, timestamped. Run this first.
2. `make down` — stops the `make up` containers, KEEPS volumes. The default.
3. `make nuke` — deletes its volumes; demands you type `unwrite`.

## Deployment

### Provision from nothing → everything (the make-driven runbook)

The whole deploy in order, driven by `make`. **Validate locally first** — `make up` (play the real
site + watch the dashboards) and `make deploy-smoke` (black-box: site + game + journal, auto-teardown).
Then steps 1–7; they need a short-lived **admin AWS session** (`aws sso login`), a **Cloudflare account
with your domain's zone added**, and the **domain**. Run from the repo root. The per-key detail (SSO
setup, the Cloudflare token's exact scopes, every Pulumi config key) is in
[`deploy/README.md`](../deploy/README.md) — this is the concise path.

0. **Push** (once): `git push -u origin main` — the repo + CI workflows on GitHub.
1. **Pulumi state backend** (once): set the secrets passphrase, then create the hardened S3 state
   bucket + `pulumi login`:
   ```
   export PULUMI_CONFIG_PASSPHRASE='<a-strong-passphrase>'   # store it in your password manager
   make pulumi-state-bucket
   ```
2. **FOUNDATION** (once — the ECR repo + GitHub-OIDC CI push role). The lifecycle targets run a
   preflight that creates the `prod` stack, defaults `region`, and prompts for
   `PULUMI_CONFIG_PASSPHRASE` + `githubRepo` if unset (deploy/README.md "The make targets' preflight"):
   ```
   make foundation-install
   make foundation-preview && make foundation-up && make foundation-outputs   # prompts for githubRepo
   ```
   Pre-seed to skip the prompt:
   `(cd deploy/pulumi/foundation && pulumi config set githubRepo laconc/recollect-tcg)`
3. **Wire FOUNDATION's outputs into GitHub Actions VARIABLES** (turns CI's ECR push on):
   ```
   gh variable set AWS_ROLE_ARN --body "$(cd deploy/pulumi/foundation && pulumi stack output ciRoleArn)"
   gh variable set ECR_REPO_URL --body "$(cd deploy/pulumi/foundation && pulumi stack output repoUrl)"
   gh variable set AWS_REGION   --body "$(cd deploy/pulumi/foundation && pulumi stack output ecrRegion)"
   ```
4. **CI builds + pushes the server image** to ECR — automatic on every push to `main`
   (`.github/workflows/deploy-image.yml`), or **Actions → deploy-image → Run workflow**. Note the
   `sha-<commit>` tag PLATFORM deploys.
5. **PLATFORM** (per release — the EC2 box that PULLS the image + Cloudflare). `make deploy-up` runs
   the preflight (creates the stack, defaults `region`, prompts for `PULUMI_CONFIG_PASSPHRASE`,
   `CLOUDFLARE_API_TOKEN`, and any required input you didn't pre-seed below):
   ```
   export CLOUDFLARE_API_TOKEN='<scoped token: Tunnel+Access+Pages+DNS edit — see deploy/README.md>'
   make deploy-install
   (cd deploy/pulumi/platform \
      && pulumi config set domain recollect-tcg.com \
      && pulumi config set repoUrl https://github.com/laconc/recollect-tcg.git \
      && pulumi config set gitRef <SHA> \
      && pulumi config set serverImage "$(cd ../foundation && pulumi stack output repoUrl):sha-<SHA>" \
      && pulumi config set cloudflareAccountId <CF_ACCOUNT_ID> \
      && pulumi config set cloudflareZoneId <CF_ZONE_ID> \
      && pulumi config set maintainerEmail you@example.com)
   make deploy-preview && make deploy-up && make deploy-outputs
   ```
6. **Wire the static-site Pages deploy** (GitHub SECRETS + a var) so `site-deploy.yml` uploads `dist/`:
   ```
   gh secret   set CLOUDFLARE_API_TOKEN  --body '<a Pages:Edit token>'
   gh secret   set CLOUDFLARE_ACCOUNT_ID --body '<CF account id>'
   gh variable set CF_PAGES_PROJECT      --body "$(cd deploy/pulumi/platform && pulumi stack output pagesProjectName)"
   ```
7. **Verify:** `https://recollect-tcg.com` (site + game) and `https://grafana.recollect-tcg.com`
   (Grafana behind Cloudflare Access — your `maintainerEmail` + a one-time PIN), then walk
   `docs/manual_verification.md`.

**Redeploy:** push to `main` → CI pushes a new image → bump `gitRef`+`serverImage` + `make deploy-up`
(or in place: `make deploy-ssm` → `sudo recollect-update <SHA>`). **Teardown:** `make deploy-destroy`
(PLATFORM only; FOUNDATION stays up across releases).

### Launch host — EC2 + Cloudflare Tunnel (the lean §10.1 target)
The playtest deploy is **two Pulumi projects** under `deploy/pulumi/` + a deploy compose under
`deploy/compose/`:
- **FOUNDATION** (`deploy/pulumi/foundation/`, run **once** with admin SSO) — the account-level
  scaffolding: an **ECR** repo for the server image (scan-on-push, immutable tags, lifecycle policy),
  the **GitHub OIDC** provider, and a **tightly-scoped CI role** (ECR-push only, trusted to this
  repo's `main`). Outputs `repoUrl` + `ciRoleArn`.
- **PLATFORM** (`deploy/pulumi/platform/`, run **per release**) — one free-tier EC2 box in
  `us-east-2` running `recollect-server` (which also **serves the static site + wasm client** from
  `STATIC_DIR`), on-box Postgres, `cloudflared` (a named Cloudflare Tunnel — no inbound ports),
  **and the self-hosted §11 observability stack** (`grafana/otel-lgtm` + a `node-exporter`). The box
  **PULLS** the server image from FOUNDATION's ECR (instance role: ECR read-only) — **production no
  longer builds Rust on the 1 GB box**; the site (wasm) is the only on-box build.

**The flow:** FOUNDATION (once) → **CI** builds + pushes the server image to ECR on every push to
`main` (GitHub→AWS **OIDC**, no stored keys — `.github/workflows/deploy-image.yml`) → PLATFORM
(`pulumi up`, the box pulls the pinned image). Cloudflare fronts `recollect-tcg.com` for DNS + edge
TLS; the site and the `wss://recollect-tcg.com/matches/{id}/ws` socket are **same-origin**, and
Grafana is at `grafana.recollect-tcg.com` **behind a Cloudflare Access gate** (maintainer-email
allow-list). CloudWatch out-of-band alarms → an SNS email topic watch box health (status checks, CPU,
mem/swap/disk). Full step-by-step (the **FOUNDATION** + **PLATFORM** runbooks), the **access runbook**
(how to reach Grafana / the box / CloudWatch alarms), and **least-privilege credential creation**
(AWS SSO short-lived admin for both stages — the **default** profile, so plain `aws sso login` with no
`--profile`/`AWS_PROFILE` and the profile's region, us-east-2; the scoped Cloudflare API token + its
exact Tunnel/Access/DNS scopes) live in **`deploy/README.md`**. Both stacks tag **every AWS resource**
via the provider's `defaultTags` (`Project`/`Environment`/`ManagedBy`/`Stack`/`Repository` + a
per-resource `Name`); the `environment` config (default `production`) sets the `Environment` tag. Make targets:
- `make foundation-typecheck` / `make foundation-preview` / `make foundation-up` /
  `make foundation-outputs` / `make foundation-destroy` — the run-once FOUNDATION stack
  (`foundation-outputs` prints `repoUrl` + `ciRoleArn` to wire into GitHub Actions variables).
- `make deploy-typecheck` / `make deploy-preview` — gate the PLATFORM plan (no cloud calls / a dry run)
- `make deploy-up` / `make deploy-outputs` / `make deploy-ssm` / `make deploy-destroy` — live infra
  (`deploy-outputs` prints the `grafanaUrl`, `alarmTopicArn`, `ssmSession`, …)
- `make deploy-local` — run the single-origin stack locally, the **LEAN** variant (no tunnel,
  observability scaled to zero) and play the real website end-to-end at `http://localhost:8080`;
  `make deploy-local-down` tears it down. For the site **with** observability use **`make up`** (the
  full mirror); for the fast db+Grafana+API loop, **`make dev-up`**. Local/smoke layer the **BUILD
  overlay** (`docker-compose.build.yml`) to compile the server image locally — they need no ECR.
- `make deploy-smoke` — **smoke-test the built deploy artifact from the OUTSIDE before a launch**
  (`deploy/smoke.sh`). It builds the deploy images, brings up the local stack, polls `/healthz`,
  then asserts the artifact as a black box: the **website** (`GET /` title+nav, the wasm play client
  under `/client/` and its JS/wasm assets actually serve — the trunk-boot class — plus `/healthz`),
  the **game** (a real PvP match driven over ws by two headless `recollect online --json` clients —
  moves apply, the match advances, and **redaction holds**: a client never receives the opponent's
  hand), and the **journal** (a `journal_events` row in the on-box Postgres — the Postgres-authoritative
  path worked in the image). It **always tears down** (a trap, even on failure/interrupt), dumps the
  server logs on failure, and is idempotent + re-runnable. Needs Docker + `jq`/`python3` (it builds
  the `recollect` CLI itself, to use as the external black-box client).

Server env (the deploy wires all of these): `DATABASE_URL` (on-box Postgres ⇒ authoritative),
`BIND_ADDR` (default `0.0.0.0:8080`), `STATIC_DIR` (set ⇒ serve the built site from this origin;
unset ⇒ API only, the dev/test default), `OTEL_EXPORTER_OTLP_ENDPOINT` (on the deploy box, defaults to
the on-box `lgtm` at `http://lgtm:4317`; set `otelEndpoint` to ship off-box; unset elsewhere ⇒ JSON
logs only).

### Scale — Kubernetes (the §10.2 target, later)
Helm chart at `deploy/helm/recollect`. Hard rules the chart enforces: `image.tag`
is required (no `:latest`), credentials only via `existingSecret` (key
`DATABASE_URL`), pods run nonroot/read-only/no-caps. `make helm-lint` /
`make helm-template` (helm on dev machines or CI).

## Persistence & signals
Postgres is **authoritative** when `DATABASE_URL` is set: each command is appended
(durable) before the ack via `apply_journaled`, and `resume_async` rebuilds a match
from the journal. No `DATABASE_URL` ⇒ the in-memory engine runs and rows are skipped
(graceful degrade). OpenTelemetry is **always compiled in** (there is no `otel` cargo
feature); the three signals export only when `OTEL_EXPORTER_OTLP_ENDPOINT` is set, so
`make server` stays log-only.
