# 2v2 fairness — the formal evidence run (D-17)

**2v2 is fair within the tripwires; no balance intervention is warranted.** This
is the fairness evidence and the tripwires that guard it.

## What is run

`recollect-bot/src/bin/fleet.rs` — the simulation evidence fleet that replaces
paper playtesting. Every match plays through `Engine::legal_commands` + `apply`
like any client; the bots get no private state and no rule exemptions. Greedy
bots, **n = 300 matches per cell**, fairness reported as raw win rates with a
**Wilson 95% interval** (half-width ±5.6pp at n=300) and a first-mover edge.

Reproduce: `cargo run -p recollect-bot --bin fleet`.

## Results

| cell | A | B | draw | A interval | first-mover edge |
|---|---|---|---|---|---|
| 1v1 · clock 9 | 43.0% | 44.7% | 12.3% | ±5.6pp | −7.0pp |
| 1v1 · clock 12 | 44.3% | 48.7% | 7.0% | ±5.6pp | −5.7pp |
| 2v2 · clock 9 | 44.3% | 48.0% | 7.7% | ±5.6pp | −5.7pp |
| 2v2 · clock 10 | 46.0% | 47.3% | 6.7% | ±5.6pp | −4.0pp |

**Reading it.** In every cell the gap between the seats/teams sits *inside* the
±5.6pp Wilson half-width — i.e. not distinguishable from a coin flip at this
sample size. The first-mover edge is mildly negative (the second teller does a
touch better) and shrinks as the clock lengthens, but never breaches the
interval. Both teams win in 2v2 across seeds (the standing CI tripwire,
`greedy_2v2_matches_terminate_and_are_not_degenerate`, already guards against a
degenerate seat; this run quantifies it).

### Supporting census (same run)

- **Quick Play deck texture** — spirit-heavy across all five styles (65–72%
  spirits, the rest split rituals/bonds/terrain). All within the expected band.
- **Evolution reality** — Primal avg power 92 (19/30 carry a real effect),
  Fabled avg power 90 (24/30). In live play both forms are reached *and* chosen
  (Primal 62% / Fabled 38% of 112 evolutions): the choice is live, neither form
  dominates.
- **Evolver draft share** — evolvers are 26% of the spirit pool and 26% of
  drafted spirits: non-evolvers hold their own. No reprice indicated.

## The tripwires

A cell **fails** and gates a balance change if any of:

1. a seat/team win-rate edge **exceeds its Wilson interval** at the run's n
   (a gap > ~5.6pp that holds as n grows);
2. draws climb out of the single-digit/low-teens band (a sign the clock starves
   decisive play);
3. 2v2 collapses to a single-team sweep across seeds (the CI degeneracy guard).

The −4…−7pp second-mover lean is logged as a thing to *watch* — if a larger
nightly soak (`RT_SEEDS`/`FUZZ_SECONDS`/a higher `N`) tightens the interval below it, revisit
the opening-tempo levers (first-placement, opening anima). It is **not** actionable
at n=300.

## Decision

No 2v2 balance intervention. The clock pair (9/10 for 2v2, 9/12 for 1v1) stays.
Re-run this fleet (and update this file) after any change that moves combat math,
the Dusk/contraction, or the Quick Play deck shapes — those are the inputs a
fairness figure depends on.
