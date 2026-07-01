# Observability — the instrumentation catalog

The canonical, living index of **everything the `recollect-server` emits**: each
metric (name, type, labels, emission point, meaning), the `tracing` spans, the three
provisioned Grafana dashboards panel-by-panel, the OTLP export path, the **synthetic
monitoring & alerting** (the active probes that check the live service is up, §5), and the
convention for adding a metric. This doc is a **source of truth** — when you add,
rename, or retire an instrument, a span, a dashboard panel, a probe, or an alert rule, update it
in the SAME change (AGENTS.md "Keep the living docs current").

Scope: the server is the only service that emits app telemetry. The engine
(`recollect-core`) is pure and silent; the CLI/bot print to stdout (not telemetry).
The numbers the dashboards read come exclusively from the server's command/event
seam. See also: `docs/operations.md` (running the stack, reaching Grafana) and
`docs/tech_design.md` §11 (the observability design summary).

---

## 1. The pipeline at a glance

```
recollect-server ──OTLP/gRPC :4317──▶ grafana/otel-lgtm
  tracing spans  ─────────────────────▶ Tempo   (traces)
  tracing events ─────────────────────▶ Loki    (logs, JSON)
  OTel metrics   ─────────────────────▶ Prometheus/Mimir (metrics) ──▶ Grafana dashboards
```

- **Always compiled in.** OpenTelemetry is unconditional in the binary (there is no
  `otel` cargo feature). Export is **gated at runtime** on the
  `OTEL_EXPORTER_OTLP_ENDPOINT` env var:
  - **unset** ⇒ structured JSON logs to stdout only (the dev/test/`make server`
    default; the meter is a cheap no-op, so every `.add()`/`.record()` is safe and
    free to call on the hot path);
  - **set** (e.g. `http://lgtm:4317` under `make up`, or the on-box `lgtm` in the
    deploy) ⇒ all **three** signals (traces, logs, metrics) also export over OTLP.
- Wiring lives in `crates/recollect-server/src/telemetry.rs` (`init` installs the
  tracing subscriber + the metric/trace/log providers; the metric provider is a
  `PeriodicReader` pushing to the collector). App metrics are defined and emitted in
  `crates/recollect-server/src/metrics.rs`.
- **Prometheus naming.** OTel instrument names map to Prometheus form on ingest:
  dots→underscores, **counters gain `_total`**, **histograms expose `_bucket`/`_sum`/
  `_count`**. So `recollect.commands.applied` → `recollect_commands_applied_total`;
  `recollect.command.duration_ms` → `recollect_command_duration_ms_bucket`. The
  catalog below lists both forms.

---

## 2. Metrics

All instruments are registered on one meter, `recollect-server`, in
`metrics.rs`. Every table row gives the OTel name, the Prometheus name the
dashboards query, the type, the labels (with value sets), the emission point, and
what it measures.

### 2.1 RED / service metrics

| OTel name | Prometheus | Type | Labels (values) | Emission point | Measures |
|---|---|---|---|---|---|
| `recollect.commands.applied` | `recollect_commands_applied_total` | counter | `outcome` = `ok` \| `reject` | `actor.rs` — after every `Session::apply*` in the command path (1v1 + 2v2), under the actor's `#[instrument]` run span | Command volume and the reject rate (the cheap intrusion signal — honest clients never send illegal commands). |
| `recollect.command.duration_ms` | `recollect_command_duration_ms_bucket` | histogram (u64 ms) | `outcome` = `ok` \| `reject` | same call site; the elapsed wall time of the apply | Engine-apply latency (p50/p90/p99). |
| `recollect.matches.created` | `recollect_matches_created_total` | counter | `mode` = `1v1` \| `2v2`; `vs_bot` = `true` \| `false`; `difficulty` = `easy` \| `normal` \| `hard` \| `expert` \| `mixed` \| `none`; `faction` = `lorekeeper` \| `solace` | `matchmaking.rs::create_match` (POST `/matches`) | Usage mix: how matches are being started. |
| `recollect.matches.finished` | `recollect_matches_finished_total` | counter | `result` = `Win(A)` \| `Win(B)` \| `Draw`; `faction` = `lorekeeper` \| `solace` \| `pvp`; `reason` = `played_out` \| `abandoned`; `led_at_contraction` = `true` \| `false`; `won` = `true` \| `false` | `actor.rs::on_finish` (the one finish funnel, 1v1 + 2v2 + the absence forfeit), on the terminal event | Outcomes (P1/P2/draw winrate, PvE faction winrate), the **forfeit vs played-out** split (`reason`), **and** the §16 leading-at-contraction correlation (see §2.2). |
| `recollect.matches.forfeits` | `recollect_matches_forfeits_total` | counter | — | `actor.rs::on_finish` (alongside `finished`, when `reason=abandoned`) | Absence forfeits: a human seat/slot that disconnected and didn't reconnect within the grace (`?abandon_grace_secs`, default 120s) → `Command::MatchAbandoned`. The bare forfeit rate, no `result` sum needed. Equivalent to `recollect_matches_finished_total{reason="abandoned"}`. |
| `recollect.ws.connections.opened` | `recollect_ws_connections_opened_total` | counter | — | `actor.rs::on_subscribe` (a seat/slot socket subscribes) | WebSocket connection volume. |
| `recollect.ws.reconnections` | `recollect_ws_reconnections_total` | counter | — | `actor.rs::on_subscribe`, when a still-live sender for the principal is superseded (#87) | First-class reconnects onto an already-occupied seat (the drop→rejoin path). |
| `recollect.ws.frames_rejected` | `recollect_ws_frames_rejected_total` | counter | `reason` = `hello_first` \| `bad_token` \| `malformed_message` \| `unsupported_protocol_version` \| `mode_mismatch` | `ws.rs` (the Hello handshake: missing-Hello, unroutable token) + `actor.rs::on_text`/`on_subscribe` (a parsed-but-refused frame) | Transport-level frame refusals by cause — the cheap **flood signal** for a bad-token / malformed storm. Distinct from a rule `Rejected` (which rides `commands.applied{outcome=reject}`): these never reach the engine, so without this counter they were invisible to the RED board. |
| `recollect.server.starts` | `recollect_server_starts_total` | counter | — | `telemetry.rs::install_metrics` at process start | A heartbeat so the metrics pipeline carries a signal before any match runs. |

### 2.2 §16 game-design metrics

These are the deeper balance cuts the spec's §16 calls for, instrumented in prod (not
just the offline sims). They emit from the **same command/event seam** the
`matches_finished` counter uses: the per-match `MatchMetrics` accumulator
(`metrics.rs`) lives on the `Session` (the single writer for a match) and observes
**every applied event batch** — player AND bot moves, 1v1 AND 2v2, in-memory AND
journaled — because they all funnel through `Session::finish_apply` (the one post-apply
funnel; it branches on the acting principal's kind only for the view-fan, so the metrics
observation is shared across modes). Streaming counters fire as the events stream; the
contraction-leader and the turn count are captured as the match runs and folded in
at match end.

| OTel name | Prometheus | Type | Labels (values) | Trigger event / emission point | Measures |
|---|---|---|---|---|---|
| `recollect.evolutions` | `recollect_evolutions_total` | counter | `kind` = `primal` \| `fabled` | one per `SpiritEvolved` (form tier read from the catalog: `rarity == "Fabled"`) | Evolutions per match (dashboard divides by finished matches). The bare `…_total` sums across `kind`; the breakdown is there for free. |
| `recollect.devolutions` | `recollect_devolutions_total` | counter | — | one per `SpiritDevolved` (a banished form receded to a base — the §5 rescue) | Devolutions per match (dashboard divides by finished matches). Counted alongside evolutions because a **Devolution IS an arrival** (design §5), so the recede is a first-class form change, not a silent undo. |
| `recollect.throughline_completed` | `recollect_throughline_completed_total` | counter | — | one per `ThroughlineCompleted` | Throughline completion rate (per match). |
| `recollect.match_length_turns` | `recollect_match_length_turns_bucket` | histogram (u64 turns) | — | recorded **once at match end** (`MatchEnded` / `MatchAbandoned`); the value is the count of `TurnEnded` over the match | Match length in turns (the dashboard reads the median via `histogram_quantile`). |
| (labels on `recollect.matches.finished`) | `recollect_matches_finished_total{led_at_contraction,won}` | counter labels | `led_at_contraction` = `true` \| `false`; `won` = `true` \| `false` | the leader is captured at the Dusk `MemoryContracted` step (strict board-score lead by the §16 model); correlated with the result at match end | **Winrate when leading at contraction** (target ≤ 70 %): `…{led_at_contraction="true",won="true"}` / `…{led_at_contraction="true"}`. |

**The contraction-leader rule.** At `MemoryContracted`, the accumulator scores the
post-fade board by the **same tally `flow::finish` uses for the final score** — one
point per tile to its standing spirit's owner, else to the most-recent impression,
plus the Solace's off-board erasure tally (seat B). A **strict** lead records that
seat; a tie records "no leader" (`led_at_contraction="false"`), so a dead-even board
never inflates either side of the panel. A match that ends before contraction (or
runs without one) is likewise "no leader". `won="true"` iff that leader is the
result's winner.

**Why turns, not rounds.** The §16 panel is "median match length (turns)"; a turn is
finer-grained than a round (each round is both seats acting). The count is the literal
number of `TurnEnded` facts the match produced.

---

## 3. Tracing spans & logs

Diagnostics go through `tracing` (never `println` — that is reserved for genuine
UI/CLI output). Spans export to Tempo; events (logs) export to Loki as JSON. The
command path carries `#[instrument]` spans, so a trace ties a command to its match.

| Span (`#[instrument]`) | Location | Carries | What it scopes |
|---|---|---|---|
| `run` | `actor.rs` (`MatchActor::run`) | `db_id` | One match actor's whole lifetime — every command it applies hangs under this. The RED command counters/latency are emitted inside it. |
| `ws_handler` | `ws.rs` | `match_id` | A WebSocket upgrade/lifetime for one seat's socket. |
| `recover_match` | `matchmaking.rs` | `match_id` | Rebuilding a forgotten journaled match from the registry + journal (the #9 restart path). |
| `healthz` | `lib.rs` | — | The liveness/readiness probe. |

Notable log events: match finished (info, with the result string + `reason`), seat
reconnect (info, `?who`), seat vacated → arming the absence-forfeit grace (info, `?who`),
the forfeit being issued (info, `?who`/`?seat`), journal append/finish failures (warn),
the OTLP connection lifecycle (error on close). Log **bodies never carry private state** —
same redaction bar as the metrics labels (§5).

---

## 4. Dashboards (provisioned, read-only)

Four dashboards ship as **JSON in the repo** under
`deploy/compose/observability/grafana/dashboards/`, mounted and provisioned into
Grafana — never clicked together, read-only in the UI. Edit the JSON, not the running
board. Touch a dashboard JSON from the server side **only when a metric name must
change to match a query** (the dashboard's intended name is the contract; if a name
must move, move it in both the code and the JSON).

### 4.1 Recollect — service (RED) · `recollect-red.json` (uid `recollect-red`)
The operational health board.
- **Command rate by outcome** — `rate(recollect_commands_applied_total)` by `outcome`.
- **Reject ratio** — rejects / total commands.
- **Engine apply latency (p50/p90/p99)** — `histogram_quantile` over
  `recollect_command_duration_ms_bucket`.
- **Command throughput (ok vs reject, stacked)**.
- **WebSocket connections & reconnections** — the two ws counters.
- **Matches created vs finished** — the two match-lifecycle counters, per-second. The
  **forfeit rate** rides alongside as `rate(recollect_matches_forfeits_total)` (equivalently
  `rate(recollect_matches_finished_total{reason="abandoned"})`): a healthy spike here means
  players are disconnecting and not returning within the grace.
- **Transport frames rejected by reason** — `rate(recollect_ws_frames_rejected_total)`
  by `reason`; the transport-level flood signal (refusals that never reach the engine,
  distinct from a rule `Rejected`).
- **Stat panels:** Commands applied / Rejected / Matches created / Matches finished
  (totals).
- **HTTP request rate by route** — eBPF auto-instrumentation; *No data* unless an OBI
  sidecar sees traffic (optional, not part of the app's own emission).

### 4.2 Recollect — game-design (§16) · `recollect-game-design.json` (uid `recollect-game-design`)
The balance board — **all panels now resolve** (they previously read *No data* awaiting
server instrumentation; this catalog's §2.2 metrics are what light them up).
- **P1 (seat A) winrate / P2 (seat B) winrate / Draw rate** — gauges off
  `recollect_matches_finished_total{result=…}`; balance target ≈ 0.50.
- **Outcome split over time** and **P1 winrate drift** — the same, as timeseries.
- **PvE (Solace) faction winrate** — Solace (seat B) win share among
  `faction="solace"` matches.
- **Match creation by mode & opponent** — `recollect_matches_created_total` by
  `mode`/`vs_bot`/`faction`.
- **Winrate when leading at contraction** (target ≤ 70 %) —
  `…{led_at_contraction="true",won="true"}` / `…{led_at_contraction="true"}`.
- **Evolutions per match** — `rate(recollect_evolutions_total)` / finished matches.
- **Devolutions per match** — `rate(recollect_devolutions_total)` / finished matches
  (a Devolution is an arrival — the recede counted next to the evolution it mirrors).
- **Throughline completion rate** — `rate(recollect_throughline_completed_total)` /
  finished matches.
- **Match length (median turns)** — `histogram_quantile(0.50, …
  recollect_match_length_turns_bucket)`.

### 4.3 Recollect — host / box · `recollect-host.json` (uid `recollect-host`)
The box health board, from a `node-exporter` sidecar (not the app): CPU utilization +
by mode, memory used / used-vs-available, swap, root-disk + filesystem-used-% by
mount, load average, and eth0 network throughput. Bounds the lean §10.1 box (1 GB RAM
+ 4 GB swap).

### 4.4 Recollect — synthetic monitoring · `recollect_synthetic.json` (uid `recollect-synthetic`)
The probes board — the active **synthetic** checks (§5), not passive app metrics. Driven by the
`blackbox` exporter's series so an operator can *see* the probes, not just receive their alerts:
- **Probe status (UP/DOWN)** + **up/down timeline** — `probe_success` per probe (green/red), all five
  probes: the 3 internal (`server-healthz` / `site-index` / `ws-origin`) + the 2 external
  (`public-site` / `public-game`).
- **Probe latency** — `probe_duration_seconds` per probe (timeseries).
- **Uptime %** over the dashboard window — `avg_over_time(probe_success[$__range]) * 100` per probe.
- **Public TLS cert — days to expiry** — `(probe_ssl_earliest_cert_expiry{probe=~"public-.*"} - time())
  / 86400` for the external HTTPS probes (warns under 14d, matching the alert rule).

The **external** panels render *gracefully empty* when external monitoring is off (no domain ⇒ no
series ⇒ a "no data" / "external off" cell, not an error) — see §5.1 for enabling them.

---

## 5. Synthetic monitoring & alerting

Everything in §§2–4 is **passive** telemetry — the server emits, the stack records, the
dashboards read. **Synthetic monitoring** is the active complement: the stack *reaches out and
checks the live service is up on a schedule*, and **alerts** if it is not. This is what answers
"is the playtest down right now?" rather than "what did the server do while it was up?". It runs
on the **same self-hosted LGTM stack** (no extra SaaS, no extra AWS cost) via the Prometheus
**Blackbox exporter** for the probes plus **Grafana alert rules** for the paging.

Scope: this is the **deploy box** (`deploy/compose/docker-compose.deploy.yml`); the local stacks
(`make up` / `make dev-up`) do not run the probes, and the local overlay scales the `blackbox` sidecar to zero (its
internal targets are the box's own services, and the external probes are inert without a domain). It
is **distinct** from the out-of-band **CloudWatch** box-health alarms (those watch the *host* —
CPU/mem/swap/disk/EC2 status — from outside the box, so a wedged box still pages; see
`deploy/README.md`). Synthetic probes watch the *application surface* — **internally** from inside the
box (the app) and **externally** through the Cloudflare edge (the public path); CloudWatch watches the
*box* itself from AWS. Three deliberately independent eyes.

### 5.1 The probes (Prometheus Blackbox exporter)

A `blackbox` sidecar (`prom/blackbox-exporter`) serves a `/probe` endpoint; `lgtm`'s Prometheus
scrapes it with `?module=<m>&target=<url>`, the exporter performs the actual HTTP/TCP check, and
emits **`probe_success`** (1 up / 0 down) plus timing/TLS series. The probe modules are committed in
`deploy/compose/observability/blackbox/blackbox.yml` (ours in full — every module explicit, IPv4-pinned
for the compose network); the scrape jobs are the `blackbox-*` jobs in
`deploy/compose/observability/prometheus/prometheus.yaml` (the standard blackbox relabel pattern:
the probed URL rides `__param_target`, `__address__` is rewritten to `blackbox:9115`, and `instance`
is relabeled to the probed target).

The probes come in **two layers** — **internal** (committed, always on) and **external** (the public
edge, domain-configurable, inert until you set a domain):

**Internal probes** — the compose-internal origin (`http://server:8080/…`), scraped every 30s. They
bypass the edge entirely, so they answer "is **the app** up?".

| Probe (`probe` label) | Module | Target | Proves |
|---|---|---|---|
| `server-healthz` | `http_2xx` | `http://server:8080/healthz` | The game server is up **and serving** — a 200 from the live router means axum + the command path are alive (the deepest cheap app-health signal; the §3 `healthz` span is the same route). |
| `site-index` | `http_2xx` | `http://server:8080/` | The **static site** is being served from `STATIC_DIR` at the single origin (the `site-builder` output is mounted and reachable). |
| `ws-origin` | `tcp_connect` | `server:8080` | The **WebSocket origin** accepts connections — `wss://…/matches/{id}/ws` rides this port, so a successful TCP connect is the liveness proxy for live play. |

**External probes** — the **public https URLs a real player hits**, scraped every 60s **through
Cloudflare**. They answer "is the service reachable **from the outside**?" — catching
**Tunnel-down / DNS-broken / TLS-expired**, which the internal probes cannot see. **Inert by default**
(no real domain is committed — see *enabling* below).

| Probe (`probe` label) | Module | Target | Proves |
|---|---|---|---|
| `public-site` | `https_2xx` | `https://<domain>/`, `https://www.<domain>/` | The **static SITE** (Cloudflare Pages) is reachable from the world — DNS + Cloudflare edge TLS + Pages. Also records `probe_ssl_earliest_cert_expiry` (the site cert) for the TLS-expiry alert. |
| `public-game` | `https_2xx` | `https://play.<domain>/healthz` | The **GAME server** is reachable from the world — DNS + Cloudflare edge TLS + the named **Tunnel** + the axum origin. Also records the game-edge cert expiry. |

#### Internal vs. external — what each catches, and how they localize a fault

This is the whole point of running **both** layers (on top of the out-of-band CloudWatch box eye):

| | Internal probes | External probes |
|---|---|---|
| **Path** | compose network → `server:8080` (bypasses the edge) | DNS → Cloudflare edge/TLS → (Pages \| Tunnel) → origin |
| **Catches** | the app process down / not serving / DB-wedged | Tunnel down, DNS broken, edge TLS expired/misconfigured, Pages down |
| **Misses** | anything between the world and the box (the edge/Tunnel/DNS) | which *internal* component failed (only sees "the whole path is down") |
| **Means** | **the app is down** | **the edge / Tunnel / DNS is down** |

Reading the two together **localizes the fault** without logging into the box:

- **internal UP + external DOWN** ⇒ the app is fine; the break is the **edge / Tunnel / DNS / TLS**
  (check Cloudflare: the Tunnel/`cloudflared`, DNS records, the Pages deployment, the zone).
- **internal DOWN** (with or without external down) ⇒ **the app itself** is down (check
  `docker logs recollect-server-1` + Postgres on the box). External-down here is just a consequence.
- **both UP** ⇒ healthy end to end.

The internal probes are the **highest-value, always-on** check (the server being down is the failure
an edge check can't distinguish from a transient edge blip); the external probes add the **end-to-end
edge view** the internal ones structurally cannot have. Neither replaces the other.

#### Enabling external monitoring (set your domain — never committed)

The live domain is **deployment-unique and never in git** (config hygiene, AGENTS.md). The external
job therefore reads its targets from a **Prometheus file-SD directory**
(`deploy/compose/observability/prometheus/external-targets/`) that ships **empty of live targets** —
only a `public-edge.yaml.example` template (which the `*.yaml` file-SD glob deliberately does not
match) and a `.gitkeep`. **No live file ⇒ zero external targets ⇒ the external probes never run**, so
a clean clone and `make deploy-local` stay completely inert (no false alarms, no committed host).

To **turn external monitoring on**, render the live target file **from your domain** (which lives only
in the box's `.env` / Pulumi `domain` config) with the committed helper, then it self-activates:

```bash
# On the box (or your fork's deploy), from the repo root:
OBS_PUBLIC_DOMAIN=your-domain.com \
  deploy/compose/observability/prometheus/render-external-targets.sh
# optional: OBS_GAME_SUBDOMAIN=play (default) — matches the Pulumi `gameSubdomain`.
# With no var set, the script also reads OBS_GRAFANA_DOMAIN from the box's /opt/recollect/.env
# (the bare domain cloud-init already writes there from Pulumi `domain`).
```

That writes `external-targets/public-edge.yaml` — **gitignored**
(`external-targets/*.yaml` is in `.gitignore`), with the site (apex + `www`) and game
(`play.<domain>/healthz`) URLs and their `probe` labels. Prometheus **hot-reloads** file-SD within the
job's `refresh_interval` (5m) — no restart. To turn it **off**, delete the file (or unset the domain
and re-run). Wiring this into your fork's Pulumi `user-data.sh` (rendering it at boot from the
`domain` config, exactly like the `.env` values) makes it automatic per deploy; the repo stays
domain-free either way.

### 5.2 The alert rules (Grafana, provisioned as code)

Grafana alert rules live in `deploy/compose/observability/grafana/provisioning/alerting/` and are
**provisioned as code** (read-only in the UI, like the dashboards) — mounted into the image's
`conf/provisioning/alerting/` dir, which Grafana reads at boot. Each rule queries the `prometheus`
datasource for a probe's `probe_success` and fires when it stays `0` for the `for:` window. A firing
alert routes through the notification policy to the **`recollect-oncall`** contact point.

The rules are in **two separate groups** mirroring the two probe layers — so a firing alert tells the
operator **which class** of failure it is (**app down** vs **edge/Tunnel/DNS down**) at a glance:

**Group `synthetic-monitoring`** — the **internal** probes (the app). `noDataState: Alerting`: if an
internal series vanishes (the exporter or the whole box is gone), that is itself an outage — fail loud.

| Rule (uid) | Fires when | For | Severity |
|---|---|---|---|
| `recollect-probe-server-down` | `probe_success{probe="server-healthz"} == 0` | 5m | critical |
| `recollect-probe-site-down` | `probe_success{probe="site-index"} == 0` | 5m | warning |
| `recollect-probe-ws-origin-down` | `probe_success{probe="ws-origin"} == 0` | 5m | critical |

**Group `synthetic-monitoring-external`** — the **external** edge probes (the public path). Every rule
carries a `scope: external` label and is **`noDataState: OK` + `execErrState: OK`**: the external job
is **inert until a domain is set** (no committed host), so a missing series must **stay silent** — no
false alarms on a clean clone / `make deploy-local`. They light up automatically once the live targets
exist.

| Rule (uid) | Fires when | For | Severity |
|---|---|---|---|
| `recollect-probe-external-site-down` | `probe_success{probe="public-site"} == 0` | 5m | critical |
| `recollect-probe-external-game-down` | `probe_success{probe="public-game"} == 0` | 5m | critical |
| `recollect-probe-external-tls-expiry` | earliest public cert `probe_ssl_earliest_cert_expiry{probe=~"public-.*"}` < 14d | 1h | warning |

- **`for: 5m`** ≈ 10 consecutive 30s scrapes down before paging — long enough to ride out a single
  missed scrape or a server restart, short enough to alert promptly. (The external down-checks scrape
  every 60s, so `for: 5m` ≈ 5 scrapes there.)
- **`noDataState`** is **`Alerting` for the internal group** (a vanished series is an outage — fail
  loud) but **`OK` for the external group** (inert-by-default ⇒ no series must stay silent).
- **Telling the two apart at the alert.** External-down fires while the internal group is **silent**
  ⇒ the edge/Tunnel/DNS is broken, the app is fine. The internal group firing ⇒ the app itself is down
  (any external-down is a consequence). See the internal-vs-external table in §5.1.
- **Contact point + config hygiene.** `contactpoints.yaml` defines a `webhook` + `email` contact
  point and the notification policy (grouped by `alertname` + `probe`, so every internal **and**
  external alert notifies independently), but with **placeholder endpoints only** (`*.example.invalid`,
  RFC-2606 reserved — they never resolve). **No real alert destination is in git.** Wire a real
  webhook/SMTP on the box (or template it from Pulumi/`.env` on your fork); until then alerts still
  **fire and are visible** in Grafana (Alerting → Active notifications) and on the dashboards — they
  just don't egress. **Paging is deliberately NOT wired pre-launch** — `recollect-oncall` stays a
  documented placeholder for now.
- **Recommended no-SMTP relay (post-launch): a Cloudflare Worker.** When paging *is* wired, the
  cleanest option that needs **no SMTP server to run** is a tiny **Cloudflare Worker** as the webhook
  target: point the `recollect-webhook` receiver at the Worker's URL, Grafana POSTs the alert JSON to
  it, and the Worker relays — to **email** (Cloudflare **Email Routing** / **MailChannels**) or a
  push/chat — a fully serverless relay on the same Cloudflare account the deploy already uses (free
  tier). It sits **alongside** the usual webhook targets (Slack/Discord incoming webhook, PagerDuty
  Events v2, Opsgenie) — pick whichever you run; the Worker is the recommended one when you want
  email without standing up SMTP. Keep the Worker URL (and any shared secret, via Grafana
  `secureSettings` / an env-templated header) **on the box, never in git**, exactly like the other
  deployment-unique values.

### 5.3 How to add a probe

1. **Add the module** (if a new check shape) to `blackbox/blackbox.yml`, or reuse `http_2xx` /
   `https_2xx` / `tcp_connect`. Validate: `blackbox_exporter --config.check --config.file=…`.
2. **Add the target** to the right `blackbox-*` job in `prometheus/prometheus.yaml`:
   - an **internal** target (a fixed compose host) goes under `static_configs[].targets` with a
     `probe:` label (the relabel block already maps target → `__param_target` → `instance`).
   - a **deployment-unique** target (anything with the live domain) must **never** be committed —
     add it the way the external probes do: a `file_sd_configs` entry whose live file is **rendered
     from the domain and gitignored** (see `external-targets/` + `render-external-targets.sh`), so the
     git tree stays domain-free and inert.

   Validate: `promtool check config prometheus.yaml` (the box's bundled `promtool` — run it from the
   pinned `grafana/otel-lgtm` image).
3. **Add the alert rule** to `grafana/provisioning/alerting/recollect-alerts.yaml`: copy a
   `probe_success{probe="<your-label>"} == 0` rule, set `for:`/severity/annotations. The two-query
   shape (instant Prometheus query `A` + a `__expr__` math condition `$A < 1`) is the pattern. Put an
   **external** rule in the `synthetic-monitoring-external` group with `noDataState: OK` so it stays
   inert until a domain is set.
4. **Update this section** (a row in the tables above) in the same change.

### 5.4 Synthetic monitoring: self-hosted Blackbox vs the alternatives

Three places could run these probes; we chose **self-hosted Blackbox on the box we already run** for
the **deep application checks**, and note **Cloudflare Health Checks** as the natural **edge/site**
complement.

| Approach | What it is | Pros | Cons | Our use |
|---|---|---|---|---|
| **Self-hosted Blackbox** (chosen) | A `blackbox-exporter` sidecar in our LGTM stack, scraped by the Prometheus we already run; alerts via the Grafana we already run. | **$0 net** (fits the §11 self-hosted posture, no SaaS/AWS add); **deep + arbitrary checks** (any internal route, the ws-origin TCP, body/version assertions); probe data is a first-class Prometheus series next to the RED/host metrics (one Grafana, one alert engine, one dashboards-as-code story); fully **IaC** (compose + provisioning YAML). | Probes from **one vantage** (the box itself) — it cannot see a failure that takes the whole box down (that is exactly why CloudWatch is the independent out-of-band eye); no global multi-region view. | **The deep app-health checks**: server `/healthz`, site index, ws-origin TCP — the committed default. |
| **AWS CloudWatch Synthetics (Canaries)** | Managed Lambda-based canaries (Node/Puppeteer or Python/Selenium) that hit a URL/flow on a schedule from AWS, with CloudWatch alarms. | Runs **off-box** (survives a box outage); **scriptable browser flows** (real UI, screenshots, multi-step); native CloudWatch alarms/SNS. | **Not free** — canary runs + Lambda + per-canary cost (~$0.0012/run, ~hundreds of runs/mo each) on top of our free-tier discipline; another system to own; heavier than a liveness ping; AWS-region vantage only. | **Not used** — cost + redundancy with the host CloudWatch alarms we already run for the out-of-band box eye; the deep checks are cheaper self-hosted. |
| **Cloudflare Health Checks** | Cloudflare polls an origin/endpoint from its **global PoPs** and can alert (and steer load-balancer traffic). Configured in the Cloudflare dashboard / IaC. | Runs **from Cloudflare's edge, globally** (true external + multi-region vantage); **$0 on the standard plan** for basic health checks; sees DNS/edge-TLS/tunnel exactly as a user does; integrates with Cloudflare LB/notifications. | **Edge-only** — checks a public URL, not arbitrary internal routes / the ws-origin TCP / DB-backed deep state; tied to Cloudflare; less expressive than Blackbox/canaries for app-level assertions. | **Noted as the EDGE/site option** — the right tool for "is the public site/edge reachable from the world", complementary to our deep internal checks. A **separate lane** wires Cloudflare (`deploy/pulumi`); our `blackbox-https-external` job (§5.1) is the cheaper same-box stand-in for the same edge view (one vantage, not Cloudflare's global PoPs). |

**The shape, then:** self-hosted Blackbox for both the **deep internal** application checks (always
on) **and** the **single-vantage external** edge check (the `blackbox-https-external` job — DNS +
Cloudflare + Tunnel, on when a domain is set); the **host CloudWatch alarms** (already in the deploy)
as the **independent out-of-band box eye**; and **Cloudflare Health Checks** as the upgrade to a
**global multi-PoP** edge vantage if/when that is wanted — each at the layer it is best at, none
paying for what another already covers.

---

## 6. The convention for adding a metric

When a new signal is worth a metric, follow the shape the catalog already holds:

1. **Emit from the command/event seam, not the engine.** The engine stays pure;
   instrument in the server where it already sees commands/events (`actor.rs` for the
   RED path, the `MatchMetrics` accumulator on `Session` for per-match game facts). A
   per-match accumulator sees every batch (player + bot, 1v1 + 2v2) because they all
   pass through `Session::finish_apply` (the one post-apply funnel).
2. **Low-cardinality labels ONLY.** Seats, enums, booleans — small, bounded value
   sets. **NEVER** a per-match id, a user/seat token, a card id, a tile, a score, or
   any unbounded/identifying value as a label (it explodes the time-series count and
   leaks). When in doubt, drop the dimension or bucket it.
3. **Redaction holds (AGENTS.md #2).** A metric is an aggregate count or duration. The
   **seed never enters a metric** (it never enters an event or view either), and
   neither does a hand, a deck order, an Echo pre-knowledge, or any per-player private
   state — not as a value, not as a label.
4. **Match the dashboard query exactly.** If a dashboard panel already names the
   intended metric/label, make your emission resolve *that* name (remember the
   Prometheus mapping: dots→underscores, `_total` on counters, `_bucket` on
   histograms). If code and dashboard must diverge, change BOTH consistently.
5. **Keep classification testable.** Split the *decision* (which event means what)
   from the OTel *emission* (the thin `.add()`/`.record()` wrapper) so a unit test can
   assert "the right event drives the right metric" without a live collector — the
   `MatchMetrics` accumulator is the pattern (`metrics.rs` tests).
6. **Update this doc and the relevant dashboard in the same change**, and add a row to
   the right table above.

---

## 7. Verifying locally

`make up` brings up `grafana/otel-lgtm` + the server (which sets
`OTEL_EXPORTER_OTLP_ENDPOINT`, so it exports). Grafana is on `:3000`; the three
dashboards are pre-provisioned. `make seed` (or any play) produces command/match
traffic; drive a few matches (e.g. the bot fleet, or `recollect` vs the server bot) to
populate the §16 panels. The unit + integration tests in
`crates/recollect-server/src/{metrics,session}.rs` assert the accumulator classifies
events and tallies a full match correctly; the live dashboards are walked at release
time per `docs/manual_verification.md` (the observability stack section).

**Synthetic monitoring (§5).** The probe + alert config is gated by static validation that mirrors
what runs on the box: `promtool check config` on `prometheus.yaml` (use the promtool bundled in the
pinned `grafana/otel-lgtm` image — it matches the box's Prometheus version), `blackbox_exporter
--config.check` on `blackbox.yml`, and `docker compose … config` on the deploy + local overlays. The
alerting provisioning is exercised by booting the pinned otel-lgtm image with the
`provisioning/alerting/` mount and confirming Grafana logs `finished to provision alerting` with no
error (the four `recollect-probe-*` rules and the `recollect-oncall` contact point then exist). To see
a probe go red by hand, bring up the deploy stack locally with the observability services **not**
scaled to zero and stop the `server` container — `probe_success{probe="server-healthz"}` drops to 0
and the rule enters Pending→Firing after `for:`. The live edge/alert delivery path is walked at
release time per `docs/manual_verification.md`.
