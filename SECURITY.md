# Security Policy

Recollect is a two-player storytelling board game with an authoritative Rust
server, a wasm web client, a CLI, and a single-origin launch host. This file is
the **threat model** and the **coordinated-disclosure policy** for the project,
and a living document: when the architecture changes, this changes with it.

The deep, test-pinned threat table lives in
[`docs/tech_design.md` §9](docs/tech_design.md) — every
row is *(threat → mitigation → the test that proves it)*. The prose below is the
launch-host summary plus the disclosure process; §9 is the authority for the
per-threat detail.

## Supported versions

The project is pre-1.0 and ships from a green `main`. Security fixes land on
`main` and deploy via the per-release `pulumi up` (the box pulls the CI-built,
pinned-tag server image — see `deploy/README.md`). There are no maintained
release branches yet; report against `main`.

## Reporting a vulnerability

**Please do not open a public issue for a security problem.** Use GitHub's
**private vulnerability reporting** for this repository
(*Security → Report a vulnerability*), which opens a private advisory thread with
the maintainer. If that is unavailable to you, contact the maintainer through the
account that owns this repository and ask for a private channel before sharing
details.

When you report, please include:

- a description of the issue and the impact you believe it has;
- the component (server / web client / CLI / journal / deploy) and, if you can, a
  commit SHA;
- reproduction steps or a proof-of-concept (a failing test is ideal — this repo
  treats *a bug fix ships with the test that would have caught it* as policy);
- any logs or crash output, with secrets redacted.

**What to expect.** This is a small project: we aim to acknowledge a report
within a few days, agree an assessment and a fix window with you, and credit you
in the fix (opt-out respected). We practice coordinated disclosure — please give
us a reasonable window to ship a fix before any public write-up. There is no paid
bounty program.

## Scope

**In scope** — the code in this repository:

- the authoritative server (`recollect-server`): the WebSocket match path, the
  REST account/match endpoints, the seat-token gate, reconnection/resume, the
  journaled apply, and the rate limiter;
- the wire protocol (`recollect-protocol`) and the redaction boundary
  (`PlayerView` / `TeamView` are the only things a client ever sees);
- the Postgres journal/accounts adapter (`recollect-journal-postgres`);
- the web/CLI clients as they pertain to the above (e.g. a client that could leak
  opponent state, or a CSP/XSS gap on the served site);
- the launch deploy (`deploy/`): the Cloudflare Tunnel + Access posture, the
  egress-only security group, the container hardening, and secrets handling.

**Out of scope:**

- **Game-balance / strategy "exploits"** — a strong line of play is gameplay, not
  a vulnerability. (A move the *server would reject* that a client nonetheless
  applies IS in scope — that is an authority break.)
- Findings that require a **compromised host, a stolen seat token handed over by
  its owner, or physical/again-on-box access** — the model assumes the box and the
  operator's machine are trusted.
- **Anonymous-play "enumeration"** of public facts (a handle is public by design;
  match ids are sequential by design and gate on a 256-bit seat token, so knowing
  an id grants nothing).
- Third-party infrastructure (AWS, Cloudflare, Postgres) — report those upstream.
- Volumetric DDoS against the edge (that is Cloudflare's layer).

## Threat model — the launch posture

The two load-bearing properties are **server authority** and **redaction**;
everything else is hygiene layered on top. (§9 has the full table + the test for
each row; this is the operator-level summary.)

### Server authority & game integrity
- Every command is validated by the **same `recollect-core`** the clients embed;
  the server never trusts a client's view of legality. `decide` is **total** over
  arbitrary commands — a hostile command rejects without panicking, without
  mutating state, and without consuming entropy (`tests/suites/security.rs`,
  `tests/suites/fuzz.rs`). The transport layer above it is total over arbitrary *bytes*
  too: a malformed/wrong-version/out-of-turn wire frame gets a typed reply and
  never desyncs or wedges the match actor
  (`actor::tests::hostile_wire_frames_never_panic_desync_or_wedge_the_actor`).
- **Replay/duplication** is blocked by a strictly-increasing per-seat sequence
  number; a stale or replayed `seq` is rejected.
- **Determinism**: same seed + same commands ⇒ identical state and events. The
  seed appears in NO event and NO view; it is disclosed only at match end via the
  commit–reveal.
- **"The server rigged my shuffle"**: a commit–reveal over the seed
  (`SHA-256(seed‖salt)` published at creation, revealed at end) lets anyone replay
  the command log and verify the shuffle was fixed before play. The salt is
  persisted, so the commitment stays honourable across a server restart.

### Confidentiality (redaction)
- A client only ever receives a **redacted** `PlayerView` / `TeamView` — never the
  raw state, the opponent's hand, the deck order, or Echo pre-knowledge. Since the
  actor-per-match design, each seat receives its frames over **its own** channel,
  so a cross-seat leak is structurally impossible (an opponent has no sender for
  your view). Redaction is proven at the core, the session boundary, and the wire
  (`tests/suites/redaction.rs`, the session-boundary and over-the-wire view tests,
  the web online-adapter redaction tests).

### Authentication & tokens
- **Seat tokens**: 256-bit CSPRNG, match-scoped, short-lived, **stored hashed
  everywhere** — the running server holds only `sha256(token)`, and the recovery
  registry row persists only the digest, so neither a memory dump nor a
  database read yields a live credential. A presented token is authorised by a
  constant-time digest compare. The token arrives in the **first WebSocket frame**
  (`Hello`), never in the URL, so proxies/logs record nothing usable.
- **Account tokens**: 256-bit CSPRNG, shown once at creation, stored only as
  SHA-256. Verification is a constant-shape hash lookup. Handle conflicts return a
  generic 409 (only the existence of a public handle is disclosable).
- Accounts are optional at launch; play is anonymous (a client-minted handle the
  server only *records*). No PII is collected (the COPPA/GDPR workstream gates the
  durable account layer — §9, launch-plan §1).

### Input validation & abuse
- All SQL is **parameterized** (`$n` bind parameters) — no string-built queries
  anywhere in the journal crate; match ids cast `::text::uuid` server-side, so
  garbage is rejected before it reaches a query plan.
- The handle/name/session-id inputs are length-clamped and character-restricted;
  untrusted text is recorded as data and must be rendered with `textContent` (the
  web client does).
- A 16 KB WebSocket message cap, a 10 s socket-recv timeout on the handshake, and
  per-IP token-bucket rate limits on the REST surface (account 20/min, match
  30/min) bound a flood. Behind the Cloudflare Tunnel the limiter keys on the
  verified `CF-Connecting-IP` (the only ingress is the tunnel, which overwrites
  that header), so distinct clients get distinct buckets. The rate map is
  **bounded** (it evicts expired buckets, so an IP-rotation flood can't grow it
  without limit).
- The match registry locks are **poison-resilient**: a panic-while-held cannot
  cascade into a server-wide lock-poisoning DoS.

### Deploy / operations
- **No inbound ports.** The launch host's security group is **egress-only**; the
  sole ingress is the **Cloudflare Tunnel** (`cloudflared` dials out). Edge TLS +
  HSTS are Cloudflare's. The operator's admin path is SSM Session Manager (no SSH
  key, no port 22); IMDSv2 is required with hop-limit 1 (no SSRF-to-metadata).
- **Single origin** (the axum server serves the site + wasm + `wss` socket from
  one origin) ⇒ no CORS on the deploy path; a path-aware **CSP** locks script to
  `'self'` on the static site and grants only the narrow `'wasm-unsafe-eval'` (never
  the broad `unsafe-eval`) to the wasm client, plus `nosniff`, `frame-ancestors
  'none'`, COOP, and a deny-by-default Permissions-Policy. These are owned and
  tested at the origin (`the_deploy_host_sets_security_headers_with_a_path_aware_csp`).
- **Grafana** is anonymous-admin **only because** it is reachable solely through a
  Cloudflare Access-gated tunnel route (a named allow-list, never public);
  optionally the connector validates the Access JWT at the origin too (R2-1,
  defense-in-depth).
- **Secrets never enter git.** The on-box Postgres password and the tunnel token
  are Pulumi-generated secrets injected at deploy time; the Helm chart takes an
  `existingSecret` and `image.tag` is required (`:latest` cannot deploy); compose
  credentials are local-only by name. Supply chain is gated by `cargo deny` +
  `cargo audit` (nightly).
- **Containers** are distroless, nonroot, read-only-root where feasible, with all
  caps dropped and `no-new-privileges`.

## Known residual risks (accepted / deferred, with levers)

Recorded honestly so they are decisions, not omissions — each accepted or
deferred with a named lever. None blocks the launch posture above.

- **Reconnect-flood throttling at the WebSocket upgrade (OPEN, low).** The REST
  surface is per-IP rate-limited; the `GET …/ws` upgrade itself is not (a match is
  gated by its 256-bit seat token, and a non-resident id triggers at most one
  indexed registry lookup before a token check). A per-IP connection cap is the
  lever if a real reconnect-storm appears post-launch; Cloudflare also fronts the
  edge. Tracked for the §10.2 scale tier (Valkey counters / WAF rules).
- **Web is the weakest anti-cheat tier (ACCEPTED, documented).** Server authority
  + redaction mean a modified web client cannot see hidden state or make an illegal
  move, but bot-farming/win-trading detection (move-time anomalies, attested-client
  weighting for ranked) is an analytics workstream, not a launch gate (§9 row 8).
- **The full §10.2 cloud threat rows** (NetworkPolicy default-deny, kube-bench,
  Trivy/SBOM/cosign) apply to the K8s scale target, not the lean launch host; they
  land with §10.2.

---

*Maintained as a source of truth (AGENTS.md). If you change the auth, redaction,
transport, or deploy posture, update this file and §9 in the same change.*
