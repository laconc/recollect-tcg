# How to play Recollect

A player's guide to the two ways you can sit down at a telling — in your
**terminal** (the `recollect` command-line client) or in your **browser** (the
web client) — and to what you can actually *do* on your turn.

Recollect is a game of a fading Memory: two storytellers, the **Lorekeepers**
and the **Solace**, contend over a 5×5 page for who is remembered when night
falls. This guide covers the *interfaces and controls*. For the rules themselves
— scoring, the Dusk, keywords, what each card does — see the
[design reference](design.md) and the in-browser
[rules page](../site/rules.html). For a screen-by-screen walkthrough of one
interface, see the [terminal player guide](how_to_play_cli.md) (the `recollect`
CLI) or the [website player guide](../site/guide.html) (the browser client).

> A note on words. Spirits are **banished** from a telling, never "killed."
> Only the Solace **Unwrites**, and only the Solace's register speaks of
> "forgetting." The interfaces use this language; so does this guide.

---

## Quick start

**In your browser** — open the play client (the site's *Play* page → *Launch the
game*). Pick a deck style, and you're in a Quick Play match against the AI. The whole
game is drawn on the canvas: **tap a piece or card to select it, then a glowing target
to act** (or drag between them), and **End Turn** when you're done. The same canvas shell
drives online PvP and 2v2.

**In your terminal** — from the workspace, run the client:

```
cargo run -p recollect-cli            # local game vs the AI (you are Seat A)
cargo run -p recollect-cli -- hotseat # two humans sharing one terminal
cargo run -p recollect-cli -- watch   # spectate two AIs
```

(Once built, the binary is simply `recollect`; the examples below use that name.)
You'll be offered three deck styles, then shown the board and a numbered list of
your legal moves. Type a number to act.

Both clients run the **same** rules engine, so a move plays out identically
whether you're local, online, in the terminal, or in the browser — and each only
ever shows you *your* seat's view (you never see the opponent's hand or deck
order).

---

## The web client

The browser client draws **the entire game on a `<canvas>`** in the game's "paper &
ink" palette — the board, the HUD, your hand, every action affordance — set in a real
**serif typeface** (EB Garamond, rendered from a glyph atlas in the canvas, so the type
is crisp and matches the website's storybook register; the same atlas builds on the
future native shells). The layout is **responsive** — it reflows for phones, tablets,
and desktops: the canvas is **full-bleed** (it fills the viewport below the nav), sharp
on high-DPI screens, re-fitting on resize/rotate; the in-canvas controls are touch-sized
(an easy tap on a phone); on a wide desktop the game is a **centered, framed "table"**
(the side margins darken into a mat) rather than the phone stretched wide; nothing
scrolls sideways or clips.

In a **local (vs-AI) match the canvas draws the whole game shell and you play on
it directly**: around the board sit your **HUD**
(your **name/character** · your score · your **Anima** · the round on a 12-pip clock that
marks the **Dusk** at round 8 and **Nightfall** at 12), the **opponent strip** (their
**character name** · faction · their score — including the Solace's off-board erasure
tally · their hand as face-down card backs with an "N in hand" count), your **hand** as
real placeholder cards (cost · name · resonance · the labeled **Atk / Def / HP** stat
pills) in a **left↔right carousel** (a few big cards show; the rest scroll in — drag /
mouse-wheel / touch-swipe, or simply **hover the mouse near the left/right edge** to glide
it), and the **End Turn** and **Glimpse** buttons in their own control lane (a right rail beside the
board, or the HUD bar on a phone — clear of the play grid). **You act by touching the
canvas** — tap a piece or card to select it, then a glowing target to act, or drag between
them — and a **virtual accessibility tree** mirrors every affordance as a labeled button
for screen-reader and keyboard play.
**Online play (1v1 PvP) and 2v2 draw the SAME full shell** — the client builds it from the
server's redacted view + its legal moves (no local engine; the server is authoritative, so
you only ever see your own seat — the opponent is face-down backs / a count). The one
board-only view left is **Watch a 2v2** (spectating four AI players). Top to bottom, the
canvas regions are:

```
┌─────────────────────────────────────────────────┐
│ Recollect   Play Guide Cards Rules Lore   Options▾│  shared site nav (HTML); Options opens a
├─────────────────────────────────────────────────┤   panel: sound · reduced-motion · anim-speed
│  𝗖 𝗖𝗼𝗿𝗶𝗻 𝗔𝘀𝗵𝗲 · the Solace  score 5 - 3 erased ▮▮│  opponent strip (bold name; hand = backs)
│                ┌───────────────┐                  │
│                │      · ·      │                  │  the board (5×5 page; 6×6 in 2v2) — a bright
│                │   the board ^ │       ┌───────┐  │  lit leaf on a dark mat; a quiet · dot = a
│                │      ·        │       │Glimpse│  │  piece that can act; a ^ = a Fading base
│                └───────────────┘       ├───────┤  │  that can evolve. The control buttons sit
│  𝗗𝗿𝗲𝗮𝗺𝗲𝗿 𝗝𝘂𝗻𝗼  3  ◆ 4    ▮▮▮○○○○○○○○○ ⟳ │End Turn│  │  in their own RAIL (desktop), clear of the
│   ┌───┐ ┌───┐ ┌───┐   your hand        └───────┘  │  grid; HUD: bold name · score · Anima ·
│   │ 2·│ │ 1 │ │ 3 │   (tap to lift)               │  clock (no "Round N" text — the pips show it)
│   │Cin│ │Hsh│ │Tcl│   ──────●──────               │  hand carousel (real cards; · = playable;
│   └───┘ └───┘ └───┘                               │  a scroll thumb shows there's more)
│      ┌ Cinderling — Spirit · Atk 3 Def 1 HP 2 ┐   │  inspect panel (hover / long-press),
│      │ reach Cross · Mobile · ★ ● ● ●         │   │  anchored to the card, in the canvas
│      └─────────────────────────────────────────┘  │
└─────────────────────────────────────────────────┘
   (the a11y tree, the live status line + the #board-sr mirror are sr-only — present
    in the accessibility tree, invisible on screen; the canvas is the visible surface)
```

- **Board (canvas).** The page itself — your spirits and the opponent's, terrain,
  impressions, and the Dusk edge as it darkens. It animates between states (unless
  your system prefers reduced motion, which it honors). At rest, a **quiet dot** sits
  on any of your pieces that has an available action, a **green chevron (^)** (upward)
  marks a base that can **evolve**, and a **downward amber chevron (v)** marks a
  **standing-Faded** form that can **recede** (devolve — the rescue). A small corner
  dot on each of your **Mobile** spirits also cues its move this turn: **green** = it
  can still take its one free step; **dim** = it is rested (it already moved, or just
  arrived and is summoning-sick). A spirit of yours **banished in combat** doesn't
  vanish at once — it stands **Faded** for a turn, rendered with a warm **amber rescue
  glow** (brighter than a hopeless fade): a glance says *this one can be saved this
  turn*. The Solace's Unwritten leave **no** mark — its forgetting scores off the board
  (the HUD/opponent strip), not as a stain. **You play directly on the board**: tap one
  of your pieces (or a hand card) and the tiles it can reach **glow green** (engageable
  targets pulse **brighter**) — tap a glow to act, *or* drag from the piece to the
  target. **Evolution is a card you play:** a Primal/Fabled **form** sits in your deck
  and is drawn to hand like any spirit; pick up the **form card** and tap the matching
  **base** it can land on — a **Primal** onto one of your **Fading** bases (the
  last-round rescue), a **Fabled** onto a **healthy** base the turn after it arrived.
  (You can't evolve a base you hold no form card for.) **Devolution is the mirror —
  also a card you play:** when a form of yours stands Faded (the amber glow), pick up a
  **base card** from your hand and tap that faded form to **recede** it a tier back to
  the base, at full HP — the rescue. You **revert** as a Lorekeeper, **recede** as the
  Solace (one action, your faction's word). A spirit can **evolve↔devolve cycle**
  without limit, bounded only by the forms and bases in your hand.
- **HUD.** Your **score**, your **Anima** (the ◆ drop — your play budget, the real
  limiter since there's no fixed action count), and the **round** on a 12-pip clock
  (the lit pips count rounds elapsed; the **Dusk** pips past round 8 darken; the final
  **Nightfall** pip is ringed).
- **Opponent strip.** Their **character name** (e.g. "Corin Ashe") with an avatar
  initial, their faction, their **running score** (including the Solace's off-board
  erasure tally, shown as `score N - M erased` when non-zero), and their hand as that
  many **face-down card backs** with an "N in hand" count — a count only; you never see
  their cards.
- **Your hand (carousel).** Your cards as real placeholder cards (cost · name ·
  resonance · the labeled **Atk / Def / HP** stat pills) in a smooth **left↔right
  carousel** — a few big cards show at once; the rest scroll in by **drag**, **mouse
  wheel**, or **touch-swipe**, or simply by **hovering the mouse near the tray's left or
  right edge** (a gild scroll-thumb under the row shows there's more). A **quiet green
  dot** marks a card that has a legal play this turn (a dimmed card has none); a **green
  chevron** marks an evolution **form** card. Tap a card to **lift** it (it rises with a
  gild halo) and its legal tiles glow — tap a glow to place it, or drag the card onto a
  tile. A card whose only play is instant (a Ritual / an Unwriting) resolves the moment
  you pick it up.
- **End Turn & Glimpse (the control lane).** The two global controls live in their own lane,
  **clear of the play grid** so a board tap is never ambiguous — a dedicated **right rail** beside
  the board on a wide screen, the right of the **HUD bar** on a phone. **End Turn** (filled,
  primary) passes the turn (nothing auto-ends); **Glimpse** (outlined) is the §5 move: you
  **burn a card of your choice from your hand** (the activation cost), then see your top card and
  **keep it** on top (no Anima) or **bottom it for +1 Anima**. Tapping **Glimpse** opens an **in-canvas
  choice modal**: first the **burn** step — a card over the board with one chip per hand card (*Burn
  `<card>` · leaves play*, framed as the cost you spend); then, on the card you peek, the **keep /
  bottom** step — the peeked top card floated with two chips (*Keep on top · no Anima* and *Bottom it ·
  for +1 Anima*). Not free — it costs a hand card (and thins your deck), so it's a deliberate move, not
  every turn; you can't Glimpse with an empty hand or empty deck. Evolve and Reclaim are **contextual**
  affordances on the eligible piece, not global buttons.
- **Inspect panel.** **Hover** a card/piece with the mouse, or **long-press** it on
  touch, to float a panel beside it — full stats, keywords, rules text, and a passive
  **reach grid** (a gild centre, soft dots on the tiles it could threaten). Reach
  shows here **soft** (what *could* this threaten); engageable targets show **bright**
  on the board when you *select* (what you can hit right now) — two distinct reads.
  Inspect is a **read, not a turn action**: while the panel is up the board + your hand
  stay visible (dimmed for context) but the action dots and the **End Turn / Glimpse**
  buttons are hidden — you can't act while reading. Moving off the card (mouse) or a tap
  (touch) dismisses the panel and the affordances return. (The keyboard / screen-reader
  "Game actions" list is unaffected — it never enters inspect, so End Turn stays reachable.)
- **Status line (sr-only) + the nav.** The live **status line** (the round, the running
  score, your Anima, whose turn — it announces each change aloud), the virtual a11y tree,
  and the `#board-sr` board mirror are **screen-reader-only** — present in the
  accessibility tree, invisible on screen (the canvas is the visible surface; the
  announcements show in-canvas as captions and the HUD). The **shared site nav** (Play ·
  Guide · Cards · Rules · Lore) sits at the top with the **settings** (sound ·
  reduced-motion · animation-speed); starting a new match is the result screen's **New
  opponent** action (or **New game** off the shell).
- **The Dusk & Nightfall (on-canvas set-pieces).** At the end of round 8 the **Dusk**
  falls and at round 12 it is **Nightfall** — each arrives as an animated set-piece over
  the board: the rim **contracts and darkens** (the page failing at its edges), the
  binding strip lights as a **clock face**, and a seal reads "Dusk falls" / "Nightfall"
  (announced aloud too). A spirit that still **holds** a darkened rim tile stands
  **lamplit** — a small pool of light around it, visibly different from the live board,
  with no reach overlay; the light goes out the moment it leaves. (Reduced motion shows
  the set-piece near-instant; the announcement always lands.)
- **The result screen (on-canvas).** When the match ends, the canvas draws the
  **verdict** in the game's voice — *the Memory keeps [the winner]*, or *forgotten* (the
  Solace's erasure carried the page), or *both are kept* (a draw) — with a **score
  breakdown** (each side's board points, and the Solace's off-board erasure tally folded
  in) over the final board. Three actions follow: **Rematch** (a fresh match, same
  styles), **New opponent** (back to the picker), **Back to site**. The verdict and the
  actions are mirrored in the accessibility tree (the actions are real buttons) and the
  verdict is announced.

### Starting a game

The picker offers three deck styles plus an opponent-difficulty dropdown — **Easy /
Normal / Hard / Expert**. Each style shows BOTH its subjective one-line voice (the
*blurb*) AND its **objective selection-info** — four at-a-glance chips for the deck's
**resonance lean · tempo · aggression · body-vs-spell mix** (measured in the engine
over many deck-gen seeds), above a preview of the style's deck and cost curve — so you
choose on substance, not just flavor. (The CLI shows the same selection-info as a line
under each style.) Each style card is a real button: keyboard-focusable, and its
accessible name reads the whole shape in words for a screen reader. Click (or press
Enter on) a style to start a Quick Play match against the AI. The picker also offers
**Watch a 2v2** (spectate four AI players on a 6×6 board) and **Play online**
(host or join a networked match — see [Online play](#online-play-shared-by-both-clients)).

### Controls

| You want to… | Mouse | Touch | Keyboard |
|---|---|---|---|
| Move a spirit on the board | Click the spirit, then a glowing tile — or drag it there | Tap the spirit, then a glowing tile — or drag it | Tab to the board, arrow to the spirit, Enter; arrow to a glow, Enter |
| Play / overwrite with a hand card | Click the card (it lifts), then a glowing tile — or drag it | Tap the card, then a glowing tile — or drag it | Tab to the board, arrow to the card region…¹ |
| Evolve (play a form card onto its base) | Click the form card in hand, then the glowing base | Tap the form card, then the base | …or use the actionable list: focus the form card, Enter; focus the base, Enter |
| Devolve / recede (play a base card onto a standing-Faded form) | Click the **base card** in hand, then the glowing **amber-lit faded form** | Tap the base card, then the faded form | …or in the actionable list: focus the base card, Enter; focus the faded form, Enter |
| End the turn / Glimpse | Click the **End Turn** / **Glimpse** button on the canvas | Tap it | Tab to **End turn** / **Glimpse** in the actions list, Enter |
| Inspect a card or piece | **Hover** it (a panel floats beside it) | **Long-press** it | Focus its button in the actions list (the panel + the text readout update) |
| Cancel a pick-up | Click the piece again, or empty canvas | Tap again, or empty canvas | Esc (on the board), or re-activate the same button |
| Start a new game | Click **New game** | Tap **New game** | Tab to it, Enter or Space |

¹ For screen-reader and keyboard players the **virtual accessibility tree** below the
canvas (the "Game actions" group) lists **every** affordance as a labeled button —
board tiles, hand cards, the opponent strip, End Turn, Glimpse — so two activations
(select, then a highlighted target) complete any action by keyboard, mirroring the
tap-then-target gesture. The board sits inside that tree as a real **ARIA grid** with a
cell per tile (each announced by its **row and column**), so a screen reader can sweep
the board coordinate by coordinate as well as jump straight to the actionable pieces.

Notes that matter:

- **Two gestures, both complete.** On the board, **tap-then-target** *and*
  **drag** both make the same move: pick up a spirit or a hand card and the legal
  destinations **glow green** (an engageable target pulses brighter) — tap a glow, or
  drag onto it. Picking up a **form card** lights the matching **base** it can land on
  as an **Evolve** target; picking up a **base card** lights any **standing-Faded form**
  it can recede as a **Devolve** target. A quiet **dot** on a piece/card means it has an
  available action; an **upward green chevron** marks a base (or form card) that can
  **evolve**, and a **downward amber chevron** marks a standing-Faded form (or its base
  card) that can **recede** (devolve).
- **Reach vs. targets — two reads, no clash.** **Inspect** (hover / long-press)
  shows a card's **reach** *softly* — what it *could* threaten. **Select** (pick up)
  shows the **engageable targets** *brightly* — what you can hit right now. Inspecting
  reveals possibility; selecting commits to action.
- **The canvas is fully accessible.** The board is a focusable input surface (one Tab
  stop; arrow keys move a **gold cursor**, Enter/Space picks up and places, Esc
  cancels) **and** a parallel text region (`#board-sr`) narrates it tile by tile. On
  top of that, the **virtual accessibility tree** mirrors every canvas affordance as a
  real `<button>` that fires the **same** command the canvas does — so the whole match
  is reachable by **keyboard alone**, and a screen reader announces each piece, card,
  and action. Inside that tree the board is exposed as a true **ARIA grid**
  (`role="grid"` → rows → a `gridcell` for **every** tile, each with its
  `aria-rowindex` / `aria-colindex`), so a screen reader announces a tile as **"row R,
  column C, &lt;occupant&gt;"** and arrow-navigates the board cell by cell — empty tiles
  included, so nothing is skipped; only the tiles you can act on are focus stops, so the
  action is never buried. The live status line narrates every change aloud. This is the
  accessible path (AGENTS.md invariant 7) — at parity with the visual canvas, never a
  lesser one, to **WCAG 2.1 AA**.
- **The opponent's turn.** In a vs-AI game you **watch** the opponent play, paced
  action-by-action (~1 second a beat — a played spirit, a move, an Unwriting), each with
  a subtle on-canvas caption naming what happened and the affected tile lit; the Solace's
  erasure tally counts up on an Unwriting. The same line is read aloud in the live status
  region, so a screen-reader hears the turn unfold exactly as the canvas shows it. While
  the opponent tells, your affordances (and their accessible twins) are inert — they
  return when it's your turn again. **Pacing speed** is the **Animation** control in the
  top bar (**normal / fast**); it's one global setting, not per-action — and if your
  system prefers reduced motion the replay plays near-instant. You'll also hear the
  opener at match start (who takes the first word) and **Dusk** / **Nightfall** when the
  binding clock turns.

---

## The terminal client (CLI)

The `recollect` command is **one binary across two choices**: *transport* (a
local in-process game, or `--server` to play over the network against the
authoritative server) and *interface* (an interactive text UI, the default, or a
headless JSON mode for scripts and bots). They all embed the same rules engine.

### Modes

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
`--faction <solace|lorekeeper>` (which faction the AI opponent fields — vs the AI
you face a named character of that faction; the Solace is the default),
`--server <URL>` (for `online`), `--json` (machine output for `online`),
`--ndjson` (stream every event for `autoplay`).

### The interactive UI — a cursor TUI, or the line REPL

The terminal client has **two** interactive faces, picked automatically:

- A **local 1v1 vs the AI in a real terminal** runs the **cursor TUI** — a
  full-screen `ratatui` board with a **gold cursor** you drive with the **arrow
  keys** (the terminal twin of the web's gold cursor): **Enter/Space** picks up the
  piece/card under the cursor and places it on a highlighted target, **Esc** cancels,
  **Tab** moves between board and hand, **i** inspects, **:** opens a verb buffer, and
  the numbered legal-move list stays visible the whole time. See the
  [terminal player guide](how_to_play_cli.md#the-cursor-tui-local-1v1-in-a-real-terminal).
- Everything else — `hotseat`, `watch`, **online**, **2v2**, and any **non-TTY** run
  (a pipe, `--json`, `headless`, CI) — uses the **line REPL** below.

The **line REPL** is **line-based**: it prints the board and a numbered menu of your
legal moves, then waits for you to type a line. There is no full-screen "press a key"
mode here — you read, then type a number (or a short command) and press Enter. A turn
looks like this:

```
══ Round 3/12 · Seat A to act · your turn ══
5 [        ][        ][        ][        ][        ]
4 [        ][        ][   ::   ][        ][        ]
3 [        ][   ::   ][ACind  3][   ..   ][        ]
2 [        ][        ][        ][        ][        ]
1 [        ][        ][        ][        ][        ]
   a         b         c         d         e      (:: yours · .. theirs · ~ fading · ^ can evolve · ! Echo · ° held/lamplit · ⌂ landmark · ▒ fabrication · ░ dusk)
 ▸c3 A Cinderling             A2   D1   HP   3/3   Adjacent Fire ⇢ can step
(▸ = action available · ⇢ = a Mobile spirit can still step · ⊘ = rested · use the numbered tellings below)
Score if Nightfall struck now: A 1 — B 0
You: 4 anima (your play budget — no fixed action count; End Turn when ready) · deck 14 · hand:
   0. Tide-Caller            3c  2/ 2/ 4 Adjacent Water
   1. Hush                   1c  0/ 0/ 1 Self     Calm
Them: 4 anima · hand 5 · deck 14

Legal tellings:
    0. Play Tide-Caller → b3
    1. Move c3 → c4
    2. Glimpse (burn a hand card, then peek your top card)
    3. End turn
  (type a number to act · 'i N' inspect hand card N · 'i <tile>' e.g. 'i c3' inspect a board card)
your move >
```

Picking **Glimpse** (§5) re-prompts TWICE. First the **burn** cost: the
menu redraws with one option per hand card — e.g. `0. Burn Tide-Caller to glimpse`,
`1. Burn Hush to glimpse` — pick which card to spend (it leaves play). Then the
**keep-or-bottom** choice on the card you peek — e.g. `0. Keep Dawnling on top` and
`1. Bottom Dawnling for +1 anima`; keep leaves it on top for no Anima (net −1 card),
bottom sends it under for +1 (net −2 cards). Glimpse is omitted from the menu when your
hand or deck is empty (nothing to burn / nothing to peek).

Reading the board:

- Columns are lettered **a–e**, rows numbered **1–5** (so `c3` is the centre).
  Each cell shows the owner (`A`/`B`), a 4-letter card abbreviation, and its HP.
- The legend after the grid explains the markers: `::` a tile *you* threaten,
  `..` a tile *they* threaten, `~` a fading spirit, `^` one that can evolve, `!` a
  live Echo, `°` a held (lamplit) spirit out on the darkened rim, `⌂` a landmark,
  `▒` a fabrication, `░` the Dusk.
- Below the grid, each spirit is listed with full stats; a green `▸` marks a
  spirit you have an action available on. For your **Mobile** spirits a movement cue
  follows: `⇢ can step` if it can still take its one free Move this turn, `⊘ rested`
  if it has spent that Move or just arrived (a freshly-arrived spirit has summoning
  sickness and can't step until next turn).
- Then a running score *if Nightfall struck now* — Seat A's board points and Seat
  B's. The Solace (Seat B) scores its board presence **plus** its off-board
  **erasure tally** (every banish or Unwriting it lands); the Unwritten leave no
  mark, so when that tally is non-zero the line shows the split, e.g.
  `B 3 (board 1 + 2 erased)`. At Nightfall the tally folds into B's total.
- Finally your Anima — **your play budget**, the real limiter on what you can play
  (there is no fixed action count; the turn runs until you End it) — your hand, and
  the opponent's public counts.

### Controls (line REPL)

*(The cursor TUI's keys — arrows / Enter-Space / Esc / Tab / `i` / `:` — are in the
[terminal player guide](how_to_play_cli.md#controls-cursor-tui). The table below is the
**line REPL**, used by `hotseat` / `watch` / online / 2v2 and any piped run.)*

| You type | What happens |
|---|---|
| a number, e.g. `1` | Take that move from the numbered **Legal tellings** menu |
| a **verb**, e.g. `m c3 c4` | Act directly — the faster path (see *Optional verb shortcuts* below) |
| `i N`, e.g. `i 0` | Inspect hand card **N** (stats, keywords, rules text, reach grid) |
| `i <tile>`, e.g. `i c3` | Inspect the card on board tile **c3** |
| `q` | Quit / pause (the seed printed at the top resumes the exact match) |
| (deck-style prompt) a number 1–3 | Choose one of the three offered deck styles at the start |

The numbered menu *is* the move set — engaging, evolving, overwriting, reclaiming,
choices, ending your turn are all just entries in the list, each spelled out in a
readable label (often with a combat forecast, e.g. `Play Cinderling → c3 ⚔ d3
[deal 2 · take 1]`). The simplest path is always to **pick a number** — but you can
also type a **verb shortcut** (below) for a faster move.

#### Optional verb shortcuts (both human modes)

In **both** human modes — local *and* online — you can pick moves by **number** from
the menu, *and* you may type a short verb instead (one shared parser, so both
modes speak one language). A tile token is accepted **either** way: a board **index**
(`12`) **or** a grid **coordinate** (`c3` — column `c`, row `3`). Type whichever your
board shows — local play prints coordinates, the networked menu prints indices `0`–`24`
(`0` = a1, counting left-to-right then up) — the parser takes both regardless of mode.

| Verb | Action |
|---|---|
| `p <hand#> <tile> [e <tile>]` | **Play** a spirit (optionally engaging a tile on arrival) |
| `o <hand#> <tile>` | **Overwrite** a tile with a hand card |
| `m <from> <to> [e <tile>]` | **Move** a spirit (optionally engaging) |
| `v <tile>` (or `evolve <tile>`) | **Evolve** the base on `<tile>` — play a held form card onto it |
| `dv <tile>` (or `devolve <tile>`) | **Devolve** the standing-Faded form on `<tile>` — the rescue (play a base card from hand onto it; you *revert* as a Lorekeeper, *recede* as the Solace) |
| `rc <tile>` (or `reclaim <tile>`) | **Reclaim** the standing spirit on `<tile>` for Anima |
| `g` (or `glimpse`) | **Glimpse** |
| `r <hand#>` | **Release** a hand card (when your hand is full at Flow) |
| `mull` (or `mulligan`) | **Mulligan** your opening hand — once, at the very start (see *The opening* below) |
| `end` | **End turn** |
| `q` | Quit |

A base may have several legal evolutions — one per matching form card you hold — so
`v <tile>` names only the tile; the client resolves it to the legal `Evolve` (and if
there's a choice, the numbered menu still shows each one). **Devolution** (`dv <tile>`)
works the same way: a Primal/Fabled form **banished in combat** lingers standing-Faded
for a turn (the `~` marker), and on your Main you may play a **base card from your hand**
onto it — the form **recedes a tier** back to that base at full HP. It costs **half the
banished form's Anima** (rounded down) and **engages no one** (the recede never strikes),
but it **is an arrival** like an evolution — so a base that recedes straight into a
standing 3-line **completes its Throughline on the spot** (+10/+10 and a full heal). The
rescued base is **summoning-sick** until your next turn (it can't move or evolve that
turn). A spirit can cycle evolve→devolve→evolve without limit, bounded only by the forms
and bases in your hand. *(On the **web canvas** the recede has its own affordance — a
standing-Faded form glows with a warm amber "rescue" light and a **downward amber
chevron**; you pick up the **base card** and tap the faded form to recede it, mirrored
in the accessibility tree and announced. See [the web client](#the-web-client) below.)*
*(Online only:* a dropped connection reconnects automatically and resumes the same match —
reopen the page or relaunch with your **seat link** and you pick up exactly where you left
off. But don't wander off: if you disconnect and **don't return within ~2 minutes**, the
match is **forfeited** to your opponent (in **2v2**, one absent teammate forfeits the whole
team). The grace is generous enough to ride out a flaky network or a tab reload; it only
ends a match someone has truly abandoned.)

### The headless / JSON interface

For bots, scripts, and CI there is no text UI at all:

- **`recollect autoplay`** — the AI plays one seeded match to a result and prints
  a JSON result line (`{"result": …, "score_a": …, "score_b": …, "seed": …}`).
  With `--ndjson`, every game event is emitted as its own JSON line as it happens.
- **`recollect headless`** — a JSON-lines driver. Your program controls Seat A and
  the AI plays Seat B. Before each of your turns the client prints your seat's
  redacted view as JSON on stdout; you reply with **one `Command` JSON per line**
  on stdin. Illegal commands come back as `{"rejected": …}` and the game waits for
  another. The match ends with the same result line as `autoplay`. Redaction holds
  here too — only Seat A's view is ever emitted.

`recollect online --json` is the networked equivalent: it speaks the same one-
`Command`-JSON-per-line grammar over the live server connection.

---

## Online play (shared by both clients)

Both clients can play against the **authoritative server** instead of a local
engine. The server holds the one true game; each client renders the view it's sent
and forwards moves.

- **Browser:** the picker's **Play online** form lets you host (choose 1v1 / 2v2,
  opponent faction, bot difficulty, and a player-or-bot per seat) or **join** with
  an invite code. Hosting shows you the invite code(s) to hand to the other
  players; the server address is editable.
- **Terminal:** `recollect online new` creates a match and prints Seat B's invite
  token; `recollect online join <MATCH_ID> <TOKEN>` joins one. `--vs-bot` fills
  Seat B with the server's AI; `--2v2` opens a four-slot lobby and prints the other
  three tokens to share.

Online play offers the **full** move set in both clients — the server ships the
legal-move list with every (redacted) view, so a networked game plays exactly like a
local one: the **browser** renders that list as the canvas-native affordances (the same
shell as a local match), and the **terminal** as its labeled legal-move menu.

---

## Your turn, step by step

A turn moves through three phases — **Flow → Main → Fade** (the **Fade** is at the
turn-**END**, after Main) — and then you end it. The clients fold these into the
affordances and prompts, so you rarely think about the phase names; here's the shape
so you know what's happening. The full rules (income table, the Dusk at the end of
round 8, scoring, Held Ground) live in
**[design reference §5, "The Clock"](design.md)** — this is just the
player's-eye summary.

**The opening — mulligan once (before your first move).** Right at the start, before
you've acted, you may **mulligan** your opening hand exactly once: you draw a fresh
full hand and then **one card goes to the bottom of your deck** — that bottomed card
is the cost (the hand you keep is one smaller). Which card is bottomed is fixed by
the match's shuffle, not chosen (so it's a single decision — *mulligan or keep* — with
no second pick). It's a one-time decision in the opening window only — once you've
played, Glimpsed, or spent Anima, the offer is gone. Your opponent is told *that* you
mulliganed (a fair, public beat) but never *what* you drew or discarded. On the **web
canvas** it opens automatically as an **in-canvas modal** at the start — a card over the
board offering **Mulligan** (redraw, bottom one) or **Keep this hand** (Esc keeps);
in the terminal it's the `mull` verb (or a menu entry); over the network it's offered in
the legal menu the same way. *(This is a 1v1 opening mechanic.)*

1. **Flow — the turn opens.** You draw a card and gain **Anima** (your income,
   which grows in the early rounds). If your hand is over its cap, you **Release**
   one — in the browser this is a `Release …` button; in the terminal it's a
   `r <hand#>` verb / a menu entry, and the prompt tells you a release is required.

2. **Main — the heart of the turn.** There is **no fixed action count** — you act as
   far as your **Anima** reaches:
   - **Play** cards from your hand (spirits onto tiles; spells, terrain, and
     fabrications as their cards describe).
   - **Evolve** by **playing a form card from your hand onto its matching base**:
     a **Primal** onto one of your **Fading** bases, a **Fabled** onto a
     **healthy** base the turn after it arrived. The form costs **its own cost minus
     half the base's Anima** (rounded down) and arrives at **full HP**. A base evolves
     to a Primal **or** a Fabled — a Primal can't later become a Fabled (both grow from
     the base).
   - **Devolve** — the **rescue**. A Primal/Fabled form **banished in combat** stands
     Faded for a turn; play a **base card from your hand** onto it to **recede** it a
     tier down to that base, at **full HP**. It costs **half the banished form's Anima**
     (rounded down), and the rescued base is **summoning-sick** until your next turn.
     Devolution **is an arrival** (like evolution): it **engages no one** — it never
     strikes — but it can fire arrival triggers, chiefly **re-completing a Throughline**
     if the base recedes straight into a standing three-in-a-row (+10/+10 and a full
     heal). You may cycle evolve↔devolve freely. (The Lorekeeper *reverts*; the Solace
     *recedes* — one action, the faction's word.)
   - **Call** a Kindred (one living Kindred per caller).
   - **Move** each **Mobile** spirit once, for free — but never the turn it arrived
     (a freshly-arrived spirit has summoning sickness). A spirit's free move can
     carry it into an engagement.
   - **Glimpse** once (the **Glimpse** button/move): **burn a card
     of your choice from your hand** (the activation cost — it leaves play), then see the
     top card of your deck and **keep it** there (no Anima) or **bottom it for +1 Anima**.
     The burn is what makes it a real decision: net keep = −1 card for foresight, bottom
     = −2 cards for +1 Anima. No longer free or every-turn — it costs a hand card and
     thins your deck, and you can't Glimpse with an empty hand or deck.
   - **Overwrite** a tile, **Reveal** a lurker, **Reclaim**, make a **Choice** an
     effect offers — each surfaces as its own affordance (web) or labeled move
     (terminal / online) when it's available.

   Standing spirits never "act" on their own, but they still **retaliate** when
   struck and **intercept** through their reach.

3. **Fade — the turn closes.** This is at the **END** of your turn (the order is
   **Flow → Main → Fade**). A spirit of yours that was **banished in combat** stood
   Faded through this Main (your one chance to **evolve or devolve** it); if you did
   neither, it dissolves now, leaving the banisher's impression. (A base banished on
   your *own* turn skips this Fade and dissolves at your *next* turn-end, so it too gets
   a full Main.) Nothing else fades here — the **Dusk** is separate: it sweeps the empty
   rim **instantly** when round 8 ends (the Solace's rim Unwritten vanish at once).

4. **End turn.** When you're done, choose **End turn** (the button, or `end` /
   the menu entry). Play passes to your opponent. The match runs **twelve rounds**;
   at the end of round 8 the **Dusk** falls and the page's empty edges go dark.
   When the clock runs out it's **Nightfall**, and whoever holds more of the page
   wins. (See [§5](design.md) for the Dusk, Held Ground, and how the
   Solace scores its erasures apart.)

The clients always show you exactly which of these are available *right now* — on
the web as the in-canvas affordances (the dots, the glowing tiles, the FABs) and
their accessible twins in the actions list; in the terminal / online as the numbered
or labeled moves. They are the complete, legal set for your turn.

---

## What the canvas client covers

The **canvas-native client** draws the whole game — local 1v1, online 1v1 PvP, and
2v2 — on the canvas: you play entirely on it (tap-then-target **and** drag,
hover/long-press inspect, in-canvas End Turn / Glimpse / evolve / reclaim), the
**Glimpse** (burn → keep/bottom) and the opening **Mulligan** resolve on **in-canvas
choice modals** (the options mirrored in the accessibility tree, each step announced),
the opponent's turn is **watched and paced** action-by-action (in a vs-AI game), the
**Dusk** and **Nightfall** arrive as animated on-canvas set-pieces (the rim contracts
and darkens, held spirits stand **lamplit**, the binding strip lights as a clock face),
the match ends on an **in-canvas result screen** (the verdict in the game's voice · the
score breakdown · **Rematch / New opponent / Back to site**), and the **virtual
accessibility tree** mirrors every affordance — including the choice options and the
result actions — as actionable buttons, each announced in the live region.

The one board-only view is **Watch a 2v2** (spectating four AI players). The
**Souvenir** (a shareable, provably-fair artifact of a finished match) is planned
for a later release.

For the manual checks a human runs before a release — real mouse/touch/keyboard,
multi-client play, screen-reader behavior — see
[the manual verification checklist](manual_verification.md).
