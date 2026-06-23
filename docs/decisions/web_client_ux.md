# Web client UX — the canvas owns the whole game (design-of-record)

The play client is **one wgpu canvas that draws the entire game** — board, hand,
cards, HUD, every action affordance, the animations, the paced opponent-action
replay, the Dusk/Nightfall set-pieces, and the result screen. All of it is the
Rust scene/render layer (`recollect-web/src/{scene,render}.rs`), so all of it is
**shared with the future native shells** — the same wgpu compiled native
(D-24/D-25). The HTML page is a thin shell around that canvas.

In-canvas affordances plus the accessibility tree (§a11y) carry every action;
there is no parallel DOM move-list. This is the "paper & ink" client: the page
*is* the game surface, and the chrome gets out of the way.

It is **signature-tier** end to end — everything animated, solid, polished, and
obvious. A player should pick up every cue instantly and never wonder what is
interactive.

## Why the canvas owns everything

- **One renderer, three platforms.** The board already lives in `scene.rs` →
  wgpu. Pushing the hand, HUD, buttons, and set-pieces into the same scene means
  iOS and Android (UniFFI over the same core, D-25) inherit the entire game UI for
  free, instead of re-authoring it in SwiftUI/Compose. The DOM was a second UI we
  would have had to port; the canvas is the one we keep.
- **Coherence.** A single drawing surface with one easing/typography/motion
  language reads as one crafted thing — the chrome can't drift from the board.
- **The a11y tree is the honest seam.** A canvas is opaque to assistive tech, so
  we owe it a parallel accessible tree regardless (§a11y, `brand_and_accessibility.md`).
  That tree — not a duplicate set of HTML buttons — is the accessible path. One
  visual surface, one accessible mirror.

## Page shell (the only HTML left)

- **Top nav bar:** Logo · Play · Guide · Cards · Rules · Lore · **Options**. The raw play
  settings (sound · reduced-motion · animation-speed) are collapsed behind the single **Options**
  disclosure (a dropdown panel, Esc/click-out to close) so they don't clutter the bar. Semantic,
  accessible HTML.
- **The canvas mount:** a full-bleed, `devicePixelRatio`-aware canvas below the
  nav, scaling to the viewport. **Portrait-first** — the one-hand-portrait
  commitment (design §16). Landscape and desktop adapt.
- **The input bridge:** forward pointer + keyboard events into wasm.
- **The virtual a11y tree:** the ARIA mirror of the canvas (§a11y).

Everything else a player sees is drawn by Rust.

## In-canvas layout (portrait)

Top to bottom:

- **Opponent strip** (top): name, score (including the off-board **erasure
  tally** when the opponent is the Solace), and the opponent's hand as **face-down
  backs** (count only — redaction holds; you never see their cards).
- **Board** (center, the hero): the 5×5 page (6×6 in 2v2), square, the largest
  element.
- **HUD:** your score · your **Anima** · the round / clock, carrying the **Dusk**
  and **Nightfall** indicator (the binding strip rendered as a clock face,
  design §5).
- **Hand** (bottom tray): your cards as **real cards** — the full card face using
  the placeholder template (the same art pipeline the catalog page uses),
  not abbreviated chips.
- **Control buttons:** **End Turn** (primary — nothing auto-ends, design §5) and **Glimpse** — the
  only two global buttons; everything else is contextual. They live in their **own control lane,
  clear of the play grid** (so a tap on the board is never ambiguous): a dedicated right **rail**
  beside the board on a wide viewport, the right of the **HUD bar** on a phone.

Landscape and desktop reflow the same elements (board beside controls, wider
bands) — the layout is responsive, the content identical.

## Interaction — affordances replace the move list

The labeled-move list is gone; the board carries the affordances directly.

- **A quiet dot** sits on any piece that has an available action. **A glyph** marks
  a **Fading** spirit that can evolve.
- **Select a piece** → its legal targets highlight. Engageable targets read
  **bright** (actionable); the piece's own reach reads **soft** (passive, on
  inspect — see below). **Tap a target to act.**
- **Two equally-valid gestures**, both complete: **tap-select-then-target** *and*
  **drag-from-piece-to-target** (the literal drag gesture, on mouse and touch).
- **Hand:** tap a card → it **lifts** and the legal tiles **glow** → tap a tile to
  place it. (Drag works here too.)
- **Contextual actions live on the piece**, not in a global bar: **Evolve** and
  **Reclaim** appear on the eligible piece. Only **End Turn** and **Glimpse** float
  as global buttons.

## Inspect — high-frequency, so make it obvious

Inspecting a card's full detail is the most common non-move thing a player does;
the interaction must be unmistakable and never collide with selecting-to-act.

- **Mouse: hover = inspect.** **Touch: long-press = inspect** (there is no hover on
  touch). A floating **inspect panel anchored to the card** shows full stats, the
  reach grid, keywords, and rules text.
- **Two intents, two moments, no clash:**
  - **Reach shows on INSPECT** — soft, passive, "what could this threaten."
  - **Engageable targets show on SELECT** — bright, actionable, "what can I hit
    right now."
  This matches the design language already in place: a *held* tile renders with
  **no** reach overlay (design §5), so reach is an inspect-time affordance, never a
  selection-time one. Inspecting reveals possibility; selecting commits to action.

## Phases — Flow / Main / Fade

The turn moves through **Flow → Main → Fade** (Main is the player-facing name for
the design's acting phase — design §5; **Fade is at the turn-END**, after Main). The
client folds them in:

- **Flow** (draw + income; Release if over hand cap) is a quick turn-START beat —
  animated, brief, not an interactive choice in the common case.
- **Main** is the interactive heart: **Play / Call / Move / Glimpse / Evolve /
  Devolve / End Turn**. There is **no fixed action count** — **Anima is the limiter**
  (design §5), surfaced in the HUD.
- **Fade** (dissolve) is the turn-END beat: a spirit **banished in combat** stood
  Faded through this Main (the one window to **evolve or devolve** it — see below); if
  redeemed by neither, it dissolves now, laying the banisher's impression. **The Dusk
  is separate** — it sweeps the empty rim *instantly* at the round-8 contraction, not
  as a Fade event.

**Evolve and Devolve happen in Main.** Evolution is *playing a form card from hand
onto a base*; **devolution** (the §5 rescue) is *playing a base card from hand onto a
standing-Faded form* to recede it a tier (the Lorekeeper **reverts**, the Solace
**recedes**). A spirit banished in combat stands Faded through Main as the window for
both — so a player can **evolve↔devolve cycle** it across the §0.5 windows. The
**standing-Faded** state renders distinctly (a warm amber lamplit rescue glow + an
amber pip, brighter than an unrecoverable fade — "this one can be saved this turn"),
and the recede affordance is a **downward amber chevron** (evolve is an upward green
chevron — ascend vs recede) on both the faded form and the base card, with its own
actionable a11y node announcing the recede in the faction's word.

## Opponent turns — watched and paced

The opponent's turn is **replayed action-by-action through a pacing queue**
(~1 second per action). Each action: animated, its affected tile highlighted, a
subtle caption naming what happened. On an Unwriting the **erasure tally counts
up**. Reference feel: Hearthstone — you watch the opponent play, you don't get a
wall of "it did 6 things."

- **No per-action fast-forward.** Pacing is governed by one global
  **animation-speed** setting (normal / fast) in the nav settings.
- **The same replay drives spectating.** A spectator view, and each seat's view in
  2v2, render any seat's turn through the identical paced replay.

## Announcements — all subtle, all animated (signature-tier)

Every beat gets a quiet, animated announcement; these double as the a11y
live-region text (§a11y), so the screen-reader narration and the visual flourish
are one source:

- **Match start:** who opens, and **why** if a character's initiative tipped the
  seeded toss — and we **always** announce the first player (design §5, "Who opens
  the telling").
- **Phase beats** (Flow / Main / Fade).
- **Key events:** banish, evolve, **devolve** (*revert* / *recede*), **Call**,
  **Glimpse**, **Dusk** ("Dusk falls"), **Nightfall**, and **each opponent action**
  as it replays.
- **The verdict.**

All in the game's register — *banished*, never killed; only the Solace *Unwrites*.

## Mulligan

At match start the client shows the opening hand with **Mulligan** (redraw once)
and **Keep** (design §5 grants one mulligan). The announcement states the **fact**,
never the cards — e.g. "The Solace mulligans" — so redaction holds.

## Result screen (in-canvas, shared with mobile)

Drawn in the canvas like everything else, so mobile inherits it:

- **The verdict in the game's voice** — *the Memory keeps [winner]* / *forgotten* /
  *both kept* (a draw).
- **Score breakdown** (board points, and the Solace's erasure tally folded in).
- **Actions:** Rematch · New opponent · Back to site.

It **adapts to every mode**: 1v1-vs-bot, **2v2** (team vs team), and **PvP** (the
human-vs-human verdict, over the `online` host/join flow).

**The Souvenir** — a shareable artifact (final board + verdict + score +
characters + seed hash, provably fair via the journaled log and the commit-reveal
seed, design §8) — is a later-release feature, not a launch blocker.

## Accessibility (§a11y)

Non-negotiable, first-class (AGENTS.md invariant). The canvas is opaque to
assistive tech, so the client maintains a **virtual ARIA tree that mirrors the
canvas**: the board (per-tile), the hand, and the actions are **actionable
accessible elements**, with a **live region** carrying the announcements above.
The website chrome (nav, settings, Account) is semantic HTML. The bar is WCAG 2.1
AA, with the palette-contrast and a11y-mirror tests as the gate
(`brand_and_accessibility.md`).

## As built — the canvas client

The canvas client is realised across local 1v1, online 1v1 PvP, and 2v2, with the
implementation factored into pure, native-tested seams the JS bridge consumes:

- **The shell** (`recollect-web/src/shell.rs`) composes the in-canvas layout (HUD ·
  opponent strip · hand tray · the End-Turn/Glimpse controls) and the interaction
  layer (the action dots, the evolve chevron (upward green), the **devolve/recede
  chevron** (downward amber, on a standing-Faded form + its recede base — §5), the
  lifted hand-card, the inspect panel) around the board scene, drawn by
  `render.rs::draw_shell`. A spirit **banished in combat** renders in its rescuable
  **standing-Faded** state distinctly (a warm amber `Lamp`-layer rescue glow + an
  amber pip, and a *brighter* card than an unrecoverable Dusk fade — sourced from the
  view's `combat_faded` flag), so "this one can be saved this turn" reads at a glance.
  Two exports feed the bridge: `shell_regions` (the hit-test rects — one source of
  truth with the draw) and `build_a11y_tree` (the virtual ARIA tree — every affordance
  an actionable `A11yNode` firing the same legal command the canvas does, the standing-
  Faded state announced and the recede in the faction's word). The affordance lists are
  derived from the engine's legal moves, so the canvas and the a11y mirror can never
  offer a move the engine would reject. The recede is played **base-onto-form** (the
  mirror of evolve's form-onto-base): pick up the base card → the standing-Faded forms
  it can recede glow → tap one (tap-then-target *and* the keyboard two-activation both
  complete it).
- **The paced opponent replay** (vs-AI): the local engine applies the whole bot
  turn (determinism untouched) and `LocalGame::auto_play_turn_paced` returns an
  ordered stream of beats — one per discrete bot action, each with a caption, the
  a11y announcement (same text), the affected tiles, the Solace's erasure tally, and
  a self-contained redacted snapshot. The bridge paces through them ~1s apart; one
  global **animation-speed** setting governs the dwell and `prefers-reduced-motion`
  collapses it to near-instant. Every snapshot is `view_for(human)`, so the
  opponent's hand is never revealed.
- **The Dusk/Nightfall set-pieces** are drawn on-canvas: held tiles render
  **lamplit** in the board scene (`scene.rs`'s `Lamp` layer), and the round-boundary
  moment contracts and darkens the rim, lights the binding strip as a clock face, and
  fades up the seal — the same live-region line the replay narrates.
- **The in-canvas result screen** (`shell::build_result_screen`) draws the verdict in
  the game's voice, the score breakdown (with the Solace's erasure tally), and the
  Rematch / New opponent / Back-to-site actions, with its own a11y tree and announced
  verdict. It phrases by faction and mode (PvP relabels Rematch as an invite).
- **Online / 2v2** drive the same shell from the **server's redacted
  `PlayerView` / `TeamView` + its legal-move list** (the server is authoritative; the
  client never holds full state). Pure builders (`recollect-web/src/online.rs`,
  `shell_model_for_player_view` / `shell_model_for_team_view`) construct the same
  `ShellModel` the local shell draws — the running score from the view's tiles + the
  public erasure tally, the hand stat-blocks from the canon catalog, the affordances
  from the server's legal commands. The 2v2 adapter folds the opposing team into one
  **counts-only** opponent. **Redaction is by construction** (invariant 2): the
  online client only ever holds the redacted view, so the model cannot carry an
  opponent's hand or deck.

The look is **gallery-recorded deterministically**: a native, GPU-free preview
rasterizer (`recollect-web/examples/shell_preview.rs`, driven by
`tools/gen_gallery.sh`) rebuilds the exact `ShellScene`/`Scene` and rasterizes it on
the CPU — a faithful twin of the wgpu output (the same SDF rounded corners, soft drop
shadows, gradients, palette tokens, and EB Garamond atlas glyphs), reproducible with
no GPU and no seed lottery. The **live** server-backed socket play — a real
two-client match, the seq/ack round-trip, reconnection — is verified by hand
(`docs/manual_verification.md`); the headless suite covers the rendering, a11y, and
redaction that are testable without a server.
