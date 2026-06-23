# rules_coverage.md — what's tested, and how

This is the master map from **every rule and behaviour** to the tests that
guard it. It answers "is X tested? is X fuzz-tested?" at a glance. Two columns
matter:

- **Targeted test** — a test that constructs the exact situation and asserts the
  specific outcome (the contract). Named, fails loudly, easy to read.
- **Fuzzed** — whether the gameplay fuzzer (the full-catalog playthrough,
  `tests/suites/fuzz.rs`, `make fuzz`) exercises this rule under
  random/biased play while the invariant suite watches. Fuzzing catches
  *interactions* and *unreached states*; targeted tests pin *contracts*.

Both layers matter: targeted tests prove a rule does the right thing in the case
we thought of; fuzzing proves nothing breaks in the cases we didn't.

## The testing layers (where each kind of test lives)

| Layer | File | What it guarantees |
|---|---|---|
| Rules contracts | `tests/rules.rs` (25) | Core rules: arrival law, combat math, Echo, Momentum, projection, the Dusk. |
| Keyword behaviour | `tests/keywords.rs` (10) | Each combat keyword's contract, asserted directly. |
| Card effects (data) | `tests/effects_engine.rs`, `effects_choices.rs`, `spellbook.rs`, `summon.rs` | Authored card effects resolve and target correctly. |
| Effect execution coverage | `tests/card_effects_fire.rs` | Every on-arrival/reveal spec actually fires — no silent no-ops. |
| Ratchet (honesty meter) | `tests/effects_coverage.rs` | Implemented-vs-data counts only move toward done. |
| Evolution | `tests/evolve.rs` (21) | **Play-from-hand**: the form is a held card played onto its base — consumed from hand, can't evolve without it, wrong-base form rejected; plus the Primal/Fabled split, donors, the discounted charge, arrival-engage; **the no-chain lock** — a Primal cannot be evolved into a Fabled (form-onto-Primal rejected, never offered). |
| No evolution chains (canon) | `tests/canon.rs::no_evolution_chains_…` | The whole catalog: no form's base is itself a form (base→Primal/Fabled, never base→Primal→Fabled); every `evolves_to`/`evolves_from` resolves to the right kind. |
| Evolution reachable — the standing-Faded window (§0.5) | `tests/d1_evolution_window.rs` (7) | A base **banished in combat** lingers standing-Faded into its owner's Main and is Primal-evolvable there; dissolves at the owner's turn-END Fade if unredeemed (NOT turn-start — there is no turn-start Fade); the timing crux (survives through Main); **a banished base survives its owner's Flow into Main (no turn-start Fade)**; **round-12: it lingers through the round and dissolves in the Nightfall `finish` BEFORE scoring, so the BANISHER scores the tile**; banished-on-own-turn survives to the next turn-end; evolution fires in real playouts (≥30/60 seeds). |
| Turn order **Flow → Main → Fade** + the instant Dusk (§0.5/§5) | `d1_evolution_window.rs::there_is_no_turn_start_fade_…`, `solace.rs::dusk_sweeps_unwritten_from_the_rim_immediately_…`, `choice_engage_fabrication.rs::hold_the_memory_…`, model-check (`invariants.rs` `fading ⇔ fade_deadline.is_some()`) | The Fade phase is at turn-END (after Main): a combat-banished base dissolves at its owner's turn-END, not turn-start. The **Dusk is instant** — at the round-8 contraction the rim Unwritten dissolve AT ONCE (leaving nothing), never deferred. Hold the Memory makes a banished base skip one Fade (extends its window a turn). The strengthened invariant `fading ⇔ fade_deadline.is_some()` holds on every model-checked state (every Fading spirit is now a combat-banished base in its window). |
| Solace Deepenings (§5 — Primal-only forms) | `tests/solace_deepenings.rs` (19) | The 12 Solace Primal forms (8 seed + 4 menu partners): each is a Primal branching from an Unwritten/IllIntent base (never Fabled); each authored effect does its thing (release-the-FADING / Echo-suppression + −Atk / AtFlow ally heal / −20 Def / on-arrival impression-eat that scores / +Atk-per-impression / on-arrival scour / on-arrival lash); the gentle↔malign menus; a Solace deck can hold + land a Deepening (evolving a banished base). **The mercy `Release` is fading-only** (the merciful cards release the dying, NOT the living — guarded so a "release fading" card can't silently board-clear healthy enemies); the aggressive `Effect::Banish` (You Were Never Really Here / I Too Can Create Desolation) is the only line that takes a living enemy, no impression. |
| Devolution (§5 — the rescue) | `tests/devolution.rs` (14), `redteam_rules_change.rs` (the arrival-completion pair) | **Recede** a standing-Faded form to a base in hand: full HP, fade cleared, summoning-sick, **an arrival symmetric with evolution** (fires `check_throughline` + a queued next-arrival buff, but engages no one and no OnPlay — so a base receding into a standing 3-line **re-completes on the spot**, +10/+10 + full heal, at parity with a Primal-evolve into a line), cost ⌊form.cost/2⌋ rounded down; window-only (healthy / uncontested-fade rejected); base-must-be-in-line; affordability; own-form-only; the full evolve↔devolve **cycle** (base→Primal→recede→Fabled→recede); **round-12 (devolve rescues the base from the Nightfall dissolve — it then stands and scores, not the banisher)**; determinism; redaction (the played base never leaks pre-reveal). |
| Lurk / Fabrications | `tests/lurk.rs`, `fabrication_traps.rs` | Hidden spirits & traps: reveal, forced-reveal-on-engage, springing. |
| Strays (PvE) | `tests/strays.rs` (8) | Telegraph, courtship, Feral interception, midnight, OnBefriend (Pigeon draws). |
| The Solace (PvE) | `tests/solace.rs` (6) | Faction-deck play, Unwriting, cadence, Lacuna, Page-Eater. |
| Rule exceptions | `tests/rule_exceptions.rs` | One card changing another's rules (dispatch). |
| 2v2 | `tests/twovtwo.rs` (8) | Slot turn order, per-slot projection, shared score, 6×6 Dusk. |
| Redaction | `tests/redaction.rs`, `render_contract.rs` | Clients see only their `PlayerView`; every renderable field present. |
| Determinism | `tests/determinism.rs` (7) | Same seed + commands → identical state & events. |
| Quickplay | `tests/quickplay.rs` (6) | Deck generation, style offers, picker preview. |
| **Gameplay fuzz / red-team** | `tests/suites/fuzz.rs` (`make fuzz`) | Random + biased playouts over the FULL canon catalog, all modes (1v1 / 1v1-vs-Solace / 2v2), full invariant suite + snapshot/restore + redaction after every command; `canon_rejected_commands_leave_no_trace`; `canon_replays_are_bit_identical`; `FUZZ_SECONDS` soak. |
| Security fuzz | `tests/suites/security.rs` | Hostile/garbage commands reject without panic or state change. |
| Model check | `app/crates/recollect-verify` | EXHAUSTIVE bounded state-space exploration (stateright). Covers 1v1, Solace PvE, 2v2, plus seeded-root frontiers for Mulligan, Glimpse, and **Devolution** (`solace_bridge.rs::devolution_is_reachable_in_the_frontier_…` — every invariant across the recede). |

## Rules → tests

| Rule / behaviour | Targeted test | Fuzzed |
|---|---|---|
| Determinism (seed → identical run) | `determinism.rs`, `fuzz.rs::canon_replays_are_bit_identical` | ✅ |
| Redaction (no hidden-info leak) | `redaction.rs`, server `session.rs` | — (asserted at boundary) |
| Arrival law: engage → interception → momentum | `rules.rs` | ✅ |
| Combat math (A vs D, banish threshold) | `rules.rs`, `keywords.rs` | ✅ (invariants) |
| Arcane pierces 20 defense | `keywords.rs::arcane_ignores_twenty_defense` | ✅ |
| Warded negates Arcane piercing | `keywords.rs::warded_negates_arcane_piercing` | ✅ |
| Resonance edge = +10 attack | `keywords.rs::resonance_edge_adds_ten_attack`, `rules.rs` | ✅ |
| Echo: 20% +20 at/below half HP, seeded | `keywords.rs::echo_eligibility…`, `rules.rs::echo_is_twenty_percent…` | ✅ |
| Momentum: 1 base link; Relentless chains | `rules.rs::momentum_base…`, `keywords.rs::relentless_chains…` | ✅ |
| Chain preference lists (§21) | `effects_engine.rs::d13_chain_preference…` | ✅ |
| Steadfast resists displacement | `keywords.rs::steadfast_cannot_be_pushed` (+ control) | ✅ |
| Mobile: a step is an arrival | `rules.rs::mobile_step_is_an_arrival` | ✅ |
| Lurk: hidden, forced-reveal-on-engage | `lurk.rs` | ✅ |
| Attune: shared adjacent resonance | `keywords.rs::attune_grants_shared_resonance…` | ✅ |
| Mourner: heal allies when a spirit dissolves | `keywords.rs::mourner_heals_all_allies…` | ✅ |
| Throughline (§4): a line of 3 shared Imprints → +10/+10 + restore | `keywords.rs::throughline_completes_a_line…` | ✅ |
| Projection (placement legality) | `rules.rs`, `twovtwo.rs` (per-slot) | ✅ |
| Overwrite (own-projection only, F-25) | `twovtwo.rs` | ✅ |
| The Dusk / contraction (5×5 & 6×6) | `rules.rs`, `twovtwo.rs` | ✅ (biased fuzz reaches it) |
| Held Ground (occupied rim lingers) | `rules.rs` | ✅ |
| Mulligan (§5, opening): once per seat, draw fresh + bottom one (seed-chosen); deterministic; opponent learns THAT, never WHAT | `mulligan.rs`, model-check (`model.rs` "a mulligan reshuffles cleanly and never leaks the hand"), server `session.rs` | ✅ (in the opening state space) |
| Glimpse (§5): BURN a chosen hand card (activation cost), then peek your top card and KEEP it (no Anima) or BOTTOM it for +1 Anima — net keep = −1 card, bottom = −2 for +1; not free Anima; gated to a non-empty hand AND page; deterministic per branch; opponent never sees the burnable hand, the burned card, the peeked card, or the option — counts/beats only (engine cmd `Command::Glimpse`) | `rules.rs` (`glimpse_*` — burn cost, keep/bottom, card conservation, empty-hand/page gates), `determinism.rs` (`glimpse_is_deterministic_per_branch`), `redaction.rs` (`opponent_hand_deck_and_peek_are_counts_only`), bot (`glimpse_burns_the_least_valuable_hand_card`, `glimpse_keeps_a_strong_body_and_bottoms_a_low_value_card`), model-check (`model.rs` "a glimpse burns then keeps-or-bottoms cleanly and never leaks a private card" + `solace_bridge.rs::glimpse_is_reachable_in_the_frontier`) | ✅ |
| Banish law (banisher's impression; Kindred leave none) | `rules.rs`, fuzz invariant | ✅ |
| Bonds (break on separation/push) | `spellbook.rs` | ✅ |
| Landmarks / Fabrications (terrain) | `spellbook.rs`, `fabrication_traps.rs` | ✅ |
| Fabrication traps (step-on & strike-from-range) | `fabrication_traps.rs` | ✅ |
| Evolution (play-from-hand: form card → base; Primal/Fabled, donors) | `evolve.rs` | ✅ |
| Callers summon Kindred | `summon.rs`, `card_effects_fire.rs` | ✅ |
| Restore = pure heal (never un-fades) | `effects_engine.rs::adjacent_allies_all_instant_heal…` | ✅ |
| Choice resolution (PendingChoice / doctrine) | `effects_choices.rs` | ✅ |
| Strays: telegraph/courtship/Feral/midnight | `strays.rs` | ✅ |
| On-event triggers fire (no declared-but-dead spec) | `effects_coverage.rs::every_authored_spec_trigger_is_fired` | — (ratchet) |
| OnUnwrite: an Unwritten mills a deck on dissolve (Footnote/Sentence Fragment) | `solace_effects.rs::{footnote_mills…,sentence_fragment_mills…}` | ✅ |
| OnBefriend: a befriended Foundling fires its effect (Pigeon draws) | `strays.rs::befriending_pigeon_fires_its_onbefriend…` | ✅ |
| the Solace: faction-deck play, cadence, Lacuna, Page-Eater | `solace.rs` | ✅ |
| Rule exceptions (carrier dispatch) | `rule_exceptions.rs` | ✅ (active in fuzz) |
| 2v2 slot order & shared score | `twovtwo.rs` | ✅ |
| Bad input never panics | `security.rs`, `fuzz.rs::canon_rejected_commands_leave_no_trace` | ✅ |
| Snapshot/restore is lossless; views always serialize | `fuzz.rs` (the `play` harness + `redaction_probe`), `determinism.rs` | ✅ |
| Difficulty agent determinism (Expert state-fork) | `agent.rs::expert_lookahead_does_not_corrupt…` | ✅ |
| Difficulty ladder is monotonic | `agent.rs::the_difficulty_ladder_is_monotonic`, `bin/calibrate.rs` | — (measured) |

## Invariants the fuzzer checks after EVERY command (all modes)
1. A tile never holds two occupants — no **spirit + terrain** (1), no **spirit + Stray**
   (1b), and no **terrain + Stray** (1c). A Stray occupies its tile (§6), so it is held
   against every other body, exactly like a spirit or terrain.
2. No terrain on a faded tile.
3. HP is bounded (≤ max; a standing spirit has positive HP).
4. Stats never underflow past a floor.
5. A Kindred never records a banisher (Kindred leave no impression).
6. Score (standing spirits) never exceeds the board.
7. A finished match's recorded score obeys the same bound.
8. The round never runs past `last_round + 1`.

The harness is verified to BITE (a deliberately-wrong invariant fails it), so a
green run means sound, not vacuous. `RT_SEEDS=N` (`make fuzz SEEDS=N`) cranks the seed
count for nightly, or `FUZZ_SECONDS=N` (`make soak`) caps wall-clock; the per-PR default
stays fast.

## Honest gaps (what is NOT deeply tested yet)
- **Choice-routed targets** (TargetSpirit, *Choose) are tested for a few cards
  in `effects_choices.rs`; not every choice card has a bespoke assertion (the
  fuzzer plays them, the execution probe skips them by design).
- These gaps are tracked here rather than hidden; closing them is incremental.

## Operational note (for the server / persistence layer)
`legal_commands` and combat both read the **catalog**, so a persisted match
MUST be restored with the same canonical catalog it was created with. Snapshot
carries the GameState + entropy position, not the catalog (which is large and
shared). The fuzz arm above verifies a same-catalog restore is byte-identical
and offers the identical legal set.

## Where the rules themselves are written (the law)
`docs/design.md` (mechanics) and `app/crates/recollect-core/data/cards.toml`
(every card's data; the card design prose is in `docs/cards_design.md`). This file
maps those to their guards; `docs/engine.md` maps them to the *code* that implements them.
