#!/usr/bin/env python3
"""Generate site/cards.html — the browsable card catalog — from the embedded
catalog.json (the single card-truth source). Cards are emitted as semantic HTML
(a11y + SEO; no client framework, no big JSON fetch); a small vanilla-JS layer
*progressively enhances* search/filter — with JS off, every card still shows.

Run via `make site` (or directly). Re-run whenever the catalog changes.
"""
import html
import json
import pathlib

from lore_extract import load_lore

ROOT = pathlib.Path(__file__).resolve().parents[1]
DATA = ROOT / "app/crates/recollect-core/data"
CATALOG = DATA / "catalog.json"
# Evolution wiring (the same data the engine evolves from — never hand-authored here):
#   evolution_lines.json : form-name → base-name (the ascent each form lands on)
#   evolution_split.json : base-name → [tiers offered]  ("Primal", "Fabled", or both)
# Inverting `lines` gives base → its form(s); a form's catalog `rarity` ("Primal" /
# "Fabled") IS its tier, so a base offering two forms at one tier is an evolution
# MENU (a gentle/malign choice, or an apex fork). The cards page renders this as a
# per-card cross-navigation line — anchor links that work with JS off.
EVO_LINES = DATA / "evolution_lines.json"
EVO_SPLIT = DATA / "evolution_split.json"
OUT = ROOT / "site/cards.html"

# Card art (built by `make cards-images` → tools/cardpipe). The illustration path
# derives from the card's stable `key`; a master at assets/cards-src/<key>.png
# yields a per-key delivered set, otherwise the card shares one placeholder. The
# images live under site/img/cards/ so `make site` copies them into dist/.
MASTERS_DIR = ROOT / "assets/cards-src"
IMG_BASE = "img/cards"               # relative to site/cards.html
PLACEHOLDER_KEY = "_placeholder"     # the shared art-less stand-in (see cardpipe)
# Delivered widths (must match tools/cardpipe WIDTHS); the largest backs `src`.
IMG_WIDTHS = (512, 1024)
# 5:7 portrait at the canonical width — lets the browser reserve layout (no CLS).
IMG_W, IMG_H = 1024, 1434
# The card renders ~300–360 px wide on screen; one column on phones, up to three
# on wide viewports. `sizes` mirrors the .cards-grid breakpoints in brand.css.
IMG_SIZES = "(min-width: 60rem) 20rem, (min-width: 40rem) 45vw, 90vw"

COMBAT_KINDS = {"Spirit", "IllIntent"}
KEYWORDS = ("arcane", "warded", "mobile", "steadfast", "relentless", "lurk")


def esc(s):
    return html.escape(str(s))


# ── Evolution cross-navigation ───────────────────────────────────────────────
# Built once in main() and threaded through card_html via the module globals below.
# The card's own anchor is id="card-<key>"; an evolution link is href="#card-<key>".
# Forms are grouped by tier (a form's catalog `rarity` is its tier) so a multi-form
# tier renders as a menu ("Primal: A or B"); JS-off, every link is a live anchor.
BASE_TO_FORMS = {}   # base-name → [form-name, …]   (inverse of evolution_lines)
FORM_TO_BASE = {}    # form-name → base-name        (evolution_lines verbatim)
KEY_OF = {}          # card-name → stable `key` (the anchor + image stem)
RARITY_OF = {}       # card-name → rarity (a form's rarity is its evolution tier)
LORE_KEYS = set()    # keys with authored lore (→ a "Read its lore" link; see lore_extract)
# A form's tier is read from its rarity; this orders the menu Primal-then-Fabled.
TIER_ORDER = {"Primal": 0, "Fabled": 1}


def evo_link(name):
    """An in-page anchor to another card's tile, labelled with the card name."""
    return f'<a href="#card-{esc(KEY_OF[name])}">{esc(name)}</a>'


def evolution_html(c):
    """The per-card cross-navigation line: a base links DOWN to its form(s) (grouped
    by tier, menus shown as 'A or B'); a form links UP to its base. Empty for cards
    with no evolution wiring. Anchors only — fully functional with JS off."""
    name = c["name"]
    forms = BASE_TO_FORMS.get(name)
    if forms:
        # Group this base's forms by tier (Primal / Fabled); each tier is one phrase,
        # a menu when it has >1 form (the gentle/malign fork, or an apex choice).
        by_tier = {}
        for f in forms:
            by_tier.setdefault(RARITY_OF.get(f, ""), []).append(f)
        parts = []
        for tier in sorted(by_tier, key=lambda t: TIER_ORDER.get(t, 99)):
            links = " or ".join(evo_link(f) for f in sorted(by_tier[tier]))
            label = f'<span class="evo-tier">{esc(tier)}</span> ' if tier else ""
            parts.append(f"{label}{links}")
        body = " · ".join(parts)
        return (
            f'<p class="evo evo-up">'
            f'<span class="evo-label">Evolves into</span> {body}</p>'
        )
    base = FORM_TO_BASE.get(name)
    if base:
        tier = RARITY_OF.get(name, "")
        tier_label = f' <span class="evo-tier">{esc(tier)}</span>' if tier else ""
        return (
            f'<p class="evo evo-down">'
            f'<span class="evo-label">Evolves from</span> {evo_link(base)}{tier_label}</p>'
        )
    return ""


def image_key(c):
    """The delivered image stem for a card: its own `key` when a real master
    exists, else the shared placeholder. Path is derived from `key` — no
    catalog.json change (the design doc's source-of-truth rule)."""
    key = c["key"]
    if (MASTERS_DIR / f"{key}.png").exists():
        return key
    return PLACEHOLDER_KEY


def card_image_html(c):
    """A responsive, lazy <img> for the card illustration. `alt` is the card name
    (informative, player-facing — no agent/internal jargon); width/height reserve
    the 5:7 box so the grid doesn't shift as art streams in."""
    stem = image_key(c)
    srcset = ", ".join(f"{IMG_BASE}/{stem}-{w}.webp {w}w" for w in IMG_WIDTHS)
    src = f"{IMG_BASE}/{stem}-{IMG_WIDTHS[-1]}.webp"
    return (
        f'<img class="card-art" src="{src}" srcset="{srcset}" '
        f'sizes="{IMG_SIZES}" alt="{esc(c["name"])}" '
        f'loading="lazy" decoding="async" width="{IMG_W}" height="{IMG_H}" />'
    )


def card_html(c):
    name = esc(c["name"])
    kws = [k for k in KEYWORDS if c.get(k)]
    kw_html = (
        '<p class="keywords">'
        + " ".join(f'<span class="kw">{k}</span>' for k in kws)
        + "</p>"
        if kws
        else ""
    )
    stats = [f"Cost {c['cost']}"]
    if c["kind"] in COMBAT_KINDS or c.get("hp", 0):
        # The Atk/Def/HP shorthand the whole app uses (web cards, inspect, the CLI render)
        # — never A/D/H, and consistent casing across surfaces (web_client_ux.md §Hand).
        stats += [f"Atk {c['attack']}", f"Def {c['defense']}", f"HP {c['hp']}", f"Reach {c['reach']}"]
    evo = evolution_html(c)
    evo_block = f"\n      {evo}" if evo else ""
    # Cross-link to this card's lore entry on the Lore page (the prose lives there;
    # the catalog is the stat-line). Only cards with AUTHORED lore get the link
    # (LORE_KEYS, from the shared extractor) so the anchor always resolves — the
    # procedural Solace fill + summoned tokens have no prose, so no dangling link.
    lore = (
        f'\n      <p class="card-lore-link">'
        f'<a href="lore.html#lore-{esc(c["key"])}">Read its lore →</a></p>'
        if c["key"] in LORE_KEYS
        else ""
    )
    return f"""    <article class="card" id="card-{esc(c['key'])}" data-name="{esc(c['name']).lower()}" data-kind="{esc(c['kind'])}" data-rarity="{esc(c['rarity'])}" data-resonance="{esc(c['resonance'])}" data-cost="{c['cost']}">
      {card_image_html(c)}
      <h3>{name}</h3>
      <p class="badges"><span class="badge kind">{esc(c['kind'])}</span> <span class="badge">{esc(c['rarity'])}</span> <span class="badge res">{esc(c['resonance'])}</span></p>
      <p class="stats">{' · '.join(stats)}</p>
      <p class="rules">{esc(c['rules'])}</p>
{kw_html}{evo_block}{lore}
    </article>"""


def select(label, key, values):
    opts = "".join(f'<option value="{esc(v)}">{esc(v)}</option>' for v in values)
    return (
        f'<label>{label}<select data-filter="{key}">'
        f'<option value="">all</option>{opts}</select></label>'
    )


def main():
    cat = json.loads(CATALOG.read_text())
    # Wire the evolution cross-navigation (base ↔ form anchors). `lines` is
    # form → base; invert it for base → [forms]. We only keep names that exist in
    # the catalog (every one does today — asserted below — but stay defensive so a
    # data edit degrades to "no link" rather than a broken anchor).
    lines = json.loads(EVO_LINES.read_text())
    by_name = {c["name"]: c for c in cat}
    KEY_OF.update({c["name"]: c["key"] for c in cat})
    RARITY_OF.update({c["name"]: c["rarity"] for c in cat})
    for form, base in lines.items():
        if form in by_name and base in by_name:
            FORM_TO_BASE[form] = base
            BASE_TO_FORMS.setdefault(base, []).append(form)
    missing = [n for n in (*lines.keys(), *lines.values()) if n not in by_name]
    if missing:
        raise SystemExit(
            f"evolution data references {len(missing)} card(s) absent from the catalog: "
            f"{sorted(set(missing))[:5]} — fix cards.toml ([card.evolution] base/split)."
        )
    # Cross-check the tiers we derive (from each form's catalog `rarity`) against
    # evolution_split.json (base → tiers offered): the two data files must agree, or
    # the menu would label a form with a tier the base doesn't actually offer. A
    # mismatch is a data bug — fail the build loudly rather than ship a wrong label.
    split = json.loads(EVO_SPLIT.read_text())
    for base, forms in BASE_TO_FORMS.items():
        offered = set(split.get(base, []))
        derived = {RARITY_OF[f] for f in forms}
        if offered and derived - offered:
            raise SystemExit(
                f"evolution tier mismatch for base {base!r}: forms are {sorted(derived)} "
                f"but evolution_split offers {sorted(offered)} — reconcile the data."
            )
    # The lore cross-link is gated on authored prose (shared with the lore-page
    # generator), so a tile only links to a lore anchor that exists.
    LORE_KEYS.update(load_lore(cat).keys())
    kinds = sorted({c["kind"] for c in cat})
    rarities = sorted({c["rarity"] for c in cat})
    resonances = sorted({c["resonance"] for c in cat})
    costs = sorted({c["cost"] for c in cat})
    cards = "\n".join(card_html(c) for c in cat)
    page = f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Recollect — the card catalog</title>
  <meta name="description" content="Every spirit, ritual, bond, landmark and more — the things a Memory holds. Browse the catalog with stats, rules and keywords; filter and search." />
  <link rel="stylesheet" href="css/brand.css" />
  <style>
    /* Card illustration — owned by the cards-page generator (kept out of the
       shared brand.css). 5:7 portrait, paper-framed, reserving its box so the
       grid never shifts as art loads. Inherits the brand palette. */
    .card-art {{
      display: block; width: 100%; height: auto; aspect-ratio: 5 / 7;
      object-fit: cover; margin: 0 0 0.75em; border-radius: var(--radius);
      border: 1px solid var(--rule); background: #fbf8f0;
    }}
    /* Evolution cross-navigation + the lore cross-link — owned here (card-page
       specific). The evo line is quiet, prefixed with a small directional glyph:
       ↑ for a base ascending to its forms, ↓ for a form receding to its base. */
    .card .evo {{
      margin: 0.6em 0 0; font-size: 0.9rem; color: var(--ink-soft);
      max-width: none; border-top: 1px dashed var(--rule); padding-top: 0.5em;
    }}
    .card .evo-label {{
      font-size: 0.72rem; text-transform: uppercase; letter-spacing: 0.04em;
      color: var(--ink-soft); margin-right: 0.15em;
    }}
    .card .evo-up .evo-label::before  {{ content: "↑ "; color: var(--seat-a); font-weight: 700; }}
    .card .evo-down .evo-label::before {{ content: "↓ "; color: var(--gold); font-weight: 700; }}
    .card .evo-tier {{
      font-size: 0.68rem; text-transform: uppercase; letter-spacing: 0.04em;
      padding: 0.05em 0.4em; border-radius: 999px;
      border: 1px solid var(--rule); color: var(--ink-soft);
    }}
    .card .card-lore-link {{ margin: 0.5em 0 0; font-size: 0.85rem; max-width: none; }}
  </style>
</head>
<body>
  <a class="skip-link" href="#main">Skip to content</a>
  <header class="site-header">
    <div class="container">
      <a class="brand" href="index.html">Recollect</a>
      <nav class="site-nav" aria-label="Primary">
        <a href="play.html">Play</a>
        <a href="guide.html">Guide</a>
        <a href="cards.html" aria-current="page">Cards</a>
        <a href="rules.html">Rules</a>
        <a href="lore.html">Lore</a>
        <a href="contact.html">Contact</a>
      </nav>
    </div>
  </header>

  <main id="main" class="container">
    <h1>The card catalog</h1>
    <p class="note">Every spirit, ritual, bond and landmark a Memory can hold — the whole roster, to browse and search.</p>
    <form class="cards-toolbar" role="search" aria-label="Filter cards">
      <label>Search<input type="search" id="card-search" placeholder="card name…" autocomplete="off" /></label>
      {select("Kind", "kind", kinds)}
      {select("Rarity", "rarity", rarities)}
      {select("Resonance", "resonance", resonances)}
      {select("Cost", "cost", costs)}
    </form>
    <p class="note" aria-live="polite"><span id="card-count">{len(cat)} of {len(cat)}</span> cards shown</p>

    <div class="cards-grid">
{cards}
    </div>
  </main>

  <footer class="site-footer">
    <div class="container">
      <span>Recollect</span>
      <a href="rules.html">Rules</a>
      <a href="lore.html">Lore</a>
      <a href="contact.html">Contact</a>
      <span class="note">A fading Memory, told in paper &amp; ink.</span>
    </div>
  </footer>

  <!-- Filter logic in an EXTERNAL script (site/cards.js) so the site can ship a strict
       `script-src 'self'` CSP — no inline script. `defer` runs it after parse. -->
  <script src="cards.js" defer></script>
</body>
</html>
"""
    OUT.write_text(page)
    print(f"wrote {OUT.relative_to(ROOT)} — {len(cat)} cards "
          f"({len(kinds)} kinds, {len(rarities)} rarities, {len(resonances)} resonances)")


if __name__ == "__main__":
    main()
