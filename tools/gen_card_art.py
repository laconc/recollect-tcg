#!/usr/bin/env python3
"""Card-art batch generator — the runnable loop that WOULD call an
external AI image tool, once per card, to produce the masters the delivery
pipeline (tools/cardpipe) then optimizes.

WHAT THIS DOES
  * Reads the catalog (the single card-truth source) for every card's stable
    `key`, `name`, `kind`, `resonance`, `rarity`, `rules`.
  * Builds a per-card prompt from the ONE shared art template (paper & ink, 5:7
    portrait, original + PII-free, cohesive across all 407) — the template lives
    in docs/decisions/card_images.md and is mirrored here so the loop is
    self-contained.
  * Writes a review manifest (tools/card_art_prompts.jsonl) of {key, path, prompt}
    so the prompts can be audited/diffed before any spend.
  * For each card MISSING a master (assets/cards-src/<key>.png), it is ready to
    call the image API and save the PNG — but the call is STUBBED. Nothing is
    generated and no network request is made until a human/model wires a real
    backend in `generate_image()` and passes --i-have-a-budget.

WHY STUBBED
  Image generation costs money and is a deliberate, reviewed step. This script is
  the *deliverable*: a correct, runnable loop with the API call guarded so it
  cannot fire by accident in CI or a casual run. See docs/decisions/card_images.md.

USAGE
  python3 tools/gen_card_art.py                 # dry run: write the prompt manifest, report gaps
  python3 tools/gen_card_art.py --print KEY     # print the exact prompt for one card
  python3 tools/gen_card_art.py --only KEY[,KEY]  # restrict to specific card keys
  python3 tools/gen_card_art.py --i-have-a-budget # arm generation (still errors until a backend is wired)

WIRING A BACKEND
  Implement `generate_image(prompt, out_path)` against your image tool of choice
  (any text-to-image API that returns PNG bytes — e.g. an OpenAI Images, a
  Stability, a Replicate, or a local diffusion endpoint). Keep the 5:7 / 1024×1434
  output and the shared template. Then run with --i-have-a-budget. The masters
  land in assets/cards-src/<key>.png; `make cards-images` delivers them.
"""
import argparse
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
CATALOG = ROOT / "app/crates/recollect-core/data/catalog.json"
MASTERS_DIR = ROOT / "assets/cards-src"
MANIFEST = ROOT / "tools/card_art_prompts.jsonl"

# Master output spec (must match the delivery pipeline + the design doc).
ART_W, ART_H = 1024, 1434  # 5:7 portrait

# ---- The ONE shared art prompt template -------------------------------------
# Keep this in lockstep with the "Generation prompt" section of
# docs/decisions/card_images.md — that document is the source of truth; this is a
# runnable mirror. Every card's prompt is this STYLE preamble + a per-card SUBJECT
# line derived from the catalog, so all 407 cohere.
STYLE_PREAMBLE = (
    "Hand-drawn pen-and-ink illustration with restrained watercolor washes, in a "
    "warm 'paper & ink' storybook style on aged cream paper (#f5f0e3). Fine "
    "cross-hatched linework in near-black ink (#17171f); soft, faded edges as if a "
    "remembered scene. A single muted accent of antique gold (#ebc72e). Quiet, "
    "contemplative, melancholic-wonder mood — a fading memory being retold. "
    "Portrait composition, 5:7 aspect ratio, single clear focal subject centered "
    "with generous negative space and no border, frame, text, letters, numbers, "
    "watermark, signature, or UI. Cohesive across a 407-card set: same palette, "
    "same ink weight, same paper. Entirely original; depict NO real, trademarked, "
    "or recognizable real-world person, brand, logo, or existing character."
)

# Resonance → a one-word emotional/color steer so the set varies without drifting.
RESONANCE_TONE = {
    "Wonder": "luminous, awe-struck, dawn-soft light",
    "Sorrow": "rain-dim, tender, blue-grey melancholy",
    "Fury": "smoldering, kinetic, ember-warm tension",
    "Fear": "shadowed, hushed, looming negative space",
    "Harmony": "balanced, woven, gently radiant calm",
    "Resolve": "steadfast, upright, quiet determination",
    "Neutral": "even, archival, plainly observed",
}

# Card kind → what the focal subject IS, so the artist frames it right.
KIND_SUBJECT = {
    "Spirit": "a single conjured spirit-creature",
    "Caller": "a lone storyteller-caller figure",
    "Ritual": "an abstract ritual gesture or rite (no figures required)",
    "Bond": "two motifs joined by a fragile thread of light",
    "Landmark": "a quiet remembered place or structure",
    "Fabrication": "a constructed object or artifice",
    "Kindred": "a small minor conjured Kindred-creature",
    "Evolution": "a transformed, ascended form of a creature",
    "Foundling": "a small lost wandering creature",
    "Unwritten": "a half-erased antagonist creature dissolving at its edges",
    "IllIntent": "a menacing antagonist creature wreathed in unmaking",
    "Unwriting": "an abstract scene of a memory being erased (no figures required)",
}


def esc_oneline(s: str) -> str:
    """Collapse rules text to a single safe clause for the prompt (no markup)."""
    return " ".join(str(s).split())


def build_prompt(card: dict) -> str:
    """The full per-card prompt: shared style + a subject derived from the card."""
    kind = card.get("kind", "")
    subject = KIND_SUBJECT.get(kind, "a single evocative motif")
    tone = RESONANCE_TONE.get(card.get("resonance", "Neutral"), "even, archival")
    # The rules text is a flavor steer only — never asked to be rendered as text.
    flavor = esc_oneline(card.get("rules", ""))
    flavor_clause = f" Evoke its theme (do not render any text): {flavor}" if flavor else ""
    return (
        f"{STYLE_PREAMBLE} Subject: {subject} representing the card "
        f"“{card['name']}”. Emotional key: {tone}.{flavor_clause}"
    )


def generate_image(prompt: str, out_path: pathlib.Path) -> None:
    """STUB — wire a real text-to-image backend here, then remove this guard.

    Must write a {ART_W}x{ART_H} PNG (5:7 portrait) to `out_path`. Until then this
    raises so the loop is runnable but can never silently spend or hit the network.
    """
    raise NotImplementedError(
        "gen_card_art: no image backend wired. Implement generate_image() against "
        "your image API (return a 1024x1434 PNG), then re-run with --i-have-a-budget. "
        "See the module docstring + docs/decisions/card_images.md."
    )


def load_catalog() -> list:
    return json.loads(CATALOG.read_text())


def main() -> int:
    ap = argparse.ArgumentParser(description="Batch card-art prompt builder / generator (stubbed).")
    ap.add_argument("--only", help="comma-separated card keys to restrict to")
    ap.add_argument("--print", dest="print_key", metavar="KEY",
                    help="print the prompt for one card key and exit")
    ap.add_argument("--i-have-a-budget", action="store_true",
                    help="arm generation (still errors until generate_image() is wired)")
    args = ap.parse_args()

    cards = load_catalog()
    by_key = {c["key"]: c for c in cards}

    if args.print_key:
        c = by_key.get(args.print_key)
        if not c:
            print(f"no card with key {args.print_key!r}", file=sys.stderr)
            return 1
        print(build_prompt(c))
        return 0

    only = set(args.only.split(",")) if args.only else None
    selected = [c for c in cards if not only or c["key"] in only]

    MASTERS_DIR.mkdir(parents=True, exist_ok=True)
    # Write the auditable prompt manifest for the whole selection.
    with MANIFEST.open("w") as f:
        for c in selected:
            out_path = MASTERS_DIR / f"{c['key']}.png"
            f.write(json.dumps({
                "key": c["key"],
                "name": c["name"],
                "kind": c["kind"],
                "path": str(out_path.relative_to(ROOT)),
                "exists": out_path.exists(),
                "prompt": build_prompt(c),
            }) + "\n")

    missing = [c for c in selected if not (MASTERS_DIR / f"{c['key']}.png").exists()]
    print(f"gen_card_art: {len(selected)} card(s); "
          f"{len(selected) - len(missing)} master(s) present, {len(missing)} to generate.")
    print(f"gen_card_art: prompt manifest -> {MANIFEST.relative_to(ROOT)}")

    if not args.i_have_a_budget:
        print("gen_card_art: dry run (no API armed). Re-run with --i-have-a-budget to generate "
              "(requires a wired generate_image()).")
        return 0

    # Armed: attempt generation for missing masters. generate_image() is a guarded
    # stub until a backend is wired, so this raises NotImplementedError by design.
    generated = 0
    for c in missing:
        out_path = MASTERS_DIR / f"{c['key']}.png"
        prompt = build_prompt(c)
        print(f"gen_card_art: generating {c['key']}.png …")
        generate_image(prompt, out_path)  # STUB → raises until wired
        generated += 1
    print(f"gen_card_art: generated {generated} master(s). Run `make cards-images` to deliver.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
