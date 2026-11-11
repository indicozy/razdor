#!/usr/bin/env python3
"""Split the 2.5D cards by a per-pixel ground/standing label map. Offline, numbers only.

The cards of `data/cards/objects.json` (version 3) keep their structure. For every sprite:
  1. SAM's automatic masks (`cut_sprites.sam_pipeline`, facebook/sam-vit-base, Apache-2.0) are
     assigned to the opaque pixels, smallest mask first; segments under SMALL_FRAC of the sprite's
     opaque area merge into their largest neighbour.
  2. The sprite's own ground plane D = p·x + q·y + r is fitted robustly (RANSAC) to the surely-ground
     pixels: the painted cast shadow and base, and each column's lowest opaque pixel.
  3. Each segment is fitted on its own (y-slope q_s): the ratio q_s / q is its slope against the ground's
     (scale-free). A ratio in RATIO_GROUND with a small median residual to the plane is ground (a floor
     faces up). A wall (|ratio| < RATIO_WALL) and anything between stands; roof colours and a rise at the
     top edge stand; a ground-like segment with standing pixels mostly below it stands.
  4. The labels are smoothed (3x3 open, then close) into one ground map per sprite.
  5. Each card is split by the map: its ground pixels and its standing pixels become separate pieces
     (connected pieces of at least MIN_PIECE_PX), each with its own foot (`cut_sprites.build_cards`).

Usage: CUT_SAM_DIR=<sam checkpoint> CUT_DEPTH_CACHE=<depth cache> split_cards.py --out <json> <key>...
(no keys: every sprite of the data). Needs the depth cache (`cut_sprites.py` writes it) and the installs.
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

SMALL_FRAC = 0.015  # of the sprite's opaque area: a SAM segment this small merges into its largest neighbour
MIN_PIECE_PX = 60  # a split piece smaller than this is dropped (its pixels go unlabelled)
MORPH = 3  # the label map's open/close kernel (px)
UPSCALE = cs.UPSCALE


def rasterize(card, h, w):
    m = np.zeros((h, w), np.uint8)
    verts = np.array(card["verts"], np.float64).reshape(-1, 2)
    tris = np.array(card["tris"], np.int64).reshape(-1, 3)
    for a, b, c in tris:
        pts = np.round(verts[[a, b, c]]).astype(np.int32)
        cv2.fillPoly(m, [pts], 1)
    return m.astype(bool)


def sam_masks(key, rgba, op, cache_dir):
    """SAM's automatic masks at 1x (cached per sprite as a packed npz; SAM runs only once per sprite)."""
    path = Path(cache_dir) / f"{key}.npz" if cache_dir else None
    if path is not None and path.exists():
        packed = np.load(path)["masks"]
        h, w = op.shape
        return [np.unpackbits(p)[: h * w].reshape(h, w).astype(bool) for p in packed]
    h, w = op.shape
    img = cs.Image.fromarray(cs.composite_rgb(rgba)).resize((w * UPSCALE, h * UPSCALE), cs.Image.Resampling.BICUBIC)
    out = cs.sam_pipeline()(img, points_per_batch=cs.SAM_POINTS_BATCH, pred_iou_thresh=0.86, stability_score_thresh=0.92)
    masks = []
    for m in out["masks"]:
        small = np.array(cs.Image.fromarray(np.array(m).astype(np.uint8) * 255).resize((w, h), cs.Image.Resampling.NEAREST)) > 0
        masks.append(small)
    if path is not None:
        path.parent.mkdir(parents=True, exist_ok=True)
        packed = np.stack([np.packbits(m.reshape(-1)) for m in masks]) if masks else np.zeros((0, 1), np.uint8)
        np.savez_compressed(path, masks=packed)
    return masks


def segments_from_masks(masks, op):
    """Each opaque pixel to the smallest mask holding it; the rest of the opaque pixels to their connected pieces;
    then the small segments merge into their largest neighbour. An (h, w) int array, -1 outside the silhouette."""
    h, w = op.shape
    order = sorted(range(len(masks)), key=lambda i: -int(masks[i].sum()))
    lab = np.full((h, w), -1, np.int32)
    for k, i in enumerate(order):
        lab[masks[i] & op] = k
    rest = op & (lab < 0)
    n_rest, cc = cv2.connectedComponents(rest.astype(np.uint8), connectivity=4)
    base = len(order)
    for c in range(1, n_rest):
        lab[cc == c] = base + c - 1
    small_px = SMALL_FRAC * int(op.sum())
    while True:
        ids, counts = np.unique(lab[lab >= 0], return_counts=True)
        if len(ids) < 2:
            break
        smallest = ids[np.argmin(counts)]
        if counts.min() >= small_px:
            break
        m = lab == smallest
        ring = cv2.dilate(m.astype(np.uint8), np.ones((3, 3), np.uint8)).astype(bool) & ~m & (lab >= 0)
        if not ring.any():
            lab[m] = -2  # isolated: unlabelled
            continue
        neighbours, nc = np.unique(lab[ring], return_counts=True)
        target = neighbours[np.argmax([counts[ids == n][0] for n in neighbours])]
        lab[m] = target
    lab[lab == -2] = -1
    ids = sorted(set(np.unique(lab).tolist()) - {-1})
    out = np.full((h, w), -1, np.int32)
    for new, old in enumerate(ids):
        out[lab == old] = new
    return out, len(ids)


def ground_map_for(key, rgba, op, D, masks, plane):
    """The per-pixel ground map (bool, over the silhouette) from the SAM segments and the sprite's plane."""
    h, w = op.shape
    p, q, r0 = plane
    yy, xx = np.mgrid[0:h, 0:w].astype(float)
    on_ground = p * xx + q * yy + r0
    resid_all = np.abs(D - on_ground)
    rgbf = rgba[..., :3].astype(float)
    mx, mn = rgbf.max(axis=2), rgbf.min(axis=2)
    sat = np.where(mx > 0, (mx - mn) / np.maximum(mx, 1), 0.0)
    rr, gg, bb = rgbf[..., 0], rgbf[..., 1], rgbf[..., 2]
    span = np.maximum(mx - mn, 1e-6)
    hue = np.where(mx == rr, 60.0 * ((gg - bb) / span % 6), np.where(mx == gg, 60.0 * ((bb - rr) / span + 2), 60.0 * ((rr - gg) / span + 4)))
    hue = np.where(hue < 0, hue + 360.0, hue)
    thatch = (hue >= 30) & (hue <= 55) & (sat >= cs.THATCH_SAT) & (mx >= cs.THATCH_VALUE * 255)
    red_roof = (sat >= cs.ROOF_SAT) & (rr > cs.ROOF_RED * gg)
    slate = (sat < 0.2) & (bb > rr) & (mx < 170) & (mx > 60)
    roof_px = thatch | red_roof | slate
    lab, K = segments_from_masks(masks, op)
    rise = D - on_ground
    rows_idx = np.arange(h)[:, None]
    ground, stand0 = [], []
    for k in range(K):
        m = lab == k
        n = int(m.sum())
        ys, xs = np.nonzero(m)
        A = np.stack([xs, ys, np.ones(n)], axis=1).astype(float)
        qy = float(np.linalg.lstsq(A, D[ys, xs], rcond=None)[0][1])
        ratio = qy / plane[1] if abs(plane[1]) > 1e-6 else 0.0
        resid = float(np.median(resid_all[m]))
        rows = np.nonzero(m.any(axis=1))[0]
        top, bottom = rows[0], rows[-1]
        band = max(1, (bottom - top) // 5)
        top_band = m & (rows_idx <= top + band)
        bot_band = m & (rows_idx >= bottom - band)
        rise_top = float(np.median(rise[top_band])) if top_band.any() else 0.0
        rise_bot = float(np.median(rise[bot_band])) if bot_band.any() else 0.0
        on_plane = cs.RATIO_GROUND[0] <= ratio <= cs.RATIO_GROUND[1] and resid <= cs.GROUND_RESID
        roof = (rise_top - rise_bot > cs.ROOF_TOP_STEP and rise_top > cs.GROUND_RESID) or float(roof_px[m].mean()) >= 0.5
        ground.append(bool(on_plane and not roof))
        stand0.append(not ground[-1])
    ground_px = np.zeros((h, w), bool)
    for k in range(K):
        if ground[k]:
            ground_px |= lab == k
    # Roof on wall: a ground segment with standing segments mostly below it, and rising, stands.
    ld = np.full(lab.shape, -1, np.int32)
    ld[:-1, :] = lab[1:, :]
    for k in range(K):
        if not ground[k]:
            continue
        m = lab == k
        nb = ld[m]
        nb = nb[nb >= 0]
        if nb.size and float(np.mean(np.array(stand0)[nb])) >= cs.STACK_SHARE and float(np.median(rise[m])) > cs.GROUND_RESID:
            ground_px &= ~m
    kernel = np.ones((MORPH, MORPH), np.uint8)
    g = ground_px.astype(np.uint8)
    g = cv2.morphologyEx(g, cv2.MORPH_OPEN, kernel)
    g = cv2.morphologyEx(g, cv2.MORPH_CLOSE, kernel)
    return g.astype(bool) & op


def split_cards(cards, ground_map, op, D, h, w):
    """The cards cut by the map: each card's ground pixels and standing pixels as pieces; returns (lab, K, flags)."""
    assign = np.full((h, w), -1, np.int32)
    for i, c in enumerate(cards):
        m = rasterize(c, h, w) & (assign < 0)
        assign[m] = i
    lab = np.full((h, w), -1, np.int32)
    flags = []
    for i in range(len(cards)):
        for flag in (True, False):
            part = (assign == i) & (ground_map == flag) & op
            if not part.any():
                continue
            n_cc, cc = cv2.connectedComponents(part.astype(np.uint8), connectivity=4)
            for c in range(1, n_cc):
                piece = cc == c
                if piece.sum() < MIN_PIECE_PX:
                    continue
                lab[piece] = len(flags)
                flags.append(flag)
    return lab, len(flags), flags


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--data", type=Path, default=HERE.parents[1] / "data" / "cards" / "objects.json")
    ap.add_argument("--sam-cache", type=Path, default=os.environ.get("CUT_SAM_CACHE"))
    ap.add_argument("keys", nargs="*")
    args = ap.parse_args()
    data = json.loads(args.data.read_text())
    wanted = set(args.keys) if args.keys else set(data["sprites"])
    installs = [Path(p) for p in os.environ.get("CUT_INSTALLS", "").split(os.pathsep) if p]
    cs.SAM_DIR = os.environ["CUT_SAM_DIR"]
    depth_dir = Path(os.environ["CUT_DEPTH_CACHE"])
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
            dep = np.load(depth_dir / f"{key}.npy")
            lo, hi = float(dep[op].min()), float(dep[op].max())
            D = (dep - lo) / (hi - lo) if hi > lo else np.zeros_like(dep)
            plane = cs.ground_plane_xy(rgba, op, D)
            if plane is None:
                out_sprites[key] = {"w": w, "h": h, "cards": [{**c, "ground": False} for c in old["cards"]]}
                continue
            masks = sam_masks(key, rgba, op, args.sam_cache)
            gmap = ground_map_for(key, rgba, op, D, masks, plane)
            lab, K, flags = split_cards(old["cards"], gmap, op, D, h, w)
            if K == 0:
                out_sprites[key] = {"w": w, "h": h, "cards": [{**c, "ground": False} for c in old["cards"]]}
                continue
            pieces = cs.build_cards(lab, D, K)
            out_sprites[key] = {
                "w": w,
                "h": h,
                "cards": [{"foot": c["foot"], "verts": c["verts"], "tris": c["tris"], "ground": bool(flags[c["region"]])} for c in pieces],
            }
            print(key, w, h, "cards", len(old["cards"]), "->", len(pieces), "ground", sum(flags), flush=True)
    for key in wanted - done:
        if key in data["sprites"]:
            out_sprites[key] = {**data["sprites"][key], "cards": [{**c, "ground": c.get("ground", False)} for c in data["sprites"][key]["cards"]]}
    result = {"version": 4, "model": data["model"], "alpha_opaque": data["alpha_opaque"], "one_card": data["one_card"], "sprites": out_sprites}
    args.out.write_text(json.dumps(result, separators=(",", ":")) + "\n", encoding="utf-8")
    print("wrote", args.out, "sprites", len(out_sprites))


if __name__ == "__main__":
    main()
