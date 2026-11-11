#!/usr/bin/env python3
"""Split the 2.5D cards by per-pixel surface normals. Offline, numbers only.

The cards of `data/cards/objects.json` (version 3) keep their structure. For every sprite:
  1. Its surface normals come from a monocular normal model (Marigold-Normals, Apache-2.0), cached per
     sprite by `normals.py` (scratch, never committed).
  2. A pixel is ground when its normal lies within GROUND_DEG of the calibrated ground normal G and is not
     a wall (within WALL_DEG of the calibrated left or right wall normal). The strict ground map is opened
     and closed (3x3).
  3. Containment: the ground components touching the sprite's base (each column's lowest opaque pixel and
     the painted shadow band) are kept; a component not touching the base is kept only when it is large
     (at least MIN_ENCLOSED of the opaque area) and enclosed (not on the silhouette's edge), a courtyard.
  4. The v3 cards are split by that map (`split_cards.split_cards`) and each piece gets its own foot
     (`cut_sprites.build_cards`).

The calibration (`CALIBRATION`) was taken once from five easy sprites (lighthouse, village base, village,
castle, tower) by k-means over their opaque normals; the three centres are the ground normal and the two
wall normals, as the Marigold maps show them.

Usage: CUT_INSTALLS=<install>:<install> NORMAL_DIR=<normals> CUT_DEPTH_CACHE=<depth cache>
       normal_split.py --out <json> [key...]   (no keys: every sprite of the data)
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


def unit(v):
    v = np.asarray(v, float)
    return v / np.linalg.norm(v)


# Calibrated once (see the docstring): ground, left wall, right wall.
G = unit([0.077, 0.710, 0.700])
WL = unit([-0.564, 0.029, 0.825])
WR = unit([0.683, -0.169, 0.711])
GROUND_DEG = 30.0
WALL_DEG = 25.0
TAU_G = np.cos(np.radians(GROUND_DEG))
TAU_W = np.cos(np.radians(WALL_DEG))
MIN_ENCLOSED = 0.02  # of the opaque area: an enclosed ground component this large is a courtyard


def ground_map(rgba, n, op):
    """The per-pixel ground map (bool, over the silhouette)."""
    h, w = op.shape
    k3 = np.ones((3, 3), np.uint8)
    dot_g = np.einsum("hwc,c->hw", n, G)
    wall = op & ((np.einsum("hwc,c->hw", n, WL) > TAU_W) | (np.einsum("hwc,c->hw", n, WR) > TAU_W))
    ground0 = op & (dot_g > TAU_G) & ~wall
    g = cv2.morphologyEx(ground0.astype(np.uint8), cv2.MORPH_OPEN, k3)
    g = cv2.morphologyEx(g, cv2.MORPH_CLOSE, k3).astype(bool) & op
    alpha = rgba[..., 3]
    shadow = (alpha >= cs.SHADOW_LO) & (alpha <= cs.SHADOW_HI) & ~op
    seed = np.zeros((h, w), bool)
    for x in np.nonzero(op.any(axis=0))[0]:
        seed[np.nonzero(op[:, x])[0].max(), x] = True
    seed |= shadow
    near_seed = cv2.dilate(seed.astype(np.uint8), k3).astype(bool)
    edge_band = cv2.dilate(op.astype(np.uint8), k3).astype(bool) & ~op
    total = int(op.sum())
    n_cc, cc = cv2.connectedComponents(g.astype(np.uint8), connectivity=4)
    reached = np.zeros((h, w), bool)
    for c in range(1, n_cc):
        comp = cc == c
        if (comp & near_seed).any():
            reached |= comp
            continue
        if int(comp.sum()) >= MIN_ENCLOSED * total and not (comp & edge_band).any():
            reached |= comp
    return reached & op


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--data", type=Path, default=HERE.parents[1] / "data" / "cards" / "objects.json")
    ap.add_argument("keys", nargs="*")
    args = ap.parse_args()
    data = json.loads(args.data.read_text())
    wanted = set(args.keys) if args.keys else set(data["sprites"])
    normal_dir = Path(os.environ["NORMAL_DIR"])
    depth_dir = Path(os.environ["CUT_DEPTH_CACHE"])
    installs = [Path(p) for p in os.environ["CUT_INSTALLS"].split(os.pathsep) if p]
    out_sprites = {}
    done = set()
    for inst in installs:
        for sec, w, h, rgba in cs.decode_objects((inst / cs.OBJECTS).read_bytes()):
            key = cs.sprite_key(w, h, rgba)
            if key not in wanted or key in done:
                continue
            done.add(key)
            cards = data["sprites"][key]["cards"]
            op = rgba[..., 3] >= cs.ALPHA_OPAQUE
            n = np.load(normal_dir / f"{key}.npy").astype(np.float32)
            dep = np.load(depth_dir / f"{key}.npy")
            lo, hi = float(dep[op].min()), float(dep[op].max())
            D = (dep - lo) / (hi - lo) if hi > lo else np.zeros_like(dep)
            gmap = ground_map(rgba, n, op)
            lab, K, flags = sc.split_cards(cards, gmap, op, D, h, w)
            if K == 0:
                out_sprites[key] = {"w": w, "h": h, "cards": [{**c, "ground": False} for c in cards]}
                continue
            if all(flags):
                # A building never lies wholly flat: its largest piece stands (the rule is the cut's, not the normals').
                sizes = np.bincount(lab[lab >= 0], minlength=K)
                flags[int(np.argmax(sizes))] = False
            pieces = cs.build_cards(lab, D, K)
            out_sprites[key] = {
                "w": w,
                "h": h,
                "cards": [{"foot": c["foot"], "verts": c["verts"], "tris": c["tris"], "ground": bool(flags[c["region"]])} for c in pieces],
            }
            print(key, "cards", len(cards), "->", len(pieces), "ground", sum(flags), flush=True)
    for key in wanted - done:
        if key in data["sprites"]:
            out_sprites[key] = {**data["sprites"][key], "cards": [{**c, "ground": c.get("ground", False)} for c in data["sprites"][key]["cards"]]}
    result = {"version": 4, "model": data["model"], "alpha_opaque": data["alpha_opaque"], "one_card": data["one_card"], "sprites": out_sprites}
    args.out.write_text(json.dumps(result, separators=(",", ":")) + "\n", encoding="utf-8")
    print("wrote", args.out, "sprites", len(out_sprites))


if __name__ == "__main__":
    main()
