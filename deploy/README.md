# Deploying Recollect — the lean launch host

This is the **declarative deploy** for the Recollect playtest: one free-tier AWS EC2 box in
`us-east-2` running a Docker Compose stack, fronted by Cloudflare (DNS + a named Tunnel + edge
TLS). It implements tech-design **§10.1** ("Launch — lean, EC2 ↔ Cloudflare"), with the
maintainer's playtest override: **Postgres runs on-box in compose, not RDS**.

`pulumi up` is the source of truth — the box, the security group, the Cloudflare tunnel + DNS,
and the AWS budget guardrails are all reproducible from code. No click-ops.

> **This README is the GAME-SERVER deploy** (the EC2 box + Tunnel + ECR pull). The **static
> WEBSITE** (`site/` → `dist/`: the marketing/rules/cards/lore/guide pages) has its **own** path —
> the **Cloudflare Pages** project + apex/`www` domains + DNS are **IaC in this same PLATFORM stack**,
> and GitHub CI direct-uploads `dist/` to it — documented in [`deploy/site/README.md`](site/README.md).
> Same Cloudflare zone: the **website on the apex/`www`**, the **game on `play.<domain>`** (a sub-route).

## Two Pulumi stages: FOUNDATION (once) + PLATFORM (per release)

The deploy is **two separate Pulumi projects**, each with its own state and stack, so a routine
per-release `pulumi up` can never touch the account-level scaffolding:

| Stage | Dir | Run | With | Creates | Outputs |
|---|---|---|---|---|---|
| **FOUNDATION** | `deploy/pulumi/foundation/` | **ONCE**, seldom again | short-lived **admin** (AWS SSO) | the **ECR** repo for the server image (scan-on-push, immutable tags, a lifecycle policy) · the **GitHub OIDC** provider · a **tightly-scoped CI role** (ECR-push only, trusted to this repo's `main`) | `repoUrl` (ECR), `ciRoleArn` |
| **PLATFORM** | `deploy/pulumi/platform/` | **PER RELEASE** | short-lived **admin** (AWS SSO) | the **EC2** box + egress-only SG + durable EBS + the instance role (now **ECR read-only**) · the **Cloudflare** tunnel/DNS/Access (game on `play.<domain>`) · the static-website **Cloudflare Pages** project + apex/`www` domains + DNS · budgets + CloudWatch alarms. The box **PULLS** the server image from ECR (it no longer builds Rust on-box). | `site`, `gameUrl`, `pagesProjectName`, `grafanaUrl`, `instanceId`, … |

**The flow, end to end:**

```
   FOUNDATION  ──(once)──▶  ECR repo + GitHub-OIDC CI role
       │
       ▼
   CI (.github/workflows/deploy-image.yml)  ──(every push to main)──▶
       assume the CI role via OIDC (no stored keys) ▸ docker build ▸ push  <ecr>:sha-<commit> + :latest
       │
       ▼
   PLATFORM  ──(per release: `pulumi up`)──▶  EC2 box PULLS <ecr>:sha-<commit>  ▸  compose up
```

So **CI** owns the build (on a real runner, with caching), **the box** only pulls — and nothing in
the loop stores a long-lived AWS key. The two runbooks below are the **Foundation** section (run
once) and the **Platform** section (run per release); read them in that order.

> **Why split the projects.** FOUNDATION is created once and almost never changes; PLATFORM churns
> every release. Separate states mean PLATFORM's frequent `pulumi up` has no way to replace the ECR
> repo or widen the CI trust, and each stack's blast radius is its own. It is also the security win:
> the box's instance role is **ECR read-only** (it can only pull), and the **only** thing that can
> push is main-branch CI assuming the FOUNDATION role via OIDC.

---

## FOUNDATION — run once (the ECR repo + the GitHub-OIDC CI push role)

Run this **once**, with a **short-lived admin** AWS session (SSO). It creates the ECR repo the box
pulls from, the GitHub Actions OIDC provider, and the tightly-scoped role CI assumes to push — then
you wire its two outputs into GitHub and into PLATFORM. After this, **CI pushes images with no
stored AWS keys**, and you rarely touch FOUNDATION again.

### 1. Short-lived admin via AWS IAM Identity Center (SSO)

Best practice is **no long-lived keys** — an SSO session that auto-expires. One-time profile setup,
then a login that mints a short session for this terminal. The maintainer's SSO is the **default
profile**, so the primary path needs **no `--profile` and no `AWS_PROFILE`** — `aws sso login` logs
the default profile in and both Pulumi stacks read it automatically:

```bash
# One-time: configure your SSO as the DEFAULT profile against your IAM Identity Center start URL +
# region (the maintainer's default region is us-east-2).
aws configure sso                       # set it as the default profile (just press Enter at the profile-name prompt)
# Each working session (the creds auto-expire — nothing static to leak):
aws sso login                           # the DEFAULT profile — no --profile, no AWS_PROFILE; Pulumi uses it automatically
aws sts get-caller-identity             # sanity: you are the admin identity, in the right account
```

> **Region.** Set each stack's `region` config to match your SSO profile's region (the maintainer's
> is **us-east-2**) so the box and the ECR repo land where you expect; keep FOUNDATION and PLATFORM on
> the **same** region so the box pulls the image with no cross-region charge.

> **Named-profile alternative.** If your SSO is a *named* profile instead of the default, log it in
> and point your shell at it for both stacks:
> ```bash
> aws sso login --profile <name>
> export AWS_PROFILE=<name>             # both Pulumi stacks then read AWS creds from this profile
> ```

The SSO **permission set** needs admin-enough rights to create ECR + IAM (incl. the **OIDC
provider** + a role with an inline policy). For a solo maintainer account, `PowerUserAccess` **plus**
an IAM-write allowance (`iam:CreateRole`/`CreateOpenIDConnectProvider`/`PutRolePolicy`/`AttachRolePolicy`/
`PassRole`, scoped to `recollect-*` / the GitHub OIDC ARN) is the pragmatic path; the tight path is a
customer-managed policy granting exactly those. See
[AWS credentials](#aws-credentials-for-pulumi-itself--environment-not-stack-config) for the full
least-privilege list (it covers BOTH stages). Never use the **root** user.

### 2. Create the FOUNDATION stack + set its config

The only deployment-unique input is **`githubRepo`** (your `owner/repo`, which pins the CI trust);
the rest have generic defaults. Nothing real is committed — it lands in your gitignored
`deploy/pulumi/foundation/Pulumi.<stack>.yaml`.

```bash
make foundation-install                 # once: (cd deploy/pulumi/foundation && npm install)
make foundation-typecheck               # tsc --noEmit (no cloud calls)
make foundation-preview                 # review every resource before creating anything
make foundation-up                      # CREATE the ECR repo + OIDC provider + CI role
```

> **The lifecycle targets self-provision.** `make foundation-preview` / `-up` run a **preflight**
> (`deploy/pulumi/preflight.sh`) that selects/creates the `prod` stack (override with `STACK=`),
> defaults `region=us-east-2`, ensures your `PULUMI_CONFIG_PASSPHRASE` (prompting if unset), and
> **prompts for `githubRepo`** if you haven't set it — so the four lines above are enough.
> (`make pulumi-state-bucket` already did the `pulumi login`.) The **same preflight** fronts every
> `deploy-*` target — see [The make targets' preflight](#the-make-targets-preflight) for the schema.

To set values up front instead of answering prompts (e.g. a non-interactive run), pre-seed the config —
it lands in your gitignored `deploy/pulumi/foundation/Pulumi.<stack>.yaml`:

```bash
cd deploy/pulumi/foundation
pulumi config set githubRepo yourorg/recollect   # else preflight prompts for it (pins CI trust to this repo's main)
pulumi config set region     us-east-2           # optional — preflight defaults this; keep == PLATFORM's
pulumi config set imageName  recollect-server    # optional (default) — the ECR repo name
pulumi config set expireUntaggedAfterDays 14     # optional lifecycle knobs (defaults shown)
pulumi config set keepReleaseImages       20
cd ../../..
```

### 3. Read the two outputs and wire them in

```bash
make foundation-outputs                 # prints repoUrl, ciRoleArn, githubOidcProviderArn, ecrRegion
```

Take those two values to **GitHub → your repo → Settings → Secrets and variables → Actions →
Variables** (these are **non-secret config**, hence repo *variables*, not secrets):

| GitHub Actions **variable** | Set to | From |
|---|---|---|
| `AWS_ROLE_ARN` | the CI role ARN | FOUNDATION output `ciRoleArn` |
| `ECR_REPO_URL` | the ECR repo URL | FOUNDATION output `repoUrl` |
| `AWS_REGION` | your region (e.g. `us-east-2`) | FOUNDATION output `ecrRegion` |

You can set them from the terminal with the GitHub CLI:

```bash
gh variable set AWS_ROLE_ARN  --body "$(cd deploy/pulumi/foundation && pulumi stack output ciRoleArn)"
gh variable set ECR_REPO_URL  --body "$(cd deploy/pulumi/foundation && pulumi stack output repoUrl)"
gh variable set AWS_REGION    --body "$(cd deploy/pulumi/foundation && pulumi stack output ecrRegion)"
```

The `.github/workflows/deploy-image.yml` job **skips** unless `AWS_ROLE_ARN` is set, so a public clone
never fails — wiring these three is what turns CI's ECR push **on**.

> **Console fallback (no CLI).** ECR: **Console → ECR → Create repository** (`recollect-server`,
> *Scan on push* ON, *Tag immutability* ON, add a lifecycle rule). OIDC: **IAM → Identity providers →
> Add provider → OpenID Connect**, URL `https://token.actions.githubusercontent.com`, audience
> `sts.amazonaws.com`. Role: **IAM → Roles → Create role → Web identity**, pick that provider +
> audience, then **attach an inline policy** that allows `ecr:GetAuthorizationToken` on `*` and the
> push set (`ecr:BatchCheckLayerAvailability`, `BatchGetImage`, `GetDownloadUrlForLayer`,
> `InitiateLayerUpload`, `UploadLayerPart`, `CompleteLayerUpload`, `PutImage`) on the repo ARN, and
> **edit the trust policy** so the `sub` condition is `repo:yourorg/recollect:ref:refs/heads/main`.
> (The IaC does all of this for you — the console path is only if you can't run Pulumi.)

### 4. CI pushes the image (no stored keys)

With the variables set, every **push to `main`** that touches `app/` or the `Dockerfile` runs
`deploy-image.yml`: it assumes `AWS_ROLE_ARN` via OIDC, builds the server image (the repo-root
Dockerfile), and pushes `${ECR_REPO_URL}:sha-<commit>` + `:latest`. Trigger it manually anytime from
**Actions → deploy-image → Run workflow**. The pushed `sha-<commit>` tag is the **immutable** ref
PLATFORM deploys.

> **Main-only by design.** The workflow triggers on push-to-`main` + manual dispatch, **not**
> pull_request — the CI role's trust is pinned to this repo's `main` (the OIDC `sub`), so a PR's token
> couldn't assume it anyway, and a fork PR can never push. To also push on tags/PRs, broaden the `sub`
> in `foundation/index.ts` and add the matching trigger.

> **The site-deploy workflow's GitHub inputs come after PLATFORM.** `site-deploy.yml` also needs a
> Cloudflare token + account id + the `CF_PAGES_PROJECT` name — but that name is a **PLATFORM** output
> (`pagesProjectName`), so it can't be wired until the box is up. Those are wired (and the full set of
> every workflow's secrets/variables recapped) in
> [PLATFORM → Wire the site-deploy outputs](#wire-platforms-site-deploy-outputs-into-github).

### The make targets' preflight
Every `foundation-*` / `deploy-*` **lifecycle** target (preview, up, refresh, destroy, outputs, ssm)
runs `deploy/pulumi/preflight.sh` first, so a missing passphrase or unset config **asks** instead of
failing mid-apply. In order, it:

1. **Secret env vars** — ensures each needed one is set, prompting *silently* for any that isn't:
   `PULUMI_CONFIG_PASSPHRASE` (both projects) + `CLOUDFLARE_API_TOKEN` (PLATFORM preview/up/refresh/destroy).
2. **AWS auth** — a soft `aws sts get-caller-identity` check; a miss prints the `aws sso login` hint, never blocks.
3. **Stack** — selects `STACK` (default `prod`), creating it with `pulumi stack init` if absent.
4. **Optional defaults** — sets `region=us-east-2` if unset.
5. **Required config** — **auto-derived from the project's `Pulumi.yaml`**: every key with no
   `default:` (so the prompt list can never drift from the schema — add a no-default key and it's
   prompted automatically). Prompts for any you haven't set (FOUNDATION: `githubRepo`; PLATFORM:
   `domain repoUrl gitRef serverImage cloudflareAccountId cloudflareZoneId maintainerEmail`).

then runs the command in the project dir (one process, so a just-typed passphrase reaches `pulumi`).
Override the stack per-invocation — `make foundation-up STACK=staging`; pre-seeding config with
`pulumi config set` skips the matching prompts (for non-interactive runs). Schema + an example:

```bash
[STACK=prod] [ENVVARS="A B"] [REQUIRED="k1 k2"] [DEFAULTS="k=v …"] \
  deploy/pulumi/preflight.sh <project-dir> <command> [args…]

# what `make foundation-up` expands to:
ENVVARS="PULUMI_CONFIG_PASSPHRASE" REQUIRED="githubRepo" DEFAULTS="region=us-east-2" \
  bash deploy/pulumi/preflight.sh deploy/pulumi/foundation pulumi up
```

---

## PLATFORM — run per release (the box that PULLS the image)

Run this **per release**, with the same short-lived **admin** SSO session. It stands up (or updates)
the EC2 box, the Cloudflare tunnel/DNS/Access, and the observability + alarms — and the box **pulls**
the server image CI pushed to FOUNDATION's ECR (it never builds Rust on the 1 GB micro).

**Prerequisite:** FOUNDATION is up and CI has pushed at least one image (you have an
`${ECR_REPO_URL}:sha-<commit>` to deploy).

```bash
# Admin session (same as FOUNDATION) + the Cloudflare token PLATFORM needs:
aws sso login                           # the DEFAULT profile (no --profile/AWS_PROFILE); named profile? add `--profile <name> && export AWS_PROFILE=<name>`
export CLOUDFLARE_API_TOKEN=…           # the scoped custom token (Tunnel+Access+Pages+DNS) — see below

make deploy-install                     # once: (cd deploy/pulumi/platform && npm install)

# REQUIRED, all deployment-unique (no committed defaults) — pre-seed them so you're not prompted for
# seven values; they land in your gitignored Pulumi.<stack>.yaml. (cd to the project for config set.)
cd deploy/pulumi/platform
pulumi config set domain               your-domain.com
pulumi config set repoUrl              https://github.com/yourorg/recollect.git
pulumi config set gitRef               <COMMIT_SHA_OR_TAG>          # the release you're deploying
pulumi config set serverImage          "$(cd ../foundation && pulumi stack output repoUrl):sha-<COMMIT_SHA>"
pulumi config set cloudflareAccountId  <CF_ACCOUNT_ID>
pulumi config set cloudflareZoneId     <CF_ZONE_ID>
pulumi config set maintainerEmail      you@example.com
# (optional keys — region, instanceType, cfTeamName, alarmEmail, … — have sensible defaults; full
#  tables under "Stack config" below.)

cd ../../..                             # back to repo root
make deploy-typecheck                   # tsc --noEmit (no cloud calls)
make deploy-preview                     # review the plan
make deploy-up                          # CREATE/UPDATE the box (it PULLS serverImage on boot)
make deploy-outputs                     # site URL, grafanaUrl, instanceId, the SSM command
```

> **The preflight fronts the `deploy-*` targets too.** `make deploy-preview` / `-up` select/create the
> `prod` stack, default `region=us-east-2`, ensure both **`PULUMI_CONFIG_PASSPHRASE`** and
> **`CLOUDFLARE_API_TOKEN`** (prompting silently for either if unset), and **prompt for any of the seven
> required inputs you didn't pre-seed** above — so a missing value asks instead of failing mid-apply.
> `deploy-outputs` / `deploy-ssm` need only the passphrase. See
> [The make targets' preflight](#the-make-targets-preflight).

`serverImage` is the crux: it is **FOUNDATION's `repoUrl` output at the `sha-<commit>` tag CI pushed**.
The box's instance role grants **ECR read-only**, so cloud-init does a keyless `docker login` +
`docker compose pull server` and runs the image — no on-box compile.

### Wire PLATFORM's site-deploy outputs into GitHub
The mirror of [FOUNDATION §3](#3-read-the-two-outputs-and-wire-them-in), now that PLATFORM is up: its
**`pagesProjectName`** output names the Cloudflare Pages project CI uploads the website to. Wire the
static-site workflow (`site-deploy.yml`) — the project name (its **ON switch**) + a Pages-scoped token
+ the account id:

```bash
gh variable set CF_PAGES_PROJECT      --body "$(cd deploy/pulumi/platform && pulumi stack output pagesProjectName)"
gh secret   set CLOUDFLARE_API_TOKEN  --body '<a Pages:Edit token — see "Cloudflare credentials" below>'
gh secret   set CLOUDFLARE_ACCOUNT_ID --body '<your Cloudflare account id>'
```

`site-deploy.yml` skips until `CF_PAGES_PROJECT` is set; full detail (the direct-upload `wrangler`
step + rotation) is in [deploy/site/README.md §3](site/README.md#3-the-residual-manual-steps-what-pulumi-cant-do).

#### Every GitHub Actions secret & variable (all workflows)
With both stacks up, here's the complete set CI needs. `GITHUB_TOKEN` is auto-provided (nothing to set);
the two **gate** inputs (`AWS_ROLE_ARN`, `CF_PAGES_PROJECT`) are ON switches whose job skips until set,
so a fresh or public clone never red-fails.

| Kind | Name | Used by | Set from | Wired in |
|---|---|---|---|---|
| variable | `AWS_ROLE_ARN` (gate) | `deploy-image.yml` | FOUNDATION output `ciRoleArn` | [FOUNDATION §3](#3-read-the-two-outputs-and-wire-them-in) |
| variable | `AWS_REGION` | `deploy-image.yml` | FOUNDATION output `ecrRegion` | ↑ |
| variable | `ECR_REPO_URL` | `deploy-image.yml` | FOUNDATION output `repoUrl` | ↑ |
| variable | `CF_PAGES_PROJECT` (gate) | `site-deploy.yml` | PLATFORM output `pagesProjectName` | here ↑ |
| secret | `CLOUDFLARE_API_TOKEN` | `site-deploy.yml` | a **Pages : Edit** token ([Cloudflare creds](#cloudflare-credentials-for-pulumis-cloudflare-provider--environment)) | here ↑ |
| secret | `CLOUDFLARE_ACCOUNT_ID` | `site-deploy.yml` | your Cloudflare account id | here ↑ |
| (auto) | `GITHUB_TOKEN` | `ci.yml` | provided by Actions — no setup | — |

`nightly.yml` + `mutation.yml` need nothing beyond `GITHUB_TOKEN`.

**Redeploy a new release** (after CI has pushed its image): bump `gitRef` + `serverImage` to the new
SHA and `make deploy-up` (a `gitRef` change re-provisions the box via `userDataReplaceOnChange`), or
keep the box and update it in place over SSM:

```bash
make deploy-ssm
sudo recollect-update <NEW_SHA_OR_TAG>  # re-points .env at the new image tag, ECR-pulls, recreates
```

The detailed PLATFORM input tables, the Cloudflare token recipe, and the secret-rotation guide are
all below. The **rest of this document** (Architecture, Observability, day-2 Access) describes the
PLATFORM box.

---

## Architecture

```
                       ┌──────────────────────────── Cloudflare ───────────────────────────────┐
   browser  ─HTTPS─▶   │  your-domain.com / www  →  Cloudflare PAGES (the static website)      │
   browser  ─HTTPS─▶   │  play.your-domain.com (proxied) · edge TLS · Web Analytics  ┐         │
   wss://…/ws         │  grafana.your-domain.com (proxied) ─ Zero Trust ACCESS gate │(email)  │
   maintainer ─HTTPS▶ │                              ▲  named Tunnel (dials OUT)     ┘         │
                       └──────────────────────────────│────────────────────────────────────────┘
                                                      │  outbound, no inbound ports (play./grafana. only)
   ┌───────────────────────── AWS EC2 (t3.micro, us-east-2, egress-only SG) ─────────────────────┐
   │  Docker Compose (deploy/compose/docker-compose.deploy.yml), brought up by cloud-init:        │
   │                                                                                              │
   │   cloudflared ─┬─▶ recollect-server :8080 ──▶ postgres :5432                                 │
   │                │   (axum ws + REST, serves the   (on-box journal + usage_events, /data)      │
   │                │    site + wasm via STATIC_DIR)        ▲                                      │
   │                │        │ OTLP :4317                   │                                      │
   │                │        ▼                              │                                      │
   │                └─▶ lgtm :3000  ◀── node-exporter :9100 │  (Grafana+Tempo+Loki+Mimir+OTel,    │
   │   site-builder ─(one-shot → `site` volume)─┘            │   /data/observability, dashboards   │
   │                                                         │   as code, short retention)         │
   │   CloudWatch agent ─(mem/swap/disk every 5m)─▶ CloudWatch ─▶ SNS email (out-of-band alarms)   │
   └──────────────────────────────────────────────────────────────────────────────────────────────┘
```

**Single origin (`play.<domain>`).** The axum server serves its copy of the page, the wasm client
(`/client/…`), AND the `wss://play.your-domain.com/matches/{id}/ws` socket from one origin — so there
is no CORS, no mixed content, and **no separate reverse proxy**. The server gained an env-gated
static-file route (`STATIC_DIR`); when set, it serves the built site alongside the API
(`router_with_static` in `recollect-server`). cloudflared proxies `play.your-domain.com` →
`http://server:8080` directly. (The **marketing website** is the apex/`www`, served by **Cloudflare
Pages** — a separate origin; see [`deploy/site/README.md`](site/README.md).)

**No inbound ports.** The Cloudflare Tunnel (`cloudflared`) dials *out* to Cloudflare, so the
EC2 security group has **zero inbound rules**. Admin is keyless via **SSM Session Manager**
(`make deploy-ssm`) — no SSH key, no port 22. (Want SSH instead? Add a key pair + a port-22
ingress rule to `deploy/pulumi/platform/index.ts`; the tunnel means you don't need to.)

**Edge TLS, and why no Origin CA cert.** §10.1 mentions a Cloudflare Origin CA cert for
Full(strict). That applies when Cloudflare connects to a *public* origin over the internet. With
a **Tunnel** there is no public origin: cloudflared establishes its own authenticated, encrypted
outbound connection to Cloudflare, and reaches the server over the private compose network. So the
tunnel *is* the encrypted origin path; an Origin CA cert on a public 443 listener would be moot
(there is no such listener). TLS to the browser is Cloudflare's universal edge cert.

### What runs in the stack
| Service | Image | Role |
|---|---|---|
| `postgres` | `postgres:18.4-alpine` | On-box durable journal + `usage_events`. `DATABASE_URL` targets it ⇒ Postgres-authoritative (append-before-ack, resume-from-journal). Its data dir is a **host bind mount onto a separate, durable EBS data volume** (see [Durable data](#durable-data--surviving-instance-recreation) below) — it survives box recreation. |
| `site-builder` | built from `deploy/compose/Dockerfile.site` | One-shot: builds `site/*.html` + the wasm play client into the shared `site` volume; bakes the client's origin and (optional) the analytics beacon. Exits 0. |
| `recollect-server` | built from the repo-root `Dockerfile` (`--profile dist`) | The axum ws/REST server; also serves the static site from `STATIC_DIR=/srv`. Exports OTLP to `lgtm:4317` by default (the §11 self-hosted posture). |
| `cloudflared` | `cloudflare/cloudflared` | The named Tunnel connector; routes `play.your-domain.com` → `server:8080` **and** `grafana.your-domain.com` → `lgtm:3000` (the latter behind the Access gate). The website apex/`www` is on Cloudflare Pages, not this tunnel. |
| `lgtm` | `grafana/otel-lgtm:0.28.0` | **Self-hosted observability (§11):** Grafana + Tempo + Loki + Prometheus/Mimir + an OTel collector in one container. Receives the server's OTLP; serves the dashboards. Data on `/data/observability` (durable EBS); **short retention** (metrics 14d, logs 7d, traces 3d); dashboards + datasources **provisioned as code** from `deploy/compose/observability/`. Reachable only via the Access-gated tunnel. |
| `node-exporter` | `prom/node-exporter:v1.11.1` | A tiny host-metrics sidecar (reads host `/proc`,`/sys`,`/` **read-only**), scraped by `lgtm`'s Prometheus to feed the **host/box dashboard** (CPU/mem/swap/disk). ~15–25 MB RSS. |
| `blackbox` | `prom/blackbox-exporter:v0.27.0` | **Synthetic monitoring**: actively probes the live service, scraped by `lgtm`'s Prometheus on a schedule, with **Grafana alert rules** that page when a probe is down for N minutes. **Internal** probes (always on): the server's `/healthz`, the static site index, a TCP connect to the ws origin — deep app outages an edge check can't catch. **External** edge probes (opt-in, off until you set a domain): the public site + `play.<domain>/healthz` through Cloudflare — Tunnel/DNS/TLS failures the internal probes can't see. ~10–20 MB RSS. See [Synthetic monitoring](#synthetic-monitoring--internal-probes-always-on--external-edge-probes-opt-in) below + [docs/observability.md §5](../docs/observability.md). |

### Durable data — surviving instance recreation

The match journal + accounts are the one thing on this box we must not lose. They live in Postgres,
which writes to a **dedicated EBS data volume that is SEPARATE from the instance root**, mounted at
**`/data`** — so a `pulumi up` that **replaces** the box (e.g. a new pinned `gitRef` triggers
`userDataReplaceOnChange`) or a plain terminate **does not lose the data**. (Before this, Postgres
wrote to the root volume, which is destroyed with the instance — a terminate or replace wiped the
journal.)

`/data` is the box's **one durable mount, shared by every stateful service** via its own subdir:
`/data/postgres` today, and a **light self-hosted observability stack** (Grafana + a metrics TSDB,
~1–2 GB at short retention) when that lands — same volume, no AWS change (the default 20 GiB is sized
for both). user-data mounts `/data` generically; each service gets a subdir under it.

How it works, end to end:

- **A separate volume (Pulumi).** `index.ts` creates an `aws.ebs.Volume` — **gp3, encrypted**,
  `dataVolumeSizeGb` GiB (default **20**), in the **same AZ as the instance** (EBS is AZ-local) — and
  an `aws.ec2.VolumeAttachment` at `/dev/sdf`. Because it is a standalone volume (not the instance's
  `rootBlockDevice`, not an `ebsBlockDevice` on the instance), **nothing sets delete-on-termination on
  it** — it has its own lifecycle and is **not** torn down when the box goes away. It carries the tag
  `recollect:data = true`. (No `protect`/`retainOnDelete` is set — see why under *recreate* next — so a
  real `make deploy-destroy` still cleanly deletes it rather than wedging teardown.)
- **On recreate the volume moves to the new box.** When Pulumi replaces the instance (e.g. a `gitRef`
  bump under `userDataReplaceOnChange`), it replaces only the **`VolumeAttachment`** — that resource's
  `instanceId` input changed. The **`Volume`'s** own inputs (size/type/AZ) did **not** change, so
  Pulumi never replaces the Volume; it stays put, and the new instance's fresh attachment **re-binds
  the same volume** at `/dev/sdf`. The new box's cloud-init then **MOUNTS** it — it does **not**
  reformat it (see next). The data is simply there. (`pulumi stack output dataVolumeId` is the volume's
  id, so you can confirm the *same* volume re-attached.)
- **Format-only-if-empty (the safety that makes this durable).** `user-data.sh` resolves the data
  device by its stable identifier (the Nitro NVMe node behind `/dev/sdf`), checks for an existing
  filesystem with `blkid`/`lsblk -f`, and **formats ext4 ONLY when there is none** (a brand-new
  volume's first boot). An already-formatted volume is **mounted as-is, never reformatted** — a
  reformat-on-boot would wipe the data on *every* boot, the exact durability bug this avoids. It then
  mounts at `/data`, persists the mount in `/etc/fstab` **by UUID** with `nofail` (so a late/missing
  volume can never wedge boot), and creates the per-service subdirs: `/data/postgres` owned `70:70`
  (the alpine Postgres uid; bind-mounted to `/var/lib/postgresql`) and `/data/observability` (the
  self-hosted LGTM stack's data dir; bind-mounted to the `lgtm` container's `/data`).
- **Swap on `/data`.** Because the box is only 1 GB, cloud-init creates a **swap file on the data
  volume** (`/data/swapfile`, `swapSizeGb` GiB, default **4**) — RAM headroom for the memory-hungry
  first `docker build` and the self-hosted observability stack. It is created **idempotently** (only
  if absent: `fallocate`→`mkswap`→`swapon`, `chmod 600`), persisted in `/etc/fstab` (`nofail`), and
  the kernel is biased toward RAM with **`vm.swappiness=10`** so swap is a safety net, not a hot path.
  It lives on `/data` (not the root) and so is counted in `dataVolumeSizeGb`. Grow it with `swapSizeGb`.
- **Observability persists on `/data`.** The self-hosted LGTM stack writes Grafana state + the
  metrics/logs/traces stores to **`/data/observability`** on this same durable volume (so dashboards
  and history survive a box recreate), at **short retention** to bound it to ~1–2 GB. See
  [Observability](#observability--self-hosted-lgtm-grafana-access-cloudwatch) below. (To ship
  **off-box** to e.g. Grafana Cloud instead, set `otelEndpoint` — the server then exports there.)

> **No snapshots.** Recovery is by the durability of the volume itself (it outlives the box); there is
> deliberately **no** EBS-snapshot/DLM policy in this stack. `make db-backup` (a `pg_dump`) remains the
> ad-hoc logical backup if you want one.

> **Local runs are unaffected.** `make deploy-local` (the `docker-compose.local.yml` overlay) points
> Postgres at an ordinary **named volume**, so a laptop needs no `/data` mount; only the real box uses
> the EBS bind mount. The overlay also **scales the observability stack (`lgtm` + `node-exporter`) to
> zero** — local observability is the root `make dev-up` (`grafana/otel-lgtm`) stack (or `make up`), not this one.

---

## Observability — self-hosted LGTM, Grafana Access, CloudWatch

This implements tech-design **§11** as an **all-in-IaC, self-hosted** stack on the box (no Grafana
Cloud bill, no SaaS): one `grafana/otel-lgtm` container (Grafana + Tempo + Loki + Prometheus/Mimir +
an OTel collector) receives the server's OTLP, with **dashboards-as-code**, an **Access-gated**
Grafana subdomain, and **out-of-band CloudWatch alarms**. All of it is declarative — compose +
provisioning JSON + Pulumi; nothing is clicked together.

### The stack (the `lgtm` container)
`recollect-server` exports **all three signals** over OTLP to `http://lgtm:4317` **by default** (the
`OTEL_EXPORTER_OTLP_ENDPOINT` env is wired Pulumi → user-data `.env` → compose; empty `otelEndpoint`
⇒ on-box). The exporter is fire-and-forget — if `lgtm` is down the batch exporters just drop and the
server is unaffected (so the server does **not** `depends_on` it). Data lives on
**`/data/observability`** (the durable EBS volume), so dashboards + history survive a container or box
recreate.

**Short retention bounds storage to ~1–2 GB** (config, in the compose / mounted backend configs):
- **Metrics** (Prometheus TSDB — the dominant cost): `PROM_RETENTION`, default **14d**, via
  `--storage.tsdb.retention.time` (effective; drops old blocks).
- **Logs** (Loki): default **7d**, via a mounted `loki-config.yaml` with a **compactor +
  `retention_enabled`** (a period alone does not delete in Loki — the compactor must run).
- **Traces** (Tempo — the bulkiest signal ⇒ the shortest window): default **3d**, via a mounted
  `tempo-config.yaml` with a **compactor `block_retention`**.

The mounted `prometheus.yaml`/`loki-config.yaml`/`tempo-config.yaml` under
`deploy/compose/observability/` are faithful copies of the pinned image's upstream configs **plus**
those retention additions (and one node-exporter scrape job) — each file documents exactly what we
added; keep them in sync with upstream on an otel-lgtm bump.

**Container-hardened** like the rest: `cap_drop: ALL` + `no-new-privileges`. It is **not**
`read_only` — Grafana/Prometheus/Loki/Tempo each write working state and the image's supervisor
writes pid/log files on its own rootfs, so a read-only root breaks startup; the writable surface is
the durable `/data` bind mount + the ephemeral container rootfs, with no Linux capabilities. (The
`read_only` posture **is** applied where feasible — the `server` and `cloudflared` — and `node-exporter`
is fully read-only.) **It leans on the box's 4 GB swap** for headroom on the 1 GB box (see
[the RAM math](#does-it-fit-1-gb--swap--free-tier)).

### How the maintainer reaches Grafana (the Cloudflare Access story)
Grafana is reachable at **`https://grafana.your-domain.com`** but is **never publicly usable** — it
sits behind a **Cloudflare Zero Trust Access** application (free tier, ≤ 50 seats):

1. Pulumi creates a **proxied DNS CNAME** for `grafana.<domain>` → the tunnel, and a **tunnel ingress
   rule** `grafana.<domain>` → `http://lgtm:3000`. So the subdomain resolves and is fronted by
   Cloudflare, and the tunnel *can* reach the on-box Grafana.
2. Pulumi also creates a **Zero Trust Access application** bound to that hostname + an **allow
   policy** that includes **exactly the `maintainerEmail`**. Access authenticates **every request at
   the edge before it reaches the origin** — so nothing hits Grafana until the visitor proves they are
   the allowed email.
3. **The flow:** the maintainer opens `https://grafana.your-domain.com`, gets the Cloudflare Access
   login, authenticates (a **one-time PIN** emailed to the allowed address by default — or a
   configured IdP), and lands on Grafana. The session lasts `24h` before re-prompting. Everyone else
   is denied (no policy grants them in).

Grafana itself runs **anonymous-Admin** (the otel-lgtm default) — which is **safe only because**
Access is the real authentication in front; nothing public can reach it. (`make deploy-ssm` +
`docker logs` is the break-glass if Access ever locks you out.) The Grafana URL is surfaced as the
**`grafanaUrl`** stack output.

### R2-1 — origin JWT validation (defense-in-depth)

**Why it matters.** Without an origin check, the *only* thing standing between the public internet and
an **anonymous-Admin** Grafana is Cloudflare's **edge** Access check. A bare `tunnel run --token …`
connector forwards every request it receives straight to `lgtm:3000`, and anonymous-Admin Grafana does
not verify the `Cf-Access-Jwt-Assertion` header. So if Access were ever **deleted/disabled, its policy
widened by mistake, or bypassed via a misconfiguration**, Grafana would be fully exposed — anonymous
**Admin**, including the datasource proxy (read all on-box logs/metrics/traces, plus an SSRF surface).
Cloudflare's own guidance is explicit that you **should** validate the token at the origin so requests
that bypass Access are rejected
([Validate JWTs](https://developers.cloudflare.com/cloudflare-one/access-controls/applications/http-apps/authorization-cookie/validating-json/)).

**It is now wired — CONDITIONAL on `cfTeamName`.** `index.ts` captures the Grafana Access app in
`const grafanaAccessApp` and, **when you set the `cfTeamName` stack config**, adds an
`originRequest.access` block to the `grafana.<domain>` tunnel ingress so the **connector itself** (not
just the edge) rejects any L7 request lacking a valid Access JWT for that app's AUD:

```ts
// deploy/pulumi/platform/index.ts — built once, then attached to the grafana ingress:
const cfTeamName = cfg.get("cfTeamName");                       // OPTIONAL — your Zero Trust team name
const grafanaOriginRequest = cfTeamName
  ? { access: { required: true, teamName: cfTeamName, audTags: [grafanaAccessApp.aud] } }
  : undefined;                                                  // unset ⇒ edge-only Access (no origin check)
// …ingress:  { hostname: grafanaHostname, service: "http://lgtm:3000", originRequest: grafanaOriginRequest }
```

- **`cfTeamName` set** ⇒ **defense-in-depth**: the connector validates the JWT origin-side and binds it
  to THIS Access app's AUD, so a request that somehow bypasses the edge is still refused. Recommended
  for any real deployment. (`cfTeamName` is the `<team>` in `<team>.cloudflareaccess.com` — Zero Trust
  dashboard → Settings; set it: `pulumi config set cfTeamName <team>`.)
- **`cfTeamName` unset** (the default) ⇒ the current **edge-only Access** posture: still gated at
  Cloudflare, just without the second origin check. The program typechecks and deploys identically —
  `originRequest` is simply `undefined`.

It is **conditional, not forced**, because it can only be validated against a real Zero Trust org and
the team name is deployment-unique (no committed default). Whether or not you wire it, treat
"don't casually delete/recreate the Access app/policy" as load-bearing, and prefer changing the
allow-list over recreating the app.

### Dashboards-as-code
Four dashboards are **JSON in the repo** (`deploy/compose/observability/grafana/dashboards/`),
**provisioned** via a mounted Grafana provider (`…/provisioning/dashboards/recollect.yaml`) — never
clicked together, never lost with the box, read-only in the UI so the JSON stays the source of truth:

- **`Recollect — RED service metrics`** — Rate / Errors / Duration for the command path: command rate
  by `outcome`, **reject ratio** (the cheap anti-cheat signal — honest clients never send illegal
  commands), the **engine apply-duration** histogram (p50/p90/p99, exemplars → Tempo), WS
  connections/reconnections, matches created vs finished, and an HTTP-route panel (from the image's
  eBPF auto-instrumentation if present).
- **`Recollect — game-design metrics (§16 in prod)`** — the spec's balance metrics from the match
  counters: **P1 (seat A) / P2 (seat B) winrate**, **draw rate**, the outcome split, P1-winrate
  drift over time, Solace (PvE) faction winrate, and usage by mode/opponent. Panels for the **deeper
  §16 metrics** (winrate-when-leading-at-contraction ≤ 70%, evolutions/match, Throughline completion,
  median match length) are **wired and provisioned now** but render *No data* until the server emits
  those per-match facts — each is annotated with the exact metric to add, so they light up the moment
  the instrumentation lands (no UI work later). See [the gap note](#whats-not-covered-yet) below.
- **`Recollect — host / box`** — CPU / memory / **swap** / disk for the 1 GB box, from the
  `node-exporter` sidecar (the swap panels matter most here).
- **`Recollect — synthetic monitoring`** — the **probes** board (see [Synthetic monitoring](#synthetic-monitoring--internal-probes-always-on--external-edge-probes-opt-in)
  above): per-probe **UP/DOWN** status + up/down timeline (`probe_success`, all five — 3 internal + 2
  external), **latency** (`probe_duration_seconds`), **uptime %** over the window, and a **public TLS
  cert days-to-expiry** countdown for the external HTTPS probes. The external panels render gracefully
  **empty when external monitoring is off** (no domain ⇒ no series).

### CloudWatch — the out-of-band box-health net
The in-box dashboard can't alarm on its **own** outage (a wedged box takes Grafana with it), so a
**second, independent eye** lives in CloudWatch (Pulumi). The lightweight **CloudWatch agent** on the
box (installed by cloud-init from the committed `cloudwatch-agent.json`, granted by the
`CloudWatchAgentServerPolicy` instance role) publishes a few **custom host metrics** every 5 minutes;
Pulumi creates **7 alarms → an SNS email topic** (`alarmEmail`, falling back to `budgetEmail`):

| Alarm | Source | Fires when |
|---|---|---|
| `recollect-status-instance` | `AWS/EC2 StatusCheckFailed_Instance` | OS/guest unhealthy (2×5 min) |
| `recollect-status-system` | `AWS/EC2 StatusCheckFailed_System` | host/network unhealthy — **also auto-recovers** the instance (free EC2 `recover` action) |
| `recollect-cpu-high` | `AWS/EC2 CPUUtilization` | ≥ `cpuAlarmThresholdPct` (default 80%) sustained 3×5 min |
| `recollect-mem-high` | custom `mem_used_percent` | host memory ≥ 90% |
| `recollect-swap-high` | custom `swap_used_percent` | swap ≥ 70% (real memory pressure — consider `t3.small`) |
| `recollect-disk-root` / `recollect-disk-data` | custom `disk_used_percent{path=/ , /data}` | a mount ≥ 85% |

**Free-tier discipline:** 7 alarms (≤ 10 free), **4 custom metrics** (mem, swap, disk×2 — ≤ 10 free),
**basic 5-minute** metrics, **no detailed monitoring**, and **no CloudWatch Logs shipping** (logs
live in on-box Loki; Logs ingestion is not free beyond 5 GB). The SNS subscriber must **click the
confirmation email once** before alerts deliver. The topic ARN is the **`alarmTopicArn`** output.

### Synthetic monitoring — internal probes (always on) + external edge probes (opt-in)
The `blackbox` sidecar actively **probes the live service** on a schedule; **Grafana alert rules**
(provisioned from `observability/grafana/provisioning/alerting/`) page when a probe stays down for
N minutes. There are **two layers** (full detail + the fault-localization table: [docs/observability.md
§5](../docs/observability.md)):

- **Internal probes — committed, always on.** Over the compose network: the server's `/healthz`, the
  site index, and a TCP connect to the ws origin. They answer **"is the app up?"** and need no config.
- **External edge probes — opt-in, off until you set a domain.** The PUBLIC https URLs a real player
  hits, **through Cloudflare** — the static **site** (apex + `www`) and the **game** at
  `play.<domain>/healthz`. They answer **"is it reachable from the outside?"**, catching
  **Tunnel-down / DNS-broken / TLS-expired** that the internal probes structurally cannot see
  (internal-up + external-down ⇒ the edge/Tunnel/DNS, not the app). Alert rules are a **separate**
  Grafana group (`synthetic-monitoring-external`, `noDataState: OK`) so they distinguish *app down*
  from *edge down* — and stay silent until enabled.

**Enable the external probes (the live domain is NEVER committed).** Because the domain is
deployment-unique, the external job reads its targets from a Prometheus file-SD dir that ships **empty
of live targets** — so a clean clone / `make deploy-local` stays **inert**. Turn it on by rendering the
live target file **from your domain** with the committed helper, then it self-activates (Prometheus
hot-reloads within ~5 min — no restart):

```bash
make deploy-ssm                                   # onto the box
cd /opt/recollect
# Set your domain explicitly, OR omit it — the script reads OBS_GRAFANA_DOMAIN from /opt/recollect/.env
# (the bare domain cloud-init already wrote from the Pulumi `domain`):
OBS_PUBLIC_DOMAIN=your-domain.com \
  deploy/compose/observability/prometheus/render-external-targets.sh
#   → writes deploy/compose/observability/prometheus/external-targets/public-edge.yaml (gitignored)
# Turn it back OFF: delete that file.  Optional: OBS_GAME_SUBDOMAIN=play (default; matches `gameSubdomain`).
```

To make it **automatic per deploy on your fork**, render the same file from `user-data.sh` at boot —
exactly how cloud-init already writes the `.env` values from the Pulumi `domain` config (e.g. invoke
`render-external-targets.sh` with `OBS_PUBLIC_DOMAIN` set, after the repo checkout and before
`compose up`). The committed repo stays domain-free either way.

**Paging is deliberately NOT wired yet (pre-launch).** The alert **contact point** (`recollect-oncall`
in `observability/grafana/provisioning/alerting/contactpoints.yaml`) is a **documented placeholder**
(`*.example.invalid`, RFC-2606 — never resolves); **no real destination is in git**. Alerts still
**fire and are visible** in Grafana (Alerting → Active notifications) and on the dashboards — they just
don't egress. When you *do* wire paging post-launch, the recommended **no-SMTP** option is a tiny
**Cloudflare Worker** as the webhook target: point the `recollect-webhook` receiver at the Worker URL,
Grafana POSTs the alert JSON, and the Worker relays to **email** (Cloudflare **Email Routing** /
**MailChannels**) or a push — a serverless relay on the Cloudflare account this deploy already uses, no
SMTP server to run. It sits alongside the usual targets (Slack/Discord webhook, PagerDuty, Opsgenie).
Keep the Worker URL + any secret **on the box, never in git** (Grafana `secureSettings` / an
env-templated header), like every other deployment-unique value. See [docs/observability.md §5.2](../docs/observability.md).

### Does it fit (1 GB + swap + free-tier)?
**RAM (the tight constraint).** Rough steady-state RSS on the box:

| Process | ~RSS |
|---|---|
| `recollect-server` (musl, distroless) | ~30–60 MB |
| `postgres` (idle, small shared_buffers) | ~60–120 MB |
| `cloudflared` | ~30–50 MB |
| `node-exporter` | ~15–25 MB |
| **`lgtm`** (Grafana + Prometheus + Loki + Tempo + collector, JVM-free Go binaries but several of them) | **~400–700 MB** |
| CloudWatch agent + OS/Docker | ~80–150 MB |

That sum **exceeds 1 GB** under load — which is exactly why the box runs a **4 GB swap file on
`/data`** with `vm.swappiness=10`. The LGTM stack is the heavy tenant; at **playtest traffic** its
working set is modest and the cold/bulk pages live in swap, so steady-state RAM pressure stays
manageable and the **host dashboard's Swap-used % + the `recollect-swap-high` CloudWatch alarm** are
the canaries. **If swap thrashes under real load, the documented fallback is `t3.small` (2 GB)** —
`pulumi config set instanceType t3.small && make deploy-up` (not free-tier; the budget guard emails,
as expected). This is the deliberate trade: self-hosted observability *fits* the free box **because of
the swap**, with `t3.small` as the pressure valve.

**Storage / cost — all free-tier within the 12-month window.** The **#31 cost fix** shrank the root
to **10 GiB** so root (10) + the durable **`/data`** volume (20) = **30 GiB**, the **entire 30
GB/12-month EBS free tier ⇒ $0** for storage during the window (previously the 30 GiB root alone
filled it and `/data` was ~$1.60/mo). The observability stores live on `/data` at short retention
(~1–2 GB), comfortably inside the 20 GiB. **Cloudflare** (tunnel, DNS, Access ≤ 50 seats, Web
Analytics) and **CloudWatch** (≤ 10 alarms, ≤ 10 custom metrics, basic metrics, SNS email) are all
**free tier**. Net new cost of this entire observability stack: **$0**. (After the 12-month EBS window,
the 30 GiB gp3 is ~$2.40/mo — flagged by the `recollect-free-tier-guard` budget, as expected.)

### What's not covered yet
The deeper **§16** game-design panels — **winrate-when-leading-at-contraction**, **evolutions/match**,
**Throughline completion rate**, **median match length** — need per-match facts the server does **not**
emit today (it emits match *outcome* counters: `recollect_matches_{created,finished}_total`). The
panels are built and provisioned against the intended metric names (e.g.
`recollect_matches_finished_total{led_at_contraction=…}`, `recollect_evolutions_total`,
`recollect_throughline_completed_total`, `recollect_match_length_turns_bucket`) and render *No data*
until those land — a small future server change (out of scope here: "keep any server touch minimal").
P1/P2 winrate, draw rate, and PvE faction winrate **are** live now (derived from the existing
counters). HTTP per-route RED depends on the image's eBPF auto-instrumentation seeing traffic.

---

## Access — how to reach each service (day-2 runbook)

Everything below assumes the stack is already up. Nothing here uses SSH or a public port — the box
has **zero inbound rules**.

### 1. Grafana — `https://grafana.your-domain.com` (behind Cloudflare Access)
The normal path, from any browser:

1. Open **`https://grafana.your-domain.com`** (the `grafanaUrl` stack output). Cloudflare Access
   intercepts the request **before** it reaches the box.
2. You get the **Cloudflare Access login** page (titled for the "Recollect Grafana" app). With the
   default setup (no IdP configured) it offers **"Send me a code"**: enter the **`maintainerEmail`**
   you configured, click send, and Cloudflare emails a **one-time PIN**. Paste the PIN.
   - If you added an identity provider (Google/GitHub/etc.) to your Zero Trust org, you'll instead
     see that **SSO button** — click it and authenticate there. (The email allow-list still applies:
     only the allowed address passes, however it authenticates.)
3. On success Access sets a session cookie (valid **24h**, the app's `sessionDuration`) and forwards
   you to Grafana, which opens **anonymous-Admin** — so you land directly on the dashboards
   (Dashboards → Recollect → *RED service* / *game-design* / *host/box*). No Grafana login.
4. **First-time only:** the very first visit per email may show a Cloudflare consent screen; accept
   it once. If you configured an IdP in Zero Trust, set it up there first (Zero Trust dashboard →
   Settings → Authentication) — the email-PIN method needs **no** setup and works out of the box.

**Add or change who can get in:** edit the allow policy's emails — see
[Granting Grafana access to more people](#granting-grafana-access-to-more-people).

### 2. Grafana fallback — SSM port-forward (if Access is misconfigured / locks you out)
If the Access app/policy is wrong (e.g. the wrong email, or a typo'd domain) you can still reach
Grafana **directly over SSM**, with no public exposure, by port-forwarding the container's `:3000`
to your laptop through the Session Manager tunnel:

```bash
# Open an SSM port-forward from your laptop's localhost:3000 → the box's localhost:3000.
# (The lgtm container publishes Grafana on the box only via the compose network; expose it on the
#  box's loopback first if needed, OR forward to the container — simplest is to curl from on-box.)
INSTANCE=$(cd deploy/pulumi/platform && pulumi stack output instanceId)
aws ssm start-session --region us-east-2 --target "$INSTANCE" \
  --document-name AWS-StartPortForwardingSession \
  --parameters '{"portNumber":["3000"],"localPortNumber":["3000"]}'
# then browse http://localhost:3000  (only works if Grafana is reachable on the box's localhost:3000)
```

If Grafana isn't on the box's loopback (it's on the compose network by default), the most reliable
break-glass is simply to **open a shell and read it from inside**: `make deploy-ssm`, then
`docker exec -it recollect-lgtm-1 wget -qO- localhost:3000/api/health` to confirm it's healthy, and
**fix the Access app** (`pulumi config set maintainerEmail … && make deploy-up`) — the Access path is
the intended one; the port-forward is only to diagnose.

### 3. The box itself — SSM Session Manager (keyless, no SSH, no port 22)
There is **no SSH** and **no inbound port**. Admin is the AWS **Session Manager**:

```bash
make deploy-ssm            # = aws ssm start-session --region us-east-2 --target <instanceId>
# on the box:
sudo tail -f /var/log/recollect-bootstrap.log                      # cloud-init / first-boot progress
docker compose --project-directory /opt/recollect/deploy/compose \
  --env-file /opt/recollect/.env \
  -f /opt/recollect/deploy/compose/docker-compose.deploy.yml ps     # all services incl. lgtm/node-exporter
docker logs recollect-lgtm-1 --tail=50                              # Grafana/Prometheus/Loki/Tempo logs
sudo cat /opt/recollect/.env                                        # the 0600 root-only secrets file (incl. IMAGE_REF)
```

This works because the instance has the **SSM instance role** (`AmazonSSMManagedInstanceCore`) and
the SSM agent ships in AL2023 — your AWS session (SSO or keys, below) is the only credential. (Want
SSH instead? Add a key pair + a port-22 ingress rule to `platform/index.ts`; the tunnel means you don't need
to.)

### 4. CloudWatch alarms + the SNS email subscription
The out-of-band alarms live in **CloudWatch** in `us-east-2`:

- **Confirm the SNS email subscription (one-time, REQUIRED).** After the first `pulumi up`, AWS sends
  a **"Subscription Confirmation"** email to `alarmEmail` (or `budgetEmail`) from
  *no-reply@sns.amazonaws.com*. **Click the "Confirm subscription" link once** — until you do, alarms
  fire but **no email is delivered**. Verify it's confirmed:
  ```bash
  TOPIC=$(cd deploy/pulumi/platform && pulumi stack output alarmTopicArn)
  aws sns list-subscriptions-by-topic --region us-east-2 --topic-arn "$TOPIC"
  #   → SubscriptionArn should be a real ARN, NOT "PendingConfirmation"
  ```
- **See the alarms** (state + history) in the console: **CloudWatch → Alarms** (filter `recollect-`),
  or from the CLI:
  ```bash
  aws cloudwatch describe-alarms --region us-east-2 --alarm-name-prefix recollect- \
    --query 'MetricAlarms[].{name:AlarmName,state:StateValue}' --output table
  ```
- **The custom host metrics** (mem/swap/disk) appear under **CloudWatch → Metrics → Recollect/Host**
  once the CloudWatch agent has run (give it ~5–10 min after boot). If they're missing, check the
  agent on the box: `make deploy-ssm`, then
  `sudo systemctl status amazon-cloudwatch-agent` and
  `sudo cat /opt/aws/amazon-cloudwatch-agent/logs/amazon-cloudwatch-agent.log`.

---

## Prerequisites (install once)

- **Pulumi CLI** — `brew install pulumi` (or see pulumi.com/docs/install).
- **Node 20+** — both Pulumi programs are TypeScript.
- **AWS access** to an account that can create **ECR** + **IAM** (incl. the OIDC provider — FOUNDATION)
  and EC2/IAM/Budgets/**CloudWatch**/**SNS** (PLATFORM) in your region (default `us-east-2`).
  **Use IAM Identity Center (SSO)** — a short-lived **admin** session via `aws sso login` for BOTH
  stages; no long-lived keys to leak or rotate (see [AWS credentials](#aws-credentials-for-pulumi-itself--environment-not-stack-config)).
  Static IAM access keys are the fallback. **After FOUNDATION, CI needs no AWS keys at all** — it
  assumes the FOUNDATION CI role via GitHub OIDC.
- **A Cloudflare account** (PLATFORM only) with **your domain's zone** already added (nameservers
  pointed at Cloudflare). You need the **account ID**, the **zone ID**, and a **least-privilege
  API token** (scoped exactly as below). (This repo ships no real domain — you supply yours as the
  `domain` config; see [Set your deployment config](#set-your-deployment-config).)
- **Docker** is NOT needed on your laptop for either `pulumi up` — **CI builds the server image**
  (FOUNDATION's ECR) and the box **pulls** it. (Docker *is* needed for `make deploy-local` /
  `make deploy-smoke`, the local end-to-end runs below, which build the image locally.)

Install each program's dependencies (once per stack):

```bash
make foundation-install    # = (cd deploy/pulumi/foundation && npm install)
make deploy-install        # = (cd deploy/pulumi/platform   && npm install)
```

---

## The complete list of inputs / secrets / env

Set these with `pulumi config set` (add `--secret` exactly where shown — secrets are stored
**encrypted** in the stack state). Run them from `deploy/pulumi/` after `pulumi stack init`.

### Set your deployment config

**This repo is generic — it ships NO deployment-unique value.** Anything that identifies *your*
instance (your domain, your repo URL, your server image, your Cloudflare account/zone, your maintainer
email, your region) is **not committed to git**; you provide it at deploy time from your terminal, and
it lands in **your gitignored per-stack config** — `deploy/pulumi/platform/Pulumi.<stack>.yaml`
(FOUNDATION's is `deploy/pulumi/foundation/Pulumi.<stack>.yaml`), created by `pulumi stack init` +
`pulumi config set`, gitignored via `Pulumi.*.yaml`. So a public clone of this repo carries zero
specifics, and you (or anyone) configure your own.

What you set on **PLATFORM**, and which are **required** vs **optional** (FOUNDATION's one input,
`githubRepo`, is in [the FOUNDATION runbook](#foundation--run-once-the-ecr-repo--the-github-oidc-ci-push-role)):

| `pulumi config set …` | Req? | Deployment-unique value you supply |
|---|---|---|
| `domain <your-domain.com>` | **required** | YOUR public hostname (no committed default). |
| `repoUrl <https://github.com/yourorg/recollect.git>` | **required** | YOUR fork/clone URL cloud-init clones on the box for the compose files + site build (no committed default). |
| `gitRef <SHA_OR_TAG>` | **required** | The pinned commit/tag to deploy. |
| `serverImage <ecr-url>:sha-<SHA>` | **required** | The ECR image the box PULLS — FOUNDATION's `repoUrl` output at the tag CI pushed (no committed default). |
| `cloudflareAccountId <id>` | **required** | YOUR Cloudflare account ID. |
| `cloudflareZoneId <id>` | **required** | YOUR zone ID (for `domain`). |
| `maintainerEmail you@example.com` | **required** | The email allowed through Cloudflare Access to Grafana. |
| `cfTeamName <team>` | optional | YOUR Zero Trust team name. **Omit ⇒ edge-only Access; set ⇒ also validate the Access JWT at the origin** (R2-1 defense-in-depth — [below](#r2-1--origin-jwt-validation-defense-in-depth)). |
| `region`, `budgetEmail`, `alarmEmail`, … | optional | The rest (sensible defaults; full tables below). |

Plus two **environment** credentials (not stack config — they live in your shell / `~/.aws`, never
git): your **AWS creds** (SSO session or keys) and your **`CLOUDFLARE_API_TOKEN`** (a scoped token).
See [AWS credentials](#aws-credentials-for-pulumi-itself--environment-not-stack-config) and
[Cloudflare credentials](#cloudflare-credentials-for-pulumis-cloudflare-provider--environment) below.

> **None of this is committed.** `Pulumi.<stack>.yaml` (your per-stack config, possibly with
> encrypted secrets), `.env`, and `.env.*` are **git-ignored** (see
> [Never commit secrets](#where-every-secret-lives-and-how-to-rotate)). The full command list is in
> [The exact commands](#the-exact-commands); the per-key reference tables follow.

### AWS credentials (for Pulumi itself — environment, not stack config)
Pulumi's AWS provider reads creds from the environment. **Two options the maintainer weighed —
RECOMMENDED: IAM Identity Center (SSO) / short-lived creds**, because there is nothing static to
leak or rotate (the session auto-expires); a scoped IAM user is the fallback.

**Option A (recommended) — IAM Identity Center (SSO), short-lived session.** The maintainer's SSO is
the **default profile**, so the primary path uses **no `--profile` / `AWS_PROFILE`** — Pulumi reads
the default profile (and its region, us-east-2) automatically:
```bash
aws configure sso              # one-time: set it up as the DEFAULT profile (SSO start URL + region)
aws sso login                  # the DEFAULT profile — Pulumi uses it automatically
# Named profile instead? Log it in and point your shell at it:
#   aws sso login --profile <name> && export AWS_PROFILE=<name>
```
Assign the SSO **permission set** the least-privilege policy below (or, pragmatically for a solo
maintainer account, a broad-but-account-scoped set like `PowerUserAccess` **plus** an IAM-write
allowance for the instance role — `PowerUserAccess` alone can't create the role this stack needs).

**Option B (fallback) — a scoped IAM user with static access keys** (long-lived; you must rotate):
```bash
export AWS_ACCESS_KEY_ID=…
export AWS_SECRET_ACCESS_KEY=…
```

**Least-privilege permissions these deploys need** — the SAME admin session runs both stages
(attach to the SSO permission set or the IAM user; prefer customer-managed policies over `*FullAccess`):

FOUNDATION (run once):
- **ECR** — create + manage the server repo (`ecr:CreateRepository`, `PutLifecyclePolicy`,
  `PutImageScanningConfiguration`, `PutImageTagMutability`, `Describe*`, `DeleteRepository` for
  teardown).
- **IAM (the OIDC provider + the CI role)** — `iam:CreateOpenIDConnectProvider` (+ `Get*`/`Tag*`/
  `Delete*`), and create the CI role with an inline policy (`iam:CreateRole`, `PutRolePolicy`,
  `PassRole`, plus `Get*`/`List*`/`Delete*` for updates/teardown).

PLATFORM (run per release) — everything FOUNDATION needs is NOT required here; PLATFORM needs:
- **EC2 + EBS** — create/describe/terminate the instance, the security group, the durable EBS volume
  + its attachment, read the AMI/VPC/subnet, set IMDS options (`ec2:*` on the resources here, or the
  AWS-managed `AmazonEC2FullAccess` if you want a coarse start).
- **IAM** — create the **instance role** + instance profile and attach the AWS-managed policies (SSM,
  CloudWatch-agent, **`AmazonEC2ContainerRegistryReadOnly`** so the box can pull the image)
  (`iam:CreateRole`, `CreateInstanceProfile`, `AddRoleToInstanceProfile`, `AttachRolePolicy`,
  `PassRole`, plus the `Get*`/`List*`/`Delete*` for updates/teardown). `PassRole` is required so EC2
  can assume the instance role.
- **SSM** — nothing to *create* (the box uses the managed `AmazonSSMManagedInstanceCore`); your
  operator session needs `ssm:StartSession` (+ `TerminateSession`/`ResumeSession`) to run
  `make deploy-ssm`.
- **CloudWatch + SNS** — create the alarms and the SNS topic/subscription (`cloudwatch:PutMetricAlarm`,
  `DescribeAlarms`, `DeleteAlarms`; `sns:CreateTopic`, `Subscribe`, `GetTopicAttributes`,
  `ListSubscriptionsByTopic`, `DeleteTopic`).
- **Budgets** — the two free-tier guardrails (`budgets:ViewBudget`, `ModifyBudget`, plus
  create/delete).

> A tight setup is one **customer-managed policy** granting exactly the actions above, attached to a
> dedicated permission set (SSO) or user (keys). The pragmatic solo-account path is SSO with
> `PowerUserAccess` + an inline IAM-write statement for `CreateRole`/`CreateOpenIDConnectProvider`/
> `AttachRolePolicy`/`PutRolePolicy`/`PassRole` scoped to `recollect-*` roles + the GitHub OIDC ARN.
> Either way, **scope to this one account**, never use the **root** user, and prefer SSO so the
> credential is short-lived. **CI itself needs none of this** — it assumes the FOUNDATION CI role
> (ECR-push only) via OIDC.

### Cloudflare credentials (for Pulumi's Cloudflare provider — environment)
Cloudflare has no SSO for its API, so a **least-privilege, revocable API token** is the best practice
— **never the Global API Key** (it can do anything on your account). Step-by-step in the dashboard:

1. Cloudflare dashboard → **My Profile** (top-right) → **API Tokens** → **Create Token** → **Create
   Custom Token** ("Get started").
2. **Name** it e.g. `recollect-deploy`.
3. Under **Permissions**, add exactly these four rows:
   - **Account** · **Cloudflare Tunnel** · **Edit** — create/manage the named tunnel + read its token.
   - **Account** · **Access: Apps and Policies** · **Edit** — create the Zero Trust Access application
     + the allow policy that gates Grafana.
   - **Account** · **Cloudflare Pages** · **Edit** — create the static-website **Pages project** + its
     apex/`www` custom-domain bindings (PLATFORM now manages these as IaC — see
     [`deploy/site/README.md`](site/README.md)).
   - **Zone** · **DNS** · **Edit** — the proxied CNAMEs PLATFORM creates: the website **apex** + **`www`**
     → the Pages `*.pages.dev`, plus **`play.<your-domain>`** + **`grafana.<your-domain>`** → the tunnel.
4. **Account Resources:** *Include* → **your account** (the one that owns the zone — find its ID on
   the zone's **Overview** page, right rail, **Account ID**).
5. **Zone Resources:** *Include* → **Specific zone** → **your domain's zone** (scope DNS to this one
   zone only — not "All zones").
6. Optionally set **TTL / an expiry** so the token rotates itself. Continue → **Create Token**, copy
   the value **once** (it's shown only at creation), and export it. Revoke it anytime from this page.

```bash
export CLOUDFLARE_API_TOKEN=…   # the scoped custom token above — NEVER a Global API Key
```

You also need the **account ID** (step 4) and the **zone ID** (zone Overview → right rail, **Zone
ID**) as the `cloudflareAccountId` / `cloudflareZoneId` stack config below.

> **This is the PLATFORM provider token** (consumed by `pulumi up` from your shell env) — distinct
> from **CI's** Pages-deploy token, a Pages-only GitHub secret that `wrangler` uploads `dist/` with
> ([`deploy/site/README.md` §3](site/README.md#3-the-residual-manual-steps-what-pulumi-cant-do)). The
> PLATFORM token needs the **Pages · Edit** row to *create the project + domains*; CI's needs only
> **Pages · Edit** to *upload*. You may reuse one token for both, but separate scoped tokens are tidier.

### Granting Grafana access to more people
The Cloudflare Access allow policy includes exactly one email by default (`maintainerEmail`). To add
or change who can reach Grafana, **add emails to the policy** — the canonical (IaC) way is to widen
the `includes` in `index.ts` (the `grafanaAccessPolicy` resource) to a list and `make deploy-up`:

```ts
// in deploy/pulumi/platform/index.ts — the ZeroTrustAccessPolicy "grafana-maintainer":
includes: [
  { email: { email: maintainerEmail } },
  { email: { email: "teammate@example.com" } },
  // or a whole domain: { emailDomain: { domain: "example.com" } },
],
```

(Single-maintainer quick path: `pulumi config set maintainerEmail other@example.com && make
deploy-up` swaps the one allowed address.) Newly-allowed people reach Grafana exactly as in
[Access §1](#1-grafana--httpsgrafanayour-domaincom-behind-cloudflare-access) — visit the URL, enter
their email, paste the one-time PIN. Changes apply on the next `pulumi up`; no box rebuild.

### Where every secret lives, and how to rotate
| Secret | Lives in | Created by | Rotate by |
|---|---|---|---|
| **AWS creds** (SSO session or access keys) | your shell env / `~/.aws` (SSO cache) | you (IAM IC or IAM user) | `aws sso login` again (SSO auto-expires) / rotate the IAM access key |
| **`CLOUDFLARE_API_TOKEN`** | your shell env | you (custom token above) | **Roll** it on the API Tokens page; re-export; re-run `make deploy-up` |
| **on-box Postgres password** | Pulumi state (encrypted) **+** the box's `0600` `/opt/recollect/.env` | **Pulumi generates it** (`random.RandomPassword`) — never an input | delete the random resource / `pulumi up` to regenerate (compose-internal; no published port) |
| **Cloudflare Tunnel connector token** | Pulumi state (encrypted) → injected into the box `.env` | **Pulumi** reads it back from the tunnel | recreate the tunnel via `pulumi up` |
| **`cfBeaconToken`** (Web Analytics) | Pulumi **config secret** (`--secret`, encrypted in state) | you (Cloudflare → Web Analytics) | re-issue in Cloudflare; `pulumi config set --secret cfBeaconToken …` |

> **Never commit secrets — or deployment-unique values.** `Pulumi.<stack>.yaml` (your per-stack
> config — it holds your domain, account/zone ids, region, *and* any encrypted secrets) and
> `node_modules/` are **git-ignored** (`deploy/pulumi/.gitignore`); `.env` + `.env.*` are git-ignored
> at the repo root. So neither secrets nor the values unique to your deployment ever reach git — the
> repo stays generic. The rendered user-data (with the tunnel token + Postgres password) is a
> **tracked Pulumi secret** (encrypted in state) and lands on the box only as the `0600` root-only
> `.env` + the encrypted EBS root. Inspect a secret with
> `pulumi stack output <name> --show-secrets` or, on the box, `make deploy-ssm` →
> `sudo cat /opt/recollect/.env`. The `maintainerEmail`/`alarmEmail`/`budgetEmail` are **not**
> secrets (plain config).

### PLATFORM stack config — non-secret
| Key | Required | Default | What it is |
|---|---|---|---|
| `region` | no | `us-east-2` | AWS region (the maintainer's SSO-profile region). Keep it the **same as FOUNDATION's** so the box pulls the image without cross-region charges. |
| `environment` | no | `production` | The deployment environment, applied as the **`Environment`** tag on every AWS resource ([Tagging](#tagging--every-aws-resource-carries-the-same-set)). Override for a non-prod copy (e.g. `staging`). |
| `instanceType` | no | `t3.micro` | Free-tier size. Use `t3.small` if 1 GB OOMs / swap thrashes (see below). |
| `maintainerEmail` | **yes** | — | The email allowed through **Cloudflare Access** to reach `grafana.<domain>`. The Access app is an allow-list of exactly this address ([Observability](#observability--self-hosted-lgtm-grafana-access-cloudwatch)). |
| `rootVolumeSizeGb` | no | `10` | Instance **root** volume (GiB). 10 + the 20 GiB `/data` = 30 GiB = the whole **EBS free tier ⇒ $0** (the #31 cost fix; durable data is on `/data`). |
| `dataVolumeSizeGb` | no | `20` | Size (GiB) of the **durable** `/data` EBS volume — Postgres' data **+ the observability stores** ([Durable data](#durable-data--surviving-instance-recreation)). Free-tier with the 10 GiB root (see [Cost](#cost--the-free-tier-guardrails)). |
| `swapSizeGb` | no | `4` | Size (GiB) of the swap file cloud-init creates on `/data` (`/data/swapfile`) — RAM headroom for the 1 GB box (the self-hosted observability stack needs it). `vm.swappiness=10` keeps swap a safety net. Counts against `dataVolumeSizeGb`. |
| `grafanaSubdomain` | no | `grafana` | Host part for Grafana ⇒ `grafana.<domain>`. |
| `cfTeamName` | no | — | **R2-1, defense-in-depth.** Your Cloudflare Zero Trust org/team name (the `<team>` in `<team>.cloudflareaccess.com`). **Set** ⇒ the grafana tunnel ingress also validates the Access JWT at the connector (origin-side). **Unset** ⇒ edge-only Access ([R2-1](#r2-1--origin-jwt-validation-defense-in-depth)). |
| `alarmEmail` | no | `budgetEmail` | Email for the **CloudWatch** out-of-band box-health alarms (SNS). Falls back to `budgetEmail`; empty ⇒ alarms exist but don't email. |
| `cpuAlarmThresholdPct` | no | `80` | CPUUtilization alarm threshold (%). |
| `domain` | **yes** | — | The public hostname for **your** deployment (e.g. `your-domain.com`). **Deployment-unique — no committed default**; you set it. |
| `repoUrl` | **yes** | — | Git URL cloud-init clones on the box for the compose files + site build (**your** fork/clone). **Deployment-unique — no committed default.** |
| `gitRef` | **yes** | — | A pinned **commit SHA or tag** to deploy. Never a moving branch. |
| `serverImage` | **yes** | — | The ECR image the box **PULLS** (production no longer builds on-box) — FOUNDATION's `repoUrl` output at the `sha-<commit>` tag CI pushed, e.g. `<acct>.dkr.ecr.<region>.amazonaws.com/recollect-server:sha-<SHA>`. The instance role grants **ECR read-only** so the pull is keyless. **Deployment-unique — no committed default.** |
| `cloudflareAccountId` | **yes** | — | Your Cloudflare account ID. |
| `cloudflareZoneId` | **yes** | — | The zone ID for your domain. |
| `gameSubdomain` | no | `play` | Host part for the **game server** ⇒ `play.<domain>`. The apex/`www` are the static website on **Cloudflare Pages** ([`deploy/site/README.md`](site/README.md)); the game (wss + REST) lives on this sub-route, so the two share the one zone without colliding. |
| `pagesProjectName` | no | `recollect-site` | The **Cloudflare Pages** project name for the static website (direct-upload; CI `wrangler`-uploads `dist/`). Exported as the `pagesProjectName` stack output so CI's `CF_PAGES_PROJECT` var names the same project ([`deploy/site/README.md`](site/README.md)). Generic default — not deployment-unique. |
| `otelEndpoint` | no | `""` | OTLP gRPC endpoint. Empty ⇒ the server exports to the **on-box `lgtm`** (`http://lgtm:4317`, the §11 self-hosted default). Set it to ship **off-box** (e.g. a Grafana Cloud OTLP URL) instead. |
| `budgetEmail` | no | `""` | Email for the AWS Budgets free-tier alerts. Empty ⇒ budgets exist but don't email. |
| `monthlyBudgetUsd` | no | `5` | The monthly cost-budget cap (USD). |

### PLATFORM stack config — secrets (`--secret`)
| Key | Required | What it is |
|---|---|---|
| `cfBeaconToken` | no | Cloudflare **Web Analytics** beacon token (cookieless). Empty ⇒ no beacon. Create it in the Cloudflare dashboard › Analytics › Web Analytics, add a site for `your-domain.com`, copy the token. |

### FOUNDATION stack config
FOUNDATION has no secrets — just one required deployment-unique input + a few defaulted knobs (set
from `deploy/pulumi/foundation/`):

| Key | Required | Default | What it is |
|---|---|---|---|
| `githubRepo` | **yes** | — | The `owner/repo` whose **main-branch** workflows may assume the CI role (pins the OIDC trust `sub`). **Deployment-unique — no committed default.** Also the **`Repository`** tag on every resource ([Tagging](#tagging--every-aws-resource-carries-the-same-set)). |
| `region` | no | `us-east-2` | AWS region (the maintainer's SSO-profile region) — keep it the **same as PLATFORM's**. |
| `environment` | no | `production` | The deployment environment, applied as the **`Environment`** tag on every AWS resource ([Tagging](#tagging--every-aws-resource-carries-the-same-set)). Override for a non-prod copy. |
| `imageName` | no | `recollect-server` | The ECR repository name. |
| `expireUntaggedAfterDays` | no | `14` | Lifecycle: expire untagged images older than this. |
| `keepReleaseImages` | no | `20` | Lifecycle: keep at most this many `sha-…` release images. |
| `githubOidcThumbprint` | no | `""` | OPTIONAL TLS thumbprint for GitHub's OIDC endpoint. Empty ⇒ none (modern AWS verifies GitHub's cert against its trust store and ignores this). Pin one only if your account/region still requires it. |

### Tagging — every AWS resource carries the same set
Both stacks tag **every** AWS resource through the AWS provider's **`defaultTags`** (set once on
`new aws.Provider(...)` — AWS then applies the set to every taggable resource the provider creates,
so there's no per-resource tagging to keep in sync). The common set is:

| Tag | Value | Why |
|---|---|---|
| `Project` | `recollect` (constant) | The product these resources belong to. |
| `Environment` | the **`environment`** config (default `production`) | Slice cost/console by environment; override for a non-prod copy. |
| `ManagedBy` | `pulumi` (constant) | Signals IaC — **don't hand-edit these in the console**; change the code + `pulumi up`. |
| `Stack` | the Pulumi project name (`foundation` / `platform`) | Which half of the deploy owns the resource. |
| `Repository` | the source repo — **reuses** `githubRepo` (FOUNDATION) / `repoUrl` (PLATFORM) | Provenance; deployment-unique, so config-driven, never hardcoded. |

On top of `defaultTags`, each key resource also gets a per-resource **`Name`** tag (the console's
display name — `defaultTags` can't set it): FOUNDATION → `recollect-ecr`, `recollect-github-oidc`,
`recollect-ci-role`; PLATFORM → `recollect-server` (the EC2 box), `recollect-data-volume` (the
durable EBS), `recollect-sg`, `recollect-instance-role` / `recollect-instance-profile`,
`recollect-alarms` (SNS), the seven `recollect-*` CloudWatch alarms, and the two
`recollect-*-budget`s.

> **The Pulumi state bucket carries the set too.** The bootstrap `create-state-bucket.sh`
> (`make pulumi-state-bucket`) applies the same `Project` / `Environment` / `ManagedBy` / `Name`
> tags to the S3 state bucket, plus **`Stack=state-backend`** — it underlies *both* stacks and is
> bootstrap-created, never part of a `pulumi up`, so **don't `pulumi destroy` it**. `Repository`
> stays config-driven there too (set `REPOSITORY=owner/repo` on the script to add it; omitted by
> default rather than hardcoded).

> **Cloudflare resources aren't AWS-tagged.** The Cloudflare provider takes no AWS tag set. Where a
> Cloudflare resource supports a free-form **comment** — the four DNS records (website apex + `www`,
> plus `play.` + `grafana.`) — Pulumi sets a consistent marker (`managed by Pulumi — recollect`). The
> **tunnel**, the **Access app/policy**, and the **Pages project + custom domains** have no
> comment/tag field, so they aren't marked (their `name`s already read `recollect` / `Recollect
> Grafana …` / `recollect-site`).

> **The on-box Postgres password is NOT an input you provide.** Postgres lives only on the compose
> network (no published port), so its password is a purely internal DSN credential — Pulumi
> **generates** it (`random.RandomPassword`, 40 URL-safe chars), keeps it encrypted in stack state,
> and injects the same value into both the compose Postgres and the server's `DATABASE_URL` via
> cloud-init. You never set it. If you ever need it (a `docker exec … psql` on the box), it's in the
> box's 0600 root-only `/opt/recollect/.env`, or `pulumi stack output postgresPassword --show-secrets`.

> **The Cloudflare Tunnel token is NOT an input you provide.** Pulumi **creates** the named
> tunnel, generates its secret, reads back its connector token, and injects that token into the
> box's cloud-init — so there is nothing to copy/paste. (It is surfaced, encrypted, as the
> `cloudflaredToken` stack output for debugging only.)

### The exact commands
The full PLATFORM config reference (the [PLATFORM runbook](#platform--run-per-release-the-box-that-pulls-the-image)
above is the quick path):

```bash
cd deploy/pulumi/platform
pulumi stack init prod            # one-time — or skip it: the `make deploy-*` preflight inits/selects it

# required — all DEPLOYMENT-UNIQUE; the repo ships no real value, you supply yours here. They land
# in your gitignored Pulumi.<stack>.yaml (never git).
pulumi config set domain                 your-domain.com                       # YOUR public hostname
pulumi config set repoUrl                https://github.com/yourorg/recollect.git  # YOUR fork/clone (compose + site)
pulumi config set gitRef                 <COMMIT_SHA_OR_TAG>
pulumi config set serverImage            <ECR_REPO_URL>:sha-<COMMIT_SHA>        # FOUNDATION repoUrl @ CI's tag — the box PULLS this
pulumi config set cloudflareAccountId    <CF_ACCOUNT_ID>
pulumi config set cloudflareZoneId       <CF_ZONE_ID>
pulumi config set maintainerEmail        you@example.com   # the Grafana Access allow-list
# (the on-box Postgres password is generated by Pulumi — nothing to set)

# optional (shown with their defaults)
pulumi config set region                us-east-2          # the maintainer's SSO-profile region
pulumi config set environment           production         # the `Environment` tag on every AWS resource
pulumi config set instanceType          t3.micro
pulumi config set rootVolumeSizeGb      10
pulumi config set dataVolumeSizeGb      20
pulumi config set swapSizeGb            4
pulumi config set grafanaSubdomain      grafana
pulumi config set gameSubdomain         play               # the game server ⇒ play.<domain> (apex/www = Pages)
pulumi config set pagesProjectName      recollect-site     # the Cloudflare Pages project (== CI's CF_PAGES_PROJECT)
# R2-1 defense-in-depth (omit ⇒ edge-only Access; set ⇒ also validate the Access JWT at the origin):
pulumi config set cfTeamName            <YOUR_ZERO_TRUST_TEAM_NAME>   # the <team> in <team>.cloudflareaccess.com
pulumi config set alarmEmail            you@example.com    # CloudWatch alarms (defaults to budgetEmail)
pulumi config set cpuAlarmThresholdPct  80
pulumi config set otelEndpoint          ""                 # empty ⇒ on-box lgtm; set ⇒ off-box
pulumi config set budgetEmail           you@example.com
pulumi config set monthlyBudgetUsd      5
pulumi config set --secret cfBeaconToken  <CF_WEB_ANALYTICS_TOKEN>
```

---

## Deploy — step by step (PLATFORM)

(FOUNDATION must already be up and CI must have pushed an image — see
[the FOUNDATION runbook](#foundation--run-once-the-ecr-repo--the-github-oidc-ci-push-role).)

```bash
# 0. (once) install deps + log in to a Pulumi backend (Pulumi Cloud or `pulumi login --local`)
make deploy-install
pulumi login            # or: pulumi login --local

# 1. set the config above (AWS_/CLOUDFLARE_ env + `pulumi config set …`, incl. serverImage)

# 2. type-check (no cloud calls)
make deploy-typecheck

# 3. preview the plan — review every resource before creating anything
make deploy-preview

# 4. create the infra (EC2 + Cloudflare tunnel + DNS + budgets) — the box PULLS serverImage
make deploy-up

# 5. read the outputs (site URL, instance id, the SSM session command)
make deploy-outputs
```

`pulumi up` returns in ~1 minute, but the **box keeps working** for a few more: cloud-init
installs Docker, clones the repo (for the compose + site), logs in to ECR, **pulls the server image**
and **builds the wasm site** (the site is the only on-box build now — the server is pulled, not
compiled), then starts the stack. Watch it:

```bash
make deploy-ssm                                   # keyless shell on the box (SSM)
#   on the box:
sudo tail -f /var/log/recollect-bootstrap.log     # cloud-init progress (ECR login + pull + up)
docker compose --project-directory /opt/recollect/deploy/compose \
  --env-file /opt/recollect/.env \
  -f /opt/recollect/deploy/compose/docker-compose.deploy.yml ps
```

When `cloudflared` is up and the server answers `/healthz`, **https://your-domain.com** serves
the site and you can play. Quick Play vs. the AI works immediately; hosting a 1v1/2v2 mints invite
codes.

### Redeploying a new build
**Push first, then deploy:** merge to `main` so CI builds + pushes `${ECR_REPO_URL}:sha-<commit>`.
Then either re-point the live box in place, or replace it:
```bash
make deploy-ssm
sudo recollect-update <NEW_SHA_OR_TAG>     # re-points .env IMAGE_REF → :<ref>, ECR-pulls, recreates + prunes
```
Or change `gitRef` + `serverImage` and `make deploy-up` — `userDataReplaceOnChange` re-provisions a
fresh box that pulls the new image.

### Tear down
```bash
make deploy-destroy        # PLATFORM: destroys the EC2 box + Cloudflare tunnel/DNS (asks: type 'unwrite')
make foundation-destroy    # FOUNDATION: destroys the ECR repo + OIDC provider + CI role (rarely — only
                           #             when retiring the whole deployment; CI can no longer push after)
```
Destroy **PLATFORM** routinely; leave **FOUNDATION** up across releases (it is the registry + the CI
trust). Only tear FOUNDATION down when retiring the deployment entirely.

---

## Run it locally first (recommended)

You can run the **exact same single-origin stack** on your laptop — minus the Cloudflare Tunnel —
and play the real website end-to-end. This validates the deploy artifacts (the site build, the
static-serving server, on-box Postgres) with no cloud, creds, or tunnel:

```bash
make deploy-local          # builds + starts; serves at http://localhost:8080
#   → open http://localhost:8080  (the site; "Launch the game" → the wasm client)
make deploy-local-logs     # tail server logs
make deploy-local-down     # stop (keeps the Postgres volume)
```

This layers two overlays on the base deploy compose: the **BUILD overlay**
(`deploy/compose/docker-compose.build.yml`) so the server image is **compiled locally** (production
omits it and PULLS from ECR — the [FOUNDATION/PLATFORM split](#two-pulumi-stages-foundation-once--platform-per-release)),
then the **local overlay** (`deploy/compose/docker-compose.local.yml`) which publishes the server
port, points the client origin at `http://localhost:8080`, supplies a **local-only** Postgres
password, and scales `cloudflared` + the observability stack to zero. So **`make deploy-local` /
`make deploy-smoke` need no ECR, no AWS creds** — they build the image right there; only the real box
pulls.

> **Lighter local loops** (no Docker) still work and are documented in `docs/operations.md`:
> `make server` (API on :8080) + `make site-serve` (site on :8000); the in-browser client's
> *server* field defaults to `localhost:8080`. `make deploy-local` is the one that mirrors
> production's single-origin serving.

### Smoke-test the image before launch

`make deploy-local` lets *you* click around; **`make deploy-smoke`** proves the built artifact
works **without a human** — run it before any real `make deploy-up` (and in CI on an x86_64 runner):

```bash
make deploy-smoke          # build the deploy images → up → smoke the site + game + journal → tear down
```

It runs `deploy/smoke.sh`, which brings up the **same local stack** and then asserts the running
image **as a black box** (only HTTP/ws from the host — exactly what a browser or the `recollect`
CLI sees):

- **Website (single origin).** `GET /` returns 200 with the title + nav; the wasm **play client
  under `/client/`** and its `recollect-web.js` / `recollect-web_bg.wasm` assets actually serve
  (the exact **#96** trunk-boot class where `/client/` 404'd its own assets — a 200 index alone is
  *not* enough); `GET /healthz` is `ok`. One origin, one server image.
- **The game (over ws).** It mints a real PvP match via `POST /matches` and drives **both seats**
  with two headless `recollect online join --json` clients (`deploy/smoke_game.py`). It asserts the
  match is created, **moves apply over the wire**, the match advances (to a result, or a healthy
  move budget), and — the **redaction** invariant — a client's view never carries the opponent's
  hand (only a count), and the two seats are dealt different hands.
- **The journal.** A `journal_events` row exists in the on-box Postgres — proving the
  **Postgres-authoritative** append-before-ack path works inside the image.

It **always tears the stack down** (a bash trap, even on failure / Ctrl-C), **dumps the server
logs on failure**, and is **idempotent + re-runnable**. Needs Docker, `jq`, and `python3`; it
builds the `recollect` CLI itself to use as the external client. Knobs (env): `HEALTH_TIMEOUT`,
`MOVE_BUDGET`, `KEEP_UP=1` (leave it running to debug). A passing `make deploy-smoke` is the
go/no-go gate that the deploy artifact serves the site, plays a game, and journals it.

---

## Cost & the free-tier guardrails

This is **free-tier EC2 + free Cloudflare + free CloudWatch + a domain (~$10/yr)** — including the
**entire self-hosted observability stack at $0 net new cost** (it runs on the existing box + swap; no
SaaS bill). Two AWS Budgets are created by Pulumi so a mistake can't silently run up a bill:

- **`recollect-monthly`** — a cost budget at `monthlyBudgetUsd` (default $5); emails at 80% actual
  and 100% forecast (when `budgetEmail` is set).
- **`recollect-free-tier-guard`** — a $1 guard on *Usage* charges; emails the moment a
  non-free-tier charge appears (e.g. the box was bumped to a paid size, or egress overran).

**Storage — now $0 in the free-tier window (the #31 fix).** AWS's EBS free tier is **30 GB of gp3 for
12 months**. The root was **shrunk from 30 → `rootVolumeSizeGb` GiB (default 10)** so root (10) + the
durable **`dataVolumeSizeGb`** (default 20) = **exactly 30 GiB**, the whole free tier ⇒ **$0** for
storage during the window (previously the 30 GiB root alone filled it and `/data` cost ~$1.60/mo). The
durable data — Postgres + the observability stores at short retention (~1–2 GB) — lives on `/data`, so
the small root is ample. There are **no EBS snapshots** (none are created), so no snapshot cost. After
the 12-month window the 30 GiB gp3 is ~$2.40/mo — flagged by `recollect-free-tier-guard`, as expected.

**Observability / Access / CloudWatch — all free tier.** The on-box LGTM stack + node-exporter are
just containers (no AWS/SaaS charge). **Cloudflare** Tunnel, DNS, **Zero Trust Access** (≤ 50 seats),
and Web Analytics are free. **CloudWatch** stays inside the free allowance: **7 alarms** (≤ 10), **4
custom metrics** (≤ 10), **basic 5-minute** metrics (no detailed monitoring), and **no Logs shipping**
(logs live in on-box Loki); **SNS** email is free at these volumes.

**If 1 GB RAM OOMs / swap thrashes** (server + Postgres + cloudflared + **the LGTM stack** on a
`t3.micro` is tight — the LGTM container is the heavy tenant), the first relief is the **4 GB swap file
on `/data`** (`swapSizeGb` — [Durable data](#durable-data--surviving-instance-recreation)), which the
self-hosted observability stack is explicitly sized to lean on (see
[the RAM math](#does-it-fit-1-gb--swap--free-tier)). If swap thrashes under steady load, the fallback
is **`t3.small`** (2 GB): `pulumi config set instanceType t3.small && make deploy-up`. The
**`recollect-swap-high` CloudWatch alarm** + the host dashboard's Swap-used % are the tells (also watch
the OOM-killer in the bootstrap log / `dmesg`). (`t3.small` is not free-tier — the budget guard emails;
that's expected.)

---

## Files in this directory

```
deploy/
├── README.md                              ← this file (the GAME-SERVER deploy)
├── site/README.md                         the static WEBSITE deploy: Pulumi IaC (the PLATFORM Pages project + apex/www domains + DNS) + GitHub CI (`make site` → dist/ → `wrangler pages deploy`); SEPARATE from this box, apex/www vs the game's play.<domain>
├── smoke.sh                               `make deploy-smoke`: black-box smoke of the built image (site + game + journal), always tears down
├── smoke_game.py                          the game half of the smoke — drives a real PvP match via two headless `recollect online --json` clients
├── pulumi/
│   ├── foundation/                        FOUNDATION (run ONCE): the account-level scaffolding
│   │   ├── index.ts                       the IaC: ECR repo (scan-on-push, immutable tags, lifecycle) + GitHub OIDC provider + the scoped CI push role (ECR-push only, trusted to this repo's main). Outputs repoUrl + ciRoleArn
│   │   └── Pulumi.yaml  package.json  package-lock.json  tsconfig.json  .gitignore (Pulumi.*.yaml stack config, node_modules/, bin/)
│   └── platform/                          PLATFORM (run PER RELEASE): the box that pulls the image
│       ├── index.ts                       the IaC: EC2 + SG + SSM/CloudWatch/ECR-read-only role + durable EBS data volume + Cloudflare tunnel/DNS (game on play.<domain>) + Access(app+policy, optional R2-1 origin-JWT) for Grafana + the static-website Cloudflare PAGES project + apex/www domains + DNS + budgets + CloudWatch alarms/SNS
│       ├── user-data.sh                   cloud-init: mount /data (format-only-if-empty) + swap on /data + Docker + CloudWatch agent + ECR login + PULL server image + compose up (server/postgres/cloudflared/lgtm/node-exporter/blackbox), idempotent
│       └── Pulumi.yaml  package.json  package-lock.json  tsconfig.json  .gitignore (Pulumi.*.yaml stack config, node_modules/, bin/)
└── compose/
    ├── docker-compose.deploy.yml          the deploy stack (postgres + site-builder + server [PULLED from ECR] + cloudflared + lgtm + node-exporter + blackbox synthetic-probe exporter)
    ├── docker-compose.build.yml           BUILD overlay (local/smoke only): adds the server `build:` block back so the image is compiled locally (prod pulls)
    ├── docker-compose.local.yml           local overlay (no tunnel; observability + blackbox scaled to zero; serves at :8080)
    ├── Dockerfile.site                     builds the static site + wasm client into the shared volume
    ├── build-site.sh                       the shared site-build steps (catalog → trunk → bake origin + beacon)
    └── observability/                      self-hosted §11 stack as code:
        ├── grafana/provisioning/dashboards/recollect.yaml   the dashboard provider (mounted into lgtm)
        ├── grafana/provisioning/alerting/*.yaml             synthetic-monitoring ALERT RULES (internal + external groups) + contact-point/policy (placeholders — no real endpoint in git)
        ├── grafana/dashboards/*.json                        RED-service · game-design · host/box · synthetic-monitoring dashboards
        ├── prometheus/prometheus.yaml                       upstream OTLP/storage + node-exporter + the Blackbox synthetic-probe scrape jobs (internal + the opt-in external edge job)
        ├── prometheus/external-targets/                     external-probe file-SD targets — EMPTY in git (.example + .gitkeep); render-external-targets.sh writes the gitignored live public-edge.yaml from your domain
        ├── prometheus/render-external-targets.sh            renders the live external-probe targets from OBS_PUBLIC_DOMAIN (turns external monitoring on; off ⇒ inert)
        ├── blackbox/blackbox.yml                            the Blackbox exporter probe modules (http/https/tcp synthetic checks)
        ├── loki/loki-config.yaml                            upstream + a compactor (retention actually deletes)
        ├── tempo/tempo-config.yaml                          upstream + a compactor block_retention
        └── cloudwatch-agent.json                            the CloudWatch agent config (custom mem/swap/disk metrics)
```

> The repo-root **`Dockerfile`** (the hardened `--profile dist` server image) and
> **`docker-compose.yml`** (the dev stack: Postgres + Grafana LGTM, `make dev-up`) stay at the root —
> they're shared contracts: **CI builds the server from that one Dockerfile** for both GHCR (the
> dev/kind-integration registry) and **ECR** (`deploy-image.yml` → the production registry the box
> pulls), and `make deploy-local`/`make deploy-smoke` build it locally via the BUILD overlay. The
> deploy compose references the root Dockerfile by path (build overlay) or the ECR image by ref
> (prod). The Kubernetes/Helm path (`deploy/helm/`) is the §10.2 *scale* target, for later — not this
> launch.

See also: `docs/operations.md` (local stack + make targets) and
`docs/tech_design.md` §10–§11 (the hosting + observability plan this implements).
