#!/usr/bin/env python3
"""Strict validation/lint for `data/cards.toml` — the card source-of-truth.

A REQUIRED field that is missing or malformed is a HARD ERROR, never a silent skip:
this is the guard against the bug class that let cards ship incomplete (the Deepenings
without lore; dead effects on non-deck-playable cards). `gen_catalog.py` runs this
before it generates, and `make catalog-check` runs it in CI; it is also runnable
standalone (`python3 tools/validate_cards.py`).

What it checks, per `[[card]]`:
  • the identity + game-state fields are present and well-typed
    (key/name/kind/rarity/resonance + attack/defense/hp + reach, imprints, keywords, rules);
  • `cost` is present for every non-evolution card and ABSENT for evolution forms
    (form cost is derived from stats);
  • enum-valued fields take only known values (kind, rarity, resonance, reach, keywords);
  • keys and names are unique across the set;
  • the effect IR is well-formed (each `[[card.effect]]` has trigger/condition and
    ≥1 well-formed `[[card.effect.clause]]` with selector/effect/duration);
  • evolution wiring is consistent: a form's `[card.evolution].base` resolves to a real
    card, its `tier` equals its rarity, and a base's `split` lists only known tiers;
  • lore/physical, when present, are non-empty and carry a known source tag.
"""
import os
import sys
import tomllib

_ROOT = os.path.join(os.path.dirname(__file__), "..")
_CARDS_TOML = os.path.join(_ROOT, "app", "crates", "recollect-core", "data", "cards.toml")

KINDS = {
    "Spirit", "Kindred", "Evolution", "Ritual", "Bond", "Landmark", "Fabrication",
    "Caller", "Unwritten", "IllIntent", "Unwriting", "Foundling",
}
RARITIES = {"C", "U", "R", "L", "Kindred", "Primal", "Fabled", "Solace", "G", "W", "F"}
RESONANCES = {"Wonder", "Fear", "Sorrow", "Harmony", "Fury", "Resolve", "Neutral"}
REACHES = {"Cross", "Slant", "Lance", "Wide", "Spire", "Veil", "Burst"}
KEYWORDS = {"Arcane", "Warded", "Mobile", "Steadfast", "Relentless", "Lurk"}
TIERS = {"Primal", "Fabled"}
SOURCES = {"§3", "§9"}


def _is_int(v):
    return isinstance(v, int) and not isinstance(v, bool)


def validate(cards):
    """Return a list of human-readable error strings (empty ⇒ valid)."""
    errs = []
    seen_keys, seen_names = {}, {}
    names = {c.get("name") for c in cards if isinstance(c.get("name"), str)}

    for i, c in enumerate(cards):
        where = c.get("key") or c.get("name") or f"#{i}"

        # identity + required strings
        for f in ("key", "name", "kind", "rarity", "resonance", "reach", "rules"):
            if not isinstance(c.get(f), str) or c.get(f) == "" and f != "rules":
                errs.append(f"{where}: missing/empty required string field `{f}`")
        # rules may legitimately be empty (vanilla spirit), but must exist + be a str
        if not isinstance(c.get("rules"), str):
            errs.append(f"{where}: `rules` must be a string (use \"\" for vanilla)")

        # stats
        for f in ("attack", "defense", "hp"):
            if not _is_int(c.get(f)) or c.get(f) < 0:
                errs.append(f"{where}: `{f}` must be a non-negative integer")

        # cost: present for non-evolution, absent (derived) for forms
        kind = c.get("kind")
        if kind == "Evolution":
            if "cost" in c:
                errs.append(f"{where}: evolution forms must OMIT `cost` (it is derived from stats)")
        else:
            if not _is_int(c.get("cost")) or c.get("cost") < 0:
                errs.append(f"{where}: non-evolution card needs a non-negative integer `cost`")

        # enums
        if kind not in KINDS:
            errs.append(f"{where}: unknown kind {kind!r}")
        if c.get("rarity") not in RARITIES:
            errs.append(f"{where}: unknown rarity {c.get('rarity')!r}")
        if c.get("resonance") not in RESONANCES:
            errs.append(f"{where}: unknown resonance {c.get('resonance')!r}")
        if c.get("reach") not in REACHES:
            errs.append(f"{where}: unknown reach {c.get('reach')!r}")

        # imprints / keywords are string arrays; keywords from the known set
        for f in ("imprints", "keywords"):
            v = c.get(f, [])
            if not isinstance(v, list) or not all(isinstance(x, str) for x in v):
                errs.append(f"{where}: `{f}` must be an array of strings")
        for kw in c.get("keywords", []):
            if kw not in KEYWORDS:
                errs.append(f"{where}: unknown keyword {kw!r}")

        # uniqueness
        if c.get("key") in seen_keys:
            errs.append(f"{where}: duplicate key (also {seen_keys[c['key']]})")
        elif isinstance(c.get("key"), str):
            seen_keys[c["key"]] = where
        if c.get("name") in seen_names:
            errs.append(f"{where}: duplicate name (also {seen_names[c['name']]})")
        elif isinstance(c.get("name"), str):
            seen_names[c["name"]] = where

        # lore / physical (optional, but non-empty + tagged when present)
        for body, src in (("lore", "lore_source"), ("physical", "physical_source")):
            if body in c:
                if not isinstance(c[body], str) or not c[body].strip():
                    errs.append(f"{where}: `{body}` present but empty")
                if c.get(src) not in SOURCES:
                    errs.append(f"{where}: `{body}` needs a `{src}` in {sorted(SOURCES)}")

        # evolution block
        evo = c.get("evolution")
        if evo is not None:
            if not isinstance(evo, dict):
                errs.append(f"{where}: `[card.evolution]` must be a table")
            else:
                if "base" in evo:
                    if evo["base"] not in names:
                        errs.append(f"{where}: evolution.base {evo['base']!r} is not a card name")
                    if evo.get("tier") != c.get("rarity"):
                        errs.append(
                            f"{where}: evolution.tier {evo.get('tier')!r} must equal the form's rarity {c.get('rarity')!r}"
                        )
                if "split" in evo:
                    if not isinstance(evo["split"], list) or any(t not in TIERS for t in evo["split"]):
                        errs.append(f"{where}: evolution.split must list only {sorted(TIERS)}")
        # a non-base evolution FORM must declare its base
        if kind == "Evolution" and not (isinstance(evo, dict) and "base" in evo):
            errs.append(f"{where}: evolution form must declare `[card.evolution].base`")

        # effect IR
        for j, sp in enumerate(c.get("effect", [])):
            sw = f"{where} effect[{j}]"
            if not isinstance(sp, dict):
                errs.append(f"{sw}: must be a table")
                continue
            if not isinstance(sp.get("trigger"), str):
                errs.append(f"{sw}: missing `trigger`")
            if "condition" not in sp:
                errs.append(f"{sw}: missing `condition`")
            clauses = sp.get("clause", [])
            if not clauses:
                errs.append(f"{sw}: needs at least one `[[card.effect.clause]]`")
            for k, cl in enumerate(clauses):
                cw = f"{sw} clause[{k}]"
                if not isinstance(cl, dict):
                    errs.append(f"{cw}: must be a table")
                    continue
                if "selector" not in cl:
                    errs.append(f"{cw}: missing `selector`")
                if "effect" not in cl:
                    errs.append(f"{cw}: missing `effect`")
                if not isinstance(cl.get("duration"), str):
                    errs.append(f"{cw}: missing `duration`")
    return errs


def load_and_validate(path=_CARDS_TOML):
    with open(path, "rb") as f:
        cards = tomllib.load(f)["card"]
    errs = validate(cards)
    return cards, errs


if __name__ == "__main__":
    path = sys.argv[1] if len(sys.argv) > 1 else _CARDS_TOML
    cards, errs = load_and_validate(path)
    if errs:
        print(f"cards.toml INVALID — {len(errs)} error(s):", file=sys.stderr)
        for e in errs[:200]:
            print("  -", e, file=sys.stderr)
        sys.exit(1)
    print(f"cards.toml OK — {len(cards)} cards, all required fields present and well-formed.")
