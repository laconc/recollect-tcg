# Playing Recollect in your terminal

A screen-by-screen guide to the **`recollect`** command-line client — the
interactive text UI and its controls, plus the headless JSON mode for scripts
and bots. If you've never sat down at a match, read
[How to play](how_to_play.md) first for the lay of the land; this is the
terminal-specific companion, written for the person at the keyboard.

> A note on words. Spirits are **banished** from a match, never "killed." Only
> the Solace **unwrites**, and only the Solace's register speaks of "forgetting."
> The screens below use this language; so does this guide. The Memory's final
> rounds are the **Dusk**, then **Nightfall**.

The CLI is one binary across two choices — *transport* (a local in-process game,
or `--server` to play the authoritative server over the network) and *interface*
(an interactive text UI, the default, or a headless JSON mode). All of them embed
the same `recollect-core` rules engine, so a move plays out identically wherever
you sit, and you only ever see **your** seat's view — never the opponent's hand
or deck order.

---

## Starting up

From the workspace:

```
cargo run -p recollect-cli            # local game vs the AI (you are Seat A)
cargo run -p recollect-cli -- hotseat # two humans sharing one terminal
cargo run -p recollect-cli -- watch   # spectate two AIs
```

Once built, the binary is simply `recollect`; the rest of this guide uses that
name. The modes:

| Command | What it does |
|---|---|
| `recollect` (or `recollect play`) | Local interactive game vs the AI — you are Seat A |
| `recollect hotseat` | Local interactive game, two humans on one terminal |
| `recollect watch` | Local interactive game, spectate two AIs |
| `recollect online new` | Networked: create a match, claim Seat A, print Seat B's invite token |
| `recollect online join <MATCH_ID> <TOKEN>` | Networked: join an existing match as Seat B |
| `recollect autoplay` | Headless: the AI plays one seeded match and prints the result |
| `recollect headless` | Headless JSON protocol: a program drives Seat A, the AI plays Seat B |

Useful global flags: `--seed N` (replay an exact match — the seed is printed at
the start of every game), `--difficulty <easy|normal|hard|expert>`,
`--faction <solace|lorekeeper>` (which faction the AI opponent fields — see
*Screen 2½* below; the Solace is the default), `--server <URL>` (for `online`),
`--json` (machine output for `online`), `--ndjson` (stream every event for
`autoplay`).

---

## Two interactive modes — the cursor TUI, and the line REPL

The client has **two** interactive faces, chosen automatically:

- **The cursor TUI** (a real terminal, local 1v1 vs the AI). A full-screen
  `ratatui` board with a **gold cursor** you steer with the **arrow keys** — the
  terminal twin of the web client's gold-cursor interaction. **Enter/Space** picks
  up the piece (or hand card) under the cursor and places it; **Esc** cancels;
  **Tab** moves between the board and your hand. The numbered **legal-move list**
  stays on screen the whole time (it is the complete move set; the cursor is the
  fast path). This is what you get from `recollect` (or `recollect play`) at a
  normal prompt — see [The cursor TUI](#the-cursor-tui-local-1v1-in-a-real-terminal)
  below.
- **The line REPL** (everything else). `hotseat` (two humans), `watch` (two AIs),
  **online**, **2v2**, and any **non-interactive** run — a pipe, `--json`,
  `headless`, or CI, where stdout isn't a terminal — use the original **line-based**
  UI: the client **prints** the screen and **waits for you to type a line** and
  press Enter. Nothing happens on a keystroke alone. The walkthrough of the line
  screens (**welcome → deck pick → who you face → the board → Nightfall**) is
  below; it is also exactly what a piped run emits, byte-for-byte.

Both faces drive the **same** rules engine and the **same** shared verb grammar, so
a move plays out identically and you only ever see **your** seat's view.

---

## The cursor TUI (local 1v1, in a real terminal)

Run `recollect` (or `recollect play`) at a normal terminal and you drop into the
cursor board after the deck pick. The screen is split: the **interactive board** and
your **hand** on the left, the rich **match** render (stats · projections · score ·
Anima) and the **legal-move list** on the right, with a **status line** under the
board.

```
┌ Board — Seat A (playing hand 1) ───────────┐┌ Match ───────────────────────────────┐
│5                                           ││══ Round 1/12 · Seat A to act ··· ══   │
│4                                           ││ … the rich seat's-eye render …        │
│3         [  ]                              │└───────────────────────────────────────┘
│2 <  ><  ><  ><  ><  >                      │┌ Legal plays (number / : verb …) ─────┐
│1 <  ><  ><  ><  ><  >                      ││  0. Glimpse (burn a hand card …)      │
│   a  b  c  d  e                            ││  3. Play Frenzy Kit → a1              │
└────────────────────────────────────────────┘└───────────────────────────────────────┘
```

For the real thing — the **gold cursor**, the gold targets, and the brass-gold theme in
colour — see the **[cursor-TUI image gallery](gallery/tui/README.md#the-image-gallery--real-terminal-screenshots--a-clip)**:
real terminal screenshots of the board at rest, a pickup, the inspect overlay, the Glimpse
and Mulligan prompts, and Nightfall, plus a short clip of the cursor moving and a
pickup → place. (The matching deterministic `.txt` stills live there too.)

- The **`[  ]`** brackets are the **cursor**. When you pick a piece up, its legal
  **targets** light in **gold** (`<  >`); a **green** `(  )` quietly marks any tile
  that has an available action (the terminal echo of the web's "quiet dot").
- The board uses single-width tags so columns stay aligned: `A `/`B ` a seat's
  healthy spirit, `a~`/`b~` a fading one, `..`/`,,` an impression, `##` dark (faded)
  ground, `Lm`/`Fb`/`??` terrain.

### Controls (cursor TUI)

| Key | What it does |
|---|---|
| **← ↑ ↓ →** | Move the cursor (board); ↑/↓ move the selection in the hand. Clamps at the edges — never wraps. |
| **Enter** / **Space** | **Pick up** the spirit/card under the cursor, then (after aiming) **place** it on a gold target — resolving to the legal **Move / Play / Evolve / Devolve / Overwrite**. |
| **Esc** | **Cancel** a pick-up (or close an open overlay). |
| **Tab** | Toggle focus between the **board** and your **hand**. |
| **i** | **Inspect** — a passive overlay with the full card (stats · keywords · rules · the reach grid) for whatever the cursor points at. |
| **:** | Open the **verb mini-buffer** — type a verb (`m c3 c4`, `p 0 c1`, …) and Enter. The same shared grammar the line mode uses. |
| a **number** | Pick that entry from the numbered **Legal plays** list, then Enter (the list is always available, like the web's accessible button list). |
| **?** | Show the key legend. **q** quits (the seed resumes the match). |

Picking a piece highlights its **targets** (what you can act on *now*); **inspect**
(`i`) shows a card's **reach** (what it *could* threaten) — the same two reads the web
canvas draws. **Glimpse** (the `g` move) and the opening **Mulligan** appear as
selectable choice blocks ("burn a card to Glimpse"; "Keep / Bottom for +1 Anima").
The AI opponent's turn replays into the status line; then control returns to you. When
the clock runs out, the closing **Nightfall** screen prints on the normal terminal,
exactly as the line mode below.

---

## The line REPL (hotseat · watch · online · piped)

There is no full-screen mode here. The client **prints** the current screen — the
board, your moves — and then **waits for you to type a line** and press Enter. You
read, then type a number (or a short command). Nothing happens on a keystroke alone;
you're always in control of when the screen advances. (This is also exactly what a
**piped / non-TTY** run of any mode emits — local 1v1 included.)

The walkthrough below follows the screens you'll see in order:
**welcome → deck pick → who you face → the board → Nightfall**.

---

## Screen 1 — Welcome

The very first line prints the match seed. Keep it: `--seed N` replays this exact
game, and `q` at any prompt pauses you out, with the seed waiting to resume it.

```
RECOLLECT — quick play · seed 7042 (keep it to replay this exact match)
```

---

## Screen 2 — Choose your match (the deck pick)

You're offered **three** deck styles. Each prints its name and a one-line voice
("All teeth. Arrive loudly, chain hard…"), then the **objective selection-info** —
the deck's resonance lean · aggression · tempo · body-vs-spell mix (measured in the
engine over many deck-gen seeds), followed by how many spirits and spellbook cards it
runs, its cost curve, and the cards it tends to open with — so you can choose
informedly, not just on flavour. (The web picker shows the same selection-info as chips.)

```
Seat A — the Memory offers three plays:
  1. Embertide          All teeth. Arrive loudly, chain hard…
     Fury · Aggressive · even tempo · balanced bodies
     14 spirits · 6 spellbook · cost curve 1:3 2:4 3:5 4:2 5:1
     opens: Cinderling, Tide-Caller, Hush, Emberkin, Vow of Ash
  2. The Long Watch      Hold ground, blunt every arrival…
     Resolve · Defensive · grindy tempo · spirit-heavy
     16 spirits · 4 spellbook · cost curve 1:5 2:5 3:4 4:1 5:1
     opens: Wardstone, Hush, Steadfast Oak, …
  3. …
choose a style [1-3] >
```

Type **1**, **2**, or **3** and press Enter to begin a Quick Play match against
the AI. (In `hotseat` each human picks in turn.)

---

## Screen 2½ — Who you face

Against the AI, you're told **who you're up against** before the first turn — a
**named character** with their own lore, exactly as the online game names your
opponent. The character fields a real faction-pure deck (the same one the server's
matchmaking would build for that seed), so a `--seed` always faces the same
opponent with the same cards.

```
You face Mara Quint, a tempter of the Solace.
  She thinks a half-erased page the cruelest page of all, and finishes what the fading starts.
```

By default the AI is **the Solace** — the fading-Memory antagonist, who tempts you
to forget. Pass `--faction lorekeeper` to face a **Lorekeeper** keeper instead (a
Skirmish-style mirror). Either way the opponent is one of twenty named characters
per faction, picked by the seed; an aggressive character may also win the opening
coin-flip and take the first word. (In `hotseat`, with two humans, there's no AI
to name; in `watch` the spectated Seat B is named.)

---

## Screen 3 — The board, your turn

This is where the match lives. Every turn the client reprints the whole screen:
a banner, the 5×5 page, your spirits with full stats, the running score, your
Anima and hand, and then the numbered list of everything you may legally do.

```
══ Round 3/12 · Seat A to act · your turn ══
5 [        ][        ][        ][        ][        ]
4 [        ][        ][   ::   ][        ][        ]
3 [        ][   ::   ][ACind  3][   ..   ][        ]
2 [        ][        ][        ][        ][        ]
1 [        ][        ][        ][        ][        ]
   a         b         c         d         e      (:: yours · .. theirs · ~ fading · ^ can evolve · ! Echo · ° held/lamplit · ⌂ landmark · ▒ fabrication · ░ dusk)
 ▸c3 A Cinderling             A2   D1   HP   3/3   Adjacent Fire ⇢ can step
(▸ = action available · ⇢ = a Mobile spirit can still step · ⊘ = rested · use the numbered plays below)
Score if Nightfall struck now: A 1 — B 0
You: 4 anima (your play budget — no fixed action count; End Turn when ready) · deck 14 · hand:
   0. Tide-Caller            3c  2/ 2/ 4 Adjacent Water
   1. Hush                   1c  0/ 0/ 1 Self     Calm
Them: 4 anima · hand 5 · deck 14

Legal plays:
    0. Play Tide-Caller → b3
    1. Step c3 c3→c4
    2. Glimpse (burn a hand card, then peek your top card)
    3. End turn
  (type a number to act · 'i N' inspect hand card N · 'i <tile>' e.g. 'i c3' inspect a board card)
your move >
```

Picking **Glimpse** (§5) re-prompts twice: first pick which **hand
card to burn** (the activation cost — it leaves play), then **keep** the card you peek
on top (no Anima, net −1 card) or **bottom** it for **+1 Anima** (net −2 cards). It's
omitted from the menu when your hand or deck is empty.

### Reading the board

- **Columns** are lettered **a–e**, **rows** numbered **1–5** (so `c3` is the
  centre). Each occupied cell shows the owner (`A`/`B`), a four-letter card
  abbreviation, and its current HP.
- The **legend** after the grid decodes the markers: `::` a tile *you* threaten,
  `..` a tile *they* threaten, `~` a fading spirit, `^` one that can **evolve**,
  `!` a live Echo, `°` a held (lamplit) spirit out on the darkened rim, `⌂` a
  landmark, `▒` a fabrication, `░` the **Dusk**.
- Below the grid, each spirit is listed with full stats. A green **`▸`** marks a
  spirit you have an action available on. For your **Mobile** spirits a movement
  cue follows: **`⇢ can step`** if it can still take its one free Move this turn,
  **`⊘ rested`** if it has spent that move or just arrived (a freshly-arrived
  spirit has **summoning sickness** and can't step until next turn). Opponent and
  Steadfast spirits carry no cue.
- **Score if Nightfall struck now** is the live tally — Seat A's board points and
  Seat B's. The Solace (Seat B) scores its board presence **plus** its off-board
  **erasure tally** (every banish or Unwriting it lands). The Unwritten leave no
  mark on the page, so when that tally is non-zero the line shows the split, e.g.
  `B 3 (board 1 + 2 erased)`. At Nightfall the tally folds into B's total.
- **You: N anima** is your **play budget** — the real limiter on what you can do
  this turn. There is **no fixed action count**; the turn runs until you End it,
  and you can play as far as your Anima reaches. Then your hand, with each card's
  cost / attack / defense / HP / reach / resonance, and the opponent's public
  counts.

### Acting

The numbered **Legal plays** menu *is* your move set — playing a spirit,
stepping, evolving, overwriting, reclaiming, glimpsing, making a choice an effect
offers, and ending your turn are all just entries, each spelled out in a readable
label (often with a combat forecast, e.g.
`Play Cinderling → c3 ⚔ d3 [deal 2 · take 1]`). The simplest path is to **pick a
number** from the menu — but you can also type a **verb shortcut** (see *Optional
verb shortcuts* below), the same grammar both human modes share.

| You type | What happens |
|---|---|
| a number, e.g. `1` | Take that move from the numbered **Legal plays** menu |
| a **verb**, e.g. `m c3 c4` | Act directly — the faster path (see *Optional verb shortcuts* below) |
| `i N`, e.g. `i 0` | Inspect hand card **N** — stats, keywords, rules text, and a reach grid (★ the card, ● each tile it threatens) |
| `i <tile>`, e.g. `i c3` | Inspect the card on board tile **c3** |
| `q` | Quit / pause (the seed printed at the top resumes the exact match) |

When in doubt, **read the menu** — it is the complete, legal set of what you can
do right now.

---

## Screen 4 — Nightfall

When the twelve-round clock runs out, the final board prints once more under a
**Nightfall** banner with the result.

```
— NIGHTFALL — Score 7–5 · the match belongs to Seat A
```

A draw reads *the Memory keeps both names*. Watch for the round-8 banner along
the way:

```
  ░░░ DUSK FALLS at this round's end — the empty rim goes dark (marked ░); held spirits keep their light ░░░
```

After **the Dusk** (end of round 8) the page's empty edges go dark; an occupied
rim tile is **held** — it stands, scores, may step away (the ground darkening
behind it), and retaliates if struck, but it no longer intercepts and accepts no
new writing. The Solace's Unwritten are **not** held: the Dusk sweeps them from
the margin (the never-remembered are not kept). Rounds 9–12 are fought on the
inner board plus whatever is still held. **Round 12 is Nightfall**, and whoever
holds more of the page wins.

---

## The headless / JSON interface

For bots, scripts, and CI there is no text UI at all:

- **`recollect autoplay`** — the AI plays one seeded match to a result and prints
  a JSON result line (`{"result": …, "score_a": …, "score_b": …, "seed": …}`).
  With `--ndjson`, every game event is emitted as its own JSON line as it happens.
- **`recollect headless`** — a JSON-lines driver. **Your program controls Seat A**
  and the AI plays Seat B. Before each of your turns, the client prints your
  seat's redacted `PlayerView` as JSON on stdout; you reply with **one `Command`
  JSON per line** on stdin. An illegal command comes back as `{"rejected": …}` and
  the game waits for another; a malformed line comes back as `{"error": …}`. The
  match ends with the same result line as `autoplay`. Redaction holds here too —
  only Seat A's view is ever emitted.

`recollect online --json` is the networked equivalent: it speaks the same
one-`Command`-JSON-per-line grammar over the live server connection.

---

## Playing online from the terminal

Both transports can play the **authoritative server** instead of a local engine;
the server holds the one true game and ships the labeled legal-move menu with
every view, so a networked match plays exactly like a local one.

- `recollect online new` creates a match and prints Seat B's invite token
  (`--vs-bot` fills Seat B with the server's AI; `--2v2` opens a four-slot lobby
  and prints the other three tokens to share).
- `recollect online join <MATCH_ID> <TOKEN>` joins an existing match as Seat B.

A dropped connection reconnects automatically and resumes the same match.

### Optional verb shortcuts (both modes)

In **both** human modes you can pick moves by **number** from the menu — *and* you
may type a short verb instead (one shared parser). A tile token is accepted
**either** way: a board **index** (`12`) **or** a grid **coordinate** (`c3`) — type
whichever your board shows (local prints coordinates; the networked menu prints
indices `0`–`24`, `0` = a1, left-to-right then up):

| Verb | Action |
|---|---|
| `p <hand#> <tile> [e <tile>]` | **Play** a spirit (optionally engaging a tile on arrival) |
| `o <hand#> <tile>` | **Overwrite** a tile with a hand card |
| `m <from> <to> [e <tile>]` | **Move** a spirit (optionally engaging) |
| `v <tile>` (or `evolve <tile>`) | **Evolve** the base on `<tile>` — play a held form card onto it |
| `dv <tile>` (or `devolve <tile>`) | **Devolve** the standing-Faded form on `<tile>` — the rescue: play a held base card onto the banished form to recede it a tier (the Lorekeeper *reverts*, the Solace *recedes*) |
| `rc <tile>` (or `reclaim <tile>`) | **Reclaim** the standing spirit on `<tile>` for Anima |
| `g` (or `glimpse`) | **Glimpse** |
| `r <hand#>` | **Release** a hand card (when your hand is full at Flow) |
| `mull` (or `mulligan`) | **Mulligan** your opening hand — once, at the opening (see *The opening — mulligan once* below) |
| `end` | **End turn** |
| `q` | Quit |

A base may have several legal evolutions — one per matching form card you hold — so
`v <tile>` names only the tile; the client resolves it to the legal `Evolve` (and if
there's a choice, the numbered menu still shows each one). `dv <tile>` works the same
way for **Devolution**: it names only the faded-form tile and resolves to the legal
`Devolve` (the base card you hold), the menu disambiguating when several bases fit.

---

## Your turn, in plain terms

You rarely think about phase names — the menu folds them into labeled moves — but
here's the shape, so you know what's happening. The full rules (the income table,
the Dusk, scoring, Held Ground) live in
[design reference §5, "The Clock"](design.md); this is the
player's-eye summary.

**The opening — mulligan once (before your first move).** Right at the start, before
you've acted, you may **mulligan** your opening hand exactly once: you draw a fresh
full hand and then **one card goes to the bottom of your deck** — that bottomed card
is the cost (the hand you keep is one smaller). Which card is bottomed is fixed by the
shuffle, not chosen, so it's a single decision: *mulligan or keep*. It's a one-time
offer in the opening window only — once you've played, Glimpsed, or spent Anima, it's
gone. In the terminal it's the **`mull`** verb (or the `Mulligan …` menu entry); over
the network it's offered in the legal menu the same way. Your opponent is told *that*
you mulliganed (a fair, public beat) but never *what* you drew or bottomed. *(A 1v1
opening mechanic.)*

1. **Flow — the turn opens.** You draw a card and gain **Anima** (your income,
   which grows in the early rounds). If your hand is over its cap, **Release** one
   — a `Release …` menu entry, or the `r <hand#>` verb (in either mode).
2. **Fade — partings.** A spirit may leave the page: **Reclaim** one of your own
   standing spirits to cash it back for Anima (it leaves, no impression).
3. **Main — the heart of the turn.** No fixed action count — you act as far as your
   **Anima** reaches:
   - **Play** cards from your hand (spirits onto tiles; spells, terrain, and
     fabrications as their cards describe).
   - **Evolve** by **playing a form card from your hand onto its matching base**: a
     **Primal** onto one of your **Fading** bases, a **Fabled** onto a **healthy**
     base the turn after it arrived. The form costs **its own cost minus half the
     base's Anima** (rounded down) and arrives at **full HP**, leaving no impression —
     the spirit *became*. A base evolves to a Primal **or** a Fabled — a Primal can't
     later become a Fabled.
   - **Devolve** (`dv <tile>`) — the **rescue**. A Primal/Fabled form **banished in
     combat** stands Faded for a turn; play a **base card from your hand** onto it to
     **recede** it a tier down to that base, at **full HP**. It costs **half the
     banished form's Anima** (rounded down), and the rescued base is **summoning-sick**
     until your next turn. Devolution **is an arrival** (like evolution): it **engages
     no one** — it never strikes — but it can **re-complete a Throughline** if the base
     recedes straight into a standing three-in-a-row (+10/+10 and a full heal). Cycle
     evolve↔devolve freely (Lorekeeper *reverts*; Solace *recedes*).
   - **Call** a Kindred (one living Kindred per caller).
   - **Move** each **Mobile** spirit once, for free — but never the turn it
     arrived (summoning sickness). A spirit's free move can carry it into an
     engagement.
   - **Glimpse** once (the **`g`** move): **burn a chosen hand
     card** (the activation cost — it leaves play), then peek your top card and **keep**
     it (no Anima) or **bottom** it for **+1 Anima**. No longer free — net keep = −1
     card, bottom = −2 for +1; you can't Glimpse with an empty hand or deck.
   - **Overwrite** a tile, **Reveal** a lurker, **Reclaim**, make a **Choice** an
     effect offers — each appears as its own labeled move when available.

   Standing spirits never "act" on their own, but they still **retaliate** when
   struck and **intercept** through their reach.
4. **End Turn.** Nothing auto-ends — choose **End turn** (the menu entry, or the
   `end` verb) when you're done, and play passes. The match runs **twelve
   rounds**; the **Dusk** falls at the end of round 8, and **Nightfall** at round
   12. See [§5](design.md) for the Dusk, Held Ground, and how the
   Solace scores its erasures apart.

---

Prefer the browser? See the [website player guide](../site/guide.html). For the
manual checks a human runs before a release — real input, multi-client play — see
[the manual verification checklist](manual_verification.md).
