# Decisive play — the evidence, and the tripwires that guard it

**The game is decisive, and the eval already minimises the mirror draw rate.** Every
"press harder" lever makes the symmetric-mirror draw rate WORSE, not better — so the
shipping eval is left at the draw-rate floor and a regression guard locks the
decisiveness in. This is the evidence behind that conclusion.

## TL;DR

- The eval is **decisive**: the std-deck Expert mirror draws **3.2%** at n=500
  (**0.0%** at n=100), with **zero** 0-0 games. Quick-Play mirrors draw 5–9%. The PvE
  tiers are in band.
- The **held-ground** machinery (`tile_hold_value`, `SOLACE_PRESENCE_NUM`, the depth-2
  `positional_objective`) is the counterweight that makes the bot BUILD and HOLD a
  board instead of churning it away under the depth-2 exposure penalty. **A/B: turning
  those presence terms off roughly doubles the draw rate** (3.2% → 6.0%) — a degenerate,
  passive direction.
- The bots are **not hoarding**: in the std-deck mirror only **1.1% of EndTurns are
  voluntary** (an affordable card still in hand). **98.9% of passes are forced** — the
  20-card test deck simply empties. The high mean Anima at end-of-turn (~16) is "ran
  out of cards to spend it on," not "refused to spend."
- Every lever that presses HARDER in the symmetric mirror **raises** draws (both sides
  build bigger symmetric walls that tie more): see the A/B table below. The baseline
  coefficients sit at the draw-rate floor.
- **The guard:** `bot_v_bot_{1v1,2v2}_is_decisive_*` and
  `quickplay_1v1_mirror_is_decisive` in `fleet_tripwires.rs` bound the draw rate so a
  future eval change can't silently slide into a passive, draw-heavy regime.

## Diagnosis — why a symmetric mirror can stall, and why this eval doesn't

Scoring is **one point per tile** to whatever stands on it at Nightfall, plus the
Solace's off-board erasure tally (`engine/flow.rs::finish`). On the 1v1 board there are
**9 inner tiles**; each side naturally plants on ~half. A draw is equal tile control at
the close.

The original 0-0 came from the bot's depth-2 lookahead **over-fearing exchanges** (the
`exposure` term, `-0.35 · opp_best`) over-fearing exchanges with nothing to
counterbalance it: every contest reads as risk, so the bot churns its own board away
rather than hold, and both sides drift to an empty, tied close. The counterweight is
the **held-ground** family — a body planted inner + late is worth progressively more
(`tile_hold_value`'s late-round ramp), folded into the depth-2 `positional_objective`
net of the opponent's, so the bot is rewarded for BUILDING and KEEPING a board. That
is what keeps the mirror decisive.

### Measured action mix (std-deck Expert mirror, n=500)

| metric | value | reading |
|---|---|---|
| draws | **3.2%** (0-0: **0.0%**) | decisive |
| margin ≤ 1 pt | 34% | thin wins are common (9-tile board), but they DECIDE |
| plays | 37.0% (30% of them engage) | the bot presses the board |
| moves | 0.0% | `MoveSpirit` needs the **Mobile** keyword; the std deck has none — correct, not passive |
| studies (Glimpse) | 9.5% | situational, as a burn-cost Glimpse should be |
| EndTurns | 35.2% | **98.9% forced** (empty/unaffordable hand), 1.1% voluntary |

The bot spends every affordable card and only passes when the small deck is dry — no
passivity.

## A/B — the levers, and why the eval is left unchanged

All rows: the std-deck Expert self-play mirror, n=400–500, fixed seeds. "Baseline" is
the shipping eval.

| eval | draws | mean score A / B | note |
|---|---|---|---|
| **Baseline (shipping)** | **3.0–3.2%** | 4.5 / 4.7 | at the draw-rate floor |
| held-ground OFF (`NO_HOLD` proxy) | 6.0% | 4.0 / 4.8 | the pre-fix direction — ~2× draws |
| Lorekeeper depth-2 `pos 1.5 / exp 0.15` | 10.2% | 5.8 / 6.1 | more presence ⇒ bigger symmetric walls ⇒ MORE ties |
| Lorekeeper depth-2 `pos 2.0 / exp 0.10` | 7.5% | 6.3 / 6.5 | same |
| Lorekeeper depth-2 `pos 3.0 / exp 0.35` | 8.2% | 5.7 / 5.8 | same |
| denial bonus (scale banish by enemy tile hold) | 6.4% | 4.0 / 4.5 | both sides trade down the board ⇒ more empty tiles ⇒ MORE ties |

**Reading it.** In a *symmetric mirror* any symmetric "be more aggressive" incentive is
applied to BOTH seats, so it does not break the symmetry that causes draws — it amplifies
it. Up-weighting presence builds taller equal walls; a denial bonus trades the board down
to a sparser equal close. Both raise the draw rate. The shipping coefficients already
minimise it. (This matches the design's §S-6 lab note: a true double-wall stall reaches a
31% draw *mass* — far above anything here.)

The Glimpse-payoff lever ("if +1 Anima is too weak, bots never Glimpse and starve") is a
**non-issue**: the bots already Glimpse ~9% of turns and the 1.1% voluntary-pass rate
proves they are not card- or Anima-starved by choice. Raising the Glimpse payoff would
dig the same finite deck faster, not add decisiveness. No core/design change is warranted.

## The decisiveness guards

**`app/crates/recollect-bot/tests/suites/fleet_tripwires.rs`** — three decisiveness
guards bound the draw rate:

- `bot_v_bot_1v1_is_decisive_not_a_stalemate` — std-deck Expert mirror (`selfplay`),
  **draws ≤ 15%** over n=200 (measured ~3%).
- `bot_v_bot_2v2_is_decisive_not_a_stalemate` — std-deck 2v2 mirror (`selfplay_2v2`),
  **draws ≤ 20%** over n=200 (measured ~9%; the wider 6×6 board + 10-round clock draw a
  touch more).
- `quickplay_1v1_mirror_is_decisive` — the generated-deck 1v1 mirror at R12 via the same
  evidence playout the fairness anchor uses, **draws ≤ 18%** (measured ~5%).

Bounds sit far above the measured rates (no flake) yet far below a real stall (a
passive-both-sides regression spikes to 30%+), so each is a true alarm.
`two_v_two_first_team_edge_stays_in_a_fair_band` bounds 2v2 draws at <50%; these tighten
and extend it to 1v1 and the std-deck path.

## The numbers the guards lock in

**Draw rate (decisiveness):**

| path | n | draws |
|---|---|---|
| std 1v1 Expert mirror (flagged) | 100 / 500 | **0.0% / 3.2%** |
| std 2v2 Expert mirror | 60 / 200 | 10.0% / 9.0% |
| Quick-Play 1v1 mirror cr12 | 300 | 5.0% |
| Quick-Play 2v2 mirror cr10 | 200 | 9.0% |

**PvE tiers + fairness anchor (must not regress — unchanged):**

| metric | measured | target band | source |
|---|---|---|---|
| 1v1 fair anchor (A win share, cr12) | 50.3% | 30–70% (fair ~50%) | `one_v_one_is_a_fair_anchor` |
| PvE Hard mirror, player-win | ~71–73% | wide gate 25–86% (reports 50–60) | `char_sweep` / `the_solace_pve_fight_is_winnable_and_fair` |
| PvE Expert mirror, player-win | ~37–38% | 30–40% | `char_sweep` |
| difficulty ladder (Expert vs Easy) | > 62% | monotone | `the_difficulty_ladder_is_monotonic` |

## Reproduce

```
cargo run -p recollect-bot --bin fleet            # 1v1/2v2 draw + fairness, n=300
cargo run -p recollect-bot --bin char_sweep       # per-character PvE tiers (Hard + Expert)
cargo run -p recollect-bot --bin recollect-bot 500 # std-deck mirror A/B/draw summary
cargo test  -p recollect-bot --test main fleet_tripwires
```

## Decision

**No balance/eval intervention.** The game is decisive and the tiers hold; the shipping
eval already minimises the mirror draw rate, and the candidate levers move it the wrong
way. The **decisiveness regression guard** keeps it that way. Re-run the fleet and
`char_sweep` (and update this file) after any change that moves combat math, the
income/Glimpse economy, the held-ground eval terms, or the Quick-Play deck shapes —
those are the inputs decisiveness depends on.
