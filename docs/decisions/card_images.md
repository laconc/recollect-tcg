# Card images (plan — for a later task)

Cards render as text today (the catalog + the wgpu client). This plans adding per-card
**art** (illustrations made in an external AI tool) to the catalog and the in-game cards,
in the paper&ink style. Captured now; built under its own task.

## Dimensions & format
- **Aspect**: portrait card **illustration**, **5:7** (the poker-card ratio) — the client/
  catalog frame supplies the border + text, so generate just the art.
- **Masters**: generate generously — **1024×1434** (5:7) PNG masters, crisp at 2× for the
  ~300–360 px on-screen card with headroom for print. Keep originals in `assets/cards-src/`.
- **Delivered**: **WebP** with a `srcset` (e.g. 512 w + 1024 w) + a JPEG/PNG fallback;
  target ~30–80 KB each so the catalog stays fast and the client honors its ≤3 MB gzip
  bundle budget.
- **Style**: a shared prompt template + the paper&ink palette so all 419 cohere; original,
  PII-free art.

## Naming & source of truth
- Filename = the stable card **`key`** (frozen slug): `img/cards/<key>.webp`. The path is
  **derived from `key`** — no `catalog.json` change needed. (Add an optional `image` field
  only if a card ever needs a non-default path.)

## Code changes
- **Catalog** (`tools/gen_cards_page.py`): emit
  `<img src="img/cards/<key>.webp" srcset="…512w …1024w" sizes="…" alt="<name>" loading="lazy" width height>`
  per card; `make site` copies `site/img/cards/`.
- **An optimization step** (a `make` target / tool): masters → WebP at the delivered widths
  (`cwebp` / `sharp` / a Rust image crate) + a check that every deck-playable card has art
  (or a documented placeholder).
- **The wgpu client** (`recollect-web` `scene.rs`/`render.rs`): load card textures (an atlas
  or per-card), keyed by `key`; the scene samples the art for card faces. Budget-aware:
  atlas + compressed/WebP, possibly lazy beyond the opening hand. The bigger change.
- **a11y**: each `<img>` `alt` = the card name (informative). The in-canvas art needs no alt
  (the labeled move buttons + the card-detail panel carry the semantics).
- **Gates**: re-measure the client gzip budget with art; a "every card has art" check.

## Sequencing
Catalog images first (cheap, high-impact — just the generator + the build) → then the
in-client textures (the bigger wgpu work) → re-measure the budget.
