#!/usr/bin/env python3
"""Canon catalog generator: `data/cards.toml` (the single source of card truth) ->
`app/crates/recollect-core/data/{catalog,effects,evolution_lines,evolution_split,
card_keys,card_keywords}.json`. Run via `make catalog`; CI diffs the output
(`make catalog-check`) to catch source/code drift.

The cards were authored as Markdown prose until the TOML fold (one `[[card]]` block
per card holding ALL of stats + lore + physical + effects + evolution lines/menus +
keywords). The catalog and the runtime side-data are GENERATED from the TOML — never
hand-edit the JSON. Edit a `[[card]]` and re-run. The design prose + the §3 exemplar
commentary now live in `docs/cards_design.md`; this generator owns only the DATA.

Determinism of the output is the whole point: same TOML ⇒ byte-identical
`catalog.json` (the gate is a literal `diff`). Evolution-form cost is DERIVED from
stats by the same least-squares fit the Markdown pipeline used (so a re-stat rides
through); the lineage `evolves_to`/`evolves_from` edges are derived from each card's
`[card.evolution]` block.

The runtime side-data (`effects.json`, `evolution_{lines,split}.json`) feeds the
engine as HashMap/Vec inputs — only MEMBERSHIP matters, key/element order has no game
effect — so to keep those files byte-stable the generator preserves the committed
file's existing ordering and appends any genuinely new entry in catalog order.
"""
import json, os, sys, tomllib
from collections import Counter, OrderedDict

# Defaults resolve relative to this script (tools/), so the generator works from any
# CWD and after the Rust workspace moved under app/. `make` passes explicit args.
_ROOT = os.path.join(os.path.dirname(__file__), "..")
_DATA = os.path.join(_ROOT, "app", "crates", "recollect-core", "data")
SRC = sys.argv[1] if len(sys.argv) > 1 else os.path.join(_DATA, "cards.toml")
OUT = sys.argv[2] if len(sys.argv) > 2 else os.path.join(_DATA, "catalog.json")

# The keyword flag fields, in their fixed catalog order (each becomes a bool column).
KWS = ["Arcane", "Warded", "Mobile", "Steadfast", "Relentless", "Lurk"]


def _ordered_map(new_map, path):
    """Reorder `new_map` to match the committed JSON at `path`: existing keys first
    in their old order, then any new keys in `new_map`'s (catalog) order. Keeps the
    membership-only side-files byte-identical across regenerations."""
    prev = json.load(open(path)) if os.path.exists(path) else {}
    result = OrderedDict()
    for k in prev:
        if k in new_map:
            result[k] = new_map[k]
    for k in new_map:
        if k not in result:
            result[k] = new_map[k]
    return result


def _ordered_list(new_list, prev_list):
    """Order-preserving union for a membership list (effects.json pending/behavior)."""
    s = set(new_list)
    result = [k for k in prev_list if k in s]
    seen = set(result)
    for k in new_list:
        if k not in seen:
            result.append(k)
            seen.add(k)
    return result


# --- read the source ----------------------------------------------------------
with open(SRC, "rb") as f:
    _toml = tomllib.load(f)
_src_cards = _toml["card"]

# --- validate FIRST: a missing/malformed required field is a HARD ERROR ---------
# (never a silent skip — the guard against shipping incomplete cards). Reused by the
# Rust-side lint and runnable standalone via tools/validate_cards.py.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from validate_cards import validate as _validate_cards  # noqa: E402

_errs = _validate_cards(_src_cards)
if _errs:
    sys.stderr.write(f"cards.toml INVALID — {len(_errs)} error(s):\n")
    for _e in _errs[:200]:
        sys.stderr.write(f"  - {_e}\n")
    raise SystemExit(1)

# --- build the per-card catalog dicts (exact field order the catalog serializes) ---
# Order: name, kind, rarity, cost, attack, defense, hp, reach, resonance, imprints,
# rules, <keyword bools…> — then lineage (evolves_*), then id, then key (appended
# later), mirroring the historical pipeline so the byte layout is preserved.
cards = []
for c in _src_cards:
    have = set(c.get("keywords", []))
    card = OrderedDict()
    card["name"] = c["name"]
    card["kind"] = c["kind"]
    card["rarity"] = c["rarity"]
    # Evolution-form cost is derived below; the placeholder holds the field position.
    card["cost"] = int(c["cost"]) if "cost" in c else 0
    card["attack"] = int(c["attack"])
    card["defense"] = int(c["defense"])
    card["hp"] = int(c["hp"])
    card["reach"] = c["reach"]
    card["resonance"] = c["resonance"]
    card["imprints"] = list(c.get("imprints", []))
    card["rules"] = c["rules"]
    for k in KWS:
        card[k.lower()] = k in have
    cards.append(card)

# --- derive evolution-form cost (the Markdown pipeline's least-squares fit) -----
# Fit the stat budget from the priced spirits — sum(A+D+H) ≈ K·cost + B by least
# squares over every Spirit/Caller with an explicit cost > 0 — then price each form by
# inverting it: cost = round((sum − B) / K), clamped to a sane evolution band. This
# rides a re-stat automatically (forms stay proportioned to their bodies); a hardcoded
# 0 would make the discounted `form.cost − ⌊base.cost/2⌋` evolve-charge a giveaway.
_priced = [
    (c["attack"] + c["defense"] + c["hp"], c["cost"])
    for c in cards
    if c["kind"] in ("Spirit", "Caller") and c["cost"] > 0
]
if _priced:
    _n = len(_priced)
    _sx = sum(c for _, c in _priced)
    _sy = sum(s for s, _ in _priced)
    _sxx = sum(c * c for _, c in _priced)
    _sxy = sum(s * c for s, c in _priced)
    _denom = _n * _sxx - _sx * _sx
    _k = (_n * _sxy - _sx * _sy) / _denom if _denom else 20.0
    _b = (_sy - _k * _sx) / _n
    for c in cards:
        if c["kind"] == "Evolution":
            _raw = round((c["attack"] + c["defense"] + c["hp"] - _b) / _k) if _k else 5
            c["cost"] = max(2, min(8, int(_raw)))  # evolution band: a real board-investment cost

# --- evolution lineage + the tier split (from each card's [card.evolution]) -----
# A FORM's block gives `base`+`tier`; a BASE's block gives `split` (the tiers it keeps,
# default both). Edges: base.evolves_to += form (filtered by split), form.evolves_from
# = base. `evolves_*` is inserted HERE so it lands before id/key in the field order.
_lines = OrderedDict()  # form_name -> base_name  (the evolution_lines.json sidecar)
_split = OrderedDict()  # base_name -> [tiers]    (the evolution_split.json sidecar)
for src, card in zip(_src_cards, cards):
    evo = src.get("evolution")
    if not evo:
        continue
    if "base" in evo:
        _lines[card["name"]] = evo["base"]
    if "split" in evo:
        _split[card["name"]] = list(evo["split"])

_by_name = {c["name"]: c for c in cards}
for _form, _base in _lines.items():
    if _form in _by_name and _base in _by_name:
        _allowed = _split.get(_base, ["Primal", "Fabled"])
        if _by_name[_form].get("rarity") not in _allowed:
            continue
        _by_name[_base].setdefault("evolves_to", []).append(_form)
        _by_name[_form]["evolves_from"] = _base

# --- de-dup by name (rename-ledger ghosts), assign dense id ----------------------
seen, out = set(), []
for c in cards:
    if c["name"] in seen:
        continue
    seen.add(c["name"])
    c["id"] = len(out)
    out.append(c)

# --- stable per-card `key` (frozen, carried by the TOML) -------------------------
# The TOML already holds each card's frozen `key` (the canonical identity effects +
# engine logic key off — NOT the display name). A rename only repoints the TOML's
# `key`/`name` pairing; the key never moves.
_src_key = {c["name"]: c["key"] for c in _src_cards}
for c in out:
    c["key"] = _src_key[c["name"]]
_dupes = [k for k, n in Counter(c["key"] for c in out).items() if n > 1]
assert not _dupes, f"duplicate card keys: {_dupes}"

# --- write catalog.json (byte-identical to the historical layout) ----------------
json.dump(out, open(OUT, "w"), indent=0)

# --- regenerate the runtime side-data from the same source -----------------------
# Only when generating into the canonical data dir; an explicit OUT elsewhere (as
# catalog-check's /tmp target) leaves the side-files untouched.
_writing_canon = os.path.abspath(os.path.dirname(OUT)) == os.path.abspath(_DATA)
if _writing_canon:
    _src_by_name = {c["name"]: c for c in _src_cards}

    # effects.json — specs (per-card EffectSpec lists, keyed by stable key) + pending
    # + behavior. Built over the de-duped catalog so a rename-ghost contributes once.
    specs = OrderedDict()
    pending, behavior = [], []
    for c in out:
        src = _src_by_name[c["name"]]
        if src.get("effect"):
            specs[c["key"]] = [
                {
                    "trigger": sp["trigger"],
                    "condition": sp["condition"],
                    "clauses": [
                        {"selector": cl["selector"], "effect": cl["effect"], "duration": cl["duration"]}
                        for cl in sp["clause"]
                    ],
                }
                for sp in src["effect"]
            ]
        if src.get("effects_pending"):
            pending.append(c["key"])
        if src.get("behavior"):
            behavior.append(c["key"])

    _eff_path = os.path.join(_DATA, "effects.json")
    _prev_eff = json.load(open(_eff_path)) if os.path.exists(_eff_path) else {}
    _prev_specs = _prev_eff.get("specs", {})
    eff_out = OrderedDict()
    eff_out["specs"] = OrderedDict(
        (k, specs[k]) for k in list(_prev_specs) if k in specs
    )
    for k in specs:
        eff_out["specs"].setdefault(k, specs[k])
    eff_out["pending"] = _ordered_list(pending, _prev_eff.get("pending", []))
    eff_out["behavior"] = _ordered_list(behavior, _prev_eff.get("behavior", []))
    json.dump(eff_out, open(_eff_path, "w"), indent=2)

    # evolution_lines.json (form -> base) and evolution_split.json (base -> [tiers]).
    _lp = os.path.join(_DATA, "evolution_lines.json")
    _sp = os.path.join(_DATA, "evolution_split.json")
    json.dump(_ordered_map(_lines, _lp), open(_lp, "w"), indent=0)
    json.dump(_ordered_map(_split, _sp), open(_sp, "w"), indent=0)

    # card_keys.json (name -> key) and card_keywords.json (key -> [keywords]) — both
    # historically sort_keys=True, so their order is already canonical.
    _kp = os.path.join(_DATA, "card_keys.json")
    json.dump({c["name"]: c["key"] for c in out}, open(_kp, "w"), indent=0, sort_keys=True)
    _kwp = os.path.join(_DATA, "card_keywords.json")
    _kw_map = {c["key"]: [k for k in KWS if c[k.lower()]] for c in out}
    json.dump(_kw_map, open(_kwp, "w"), indent=0, sort_keys=True)

print("TOTAL:", len(out), dict(Counter(c["kind"] for c in out)))
print("rarity:", dict(Counter(c["rarity"] for c in out)))
