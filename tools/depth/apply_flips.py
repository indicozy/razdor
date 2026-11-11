#!/usr/bin/env python3
"""Apply the owner's card flips to the cards data. Offline, numbers only.

`flips.json` (from the click tool, `{sprite hash: [card indices flipped]}`) flips each named card's ground flag:
a ground piece becomes standing, a standing piece becomes ground. A card flipped to standing gets its foot at its
lowest row (the middle of that row); the standing islands keep their own feet. A flip is applied once, to the
cards of the file as it is (the indices are the file's own).

Usage: apply_flips.py --data data/cards/objects.json --flips flips.json --out data/cards/objects.json
"""

import argparse
import json
from pathlib import Path


def lowest_foot(card):
    verts = card["verts"]
    ys = verts[1::2]
    bottom = max(ys)
    xs = [x for x, y in zip(verts[0::2], ys) if y == bottom]
    return [sum(xs) / len(xs), bottom]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--data", required=True, type=Path)
    ap.add_argument("--flips", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args()
    data = json.loads(args.data.read_text())
    flips = json.loads(args.flips.read_text())
    changed = 0
    for key, entry in flips.items():
        # {hash: {flips: [indices], checked: bool}}; the older {hash: [indices]} is read too.
        indices = entry["flips"] if isinstance(entry, dict) else entry
        sprite = data["sprites"].get(key)
        if sprite is None:
            print("unknown sprite", key)
            continue
        for i in indices:
            card = sprite["cards"][i]
            card["ground"] = not card["ground"]
            if not card["ground"]:
                card["foot"] = lowest_foot(card)
            changed += 1
    args.out.write_text(json.dumps(data, separators=(",", ":")) + "\n", encoding="utf-8")
    print("flipped", changed, "cards in", len(flips), "sprites")


if __name__ == "__main__":
    main()
