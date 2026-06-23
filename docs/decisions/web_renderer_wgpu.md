# Web renderer: wgpu (tech_design §7)

The web client renders with the **wgpu ink renderer** — one deterministic Rust core
compiled to wasm, rendered by Rust/wgpu (WebGPU where available, WebGL2 fallback),
no JS game engine. The same renderer compiles native for the future UniFFI shells
(D-25), so web / iOS / Android / desktop share one renderer.

## Shape
- **`recollect-web/src/scene.rs`** — the renderer-agnostic *scene*: `PlayerView`/
  `TeamView` → flat draw primitives (quads + labels) in tile-grid coords. Pure,
  native-unit-tested; this is where the D-22 data-completeness lives (terrain,
  evolutions, stains, washes all become primitives). Survives any backend swap.
- **`recollect-web/src/render.rs`** — wasm-only wgpu backend (`WebRenderer`): one
  alpha-blended quad pipeline, async adapter/device, downlevel-WebGL2 limits. Draws
  the scene; nothing decides *what* to draw here. wgpu is a `cfg(target_arch="wasm32")`
  dependency, so native builds and `make test` never pull wgpu/naga.
- **Build**: `index.html` instantiates `WebRenderer.new(canvas)` and drives it from
  `LocalGame.view_json()`; `Trunk.toml` for `trunk serve` / `trunk build --release`.
  CI already compiles the wasm path (`cargo build -p recollect-web --target wasm32`).

## Properties

- **Text** rasterizes through the same quad pipeline as everything else: a glyph
  **atlas** (`atlas.rs`) bakes the bundled **EB Garamond** into a coverage texture at
  startup, so every label is real anti-aliased serif type and every shape samples the
  atlas's solid texel — one textured draw, no DOM text. (`wasm32-unknown-unknown` has
  no system fonts, so the face is bundled; a tiny bitmap `font` survives only as the
  pure width estimate the layout uses.)
- **Bundle budget.** `trunk build --release` (wasm-opt `-z`) lands the wasm bundle
  comfortably under the ≤3 MB gzipped budget — wgpu is not tight, with headroom for
  text and assets.

## Open

- **Browser / WebGL2-fallback verification** — can't be done headless; needs a real
  `trunk serve` run in a browser (`docs/manual_verification.md`).
- **2v2 board orientation** — a cosmetic flip so seat A's home renders at the bottom.
