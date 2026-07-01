# Manual verification checklist (sandbox-deferred)

CI builds and tests everything headless, but some behavior can only be confirmed by a
human actually running it — the browser UI, real input devices (mouse/touch/keyboard),
multi-client play, and the observability stack. This is that checklist. Run it before a
release, and after touching any surface below.

Setup: build the site + client with `make site` and serve `dist/` (`make site-serve`);
run the server with `cargo run -p recollect-server` (set `DATABASE_URL` for the
Postgres-authoritative path, or leave it unset for in-memory play).

## Play client (the wasm canvas — the canvas-native shell)
- [ ] The whole game renders in the paper&ink palette **in the canvas** — the board, the HUD (score · Anima · the 12-pip clock with the Dusk + Nightfall markers), the opponent strip (bold name · score incl. erasures · face-down backs), the hand tray (real cards), and the End-Turn/Glimpse control lane all draw.
- [ ] **Visual polish** — cards/buttons/panels/board-pieces are **rounded** with **soft single-light drop shadows** (no hard grey halos); the board is a **bright lit leaf** framed in burnished gold on a **dark mat** (wide desktop); placed spirits wear the **hand-card look** (seat-ink title band · name · Atk/Def/HP foot — not flat squares); **character names are bold + seat-coloured**; the gold accent reads **burnished/antique** (not bright lemon); there is **no "N in hand" or "Round N" text** (the fanned backs + the pip strip convey both).
- [ ] **Options nav** — the top nav reads **Play · Guide · Cards · Rules · Lore · Options**; clicking **Options** opens a panel with **sound · reduced-motion · animation-speed** (Esc / click-outside closes it; keyboard-reachable). The raw controls are NOT inline in the bar.
- [ ] **The picker (Choose your match)** — each of the three offered styles shows its one-line voice (*blurb*) AND the **objective selection-info**: four chips for **resonance lean · tempo · aggression · body-vs-spell mix** (above the spirits/spellbook/cost-curve/opens preview). Each style card is a real **button**: keyboard-focusable with a visible focus ring, hovering a chip reveals its detail gloss, and with a screen reader the card's accessible name reads the whole objective shape in words ("Play Embertide. … Fury · Aggressive · even tempo · balanced bodies. …"). (The CLI shows the same selection-info as a line under each style.)
- [ ] Quick Play vs the AI is playable end-to-end **entirely on the canvas**, to a result — no HTML move buttons (`#moves`/`#hand` stay empty).
- [ ] **Affordances at rest** — a quiet **dot** sits on each of your pieces / hand cards that has an available action; a **green chevron** marks a Fading spirit that can evolve and an evolution **form** card in hand; a card with no legal play reads **dimmed**.
- [ ] **Select-then-target** — tapping/clicking one of your pieces highlights its legal targets (engageable ones read **brighter**); tapping a highlight acts. Tapping a hand card **lifts** it (a gild halo) and highlights where it can be placed; tapping a highlight places it. A move that lands on a defender resolves the combat (engage). Tapping the picked-up piece/card again (or empty canvas) cancels.
- [ ] **Drag** — dragging from a piece/card onto a legal target does the **same** action as select-then-target (the literal drag gesture, on mouse and touch). A drag that drops on nothing legal cancels cleanly.
- [ ] **Evolve = play a form card** — pick up a **form** card from your hand; the matching **base(s)** on the board highlight as Evolve targets; tap/drag onto a base to evolve (a Primal onto a Fading base, a Fabled onto a healthy base). A Fading base also shows the evolve chevron.
- [ ] **Inspect** — **hovering** a card/piece (mouse) or **long-pressing** it (touch) floats a panel beside it with full stats, keywords, rules text, and a **passive reach grid** (soft dots = what it could threaten). The reach read (soft, on inspect) is visually distinct from the engageable-target highlight (bright, on select) — they don't clash. Moving away / dragging dismisses the panel.
- [ ] **Control lane** — the **End Turn** (filled/primary) passes the turn; **Glimpse** (outlined) is the §5 move — burn a chosen hand card (activation cost), then peek the top and keep or bottom it for +1 Anima. Both sit in their own lane — a right **rail** beside the board (wide screen) or the **HUD bar** (phone) — never over the play grid, so a board tap is never ambiguous. Both are tap targets on the canvas. The burn → keep/bottom choice resolves on an in-canvas modal.
- [ ] **Keyboard board navigation** — Tab reaches the board (one Tab stop) with a visible outline + a gold in-canvas cursor; **arrow keys** move the cursor; **Enter/Space** picks up / drops; **Esc** cancels.
- [ ] **The virtual a11y tree (invariant 7)** — below the canvas, the off-screen "Game actions" group (`#shell-a11y`) lists **every** affordance as an actionable `<button>` — board tiles, hand cards, the opponent strip (a redaction-safe count, never their cards), End Turn, Glimpse — each firing the **same** command its canvas affordance does. Tab reaches each; Enter activates; two activations (select, then a highlighted target) complete any action. The labels read clearly (e.g. "Cinderling, cost 2, Spirit, attack 3…, playable").
- [ ] **The board is a true ARIA grid** — within `#shell-a11y` the board is a `role="grid"` (with `aria-rowcount`/`aria-colcount`) → `role="row"` rows → a `role="gridcell"` for **every** tile (each with `aria-rowindex`/`aria-colindex`). With a real screen reader using **grid / table navigation** (e.g. VoiceOver `Ctrl+Opt+arrows`, NVDA `Ctrl+Alt+arrows`): the board reads as a grid, moving cell to cell announces **"row R, column C"** with the occupant (or "empty"), every tile is reachable (no gaps), and only the actionable tiles are Tab stops (the empties don't trap focus). 5×5 in 1v1, 6×6 in 2v2.
- [ ] Layout works on a **phone** (narrow) and a **large screen** — the canvas scales, the in-canvas bands reflow, nothing overflows or clips. (Responsive — check each below.)
  - [ ] **Phone portrait** (~360–414px): the 5×5 *and* 6×6 board fit without clipping; **no horizontal scroll** anywhere; the in-canvas HUD / hand tray / FABs stay legible and on-screen.
  - [ ] **Phone landscape** (short, wide): the board sits beside the bands (not squeezed under the address bar); nothing overlaps.
  - [ ] **Tablet** and **desktop**: the board caps at a comfortable size and stays centered; the bands widen but stay readable.
  - [ ] **Touch targets**: the in-canvas hand cards, FABs, and board tiles are easy taps on a real phone/tablet; the picker's HTML controls are ≥44px; no sticky "stuck-hovered" look after a tap.
  - [ ] **Crispness**: the canvas is sharp (not blurry) on a high-DPI phone and after rotating / resizing; tapping a piece, a hand card, or a FAB still hits the right one at every size.
- [ ] **The paced opponent replay** — ending your turn **watches** the AI play, action-by-action (~1s/beat), each with an on-canvas caption + the affected tile lit and the same line read aloud; your affordances are inert while it tells and return after. The **Animation** control (normal/fast) paces it; reduced motion plays it near-instant.
- [ ] **The Dusk & Nightfall set-pieces** — at the end of round 8 (**Dusk**) and round 12 (**Nightfall**) an animated set-piece plays over the board: the **rim contracts and darkens** (deeper at Nightfall), the binding strip lights as a **clock face**, and a seal ("Dusk falls" / "Nightfall") fades up and is read aloud. A spirit that still **holds** a darkened rim tile stands **lamplit** (a warm pool of light around it, no reach overlay); an empty faded tile is pure night.
- [ ] **The in-canvas result screen** — when the match ends, the canvas draws the **verdict in the game's voice** (*the Memory keeps [winner]* / *forgotten* / *both kept*, tinted to the winner's ink) over a scrim of the final board, with a **score breakdown** (board points + the Solace's erasure tally) and three actions: **Rematch** (a fresh match), **New opponent** (the picker), **Back to site**. Tapping each works; the verdict + actions are mirrored in `#shell-a11y` (the actions are real buttons) and the verdict is announced.
- [ ] With a screen reader: the board is a labeled, focusable input surface described tile by tile by the `#board-sr` mirror; **the `#shell-a11y` tree announces every piece/card/action and is fully operable** (Tab + Enter); status messages (each pick-up / move / turn change, the opener, the Dusk/Nightfall seals, the verdict) are read aloud as they change — at phone and desktop sizes alike (the responsive layout must not break the mirror or the tree). The result screen's actions are reachable + operable by keyboard.
- [ ] With "reduce motion" enabled in the OS, animations collapse to near-instant (the paced replay and the Dusk/Nightfall set-pieces still narrate + show their end-state; nothing forces you to wait on motion).

## Host a match + online play
- [ ] The host form exposes mode (1v1 / 2v2), opponent faction, bot difficulty, and a per-seat human/bot choice; the 2v2 seat controls appear only in 2v2.
- [ ] Creating a match shows the invite code(s); the server-address field is editable and used.
- [ ] A second client joins with an invite code and plays; each move shows up for both players.
- [ ] **Online play (1v1 PvP) draws the FULL canvas shell** — the same board · HUD · hand · affordances · inspect · result screen as a local match, built from the server's redacted `PlayerView` (no local engine). You act on the canvas (tap-select / drag / FABs / keyboard); there are no HTML move buttons (`#moves`/`#hand` stay empty). **Redaction:** you only ever see your own seat — the opponent's hand is **face-down backs / a count**, never their cards; their board plays appear as the server pushes the new view.
- [ ] **2v2 (online) draws the full shell over the 6×6 board** — your slot's HUD / hand / affordances; the **opposing team is combined counts** (face-down backs), never their cards. The §5 Glimpse modal surfaces for **your** seat only; the opening Mulligan is offered at the window.
- [ ] **The online a11y tree (invariant 7)** mirrors the canvas the same way it does locally — board tiles / hand cards / End Turn / Glimpse as actionable buttons firing the server commands, the opponent strip a redaction-safe count; Tab + Enter operate it. The verdict (the in-canvas result screen, built from the server's Finished view) is announced + keyboard-operable.
- [ ] A display name typed in the form is shown to the table (both the host's and a joiner's name appear) — and on the canvas HUD (you) + opponent strip (them).
- [ ] 1v1 vs a bot: the bot takes its turns at the chosen difficulty, playing as the chosen faction.
- [ ] 2v2 with one or more bot seats: the server drives those seats in turn — the match never stalls waiting on a bot.
- [ ] Leaving a match resolves it (the remaining player wins) rather than hanging.

## Terminal client (CLI)
The `recollect` client (see [how_to_play_cli.md](how_to_play_cli.md)). The text/frame
screens are golden-snapshotted GPU-free in `make test` (`docs/gallery/tui/`, the
line-based stills regen with `make tui-gallery`; the cursor frames re-bless with
`BLESS=1 cargo test -p recollect-cli --test cursor_tui`); these checks are the
**real-TTY** behaviours those goldens can't exercise — live keyboard input. Run
`cargo run -p recollect-cli` (local vs the AI) and `recollect online …` against a server.

A local 1v1 vs the AI in a **real terminal** runs the **cursor TUI** (the ratatui
arrow-key board, the terminal twin of the web's gold cursor); `hotseat` / `watch` /
online / 2v2, and any **non-TTY** run (a pipe, `--json`, `headless`, CI), keep the
**line REPL**. Verify the cursor mode first, then the line REPL (e.g. `recollect | cat`
forces the line path):
- [ ] **(TTY cursor)** Launching `recollect` (local vs AI) opens the full-screen cursor
  board: a gold **cursor** sits on the centre tile, the **legal-move list** stays visible
  beside it, and the reused stats/score/hand render fills the right pane. **Arrow keys**
  move the cursor and **clamp** at the board edges (never wrap). `q` quits to the closing
  screen; the terminal is **restored cleanly** (no raw-mode residue, cursor visible) on
  exit, on `q`, and after a `Ctrl-C`-free finish.
- [ ] **(TTY cursor)** **Pick up → place**: arrow to one of your spirits, **Enter/Space**
  lifts it (its legal **targets highlight gold**), arrow to a gold target, Enter commits
  the **Move/Evolve**. **Tab** switches to the **hand**; pick a card, Enter lifts it
  (targets glow), place it on a tile to **Play/Overwrite/Evolve**. **Esc** cancels a
  pickup. A piece/card with no legal action does not lift (it flashes a hint).
- [ ] **(TTY cursor)** **`i`** opens the inspect overlay (full stats + reach grid) for the
  piece/card under the cursor; **`?`** shows the key legend; **`:`** opens the verb
  mini-buffer (type `m c3 c4`, Enter) — all close/return cleanly. A bare **number** + Enter
  still picks from the numbered legal list. **Glimpse** (`g`) and the opening
  **Mulligan** appear as selectable choice blocks; the opponent's AI turn replays into the
  status line, then control returns to you.
- [ ] **(non-TTY / line REPL)** Piping the client (`recollect | cat`, or `--json` /
  `headless`) uses the **line REPL** unchanged: it prints the board + numbered menu and
  waits for a typed line. At a real prompt the line mode also serves `hotseat` / `watch` /
  online. You can act **two ways**: type a **number** from the "Legal plays" menu,
  **or** type a **verb** (`p`/`o`/`m`/`g`/`r`/`v`/`rc`/`end`, tiles as `c3` or an index) —
  verbs work in **both** local and online (one shared grammar). `q` pauses out (the printed
  seed resumes it).
- [ ] **(TTY)** **Glimpse** (the `g` move) re-prompts **twice**: first the
  **burn** menu (one `Burn <card> to glimpse` per hand card), then — on the peeked top
  card — the **keep-or-bottom** menu (`Keep <card> on top` / `Bottom <card> for +1 anima`).
  It is **absent** from the menu when your hand or deck is empty.
- [ ] **(TTY)** **Mulligan** is offered **at the opening only** — a `Mulligan …` entry in
  the round-1 menu (and the `mull` verb) — and is **gone** once you've played, Glimpsed, or
  spent Anima. Taking it redraws a fresh hand and bottoms one card; the opponent is told
  only *that* you mulliganed.
- [ ] **(TTY)** **Inspect** renders: `i 0` (a hand card) and `i c3` (a board tile) print
  the card panel — stats, keywords, rules text, and the reach grid (★ the card, ● each
  tile it threatens) — without consuming your turn.
- [ ] **(TTY)** The **result** prints at Nightfall: the final board under a `— NIGHTFALL —`
  line with the score and who the match belongs to (a draw reads *the Memory keeps both
  names*); a `watch` (two-AI) game also plays to that screen.
- [ ] **Redaction by eye:** across a whole local match you never see the opponent's hand
  cards or deck order — only public board state and the truthful counts (`hand N · deck M`).
- [ ] **Online parity (TTY):** the same number/verb input drives a networked match against a
  running server; a dropped connection reconnects and resumes the same match.
- [ ] **Absence forfeit:** in a two-human online match, one player closing their client and
  NOT returning within the grace forfeits — the other player's view finishes as their win
  (and the seed reveal lands). Reconnecting before the grace lapses cancels it (no forfeit).
  Drive it fast with a short `?abandon_grace_secs=N` on match creation (default is 120s).

## Static site
- [ ] Every page renders and is responsive on phone + large screen: landing, rules, lore, play, cards, feedback.
- [ ] The card catalog's filter/search (by kind, rarity, resonance, cost, and name) works; with JavaScript disabled it still shows the full list.
- [ ] Keyboard navigation works across the nav and the catalog; the skip-link, heading order, and color contrast hold up.
- [ ] "Launch the game" opens the live client; "Back to site" returns to the landing page.

## Brand / visual language
- [ ] The palette and wordmark are consistent between the site and the in-game client; the fading-Memory feel reads on both.

## Observability
- [ ] Local (`make up`): with `OTEL_EXPORTER_OTLP_ENDPOINT` set and the Grafana stack running, traces, metrics, and logs show up in Grafana (`:3000`); with the endpoint unset, the server still runs and emits JSON logs only.
- [ ] Deploy host — **reach Grafana via Cloudflare Access**: open `https://grafana.recollect-tcg.com`, get the Cloudflare Access login, authenticate as the allowed `maintainerEmail` (one-time PIN or IdP), and land on Grafana. Confirm a **non-allowed** email is **denied** (Access blocks it; Grafana is never publicly reachable).
- [ ] Deploy host — **dashboards-as-code present**: Dashboards → Recollect shows the three provisioned dashboards (RED service, game-design, host/box), read-only, with live data (play a few matches to populate command/match/ws panels; P1/P2 winrate + draw rate populate from finished matches).
- [ ] Deploy host — **host/box dashboard** shows real CPU/mem/**swap**/disk from `node-exporter`; the swap panel is non-empty (the box uses the 4 GB swap).
- [ ] Deploy host — **retention/storage**: `/data/observability` exists on the durable volume and stays bounded (~1–2 GB) over time; a box recreate (`gitRef` bump) preserves the dashboards + history.
- [ ] Deploy host — **CloudWatch out-of-band**: the SNS email subscription is **confirmed** (clicked the confirmation email); `recollect-*` alarms exist in CloudWatch and the `Recollect/Host` custom metrics (mem/swap/disk) appear; an induced condition (e.g. fill a disk in a test) pages the email.
- [ ] Deploy host — **break-glass**: `make deploy-ssm` reaches the box keylessly (no SSH); the SSM port-forward fallback for Grafana works if Access is misconfigured.

## Build artifacts
- [ ] `make site` reports the wasm bundle under the 3 MB gzipped budget.
- [ ] `cargo build --profile dist -p recollect-server` produces the small shipping binary (~3–4 MB).
