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

## The compose stack (`make up`)
postgres:18.4 + grafana/otel-lgtm (Grafana :3000; logs/metrics/traces over
OTLP :4317/:4318) + the server built from the root `Dockerfile` (static musl,
distroless/static nonroot runtime). OpenTelemetry is always compiled in; the
compose `server` sets `OTEL_EXPORTER_OTLP_ENDPOINT`, so it exports to the LGTM
stack — unset it (e.g. `make server`) and you get JSON logs only. `make seed` mints two demo
accounts and a match — tokens print ONCE. `make db-test` runs the postgres
integration suite against the stack.

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
1. `make db-backup` — pg_dump to `backups/`, timestamped. Run this first.
2. `make down` — stops containers, KEEPS volumes. The default.
3. `make nuke` — deletes volumes; demands you type `unwrite`.

## Deployment

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
- `make deploy-local` — run the **same single-origin stack locally** (no tunnel; the observability
  stack is scaled to zero — dev observability is `make up`) and play the real website end-to-end at
  `http://localhost:8080`; `make deploy-local-down` tears it down. Local/smoke layer the **BUILD
  overlay** (`docker-compose.build.yml`) to compile the server image locally — they need no ECR.
- `make deploy-smoke` — **smoke-test the built deploy artifact from the OUTSIDE before a launch**
  (`deploy/smoke.sh`). It builds the deploy images, brings up the local stack, polls `/healthz`,
  then asserts the artifact as a black box: the **website** (`GET /` title+nav, the wasm play client
  under `/client/` and its JS/wasm assets actually serve — the trunk-boot class — plus `/healthz`),
  the **game** (a real PvP match driven over ws by two headless `recollect online --json` clients —
  moves apply, the telling advances, and **redaction holds**: a client never receives the opponent's
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
