# Website plan — recollect-tcg.com (the player-facing site)

The frontend site: a **marketing landing**, a **play page** (the wasm client), and a
**browsable card catalog**. This plan owns the *site + its build*; it defers to:
- `playtest_launch_plan.md` for **infra / accounts / branding** (§0 brand, §1 accounts,
  §4 single-region EC2 + Cloudflare CDN, §3 the page requirements);
- `brand_and_accessibility.md` for the **visual language + a11y + responsive bars**.

## Site map
- **`/` landing** — brand hero, a short "what is this game", a **Play now** CTA → `/play`,
  a link to `/cards`. Marketing assets (screenshots, the paper&ink identity).
- **`/play`** — the wasm client. Quick Play vs AI works today (greedy bot); vs-human via
  code-join (the launch plan's lean). How-to-play instructions on the page; a feedback
  button (→ form). Carries the accessible + responsive shell.
- **`/cards`** — the 419-card catalog: filter (kind / rarity / resonance / cost / keywords),
  search by name; each card shows stats + rules + keywords. **Build-generated** semantic
  HTML from `catalog.json` (SEO + a11y; no 144 KB client fetch).
- **`/rules`** — rules in brief, from the design-doc summary.
- **`/lore`** — the world &amp; story, drawn from the Lore Bible (`lore.md`). May grow to
  multiple pages (the Memory, the factions, the Unwritten / ill intent, the resonances).

## Match settings (hosting a match)
The `/play` host flow lets a player configure the match they're creating:
- **Mode** — 1v1 or 2v2.
- **Seat composition** — who fills each seat: human or bot. For **2v2** any mix is allowed
  (an ally may be a bot, an opponent a human); the host sets all four seats. The **faction
  fought against** (Lorekeepers vs the Solace) is part of the matchup.
- **Bot difficulty** — per bot seat (the `Difficulty` levels the bot exposes).
- A join **code** for the human seats (launch-plan lean over matchmaking).

Drives two pieces (phase 3): the `/play` host UI (a settings form → the create-match request)
and a **server change** — extend `create_match` to take a structured settings payload (mode,
per-seat human|bot+difficulty, faction); today it takes a single bot difficulty for vs-bot
1v1. The 2v2 lobby + `apply_slot` already fan out four seats — the settings choose which slots
are bot-driven vs code-join human.

## Tech stack (recommendation)
Static site, generated in the Rust toolchain, **no JS framework** (matches the "one Rust
core, no JS game engine" ethos):
- **Play page** = the existing `recollect-web` wasm, built with **Trunk** (already set up)
  + the accessible/responsive shell. No client rewrite.
- **Catalog** = generated at **build time** from `catalog.json` (the single card-truth
  source) into static semantic HTML — a small Rust `xtask` (or extend `tools/gen_catalog.py`).
  A tiny vanilla-JS layer *progressively enhances* filter/search (cards are in the HTML; it
  works with JS off).
- **Marketing pages** = static HTML/CSS sharing a layout + **brand CSS** (the palette from
  `brand_and_accessibility.md` as CSS custom properties — cohesive with the wgpu client).
- **Assembly** = a `make site` target: Trunk builds the wasm; the catalog generator emits
  `/cards` and the lore generator emits `/lore` (both from `cards.toml` — the catalog +
  the per-card lore, sectioned by resonance, cross-linked to the tiles); landing + play +
  cards + lore + assets are assembled into `dist/`. `make site-serve` for local preview.
- *Decision point:* a Rust SSG (**Zola**) vs hand-written + the source-driven generators.
  **Lean:** hand-written prose + the catalog/lore generators now; adopt Zola if marketing
  content grows. Either way the card content (catalog + lore) is build-generated from
  `cards.toml` — the single source of truth.

## Reuse
- The Trunk-built `recollect-web` wasm (play page) · the embedded `catalog.json` (catalog,
  build-generated) · the brand palette (→ CSS custom properties) · the launch plan's infra:
  static bundle (content-hashed) → **Cloudflare CDN**; online play → the EC2 `recollect-server`
  over WebSocket.

## Responsive + a11y (per brand_and_accessibility.md)
- Fluid layout + breakpoints (phone single-column → tablet/desktop wider); canvas scales
  `devicePixelRatio`-aware; touch + mouse + keyboard.
- Catalog + marketing are semantic HTML → inherently a11y + responsive.
- The play page carries the **a11y DOM-mirror** over the opaque wgpu canvas (role=grid board
  with per-tile aria-labels, hand as buttons, `aria-live` status, keyboard nav), WCAG-AA
  contrast (the locked, test-gated palette).

## Changes to existing code
- `recollect-web`: `index.html` → the accessible/responsive `/play` shell + the a11y
  DOM-mirror + responsive canvas.
- A catalog-page generator (Rust xtask / `gen_catalog.py` extension) → `/cards`.
- Brand CSS (palette custom properties) + site layout/nav; marketing copy + assets.
- `make site` + `make site-serve` targets.

## Sequencing
1. Brand CSS + layout/nav + the landing (static, quick).
2. The catalog generator + `/cards` (build-generated, a11y/SEO).
3. The `/play` shell + a11y DOM-mirror + responsive canvas (the substantial build).
4. `make site` assembly + Cloudflare deploy wiring.
5. Feedback form + rules-in-brief.

## Open decisions
- SSG (Zola) vs hand-written + catalog tool — **lean: hand-written now.**
- Catalog filter/search — build-time facets + progressive-enhancement JS (**lean**) vs a heavier client.
- Online play at launch — code-join (**launch-plan lean**) vs matchmaking.
