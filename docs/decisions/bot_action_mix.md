# Bot action mix — what the opponent actually *does*, per decision

**The bot's activity mix is human-sensible at every tier and faction: it builds
(Play), reads ahead (Glimpse, rising with skill and front-loaded), maneuvers
(Move), and stops cleanly (one EndTurn per turn) — and the evolve/devolve cycle
fires at a near-zero rate that is *correct* for curve-tuned generated decks, not
a bug.** This note is the per-decision companion to `docs/difficulty.md`'s
per-*match* win rates: difficulty answers *who wins*; this answers *how the bot
spends its turns*. It sharpens the balance lane's "~6–8% of matches use the
evolve/devolve cycle" finding into a full per-decision distribution.

## What is run

`recollect-bot/src/bin/action_mix.rs` tallies, across many seeded sim matches,
the **percentage of decisions** the bot devotes to each **activity** — the eight
the maintainer asked for (Play / Call / Evolve / Devolve / Glimpse / Move /
EndTurn / Mulligan) plus the rest of the command vocabulary (so the columns sum
to ~100% with nothing hidden) — broken out **per tier × faction × game phase**.
Every match plays through the same public seam a real client uses
(`Engine::new_with_rules` / `new_2v2_with_opener` + `legal_commands`/`apply`),
each seat piloted in its own faction by `choose_as` at the chosen tier. Each
command is classified **before** it is applied, so a `PlaySpirit` is read against
the live hand.

- **N = 200 matches/cell**; 4 tiers × {Lorekeeper-mirror, Lorekeeper-vs-Solace,
  Solace} for 1v1, plus the two 2v2 teams; tens of thousands of decisions per
  cell.
- **Reproducible.** Fixed seeds (`0..N`) + seeded RNGs, like the rest of the
  fleet — the tables below **re-derive bit-identically on a re-run** (verified:
  two fresh full runs are byte-identical). The counts are exact for this N, not
  Monte-Carlo estimates that wobble.

Reproduce:

```
cargo run -p recollect-bot --bin action_mix --release          # 1v1 + 2v2
cargo run -p recollect-bot --bin action_mix --release -- 1v1   # skip the (slower) 2v2 pass
```

### Two classification subtleties (both deliberate)

- **Play vs Call.** The engine has *no* `Call` command — a Call (summoning a
  Kindred) is a `PlaySpirit` of a **Caller**-kind card. So the probe reads the
  played card's kind: a Caller is a *Call*, any other body is a *Play* — exactly
  the distinction a player draws ("a body, or a companion?"). The Solace fields no
  Callers, so its Call rate is a clean **0.00%** at every tier — a built-in
  cross-check that the classifier reads the kind, not the command.
- **Glimpse is one activity, not three.** A Glimpse is a 3-command sequence
  (`Glimpse`, then two `Choose` steps: burn, then keep-or-bottom). The headline
  **Glimpse** counts only the *initiating* command; the two follow-ups (and any
  target-pick) are counted apart under **Choose**, so the headline mix isn't
  inflated by a single activity's bookkeeping. (`Choose` therefore tracks Glimpse
  closely, plus target picks from multi-target plays — it runs ~10–23% of
  decisions and *rises with skill*, because a stronger bot Glimpses more.)

**Phases** split on the Dusk (the round-8 contraction, the one real gameplay
boundary): **opening** = rounds 1–3, **mid** = rounds 4..=contraction, **post-Dusk**
= beyond it (the inner endgame, rounds 9–12 in 1v1).

## Results — 1v1 headline mix (% of all decisions)

Both-seat aggregate for the mirror; the acting seat for the PvE cells. EndTurn is
**exactly 12.00 per match** in every 1v1 cell (one per turn over the 12-round
clock) — a structural sanity check that the loop drives full matches.

| Tier | Faction (cell) | Play | Call | Evolve | Devolve | Glimpse | Move | EndTurn | Mulligan | Reclaim* |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Easy | LK (mirror) | 26.9 | 0.8 | 0.10 | 0.01 | 4.8 | 3.6 | 28.0 | 0.08 | 14.6 |
| Easy | LK (vs Solace) | 26.0 | 0.9 | 0.08 | 0.01 | 4.7 | 4.1 | 27.3 | 0.09 | 16.6 |
| Easy | Solace | 29.1 | 0.0 | 0.06 | 0.01 | 5.7 | 2.3 | 30.6 | 0.17 | 11.7 |
| Normal | LK (mirror) | 29.3 | 0.9 | 0.11 | 0.00 | 4.2 | 3.7 | 29.8 | 0.06 | 10.6 |
| Normal | LK (vs Solace) | 26.5 | 1.0 | 0.17 | 0.01 | 4.4 | 3.9 | 29.5 | 0.09 | 12.8 |
| Normal | Solace | 32.6 | 0.0 | 0.06 | 0.00 | 5.0 | 2.1 | 33.4 | 0.03 | 7.4 |
| Hard | LK (mirror) | 27.3 | 0.9 | 0.04 | 0.03 | 6.8 | 3.0 | 30.1 | 0.03 | 6.9 |
| Hard | LK (vs Solace) | 21.4 | 1.0 | 0.07 | 0.01 | 7.5 | 3.2 | 33.0 | 0.04 | 7.3 |
| Hard | Solace | 26.6 | 0.0 | 0.22 | 0.07 | 7.5 | 1.0 | 33.7 | 0.00 | 3.8 |
| Expert | LK (mirror) | 25.4 | 0.9 | 0.04 | 0.03 | 8.5 | 2.4 | 31.4 | 0.00 | 1.9 |
| Expert | LK (vs Solace) | 18.2 | 0.9 | 0.11 | 0.01 | 10.2 | 3.1 | 33.1 | 0.00 | 3.0 |
| Expert | Solace | 25.4 | 0.0 | 0.20 | 0.13 | 6.9 | 0.9 | 35.0 | 0.00 | 3.6 |

*Reclaim is not one of the headline eight, but it is the one non-headline command
worth tracking (the reclaim-churn the agent's stopping-floor exists to suppress) —
see "Flagged" below.

### 1v1 — Glimpse is front-loaded (mid-cell example: Expert LK vs Solace)

Glimpse share by phase, showing the foresight is spent early where it is worth
most, then tails off as the deck thins and the clock closes:

| Cell | opening (r1–3) | mid (r4–Dusk) | post-Dusk |
|---|---:|---:|---:|
| Expert · LK mirror | 14.3% | 7.2% | 1.8% |
| Expert · LK vs Solace | 16.1% | 5.6% | 6.7% |
| Hard · LK mirror | 11.1% | 6.3% | 2.4% |
| Easy · LK mirror | 4.6% | 4.6% | 5.4% |

(At Easy the temperature is so hot the front-loading washes out — a near-random
bot Glimpses about as often whenever it can; the *shape* sharpens with skill.)

## Results — 2v2 headline mix (% of all decisions)

Team aggregate; EndTurn is **exactly 10.00 per match** (the 10-round 2v2 clock).
Two differences from 1v1 stand out and both are sensible: **Move is markedly
higher** (the 6×6 has more board to maneuver on — LK teams move ~1.6–2.7/match vs
~1.2–1.5 in 1v1), and **EndTurn's share is lower** (more bodies in play ⇒ more
constructive actions per turn ⇒ EndTurn is a smaller slice).

| Tier | Team | Play | Call | Evolve | Devolve | Glimpse | Move | EndTurn | Reclaim* |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Easy | LK (A) | 30.4 | 1.2 | 0.13 | 0.00 | 3.7 | 5.3 | 21.9 | 18.2 |
| Easy | Solace (B) | 34.1 | 0.0 | 0.01 | 0.00 | 4.6 | 3.4 | 24.1 | 15.2 |
| Normal | LK (A) | 32.1 | 1.3 | 0.10 | 0.00 | 3.3 | 6.1 | 23.1 | 14.3 |
| Normal | Solace (B) | 37.1 | 0.0 | 0.04 | 0.00 | 4.2 | 3.2 | 25.5 | 11.2 |
| Hard | LK (A) | 30.8 | 1.2 | 0.05 | 0.02 | 5.5 | 3.9 | 24.6 | 11.2 |
| Hard | Solace (B) | 34.0 | 0.0 | 0.04 | 0.02 | 5.1 | 2.1 | 24.3 | 13.8 |
| Expert | LK (A) | 27.2 | 1.2 | 0.03 | 0.01 | 8.1 | 5.4 | 23.2 | 6.9 |
| Expert | Solace (B) | 36.5 | 0.0 | 0.08 | 0.03 | 3.9 | 1.6 | 25.7 | 12.3 |

## Reading it — is this how a thoughtful human plays?

Yes, on every axis that matters, with one cosmetic caveat (Reclaim at Easy).

- **Play is the spine; EndTurn is the metronome.** Play is the single largest
  *constructive* activity (~18–37%), and EndTurn is one-per-turn by construction.
  A human's turn is "develop the board, then pass" — the bot's is the same shape.
  The Solace plays *more* bodies than the Lorekeeper (it walls; its Unwritten both
  deny tiles and live to bank erasures) and *moves less* — exactly the attrition
  posture the design and the eval prescribe.

- **Glimpse rises with skill and is front-loaded — textbook.** Easy ~4–6% → Hard
  ~7–8% → Expert ~7–10%, and concentrated in the opening (up to ~16% of opening
  decisions, ~2% post-Dusk). A reading player spends foresight early — to shape
  the opening and dig for a curve — and stops late, when the deck is thin and
  there is nothing left to set up. The cold tiers Glimpse *more* because the burn
  cost is priced honestly (it skips a Glimpse when its worst card is too good to
  pitch), and a sharper bot finds more spots where the peek beats the card. A
  human would do exactly this. **Not over-glimpsing** (no tier burns through its
  hand peeking).

- **Move at a sane, board-shape-aware rate.** ~2–4% in 1v1, higher (~4–6% LK) in
  2v2 where the wider board rewards repositioning. The Lorekeeper moves more than
  the Solace (it relocates rim bodies inward to hold tiles to Nightfall; the
  Solace would rather plant a fresh wall). Both are the right instinct.

- **Call is rare but non-zero for the Lorekeeper, zero for the Solace.** ~0.8–1.3%
  Lorekeeper, 0.00% Solace. Callers are a small slice of a generated Lorekeeper
  deck (six Uncommon callers in the pool), and a Call costs 2 Anima for one
  Kindred at a time — a real but occasional play. The Solace has no Callers, so 0%
  is correct, not an omission.

- **Mulligan is near-zero and opening-only.** ~0.0–0.17%, and every Mulligan lands
  in the opening phase (it is legal only in round 1, once per seat). The greedy
  eval can't judge hand quality yet, so it holds its opener by default and only
  mulligans when nothing else scores — a known, documented limitation
  (`greedy_score_as`), not a misplay. A hand-quality-aware mulligan is future
  agent work; for now "rarely mulligans" is the honest, intended behaviour.

- **The evolve/devolve cycle fires at a near-zero rate — and that is CORRECT.**
  Evolve ~0.03–0.22%, Devolve ~0.00–0.13% of decisions (≈ 0.01–0.08 evolves and
  0.00–0.04 devolves *per match*). This is the per-decision face of the balance
  lane's "~6–8% of matches use the cycle" finding, and it is **expected, not a
  bug**: the Quick-Play decks these sims field are **curve-tuned, not
  evolution-density-tuned** (the player deck-builder's evolution-aware density is
  deferred work). A deck that holds one or two form/base pairs out of twenty cards
  *should* evolve rarely — the bot is correctly playing the decks it is dealt, not
  ignoring a mechanic. Two further signals it is **not** dead:
  - **Devolve is non-zero and *rises with skill*** — the Solace devolves most at
    Expert (0.13% / ~0.04 per match) and least at Easy (~0.00). The rescue is a
    deliberate, hard-to-find line, and only the sharp tiers find it. A bot that
    *never* devolved would be the red flag; this one devolves when the standing-
    Faded window actually opens.
  - **Evolve also rises in the right places** — the Solace (which has Primal
    Deepenings) evolves more than the Lorekeeper at the depth-2 tiers, and the rate
    ticks up post-Dusk where a banished base sits Faded under the window.

  So the low rate is a property of the *decks*, not a blindness in the *bot* — and
  the cross-check (it climbs with skill and with the window opening) confirms the
  eval prices the cycle. No eval change is warranted from these numbers; that
  matches `docs/difficulty.md`'s "hold the dials" verdict on the evolution economy.

## Flagged — the one thing that looks high, and why it isn't a defect

- **Reclaim at Easy (~12–18% of decisions; up to ~8/match in 2v2).** This is the
  highest non-Play/EndTurn bucket at the hot tier, and at a glance it reads as
  "the bot churns." It is the **reclaim-churn the agent's stopping-floor is built
  to bound**, and the data shows the floor working exactly as designed: Reclaim's
  share **falls monotonically as the bot sharpens** — 1v1 Lorekeeper-mirror Easy
  14.6% → Normal 10.6% → Hard 6.9% → **Expert 1.9%**. Crucially, the floor
  guarantees the bot **never reclaims a body it placed this turn** and never takes
  a *net-negative* reclaim (holding strictly dominates), so even Easy's churn is
  *legal, non-self-harming* play — a hot softmax sampling the least-bad standing-
  body cash, not "play a spirit then reclaim it." A new player facing Easy sees a
  bot that sometimes cashes a tile it could have held — gentle, slightly loose
  play, which is what Easy is *for*. At the tiers a strong player actually
  contests (Hard/Expert) it nearly vanishes. **No fix indicated; flagged for
  visibility.** (If a future change wanted Easy to also *look* tidy, the lever is
  the Reclaim floor's flat penalty in `greedy_score_as`, not the tier knobs — but
  that would make Easy play above its intended strength, so it is deliberately
  left alone.)

Nothing else trips a flag: **no tier ignores Calls** (the Lorekeeper always Calls
at its small deck-share rate; the Solace correctly never does), **no tier
over-glimpses** (Glimpse stays a single-digit-to-low-teens opening tool and tails
off), **no tier never-devolves** (the rate is low but present and skill-correlated),
and **EndTurn is the clock, not churn** (exactly one per turn — the bot is not
ending turns early to dodge a decision; when it ends, the stopping-floor confirms
nothing constructive remained).

## Where this sits

- Per-*match* outcomes and the tier ladder: `docs/difficulty.md`.
- The "~6–8% of matches use the evolve/devolve cycle" finding this refines:
  `docs/difficulty.md` (the evolution-economy re-tune note) and
  `docs/decisions/recollect_balance_decisive_retune.md`.
- The eval that produces this mix: `recollect-bot::agent` (`choose`/`choose_as`,
  the stopping-floor) and `recollect-bot::greedy_score_as`.

*Regenerate with the command above; the seeded probe re-derives identically, so
every figure here is checkable, not asserted. If an eval or knob change moves the
mix, update this note in the same change (the living-doc rule).*
