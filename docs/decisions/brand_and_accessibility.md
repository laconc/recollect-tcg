# Brand, visual language & accessibility (frontend + website)

The foundation shared by the wasm client, the future native shells, **and** the
marketing website, so the look is one language across web / iOS / Android /
desktop. The renderer is Rust + `wgpu` (one `scene.rs` → primitives → WebGPU/Metal/
Vulkan), so the *visuals* are defined once; this doc names them and sets the
accessibility + responsive bars the website must also clear.

## Visual identity — "paper & ink, a fading Memory"
The board is a page; play is ink on it; the Dusk is the page going dark. The palette
already lives in `recollect-web/src/scene.rs` — this is its canonical form (with hex
for the website's CSS custom properties):

| Token | rgba (renderer) | hex (web CSS) | use |
|---|---|---|---|
| `PAPER` | 0.96, 0.94, 0.89 | `#F5F0E3` | the page / board ground |
| `NIGHT` | 0.09, 0.09, 0.12 @ .92 | `#17171F` | ink text, the Dusk/Nightfall overlay |
| `SEAT_A` (Lorekeepers) | 0.18, 0.36, 0.62 | `#2E5C9E` | ink-blue |
| `SEAT_B` (the Solace) | 0.66, 0.24, 0.20 | `#A83D33` | ink-red |
| accents | gold / green / grey | — | resolve pips / growth / hidden·fabrication |

Typography: the in-canvas glyphs (`font.rs`) and the website's CSS must use the same
family (a humanist/serif "storybook" face) so headings and the board agree.

## Accessibility — WCAG 2.1 **AA** target (legal + a genuinely good experience)
A `wgpu` canvas is a single opaque element to assistive tech, so visuals alone are
not enough. Required:
- **Semantic DOM mirror.** An off-screen/overlay DOM reflects game state for screen
  readers while the canvas draws: the board as `role="grid"` with per-tile
  `aria-label`s, the hand as `<button>`s, score/phase/last-event as `aria-live`
  regions. Driven by the same `PlayerView` the scene renders.
- **Keyboard play.** Tab/arrow to move focus across tiles + hand, Enter/Space to act,
  Esc to cancel — never mouse/touch-only. Visible focus rings.
- **Contrast.** Every text/UI pair meets AA (4.5:1 text, 3:1 large/UI). PAPER↔NIGHT
  passes easily; SEAT inks on PAPER must be verified (add a palette-contrast test).
- **No color-only signaling.** Seat = ink color **and** a shape/label; states get an
  icon or text, not just a hue (colour-blind safe).
- **Motion & audio.** Honor `prefers-reduced-motion`; any audio has a visual cue and
  is never required to play.
- **Shell.** `lang` + responsive viewport are present; add a skip link, focus styles,
  and a no-JS/loading message.

## Responsive — mobile **and** larger screens
- Canvas scales to the viewport, `devicePixelRatio`-aware (capped at 2×), square-fit
  board; the wgpu backing buffer is re-fit to the displayed CSS size × DPR on
  resize/rotate (a `ResizeObserver` + `resize`/`orientationchange` listeners drive
  `WebRenderer::resize`), so the board stays crisp from a 320px phone to a 4K desktop.
- **Touch** — tap to act sized to a ≥44px target on coarse pointers, alongside mouse
  + keyboard: tap a spirit/hand-card → legal targets glow → tap one to move/play (or
  drag); a Fading spirit's own tile lights as an Evolve (rescue) target.
- Breakpoints (`recollect-web/play.css`): phone portrait = single column with a wider
  board; landscape phone = board floated beside the controls; tablet/desktop = wider
  control band, board capped. No horizontal scroll at any width. The shell layout is
  CSS-driven; the website mirrors the same breakpoints.

## Tests
- The render-contract tests over `scene`/`font`/`lib`.
- A **palette-contrast** test (each text/UI pair ≥ AA), and **a11y-mirror** assertions
  (every tile/card exposes a label; focus order is stable).
