#!/usr/bin/env python3
"""Cut the map sprites (Graphics/Objects/Objects.ugs) into depth cards. Offline, numbers only.

For every sprite of every install: a depth map from Depth Anything V2 Large (CUDA, fp16), the
opaque pixels split into a few depth regions, and each region becomes a card: its foot point
(where it meets the ground) and a triangulated polygon. Only numbers are written, keyed by a hash
of the sprite's decoded pixels. No sprite, depth map or preview is ever written to the repo.

Usage (see tools/depth/README.md):
  cut_sprites.py --out data/cards/objects.json [--preview DIR] <install>...
"""

import argparse
import colorsys
import json
import math
import struct
import time
from collections import Counter
from pathlib import Path

import cv2
import mapbox_earcut
import numpy as np
from PIL import Image, ImageDraw
from scipy.sparse import csr_matrix
from scipy.sparse.csgraph import connected_components
from sklearn.cluster import KMeans
from sklearn.metrics import silhouette_score

MODEL = "depth-anything/Depth-Anything-V2-Large-hf"
ALPHA_OPAQUE = 204  # shadows are alpha 153..187 and are not part of any card
MIN_OPAQUE = 200  # fewer opaque pixels: no cards
DEPTH_EDGE = 0.06  # T: neighbours join a region when their depths differ by at most this
SMALL_MIN_PX = 60
SMALL_FRAC = 0.015  # of the sprite's opaque pixels
MAX_REGIONS = 8
UPSCALE = 4  # bicubic, before the depth model
GREY = 128  # the composite background
OCCLUDED_FRAC = 0.30
SIMPLIFY_EPS = 1.0
FNV_OFFSET = 0xCBF29CE484222325
FNV_PRIME = 0x100000001B3
MASK64 = (1 << 64) - 1
OBJECTS = Path("Graphics") / "Objects" / "Objects.ugs"

# Bends pass: a building sprite left with one region is cut at its structural bends.
SECTION_BUILDING = 1  # the object-section of buildings (castles, towns)
BENDS_MIN_OPAQUE = 2500
BENDS_MIN_RANGE = 0.25  # p95 - p5 of the normalised opaque depth
BENDS_SIGMA = 2.0  # Gaussian of the normalised convolution
BENDS_K_MIN = 2
BENDS_K_MAX = 5
BENDS_MIN_CLUSTER_FRAC = 0.08  # of the opaque pixels: a smaller cluster rejects its K
BENDS_SAMPLE = 3000  # silhouette sample
BENDS_SEED = 0
BENDS_MAX_REGIONS = 5
BENDS_REACH = 4  # px, into the neighbours across a cut (a 9x9 square)


# ----------------------------------------------------------------------------------------------
# Reading Objects.ugs (the port of src/dt/gfx.rs: decode_objects_ugs and ugs_pixel)
# ----------------------------------------------------------------------------------------------


def ugs_rgba(px: np.ndarray, w: int, h: int) -> np.ndarray:
    """Decode w*h little-endian UGS pixels (uint32 array) to an (h, w, 4) uint8 RGBA array."""
    v = ((px >> 3) | (px << 13)) & 0xFFFF  # rotate_right(3) on 16 bits
    p = v ^ 0x5555

    def n(shift):
        return (((p >> shift) & 15) * 17).astype(np.uint8)

    return np.stack([n(8), n(4), n(0), n(12)], axis=-1).reshape(h, w, 4)


def decode_objects(data: bytes):
    """Yield (section, w, h, rgba) for each sprite record, in file order."""
    n = len(data)

    def u32(p):
        return struct.unpack_from("<I", data, p)[0] if p + 4 <= n else None

    def u16(p):
        return struct.unpack_from("<H", data, p)[0] if p + 2 <= n else None

    o = 0
    section = 0
    prev = None
    while o < n:
        first = u32(o)
        if first is None:
            raise ValueError(f"Objects.ugs: bad slot at {o}")
        if first == 0:
            o += 4
            continue
        if o + 20 > n:
            raise ValueError(f"Objects.ugs: truncated record at {o}")
        w, h = u32(o + 12), u32(o + 16)
        k = None
        for kk in range(5, 14):
            p = o + 4 * kk
            if u16(p) == w and u16(p + 2) == h and p + 4 + w * h * 2 <= n:
                k = kk
                break
        if k is None:
            raise ValueError(f"Objects.ugs: no frame header for a record at {o}")
        el = 4 * k - 20  # the extra field's length: a change starts the next section
        if prev is not None and prev != el:
            section += 1
        prev = el
        b = o + 4 * k + 4
        px = np.frombuffer(data, dtype="<u2", count=w * h, offset=b).astype(np.uint32)
        yield section, w, h, ugs_rgba(px, w, h)
        o = b + w * h * 2


def sprite_key(w: int, h: int, rgba: np.ndarray) -> str:
    """FNV-1a 64 over u32le(w) + u32le(h) + the RGBA bytes, as 16 lowercase hex digits."""
    acc = FNV_OFFSET
    for byte in struct.pack("<II", w, h) + rgba.tobytes():
        acc = ((acc ^ byte) * FNV_PRIME) & MASK64
    return f"{acc:016x}"


# ----------------------------------------------------------------------------------------------
# Depth
# ----------------------------------------------------------------------------------------------


def composite_rgb(rgba: np.ndarray) -> np.ndarray:
    a = rgba[..., 3:4].astype(np.float32) / 255.0
    rgb = rgba[..., :3].astype(np.float32) * a + GREY * (1.0 - a)
    return np.clip(np.rint(rgb), 0, 255).astype(np.uint8)


def load_model():
    import torch
    from transformers import AutoImageProcessor, AutoModelForDepthEstimation

    torch.set_num_threads(12)
    if not torch.cuda.is_available():
        raise SystemExit("CUDA is not visible: run with CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=1")
    name = torch.cuda.get_device_name(0)
    if "4060" not in name:
        raise SystemExit(f"GPU 0 is {name!r}, not the RTX 4060: run with CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=1")
    proc = AutoImageProcessor.from_pretrained(MODEL)
    model = AutoModelForDepthEstimation.from_pretrained(MODEL, torch_dtype=torch.float16)
    model = model.to("cuda").eval()
    return torch, proc, model


def predict_depth(torch, proc, model, rgba: np.ndarray, w: int, h: int) -> np.ndarray:
    """Relative depth (higher = closer), an (h, w) float32 array at the sprite's size."""
    img = Image.fromarray(composite_rgb(rgba)).resize((w * UPSCALE, h * UPSCALE), Image.Resampling.BICUBIC)
    pixels = proc(images=img, return_tensors="pt")["pixel_values"].to("cuda", torch.float16)
    with torch.inference_mode():
        pred = model(pixel_values=pixels).predicted_depth  # (1, H', W')
        d = torch.nn.functional.interpolate(
            pred.unsqueeze(1).float(), size=(h, w), mode="bilinear", align_corners=False
        )
    return d[0, 0].cpu().numpy()


# ----------------------------------------------------------------------------------------------
# Regions and merging
# ----------------------------------------------------------------------------------------------


def label_regions(op: np.ndarray, eh: np.ndarray, ev: np.ndarray) -> np.ndarray:
    """4-connected components of the opaque pixels. eh (h, w-1) and ev (h-1, w) mark the
    neighbour pairs that are joined. Returns an (h, w) int array, -1 outside the opaque pixels."""
    h, w = op.shape
    idx = np.arange(h * w).reshape(h, w)
    ea = np.concatenate([idx[:, :-1][eh], idx[:-1, :][ev]])
    eb = np.concatenate([idx[:, 1:][eh], idx[1:, :][ev]])
    graph = csr_matrix((np.ones(len(ea), np.int8), (ea, eb)), shape=(h * w, h * w))
    _, comp = connected_components(graph, directed=False)
    lab = np.full((h, w), -1, np.int64)
    comp = comp.reshape(h, w)
    lab[op] = np.unique(comp[op], return_inverse=True)[1]
    return lab


def find_regions(op: np.ndarray, D: np.ndarray) -> np.ndarray:
    """Label the opaque pixels: 4-connected, neighbours joined when |d1 - d2| <= DEPTH_EDGE.

    This is the union-find of the spec, computed as connected components of the same edges.
    Returns an (h, w) int array, -1 outside the opaque pixels.
    """
    eh = op[:, :-1] & op[:, 1:] & (np.abs(D[:, :-1] - D[:, 1:]) <= DEPTH_EDGE)
    ev = op[:-1, :] & op[1:, :] & (np.abs(D[:-1, :] - D[1:, :]) <= DEPTH_EDGE)
    return label_regions(op, eh, ev)


def compact(lab: np.ndarray):
    m = lab >= 0
    u, inv = np.unique(lab[m], return_inverse=True)
    out = lab.copy()
    out[m] = inv
    return out, len(u)


def region_stats(lab, D, K):
    m = lab >= 0
    l = lab[m]
    ys, xs = np.nonzero(m)
    cnt = np.bincount(l, minlength=K).astype(float)
    md = np.bincount(l, weights=D[m], minlength=K) / cnt
    mx = np.bincount(l, weights=xs, minlength=K) / cnt
    my = np.bincount(l, weights=ys, minlength=K) / cnt
    return cnt, md, mx, my


def adjacency(lab):
    pairs = set()
    for a, b in ((lab[:, :-1], lab[:, 1:]), (lab[:-1, :], lab[1:, :])):
        m = (a >= 0) & (b >= 0) & (a != b)
        for x, y in zip(a[m].tolist(), b[m].tolist()):
            pairs.add((min(x, y), max(x, y)))
    return pairs


def merge_into(lab, r, t):
    """Merge region r into region t; returns the compacted labels and the new region count."""
    out = lab.copy()
    out[out == r] = t
    return compact(out)


def merge_regions(lab, D, n_op, max_regions=MAX_REGIONS):
    """Merge small regions, then reduce to at most max_regions. Returns (lab, K, fallbacks)."""
    lab, K = compact(lab)
    thr = max(SMALL_MIN_PX, SMALL_FRAC * n_op)
    while K > 1:
        cnt, md, mx, my = region_stats(lab, D, K)
        small = np.nonzero(cnt < thr)[0]
        if len(small) == 0:
            break
        r = int(small[np.argmin(cnt[small])])
        neighbours = [b if a == r else a for a, b in adjacency(lab) if r in (a, b)]
        if neighbours:
            t = min(neighbours, key=lambda q: abs(md[r] - md[q]))
        else:
            t = min((q for q in range(K) if q != r),
                    key=lambda q: (mx[q] - mx[r]) ** 2 + (my[q] - my[r]) ** 2)
        lab, K = merge_into(lab, r, t)
    fallbacks = 0
    while K > max_regions:
        _, md, _, _ = region_stats(lab, D, K)
        pairs = adjacency(lab)
        if not pairs:
            # no two regions touch (only possible when they are separate pieces): merge the
            # pair with the closest mean depth anyway, and count it
            fallbacks += 1
            pairs = {(a, b) for a in range(K) for b in range(a + 1, K)}
        a, b = min(pairs, key=lambda p: abs(md[p[0]] - md[p[1]]))
        lab, K = merge_into(lab, b, a)
    return lab, K, fallbacks


# ----------------------------------------------------------------------------------------------
# Bends pass (castles: one region whose depth changes gradually)
# ----------------------------------------------------------------------------------------------


def smooth_depth(op: np.ndarray, D: np.ndarray) -> np.ndarray:
    """Normalised convolution: Gaussian(D * mask) / Gaussian(mask), so only opaque pixels count."""
    m = op.astype(np.float32)
    num = cv2.GaussianBlur(D.astype(np.float32) * m, (0, 0), BENDS_SIGMA)
    den = cv2.GaussianBlur(m, (0, 0), BENDS_SIGMA)
    return num / np.maximum(den, 1e-6)


def bends_regions(op: np.ndarray, D: np.ndarray, n_op: int):
    """Cut a one-region sprite at its bends. Returns (lab, K, k) with k the chosen cluster count,
    or None when no K in BENDS_K_MIN..BENDS_K_MAX is valid (the sprite keeps one region)."""
    ds = smooth_depth(op, D)
    gx = cv2.Sobel(ds, cv2.CV_32F, 1, 0, ksize=3)
    gy = cv2.Sobel(ds, cv2.CV_32F, 0, 1, ksize=3)
    s = max(float(np.median(np.hypot(gx, gy)[op])), 1e-6)  # the median gradient, no division by 0
    X = np.stack([2.0 * ds[op], gx[op] / s, gy[op] / s], axis=1).astype(np.float64)
    sample = np.random.default_rng(BENDS_SEED).choice(n_op, min(BENDS_SAMPLE, n_op), replace=False)
    best = None
    for k in range(BENDS_K_MIN, BENDS_K_MAX + 1):
        lbl = KMeans(n_clusters=k, n_init=10, random_state=0).fit(X).labels_
        if np.bincount(lbl, minlength=k).min() < BENDS_MIN_CLUSTER_FRAC * n_op:
            continue
        ls = lbl[sample]
        if len(np.unique(ls)) < 2:
            continue
        score = float(silhouette_score(X[sample], ls))
        if best is None or score > best[0]:
            best = (score, k, lbl)
    if best is None:
        return None
    _, k, lbl = best
    lc = np.full(op.shape, -1, np.int64)
    lc[op] = lbl
    eh = op[:, :-1] & op[:, 1:] & (lc[:, :-1] == lc[:, 1:])
    ev = op[:-1, :] & op[1:, :] & (lc[:-1, :] == lc[1:, :])
    lab = label_regions(op, eh, ev)  # 4-connected pieces of one cluster
    lab, K, _ = merge_regions(lab, D, n_op, BENDS_MAX_REGIONS)
    return lab, K, k


def is_bends_candidate(section: int, n_op: int, D: np.ndarray, op: np.ndarray) -> bool:
    if section != SECTION_BUILDING or n_op < BENDS_MIN_OPAQUE:
        return False
    p5, p95 = np.percentile(D[op], [5, 95])
    return float(p95 - p5) >= BENDS_MIN_RANGE


# ----------------------------------------------------------------------------------------------
# Feet, polygons, cards
# ----------------------------------------------------------------------------------------------


def feet(lab, D, K):
    """Foot (bx, y_true) of each region, following the spec's ground fit."""
    h, _ = lab.shape
    rows = []
    for r in range(K):
        ys, xs = np.nonzero(lab == r)
        bottom = int(ys.max())
        near = ys >= bottom - 2
        bx = float(xs[near].mean())
        dlow = float(D[ys[near], xs[near]].mean())
        sel = ys >= bottom - 1
        ysel, xsel = ys[sel], xs[sel]
        below = ysel + 1 < h
        other = np.zeros(len(ysel), bool)
        lb = lab[ysel[below] + 1, xsel[below]]
        other[below] = (lb >= 0) & (lb != r)
        occluded = bool(other.mean() >= OCCLUDED_FRAC)
        rows.append(dict(bottom=bottom, bx=bx, dlow=dlow, occluded=occluded))

    pts = [(q["bottom"], q["dlow"]) for q in rows if not q["occluded"]]
    fit = None
    if len(pts) >= 2:
        x = np.array([p[0] for p in pts], float)
        y = np.array([p[1] for p in pts], float)
        sxx = float(((x - x.mean()) ** 2).sum())
        if sxx > 0:
            b = float(((x - x.mean()) * (y - y.mean())).sum()) / sxx
            a = float(y.mean() - b * x.mean())
            if b > 0:
                fit = (a, b)

    out = []
    for q in rows:
        y_true = float(q["bottom"])
        if q["occluded"] and fit is not None:
            a, b = fit
            y_true = min(max((q["dlow"] - a) / b, q["bottom"]), h - 1)
        out.append((round(q["bx"], 1), round(y_true, 1)))
    return out


def card_grown(mask: np.ndarray, op=None) -> np.ndarray:
    """The card's area as a 0/255 mask. Plain regions: the region dilated by 1 px.
    Bends regions (op given): dilated by BENDS_REACH px (a 9x9 square), clipped to the sprite's
    silhouette (opaque dilated by 1 px), so a card overlaps its neighbour across a cut."""
    m = mask.astype(np.uint8) * 255
    if op is None:
        return cv2.dilate(m, np.ones((3, 3), np.uint8))
    side = 2 * BENDS_REACH + 1
    reach = cv2.dilate(m, np.ones((side, side), np.uint8))
    silhouette = cv2.dilate(op.astype(np.uint8) * 255, np.ones((3, 3), np.uint8))
    return reach & silhouette


def region_card(grown: np.ndarray):
    """Polygon and triangles of one card area (a 0/255 mask): (verts, tris) as flat int lists,
    or None."""
    contours, hier = cv2.findContours(grown, cv2.RETR_CCOMP, cv2.CHAIN_APPROX_SIMPLE)
    if hier is None:
        return None
    hier = hier[0]
    verts, tris = [], []
    for i, c in enumerate(contours):
        if hier[i][3] != -1:
            continue  # a hole, handled with its outer contour
        outer = cv2.approxPolyDP(c, SIMPLIFY_EPS, True).reshape(-1, 2)
        if len(outer) < 3:
            continue
        rings = [outer]
        for j in range(len(contours)):
            if hier[j][3] == i:
                hole = cv2.approxPolyDP(contours[j], SIMPLIFY_EPS, True).reshape(-1, 2)
                if len(hole) >= 3:
                    rings.append(hole)
        flat = np.concatenate(rings).astype(np.float64)
        ends = np.cumsum([len(r) for r in rings]).astype(np.uint32)
        t = mapbox_earcut.triangulate_float64(flat, ends).astype(int)
        if len(t) == 0:
            continue
        base = len(verts) // 2
        verts.extend(flat.astype(int).reshape(-1).tolist())
        tris.extend((t + base).tolist())
    if not tris:
        return None
    return verts, tris


def build_cards(lab, D, K, op=None):
    """Cards sorted back to front (foot y ascending). Each: dict(foot, verts, tris, region).
    op given: the bends pass's overlapping card areas (see card_grown)."""
    cards = []
    for r, (fx, fy) in enumerate(feet(lab, D, K)):
        got = region_card(card_grown(lab == r, op))
        if got is None:
            continue
        verts, tris = got
        cards.append({"foot": [fx, fy], "verts": verts, "tris": tris, "region": r})
    cards.sort(key=lambda c: (c["foot"][1], c["foot"][0]))
    return cards


# ----------------------------------------------------------------------------------------------
# Output and previews (scratch only)
# ----------------------------------------------------------------------------------------------


def write_json(path: Path, sprites: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    keys = sorted(sprites)
    lines = [f'{{"version":1,"model":{json.dumps(MODEL)},"alpha_opaque":{ALPHA_OPAQUE},"sprites":{{']
    for i, k in enumerate(keys):
        body = json.dumps(sprites[k], separators=(",", ":"))
        lines.append(f"{json.dumps(k)}:{body}" + ("," if i < len(keys) - 1 else ""))
    lines.append("}}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def tints(n):
    out = []
    for i in range(n):
        r, g, b = colorsys.hsv_to_rgb((i / max(n, 1) + 0.07) % 1.0, 0.75, 1.0)
        out.append((int(r * 255), int(g * 255), int(b * 255)))
    return out


def draw_cards(img, cards, scale, ox=0.0, oy=0.0):
    """Triangle outlines and red foot crosses, in sprite pixel coordinates times scale."""
    d = ImageDraw.Draw(img)
    for c in cards:
        v = c["verts"]
        pts = [(ox + (v[2 * i] + 0.5) * scale, oy + (v[2 * i + 1] + 0.5) * scale) for i in range(len(v) // 2)]
        t = c["tris"]
        for j in range(0, len(t), 3):
            d.line([pts[t[j]], pts[t[j + 1]], pts[t[j + 2]], pts[t[j]]], fill=(0, 0, 0), width=1)
        cx = ox + (c["foot"][0] + 0.5) * scale
        cy = oy + (c["foot"][1] + 0.5) * scale
        d.line([(cx - 4, cy - 4), (cx + 4, cy + 4)], fill=(255, 0, 0), width=2)
        d.line([(cx - 4, cy + 4), (cx + 4, cy - 4)], fill=(255, 0, 0), width=2)


def tinted_composite(rgba, lab, K) -> np.ndarray:
    """The sprite over grey, each region tinted with its own colour."""
    comp = composite_rgb(rgba)
    tinted = comp.copy()
    cols = tints(K)
    for r in range(K):
        m = lab == r
        tinted[m] = (0.45 * np.array(cols[r]) + 0.55 * comp[m]).astype(np.uint8)
    return tinted


def preview_sprite(path: Path, rgba, D, lab, K, cards) -> None:
    h, w = lab.shape
    scale = max(1, min(8, 240 // max(w, h)))
    base = Image.fromarray(composite_rgb(rgba)).resize((w * scale, h * scale), Image.Resampling.NEAREST)
    op = rgba[..., 3] >= ALPHA_OPAQUE
    dg = np.where(op, np.clip(D * 255, 0, 255), GREY).astype(np.uint8)
    depth = Image.fromarray(np.stack([dg] * 3, -1)).resize((w * scale, h * scale), Image.Resampling.NEAREST)
    regions = Image.fromarray(tinted_composite(rgba, lab, K)).resize((w * scale, h * scale), Image.Resampling.NEAREST)
    draw_cards(regions, cards, scale)
    gap = 6
    sheet = Image.new("RGB", (3 * w * scale + 2 * gap, h * scale), (255, 255, 255))
    sheet.paste(base, (0, 0))
    sheet.paste(depth, (w * scale + gap, 0))
    sheet.paste(regions, (2 * (w * scale + gap), 0))
    sheet.save(path)


def contact_sheet(path: Path, items, cell=220, cols=12) -> None:
    """items: (w, h, rgb tile as an (h, w, 3) uint8 array, cards)."""
    rows = max(1, math.ceil(len(items) / cols))
    sheet = Image.new("RGB", (cols * cell, rows * cell), (60, 60, 60))
    for i, (w, h, rgb, cards) in enumerate(items):
        s = min(4.0, 200.0 / max(w, h))
        tw, th = max(1, round(w * s)), max(1, round(h * s))
        tile = Image.fromarray(rgb).resize((tw, th), Image.Resampling.NEAREST)
        ox, oy = (cell - tw) // 2, (cell - th) // 2
        sheet.paste(tile, ((i % cols) * cell + ox, (i // cols) * cell + oy))
        draw_cards(sheet, cards, s, (i % cols) * cell + ox, (i // cols) * cell + oy)
    sheet.save(path)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", required=True, type=Path, help="the data file, e.g. data/cards/objects.json")
    ap.add_argument("--preview", type=Path, help="scratch dir for previews (never committed)")
    ap.add_argument("installs", nargs="+", type=Path, help="install folders holding Graphics/Objects/Objects.ugs")
    args = ap.parse_args()
    t0 = time.time()

    sprites = {}  # key -> (section, w, h, rgba), first occurrence wins
    per_install = []
    for inst in args.installs:
        data = (inst / OBJECTS).read_bytes()
        count = 0
        for section, w, h, rgba in decode_objects(data):
            key = sprite_key(w, h, rgba)
            count += 1
            sprites.setdefault(key, (section, w, h, rgba))
        per_install.append((inst.name, count))
    print(f"read {len(sprites)} unique sprites from {', '.join(f'{n} ({c})' for n, c in per_install)}")

    torch, proc, model = load_model()
    out = {}
    depth_run = skipped_small = one_region = fallback_sprites = 0
    histogram = Counter()
    bends_eligible = bends_no_k = 0
    bends_k = Counter()  # the K the silhouette chose
    bends_histogram = Counter()  # cards per bends-cut sprite
    bends_items = {}  # key -> (w, h, rgba, lab, K, cards), for the bends contact sheet
    if args.preview:
        args.preview.mkdir(parents=True, exist_ok=True)

    for key in sorted(sprites):
        section, w, h, rgba = sprites[key]
        if w == 0 or h == 0:
            skipped_small += 1
            continue
        op = rgba[..., 3] >= ALPHA_OPAQUE
        n_op = int(op.sum())
        if n_op < MIN_OPAQUE:
            skipped_small += 1
            continue
        depth_run += 1
        dep = predict_depth(torch, proc, model, rgba, w, h)
        vals = dep[op]
        lo, hi = float(vals.min()), float(vals.max())
        D = (dep - lo) / (hi - lo) if hi > lo else np.zeros_like(dep)
        lab0 = find_regions(op, D)
        lab, K, fallbacks = merge_regions(lab0, D, n_op)
        if fallbacks:
            fallback_sprites += 1
        bent = False
        if K == 1 and is_bends_candidate(section, n_op, D, op):
            bends_eligible += 1
            got = bends_regions(op, D, n_op)
            if got is None:
                bends_no_k += 1
            else:
                lab, K, k = got
                bends_k[k] += 1
                bent = True
        if K < 2:
            one_region += 1
            continue
        cards = build_cards(lab, D, K, op if bent else None)
        if len(cards) < 2:
            one_region += 1
            continue
        histogram[len(cards)] += 1
        out[key] = {
            "w": w,
            "h": h,
            "cards": [{"foot": c["foot"], "verts": c["verts"], "tris": c["tris"]} for c in cards],
        }
        if bent:
            bends_histogram[len(cards)] += 1
            bends_items[key] = (w, h, rgba, lab, K, out[key]["cards"])
        if args.preview:
            preview_sprite(args.preview / f"{key}_n{len(cards)}.png", rgba, D, lab, K, cards)

    write_json(args.out, out)
    if args.preview:
        buildings = [k for k in sorted(sprites) if sprites[k][0] == SECTION_BUILDING]
        items = [(sprites[k][1], sprites[k][2], composite_rgb(sprites[k][3]),
                  out[k]["cards"] if k in out else []) for k in buildings]
        if items:
            contact_sheet(args.preview / "contact_sheet.png", items)
        if bends_items:
            items = [(w, h, tinted_composite(rgba, lab, K), cards)
                     for w, h, rgba, lab, K, cards in bends_items.values()]
            contact_sheet(args.preview / "bends_sheet.png", items)

    total_cards = sum(len(v["cards"]) for v in out.values())
    print(f"sprites processed (unique): {len(sprites)}")
    print(f"  skipped, under {MIN_OPAQUE} opaque px: {skipped_small}")
    print(f"  depth run: {depth_run}")
    print(f"  one region (no entry): {one_region}")
    print(f"  sprites with cards: {len(out)}, cards: {total_cards}")
    print(f"  sprites where a merge had to join non-touching regions: {fallback_sprites}")
    print("cards histogram (cards per sprite: sprites):",
          ", ".join(f"{k}: {v}" for k, v in sorted(histogram.items())))
    print(f"bends pass: eligible {bends_eligible}, no valid K {bends_no_k}, "
          f"cut {len(bends_items)} sprites")
    print("  chosen K histogram (K: sprites):",
          ", ".join(f"{k}: {v}" for k, v in sorted(bends_k.items())) or "none")
    print("  cards histogram of the bends-cut sprites (cards: sprites):",
          ", ".join(f"{k}: {v}" for k, v in sorted(bends_histogram.items())) or "none")
    size = args.out.stat().st_size
    print(f"data file: {args.out} ({size / 1024:.1f} KiB)")
    if args.preview:
        print(f"previews: {args.preview}")
    print(f"runtime: {time.time() - t0:.1f} s")


if __name__ == "__main__":
    main()
