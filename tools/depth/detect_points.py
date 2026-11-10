#!/usr/bin/env python3
"""Find windows, doors, gates, chimneys, flags and mill sails on the building sprites. Offline,
numbers only.

For every building sprite (section 1 of Graphics/Objects/Objects.ugs) of every install, an
open-vocabulary detector (OWLv2, Apache-2.0) looks for the classes in QUERIES. The sprite is
composited over grey and upscaled first. The boxes that pass the filters and the per-class
thresholds become points in data/lights/objects.json, keyed by the sprite's pixel hash (the same
key as cut_sprites.py). No sprite, image or preview is ever written to the repo.

Usage (see tools/depth/README.md):
  detect_points.py --out data/lights/objects.json [--preview DIR] <install>...
"""

import argparse
import json
import math
import sys
import time
from collections import Counter
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

# The Objects.ugs decoder, the sprite key and the grey composite are cut_sprites.py's, shared.
sys.path.insert(0, str(Path(__file__).resolve().parent))
import cut_sprites as cs  # noqa: E402

MODEL = "google/owlv2-base-patch16-ensemble"
MODEL_LICENCE = "Apache-2.0"
UPSCALE = 4  # bicubic, before the detector (it then pads to a square and resizes to its own 960)
ALPHA_OPAQUE = cs.ALPHA_OPAQUE  # 204: a box's pixels count as opaque from this alpha on
# One detector class per query: (text query, kind).
QUERIES = [
    ("a window", "window"),
    ("a door", "door"),
    ("a castle gate", "gate"),
    ("a chimney", "chimney"),
    ("a flag", "flag"),
    ("windmill sails", "sails"),
]
KINDS = [kind for _, kind in QUERIES]
# Score cut per kind, set by reviewing the contact sheet (see the README section). OWLv2 scores
# windows low (its best window box is 0.27), so their cut is low; chimneys and flags are noisy.
THRESHOLDS = {"window": 0.10, "door": 0.10, "gate": 0.20, "chimney": 0.35, "flag": 0.35, "sails": 0.15}
CANDIDATE_MIN = 0.02  # raw boxes below this score are never considered
NMS_IOU = 0.5  # per class: boxes overlapping a higher-scoring one by more than this go
MIN_BOX_OPAQUE = 0.50  # share of a box's pixels that are opaque (alpha >= 204)
MAX_BOX_SHARE = 0.40  # a box larger than this share of the sprite is a false positive
MIN_BOX_SIDE = 2  # px

# Preview colours (scratch only), per kind.
COLOURS = {
    "window": (255, 215, 0),
    "door": (0, 200, 255),
    "gate": (255, 0, 255),
    "chimney": (255, 128, 0),
    "flag": (255, 40, 40),
    "sails": (60, 255, 60),
}


# ----------------------------------------------------------------------------------------------
# Detection
# ----------------------------------------------------------------------------------------------


def load_model():
    import torch
    from transformers import Owlv2ForObjectDetection, Owlv2Processor

    if not torch.cuda.is_available():
        raise SystemExit("CUDA is not visible: run with CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=<RTX index>")
    print(f"device: {torch.cuda.get_device_name(0)}")
    proc = Owlv2Processor.from_pretrained(MODEL)
    model = Owlv2ForObjectDetection.from_pretrained(MODEL).to("cuda").eval()
    return torch, proc, model


def raw_boxes(torch, proc, model, rgba: np.ndarray, w: int, h: int):
    """The detector's answer for one sprite: scores (N, Q) and boxes (N, 4) as x0, y0, x1, y1 in
    sprite pixels (floats, not clipped). The detector pads the upscaled image to a square at the
    bottom and right, so its normalised boxes map back with the sprite's longer side."""
    rgb = cs.composite_rgb(rgba)
    img = Image.fromarray(rgb).resize((w * UPSCALE, h * UPSCALE), Image.Resampling.BICUBIC)
    inputs = proc(text=[[q for q, _ in QUERIES]], images=img, return_tensors="pt")
    inputs = {k: v.to("cuda") for k, v in inputs.items()}
    with torch.no_grad():
        out = model(**inputs)
    scores = out.logits[0].sigmoid().float().cpu().numpy()  # (N, Q)
    cxcywh = out.pred_boxes[0].float().cpu().numpy()  # (N, 4), normalised to the padded square
    side = max(w, h)  # the padded square is UPSCALE * side px, i.e. side sprite px
    cx, cy, bw, bh = (cxcywh * side).T
    boxes = np.stack([cx - bw / 2, cy - bh / 2, cx + bw / 2, cy + bh / 2], axis=1)
    return scores, boxes


def nms(boxes: np.ndarray, scores: np.ndarray, iou_thr: float) -> list:
    """Greedy non-maximum suppression; returns the kept indices, best first."""
    order = np.argsort(-scores)
    x0, y0, x1, y1 = boxes.T
    areas = (x1 - x0) * (y1 - y0)
    keep = []
    while order.size:
        i = order[0]
        keep.append(int(i))
        rest = order[1:]
        ix = np.maximum(0, np.minimum(x1[i], x1[rest]) - np.maximum(x0[i], x0[rest]))
        iy = np.maximum(0, np.minimum(y1[i], y1[rest]) - np.maximum(y0[i], y0[rest]))
        inter = ix * iy
        iou = inter / np.maximum(areas[i] + areas[rest] - inter, 1e-9)
        order = rest[iou <= iou_thr]
    return keep


def candidates(scores: np.ndarray, boxes: np.ndarray, alpha: np.ndarray) -> list:
    """Filter the raw boxes of one sprite and keep, per class, the non-overlapping ones.
    Each candidate: {"kind", "score", "box": [x0, y0, x1, y1]} with integer sprite pixels."""
    h, w = alpha.shape
    opaque = alpha >= ALPHA_OPAQUE
    found = []
    for qi, kind in enumerate(KINDS):
        s = scores[:, qi]
        rows = []
        for i in np.nonzero(s >= CANDIDATE_MIN)[0]:
            x0 = min(max(int(math.floor(boxes[i, 0])), 0), w)
            y0 = min(max(int(math.floor(boxes[i, 1])), 0), h)
            x1 = min(max(int(math.ceil(boxes[i, 2])), 0), w)
            y1 = min(max(int(math.ceil(boxes[i, 3])), 0), h)
            if x1 - x0 < MIN_BOX_SIDE or y1 - y0 < MIN_BOX_SIDE:
                continue
            if (x1 - x0) * (y1 - y0) > MAX_BOX_SHARE * w * h:
                continue
            if opaque[y0:y1, x0:x1].mean() < MIN_BOX_OPAQUE:
                continue
            rows.append((i, [x0, y0, x1, y1]))
        if not rows:
            continue
        bx = np.array([b for _, b in rows], dtype=np.float64)
        sc = np.array([s[i] for i, _ in rows])
        for j in nms(bx, sc, NMS_IOU):
            found.append({"kind": kind, "score": float(sc[j]), "box": rows[j][1]})
    return found


def select(cands: list, thresholds: dict) -> list:
    """The points a sprite keeps: candidates at or above their kind's threshold, in a fixed order."""
    kept = [c for c in cands if c["score"] >= thresholds[c["kind"]]]
    kept.sort(key=lambda c: (KINDS.index(c["kind"]), c["box"][1], c["box"][0]))
    return kept


def collect_sprites(installs):
    """key -> (section, w, h, rgba), building sprites only (first occurrence wins), and the
    per-install counts of the building section."""
    sprites = {}
    per_install = []
    for inst in installs:
        data = (inst / cs.OBJECTS).read_bytes()
        count = 0
        for section, w, h, rgba in cs.decode_objects(data):
            if section != cs.SECTION_BUILDING:
                continue
            count += 1
            sprites.setdefault(cs.sprite_key(w, h, rgba), (section, w, h, rgba))
        per_install.append((inst.name, count))
    return sprites, per_install


# ----------------------------------------------------------------------------------------------
# Output
# ----------------------------------------------------------------------------------------------


def write_json(path: Path, sprites: dict) -> None:
    """One sprite per line, keys sorted. sprites: key -> {"w", "h", "points"}."""
    path.parent.mkdir(parents=True, exist_ok=True)
    keys = sorted(sprites)
    lines = [f'{{"version":1,"model":{json.dumps(MODEL)},"sprites":{{']
    for i, k in enumerate(keys):
        entry = sprites[k]
        points = [{"kind": p["kind"], "box": p["box"], "score": round(p["score"], 2)} for p in entry["points"]]
        body = json.dumps({"w": entry["w"], "h": entry["h"], "points": points}, separators=(",", ":"))
        lines.append(f"{json.dumps(k)}:{body}" + ("," if i < len(keys) - 1 else ""))
    lines.append("}}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def _font():
    try:
        return ImageFont.load_default(size=11)
    except TypeError:  # older Pillow
        return ImageFont.load_default()


def draw_points(img: Image.Image, points, scale: int, ox: int = 0, oy: int = 0, labels: bool = True) -> None:
    draw = ImageDraw.Draw(img)
    font = _font()
    for p in points:
        x0, y0, x1, y1 = p["box"]
        col = COLOURS[p["kind"]]
        rect = [ox + x0 * scale, oy + y0 * scale, ox + x1 * scale - 1, oy + y1 * scale - 1]
        draw.rectangle(rect, outline=col, width=max(1, scale // 2))
        if labels:
            text = f"{p['kind']} {p['score']:.2f}"
            tx, ty = rect[0], max(0, rect[1] - 12)
            tw = draw.textlength(text, font=font)
            draw.rectangle([tx, ty, tx + tw + 2, ty + 12], fill=(0, 0, 0))
            draw.text((tx + 1, ty), text, fill=col, font=font)


def sprite_preview(path: Path, rgba: np.ndarray, points, scale: int = 4) -> None:
    rgb = cs.composite_rgb(rgba)
    h, w = rgb.shape[:2]
    img = Image.fromarray(rgb).resize((w * scale, h * scale), Image.Resampling.NEAREST)
    draw_points(img, points, scale)
    img.save(path)


def contact_sheet(path: Path, items, scale: int = 2, cols: int = 8) -> None:
    """items: (key, w, h, rgba, points) for every building sprite, at `scale` times its size.
    Column widths and row heights are the largest tile in that column or row."""
    font = _font()
    gap, head = 10, 90
    rows = math.ceil(len(items) / cols)
    col_w = [0] * cols
    row_h = [0] * rows
    for i, (_, w, h, _, _) in enumerate(items):
        col_w[i % cols] = max(col_w[i % cols], w * scale + gap)
        row_h[i // cols] = max(row_h[i // cols], h * scale + gap + 14)
    width = sum(col_w)
    height = head + sum(row_h)
    sheet = Image.new("RGB", (width, height), (60, 60, 60))
    # legend
    draw = ImageDraw.Draw(sheet)
    total = Counter()
    sprites_with = Counter()
    for _, _, _, _, points in items:
        seen = set()
        for p in points:
            total[p["kind"]] += 1
            seen.add(p["kind"])
        for k in seen:
            sprites_with[k] += 1
    x = 10
    for kind in KINDS:
        draw.rectangle([x, 12, x + 14, 26], fill=COLOURS[kind])
        text = f"{kind}: {total[kind]} points on {sprites_with[kind]} sprites"
        draw.text((x + 20, 12), text, fill=(255, 255, 255), font=font)
        x += 20 + int(draw.textlength(text, font=font)) + 24
    draw.text((10, 42), f"{len(items)} building sprites, {scale}x, {cols} per row. Each tile is labelled "
              "with its key's first 6 hex digits.", fill=(230, 230, 230), font=font)
    y = head
    for r in range(rows):
        x = 0
        for c in range(cols):
            i = r * cols + c
            if i >= len(items):
                break
            key, w, h, rgba, points = items[i]
            rgb = cs.composite_rgb(rgba)
            tile = Image.fromarray(rgb).resize((w * scale, h * scale), Image.Resampling.NEAREST)
            tx, ty = x + gap // 2, y + 14
            sheet.paste(tile, (tx, ty))
            draw_points(sheet, points, scale, tx, ty)
            draw.text((tx, y + 1), key[:6], fill=(230, 230, 230), font=font)
            x += col_w[c]
        y += row_h[r]
    sheet.save(path)


# ----------------------------------------------------------------------------------------------


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", required=True, type=Path, help="the data file, e.g. data/lights/objects.json")
    ap.add_argument("--preview", type=Path, help="scratch dir for previews (never committed)")
    ap.add_argument("installs", nargs="+", type=Path, help="install folders holding Graphics/Objects/Objects.ugs")
    args = ap.parse_args()
    t0 = time.time()

    sprites, per_install = collect_sprites(args.installs)
    print(f"building sprites: {len(sprites)} unique ({', '.join(f'{n}: {c}' for n, c in per_install)})")

    torch, proc, model = load_model()
    results = {}  # key -> (w, h, rgba, candidates)
    for key in sorted(sprites):
        _, w, h, rgba = sprites[key]
        alpha = rgba[..., 3]
        scores, boxes = raw_boxes(torch, proc, model, rgba, w, h)
        results[key] = (w, h, rgba, candidates(scores, boxes, alpha))

    out = {}
    for key, (w, h, _, cands) in results.items():
        points = select(cands, THRESHOLDS)
        if points:
            out[key] = {"w": w, "h": h, "points": points}
    write_json(args.out, out)

    counts = Counter(p["kind"] for v in out.values() for p in v["points"])
    per_kind_sprites = Counter(k for v in out.values() for k in {p["kind"] for p in v["points"]})
    print(f"sprites with points: {len(out)} of {len(sprites)}")
    print("points per kind: " + (", ".join(f"{k}: {counts[k]} ({per_kind_sprites[k]} sprites)" for k in KINDS)))
    print(f"data file: {args.out} ({args.out.stat().st_size / 1024:.1f} KiB)")

    if args.preview:
        args.preview.mkdir(parents=True, exist_ok=True)
        for key, v in out.items():
            _, _, rgba, _ = results[key]
            sprite_preview(args.preview / f"{key}.png", rgba, v["points"])
        items = [(k, w, h, rgba, out[k]["points"] if k in out else []) for k, (w, h, rgba, _) in sorted(results.items())]
        contact_sheet(args.preview / "contact_sheet.png", items)
        print(f"previews: {args.preview}")
    print(f"runtime: {time.time() - t0:.1f} s")


if __name__ == "__main__":
    main()
