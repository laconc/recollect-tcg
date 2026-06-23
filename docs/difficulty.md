# Difficulty — what to expect, for a strong player

This is the difficulty map of Recollect for a player who already plays at a
**"Hard"-tier ability level** — a strong human who reads the board, lines up
exchanges, and holds inner ground to Nightfall. It answers, per **mode × faction ×
tier**, *how hard is this fight, really?* — grounded in the bot's own head-to-head
and PvE win-rate sims, with honest measured numbers (no invented figures), Wilson
95% confidence intervals where the sims sample, and the sample sizes.

It is a **player-facing companion** to the design and the bot plan, not a balance
spec. The balance law lives in `docs/decisions/bot_and_ml_plan.md` (the tier
ladder), `app/crates/recollect-bot/tests/suites/solace_winnability.rs` (the PvE
fairness gate), and the design doc's §9 (Modes & AI) / §11 (the Solace). If a
number here and a number there disagree, *those* are canon — this doc is the tour.

> A note on "Hard". The game ships **four AI tiers** — Easy / Normal / Hard /
> Expert (Normal is the default; `agent.rs`). "Hard" is also a useful label for a
> *human's* skill. When this doc says "you play at a Hard level", it means your
> decisions are about as sound as the **Hard bot's**: you read a reply ahead, line
> up exchanges, and rarely blunder — but you still slip. That gives a clean
> yardstick — *the bot tier whose play matches yours is the one you should roughly
> trade evenly with.* (Since the **Bal2 re-sweep** — a full four-tier re-derivation
> of the `(temperature, depth)` knobs — the ladder is monotone with clear, gentle
> steps: Easy 400/1, Normal 90/1, Hard 35/2, Expert 8/2. Hard is a genuine two-ply
> opponent — depth-2 like Expert, only hotter — so "Hard-level" means a *real*
> reading player, not a greedy-this-move one. This doc is the post-re-sweep map, and
> every figure below was re-measured against the live engine + catalog.)

---

## Where the numbers come from

Every figure below is produced by a binary in `recollect-bot`, replayed through
the **same** `Engine::legal_commands` + `apply` seam a real client uses — the bots
get no private state and no rule exemptions. To regenerate:

| Sim | Binary | What it measures | Cost |
|---|---|---|---|
| Tier ladder (1v1) | `cargo run --release -p recollect-bot --bin calibrate` | Each tier vs each other tier, head-to-head, Lorekeeper mirror | 200 matches / ordered pairing |
| Knob sweep | `cargo run --release -p recollect-bot --bin tier_sweep` | The `(temp, depth)` search behind the re-sweep: a 1-D strength `profile`, a candidate-tuple `ladder` matrix + monotonicity verdict, and a `pve` 1v1/2v2 probe | tunable (160–200 / point) |
| Brain floor | `cargo run --release -p recollect-bot --bin strength` | Easy & Expert vs pure-random | 600 games each |
| PvE faction (1v1) | `cargo run --release -p recollect-bot --bin char_sweep` | Player (Lorekeeper) win % vs each Solace disposition, and each Lorekeeper character vs the Solace, at the **Hard** and **Expert** mirrors | 200 matches / character (40 chars × 2 tiers) |
| PvE faction (2v2) | `cargo run --release -p recollect-bot --bin char_sweep_2v2` | The same, on the 6×6 board with two-a-side | 200 matches / character (40 chars × 2 tiers) |

**Reproducibility.** `calibrate`, `tier_sweep`, `char_sweep`, and `char_sweep_2v2`
iterate fixed seeds (`0..N`) with seeded RNGs, so every table below **re-derives
bit-identically on a re-run** — these are not Monte-Carlo estimates that wobble
between runs; they are exact for the given `N`. The intervals quoted are the
**sampling** uncertainty of that fixed-N estimate (Wilson 95%: roughly ±7pp at 50%
for N=200, ±2–4pp at the extremes), i.e. how far the true rate could sit from this
sample — *not* run-to-run noise, of which there is none. (`make sim` runs a separate
self-play fairness probe; the difficulty data is the binaries above.)

A reading convention that holds throughout: in the PvE sims **"player-win" is the
Lorekeeper seat's win rate** (seat A). A *high* player-win against a Solace
disposition means that disposition is, for you, a **pushover**; a *low* one means a
**wall**. In the tier ladder, a cell is **row tier's** win rate vs the column tier.

---

## 1v1 — the difficulty tiers

### 1v1 tier-vs-tier ladder (the AI you select)

`calibrate`, 200 matches per ordered pairing, A = row tier vs B = column tier, cell
= **A's win rate ± Wilson 95%**:

```
                  Easy        Normal          Hard         Expert
     Easy           —        34% ± 7       16% ± 5         10% ± 4
   Normal        69% ± 6        —          34% ± 7         16% ± 5
     Hard        78% ± 6     63% ± 7          —            33% ± 7
   Expert        89% ± 4     80% ± 6       59% ± 7            —
```

The ladder is **monotone *and* well-separated** — read any column top-to-bottom and
a stronger tier beats the column tier strictly more often than a weaker one does (vs
Hard: Easy 16 < Normal 34 < Expert 59; vs Expert: Easy 10 < Normal 16 < Hard 33).
The headline of the **Bal2 re-sweep** is the *adjacent spacing*: each tier beats the
one immediately below it by a clear, gently-narrowing margin — **Normal beats Easy
69%, Hard beats Normal 63%, Expert beats Hard 59%**. No rung is a coin-flip and none
is a cliff: it is a smooth descending gradient (69 → 63 → 59), exactly the shape a
four-step ladder wants (wide at the bottom where a new player needs daylight,
gentle at the top where two strong tiers should feel close). A regression test
(`tests/suites/agent.rs::the_difficulty_ladder_is_monotonic`) now pins the **full
adjacent ordering** (Normal > Easy, Hard > Normal, Expert > Hard, each > 55%) so a
future knob change can't silently flatten or invert a rung; the fine spacing is the
`calibrate` output above.

> **What Bal2 fixed (vs the Bal1 ladder).** Bal1 closed the old Hard→Expert *depth*
> cliff by putting Hard on depth-2 — but it overshot: it left **Normal↔Hard
> compressed** (Hard beat Normal only ~53%, a near coin-flip) and Hard↔Expert nearly
> as tight (~52%), so Normal and Hard were barely distinguishable. Bal2 re-derived
> all four knobs from scratch (the `tier_sweep` search): **Normal moved 60→90**
> (hotter, so it sits clearly below Hard) and **Expert moved 4→8** (a touch hotter —
> at depth-2 an *over*-cold agent turns passive and can lose to a hotter depth-2
> Hard, so 8 is the point where Expert reliably tops Hard). Hard stayed 35/2. The
> result: the 53%/52% adjacent rungs became **63%/59%** — real, monotone separation
> with no cliff.

**What each tier is.** One agent, scaled by two honest knobs — **softmax
temperature** (how often it picks a worse-than-best move; high = more mistakes) and
**search depth** (plies of lookahead) (`agent.rs`):

| Tier | temp / depth | Character |
|---|---|---|
| Easy | 400 / 1 | near-random; barely prefers a good move |
| Normal | **90 / 1** | mostly sensible, frequent sub-optimal line |
| Hard | **35 / 2** | reads a reply ahead like Expert, but hotter — a real opponent that still slips |
| Expert | **8 / 2** | almost always the best move; reads one reply ahead |

The **depth split is load-bearing**: Easy/Normal are depth-1 (greedy this move),
Hard/Expert depth-2 (they weigh the opponent's best reply). Lookahead is where the
Solace earns its keep (below), so the depth-2 pair are the tiers that make the
Solace a real fight. Within each depth, temperature does the separating — and the
re-sweep's four temperatures (400 / 90 / 35 / 8) are spaced to give the monotone
rungs above. (A subtlety the search surfaced: in the *Lorekeeper mirror*, a
very-cold depth-2 agent is not strictly stronger than a warm one — the one-ply
exposure penalty can over-weight caution — which is why Expert is 8, not 4, and why
all-depth-1 or single-depth-2 tier sets produced inverted top rungs. 35/2 over 8/2
is the pairing that orders cleanly.)

**Brain floor** (`strength`, 600 games vs pure-random): **Easy wins 88% ± 3%** —
the stopping floor (it never plays a self-harming move, even at temperature 400)
gives even the near-random tier real signal against pure chaos, so Easy is a
*gentle* first game, not a *coin-flip-with-random* one. **Expert wins 99% ± 1%** —
the heuristic is decisive. (Easy still loses to every shipped tier above it —
Normal 69%, Hard 78%, Expert 89% — so it remains the floor; it just isn't
self-destructive.)

### What to expect at each tier (you play at a Hard level)

- **vs Easy** — you should win **~78%+**. The Hard bot beats Easy 78% ± 6, and you
  *are* Hard-level — a strong human reads cleaner than the hot Hard bot, so call it
  comfortably in the high 70s to 80s. Easy never plays a self-harming move (the
  stopping floor) but barely prefers a good one; use it to learn a deck. Losses are
  variance (a cold opening hand) or your own slip, not the opponent outplaying you.
- **vs Normal** — you should win **~63%+**. Hard beats Normal 63% ± 7 — the re-sweep
  re-opened this step (it was a compressed ~53% under Bal1), so Normal is now clearly
  the *easier* mid-tier: it makes more sub-optimal moves than Hard (temperature 90 vs
  35) but never a self-harming one (the floor). As a *human* reading a reply ahead
  you should sit well above 50% — real games, but you should win the majority.
- **vs Hard** — **a true 50/50 mirror, and a genuine test.** Head-to-head the two
  Hard seats split 50/50 by construction; the small first-mover texture (below) is
  the only systematic tilt. Hard *reads a reply ahead* — it won't hang a spirit into
  your answer and it punishes yours — so this is a real two-ply contest, not a greedy
  one. **This is the tier that fairly tests a strong player.** Expect close games
  decided by the opening coin-flip and a mid-game exchange or two.
- **vs Expert — the hardest tier, a real but gentle step up from Hard.** Expert beats
  Hard **59% ± 7**, so a Hard-level human is a *modest* underdog (~41%). Expert and
  Hard are the **same kind of player** (both depth-2); Expert just runs colder (temp
  8 vs 35), so it slips less. To gain on it you out-plan it across turns (it sees only
  *one* reply deep — multi-turn plans are where a human wins). Expect a fight you can
  win but should expect to lose slightly more than half the time.

> **No difficulty wall, no compression.** Under Bal1 the ladder had a *flat middle*
> (Normal≈Hard) after the old top cliff was removed. Bal2's re-derived temperatures
> spread the four tiers evenly: the steps are 69 / 63 / 59 — each a clear majority,
> none a wall. The 1v1 mirror is now a smooth ladder from "my kid can win" (Easy) to
> "I have to try" (Expert) with a fair, true-50/50 test in the middle (Hard).

---

## 1v1 — the factions

In a vs-AI game you choose the **opponent's faction**: other **Lorekeepers** (a
mirror — both sides bank Score from standing spirits + impressions) or the
**Solace** (the asymmetric antagonist — its Unwritten leave no mark; it scores an
off-board **erasure tally** instead, +1 per player spirit it banishes or impression
it unwrites). `--faction` on the CLI; the dropdown on the web picker. **The Solace
is the default opponent.**

These are **not symmetric fights.** The combat re-stat made spirits decisive (~3
hits to fall) and the board the win condition — which **structurally favors the
board-*scoring* Lorekeeper** over the Solace's off-board tally at equal skill. But
the depth-2 tiers' lookahead is exactly where the Solace's denial game earns its
keep, so at Hard/Expert the fight lands close to even. The PvE sims measure exactly
how much.

### Mirror (vs Lorekeeper) — symmetric

A Lorekeeper-vs-Lorekeeper game *is* the tier ladder above (that's what `calibrate`
plays). It's symmetric by construction: same scoring, same card pool shape, only
the deck-bias (character/style) differs. So "vs a Lorekeeper opponent" reduces to
"vs that tier" — read the tier section. The only asymmetry is the **opening
coin-flip** (below).

### vs the Solace — *a near-even fight at both tiers; the hardest faction in 1v1*

`char_sweep`, 1v1, **player = Lorekeeper, opponent = Solace**, 200 matches per
character. Player-win % (high = the Solace is a pushover for you):

| Mirror tier | Player-win vs Solace (roster mean) | Sample |
|---|---|---|
| **Hard** | **46.5%** (Solace roster); 45.6% the Lorekeeper roster's own win | 200 / char, 40 chars |
| **Expert** | **51.2%** (Solace roster); 48.6% the Lorekeeper roster's own win | 200 / char, 40 chars |

**The Solace is a real, near-even fight at both tiers — and a touch *easier* at
Expert.** At the Hard mirror a Hard-level player wins **~46%** against the Solace
roster: a hair under even, and a bit *below* the same player's ~50% against a Hard
Lorekeeper, so the Solace is the **harder faction** — but not a wall. At Expert it
ticks up to **~51%** (a coin-flip): both mirrors are depth-2, and Expert's colder,
cleaner play leaves the player slightly more room than Hard's hotter aggression,
which over-presses into trades the player can punish. Either way this sits
comfortably inside the `solace_winnability` fair band (**0.25–0.86** player-win) —
near its centre, not its edges. **The Solace is the hardest single-faction fight in
1v1 (~46–51% player-win), a clear-but-fair underdog spot, not a brutal one.**

> Earlier editions of this doc recorded a much harsher Hard Solace (~34% player-win);
> that figure was **stale** — re-measuring `char_sweep` against the live engine and
> catalog gives **46.5%** at the unchanged Hard knob (35/2). The re-sweep didn't
> *make* the Solace easier at Hard (Hard's knob is unchanged); it surfaced that the
> Solace was never as brutal as the old number implied. The living-doc rule applies:
> the table above is the re-derived truth.

#### The Solace is not one opponent — dispositions matter

The 20 Solace characters split into five **dispositions**, and they are *not*
equally hard. `char_sweep`, 200/char, player-win by disposition at **both** mirror
tiers:

| Disposition | Player-win @ **Hard** | Player-win @ **Expert** | Read |
|---|---|---|---|
| **Sorrow** | **57.1%** | **64.6%** | softest at both; a tilt-to-player |
| **Erasure** | **51.9%** | **58.1%** | soft; ~even at Hard, tilt-to-player at Expert |
| The Long Forgetting | 46.9% | 50.1% | ~even at both |
| Cruelty | 43.9% | 45.8% | a slight wall — denial |
| **Relentless** | **32.6%** | **37.1%** | the **wall**, hardest at both tiers |

Two readings here. (1) **The spread is wide and centred near even** — at Hard it runs
from Relentless (~33%, a real wall) up to Sorrow (~57%, a tilt to you), a ~25pp swing,
but the band now *straddles* 50% rather than sitting below it: the soft dispositions
(Sorrow/Erasure) tilt to the player, the middle (Long Forgetting/Cruelty) is roughly
even, and only Relentless is a clear loss. (2) **Expert nudges the whole band up,
not down.** Every disposition is a few points softer at Expert than at Hard
(Relentless 33→37, the traders to ~58–65) — the player's cleaner Expert play and
Expert's colder, less-trade-happy Solace both help you. So the **soft/hard ordering
holds at both tiers** (Sorrow/Erasure easiest, Relentless hardest); Expert mostly
*shifts* the band a touch toward the player. There is no pushover Solace and no
unwinnable one at either tier.

> **The evolution/devolution economy re-tune — measured, and the verdict is "hold the
> dials".** This whole band was *re-measured across the full evolution cycle* — the
> standing-Faded window, **devolution** + the base⇄form cycle, the **12 Solace
> Deepenings** (Primal-only, gentle↔malign), and the Fade-after-Main / instant-Dusk
> turn — to judge whether any of that machinery pushes a disposition out of range. It
> does **not**. Every 1v1 disposition sits inside the `solace_winnability` gate
> (0.25–0.86) at both tiers, and the figures above *reproduce the post-Bal2
> measurement bit-for-bit* — i.e. the deep evolution surface (already in the catalog
> when Bal2 was taken) is **absorbed by the bands, not band-breaking**. Two reasons it
> stays absorbed: (a) the bot eval already values the cycle faction-aware — `Evolve`
> by the form's stat budget (+ Fabled fuel + arrival strike), `Devolve` by the tile it
> rescues, `Glimpse` burn-aware with evolution **forms never burned** — so neither
> side mis-prices it; (b) on the Quick-Play *generated* decks these sims field, the
> cycle is **spice, not spine** — it fires in only ~6–8% of matches (a probe over 200
> seeds/disposition saw ~5–12 evolves and ~1–4 devolves per 200, both seats), because
> generated decks are curve-tuned but not evolution-density-tuned (the player
> deck-builder's *evolution-aware density* is deferred work). A mechanic that fires
> ~7% of the time has near-zero leverage on a roster mean, so **no economy/eval dial
> change is warranted from these numbers** — re-tuning an in-band surface would be
> unmotivated meddling. The two evolution balance flags are **kept as deliberate
> tension** (maintainer's call), not nerfed: **The Gnawing Unending** (95/5/55,
> Arcane+Relentless) as a stat-swing outlier, and the **evolve↔devolve rescue cost**
> that makes a key body hard to remove permanently (bounded by Anima + summoning
> sickness). The durable, flagged variance a player actually feels is the
> *per-disposition deck lean* below — which is independent of the evolution cycle.

> **Faction-imbalance flag (1v1) — the Solace is the harder faction, by a little.**
> The Solace runs ~4pp *harder* than the mirror at Hard (player-win ~46% vs ~50%
> mirror) and ~even at Expert (~51%). Within it the disposition swing is the durable
> part: Sorrow/Erasure (~52–57% at Hard) are ~20–25pp softer than Relentless (~33%).
> This is **known and gated** (`solace_winnability` 0.25–0.86 — now sitting near
> band-centre; `char_sweep`'s ±12pp flag catches the per-character outliers). The
> **per-disposition swing is deferred balance work** — it persists at every tier and
> is not addressed by a `(temp, depth)` re-sweep (it is a deck/eval question). Takeaway
> for a strong player: **the Solace is the harder faction, but only slightly — pick the
> mirror for a dead-even fight, the Solace to be a modest underdog; and brace for
> Relentless, the one true wall.**

---

## 2v2 — tiers, factions, and the team dynamics

2v2 is the **co-op default**: two humans (slots A1 + A2, both Lorekeepers) against
the Solace's pair (B1 + B2), on the wider **6×6** board, same 12-round clock, with
**shared per-team score and Throughlines** and **cross-team Bonds**. The shell is
the full TeamView HUD, not board-only (design §5).

### 2v2 PvE — what to expect (you + a partner, both Hard-level)

`char_sweep_2v2`, **team A = two Lorekeepers vs team B = two Solace**, 200 matches
per character, player(team-A)-win %:

| Mirror tier | Team-A (player) win, overall | Solace roster | Lorekeeper roster | Band | Sample |
|---|---|---|---|---|---|
| **Hard** | **75.9%** | 76.0% (range 56.0–90.0) | 75.8% (range 69.5–80.5) | **+15.9pp ABOVE** (50–60) | 200 / char, 40 chars |
| **Expert** | **73.8%** | 73.4% (range 53.5–88.5) | 74.2% (range 66.0–81.0) | **+33.8pp ABOVE** (30–40) | 200 / char, 40 chars |

**2v2 is too easy for the player at both tiers — a structural gap the knobs can't
close.** On the wide 6×6 with four hands, two depth-2 Solace coordinating only
through a *shared eval* cannot wall the board the way a single depth-2 Solace walls
the 5×5, and the Lorekeeper team's structural board-scoring edge reasserts. So a
Hard-level pair beats a Hard Solace pair **~76%** and an Expert pair **~74%** —
Expert is barely harder than Hard (73.8% vs 75.9%), and **both sit well above their
reporting bands** (Hard target 50–60%, Expert 30–40%). The per-disposition spread
carries over and is large: **Relentless** Solace pairs are the wall (~54–63%
player-win — your worst games), **Sorrow/Erasure** pairs the softest (~83–89%),
Cruelty/Long Forgetting in between (~60–80%).

This is **the standing 2v2 imbalance** (imbalances list, item 3), and it is a
**structural gap, not a knob choice** — re-confirmed by this pass and now stated as
policy: **no `(temp, depth)` dial closes it.** The proof is three-fold. (1) *It
predates the re-sweep*: 2v2-Hard runs the unchanged Hard knob (35/2), so its ~76%
was there before Bal2 touched anything. (2) *The knob grid can't reach the band*:
across `tier_sweep pve` **no** `(temp, depth)` for the Solace pair pulls 2v2
player-win into 50–60% (Hard) or 30–40% (Expert) — and a *colder* Expert pair is, if
anything, marginally **easier**, because an over-cold depth-2 agent turns passive and
walls *less*, the opposite of what the band needs. (3) *The shared eval is the
ceiling.* The two Solace bots are **two independent depth-2 searches over one shared
heuristic, with no joint plan** — each picks the move that maximizes the *team* eval
*given the board it sees*, but neither reasons about *what its partner will do this
round*. On the 5×5 that is enough: one body's wall is the whole denial, and a single
depth-2 Solace holds the inner board. On the **6×6 with four hands** it is not — a
real wall needs the two bodies to *divide the board between them* (one seals the left
inner lane while the other seals the right, neither doubling up, both timing their
Deepenings so a contested tile is always covered). A shared *scalar* eval cannot
express "you take that lane, I'll take this one"; the two searches greedily contest
the same high-value tiles and leave the flanks open, so the Lorekeeper team's
structural board-scoring edge (its on-board Score out-paces the Solace's off-board
erasure tally at equal skill — the same edge that makes the Solace the harder 1v1
faction) reasserts on the wider board. That is **why the lookahead the Solace earns
its keep with in 1v1 doesn't translate to 2v2**, and why Expert buys the pair almost
nothing over Hard.

**The fix is coordination, and coordination is policy-net work — see
`docs/decisions/bot_and_ml_plan.md`.** A bigger held-ground/erasure weight, a
2v2-specific re-band of the targets, or any other *knob* leaves the root cause (two
un-coordinated searches) untouched — at best it shifts the mean a few points while
the flanks stay open. Genuine team play needs a model that conditions each Solace
seat's move on its *partner's* projected play — i.e. the **policy+value net** the bot
plan already scopes (a learned policy trained on 2v2 self-play *sees the whole team
state* and can learn lane-division and Deepening-timing that a shared scalar can't
encode), or, short of ML, an explicit **joint-plan layer** (a cheap 2v2-only
coordinator that assigns the two seats complementary objectives before each searches).
Both are **deferred roadmap balance work, gated on the policy-net milestone**; neither
is a `(temp, depth)` re-sweep deliverable. Until then 2v2-PvE is honestly logged as
*above band at both tiers* — a fair, winnable-leaning co-op fight, not the test the
1v1 Solace is.

> Earlier editions recorded 2v2 as ~54% at both tiers (Hard "in band", Expert "in
> band"); those figures were **stale**. Re-running `char_sweep_2v2` against the live
> engine + catalog gives ~74–76% — the team game is meaningfully *easier* for the
> player than the old numbers implied. The table above is the re-derived truth.

### The 2v2-specific dynamics

- **The team opener (the coin-flip, four ways).** First word is a **seeded
  coin-flip across all four seats**; the `A1→B1→A2→B2` cycle rotates to begin at
  the chosen opener (still team-alternating). A bot character's **initiative** trait
  *weights* its own seats' odds — an edge, not a guarantee. **What this means for
  you:** which side gets the opening tempo (and which gets the Listener's
  last-action-each-round edge) is decided at genesis and announced as a match-start
  beat. Going-second compensation is a **held lever** — the sims fold the opener
  into the win rates above (each seed played once), so the ~74% already *includes*
  whatever first-mover tilt exists; it is not measured as catastrophic, and no
  handicap is applied. Expect the opener to swing individual close games, not the
  match average.
- **Player-one's first placement is home-rows-restricted** (§4) — in 2v2 the opener
  seat plants into its home two rows on turn one; the wider 6×6 makes that opening
  position matter less than in 1v1's 5×5, but it is the one structural edge the
  opener carries beyond tempo.
- **Shared score + Throughlines, cross-team Bonds.** Your two bodies bank into one
  team total, and Throughlines/Bonds can span the table. Coordination — not raw
  per-seat play — is the lever a human team has that the bot pair coordinates only
  through its shared eval. This is *why* the bot Solace pair under-performs its 1v1
  self: two depth-2 bots sharing one eval don't wall the wide 6×6 the way a single
  Solace walls the 5×5, which is the whole reason **2v2 is above band at both tiers**
  (the lookahead the Solace earns its keep with in 1v1 doesn't translate to the team
  game). A coordinating human pair widens that gap further.
- **Placement may use either teammate's projection; Overwrite requires your own**
  (R-25 / F-30 — "you may not Overwrite what your partner tells"). Practically:
  lean on your partner's footprint to *place* into contested space, but you must
  bring your *own* reach to Overwrite an enemy body.
- **Longer games, wider board.** 6×6 + four hands = more bodies, more turns; the
  inner contraction after the Dusk still funnels the endgame to the inner board, but
  there's more material in play to hold it with. Matches run longer than 1v1. The
  wider board cuts *both* ways: the Solace has more room to field a wall, but it also
  has more board to cover with only a shared eval to coordinate two bodies — and on
  net the latter wins, which is why the depth-2 Solace pair doesn't dominate the way
  a single depth-2 Solace does in 1v1.

---

## The whole picture — a "what to expect" grid (you play at a Hard level)

Win expectations for a **Hard-level human** (or human pair). 1v1 tier cells are
your expected win rate *as* a Hard player (from the `calibrate` row for Hard, and
the Expert-vs-Hard cell read as your underdog odds); faction cells are the measured
PvE player-win. The headline shifts this re-sweep: the mirror ladder is **monotone
with clear, gentle steps** (Hard is the true 50/50 test, Expert a modest step up);
the **Solace is a near-even fight** in 1v1 (~46–51%, the harder faction but not
brutal); the **evolution/devolution economy measures in band across the whole cycle**
(no re-tune; the deep surface is absorbed, not band-breaking); and **2v2 remains too
easy** at both tiers (a structural coordination gap, policy-net-gated — flagged).

| Mode | Opponent | Tier | You should expect | Source |
|---|---|---|---|---|
| **1v1** | Lorekeeper (mirror) | Easy | **~78%+ win** — comfortable | calibrate (Hard vs Easy 78±6) |
| 1v1 | Lorekeeper (mirror) | Normal | **~63%+ win** — real games, you win most | calibrate (Hard vs Normal 63±7) |
| 1v1 | Lorekeeper (mirror) | Hard | **~50% — your true mirror, the fair test** | calibrate (Hard mirror, by construction) |
| 1v1 | Lorekeeper (mirror) | Expert | **~41% — a modest underdog** | calibrate (Hard vs Expert: Expert wins 59±7) |
| 1v1 | **Solace** | Hard | **~46% win** — near-even; the harder faction | char_sweep Hard (46.5%) |
| 1v1 | **Solace** | Expert | **~51% win** — a coin-flip; slightly easier than Hard | char_sweep Expert (51.2%) |
| **2v2** | Solace pair | Hard | **~76% win** — *too easy* (above band) | char_sweep_2v2 Hard (75.9%) |
| 2v2 | Solace pair | Expert | **~74% win** — *not harder than Hard* (above band) | char_sweep_2v2 Expert (73.8%) |

Per-disposition, at Hard, **vs the Solace**: Sorrow/Erasure ≈ softest (~52–57% 1v1,
~83–90% 2v2); Relentless ≈ the wall (~33% 1v1, ~56–62% 2v2); Cruelty / Long
Forgetting in between (~44–47% 1v1, ~60–80% 2v2). The 1v1 band straddles 50% (the
Solace is a fair underdog spot for you); the 2v2 band sits high (the player team is
favoured against every disposition).

---

## Imbalances and cliffs the data reveals — flagged

1. **The 1v1 mirror ladder is monotone, gradual, and well-separated (Bal2 fixed the
   Bal1 overshoot).** Bal1 closed the old Hard→Expert *depth* cliff but compressed
   the middle — Hard beat Normal only ~53% and Hard↔Expert ~52%, so Normal/Hard were
   nearly indistinguishable. Bal2 re-derived all four knobs (Easy 400/1, Normal 90/1,
   Hard 35/2, Expert 8/2): the adjacent rungs are now **69 / 63 / 59** — every step a
   clear majority, none a coin-flip, none a wall. **This was the re-sweep's primary
   goal and it is met**; the `the_difficulty_ladder_is_monotonic` gate pins the full
   ordering so it can't regress.

2. **Faction imbalance (1v1): the Solace is the harder faction, but only slightly.**
   A Hard-level player beats a Hard Lorekeeper ~50% but a Hard **Solace ~46%** — a
   ~4pp tilt, near-even — and an Expert Solace ~51% (a coin-flip). This is well inside
   the `solace_winnability` band (0.25–0.86) and near its centre, *not* the ~34% wall
   an earlier (stale) edition recorded. **Known and gated.** The Solace is a fair
   underdog spot, not a brutal one.

3. **2v2 is too easy for the player at both tiers — a STRUCTURAL gap no knob closes
   (policy-net work).** The `char_sweep_2v2` targets are 50–60% player-win at Hard,
   30–40% at Expert. Measured: Hard **75.9%** (+15.9pp above band), Expert **73.8%**
   (+33.8pp above band). Root cause: the two Solace bots are **two independent depth-2
   searches over one shared *scalar* eval, with no joint plan** — fine to wall the 5×5
   (one body is the whole denial), but on the **6×6 with four hands** a real wall needs
   the bodies to *divide the board* (complementary lanes, staggered Deepenings), which
   a shared scalar cannot express; the two searches contest the same tiles and leave
   the flanks open, so the Lorekeeper team's structural board-scoring edge reasserts.
   This is **not a knob choice**: 2v2-Hard uses the unchanged Hard knob (the gap
   predates Bal2), and `tier_sweep pve` shows **no** `(temp, depth)` pulls 2v2 into
   band (a colder Expert pair is marginally *easier* — over-cold ⇒ passive ⇒ walls
   less). The lever is **team coordination**, which is **policy-net-gated**: a learned
   policy trained on 2v2 self-play (or a cheap 2v2-only joint-plan coordinator) can
   condition each seat's move on its partner's projected play the way a shared scalar
   can't. **Deferred roadmap balance work, gated on the policy-net milestone — see
   `docs/decisions/bot_and_ml_plan.md` ("2v2 coordination"); out of scope for a
   `(temp, depth)` re-sweep.**

4. **Disposition spread inside the Solace — persistent, and deferred balance work.**
   The soft/hard ordering by character is stable across tiers and modes: **Sorrow/
   Erasure** softest (1v1 ~52–57% Hard / ~58–65% Expert; 2v2 ~83–90%), **Relentless**
   the wall (1v1 ~33% / ~37%; 2v2 ~56–62%), Cruelty/Long Forgetting between — a
   ~20–25pp swing in 1v1 by which character you draw. `char_sweep`'s ±12pp-from-mean
   flag catches the outliers; because it is *relative* to the (tier-dependent) roster
   mean, exactly which characters trip it shifts a little by tier — at **Hard** the
   four Relentless (Ona Verrin, Brother Tace, Hesper Lund, Galen Roe) flag LOW and
   Sorrow's Old Damaris flags HIGH; at **Expert** the band lifts, so only Brother Tace
   + Hesper Lund stay flagged-LOW (Ona Verrin/Galen Roe rise to ~40–41%) and Sorrow's
   Katherine + Old Damaris flag HIGH. **This per-disposition swing is a deck/eval
   question, not a `(temp, depth)` one — it is NOT addressed by this re-sweep** and
   remains a real variance a player will feel. All inside the gate (not a regression),
   but flagged as outstanding.

5. **The evolution/devolution economy is IN BAND — measured across the full cycle, no
   re-tune made.** The standing-Faded window, **devolution** + the base⇄form cycle, the
   **12 Solace Deepenings**, and the Fade-after-Main / instant-Dusk turn were
   re-measured per-disposition (`char_sweep`) and in 2v2 (`char_sweep_2v2`) to judge
   whether the deep evolution surface pushes any band out of range. **It does not** —
   the 1v1 bands reproduce the post-Bal2 figures bit-for-bit and every disposition sits
   inside the `solace_winnability` gate, so the cycle is **absorbed, not band-breaking**.
   Two reasons: the bot eval prices the cycle faction-aware (`Evolve`/`Devolve`/`Glimpse`,
   forms never burned), and on the generated PvE decks the cycle is **spice, not spine**
   — it fires in only ~6–8% of matches (a probe saw ~5–12 evolves + ~1–4 devolves per
   200 seeds/disposition), so it has near-zero leverage on a roster mean. **No
   economy/eval dial was changed** (re-tuning an in-band surface would be unmotivated).
   The two evolution balance flags are **kept as deliberate tension** (maintainer's
   call): **The Gnawing Unending** (95/5/55, Arcane+Relentless) and the
   **evolve↔devolve rescue cost**.

---

*Regenerate every figure here with the binaries in the table above; the seeded sims
re-derive identically, so this doc's numbers are checkable, not asserted. If a
re-stat or heuristic change moves them, update this doc in the same change (the
living-doc rule).*
