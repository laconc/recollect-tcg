#!/usr/bin/env python3
"""Shared lore extraction — the single source for both site generators.

The card lore (and physical) is authored in `data/cards.toml` (THE source of card
truth) — each `[[card]]` carries a `lore` / `physical` multi-line string and a
`lore_source` / `physical_source` tag (`§3` for a fully-realized exemplar, `§9` for
a "cards completed" entry; richest already chosen at migration). Cards without
authored prose (the procedural Solace fill + the summoned tokens, whose character
lives in their summoner's entry) simply omit `lore` — callers MUST treat the keyset
as authoritative so the site never links to a lore anchor that isn't there. The lore
page renders these keys as anchored prose; the cards page only shows a "Read its
lore" link for a key this module yields. One source, one keyset, no drift.

`load_lore(catalog)` → { key: {"name", "key", "resonance", "kind", "rarity",
"lore", "source"} }, where `source` is "§3" or "§9".
"""
import os
import pathlib
import tomllib

_ROOT = pathlib.Path(__file__).resolve().parents[1]
_CARDS_TOML = _ROOT / "app" / "crates" / "recollect-core" / "data" / "cards.toml"


def _load_cards():
    with open(_CARDS_TOML, "rb") as f:
        return tomllib.load(f)["card"]


def load_lore(catalog=None):
    """Build { key: record } for every card with authored lore prose.

    `catalog` is accepted (and used to bound the keyset to the live catalog) for
    backward compatibility with the site generators, which pass the parsed
    catalog.json; the lore TEXT comes from `cards.toml`. Both are generated from
    the same source, so the keyset agrees.
    """
    allowed = None
    if catalog is not None:
        if isinstance(catalog, (str, pathlib.Path)):
            import json

            catalog = json.loads(pathlib.Path(catalog).read_text())
        allowed = {c["key"] for c in catalog}

    out = {}
    for c in _load_cards():
        if not c.get("lore"):
            continue
        if allowed is not None and c["key"] not in allowed:
            continue
        out[c["key"]] = {
            "name": c["name"],
            "key": c["key"],
            "resonance": c["resonance"],
            "kind": c["kind"],
            "rarity": c["rarity"],
            "lore": c["lore"],
            "source": c.get("lore_source", "§9"),
        }
    return out


def load_physical(catalog=None):
    """Companion to `load_lore` for the card's `physical` (art-direction) prose.
    Same shape, `physical`/`physical_source` instead of lore."""
    allowed = None
    if catalog is not None:
        if isinstance(catalog, (str, pathlib.Path)):
            import json

            catalog = json.loads(pathlib.Path(catalog).read_text())
        allowed = {c["key"] for c in catalog}

    out = {}
    for c in _load_cards():
        if not c.get("physical"):
            continue
        if allowed is not None and c["key"] not in allowed:
            continue
        out[c["key"]] = {
            "name": c["name"],
            "key": c["key"],
            "physical": c["physical"],
            "source": c.get("physical_source", "§9"),
        }
    return out


if __name__ == "__main__":  # quick coverage probe: `python3 tools/lore_extract.py`
    import json

    cat = json.loads((_ROOT / "app/crates/recollect-core/data/catalog.json").read_text())
    lore = load_lore(cat)
    have = len(lore)
    missing = [c["name"] for c in cat if c["key"] not in lore]
    print(f"authored lore: {have} / {len(cat)} cards  ({len(missing)} without prose)")
    from collections import Counter

    print("sources:", Counter(r["source"] for r in lore.values()))
    if missing:
        print("without authored lore (link omitted on the cards page):")
        for n in missing:
            print("  ", n)
