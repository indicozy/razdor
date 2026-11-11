#!/usr/bin/env python3
"""Standing islands as cards, each on its own base. Offline, numbers only.

The v4 cards (`data/cards/objects.json`) keep their ground pieces. Their standing pixels (the opaque pixels of the
standing cards) are re-cut into alpha-connected islands: a 3x3 opening first (1-2 px bridges such as fences
fall away), then the connected components of the standing pixels. Each island becomes its own card, with
its own foot: the lowest opaque row of the island and the middle of that row, not a depth-extrapolated or
a shared foot. Islands under MIN_ISLAND_PX are specks and are left out.

Usage: CUT_INSTALLS=<install>:<install> island_cards.py --data <v4.json> --out <v5.json>
"""

import argparse
import json
import os
import sys
from pathlib import Path

import cv2
import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import cut_sprites as cs  # noqa: E402
import split_cards as sc  # noqa: E402

MIN_ISLAND_PX = 30


def islands(standing: np.ndarray):
    """Labels (0 = none, 1..n = islands) of the opened standing mask."""
    opened = cv2.morphologyEx(standing.astype(np.uint8), cv2.MORPH_OPEN, np.ones((3, 3), np.uint8))
    n, lab = cv2.connectedComponents(opened, connectivity=4)
    return lab, n


def island_card(mask: np.ndarray):
    """A card for one island mask: its foot on its own lowest row, and its polygon."""
    rows = np.nonzero(mask.any(axis=1))[0]
    bottom = int(rows[-1])
    xs = np.nonzero(mask[bottom])[0]
    foot = [float(xs.mean()), float(bottom)]
    got = cs.region_card(cs.card_grown(mask))
    if got is None:
        return None
    verts, tris = got
    return {"foot": foot, "verts": verts, "tris": tris, "ground": False}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--data", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args()
    data = json.loads(args.data.read_text())
    installs = [Path(p) for p in os.environ["CUT_INSTALLS"].split(os.pathsep) if p]
    wanted = set(data["sprites"])
    out_sprites = {}
    done = set()
    for inst in installs:
        for sec, w, h, rgba in cs.decode_objects((inst / cs.OBJECTS).read_bytes()):
            key = cs.sprite_key(w, h, rgba)
            if key not in wanted or key in done:
                continue
            done.add(key)
            old = data["sprites"][key]
            op = rgba[..., 3] >= cs.ALPHA_OPAQUE
            ground_cards = [c for c in old["cards"] if c["ground"]]
            standing_cards = [c for c in old["cards"] if not c["ground"]]
            standing = np.zeros((h, w), bool)
            for c in standing_cards:
                standing |= sc.rasterize(c, h, w)
            standing &= op
            lab, n = islands(standing)
            cards = list(ground_cards)
            for i in range(1, n):
                mask = lab == i
                if int(mask.sum()) < MIN_ISLAND_PX:
                    continue
                card = island_card(mask)
                if card is not None:
                    cards.append(card)
            if not any(not c["ground"] for c in cards):
                cards = old["cards"]  # a building never lies wholly flat: keep the v4 cut as it was
            cards.sort(key=lambda c: (c["foot"][1], c["foot"][0]))
            out_sprites[key] = {"w": w, "h": h, "cards": cards}
            print(key, "standing cards", len(standing_cards), "-> islands", sum(1 for c in cards if not c["ground"]), flush=True)
    for key in wanted - done:
        out_sprites[key] = data["sprites"][key]
    result = {"version": 5, "model": data["model"], "alpha_opaque": data["alpha_opaque"], "one_card": data["one_card"], "sprites": out_sprites}
    args.out.write_text(json.dumps(result, separators=(",", ":")) + "\n", encoding="utf-8")
    print("wrote", args.out, "sprites", len(out_sprites))


if __name__ == "__main__":
    main()
