# Recollect — engine, server, and clients

Deterministic Rust core shared by server (authoritative), web (wasm), and
mobile shells. Start here: `architecture.md` (how the code fits together) and
`AGENTS.md` (the contributor guide).

The Rust workspace lives in **`app/`** (the repo root holds the docs, ops, and
the Docker files). The `make` targets below run `cargo` inside `app/` for you;
to call `cargo` directly, `cd app` first.

    make test                # fast suite, no infra (model-check: make test-verify)
    make up                  # local stack: Postgres + Grafana LGTM + server
    make server              # run the server (= cd app && cargo run -p recollect-server)
    ./scripts/kind-integration.sh   # ephemeral-cluster integration (CI parity)


## Local development — run the server and play a match

Everything below uses the in-memory server (no database, no Docker). The
server is authoritative; every client embeds the same `recollect-core`, so a
client's preview can never diverge from the server's ruling.

### 1. Start the server

    make server                      # or: cargo run -p recollect-server
    # listens on 0.0.0.0:8080 (override with BIND_ADDR=127.0.0.1:9000)

In-memory by default — match state lives in the process and is lost on
restart. Set `DATABASE_URL` to use the Postgres event store instead.

There is **one** client binary — `recollect` (the `recollect-cli` crate). It
spans two orthogonal axes: *transport* (a local in-process engine, or
`--server URL` to talk WebSocket to the authoritative server) and *interface*
(an interactive **TUI**, the default, or a **headless** machine mode). All modes
embed the same `recollect-core`, so a preview can never diverge from the
server's ruling, and every mode renders only its own seat's view (redaction
holds — you never see the opponent's hand).

### 2. Play locally with no server (TUI, vs. AI or hotseat)

    cargo run -p recollect-cli                  # vs. the AI (you are Seat A) — the default
    cargo run -p recollect-cli -- hotseat       # two humans, one terminal
    cargo run -p recollect-cli -- watch         # spectate two AIs
    cargo run -p recollect-cli -- play --seed 42 --difficulty hard   # reproducible, harder AI

The TUI is the quickest way to see a full match: it renders the board, the
clock, fading/Echo/Held markers, the threatened-reach grid, combat forecasts,
and your legal moves each turn. Local 1v1 drives a **gold cursor** (the terminal
analogue of the web canvas): the arrows move it, **Enter/Space** picks up and
places a spirit or hand-card, **Tab** switches board↔hand — with the numbered
move list always there as the fallback. (Online, 2v2, and headless use the
numbered menu.)

### 3. Play over the network (same TUI, against the server)

The server is authoritative; the CLI in `online` mode is a thin WebSocket
shell. Open **two terminals** alongside a running server:

    # Terminal A — create a match, claim seat A. Prints the match id and the
    # seat-B token to hand to your opponent.
    make client                                 # = cargo run -p recollect-cli -- online new

    # Terminal B — join that match as seat B with the printed values.
    make client-join ID=<match-id> TOKEN=<seat-b-token>
    # or: cargo run -p recollect-cli -- online join <match-id> <seat-b-token>

Point at a non-default server with `--server`:

    cargo run -p recollect-cli -- online new --server http://localhost:9000

Or play the **server's bot** over the wire instead of waiting for a human — the
server fills Seat B with the AI and drives it (pick a tier with `--difficulty`):

    cargo run -p recollect-cli -- online new --vs-bot --difficulty hard

Or open a **2v2** lobby — it prints the other three slot tokens to hand out;
each player joins their slot and plays on the 6×6 board:

    cargo run -p recollect-cli -- online new --2v2
    cargo run -p recollect-cli -- online join <match-id> <slot-token>

Online play shows the full legal-move menu (the server computes it): type a
move's number, or use the terse verbs.

### 4. Headless (machine / sim) — no terminal UI

For bots, scripts, and CI, the same binary runs without the TUI:

    # Self-play one seeded match to a result (add --ndjson for the event stream):
    cargo run -p recollect-cli -- autoplay --seed 42 --difficulty expert

    # JSON-lines protocol: read one Command JSON per line on stdin, emit the
    # resulting PlayerView / result as JSON. Local (AI opponent) or --server.
    cargo run -p recollect-cli -- headless --seed 42

### 5. The web UI (browser)

The browser client (`app/crates/recollect-web/`) is a wasm shell over the same
core, with **two modes**:

- **Local** — runs the engine in your browser, you play the in-browser AI (the
  full legal-move menu, card inspect, reach grid; a 2v2 watch mode too).
- **Online** — connects to a running `recollect-server` over WebSocket and plays
  the authoritative match: pick **"vs the server bot"**, **"create (vs human)"**
  (hands you a seat-B token to share), **"create 2v2"** (hands out three slot
  tokens; plays the 6×6 board), or **"join…"**. The server ships the legal-move
  menu with each view, so online play has the full move set as buttons.

Three files, easy to confuse:

- `recollect-web/index.html` — the real **trunk** entry (the wired wasm game).
- `recollect-web/shell/index.html` — a **static design mock** (open in any
  browser, no toolchain; a shape preview, not the playable game).
- `recollect-web/dist/` — trunk **build output** (gitignored).

Build and serve (trunk listens on **127.0.0.1:8088**, so it does not clash with
the server's 8080):

    cd app/crates/recollect-web && trunk serve   # then open http://127.0.0.1:8088

For online play, also run the server (`make server`) and enter its URL
(`http://localhost:8080`) in the picker. The server sends permissive CORS so the
trunk-served page can reach it cross-origin; tighten the allowed origin for a
real deployment.

### Common loop

    make test               # fast suite (no infra)
    make catalog-check      # the catalog must match the card source (cards.toml)
    make sim N=10000        # headless balance simulation
    cd app && cargo run -p recollect-bot --bin fleet   # the evidence fleet (fairness, texture, evolution)


## For contributors — humans and agents

Start at **[AGENTS.md](AGENTS.md)** (CLAUDE.md points there). The design and
cards documents in `docs/` are the law; `docs/testing.md` and
`docs/operations.md` cover the test taxonomy and the ops stack. `make help`
lists every entry point — notably `make server` + `make client` for network
play, `make up` for the full compose stack with Grafana, and `make test`
+ `make catalog-check` as the gate before any change lands.

Security: see **[SECURITY.md](SECURITY.md)** for the threat model and how to
report a vulnerability (the test-pinned per-threat table lives in
`docs/tech_design.md` §9).
