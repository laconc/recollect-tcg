# Usage & action tracking

How we learn who plays and what they do — privacy-respecting, mostly free, and using the
right tool for each kind of question.

## The split (and why)
- **Aggregate metrics → OpenTelemetry** (already wired; the `recollect-server` meter → Grafana).
  Answers "how many / what kinds / how fast." It canNOT reconstruct an individual journey —
  metrics are aggregates, traces are sampled + short-lived, logs are retention-limited.
- **Per-match actions → the Postgres journal.** Every command is in `journal_events` (the
  authoritative stream); match shape/result is in `matches` / `match_registry`. A durable,
  ordered, queryable event log.
- **Who played each match → `match_participants`**: one name-tagged row per occupied
  seat, anonymous and signed-in alike, so the event stream is complete handle-in-use data.
- **Cross-match player journeys → an anonymous session id + `usage_events`.** No accounts, no PII.

## Identity at launch — anonymous handles, no accounts
At launch there are **no accounts** (a later milestone). A player is an **anonymous identity: a
generated handle**. The split between client and server is deliberate:
- The **client** mints + persists the real handle (a `localStorage` value on the web — a
  separate lane), renameable in the Account area, and sends it on `ClientMsg::Hello.name`.
- The **server only records it** (`recollect-server/src/identity.rs`): `participant_handle`
  takes the optional `Hello.name`, clamps a present one to 40 chars, and falls back to a
  minimal generated `guest-<6 hex>` handle (`fallback_handle`, OS entropy) when it's absent —
  so **no journaled match is ever name-less**. The server supplies the floor; it does not
  generate the durable identity.
- **Forward-compatible with accounts:** "who played" is a **handle**; an account later
  *claims* a handle. `match_participants.account_id` (nullable, FK → `accounts(id)`) is `None`
  at launch and is exactly the seam accounts fill on claim. No schema rework when accounts land.
- The handle is untrusted text: a UI that renders it MUST use `textContent` (no XSS), and it
  stays **public** — a table roster name, never hidden information (the redacted `PlayerView`
  carries no name; only the `Welcome`/`TeamWelcome` roster does. Guarded by
  `the_session_name_reaches_the_roster_but_not_the_opponent_view` in `recollect-server`).

## Server-side
### Metrics (OTel) — `recollect-server/src/metrics.rs`
- `recollect.matches.created{mode, vs_bot, difficulty, faction}`
- `recollect.matches.finished{result}` (outcome only — score dropped to stay low-cardinality)
- `recollect.commands.applied{outcome}` + `recollect.command.duration_ms`
- `recollect.ws.connections.opened`

Visible in the LGTM/Grafana compose stack (see `docs/operations.md`). No PII.

### Anonymous session id
- The web client mints an opaque id (`crypto.randomUUID()`), persists it in `localStorage`
  (`recollect_sid`), and sends it on the `Hello` (`ClientMsg::Hello.session_id`).
- On connect, the server records a `usage_events` row `(session_id, "joined", match_id)` —
  best-effort, spawned, and only when a journal is attached (never blocks the connection).
- It links a player's matches for journey reconstruction **before** real accounts exist, and
  carries forward when accounts arrive (link session → account at login). Not tied to an IP.

### Name-tagged participants
- On an **authorised** join (a routed seat token — a `bad_token` connection records nothing),
  alongside the `usage_events` join, the server records a `match_participants` row for the seat:
  `(match_id, seat, handle, session_id, account_id=NULL)` — best-effort, spawned, journal-gated,
  exactly like the join log. `handle` is `participant_handle(Hello.name)` (the client's name, or
  the `guest-…` fallback); `seat` is `A`/`B` (1v1) or `A1`/`B1`/`A2`/`B2` (2v2).
- The row **upserts on `(match_id, seat)`**, so a reconnect (a fresh `Hello` on the same seat —
  the resume path) refreshes the handle rather than duplicating the participant.
- This guarantees **every journaled match is name-tagged** — anonymous and signed-in alike —
  so the durable event stream is complete, handle-in-use training/telemetry data from day one.
  (The in-memory degrade — no `DATABASE_URL` — keeps no journal, so there is nothing to tag;
  the same graceful-degrade posture as the rest of the record.)

### usage_events / match_participants — `recollect-journal-postgres`
- `usage_events(id, ts, session_id, event, match_id)` + `Journal::record_usage(...)`. Add more
  product events here as needed (e.g. created / joined / finished).
- `match_participants(match_id, seat, handle, session_id, account_id, joined_at)` +
  `Journal::record_participant(...)`. `account_id` is a nullable FK → `accounts(id)`: NULL at
  launch (anonymous), the seam accounts fill when an account claims a handle.

### Example analytics queries
```sql
-- matches per day
SELECT date_trunc('day', created_at) AS d, count(*) FROM matches GROUP BY d ORDER BY d;
-- one player's journey: the matches a session joined, in order
SELECT ts, event, match_id FROM usage_events WHERE session_id = $1 ORDER BY ts;
-- weekly active players (distinct anonymous sessions)
SELECT count(DISTINCT session_id) FROM usage_events WHERE ts > now() - interval '7 days';
-- who played a given match (the name-tagged roster), with the outcome
SELECT p.seat, p.handle, m.result
FROM match_participants p JOIN matches m ON m.id::text = p.match_id
WHERE p.match_id = $1 ORDER BY p.seat;
```

## Website
**Cloudflare Web Analytics** — free, cookieless, zero-infra (we're already on Cloudflare),
privacy-by-design (aggregate page paths/referrers, not per-visitor tracking). Added **at deploy**
(it needs the Cloudflare beacon snippet/token) — see the hosting doc. Self-hosted **Umami** or
**Plausible** are the alternatives if we ever want data ownership.

## Privacy posture
No accounts required; the session id is opaque and client-minted (no PII); the web analytics is
cookieless (no consent banner needed). What's collected: aggregate match/usage metrics + anonymous
per-session match joins + the **handle a seat chose for a match** (a self-selected display name, or
a generated `guest-…` fallback — not legal identity, not an email). That's the floor we'd document
publicly if/when a privacy page is added.
