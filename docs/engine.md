# engine.md — a guided tour of `recollect-core`

The map for anyone (human or agent) about to change the rules engine: **where
each thing lives**, **how a turn flows**, and **how a card becomes events**, so a
change lands in the right place with the right test. The design docs
(`design.md` for the rules, `cards.toml` for the card data + `cards_design.md`
for the card design) are the law; this explains how the law is implemented.

## The one-paragraph mental model

A match is an **event-sourced aggregate**. Every change goes through
`decide(command) -> Result<Vec<Event>, Reject>`, which computes what *should*
happen on a working clone, then `evolve(event)` applies each event mechanically to
real state. **All rules live in `decide`; `evolve` is dumb application.** Entropy
is counter-mode (a seeded position, never in any event or view), so the same seed
+ commands always produce identical state and events. Clients only ever see a
redacted `PlayerView` — never raw `GameState`.

## The engine, by module (`src/engine/`)

`engine.rs` was one 8,600-line file; it is now **18 focused modules** (none over
~1000 lines), all sharing the crate types via `use super::*`:

| Module | Holds |
|---|---|
| `mod.rs` | the `Engine` + `Decided` structs, the public API (`apply`, `decide_journaled`, `snapshot`, `resume`), the `AggregateRules` glue, and small shared utils (`push`, `card`). |
| `decide.rs` | the `decide_impl` **dispatcher** + the smaller command handlers (Glimpse — `decide_glimpse`, BanishStray, Reclaim, Evolve, **Devolve** — `decide_devolve`, Reveal, SetOrders, Choose, EndTurn), the §5 opening `Mulligan` (`decide_mulligan` + the `mulligan_window` gate), and the `MatchAbandoned` forfeit. |
| `decide_spellbook.rs` | spellbook handlers — `CastRitual`, `AttachBond`, `PlaceLandmark`, `SetFabrication`. |
| `decide_arrival.rs` | arrival + movement handlers — `PlaySpirit`, `Overwrite`, `StrikeFabrication`, `MoveSpirit`. |
| `evolve.rs` | the `AggregateRules` impl — applies each `Event` to `GameState`. Dumb, total, no validation. |
| `combat.rs` | `full_exchange`, `interception`, `momentum_prefs`, `forecast_exchange` — the arrival/exchange algorithm. |
| `combat_stats.rs` | `combat_stats` — the derived-stats **fold** (base stats + each `cs_*` aura/bond/terrain/keyword scan). |
| `effects_exec.rs` | the effect-interpreter **core**: `effect_targets`, `exec_clause_mode` (choice-routing coordinator), `apply_direct_clause` (the `match &cl.effect` executor). |
| `effects_fire.rs` | the `fire_*` dispatch + target resolvers (`release_targets` = mercy, **fading-only**; `banish_targets` = the IllIntent erasure, any state; `restore_targets`), `spring_fabrication`, dissolution, suppression predicates. |
| `effects_phases.rs` | the per-phase `exec_*` handlers `exec_clause_mode` dispatches to (target-spirit, copy/control/strip, reveal-fab, …). |
| `clause.rs` | `apply_clause_at`, keyword/Warded/exception predicates, `banish_or_replace`, `push_away`, `apply_choice_effect` (the choice dispatcher). |
| `choice_effects.rs` | the resolved-choice `choice_*` handlers (§21 Standing Orders). |
| `aura_helpers.rs` | static auras / standing-spirit grants — bond-pair shares, terrain/landmark deltas, cost & push-immunity auras. |
| `throughline.rs` | the §4 Throughline subsystem — imprint links, the run/grant, the +10/+10 reward riders. |
| `strays.rs` | Strays (D-7) — temperament, surfacing, courtship, the Echo-wounded check. |
| `flow.rs` | the turn flow — Flow/Fade/**Main** (the player-facing name for the `Phase::Acting` state), anima income, round advance, the Dusk/Held-Ground contraction, `finish` (scoring). |
| `projection.rs` | reach geometry: `reach_tiles`, `oriented_w`, placement `projection`, `eff_defense`. |
| `conditions.rs` | `manhattan` + the `condition_holds` spec-condition predicates. |

## The other core modules (`src/`)

- **`state.rs`** — the data: `GameState`, `Spirit`, `TileState`, `Terrain`,
  `Stray`, the `Command` enum, `ChoiceEffect`, `MatchRules`. The `Event` enum is in
  the `state/events.rs` submodule (re-exported, so `state::Event` paths are
  unchanged). A new command/event is defined here AND handled in `decide`/`evolve`.
- **`effects.rs`** — the effect **IR** (see below); the `supported_*_clause`
  predicates (which clauses the engine implements) are in the `effects/support.rs`
  submodule.
- **`cards.rs`** — `canon_catalog()` loads embedded `data/catalog.json` (generated
  from the cards doc — never hand-edit it); `key_of(name)` resolves a display name
  to its stable `key`.
- **The Solace (PvE)** has no special module: under the uniform-seat model the
  **Solace is just another faction** — seat B draws a Solace deck and plays
  Unwritten/IllIntent/Unwriting through the same `decide`/`evolve` path as any player.
- **`invariants.rs`** — `check(state)`: the one state-validity suite, shared by the
  fuzz, props, and the stateright model-check (see `docs/testing.md`).
- **`view.rs`** — `PlayerView`/`TeamView` + redaction; the ONLY thing a client sees.
- **`rng.rs`** — counter-mode seeded RNG; determinism depends on it.
- **`types.rs`** — `CardDef` (incl. the frozen `key`), `CardId`, `Seat`, `SeatSlot`,
  `Reach`, `CardKind`, board geometry helpers.

`GameState` implements `ironstate_aggregate::AggregateRules` — the journal/lifecycle
framework the engine plugs into.

## How a turn flows

1. A client sends a `Command`. `Engine::apply` records the entropy position, calls
   `decide`, and on success commits the events (on `Reject` it re-seeks the RNG, so
   a failed command leaves nothing observable).
2. `decide_impl` validates phase/turn, then dispatches to the `decide_<command>`
   handler. Each handler validates (reach, projection, ownership, affordability) and
   `push`es events onto a working clone (so later steps see earlier ones). Combat
   goes through `full_exchange`; an arrival triggers `interception` then `momentum`;
   instant effects fire via `fire_effects`.
3. **The turn is Flow → Main → Fade** (`flow.rs`). **Acting** (Main) has no action cap: a seat
   Plays / Calls / Overwrites as far as its **Anima** reaches and **Moves** each Mobile spirit once
   (never the turn it arrived — summoning sickness, tracked in `moved_this_turn`), then ends the turn
   explicitly. `EndTurn` runs the turn's tail: the **Fade phase** (now at turn-END, after Main) —
   dissolve the seat's **lingering banished bases** whose standing-Faded deadline has come
   (`dissolve_faded_at`, firing Partings + OnAnyBanish; Hold the Memory may make one skip a turn) —
   then sweep orphan tokens, break Bonds, advance the seat (round/contraction at the wrap), then
   `start_turn` (the next seat's **Flow**: orphan-sweep, bond-break, income + draw, Stray surfacing).
   Bond-breaking is `prune_broken_bonds` (ownership-aware: an endpoint must still hold a standing
   spirit *owned by the bond's owner*), shared by the Flow and by `decide_overwrite` — an Overwrite
   that takes a tile breaks the banished occupant's Bond **immediately**, before Momentum chains, so
   a dead occupant's Promise can't redirect a chain blow onto the enemy overwriter that replaced it.
   **There is NO turn-START Fade step** and **the Dusk is instant** (see below), so every Fading
   spirit is a combat-banished base in its window — `start_turn` never has a fade to process.
   **The standing-Faded window (§0.5):** a base **banished in combat** does not dissolve at once; it
   stands Faded (`fade_deadline` set to the round of its owner's next turn-end) and lingers INTO its
   owner's Main so the owner can **evolve or Devolve** it, dissolving at that turn's **END Fade** if
   unredeemed. **Round 12** has no next owner turn, so the base lingers through the rest of the round
   and is dissolved by the Nightfall `finish` pass **before scoring** (laying the banisher's
   impression so the opponent scores the tile) — `banish_or_replace` does not dissolve it on defeat.
   **The Dusk is decoupled from Fade:** at the round-8 contraction the `MemoryContracted` apply
   (`evolve.rs`) darkens the empty rim AND **dissolves the rim Unwritten immediately** (they leave
   nothing — the Solace-no-mark rule), in that same step — no window, no deferred fade.
   Seat B (the Solace, when bot-controlled) takes its turn through this **same flow** — no
   special-casing; the server's bot loop drives it like any seat.

## How a card becomes events (the Effect IR)

Card behaviour is **data**, not code. `data/effects.json` maps each card `key` to
`EffectSpec`s; the engine interprets them — no per-card branches.

- **`EffectSpec { trigger, condition, clauses }`** — *when* (a `Trigger`: OnPlay,
  OnReveal, Parting, Static, OnAnyBanish, OnUnwrite, …) it fires, gated by a
  `Condition`, producing one or more `Clause`s.
- **`Clause { selector, effect, duration }`** — *who* (`Selector`: SelfSpirit,
  AdjacentEnemiesAll, TargetEnemySpirit, …), *what* (`Effect`: StatDelta, Damage,
  Release, Displace, Summon, RuleException, …), for *how long* (`Duration`).

Execution path: `fire_effects` (instant triggers) → `exec_clause_mode`
(`effects_exec.rs`) routes **choice-bearing** selectors to a `PendingChoice`
(resolved later by `apply_choice_effect` in `clause.rs`) and everything else falls
through to **`apply_direct_clause`**, the `match &cl.effect` that emits the events.
Static clauses are folded into derived stats by `combat_stats` (`cs_*` scans), not
fired. The **`supported_*_clause`** predicates in `effects.rs` declare which clauses
the engine actually handles; the `effects_coverage` ratchet counts them.

**To add a card effect:** author the clause in `data/effects.json` under the card's
`key`; if it needs a new primitive, add an `Effect`/`Selector` variant in
`effects.rs`, handle it in `apply_direct_clause` (or the static fold / choice path),
and extend the matching `supported_*_clause`. Behaviour-test it; the
`card_effects_fire` test proves on-arrival specs emit an event.

## Where each mechanic lives (jump table)

- **Placement & projection** → `decide.rs` (`decide_play_spirit`/`decide_overwrite`); `projection.rs`.
- **Combat / Echo / edges** → `combat.rs`; `eff_defense` in `projection.rs`; `ECHO_*`/`EDGE` consts.
- **Derived stats / auras** → `combat_stats.rs` (the `cs_*` scans) + `aura_helpers.rs` (the static-aura/grant helpers it folds).
- **Throughline (§4)** → `throughline.rs` (`check_throughline`, the run/grant/riders).
- **Evolution** → `decide_evolve` + `data/evolution_{split,lines}.json`. **Play-from-hand:**
  the Primal/Fabled form is a deck-playable card (`CardKind::Evolution.deck_playable()`) you draw and
  play onto its base. `Command::Evolve { tile, form_hand, fuel, engage }` names the form card in hand;
  `decide_evolve` checks it's a form whose `evolves_from` is the base, the shared-Imprint rule admits
  it, and the base-state↔form-type pairing holds (Primal←Fading, Fabled←healthy-post-arrival), then
  consumes the form from hand (the `SpiritEvolved` event carries `seat`; `evolve` removes the card).
  `legal_commands` enumerates playable forms in hand against eligible bases. Deck-gen pairs base↔form
  (no orphan forms — `quickplay::ensure_evolution_floor` + the `validate_deck_for` orphan check).
  **The no-chain lock (§5):** a base evolves to a Primal *or* a Fabled — a Primal cannot evolve to a
  Fabled. Both branch from the *base*; `legal_evolutions` returns nothing for a base that is itself a
  form (`evolves_from` set), so `decide_evolve` rejects a form-onto-form. The canon carries no chains
  (guarded by `canon.rs::no_evolution_chains_…`); a base may offer >1 form of one tier (a branch
  choice, filtered per-base by `evolution_split.json`).
- **The Solace Deepenings (§5)** → 8 Solace **Primal** forms (the Solace deepens, never ascends — no
  Fabled). Each branches from an Unwritten/IllIntent base (`evolution_lines.json`, split `["Primal"]`),
  is **deck-playable for the Solace** (`CardKind::deck_playable_for(Solace)` now admits `Evolution`;
  `validate_deck_for` enforces Primal-only + the no-orphan pairing), and is authored in `effects.json`
  (the ratchet counts them). Solace deck-gen seeds base↔Deepening pairs through the same
  `ensure_evolution_floor` (the primary draw zeroes `Evolution` for both factions). Authored in
  `cards.toml` (the Deepenings — Neutral `Evolution` Primals with a `[card.evolution]` base/split).
- **Devolution (§5)** → `decide_devolve` (`decide.rs`). The **rescue**: `Command::Devolve { tile,
  base_hand }` recedes a **standing-Faded form** you own (a Primal/Fabled banished in combat, still in
  its §0.5 window — `fading` + `fade_deadline` Some) to a **base in its line** you hold (a hand card
  whose name is the form's direct `evolves_from`; lines are 2-stage, so the no-chain lock makes this
  exact). Cost = **⌊form.cost/2⌋** (no cost-aura — priced off the form). It **IS an arrival, symmetric
  with evolution** (the maintainer's ruling): the recede fires the same arrival triggers — `check_throughline`
  (a base receding into a standing 3-line **re-completes on the spot**) and `apply_next_arrival` (a queued
  Kindle/Again! buff lands on the base) — but **engages no one** (no strike target) and fires **no OnPlay**;
  the base arrives **full-HP**, fade cleared, **summoning-sick**
  (`evolve` marks `moved_this_turn`, blocking move/evolve until the owner's next turn). A distinct
  `Event::SpiritDevolved { seat, tile, from, to, … }` (`evolve` removes the base card `to` from hand);
  redaction-safe (the opponent sees the recede + the resulting base, never the rest of your hand).
  `legal_commands` offers it for any standing-Faded form with a matching base in hand. Vocabulary:
  the Lorekeeper **reverts**, the Solace **recedes** (one engine action, the faction's verb in
  `protocol::label` / the web labeler / the CLI). A spirit may cycle evolve↔devolve without limit. The
  canvas/web recede *glyph* is a deferred follow-up; the action is playable + labelled today.
- **The standing-Faded window + round-12 (§0.5)** → `clause.rs::banish_or_replace` stamps a combat
  fade (`SpiritBecameFading` with `banished_by`); `evolve` sets `fade_deadline` via
  `flow.rs::fade_deadline_round`; the dissolve fires at the owner's turn-**END** in `end_turn`
  (`dissolve_faded_at`). **Round 12** has no next owner turn, so a banished base does **not** dissolve on
  defeat — it lingers standing-Faded through the rest of the round and the Nightfall `finish` pass
  dissolves it **before scoring**, laying the banisher's impression so the opponent scores the tile.
- **Spellbook** (Ritual/Bond/Landmark/Fabrication) → the `decide_*` handlers; `spring_fabrication`.
- **Effects / choices / RuleExceptions** → `effects_exec.rs`/`effects_fire.rs`/`effects_phases.rs` + `clause.rs`/`choice_effects.rs`; IR variants in `effects.rs`.
- **Mulligan (§5, opening)** → `decide.rs` (`decide_mulligan` + `mulligan_window`): once per seat, round 1, before that seat acts — reshuffle, draw a fresh full hand, bottom one (seed-chosen). `Command::Mulligan { seat }` → `Event::Mulliganed { seat, hand, deck }` (the new order rides the event so `evolve` touches no entropy); the public `mulliganed[2]` beat is in `GameState` + the view (`PlayerView`/`TeamView`), the cards redacted. 1v1 only.
- **Glimpse (§5, burn-then-peek-then-spend)** → `decide.rs` (`decide_glimpse`): the turn's one Glimpse is not free — to activate it you BURN a chosen hand card (the activation cost; the card leaves play, self-thinning the 20-card deck), THEN peek the top and keep-or-bottom it. Two choices through the shared `PendingChoice`/`Choose` flow. `Command::Glimpse` emits `Event::Glimpsed` (marks the once-per-turn flag) + `Event::ChoiceOffered { GlimpseBurn { seat, burnable } }` (step 1 — `burnable` is the hand, owner-visible, redacted in `view.rs`; the field is NOT named `hand` so it never trips the `"hand":` leak probe). `decide_choose` on `GlimpseBurn`: `Choose { index }` emits `Event::GlimpseBurned { seat, hand_index }` (`evolve` removes that card from the active slot's hand) + `Event::ChoiceOffered { Glimpse { seat, top } }` (step 2 — the top peeked AFTER the burn; the burn doesn't touch the deck, so it's the same top). `decide_choose` on `Glimpse`: `Choose { index: 0 }` KEEPS (the card stays on top, `peeked_top` set, no Anima); `Choose { index: 1 }` BOTTOMS it (`evolve` rotates the top card under) + `AnimaGained { +1 }`; the resolution rides `Event::GlimpseResolved { seat, kept }`. Net: keep = −1 card, bottom = −2 cards for +1 Anima. Offered only with a NON-EMPTY HAND (nothing to burn → `Reject::NothingToBurn`) AND a NON-EMPTY PAGE (nothing to peek → `Reject::NothingToPeek`); the burnable hand, the burned card, the peeked card, and the chosen options never reach the opponent's view (counts/beats only).
- **The Dusk / Held Ground** → `flow.rs` contraction logic; `is_rim_w` (width-aware).
- **Strays / Foundlings** → `strays.rs` (`advance_courtship`, surfacing); `combat.rs` (`feral_stray_intercepts`).
- **The Solace** (PvE) → no special module; seat B fields a Solace faction deck (`quickplay`), its Unwritten/IllIntent/Unwriting effects living in `effects.json` + `effects_exec.rs` like any card.
- **2v2** → `new_2v2`, `player_slot`, `active_slot`, `placed_by`; `TeamView` in `view.rs`.

## Rules for changing things (also in AGENTS.md)

- **Rule change** → design doc first, then the engine, then a test. **Card change**
  → cards doc → `make catalog` (never hand-edit `catalog.json`).
- **New command/event** → define in `state.rs`, handle in `decide`/`evolve`, AND
  keep the exhaustive matches in sync: the bot, `recollect_protocol::label`, the CLI
  verb parser, and the web LocalGame labeler.
- **New `Spirit`/`GameState` field** → `#[serde(default)]`, then the constructors +
  the `common/mod.rs` test builders.
- **Red tests are contracts** (`tests/*_backlog.rs`) — implement, never delete.
- **The ratchet** (`tests/suites/effects_coverage.rs`) only moves toward done.
- Finish with `make test && make catalog-check` green (`make test-verify` for the
  model-checker; `make nightly` for the soak/mutants/golden suite).
