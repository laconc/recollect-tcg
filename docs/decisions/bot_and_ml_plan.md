# Bot & ML plan — the opponent, its difficulties, and the road to a learned agent

This documents what the AI opponent is today, how its difficulty ladder works
and is verified, and the honest path to a machine-learned agent that could run
on a phone. It answers the questions: how many difficulties, how are they
denoted, can simulations verify them, and can we make an ML agent.

## What exists today (shipped, tested)

One agent, in `recollect-bot::agent`, exposed as `choose(engine, seat,
difficulty, rng) -> Command`. It plays through the same `legal_commands` +
`apply` as any client — **no private state, no rule exemptions, only the public
forecast** every player can see. Two honest knobs scale it:

- **temperature** — a softmax over the greedy move scores. High temperature
  picks worse moves more often (mistakes); low temperature almost always picks
  the best. Temperatures are scaled to the score range (~±120), not arbitrary.
- **depth** — plies of lookahead. Depth 1 scores only this move; depth 2 (Keeper)
  applies the move on a forked state and subtracts the opponent's best reply, so
  it avoids handing the opponent a strong answer.

The brain underneath is the `greedy_score` heuristic: it values banishing an
enemy, advancing off the rim, cost-efficiency, evolution form value, and arrival
strikes, using the real `forecast_exchange` combat math. It also weighs the **authored effect** of the card it plays
(Damage, Draw, Anima, stat buffs, bounce/release denial), read from the same
effect IR the engine runs — so a ritual or an effect-bearing spirit is valued by
what it does, not just its body. It reads **both sides
of the board** — it forecasts the exchange against the specific enemy it would
engage, including that enemy's defense, resonance edge, Arcane/Warded, and HP —
so it does make choices informed by the card effects in play. What it does NOT
yet do: model multi-turn plans, opponent hidden information, or long-range
positional value beyond one ply.

## How many difficulties, and how they're denoted

**Four tiers, denoted by named characters** (not a 1–10 number), matching the
design's ladder:

| Tier | Label | Feel | temperature / depth |
|---|---|---|---|
| 1 | **Easy** | a gentle first game; plays nearly at random | 400 / 1 |
| 2 | **Normal** (default) | frequent sub-optimal lines; a balanced match | 90 / 1 |
| 3 | **Hard** | a real opponent; reads a reply ahead, but slips | 35 / 2 |
| 4 | **Expert** | plays to win; looks a move ahead | 8 / 2 |

Hard joins Expert at **depth-2** (differing only by a hotter temperature). A
depth-1 Hard made the Hard→Expert step a cliff — lookahead is where the Solace
earns its keep — so the depth split is **Easy/Normal depth-1, Hard/Expert depth-2**.
These four `(temperature, depth)` points are the **Bal2 re-sweep**: a from-scratch
re-derivation (via `bin/tier_sweep`'s knob search) chosen to give a monotone,
well-separated ladder. Bal1 had overshot — it left Normal↔Hard compressed (~53%) and
Hard↔Expert nearly as tight (~52%). Bal2 moved **Normal 60→90** (hotter, clearly
below Hard) and **Expert 4→8** (a touch hotter — at depth-2 an *over*-cold agent
turns passive and can lose to a hotter depth-2 Hard, so 8 is where Expert reliably
tops Hard); Hard stayed 35/2. The matrix below is the re-measure.

The common labels (Easy/Normal/Hard/Expert) are what players expect; **Normal is
the default**. Four is the right count for launch: enough range from "my kid can
win" to "I have to try," without the false precision of ten levels players can't
feel apart. A single trained net (below) serves all four via temperature.

## Can simulations verify the difficulties are accurate? YES — and they did.

`bin/calibrate.rs` plays every tier head-to-head (200 matches per ordered
pairing) and reports win rates with 95% Wilson intervals. The verified ladder
after the **Bal2 re-sweep** (A = row tier, B = column tier; cell = A's win rate, ±~6pp):

```
                  Easy     Normal       Hard      Expert
     Easy           —         34%         16%         10%
   Normal          69%         —          34%         16%
     Hard          78%        63%          —          33%
   Expert          89%        80%         59%          —
```

This is a **monotone, well-separated ladder**: each tier beats every weaker tier
above 50%, and the adjacent rungs are clear and gentle — **Normal beats Easy 69%,
Hard beats Normal 63%, Expert beats Hard 59%** (a smooth descending 69 → 63 → 59,
no coin-flip rung, no cliff). A separate probe (`bin/strength.rs`) confirms the brain
has signal at all: the agent beats pure-random handily (Easy 88%, Expert 99%). A fast
regression test (`tests/suites/agent.rs::the_difficulty_ladder_is_monotonic`) asserts
the **full adjacent ordering** (Normal>Easy, Hard>Normal, Expert>Hard, each >55%) so a
future knob change can't silently flatten or invert a rung.

**Top-tier step:** Expert beats Hard 59% — a real but *gradual* step now that Hard
also looks a ply ahead (it differs by temperature, not depth). The effect-aware
heuristic — valuing what a card's effect *does*, not just its body — sharpens the
score landscape, so the colder Expert (temp 8) pulls ahead of the mistake-prone Hard
(temp 35). A subtlety the `tier_sweep` search surfaced: in the Lorekeeper mirror a
*very*-cold depth-2 agent is not strictly stronger than a warm one (the one-ply
exposure penalty over-weights caution), which is why Expert is **8, not 4** — at 4 it
turned passive and the top rung inverted. ML below is the path to genuine multi-turn
*planning*, not tier separation (classical search already delivers that, and deeper
search gets costly on mobile).

### Calibration catches what the fuzzer misses
The heuristic-driven calibration matches reach states the random fuzzer rarely
sets up. A worked example: a `PeekDeck` effect (Glimpse family) that opened an
interactive Peek choice for a player on the *opponent's* turn (via a
Parting/OnAnyBanish trigger) left a dangling cross-turn choice `legal_commands`
couldn't satisfy — a soft-lock the fast-ending fuzzer missed but the longer
calibration games hit immediately. The rule: a Peek only opens for the active
player (the async law forbids opponent-turn windows).

## Can we make an ML-powered agent? Yes — and the architecture is ready.

The `choose(engine, seat, difficulty, rng)` signature is the seam. A learned
policy swaps in behind it without touching the engine or any caller. The plan:

1. **Generate data via self-play.** We already have deterministic, headless
   self-play at scale (the fleet runs hundreds of matches in seconds). Run the
   strongest classical agent (or an early net) against itself for millions of
   games, recording (state features → chosen move → eventual result).

2. **Features, not pixels.** The input is a compact feature vector derived from
   the public state: per-tile occupancy/owner/stats, stains, projection, each
   side's hand size and anima, round/contraction, the legal-move mask. A few
   hundred floats — tiny.

3. **A small policy+value net.** A model on the order of 100k–1M parameters
   (a few fully-connected or small conv layers over the 5×5/6×6 grid). That is
   **small enough to run on a phone** in well under a frame — comparable to the
   nets in mobile chess/Go apps. Policy head ranks legal moves; value head
   estimates win probability (useful for a smarter lookahead than greedy).

4. **Train by policy iteration.** Start from the heuristic as a teacher (behavior
   cloning), then improve with self-play + a search (even shallow MCTS guided by
   the value head). This is the AlphaZero recipe at miniature scale; the game's
   small branching factor and short games (≤12 rounds) make it very tractable.

5. **Difficulty from one model.** A single trained net gives the *top* tier.
   Lower tiers come free from the same knobs: raise the softmax temperature over
   the policy logits (more mistakes) and/or reduce search. So ML deepens the top
   of the ladder without needing four separate models — exactly the
   "as smart as the difficulty requires" property you asked for.

6. **On-device.** Export to a mobile-friendly runtime (ONNX / a tiny hand-rolled
   inference loop in Rust via the UniFFI core, so the same model runs on web and
   mobile). No server round-trip; the opponent runs locally even offline.

**Can it make complex choices across both sides' cards and plan ahead?** The
classical agent already reasons over both sides for the immediate exchange; the
value-net + shallow search is what adds genuine *planning* (it learns that a
move is good because of where it leads, not just its immediate score) and lets
it weigh the card effects available to both players. The net can also learn to
reason under hidden information (it sees hand *sizes*, not contents) the way a
human estimates threats.

### The 2v2 coordination gap — the concrete balance case the policy net is gated on

2v2-PvE is **above its difficulty band at both tiers** — a Hard-level player *pair*
beats a Hard Solace pair ~76%, an Expert pair ~74% (`char_sweep_2v2`; the targets are
50–60% / 30–40%, see `docs/difficulty.md`). This is **not a knob-tuning gap** — it is
the single clearest motivating case for the learned policy, so it is recorded here as
policy.

**Why the knobs can't fix it.** The Solace pair is **two independent depth-2 searches
over one shared *scalar* heuristic, with no joint plan**. Each B-seat picks the move
that maximizes the *team* eval given the board it sees; neither reasons about *what
its partner will play this round*. On the 5×5 that suffices — a single depth-2 Solace
walls the inner board, and lookahead is exactly where the Solace earns its keep, so
the 1v1 Solace is a fair, near-even fight (~46% Hard / ~51% Expert player-win). On the
**6×6 with four hands** it does not: a real wall needs the two bodies to *divide the
board* — one seals the left inner lane, the other the right, neither doubling up, both
timing their Deepenings so a contested tile is always covered. A shared **scalar** eval
cannot express "you take that lane, I'll take this one"; the two greedy searches
contest the same high-value tiles and leave the flanks open, so the Lorekeeper team's
structural board-scoring edge (on-board Score out-paces the off-board erasure tally at
equal skill) reasserts on the wider board. Evidence it is structural, not a dial:
2v2-Hard runs the unchanged Hard knob (the gap predates the Bal2 re-sweep), and across
the `tier_sweep pve` knob grid **no** `(temp, depth)` for the pair pulls 2v2 into band
— a *colder* Expert pair is, if anything, marginally **easier** (over-cold ⇒ passive ⇒
walls less). So Expert buys the pair almost nothing over Hard.

**Why the policy net is the fix.** A learned **policy+value net trained on 2v2
self-play sees the whole team state** (both B-seats' bodies, both projections, the
shared score) and can learn the lane-division and Deepening-timing that a shared scalar
can't encode — genuine coordination, conditioned on the partner's likely play, falls
out of training rather than being hand-weighted. The same net naturally handles the
6×6's larger grid (the feature plane already scopes 5×5/6×6). **Short of full ML**, a
cheaper interim is an explicit **joint-plan layer**: a 2v2-only coordinator that, once
per round, assigns the two Solace seats complementary objectives (lanes / targets)
*before* each runs its existing depth-2 search — a small classical bridge that buys
most of the coordination without a trained model. Either path is **balance work gated
on this milestone**, not a `(temp, depth)` re-sweep deliverable; until it lands, 2v2
is honestly logged as a fair, winnable-leaning co-op fight that is *easier* than the
1v1 Solace, and the `solace_winnability` gate guards 1v1 fairness in the meantime.

### Effort & sequencing (honest)
- Shipped now: the 4-tier classical ladder. Good enough to launch.
- M-class (post-launch): the self-play data pipeline + behavior-cloned net to
  match the classical top tier, then policy-iteration to exceed it. This is real
  work — weeks, not days — but every prerequisite (deterministic engine,
  headless self-play, feature access, a mobile core) already exists.
- **2v2 coordination** (above): the net's first concrete *balance* payoff — a
  team-aware policy (or the interim joint-plan coordinator) to pull 2v2-PvE into
  band. Gated on the data-pipeline + net being in place; no `(temp, depth)` reaches
  it. Tracked in `docs/roadmap.md` (the 2v2 imbalance) and `docs/difficulty.md`.

The classical agent is the floor we ship on; the ML agent is the ceiling we
climb to behind the same interface, with simulations calibrating both.
