#!/usr/bin/env python3
"""Combat re-stat: rewrite the A/D/H stats in `data/cards.toml` so combat BITES
(~3 hits to fall an average spirit) and feels meatier.

A per-stat scale that PRESERVES each card's personality (its A:D:H tilt) while
opening net attack-minus-defense toward ~HP/3 and fattening HP:
    attack  *= KA   (up   — strikes land)
    defense *= KD   (down — net damage opens; combat currently whiffs at A==D)
    hp      *= KH   (up   — meatier bodies, ~3 hits)
All values rounded to multiples of 5 (the scale). Every card's `attack`/`defense`/
`hp` is scaled (spirits, callers, AND evolution forms — forms stay proportioned to
the bodies they grow into); evolution-form COST is then re-derived by `make catalog`'s
least-squares fit, so the discounted evolve-charge tracks the re-stat automatically.

  DRY RUN (default):  python tools/restat.py           # parse + preview, NO write
  APPLY:              python tools/restat.py --apply    # backs up to /tmp, then `make catalog`

Scales are env-overridable for sim-tuning (the exact values are found via
`cargo run -p recollect-bot --bin rebalance`):
    RESTAT_KA  RESTAT_KD  RESTAT_KH
"""
import os, re, sys, shutil, time

SRC = os.path.join(os.path.dirname(__file__), "..", "app", "crates", "recollect-core", "data", "cards.toml")
KA = float(os.environ.get("RESTAT_KA", "1.4"))  # attack up
KD = float(os.environ.get("RESTAT_KD", "0.6"))  # defense down (net opens)
KH = float(os.environ.get("RESTAT_KH", "1.4"))  # hp up (meatier)

# A bare `attack = N` / `defense = N` / `hp = N` line (the TOML stat fields). The
# leading-whitespace capture preserves indentation; we only touch the integer.
STAT = re.compile(r"^(\s*(attack|defense|hp)\s*=\s*)(\d+)\s*$")
SCALE = {"attack": KA, "defense": KD, "hp": KH}


def r5(x):
    return int(round(x / 5.0)) * 5


def main():
    apply = "--apply" in sys.argv
    with open(SRC) as f:
        lines = f.readlines()  # capture BEFORE any write (safe-rewrite discipline)
    out, changed, samples, cur_name = [], 0, [], "?"
    for line in lines:
        nm = re.match(r'^name\s*=\s*"(.*)"\s*$', line)
        if nm:
            cur_name = nm.group(1)
        m = STAT.match(line)
        if m:
            prefix, field, val = m.group(1), m.group(2), int(m.group(3))
            nv = r5(val * SCALE[field])
            if nv != val:
                line = f"{prefix}{nv}\n"
                changed += 1
                if field == "attack" and len(samples) < 8:
                    samples.append((cur_name, field, val, nv))
        out.append(line)
    print(f"restat KA={KA} KD={KD} KH={KH}  ->  {changed} stat values rewritten")
    for nm, field, o, n in samples:
        print(f"  {nm:30} {field:8} {o:>4}  ->  {n}")
    if apply:
        bak = f"/tmp/cards_pre_restat_{int(time.time())}.toml"
        shutil.copy(SRC, bak)
        with open(SRC, "w") as f:
            f.writelines(out)
        print(f"APPLIED to cards.toml. backup: {bak}")
        print("now run `make catalog` to regenerate catalog.json + side-data (re-derives form costs).")
    else:
        print("dry run — no write (pass --apply to write)")


if __name__ == "__main__":
    main()
