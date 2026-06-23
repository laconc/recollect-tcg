# Playtest-website launch plan

The launch direction for getting real playtesters into the game through a website,
and the answers to the open questions it raises. Companion to
`tech_design.md` (which fixes server-authority, redaction, COPPA/GDPR
gating, and the M-milestones).

## Goal
Get real playtesters into the game **through a website, now** — the minimum
real product that yields play data, not the full narrative game. Mobile comes
later; the website should be built so the mobile work reuses as much as
possible.

## 0. Branding (locked)
- **Name: Recollect TCG.** Short form "Recollect" in prose; the branded/distinct
  name is "Recollect TCG". World + naming canon: `docs/lore.md`.
- **Canonical domain: `recollect-tcg.com`** — the website, the Play-link
  target in the landing page, and any canonical URLs.
- **Trademark (not legal advice):** no mark is required to publish on iOS/Android —
  only the right to use the name. "Recollect" is a common English word, arguably
  descriptive for a memory game, which both weakens registrability and raises the
  chance it is already in use. A free USPTO TMSearch clearance check (Class 9
  downloadable game software; Class 41 online game services) is worth doing before
  sinking marketing in; registration itself is optional and can wait. Per owner, not
  blocking launch.

## 1. Accounts & identity

### The anonymous-identity server model
The launch identity story is **anonymous handles, no accounts**:
- A generated handle **is** an anonymous identity (no account). The **client** mints + persists
  the real handle (web `localStorage` — a separate lane) and sends it on `Hello.name`; the
  **server records it** and supplies a floor: `recollect-server/src/identity.rs`
  (`participant_handle` / `fallback_handle` — a `guest-…` handle when `Hello.name` is absent, so
  a match is never name-less).
- **Every journaled match is name-tagged**, anonymous and signed-in alike, via the
  `match_participants` table (one row per occupied seat: handle + session_id + nullable
  `account_id`). The data shape is **forward-compatible with the OIDC accounts below**:
  "who played" is a handle an account later *claims* (the nullable `account_id` FK is that seam).
  See `decisions/usage_tracking.md` for the table + the recording path.
- The match-token ↔ account boundary below is untouched — a handle is a display name, not a
  credential; the per-seat match token remains the only thing that authorises a seat.

### What we need at playtest
- Anonymous/guest play **first** (no account) — lowest friction, and the
  tech-design already supports local/anonymous playtests without the
  COPPA/GDPR workstream (T-13). A guest gets a device-scoped id; their match
  history is best-effort.
- Optional **lightweight account** to persist identity across devices and join
  ranked later. Sign-in via **OIDC** (Google, Apple, optionally email magic
  link). We are an OIDC *relying party*; we do not roll our own password store.
  AWS **Cognito** user pools can broker the OIDC providers and issue our own
  JWTs, OR we federate directly — to be decided (Cognito buys
  parental-consent and hosted-UI scaffolding cheaply; direct federation is
  fewer moving parts). Either way the **match token stays separate** from the
  account token (tech-design §security: match tokens are short-lived per-seat
  credentials, never account tokens — keep that boundary).

### How iOS / Android handle this (the reuse question)
- **Both platforms speak OIDC.** "Sign in with Apple" and "Sign in with Google"
  are OIDC/OAuth2 under the hood. So a single OIDC-based identity layer on the
  server is reused by web AND both mobile shells — this is the reuse win. The
  native shells use the platform sign-in SDK to obtain an OIDC id_token, then
  exchange it with our server for our session JWT — identical server endpoint
  for all three clients.
- **Apple requires "Sign in with Apple"** if you offer any third-party social
  login on iOS (App Store guideline 4.8). Plan for it from the start so the
  iOS submission isn't blocked later.
- **iOS Game Center / Google Play Games**: these are *optional convenience*
  identity + social layers (leaderboards, achievements, friend invites),
  **not** a replacement for our account system. Recommendation: treat Game
  Center / Play Games as an **opt-in linked identity** for social features
  (leaderboards, friends), layered on top of the OIDC account — NOT the
  primary auth, because (a) they're platform-locked (no cross-play identity),
  and (b) we want one account that works on web + iOS + Android. Game Center
  auth *can* be verified server-side (Apple's `generateIdentityVerificationSignature`),
  so if we later want "Game Center = instant iOS guest login" that's possible,
  but it's a nice-to-have, not the spine.
- **COPPA/GDPR (T-13) gates account-based public playtest**: no PII telemetry,
  parental-consent flow, emote-only chat, deletion path. Until that workstream
  lands, public playtests run **guest/anonymous only**. This is the gating
  decision — anonymous playtest can ship first; accounts ship behind T-13.

## 2. Card access & Quick Play economy (the unlock question)

**Decision for the playtest website: ALL cards available, Quick Play only.**
Rationale:
- A full collection/unlock economy is progression scaffolding we don't need to
  *test the game's fun*. Gating cards behind unlocks at playtest would (a) slow
  data collection, (b) confound balance signal with progression friction, and
  (c) pull toward the gambling-adjacent mechanics the design explicitly rejects
  (anti-gambling is a core principle).
- So at playtest: **every player has the full card pool**, and Quick Play
  generates decks from the whole catalog (the styles + the D-17 kind-weighting
  we already built). No unlocks, no collection, no narrative arc — the narrative
  is correctly out of scope for a playtest site.
- Keep the DATA SHAPE forward-compatible: model "what a player owns" as a set
  that, at playtest, is simply "all". When a collection economy is designed
  later, the same field narrows — no schema rework. (Quick Play already draws
  from a pool; it will draw from `owned ∩ pool`, which today = pool.)
- **Future, non-gambling progression** (post-playtest, owner's call): cards
  earned through play/achievements, never random paid packs. Out of scope now;
  noted so the data shape stays ready.

## 3. What else the playtest website needs (minimum)
- Quick Play vs. AI (works today: greedy bot) — instant, no waiting for a human.
- Quick Play vs. human — matchmaking (Valkey sorted-set queue, tech-design
  §matchmaking) OR a simple "create/join by code" (the `recollect-cli`
  `online new`/`online join` flow already proves this server-side).
- The wgpu/Canvas2D web client (D-24 / D-18) rendering the real PlayerView.
- Reconnection (tech-design M2) — playtesters WILL drop; resume from snapshot.
- Lightweight, PII-free telemetry: match outcomes, round length, command
  mix, rage-quits — the same event-stream subscriptions the design already
  routes through the journal. This is the POINT of the playtest.
- A feedback path (a button → form). No in-game chat at launch (T-13:
  emote-only later; nothing free-text for an 8+ audience without moderation).
- A "what is this game" landing page + the rules in brief.

## 4. Infrastructure — cheaper, single-region first

Owner direction: **single region, start cheap, Cloudflare CDN for assets.**
This is a deliberate, sound departure from the tech-design's EKS path (which is
the M3 hardened target, not the playtest target). For a playtest:

- **One EC2 instance** runs `recollect-server` (the in-memory or PG-backed
  authoritative server). No EKS, no Karpenter — a single small/medium instance
  handles playtest load; matches are cheap (one in-process task each). Behind a
  systemd unit or a single container; TLS via the load balancer or Caddy/nginx.
- **Cloudflare in front** for: (a) CDN/cache of the static wasm client + assets
  (cheaper egress than CloudFront for our volume, and a generous free tier),
  (b) TLS termination at the edge, (c) DDoS/WAF basics for free, (d) DNS. The
  wasm bundle + textures are immutable content-hashed assets — ideal for edge
  caching. WebSocket traffic to the EC2 server can proxy through Cloudflare too.
- **Postgres**: start with PG **on the same EC2 box** (or a tiny RDS) — the
  event store is small at playtest scale; Multi-AZ RDS is an M3 concern, not now.
- **Cost posture**: this is roughly "one EC2 + Cloudflare free/low tier + a
  small DB" — far below the EKS staging stack, which the tech-design itself says
  not to stand up before the game is proven fun (red team T-12). Document the
  upgrade path: EC2 → containerize → EKS when load/ranked demands it.
- **Reuse with mobile**: the EC2 server is the SAME `recollect-server` the
  mobile shells will talk to; the OIDC identity layer is shared; only the
  client differs. Nothing here forecloses the M4 mobile path.

## 5. CI / simulation budget (GitHub Actions)
Run the playtests/sims/stateright/many-iteration tests, wiring the heavy ones into
GHA **nightly where appropriate**, mindful of free-minute limits.
- **Per-PR (fast, every push)**: `make test` + `make catalog-check` + the
  3,000-state stateright CI gate (~12s) + a SMALL sim (e.g. fleet n=100). Keep
  this under a few minutes to stay cheap.
- **Nightly (cron, heavier)**: large balance sim (fleet n=2000+), the full
  stateright frontier (the 15k-state solace-modelcheck bin), differential wasm
  determinism (native vs wasmtime), longer prop runs. Nightly amortizes the
  minutes; failures open an issue rather than block a PR.
- Stay within free minutes: fast per-PR job; heavy work nightly + manually
  dispatchable; cache the cargo registry/target aggressively; consider a
  self-hosted runner on the same EC2 box if minutes get tight (it's already
  paid for).

## Open decisions
- Cognito-brokered OIDC vs. direct federation (lean: Cognito for the
  parental-consent + hosted-UI scaffolding, revisit if it's overkill).
- Matchmaking (Valkey queue) vs. create/join-by-code for the first human-vs-
  human playtest (lean: code-join first — it's nearly free and the server path
  exists; add matchmaking when there's a population).
- Whether Game Center/Play Games linking ships with the first mobile build or
  later (lean: later; OIDC is the spine).
