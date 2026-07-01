# RECOLLECT — How the Code Fits Together

One covenant across every crate: **same seed + same commands ⇒ same state, on
every platform, forever.** Everything below is in service of that sentence.

## The crates

```
recollect-core             the rules. sans-I/O, no floats, no HashMap, no clocks.
recollect-protocol         the wire envelopes (versioned ClientMsg/ServerMsg over serde).
recollect-server           axum websocket host: an actor task per match (owns the Session, per-seat mpsc fan-out), seat tokens, seq anti-replay.
recollect-cli              the `recollect` binary: local/online play, TUI or headless.
recollect-bot              headless agents: random-legal selfplay, balance instrumentation.
recollect-web              wasm canvas shell: the SAME core, in the browser — local (vs AI), online (ws to the server), and 2v2 on the 6×6 board.
recollect-journal-postgres accounts + the authoritative async event journal (store::AsyncStore).
recollect-determinism      the wasm32 differential: a seeded playout hashed native and under wasmtime.
recollect-ffi              UniFFI native bindings over the core (Swift/Kotlin).
recollect-verify           the stateright model-check of the bounded state space (1v1 + Solace PvE + 2v2).
```

## recollect-core, inside

- **types.rs** — Seat, Resonance (the wheel), Reach offsets, CardDef, board math.
- **state.rs** — `GameState` (the aggregate), `Command` (player intent, never
  trusted), `Event` (self-sufficient facts; whole-enum versioning policy in the
  doc comment), `Phase` (Acting / PendingRelease / Finished).
- **engine/** — every rule, in the Ironstate family shape. Split from one 8.6k-line
  file into **18 focused modules** (`decide`/`decide_spellbook`/`decide_arrival`,
  `evolve`, `combat`/`combat_stats`, `effects_exec`/`effects_fire`/`effects_phases`,
  `clause`/`choice_effects`/`aura_helpers`/`throughline`/`strays`, `flow`,
  `projection`, `conditions`); see `docs/engine.md` for the map.
  - `decide(&state, cmd, ctx) -> Result<Vec<Event>, Reject>` — validation plus
    resolution. It runs on a CLONE, applying each event via `evolve` as it is
    recorded, so decide-time simulation and replay agree *by construction*.
  - `evolve(&mut state, &event)` — total, mechanical, draws nothing.
  - `Engine` — the Aggregate wrapper: `apply` = decide → evolve-all, with
    rewind-on-rejection (a failed command leaves no observable trace, not even
    an entropy draw). `why_not` runs decide on a probe stream. `legal_commands`
    enumerates moves for bots, props tests, and the ink wash.
- **rng.rs** — counter-addressable seeded entropy (`at(seed, pos)` is an O(1)
  seek). The draw counter is the journal's; snapshots are `(state, DrawPos)`.
  The seed lives beside the journal, never in state, never in an event.
- **`ironstate_aggregate`** — the published aggregate runtime (crates.io 0.1.1).
  recollect's `GameState` implements its `AggregateRules`; `lib.rs` re-exports
  `{AggregateRules, DrawPos, EntropySource}` at the crate root.
- **view.rs** — per-seat redaction by construction: your hand, their counts.
- **quickplay.rs** — seeded style offers and style-weighted deck generation;
  pure in (style, seed, catalog), so decks are derived everywhere, sent nowhere.
- **cards.rs** — `canon_catalog()`: the full generated catalog (all 419 cards),
  `include_str!`'d from `data/catalog.json` and parsed at startup. Never
  hand-edited — edit the `[[card]]` in `data/cards.toml`, run `make catalog`.

## One command's life (online)

client builds Command → protocol envelope (version, seq) → the socket task routes
it (token → seat) to the match's **actor task** over an mpsc channel → the
actor (the single, lock-free owner of the `Session`) does **append-before-ack**:
`Engine::decide_journaled` runs `decide` (advancing entropy, not yet evolving), the
events + post-decide `DrawPos` are appended to the journal (`store::AsyncStore`),
and only on the durable `Ok` does `Decided::commit` evolve the state and the seat
get its ack (`Session::apply_journaled`). A failed append rewinds and the command
is refused — nothing observable changed. Then the actor pushes each seat its own
`view_for` over that seat's **per-seat mpsc** sender (the acting seat's ack frame
is the reply to its command; the opponent's `Update` is fanned out per-seat) →
wasm client applies the same events to its local mirror. With no `DATABASE_URL`
the actor degrades to the in-memory `Engine::apply` (no durability, same rules).
Disagreement is impossible in honest clients and *detectable* in dishonest ones:
replaying the journal through the same core re-derives everything, AI moves
included.

**Actor-per-match.** One owning task per live match holds the `Session` by
value — there is no `Mutex<Session>` (no lock to slip an `.await` under) and
no lossy `broadcast` (per-seat mpsc never drops a neighbour's frame). The
socket task is a thin pump: it `subscribe`s its seat (the actor registers a
per-seat sender and replies with the welcome), then selects between actor-pushed
frames and incoming socket text (forwarded to the actor, whose reply is the ack).
The actor outlives any one socket and addresses fan-out per-principal — the seam
reconnection builds on; it dispatches by mode (1v1 vs 2v2). See
`recollect-server/src/actor.rs`.

## Offline / Quick Play / AI

Quick Play offers three seeded styles; the chosen style + seed derive the deck
(legal by construction: size, copy cap, opening curve). Vs-AI uses a
deterministic policy drawing from the seeded stream — today random-legal, later
the int8 policy net — which is why offline results can be uploaded, replayed,
and trusted (see `docs/decisions/bot_and_ml_plan.md`).

## Mobile (when we get there): FFI, one core, native shells

The same `recollect-core` ships to phones via FFI — Rust compiled as a native
library, called from Swift and Kotlin through generated bindings (UniFFI is
the planned generator: one interface definition → Swift + Kotlin, memory and
panic safety handled at the boundary). The boundary stays narrow and
message-shaped — commands in, views/events/forecasts out — exactly the API
the TUI already proves sufficient. Everything platform-flavored lives in the
native shell: notifications, haptics, share sheets, widgets, accessibility,
store. iOS: static lib in an XCFramework. Android: per-ABI `.so` via
cargo-ndk, JNI glue generated. The determinism covenant extends with the
targets: aarch64-apple-ios and aarch64-linux-android join the golden-digest
CI the day mobile starts.

## Wire format and observability

postcard is the wire and journal payload format (measured on our types:
~16x smaller than JSON on views, ~13x on event batches; frozen 1.0 spec, same
serde derives, byte-stable per schema version — pairs with whole-enum event
versioning). Observability is traces + the journal itself, not a second wire
format: any stored batch decodes through the same types in offline tooling.
JSON survives only at the wasm↔JS boundary until that moves to structured
values, and in tools' output.

## Invariants — what must hold, what enforces it, what tests it

| Invariant | Enforced by | Tested by |
|---|---|---|
| Same seed+commands ⇒ same state+draws, all platforms | counter-mode entropy, no floats/HashMap/clocks (review + lints) | determinism.rs; recollect-verify "decide is deterministic" (re-run identical at every reachable state, in 1v1 + 2v2); recollect-determinism hashes a seeded playout identically native and under wasmtime, gated cross-target in CI (`make wasm-diff`) |
| decide is pure; evolve is total and draw-free; replay ≡ live | decide-on-clone via push≡evolve construction | decide_evolve_replay_equivalence |
| Rejection leaves NOTHING observable (state, entropy) | apply's rewind-on-Err | failed_command_leaves_no_position_change; security.rs |
| decide is total over hostile input (no panics, any Command) | validation-first decide | security.rs fuzz (garbage commands ×8000) |
| The seed appears in no state, no event, no view | seed lives in the stream/journal only | seed_appears_in_no_state_and_no_event; recollect-verify "the seed leaks into no state, event, or view" (every reachable state — both seats in 1v1, all four slots in 2v2) |
| Hidden info never crosses seats (hand, deck order, peek) | view_for / view_for_slot redaction-by-construction | redaction.rs; recollect-verify "the opponent crosses only as truthful counts" (every reachable state; `view_for` for the two 1v1 seats, `view_for_slot` for all four 2v2 slots); family leak_test! on delivery |
| legal_commands ⊆ decide-accepts (enumerator soundness) | shared validation paths | props.rs random playouts under DEFAULT and every MatchRules variant |
| Every match terminates; scores ≤ 25; hp ≤ hp_max | clock in rules; engine math | props.rs; bot selfplay ×1000 |
| Dealt decks are always legal (size, copies, curve) | quickplay generation + validate_deck debug_assert | quickplay.rs ×300 |
| Wire bytes are stable per schema version | postcard frozen spec + whole-enum event versioning | wire_formats.rs |
| Stale/replayed client messages rejected | protocol seq + server session | session tests |
| Supply chain pinned | deny.toml (licenses/advisories/sources), exact-pin wasm family | cargo-deny in CI |
| No seat waits forever (live matches end) | MatchAbandoned system command via the lifecycle tier | m1_backlog abandonment test; recollect-verify "abandonment forfeits to the present seat" (MatchAbandoned ⇒ Win(present) at every reachable state, in 1v1 + 2v2) |
| Mulligan (§5) reshuffles cleanly and never leaks the hand | `Mulligan` gated to the opening window (round 1, untouched seat, once); new order rides `Event::Mulliganed` so evolve draws no entropy; cards redacted by view_for | mulligan.rs (mechanic, determinism, replay, redaction); recollect-verify "a mulligan reshuffles cleanly and never leaks the hand" (every opening-frontier state); server session.rs (opponent view redacted) |
| End-to-end: views are per-principal prefixes; no leak across reconnect/redelivery; subscriptions converge to ReferenceRun | server outbox discipline | ironstate testkit + turmoil, three oracles (M2 gate) |

Standing policy: every new `MatchRules` variant ships with a props arm.

`recollect-cli` is the single client binary `recollect` — local engine or
`--server` WebSocket, interactive TUI or headless JSON/autoplay.
`recollect-journal-postgres` carries two journals: `Journal` (accounts, the
`matches` metadata row [seed, result], and the `match_participants` roster — who
played each seat, name-tagged: handle + session id + a nullable `account_id` an
account later claims) and `store::AsyncStore` — the **authoritative** async event
journal (`execute_async`: append-before-ack via ironstate's
`prepare`/`commit`/`abort`; `resume_async` rebuilds from the head). The
seven-property storage proof runs over the same schema through a synchronous
`Journal` twin (`tests/journal_contract.rs`, `make db-test`). With `DATABASE_URL`
set the server routes every command through `Session::apply_journaled` (the
engine's `decide_journaled`/`Decided` seam + `AsyncStore::append`), so a command
is durable before it is acked and a fresh process resumes a match from the journal
alone (`a_journaled_match_resumes_bit_identical`, `make db-test`); with no
`DATABASE_URL` it degrades to the in-memory engine. The per-command event record is
`journal_events`. The `MatchAbandoned` system command forfeits a stalled match to
the present seat, and recollect-verify asserts the `Win(present_seat)` outcome on
every reachable state.

Contributor entry point: AGENTS.md (CLAUDE.md points there); test taxonomy in
docs/testing.md; ops in docs/operations.md. CI: `.github/workflows/ci.yml` runs
tests + catalog gate + determinism + pg integration + rustdoc + cargo-deny + wasm
(compile + native==wasm differential + bundle-size) + helm lint + docker + the
Playwright UI suite. `deploy-image.yml` builds and pushes the server image to ECR
via OIDC (the FOUNDATION CI half).

**Model-check scope (`recollect-verify`).** The one `EngineModel` (`model.rs`) bridges
the real `recollect-core` aggregate to stateright and runs in THREE shapes, selected by
`Mode`: **1v1 duel**, **Solace PvE** (the two differ only by seat B's faction), and
**2v2 team** (M3 — the four-slot 6×6 match: init `new_2v2_with_opener`, actions from
the active slot's team via `legal_commands(active_slot.team())`, redaction from all four
slots via `view_for_slot`). Every property (validity, liveness, determinism, no-seed-leak,
redaction, abandonment) is asserted exhaustively on every reachable state of each mode's
bounded frontier. The 2v2 board (6×6 × four hands) branches hard, so it runs a TIGHT bound
(3-card decks, `max_round = 2`); `tests/solace_bridge.rs` is the fast CI slice of all three,
and the `solace-modelcheck` binary the deeper sweep. (Before M3 only the two 1v1 modes were
model-checked; the 2v2 path had no formal coverage.)

Verification routing for future features (recorded so a new feature queues behind
the right tool): **Solace PvE escalation** → re-run the aggregate stateright red
team (round-coupled spawning is the one shape that could give branch exploration
something to bite). **Hundredname cross-match persistence** → not bridge-shaped:
it wants the **multi-journal ReferenceRun** (cross-journal conservation under
at-least-once delivery).

## The test suites as a map

rules.rs (the v1.2 law as spec sentences) · determinism.rs (replay equivalence,
snapshot resume, counter seek, no-seed-anywhere) · redaction.rs ·
mulligan.rs (§5 opening reshuffle: mechanic, determinism, replay, redaction) ·
props.rs (random playouts, invariants) · quickplay.rs · m1_backlog.rs (the red
tranche: interactive windows, Strays, Lurk, Kindred, evolution, the Solace).
