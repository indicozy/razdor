#!/usr/bin/env python3
"""Apply the trained ground classifier to the cards data. Offline; numbers only (docs/flows/ground-classifier.md).

For every sprite that is not owner-checked: each pixel's ground probability from the model
(`ground_model.json`, the features of `ground_features.py`); each card's flag is set by the majority of its
opaque pixels (ground when at least half are). Polygons, feet and the card order stay as they are. A checked
sprite (in the flips file) keeps the owner's labels. A card with no opaque pixel keeps its flag.

Usage:
  apply_ground.py --data data/cards/objects.json --model data/cards/ground_model.json \
                  --sprites <dir of <hash>.png> --onnx <model.onnx> [--flips flips.json] --out <data.json>
"""

import argparse
import json
from pathlib import Path

import cv2
import numpy as np
import onnxruntime as ort
from PIL import Image

import ground_features as gf
import train_ground as tg


def probability(model, feats):
    mean = np.array(model["mean"], np.float32)
    scale = np.array(model["scale"], np.float32)
    w = np.array(model["weights"], np.float32)
    z = ((feats - mean) / scale) @ w + model["bias"]
    return 1.0 / (1.0 + np.exp(-z))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--data", required=True, type=Path)
    ap.add_argument("--model", required=True, type=Path)
    ap.add_argument("--sprites", required=True, type=Path)
    ap.add_argument("--onnx", required=True, type=Path)
    ap.add_argument("--flips", type=Path, default=None)
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args()
    data = json.loads(args.data.read_text())
    model = json.loads(args.model.read_text())
    if model["features"] != gf.FEATURE_NAMES:
        raise SystemExit("the model's feature list differs from ground_features.FEATURE_NAMES")
    checked = tg.load_checked(args.flips) if args.flips else {}
    session = ort.InferenceSession(str(args.onnx), providers=["CPUExecutionProvider"])
    changed = kept = 0
    for key, sprite in data["sprites"].items():
        if key in checked:
            kept += 1
            continue
        path = args.sprites / f"{key}.png"
        if not path.exists():
            continue
        rgba = np.asarray(Image.open(path).convert("RGBA"))
        feats, op = gf.features(rgba, gf.run_depth(session, rgba))
        prob = np.zeros(op.shape, np.float32)
        prob[op] = probability(model, feats[op])
        for c in sprite["cards"]:
            m = np.zeros(op.shape, np.uint8)
            verts = np.array(c["verts"], np.float64).reshape(-1, 2)
            for a, b, d in np.array(c["tris"], np.int64).reshape(-1, 3):
                cv2.fillPoly(m, [np.round(verts[[a, b, d]]).astype(np.int32)], 1)
            inside = m.astype(bool) & op
            if not inside.any():
                continue
            ground = bool((prob[inside] >= 0.5).mean() >= 0.5)
            if ground != c["ground"]:
                changed += 1
            c["ground"] = ground
    args.out.write_text(json.dumps(data, separators=(",", ":")) + "\n", encoding="utf-8")
    print(f"sprites kept (owner-checked): {kept}; cards whose flag changed: {changed}; wrote {args.out}")


if __name__ == "__main__":
    main()
