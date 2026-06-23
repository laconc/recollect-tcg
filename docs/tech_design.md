# RECOLLECT — Technical Design v1
**Engine, server, clients, infrastructure, and test strategy · companion to the design law `design.md`**

Status of the accompanying repo: everything in §15/M0 compiles and its tests pass (24 green, 4 deliberately-red M1 backlog tests, 2,000-match balance sim runs).

---

## 1. Goals and non-negotiables

1. **One deterministic rules core, embedded everywhere.** Server is authoritative; iOS/Android/web embed the *same* `recollect-core` for instant previews and offline play. Divergence between client preview and server ruling is a bug class we eliminate structurally, not socially.
2. **Cheating is defeated by architecture, not detection.** Clients hold no secrets, send only commands, and receive only their own redacted view. Detection (stats, attestation) is a second layer, never the first.
3. **Replayability is a feature and the audit log.** `seed + command log ⇒ exact state hash`, on every platform, across versions (via versioned snapshots + upcasters). Every ranked match is verifiable after the fact.
4. **TDD throughout.** Tests are written as the executable form of the game spec; red tests for unbuilt features live in the repo (`m1_backlog.rs`) as the work queue.
5. **No gambling-adjacent infrastructure anywhere** — no real-money trade hooks, no odds machinery. (Echoes the game spec's ethics line.)
6. **Audience is 8+** — COPPA/GDPR constraints shape telemetry and accounts from day one (§13.4, red team T-13).

---

## 2. Architecture overview

```
        iOS / Android (UniFFI shells, M4)        Web (wasm32, trunk)
                 │  embeds                            │  embeds
                 ▼                                    ▼
        ┌─────────────────────  recollect-core  ─────────────────────┐
        │  deterministic, sans-I/O rules engine (no clocks, floats,  │
        │  HashMaps, threads). Commands in → Events out. Per-seat     │
        │  views by construction. Seeded RNG lives inside state.      │
        └──────────────────────────┬──────────────────────────────────┘
                                   │ embedded by
                                   ▼
   wss (ALB) ──► recollect-server (axum) ──► Postgres: append-only match
   per-seat        single writer per match        events + snapshots (truth)
   tokens          validates via the same core
                        │
                        └────────► Valkey: matchmaking queues, presence,
                                   WS routing, rate counters (ephemeral ONLY)
   OTLP ──► Grafana Alloy ──► Tempo (traces) / Loki (logs) / Mimir (metrics)
```

Crate layout (exists today):

| Crate | Role | State |
|---|---|---|
| `recollect-core` | Rules engine: state, commands→events, combat, contraction, scoring, views, stable hash, RNG | M0 vertical slice, 18 tests + 4 red M1 tests |
| `recollect-protocol` | Versioned wire envelope (`ClientMsg`/`ServerMsg`), seq numbers | Done for M0 |
| `recollect-server` | axum WS sessions over the core; idempotency; per-seat fan-out | M0 in-memory; PG/Valkey in M1 |
| `recollect-bot` | Random-legal headless player: sims, integration, load | Done; 2k-match sim wired |
| `recollect-web` | wasm client: `LocalGame` hotseat + Canvas2D shell (trunk) | M0 shell; renderer next |

---

## 3. The deterministic core (doctrine)

Hard rules, enforced by the determinism tests (same seed ⇒ identical events;
the fuzz replay arm and the `recollect-verify` model-check) and review:

- **No clocks** (`Instant`, `SystemTime`), no threads, no I/O, no network in `recollect-core`. Time is a *round counter*; anything temporal arrives as a command/event from the host.
- **No floats** in state or rules math — `f32`/`f64` round differently across wasm/x86/ARM. All math is integer. Rendering may float; rendering is presentation.
- **No `HashMap`/`HashSet`** in state — iteration order is nondeterministic. `Vec` + `BTreeMap` only.
- **All randomness through one seeded RNG** (`rng::Rng`, xoshiro256** + splitmix64, hand-rolled: zero deps, integer-only, identical on every target). The RNG state serializes *inside* `GameState`, so a mid-match snapshot resumes bit-identically (tested).
- **Commands in, Events out.** `Engine::apply(seat, Command) -> Result<Vec<Event>, Reject>` is the only mutation path. Commands are intents (validated, rejectable); Events are facts (the append-only log). This is the event-sourcing `decide/evolve` shape — relevant to the Ironstate discussion below.
- **Views by construction.** `view_for(engine, seat)` returns `PlayerView`, whose *types* contain no field that could carry the opponent's hand or either deck's order. Redaction that doesn't exist in the type system can't regress.
- **Stable hashing.** `state_hash()` = FNV-1a over canonical JSON today; BLAKE3 over a canonical binary encoding before ranked play. Hash-chain of per-step hashes is the replay-verification primitive.

---

## 4. Ironstate assessment (v0.4 spec, as uploaded)

**Question asked:** does Ironstate meet our state-machine requirements, and if not, how do we improve it?

**Verdict: yes for the service-layer lifecycle machines; no for the rules engine as currently specified — and the gap is closable with a v0.5 feature set that fits Ironstate's own philosophy.**

### 4.1 Where Ironstate fits Recollect today
These are *exactly* the enum-states / event-driven / invariant-rich machines Ironstate was built for, and we should adopt it for them in M1:

- **Match session lifecycle**: `Created → AwaitingPlayers → MulliganWindow → InProgress → {Complete, Abandoned} → Archived`. Event kinds map beautifully: `#[event_kind = "client"]` vs `"server"` vs `"operator"` distinguishes player commands from internal timers from admin actions — the same pattern as the spec's Deploying/Held example.
- **Matchmaking ticket**: `Queued → Proposed → Accepted/Declined → Matched/Expired`, with invariants like "a ticket is never matched twice" and a reference model for queue fairness via `model_test!()`.
- **WebSocket connection**: `Connecting → Authenticated → Live → Draining → Closed`, with `analyze!()` proving no deadlock states and the stateright bridge checking "every connection eventually closes."
- **Account/moderation/purchase flows** later.

The sans-I/O law (spec §2.4) is the same discipline our core already follows, the verification ladder gives these machines cheap assurance, and `MachineMetadata` + mermaid output documents them for free. This is a genuine fit, not a courtesy.

### 4.2 Where it does not fit: the rules engine
Measured against the engine we just built, the v0.4 spec is missing six things:

1. **No context parameter / randomness contract.** `fn transition(&self, &Event) -> Option<S>` admits no `&mut Rng`, no card catalog, no config. Recollect's transitions shuffle decks, draw cards, and consult static card data. Threading these through state is possible but wrong (catalog in every snapshot) and threading them through events only (pre-rolled randomness as event payloads) pushes the RNG into every host.
2. **No command/event split.** Ironstate's "event" is the *input*; there is no notion of validated intents producing emitted facts. Our server's anti-cheat shape is `Command → Vec<Event>`; listeners (observational, non-replayable) cannot express it. The spec's own replay claim ("replayable from an event log") has no journal API.
3. **Whole-state-by-value transitions on a fat struct.** Game state is one large struct, not an enum of thin variants. Modeling it as a single data-carrying variant makes `analyze!()` vacuous (the spec admits variant-level collapse) and clones the world per step.
4. **No hidden-information / projection concept.** Per-audience redaction is our single most important correctness property and the spec has no vocabulary for it.
5. **Typed errors and wasm posture.** `TransitionError` carries `String` names — fine for workflows, unusable for protocol error codes. `TransitionRecord.timestamp: Instant` is a std type that panics on `wasm32-unknown-unknown`; the injectable clock helps but the type and a wasm CI commitment don't exist.
6. **No versioned restore.** `Machine::restore` validates "known variant" only; live-service deploys need `version = N` snapshots with upcasters.

### 4.3 Proposed Ironstate v0.5 improvements (in priority order)
Each extends the crate's existing identity ("the definition is the test") rather than fighting it:

1. **Aggregate mode (`decide`/`evolve`)** — a second trait alongside `TransitionRules`:
   ```rust
   trait AggregateRules {
       type Command; type Event; type Ctx; type Reject;
       fn decide(&self, cmd: &Self::Command, ctx: &mut Self::Ctx)
           -> Result<Vec<Self::Event>, Self::Reject>;
       fn evolve(&self, event: &Self::Event) -> Self;
   }
   ```
   `test!()`/`model_test!()`/stateright then generate *commands* and fold *events*; invariants run over folded states; `Ctx` (seeded RNG + static data) is owned by the harness and recorded, preserving purity and replay. This one feature makes Ironstate suitable for game engines, order systems, and anything event-sourced.
2. **Verified projections** — `#[derive(Project)]` with `#[hidden]` field annotations and a `leak_test!(Machine, Audience)` macro that property-tests serialized projections for bytes derived from hidden fields. "Verified redaction" is squarely on-brand for the verification ladder.
3. **Determinism rung** — `determinism_test!(Machine, seed)` replaying one sequence under two builds and diffing a `#[derive(StableHash)]`; a `deterministic` feature that emits teaching errors when state contains `f32/f64/HashMap`. (Ladder level 3.5.)
4. **wasm32 commitment** — CI target `wasm32-unknown-unknown`; make the record timestamp generic (`type Stamp = ()` by default) so core types are target-clean.
5. **Typed errors** — `TransitionError` generic over state/event discriminants (Display keeps the teaching text).
6. **Journal API** — `Journal<S>`: append, snapshot-every-N, replay; gives the replay claim a first-class type and gives us the PG event-store adapter shape for free.
7. **Versioned restore** — `#[state_machine(version = 3)]` + `restore_versioned` with upcaster chain.
8. **`analyze!()` honesty** — when a machine is one fat variant, say so in the report and point to the stateright `with_domain` path instead of printing a vacuous "1 state, no deadlocks ✓".

### 4.4 Our adoption decision
- **M1**: adopt Ironstate for SessionLifecycle, MatchmakingTicket, and ConnectionState, with `model_test!()` + the stateright bridge in CI.
- **Rules engine**: stays hand-rolled but Ironstate-shaped (sans-I/O, decide/evolve, journaled, seeded). If v0.5 ships aggregate mode, migrating is mechanical — our `apply` already has the target signature.

---

## 5. Protocol

JSON for v0 playtesting (debuggable in browser devtools), binary (postcard or CBOR) later behind the *same* envelope. Every message carries `v`; servers reject unknown majors, clients surface a forced update.

- `ClientMsg::Hello { v, match_token }` — short-lived per-seat credential, never an account token.
- `ClientMsg::Cmd { v, seq, command }` — `seq` is strictly increasing per seat; the server rejects `seq <= last_seq` (idempotency + anti-replay, tested in `session.rs`).
- `ServerMsg::{Welcome, Applied, Rejected, Update, Pong, Error}` — state always travels as the recipient's own `PlayerView`. The raw `GameState` has no route to a socket; code review rule + the redaction tests enforce it.
- Trace context (`traceparent`) rides as an optional envelope field in M2 so a tap on a phone is one trace through ALB → server → engine → Postgres.

## 6. Server design

**Single writer per match.** One in-process task owns each live match; all commands for that match serialize through it. **LANDED (#86):** the actor task owns the `Session` by value (no mutex), receiving commands over an mpsc channel and fanning per-seat frames out over per-seat mpsc senders — see `recollect-server/src/actor.rs`. (M0 used a mutex'd `Session` + a lossy broadcast; the actor retired both — T-9/T-10 below.) Horizontal scale = matches sharded across pods.

**Writer election & failover (M1):** the authority is a **Postgres advisory lock** on `hash(match_id)` — whoever holds it may append. Valkey stores only a routing hint (`match:{id} → pod`) with TTL. If a pod dies, the lock releases, another pod restores the latest snapshot + replays the event tail, takes the lock, and play resumes; the snapshot-resume determinism test already proves the engine side of this. Any cross-system write carries a **fencing token** (the event `seq`) so a zombie pod's stale writes fail the `seq` uniqueness constraint.

**Persistence (Postgres = truth):**
```sql
create table matches (
  id uuid primary key,
  ruleset_version int not null,
  seed_commitment bytea not null,        -- sha256(seed || salt), published at create
  seed_reveal bytea,                     -- set at match end (commit–reveal, §9)
  status text not null,
  snapshot jsonb, snapshot_seq bigint,   -- every 50 events
  created_at timestamptz not null default now()
);
create table match_events (
  match_id uuid references matches(id),
  seq bigint not null,
  seat smallint,                          -- null for system events
  command jsonb,                          -- the validated input, if any
  events jsonb not null,                  -- facts emitted by the core
  state_hash bytea not null,              -- replay verification chain
  created_at timestamptz not null default now(),
  primary key (match_id, seq)
);
```
Append-only; sqlx with compile-time-checked queries; no ORMs. *(The sketch above is
the design target. As built, the authoritative event journal is the generic
`journal_events`/`journal_snapshots` schema in `recollect-journal-postgres::store`; the
**cross-restart recovery** state — what a restart needs to rebuild a live match's routing
and re-authorise a seat — lives in `match_registry`: the seat-token **hashes**
`token_{a,b,a2,b2}_hash` and the commit–reveal **`seed_salt`**, both `BYTEA` (#85-fu), so
no clear-text seat credential is at rest and a recovered match honours its original
commitment. The salt is secret until the end-of-match reveal — it never enters a view or
event.)*

**Valkey = ephemeral only** (red team T-3): matchmaking queues (sorted sets by rating/wait), presence heartbeats, WS routing hints, rate-limit counters, later spectator pub/sub. AUTH + TLS, private subnets, and the invariant: *if Valkey vanished, no match state would be lost* — everything in it is reconstructible.

## 7. Web client (wasm) and native shells

- **M0 (exists):** `recollect-web` exposes `LocalGame` (hotseat over the real engine) via wasm-bindgen; `trunk serve` gives an in-browser playtest with zero server. Rendering is Canvas2D: a 5×5 grid, stains as translucent washes, combat previews from `legal_commands()` — playtesting beats poetry.
- **M2:** networked play via the protocol crate over `WebSocket`; combat forecasts computed locally by the embedded core and *verified* against server `Applied` views (any mismatch is a telemetry alarm — it would mean nondeterminism).
- **M3:** wgpu ink renderer — WebGPU where available, WebGL2 fallback; wasm budget ≤ 3 MB gzipped (red team T-5).
- **M4:** iOS/Android shells over the same core via UniFFI; App Attest / Play Integrity attach here (§9).

## 8. Local development (tiered) and environments

The user asked for kind locally; the red team pushed back on *kind for every loop* (T-4) and we landed on tiers — kind stays, but as parity, not friction:

1. **Tier 1 — seconds:** `make test`, `make sim`. Pure logic; this is where TDD lives.
2. **Tier 2 — services:** `make up` → Postgres, Valkey, and `grafana/otel-lgtm` (Grafana+Tempo+Loki+Mimir+OTLP in one container) on localhost; `make server` against them. Sub-second rebuilds.
3. **Tier 3 — cluster parity:** `scripts/kind-integration.sh` builds the hardened image, loads it into kind, applies the restricted-PSS kustomize overlay. Used before merging anything that touches deploy/runtime behavior.
4. **CI — ephemeral kind:** `scripts/kind-integration.sh` creates a throwaway cluster per run (random name, trap-deleted), deploys, smoke-tests health + match creation; M1 extends it with a bot Job playing full matches over WS and a **chaos step**: kill the pod mid-match, assert snapshot-resume hash equality.

Environments: local (kind) → staging (EKS, Terraform) → prod (EKS). Staging stands up at **M2**, not before — burning AWS spend before the game is proven fun is the wrong order (red team T-12).

## 9. Security & anti-cheat threat model

Hidden information and server authority do most of the work; everything else is hygiene. Each row is (threat → mitigation → the test that proves it).

| # | Threat | Mitigation | Proven by |
|---|---|---|---|
| 1 | Modified client sends illegal moves | Server validates every command through the same core | `acting_out_of_turn_is_rejected`, rule rejects, authz matrix (L10) |
| 2 | Client reads opponent hand / deck order / fabrication identity from memory or wire | Secrets never leave the server: per-seat views whose *types* can't carry them | `redaction.rs` both tests, session-boundary view test, L10 wire-leak scanner |
| 3 | Replayed or duplicated commands | Strictly-increasing per-seat `seq` | `sessions_reject_replayed_sequence_numbers` |
| 4 | Token theft / seat hijack | Short-lived per-seat match tokens, 256-bit CSPRNG, **stored hashed everywhere** — the running server (`SeatToken`) and the `match_registry` recovery row alike hold only `sha256(token)` (#85-fu), so neither a memory dump nor a DB read yields a live credential; bound to match+account; TLS 1.3 everywhere; cert pinning on mobile | `a_token_rebuilt_from_its_stored_hash_authorises_the_same_plaintext`, `match_registry_round_trips_for_restart_recovery`, the over-the-wire `bad_token` rejection; M0's predictable dev tokens were red-team finding T-1 |
| 5 | "The server rigged my shuffle" | **Commit–reveal:** `sha256(seed‖salt)` published at match start, revealed at end; anyone replays the command log and verifies every hash. The **salt is persisted** with the match (#85-fu `match_registry.seed_salt`), so a match recovered after a restart re-commits under the ORIGINAL salt — the published commitment stays honourable across a crash | determinism suite + `a_commitment_recovered_from_persisted_seed_and_salt_is_identical` + the recovery test's original-commitment assertion |
| 6 | Seed prediction | Seed from OS CSPRNG (M0's SystemTime placeholder is T-1, banned from deployed envs by CI lint) | grep-lint in CI |
| 7 | DoS / flooding | 16 KB WS message cap (done), per-conn & per-IP rate limits (Valkey counters), ALB + WAF rate rules, connection caps | L10 rate tests, k6 load suite |
| 8 | Bot farming / win-trading | Server-side stats anomalies (winrate vs move-time distributions), device attestation on mobile, attested-client weighting for ranked. Web is the weakest tier — accepted, documented | L10 + analytics jobs |
| 9 | SQL injection | sqlx compile-time-checked queries only | compile gate |
| 10 | Container escape / lateral movement | distroless static nonroot, read-only rootfs, drop ALL caps, seccomp RuntimeDefault, no SA token, default-deny NetworkPolicy, restricted PSS namespace | kustomize manifests in repo; kube-bench in CI (M2) |
| 11 | Supply chain | cargo-deny (licenses/advisories/yanked), committed lockfile, pinned base-image digests, Trivy gate, SBOM + cosign keyless signing at release | CI jobs in repo |
| 12 | Secrets leakage | IRSA for AWS access, External Secrets → ASM, no secrets in env dumps/logs, JSON logs with deny-listed keys | log-scrub tests (M2) |

**#85-fu — credentials hashed at rest, the commitment honourable across a restart (LANDED).**
The #85 match-seed work hardened the *running* server (CSPRNG seeds, in-memory `SeatToken`,
commit–reveal); two gaps in the cross-restart recovery path (`match_registry`, owned by
`recollect-journal-postgres`) closed in this follow-up:
- **Seat tokens are persisted hashed, never in the clear.** The registry's
  `token_{a,b,a2,b2}_hash` columns are `BYTEA` holding `sha256(token)` — the plaintext
  columns were dropped. The server passes the digest it already holds (`SeatToken::digest`);
  recovery rebuilds the in-memory handle from the digest (`SeatToken::from_hash`) and a
  reconnect authorises against it with the same constant-time compare. A DB compromise can no
  longer leak a live seat credential (it never could from a memory dump — now neither from disk).
- **The commitment salt is persisted (`match_registry.seed_salt`).** A match recovered after a
  restart re-commits from the persisted `{seed, salt}` (`SeedCommitment::from_parts`), so its
  published commitment is bit-identical to the one announced at creation — the commit–reveal
  stays provably-fair across a crash, not only within one process. Determinism + redaction are
  untouched: the seed and salt still reach a client *only* at the end-of-match reveal, never a
  view or event. The salt lives solely in the server-side registry row.

**COPPA/GDPR (T-13):** age-8+ audience ⇒ telemetry carries no PII, accounts need parental-consent flows, no free-text chat at launch (emotes only), deletion path required. Public playtests with accounts wait for this workstream; local/anonymous playtests don't.

## 10. Hosting

Two tiers — a lean launch host now, the full cloud plan when scale + spend justify it ("the spend follows the fun", T-12).

### 10.1 Launch — lean (EC2 ↔ Cloudflare)

The launch/playtest host, before §10.2 is warranted. One small box, one CDN, a domain — mostly
free. **LANDED** as Pulumi IaC under `deploy/` (`deploy/README.md` is the operator guide). What was
built differs from the original sketch in four deliberate ways, recorded here so the design stays
the law: **Postgres runs on-box in compose** (not RDS — the maintainer's playtest override); the
**static marketing website is on Cloudflare Pages** at the **apex + `www`** (direct-upload, IaC in the
PLATFORM stack — see `deploy/site/README.md`), so the **box serves only the game** (the wss/REST API +
its own copy of the play client) at **`play.<domain>`**, a single origin there (no Caddy); and there
is **no Origin CA cert** (the Tunnel makes a public origin listener moot — see below). The site +
the box's play client both embed the same `recollect-core`; only the box's client reaches a live wss.

- **Service stack (declarative):** the **EC2** box (free-tier `t3.micro`) runs a **Docker Compose**
  stack (`deploy/compose/docker-compose.deploy.yml`), brought up by **cloud-init** (`deploy/pulumi/user-data.sh`):
  `recollect-server` (the **`--profile dist`** image) + on-box **Postgres** + a one-shot **site-builder**
  + `cloudflared` (the tunnel), each with `restart: unless-stopped` (the builder is one-shot). The
  server is hardened — read-only root, all caps dropped, `no-new-privileges`.
- **Single origin for the game (`play.<domain>`):** the axum server **serves its copy of the static
  site + the wasm play client** via an env-gated `STATIC_DIR` (`router_with_static`, tower-http
  `ServeDir`), so the page, its assets, and `wss://play.<domain>/matches/{id}/ws` all share one origin
  — no CORS, no mixed content, one fewer container on the 1 GB free tier. cloudflared proxies
  `play.<domain>` → `http://server:8080`. (The marketing **website** is the apex/`www` on **Cloudflare
  Pages**, a separate origin; the site's "Play" link points at `play.<domain>`.)
- **Edge:** **Cloudflare** in front — DNS for the zone (apex/`www` → Pages, `play.`/`grafana.` → the
  tunnel), proxy/CDN caching the static assets at the edge, WebSocket proxying for the socket, and
  **TLS** terminated at the edge (free universal cert).
- **No public inbound:** a **Cloudflare Tunnel** (`cloudflared` on the box) dials *out* to Cloudflare,
  so the EC2 security group opens **no** inbound ports (egress-only). Admin is keyless via **SSM
  Session Manager** — no SSH key, no port 22. Pulumi creates the named tunnel, generates its secret,
  reads back the connector token, and injects it into cloud-init (nothing to copy/paste).
- **Why no Origin CA cert.** An Origin CA cert + Full(strict) applies when Cloudflare connects to a
  *public* origin over the internet. With a Tunnel there is no public origin: cloudflared makes its
  own authenticated, encrypted outbound connection and reaches the server over the private compose
  network. The tunnel *is* the encrypted origin path; a cert on a public 443 listener would be moot
  (there is no such listener).
- **Database:** on-box **Postgres** in compose (no published port — compose-network-only); `DATABASE_URL`
  targets it ⇒ Postgres-authoritative (append-before-ack, resume-from-journal). Its password is
  **generated by Pulumi** (`random.RandomPassword`, kept secret in state, injected via cloud-init into
  the box's 0600 `.env`) — not an operator input. Unset `DATABASE_URL` ⇒ the server degrades to
  in-memory (fine for the earliest playtests). Its data dir is a **host bind mount onto the durable
  data volume** (next bullet), so the journal + accounts survive the box.
- **Durable data (survives instance recreation):** the box's stateful data lives on a **dedicated,
  encrypted gp3 EBS volume that is SEPARATE from the instance root** (Pulumi: `aws.ebs.Volume` +
  `aws.ec2.VolumeAttachment` at `/dev/sdf`, size `dataVolumeSizeGb`, default 20 GiB, same-AZ as the
  box, no delete-on-termination). A `pulumi up` that **replaces** the box (`userDataReplaceOnChange`)
  or a terminate destroys the root but **not** this volume: a replace re-creates only the *attachment*
  (the Volume's own inputs are unchanged), so the **same volume re-attaches to the new instance**,
  where cloud-init **MOUNTS it at `/data`**.
  The mount is **format-only-if-empty**: user-data checks for an existing filesystem (`blkid`/`lsblk
  -f`) and formats ext4 **only when there is none** (a brand-new volume) — an already-formatted volume
  is mounted **as-is, never reformatted** (a reformat-on-boot would wipe the data every boot — the
  durability bug this explicitly avoids), persisted in `/etc/fstab` **by UUID** with `nofail`. `/data`
  is the **one durable mount, shared by stateful services via per-service subdirs**: `/data/postgres`
  today (owned 70:70, the alpine Postgres uid; bind-mounted to `/var/lib/postgresql`), and a **light
  self-hosted observability stack** (Grafana + a metrics TSDB, ~1–2 GB short-retention) when it lands —
  same volume, no AWS change. **No EBS snapshots** are taken (durability is the volume outliving the
  box; `pg_dump` via `make db-backup` is the ad-hoc logical backup). cloud-init also creates a **swap
  file on `/data`** (`/data/swapfile`, `swapSizeGb` GiB, default 4) for RAM headroom on the 1 GB box —
  created idempotently (only if absent), persisted in `/etc/fstab` (`nofail`), with `vm.swappiness=10`
  so swap is a safety net, not a hot path; it grows the self-hosted observability stack's headroom.
- **Analytics:** the cookieless **Cloudflare Web Analytics** beacon in the site `<head>` (see
  `usage_tracking.md`), baked in at site-build time from an optional `cfBeaconToken`.
- **Observability (self-hosted, §11):** the deploy compose also runs the **`grafana/otel-lgtm`** stack
  + a `node-exporter`; the server exports OTLP to it on-box; Grafana is at **`grafana.<domain>`**
  behind a **Cloudflare Access** gate (allow-list = the maintainer email), with **dashboards-as-code**
  and **CloudWatch out-of-band alarms** → SNS email. Persists to `/data/observability` (short
  retention), leans on the 4 GB swap. Full access + credential runbook in `deploy/README.md`.
- **Infrastructure as code (declarative):** **Pulumi** provisions the cloud — the EC2 instance + its
  egress-only security group + an **SSM + CloudWatch-agent** instance role (IMDSv2 required, hop-limit
  1; encrypted EBS root, **shrunk to 10 GiB** so root + the 20 GiB data volume fit the EBS free tier)
  + the **durable encrypted EBS data volume** + its attachment, the **Cloudflare** DNS + Tunnel +
  **Zero Trust Access** app/policy (gating Grafana), **AWS Budgets** guardrails (a monthly cost cap +
  a $1 free-tier-overrun guard), and **CloudWatch** box-health alarms → an **SNS** email topic.
  `pulumi up` is the source of truth; no click-ops. **Credentials are least-privilege** — AWS via SSO
  (short-lived, recommended) or a scoped IAM user; Cloudflare via a **custom API token** scoped to
  Tunnel:Edit + Access:Apps-and-Policies:Edit + Zone-DNS:Edit on the one zone (never the Global Key).
- **Deploy / redeploy:** `pulumi up` stands up the box + edge; a new pinned `gitRef` then either
  `pulumi up` (re-provisions a fresh box — `userDataReplaceOnChange`) or `sudo recollect-update <ref>`
  on the box (checkout + `compose up -d --build` + image prune). The box builds the images itself.
- **Cost & posture:** free-tier EC2 + free Cloudflare + free CloudWatch + a domain (~$10/yr) —
  **including the whole self-hosted observability stack at $0 net** (it rides the existing box + swap);
  graduate to §10.2 only when load/availability demands it. `t3.small` is the fallback if 1 GB OOMs /
  swap thrashes (the budget guard emails). The **#31 cost fix** shrank the root to 10 GiB so root (10)
  + the durable data volume (20) = **30 GiB = the entire 30 GB/12-month EBS free tier ⇒ $0** for
  storage in the window (previously the 30 GiB root alone filled it and `/data` was ~$1.60/mo).

### 10.2 Scale — AWS & Kubernetes (later)

- **EKS** (managed node groups; Karpenter later), private subnets, IRSA per workload, restricted Pod Security on every namespace.
- **RDS Postgres 16** (Multi-AZ at M3), TLS required, IAM auth for operators, no public access.
- **ElastiCache for Valkey**, AUTH + in-transit TLS, private subnets.
- **ALB** for wss (idle timeout ≥ 300 s for long matches, sticky not required — routing is by match token), **WAF** rate rules in front.
- **S3 + CloudFront (OAC)** serve the wasm client; immutable content-hashed assets.
- **ECR** with scan-on-push; images referenced by digest in overlays.
- **Terraform** for all of it; one workspace per environment; budget alarms from day one.
- The hardened Dockerfile (in repo) is the contract: cargo-chef cached builds → static musl → `distroless/static:nonroot`, no shell, stripped binary; probes and resources live in the Deployment.

## 11. Observability (Grafana stack)

- **Emit:** `tracing` spans + JSON logs in the server; OTLP out via `opentelemetry-otlp` (always compiled in; export gated on `OTEL_EXPORTER_OTLP_ENDPOINT`).
- **Collect:** Grafana **Alloy** (agent) → **Tempo** (traces), **Loki** (logs), **Mimir** (metrics). Both locally (`make up`) and **on the launch host**, the whole stack is the one **`grafana/otel-lgtm`** container (Grafana + Tempo + Loki + Prometheus/Mimir + an OTel collector).
- **Launch-host stack — SELF-HOSTED, all-in-IaC (§10.1, LANDED):** the lean box runs `grafana/otel-lgtm` in the deploy compose; the server exports OTLP to it **by default** (`OTEL_EXPORTER_OTLP_ENDPOINT=http://lgtm:4317`; set the `otelEndpoint` stack config to ship **off-box** to e.g. Grafana Cloud instead). It persists to its **own subdir on the durable `/data` volume** (`/data/observability`) — the same volume Postgres uses, not a throwaway local volume — at **short retention** (metrics 14d / logs 7d / traces 3d via the mounted backend configs) to bound it to ~1–2 GB, and **leans on the box's 4 GB swap** for RAM headroom on the 1 GB box (`t3.small` is the pressure valve). Container-hardened (`cap_drop: ALL`, `no-new-privileges`).
- **Access (the gate):** Grafana is reachable at **`grafana.<domain>`** through the Cloudflare Tunnel but **never publicly usable** — a **Cloudflare Zero Trust Access** application + an allow policy gate it to the **maintainer email** (free tier). The maintainer authenticates at the edge (one-time PIN or IdP) before any request reaches Grafana. Grafana's own anonymous-Admin is safe only because Access is the real auth in front.
- **Dashboards-as-code:** three Grafana dashboards are **JSON in the repo** (`deploy/compose/observability/grafana/dashboards/`), provisioned via a mounted provider — never clicked together, read-only in the UI: **RED service** (the metrics below), **game-design** (§16, below), and **host/box** (CPU/mem/swap/disk from a `node-exporter` sidecar).
- **The instrumentation catalog is a living doc:** every metric the server emits — name, type, labels + value sets, emission point, what it measures — plus the `#[instrument]` spans, the three dashboards panel-by-panel, the OTLP export path, and the convention for adding a metric, are catalogued in **`docs/observability.md`** (the source of truth; keep it current with the code). This section is the summary; that doc is the index.
- **RED metrics:** request rate/errors/duration per route; the command path's `recollect.commands.applied{outcome=ok|reject}`, the `recollect.command.duration_ms` engine-apply histogram (exemplars → Tempo), `recollect.ws.connections.opened` / `recollect.ws.reconnections`, and `recollect.matches.{created,finished}`. (Names map to Prometheus form on ingest — dots→underscores, counters gain `_total`.)
- **Game-design metrics** (the spec's §16 instrumented in prod, not just sims): **all live now.** P1/P2 winrate + draw rate derive from `recollect_matches_finished_total{result="Win(A)"|"Win(B)"|"Draw"}` (seat A = P1/opener). The **deeper** cuts are emitted from the server's command/event seam (the same `#[instrument]` path that emits `matches_finished`), so their panels now resolve instead of reading *No data*: **winrate-when-leading-at-contraction** (target ≤ 70 %) rides as the `led_at_contraction`/`won` boolean labels on `recollect_matches_finished_total` (the leader is captured at the Dusk `MemoryContracted` step and correlated with the result); **evolutions/match** is `recollect_evolutions_total{kind=primal|fabled}` (one per `SpiritEvolved`); **Throughline completion rate** is `recollect_throughline_completed_total` (one per `ThroughlineCompleted`); **median match length** is the `recollect_match_length_turns` histogram (`_bucket`), the turn count recorded at match end. All labels are low-cardinality seats/enums/booleans — never a per-match id, the seed, or per-player private state (redaction holds; see `docs/observability.md`). The 2k-match sim prints the same cuts offline.
- **Alerts — two tiers.** *In-band* (Grafana, future): SLO burn (availability, p99 apply latency), reject-rate spikes by reason, winrate drift beyond bands. *Out-of-band* (CloudWatch, **LANDED**): because the in-box stack can't alarm on its **own** outage, Pulumi creates **CloudWatch alarms → an SNS email topic** for EC2 status checks (instance + system, the latter auto-recovering the box), high CPU, and the **custom memory/swap/disk** metrics a lightweight **CloudWatch agent** on the box publishes — all inside the free tier (≤ 10 alarms, ≤ 10 custom metrics, basic 5-min metrics, no detailed monitoring, no Logs shipping).
- A spike in `Rejected{reason}` *is* the cheap intrusion-detection system — honest clients never send illegal commands.

## 12. Test strategy (the TDD answer)

The taxonomy, lowest to highest. **Bold = exists and passes in the repo today.**

- **L0 — Rule unit tests.** One test per spec sentence, named as the sentence (income curve, the Listener tie-break lever, no-fixed-action-count Main, pass-once, reach orientation, simultaneous combat with wheel edge, Arcane vs Warded, contraction/stain-locking/scoring, deck legality…). *(This taxonomy is the M0 snapshot; the current, exhaustive test map is [`docs/testing.md`](testing.md). "Listener's Grace" — an M0 going-second compensation — was **removed**: only the Listener name + the holstered tie-break lever remain, per `design.md` §5.)* The red **M1 backlog** (momentum, evolution, fabrications-never-score, mobile) is checked in as `#[ignore]` tests — the work queue is executable.
- **L1 — Property/invariant tests.** **proptest playouts** over random legal command sequences: legality of `legal_commands()`, anima/board/hand invariants, termination, score bounds. Grows a custom `GameState` strategy per feature.
- **L2 — Reference models.** A deliberately-dumb scorer diffed against the engine (M1); Ironstate `model_test!()` for the lifecycle machines (M1).
- **L3 — Golden replays.** **Determinism suite passes today** (same seed+commands ⇒ identical hash at every step; snapshot-resume byte-identical). M1 adds committed `.replay` fixtures that gate every release: old replays must hash identically or carry an explicit ruleset-version bump.
- **L4 — Differential determinism.** Same seeded playout natively and under wasm32 (wasmtime in CI); hashes must match. Catches float/usize/library divergence before players do.
- **L5 — Fuzzing.** cargo-fuzz on the protocol decoder and on raw command streams against the engine (must never panic — only `Reject`).
- **L6 — Mutation testing.** cargo-mutants on `recollect-core` nightly; kill-rate is the honest measure of L0/L1 quality.
- **L7 — Service integration.** testcontainers-rs: real Postgres (append/snapshot/replay round-trips, fencing), real Valkey (queues, leases). **Session-level tests pass today** (seq replay rejection, rejection-without-state-change, per-seat view routing).
- **L8 — System e2e on ephemeral kind.** `scripts/kind-integration.sh` (in CI now: deploy + health + match creation). M1: bot pair plays full matches over wss inside the cluster, then the chaos step — `kubectl delete pod` mid-match, assert resume hash equality.
- **L9 — Load.** k6/goose WS bot fleets; targets: 5k concurrent matches/pod-set, p99 `apply` < 50 ms, zero leaks under sampling proxy.
- **L10 — Security tests.** Authz matrix (A's token can never act as or see B), replay, malformed/oversized frames, rate limits, plus the CI determinism grep-lint and cargo-deny gates. Scheduled: 1M-game balance sims charting the design targets.

TDD loop in practice: write the L0 test from the spec sentence → red → implement → green → extend the L1 generator → let nightly L6 tell you if the test would actually catch regressions.

## 13. Red team — technical findings & resolutions

- **T-1 (fixed-by-policy, open-in-code):** M0 dev tokens derive from the seed and the seed from `SystemTime` — fine for localhost, *banned* beyond it. CI grep-lint blocks `SystemTime` in core; M1 replaces with OS CSPRNG + hashed tokens + commit–reveal. Tracked as the first M1 ticket.
- **T-2 (measured):** Random-legal sim: **P1 48.4 % / P2 31.4 % / draws 20.2 %** over 2,000 matches — a 17-point first-mover skew under random play. Not the design target population (skilled play), but the direction is a flag; the going-second compensation lever is the dial (the **Listener's Grace** mechanic floated at M0 was since **removed** — `design.md` §5 keeps the going-second edge as the held *Listener tie-break* lever instead, measured in the skilled-bot sims), and the smarter-bot sim is an M1 deliverable before tuning.
- **T-3 (ruled):** Valkey must never be authoritative; PG advisory lock is writer election; fencing tokens on cross-system writes. "If Valkey vanished, no match state is lost" is an invariant with an L7 test.
- **T-4 (pushback accepted):** kind for the everyday inner loop is friction without payoff; tiered loop adopted, kind kept for parity + CI.
- **T-5:** WebGPU isn't universal; Canvas2D first, wgpu+WebGL2 fallback later; 3 MB gz wasm budget with size tracking in CI.
- **T-6:** JSON's flexibility is a leak vector; only `PlayerView` types cross the wire, `GameState` has no socket path, leak tests guard it; binary later behind the same envelope.
- **T-7:** Float/HashMap nondeterminism — banned in core, enforced by CI grep (already wired), to be upstreamed as the proposed Ironstate `deterministic` lint.
- **T-8 (open design question → game team):** hand-cap behavior — engine currently *skips* the draw at 8 (emits `DrawSkippedHandFull`); alternative is draw-and-mill. Decide before M1 golden replays freeze.
- **T-9:** ~~broadcast fan-out can drop under lag (capacity 64)~~ **RESOLVED (#86):** the actor pushes per-seat over per-seat mpsc senders — no shared channel, no drop class. (A wedged socket grows its own unbounded queue and tears down on its send error, costing no neighbour a frame.)
- **T-10:** ~~`Mutex<Session>` held across synchronous engine calls — correct but fragile~~ **RESOLVED (#86):** the actor owns the `Session` by value; there is no lock to slip an `.await` under. The journaled append `.await` runs single-threaded inside the actor.
- **T-11:** Base images by tag in the repo today; release pipeline pins digests + cosign-verifies before deploy (policy-controller at M2).
- **T-12 (scope honesty):** no AWS until M2; playtests run on compose/kind; the spend follows the fun, not the reverse.
- **T-13:** COPPA/GDPR workstream gates any public account-based playtest (no PII telemetry, parental consent, emote-only chat, deletion path).
- **T-14:** Ephemeral-kind CI flake budget: preloaded images, generous rollout timeouts, retry-once, suite must stay < 10 min or it gets split.
- **T-15:** Protocol has no compression negotiation yet; per-message-deflate decision deferred until view payload sizes are measured (M2 metric).

## 14. Milestones

- **M0 — walking skeleton (this repo, done):** deterministic core slice; L0/L1/L3-determinism/redaction green; protocol v1; in-memory WS server with seq idempotency; bot + balance sim; wasm hotseat shell; hardened Dockerfile; kind/kustomize/compose/CI incl. ephemeral-kind smoke.
- **M1 — trustworthy persistence & the four mechanics:** momentum/evolution/fabrications/mobile (turn the red tests green); PG event store + snapshots + advisory-lock writer + fencing; real tokens + commit–reveal; testcontainers L7; golden replay fixtures; Ironstate for lifecycle machines; wasm differential CI; actor-per-match.
- **M2 — content & telemetry:** 240-card data pipeline (cards as data files, schema-validated against the stat-budget formula); reconnection; full OTel→LGTM with design-metric dashboards; kind chaos e2e; AWS staging via Terraform; matchmaking on Valkey.
- **M3 — ranked & hardened:** ranked + attestation weighting, load suite to targets, WAF, cosign enforcement, wgpu renderer behind a flag.
- **M4 — mobile:** UniFFI shells, store pipelines, push for async matches.

## 15. Open questions

1. Hand-cap rule (T-8) — *resolved:* at cap 8 the seat **Releases** (draws, then bottoms one) — `design.md` §5.
2. Going-second compensation sizing once skilled-bot sims land (T-2). *(The M0 "Listener's Grace" was removed; the held lever is now the **Listener tie-break**, sized only if the data shows imbalance — `design.md` §5.)*
3. Binary protocol choice (postcard vs CBOR) after payload measurements (T-15).
4. Spectator/replay-sharing privacy model (replays expose hands post-hoc — fine after match end? probably, since commit–reveal already discloses the seed).
5. Whether to upstream the Ironstate v0.5 proposals (§4.3) or carry a local fork until Kassian v1 extraction happens.

*Determinism is the anti-cheat. The log is the truth. The Memory keeps both tellings.*
