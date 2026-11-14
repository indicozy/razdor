#!/usr/bin/env python3
"""Find windows, doors, gates, chimneys, flags and mill sails on the building sprites. Offline,
numbers only (detector v2).

For every building sprite (section 1 of Graphics/Objects/Objects.ugs) of every install:
- Qwen3-VL-4B-Instruct (Apache-2.0) is asked per class for boxes (0-1000 coordinates) on the
  sprite composited over grey and upscaled x4. It gives doors, gates, chimneys, flags and sails.
- Florence-2-large (MIT) phrase grounding for "window" gives candidate wall-sized boxes. They are
  containers only: each one smaller than 30 % of the sprite is refined to the dark pane blobs
  inside it (classical, see window_blobs). Whole-wall boxes are dropped.
- Chimney boxes are kept only at the top of a roof (under 25 % of the sprite's height, centre in
  the upper 45 %); a box over 20 % of the sprite's area is dropped for every class.

The boxes become points in data/lights/objects.json, keyed by the sprite's pixel hash (the same
key as cut_sprites.py). No sprite, image or preview is ever written to the repo.

Usage (see tools/depth/README.md):
  detect_points.py --out data/lights/objects.json [--preview DIR] <install>...
"""

import argparse
import json
import math
import re
import sys
import time
from collections import Counter
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont
from scipy import ndimage

# The Objects.ugs decoder, the sprite key and the grey composite are cut_sprites.py's, shared.
sys.path.insert(0, str(Path(__file__).resolve().parent))
import cut_sprites as cs  # noqa: E402

MODEL_VLM = "Qwen/Qwen3-VL-4B-Instruct"
MODEL_VLM_LICENCE = "Apache-2.0"
MODEL_FLO = "microsoft/Florence-2-large"
MODEL_FLO_LICENCE = "MIT"
MODEL = f"{MODEL_VLM}+{MODEL_FLO}+dark-pane"
UPSCALE = 4  # bicubic, before the models
ALPHA_OPAQUE = cs.ALPHA_OPAQUE  # 204
KINDS = ["window", "door", "gate", "chimney", "flag", "sails"]
# Text for each class: the VLM prompt and the Florence phrase.
DESC = {
    "window": "every window (small openings with dark or warm-lit panes, on walls and roofs)",
    "door": "every door (doorways at the ground level)",
    "gate": "every castle gate or large archway of a wall",
    "chimney": "every chimney (a small chimney stack at the top of a roof; not the whole roof)",
    "flag": "every flag or banner",
    "sails": "every windmill sail (the blades of a windmill)",
}
PHRASE = {"window": "window", "door": "door", "gate": "castle gate", "chimney": "chimney",
          "flag": "flag", "sails": "windmill sails"}
VLM_KINDS = ["door", "gate", "chimney", "flag", "sails"]  # window comes from Florence + pixels
MAX_BOX_SHARE = 0.20  # a box over this share of the sprite's area is dropped (a wall, not a part)
CONTAINER_MAX_SHARE = 0.30  # a Florence window box over this share is a wall: no container
PANE_DARK = 35  # luminance below the box's wall median by this: a dark pane
PANE_MAX_SIDE = 14  # px
PANE_MIN_FILL = 0.5
PANE_MIN_AREA = 6

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
# Models
# ----------------------------------------------------------------------------------------------


def load_models():
    """Qwen3-VL-4B and Florence-2-large on the CUDA device the environment selects."""
    import torch
    from transformers import AutoProcessor, Qwen3VLForConditionalGeneration

    if not torch.cuda.is_available():
        raise SystemExit("CUDA is not visible: run with CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=<RTX index>")
    print(f"device: {torch.cuda.get_device_name(0)}")
    vlm = Qwen3VLForConditionalGeneration.from_pretrained(
        MODEL_VLM, dtype=torch.bfloat16, attn_implementation="sdpa").to("cuda").eval()
    vlm_proc = AutoProcessor.from_pretrained(MODEL_VLM)
    flo, flo_proc = load_florence(torch)
    return torch, vlm, vlm_proc, flo, flo_proc


def load_florence(torch):
    """Florence-2 ships remote code that imports flash_attn (not needed here): the import check
    skips it. The beam search and the KV cache do not work with transformers 4.57, so the
    generation runs greedy without the cache (short outputs, so it is fast enough)."""
    from unittest.mock import patch
    from transformers import AutoModelForCausalLM, AutoProcessor
    from transformers.dynamic_module_utils import get_imports

    def no_flash(filename):
        return [i for i in get_imports(filename) if i != "flash_attn"]

    with patch("transformers.dynamic_module_utils.get_imports", no_flash):
        model = AutoModelForCausalLM.from_pretrained(
            MODEL_FLO, trust_remote_code=True, dtype=torch.float16, attn_implementation="eager").to("cuda").eval()
    proc = AutoProcessor.from_pretrained(MODEL_FLO, trust_remote_code=True)
    return model, proc


def upscaled(rgba: np.ndarray) -> Image.Image:
    rgb = cs.composite_rgb(rgba)
    h, w = rgb.shape[:2]
    return Image.fromarray(rgb).resize((w * UPSCALE, h * UPSCALE), Image.Resampling.BICUBIC)


def vlm_boxes(torch, vlm, proc, img: Image.Image, kind: str, w: int, h: int) -> list:
    """Qwen3-VL's boxes for one class, in sprite pixels (its 0-1000 coordinates are relative to the
    upscaled image, which is the sprite scaled, so they map to the sprite's size)."""
    msgs = [{"role": "user", "content": [
        {"type": "image", "image": img},
        {"type": "text", "text": f"Locate {DESC[kind]} in this image. Answer only with JSON: a list of "
                                 "objects with keys \"label\" and \"bbox_2d\" ([x1,y1,x2,y2] in 0-1000 "
                                 "coordinates). If there are none, answer []."}]}]
    inputs = proc.apply_chat_template(msgs, tokenize=True, add_generation_prompt=True,
                                      return_dict=True, return_tensors="pt").to("cuda")
    with torch.no_grad():
        gen = vlm.generate(**inputs, max_new_tokens=300, do_sample=False)
    text = proc.batch_decode(gen[:, inputs["input_ids"].shape[1]:], skip_special_tokens=True)[0]
    m = re.search(r"\[.*\]", text, re.S)
    try:
        items = json.loads(m.group(0)) if m else []
    except ValueError:
        items = []
    boxes = []
    for it in items if isinstance(items, list) else []:
        b = it.get("bbox_2d") if isinstance(it, dict) else None
        if not b or len(b) != 4:
            continue
        x1, y1, x2, y2 = [float(v) / 1000.0 for v in b]
        boxes.append([x1 * w, y1 * h, x2 * w, y2 * h])
    return boxes


def florence_boxes(torch, flo, proc, img: Image.Image, kind: str, w: int, h: int) -> list:
    """Florence-2's phrase-grounding boxes for one class, in sprite pixels."""
    prompt = "<CAPTION_TO_PHRASE_GROUNDING>"
    inputs = proc(text=prompt + PHRASE[kind], images=img, return_tensors="pt").to("cuda", torch.float16)
    with torch.no_grad():
        gen = flo.generate(input_ids=inputs["input_ids"], pixel_values=inputs["pixel_values"],
                           max_new_tokens=256, num_beams=1, do_sample=False, use_cache=False)
    text = proc.batch_decode(gen, skip_special_tokens=False)[0]
    parsed = proc.post_process_generation(text, task=prompt, image_size=img.size)
    return [[v / UPSCALE for v in b] for b in parsed[prompt]["bboxes"]]


# ----------------------------------------------------------------------------------------------
# Candidates
# ----------------------------------------------------------------------------------------------


def luminance(rgba: np.ndarray) -> np.ndarray:
    rgb = rgba[..., :3].astype(np.float64)
    return 0.299 * rgb[..., 0] + 0.587 * rgb[..., 1] + 0.114 * rgb[..., 2]


def window_blobs(rgba: np.ndarray, box) -> list:
    """Dark pane blobs inside a container box: pixels darker than the box's opaque wall median by
    PANE_DARK. A blob is kept when it is small (at most PANE_MAX_SIDE a side), filled (at least
    PANE_MIN_FILL of its box) and not a sliver. Returns integer boxes in sprite pixels."""
    h, w = rgba.shape[:2]
    x0, y0 = max(0, int(math.floor(box[0]))), max(0, int(math.floor(box[1])))
    x1, y1 = min(w, int(math.ceil(box[2]))), min(h, int(math.ceil(box[3])))
    if x1 - x0 < 2 or y1 - y0 < 2:
        return []
    op = rgba[y0:y1, x0:x1, 3] >= ALPHA_OPAQUE
    if op.sum() < 20:
        return []
    sub = luminance(rgba)[y0:y1, x0:x1]
    dark = (sub < np.median(sub[op]) - PANE_DARK) & op
    lab, n = ndimage.label(dark)
    out = []
    for i, sl in enumerate(ndimage.find_objects(lab), 1):
        ys, xs = sl
        bw, bh = xs.stop - xs.start, ys.stop - ys.start
        area = int((lab[sl] == i).sum())
        if area < PANE_MIN_AREA or bw < 2 or bh < 2 or bw > PANE_MAX_SIDE or bh > PANE_MAX_SIDE:
            continue
        if area / float(bw * bh) < PANE_MIN_FILL or not (0.4 <= bw / bh <= 2.5):
            continue
        out.append([x0 + xs.start, y0 + ys.start, x0 + xs.stop, y0 + ys.stop])
    return out


def candidates(models, rgba: np.ndarray, w: int, h: int) -> list:
    """All points of one sprite before the output step: {"kind", "box", "score"}."""
    torch, vlm, vlm_proc, flo, flo_proc = models
    img = upscaled(rgba)
    area = w * h
    pts = []
    # Windows: Florence containers refined to dark panes.
    for fb in florence_boxes(torch, flo, flo_proc, img, "window", w, h):
        if (fb[2] - fb[0]) * (fb[3] - fb[1]) > CONTAINER_MAX_SHARE * area:
            continue
        for b in window_blobs(rgba, fb):
            pts.append({"kind": "window", "box": b, "score": 1.0})
    # The other classes: the VLM's boxes, with per-class rules.
    for kind in VLM_KINDS:
        for b in vlm_boxes(torch, vlm, vlm_proc, img, kind, w, h):
            x0, y0 = max(0, int(math.floor(b[0]))), max(0, int(math.floor(b[1])))
            x1, y1 = min(w, int(math.ceil(b[2]))), min(h, int(math.ceil(b[3])))
            bw, bh = x1 - x0, y1 - y0
            if bw < 2 or bh < 2 or bw * bh > MAX_BOX_SHARE * area:
                continue
            if kind == "chimney" and (bh > 0.25 * h or (y0 + y1) / 2 > 0.45 * h):
                continue
            pts.append({"kind": kind, "box": [x0, y0, x1, y1], "score": 1.0})
    return dedupe(pts)


def dedupe(pts: list) -> list:
    """Within a kind, drop a box that overlaps a kept one by more than half of its own area."""
    kept = []
    for p in sorted(pts, key=lambda p: (KINDS.index(p["kind"]), p["box"][1], p["box"][0])):
        dup = False
        for q in kept:
            if q["kind"] != p["kind"]:
                continue
            ix = max(0, min(p["box"][2], q["box"][2]) - max(p["box"][0], q["box"][0]))
            iy = max(0, min(p["box"][3], q["box"][3]) - max(p["box"][1], q["box"][1]))
            pa = (p["box"][2] - p["box"][0]) * (p["box"][3] - p["box"][1])
            if ix * iy > 0.5 * pa:
                dup = True
                break
        if not dup:
            kept.append(p)
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
            text = p["kind"]
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
    sheet = Image.new("RGB", (sum(col_w), head + sum(row_h)), (60, 60, 60))
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


OVERRIDES = Path(__file__).resolve().parent / "lights_overrides.json"


def apply_overrides(out: dict, path: Path) -> int:
    """Remove the points the owner's corrections name (`lights_overrides.json`): each entry is a
    sprite key, a kind and the exact box of the point to remove, with a short reason. A sprite left
    with no point has no entry. An entry that names no point is an error (a stale correction).
    Returns the number of points removed."""
    if not path.exists():
        return 0
    entries = json.loads(path.read_text(encoding="utf-8"))["overrides"]
    removed = 0
    for e in entries:
        entry = out.get(e["key"])
        hits = [p for p in entry["points"] if p["kind"] == e["kind"] and list(p["box"]) == list(e["box"])] if entry else []
        if len(hits) != 1:
            raise SystemExit(f"override matches {len(hits)} points: {e['key']} {e['kind']} {e['box']} ({e['reason']})")
        entry["points"].remove(hits[0])
        removed += 1
        if not entry["points"]:
            del out[e["key"]]
    print(f"overrides applied: {removed} points removed ({path})")
    return removed


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", required=True, type=Path, help="the data file, e.g. data/lights/objects.json")
    ap.add_argument("--preview", type=Path, help="scratch dir for previews (never committed)")
    ap.add_argument("--overrides", type=Path, default=OVERRIDES, help="the owner's corrections (default: lights_overrides.json)")
    ap.add_argument("--apply-overrides", action="store_true",
                    help="no models: apply the corrections to the existing --out file and rewrite it")
    ap.add_argument("installs", nargs="*", type=Path, help="install folders holding Graphics/Objects/Objects.ugs")
    args = ap.parse_args()
    t0 = time.time()

    if args.apply_overrides:
        raw = json.loads(args.out.read_text(encoding="utf-8"))["sprites"]
        out = {k: {"w": v["w"], "h": v["h"], "points": v["points"]} for k, v in raw.items()}
        apply_overrides(out, args.overrides)
        write_json(args.out, out)
        print(f"data file: {args.out} ({args.out.stat().st_size / 1024:.1f} KiB)")
        return
    if not args.installs:
        ap.error("installs are required unless --apply-overrides")

    sprites, per_install = collect_sprites(args.installs)
    print(f"building sprites: {len(sprites)} unique ({', '.join(f'{n}: {c}' for n, c in per_install)})")

    models = load_models()
    results = {}  # key -> (w, h, rgba, points)
    for key in sorted(sprites):
        _, w, h, rgba = sprites[key]
        results[key] = (w, h, rgba, candidates(models, rgba, w, h))
        print(f"{key[:6]} {w}x{h} {len(results[key][3])} points", flush=True)

    out = {}
    for key, (w, h, _, pts) in results.items():
        if pts:
            out[key] = {"w": w, "h": h, "points": pts}
    apply_overrides(out, args.overrides)
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
