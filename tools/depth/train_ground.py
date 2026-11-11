#!/usr/bin/env python3
"""Train the ground classifier on the owner's labels. Offline; writes numbers only (see docs/flows/ground-classifier.md).

Labels: the owner's flips (`flips.json` from the click tool, `{hash: {flips: [...], checked: true}}`) over the cards
of `data/cards/objects.json`. Only the checked sprites are used. A pixel's label is the effective ground flag of the
last card (in index order) whose polygon covers it: the card's flag, inverted when the owner flipped it.

Model: standardised features (ground_features.FEATURE_NAMES) into one logistic regression, 19 numbers. The held-out
split is by sprite: every HOLDOUT_EVERY-th checked sprite (by hash order) is the test set.

Usage:
  train_ground.py --flips flips.json --data data/cards/objects.json --sprites <dir of <hash>.png> \
                  --onnx <depth-anything-v2-small-fp16-266.onnx> --out data/cards/ground_model.json
"""

import argparse
import json
from pathlib import Path

import cv2
import numpy as np
import onnxruntime as ort
from PIL import Image
from sklearn.linear_model import LogisticRegression

import ground_features as gf

HOLDOUT_EVERY = 5


def label_map(cards, flips, op):
    """The owner's ground label per pixel (-1 = no card covers it), last card wins."""
    h, w = op.shape
    lab = np.full((h, w), -1, np.int8)
    for i, c in enumerate(cards):
        ground = bool(c["ground"]) != (i in flips)
        m = np.zeros((h, w), np.uint8)
        verts = np.array(c["verts"], np.float64).reshape(-1, 2)
        for a, b, d in np.array(c["tris"], np.int64).reshape(-1, 3):
            cv2.fillPoly(m, [np.round(verts[[a, b, d]]).astype(np.int32)], 1)
        lab[m.astype(bool)] = 1 if ground else 0
    return lab


def load_checked(flips_path, labels_json=None):
    """The owner-checked sprites: {hash: set of flipped card indices}. A sprite is checked in the labels index
    (labels.json, the click tool's server) or in the older flips file (flips.json, {hash: {flips, checked}})."""
    checked = {}
    if flips_path:
        raw = json.loads(Path(flips_path).read_text())
        for key, entry in raw.items():
            if isinstance(entry, dict) and entry.get("checked"):
                checked[key] = set(entry.get("flips", []))
    if labels_json and Path(labels_json).exists():
        for key, ok in json.loads(Path(labels_json).read_text()).items():
            if ok:
                checked.setdefault(key, set())
    return checked


def mask_labels(path, op):
    """The owner's pixel labels from a saved mask (the click tool's PNG, label = the R channel: 1 ground, 2 standing,
    0 none): 1 ground, 0 standing, -1 unlabelled; -1 outside the opaque pixels."""
    from PIL import Image

    r = np.asarray(Image.open(path).convert("RGBA"))[..., 0]
    lab = np.full(op.shape, -1, np.int8)
    lab[(r == 1) & op] = 1
    lab[(r == 2) & op] = 0
    return lab


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--flips", type=Path, default=None, help="the older flips.json (optional)")
    ap.add_argument("--data", required=True, type=Path)
    ap.add_argument("--sprites", required=True, type=Path)
    ap.add_argument("--onnx", required=True, type=Path)
    ap.add_argument("--labels", type=Path, default=None, help="the click tool's labels folder (<hash>.png)")
    ap.add_argument("--labels-json", type=Path, default=None, help="the click tool's labels.json (checked flags)")
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args()
    data = json.loads(args.data.read_text())["sprites"]
    checked = load_checked(args.flips, args.labels_json)
    keys = sorted(k for k in checked if k in data)
    if not keys:
        raise SystemExit("no owner-checked sprites in the flips file")
    session = ort.InferenceSession(str(args.onnx), providers=["CPUExecutionProvider"])
    sets = {}
    for key in keys:
        rgba = np.asarray(Image.open(args.sprites / f"{key}.png").convert("RGBA"))
        depth = gf.run_depth(session, rgba)
        feats, op = gf.features(rgba, depth)
        mask = args.labels / f"{key}.png" if args.labels else None
        if mask is not None and mask.exists():
            lab = mask_labels(mask, op)
        else:
            lab = label_map(data[key]["cards"], checked[key], op)
        use = op & (lab >= 0)
        sets[key] = (feats[use], lab[use].astype(int))
        print(f"{key}: {int(use.sum())} labelled pixels, ground {int(lab[use].sum())}", flush=True)
    test_keys = {k for i, k in enumerate(keys) if i % HOLDOUT_EVERY == 0}
    train_keys = [k for k in keys if k not in test_keys] or keys
    test_keys = test_keys or set(keys)
    X_train = np.concatenate([sets[k][0] for k in train_keys])
    y_train = np.concatenate([sets[k][1] for k in train_keys])
    mean = X_train.mean(axis=0)
    scale = X_train.std(axis=0) + 1e-6
    clf = LogisticRegression(C=1.0, max_iter=2000, class_weight="balanced")
    clf.fit((X_train - mean) / scale, y_train)
    weights = clf.coef_[0].astype(float)
    bias = float(clf.intercept_[0])
    print(f"train sprites {len(train_keys)}, pixels {len(y_train)}; test sprites {len(test_keys)}")
    for name, keys_set in (("train", train_keys), ("test", sorted(test_keys))):
        X = np.concatenate([sets[k][0] for k in keys_set])
        y = np.concatenate([sets[k][1] for k in keys_set])
        pred = (1 / (1 + np.exp(-(((X - mean) / scale) @ weights + bias))) > 0.5).astype(int)
        acc = float((pred == y).mean())
        g = y == 1
        s = y == 0
        print(f"{name}: pixels {len(y)}, accuracy {acc:.3f}, ground recall {float((pred[g] == 1).mean()):.3f}, "
              f"standing recall {float((pred[s] == 0).mean()):.3f}")
    print("per sprite (test):")
    for k in sorted(test_keys):
        X, y = sets[k]
        pred = (1 / (1 + np.exp(-(((X - mean) / scale) @ weights + bias))) > 0.5).astype(int)
        print(f"  {k}: accuracy {float((pred == y).mean()):.3f}")
    model = {
        "version": 1,
        "depth_model": "depth-anything-v2-small-fp16-266.onnx",
        "depth_model_sha256": "897f1b8788fdab97f0e75155e4fdc9a8e7605734f1cd31469a4ca6fb02ea8e74",
        "input": gf.UPSCALE_SIZE,
        "features": gf.FEATURE_NAMES,
        "mean": [round(float(v), 6) for v in mean],
        "scale": [round(float(v), 6) for v in scale],
        "weights": [round(v, 6) for v in weights],
        "bias": round(bias, 6),
        "threshold": 0.5,
        "trained_on": keys,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(model, indent=1) + "\n", encoding="utf-8")
    print("wrote", args.out)


if __name__ == "__main__":
    main()
