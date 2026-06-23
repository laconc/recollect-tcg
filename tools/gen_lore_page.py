#!/usr/bin/env python3
"""Generate site/lore.html — the world & its lore — from the card source.

The page keeps its hand-authored *narrative* opening (the world, the two factions,
the six resonances) verbatim, then adds a TABLE OF CONTENTS and the per-card lore
SECTIONED BY RESONANCE (Wonder · Fear · Sorrow · Harmony · Fury · Resolve ·
Remnants & Neutral · the Solace · the Foundlings). Each section is anchored so the
TOC jumps to it; each card is an anchored entry (`id="lore-<key>"`) so the catalog
page can link straight to a card's lore — and each entry links back to the card's
catalog tile.

Lore prose comes from the SHARED extractor (`lore_extract.load_lore`) over
`app/crates/recollect-core/data/cards.toml` — the same keyset the cards page gates
its "Read its lore" link on, so the cross-links between the two pages always
resolve. Cards without authored prose (the procedural Solace fill + summoned tokens)
are simply omitted from the index, exactly as they are from the cards-page links.

Like the cards page, this is plain semantic HTML (a11y + SEO; no client framework)
copied into dist/ by `make site`. Re-run whenever the card source or catalog changes.
"""
import html
import json
import pathlib
import re

from lore_extract import load_lore

ROOT = pathlib.Path(__file__).resolve().parents[1]
CATALOG = ROOT / "app/crates/recollect-core/data/catalog.json"
OUT = ROOT / "site/lore.html"


def esc(s):
    return html.escape(str(s))


# The card source writes lore in light Markdown: *emphasis* spans, and (in the §3
# exemplars) multi-line dialogue. We escape first, then re-introduce just those two —
# italics and line breaks — so nothing in the prose can inject markup.
_EM = re.compile(r"\*(.+?)\*")


def lore_to_html(text):
    """Render a lore clause as inline HTML: HTML-escaped, `*word*` → <em>word</em>,
    and dialogue newlines → <br> so a transcript keeps its shape inside one <p>."""
    safe = esc(text)
    safe = _EM.sub(lambda m: f"<em>{m.group(1)}</em>", safe)
    return "<br>".join(line.strip() for line in safe.split("\n"))


# ── Sectioning — by resonance (the lore reading order) ───────────────────────
# Each section is (anchor-id, heading, blurb, predicate). A card lands in the FIRST
# section whose predicate matches, so the Solace + Foundlings are pulled out of
# Neutral before the Remnants catch-all. Within a section, cards keep catalog order
# (curve-ascending, the design's reading order), so evolutions sit by their kin.
def _is_solace(c):
    # The Unwritten and the Solace (Unwritten · IllIntent · Unwriting) — rarity "Solace".
    return c["rarity"] == "Solace"


def _is_foundling(c):
    return c["kind"] == "Foundling"


def _res(name):
    return lambda c: c["resonance"] == name and not _is_solace(c) and not _is_foundling(c)


def _is_remnant(c):
    # Whatever Neutral remains once the Solace + Foundlings are taken: the neutral
    # spellbook, landmarks, and the neutral evolution lines — the §9.7 "Remnants & Neutral".
    return c["resonance"] == "Neutral" and not _is_solace(c) and not _is_foundling(c)


SECTIONS = [
    ("wonder", "Wonder", "Storm and Wanderer — the things that look up, and ask.", _res("Wonder")),
    ("fear", "Fear", "Shade and Trickster — the held breath, the thing at the edge of sight.", _res("Fear")),
    ("sorrow", "Sorrow", "Tide and Shade — what is carried, and what is let go.", _res("Sorrow")),
    ("harmony", "Harmony", "Bloom and Song — the kept bond, the green that overruns.", _res("Harmony")),
    ("fury", "Fury", "Flame and Beast — momentum, and the price it asks.", _res("Fury")),
    ("resolve", "Resolve", "Stone and Guardian — what will not be moved.", _res("Resolve")),
    ("remnants", "Remnants &amp; Neutral", "The unaligned — neutral spellbook, landmarks, and the lines that belong to no one register.", _is_remnant),
    ("solace", "The Solace", "The Unwritten, and the Solace who lift them away — one voice, recovered from the Archive.", _is_solace),
    ("foundlings", "The Foundlings", "The Strays — the small, the lost, the not-yet-trusted, won one telling at a time.", _is_foundling),
]


# ── The hand-authored narrative opening (preserved verbatim; the page's voice) ──
# Kept as a template constant so the generator only ADDS the index below it — the
# crafted world-introduction prose is unchanged from the prior hand-authored page.
NARRATIVE = """      <h1>The world</h1>
      <p class="note">A first telling of the world. The factions, the Unwritten, the six
        resonances — each runs deeper than the page has room for. Below the telling, the
        <a href="#contents">whole roster's lore</a>, sectioned by resonance.</p>

      <h2 id="a-fading-memory">A fading Memory</h2>
      <p>Every match of Recollect is told inside a <strong>Memory</strong> — a remembered place,
        slipping. The board is its page. As the telling runs on, the page darkens at the edges
        (the <strong>Dusk</strong>), and the Memory contracts toward forgetting. To play is to
        decide what is worth keeping while there is still page to keep it on.</p>

      <h2 id="two-who-tell-it">Two who tell it</h2>
      <p>The <strong class="ink-a">Lorekeepers</strong> hold the Memory whole. They
        summon the loved and the remembered, and press their <strong>Impression</strong> into the
        ground so the telling cannot be denied.</p>
      <p>The <strong class="ink-b">Solace</strong> are not villains. They would let a
        painful thing <em>rest</em>. Where others banish a spirit, the Solace
        <strong>Unwrite</strong> it — lifted away gently, leaving no mark, as though it had never been
        written. They mean it as a kindness. Sometimes it is one.</p>

      <h2 id="what-lives-on-the-page">What lives on the page</h2>
      <p><strong>Spirits</strong> are the remembered things — some plain and near at hand, some
        Legends with names like sentences. They are never destroyed; defeated, they are
        <strong>banished</strong> back to the page, and may be told again.</p>
      <p>Not everything was meant to be written. The <strong>Unwritten</strong> are names-that-aren't —
        fragments, negations, the almost-said. And among them moves the <strong>ill intent</strong>:
        the menacing few that would not merely fade the Memory but <em>devour</em> it.</p>

      <h2 id="resonance">Resonance</h2>
      <p>Every spirit carries an emotional register — its <strong>resonance</strong> — and the
        registers answer to one another:</p>
      <ul>
        <li><strong>Wonder</strong> · <strong>Fear</strong> · <strong>Sorrow</strong></li>
        <li><strong>Harmony</strong> · <strong>Fury</strong> · <strong>Resolve</strong></li>
      </ul>
      <p>What you remember, and how you remember it, decides who answers your call.</p>

      <p><a class="btn btn-primary" href="play.html">Begin a telling</a> <a class="btn" href="cards.html">Meet the spirits</a></p>"""


def toc_html(sections_with_counts):
    """The table of contents — a nav list to every lore section, with its count.
    Real anchor links (work JS-off); labelled for screen readers."""
    items = "\n".join(
        f'        <li><a href="#{sid}">{heading}</a> '
        f'<span class="toc-count">{count}</span></li>'
        for sid, heading, count in sections_with_counts
    )
    return f"""      <nav class="lore-toc" aria-labelledby="contents">
        <h2 id="contents">The whole telling</h2>
        <p class="note">Every card's lore, by resonance. Jump to a section:</p>
        <ol class="toc-list">
{items}
        </ol>
      </nav>"""


def entry_html(rec):
    """One card's lore entry — an anchored article: the name (linking to its catalog
    tile), the lore prose, and a small register/tier line. `id="lore-<key>"` is the
    cross-link target the cards page uses."""
    key = rec["key"]
    name = esc(rec["name"])
    body = lore_to_html(rec["lore"])
    return f"""        <article class="lore-entry" id="lore-{esc(key)}">
          <h3><a href="cards.html#card-{esc(key)}">{name}</a></h3>
          <p class="lore-text">{body}</p>
        </article>"""


def section_html(sid, heading, blurb, records):
    entries = "\n".join(entry_html(r) for r in records)
    return f"""      <section class="lore-section" id="{sid}" aria-labelledby="{sid}-h">
        <h2 id="{sid}-h">{heading} <span class="lore-section-count">{len(records)}</span></h2>
        <p class="note">{blurb}</p>
        <a class="lore-top" href="#contents">↑ Back to contents</a>
{entries}
      </section>"""


def main():
    cat = json.loads(CATALOG.read_text())
    lore = load_lore(cat)  # key → record (authored prose only)

    # Bucket cards into their section, preserving catalog order within each.
    buckets = {sid: [] for sid, *_ in SECTIONS}
    for c in cat:
        rec = lore.get(c["key"])
        if not rec:
            continue  # no authored lore → not indexed (matches the cards-page link gating)
        for sid, _heading, _blurb, pred in SECTIONS:
            if pred(c):
                buckets[sid].append(rec)
                break

    counts = [(sid, heading, len(buckets[sid])) for sid, heading, _blurb, _pred in SECTIONS]
    toc = toc_html(counts)
    sections = "\n\n".join(
        section_html(sid, heading, blurb, buckets[sid])
        for sid, heading, blurb, _pred in SECTIONS
        if buckets[sid]
    )
    total = sum(len(v) for v in buckets.values())

    page = f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Recollect — the world &amp; its lore</title>
  <meta name="description" content="The lore of Recollect: a fading Memory, the Lorekeepers and the Solace, the six resonances — and every card's telling, sectioned by resonance with jump-to-card links." />
  <link rel="stylesheet" href="css/brand.css" />
  <style>
    /* Lore index — owned by the lore-page generator (kept out of shared brand.css).
       The paper & ink palette only; motion is disabled under prefers-reduced-motion
       by brand.css. The TOC and section headers are the new navigation. */
    .lore-toc {{
      border: 1px solid var(--rule); border-radius: var(--radius);
      background: #fbf8f0; padding: var(--space); margin: 2.5em 0 1em;
    }}
    .lore-toc h2 {{ margin-top: 0; }}
    .toc-list {{
      list-style: none; padding: 0; margin: 0; columns: 2; column-gap: var(--space);
    }}
    .toc-list li {{ break-inside: avoid; margin: 0 0 0.35em; max-width: none; }}
    .toc-count, .lore-section-count {{
      font-size: 0.72rem; color: var(--ink-soft); font-variant-numeric: tabular-nums;
    }}
    .lore-section-count {{
      font-size: 0.85rem; font-weight: 400; vertical-align: middle;
      padding: 0.05em 0.5em; border: 1px solid var(--rule); border-radius: 999px;
    }}
    .lore-section {{ margin-top: 2.5em; padding-top: 0.5em; border-top: 1px solid var(--rule); }}
    .lore-section > .note {{ margin-top: 0; }}
    .lore-top {{ display: inline-block; font-size: 0.8rem; margin: 0 0 0.5em; }}
    .lore-entry {{
      border-left: 3px solid var(--rule); padding: 0.1em 0 0.1em 1em; margin: 1.2em 0;
    }}
    .lore-entry h3 {{ margin: 0 0 0.2em; font-size: 1.1rem; }}
    .lore-entry h3 a {{ color: var(--night); text-decoration: none; }}
    .lore-entry h3 a:hover {{ color: var(--seat-b); text-decoration: underline; }}
    .lore-text {{ margin: 0; color: var(--ink-soft); }}
    /* The Solace section speaks in one voice — tint its left rule the Solace ink. */
    #solace .lore-entry {{ border-left-color: var(--seat-b); }}
    @media (max-width: 38rem) {{ .toc-list {{ columns: 1; }} }}
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
        <a href="cards.html">Cards</a>
        <a href="rules.html">Rules</a>
        <a href="lore.html" aria-current="page">Lore</a>
        <a href="contact.html">Contact</a>
      </nav>
    </div>
  </header>

  <main id="main">
    <article class="prose container">
{NARRATIVE}

{toc}
    </article>

    <div class="prose container">
{sections}
    </div>
  </main>

  <footer class="site-footer">
    <div class="container">
      <span>Recollect</span>
      <a href="rules.html">Rules in brief</a>
      <a href="cards.html">Card catalog</a>
      <span class="note">A fading Memory, told in paper &amp; ink.</span>
    </div>
  </footer>
</body>
</html>
"""
    OUT.write_text(page)
    n_sections = sum(1 for v in buckets.values() if v)
    print(
        f"wrote {OUT.relative_to(ROOT)} — {total} card lore entries across "
        f"{n_sections} sections"
    )


if __name__ == "__main__":
    main()
