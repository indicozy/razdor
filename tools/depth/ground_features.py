#!/usr/bin/env python3
"""Per-pixel features of a building sprite for the ground classifier. Only what the game can compute.

The inputs are the sprite's RGBA pixels and the built-in depth model (Depth Anything V2 Small, the shape-fixed
ONNX `depth-anything-v2-small-fp16-266.onnx` that `tools/depth/fix_onnx.py` makes; the engine loads the same
bytes). The depth is run the way the engine runs it (`src/depth/model.rs`): the sprite composited over grey 128,
resized to 266x266 (Catmull-Rom), normalised with ImageNet's mean and deviation, channels first; the output is
resized back to the sprite's size (bilinear, align_corners off).

The features (FEATURE_NAMES, in this order; the runtime must compute them in this order):
  depth      the depth normalised over the opaque pixels to 0..1
  grad_x     the depth's horizontal gradient (per sprite pixel)
  grad_y     the depth's vertical gradient
  n_x n_y n_z   the surface normal from the gradients, (-grad_x*K, -grad_y*K, 1) normalised, K = NORMAL_K
  r g b      the colour, 0..1
  sat val    HSV saturation and value, 0..1
  hue_sin hue_cos   the hue as a circle (0 for grey)
  edge       the distance to the silhouette's edge, min(d, EDGE_PX) / EDGE_PX
  in_island  1 when the pixel is in an island (the opaque pixels after a 3x3 opening, alpha-connected)
  island_y   the island's relative height of the pixel: (y - top) / (bottom - top + 1); 0 outside an island
  height     the pixel's height above its island's base: (bottom - y) / sprite height; 0 outside an island
  island_size  the island's area over the sprite's area, at most 1

Stdlib + numpy + cv2 + onnxruntime + PIL only (no torch).
"""

import numpy as np

UPSCALE_SIZE = 266
NORMAL_K = 8.0
EDGE_PX = 8.0
ALPHA_OPAQUE = 204
FEATURE_NAMES = [
    "depth", "grad_x", "grad_y", "n_x", "n_y", "n_z", "r", "g", "b", "sat", "val",
    "hue_sin", "hue_cos", "edge", "in_island", "island_y", "height", "island_size",
]
MEAN = np.array([0.485, 0.456, 0.406], np.float32)
STD = np.array([0.229, 0.224, 0.225], np.float32)


def composite_grey(rgba):
    a = rgba[..., 3:4].astype(np.float32) / 255.0
    rgb = rgba[..., :3].astype(np.float32) * a + 128.0 * (1.0 - a)
    return np.clip(np.rint(rgb), 0, 255).astype(np.uint8)


def run_depth(session, rgba):
    """The raw depth at the sprite's size (float32, h x w), as the engine computes it."""
    import cv2
    from PIL import Image

    h, w = rgba.shape[:2]
    img = Image.fromarray(composite_grey(rgba)).resize((UPSCALE_SIZE, UPSCALE_SIZE), Image.Resampling.BICUBIC)
    x = (np.asarray(img, np.float32) / 255.0 - MEAN) / STD
    x = x.transpose(2, 0, 1)[None].astype(np.float32)
    name = session.get_inputs()[0].name
    out = session.run(None, {name: x})[0]
    depth = np.asarray(out, np.float32).reshape(UPSCALE_SIZE, UPSCALE_SIZE)
    return cv2.resize(depth, (w, h), interpolation=cv2.INTER_LINEAR)


def islands(op):
    """Island labels (0 = none) of the opaque pixels after a 3x3 opening, and each island's bounding rows and area."""
    import cv2

    opened = cv2.morphologyEx(op.astype(np.uint8), cv2.MORPH_OPEN, np.ones((3, 3), np.uint8))
    n, lab = cv2.connectedComponents(opened, connectivity=4)
    info = {}
    for i in range(1, n):
        ys = np.nonzero((lab == i).any(axis=1))[0]
        info[i] = (int(ys[0]), int(ys[-1]), int((lab == i).sum()))
    return lab, info


def features(rgba, depth):
    """(features, opaque mask): the feature matrix (h, w, len(FEATURE_NAMES)) float32, zero outside the silhouette."""
    import cv2

    h, w = rgba.shape[:2]
    op = rgba[..., 3] >= ALPHA_OPAQUE
    lo, hi = float(depth[op].min()), float(depth[op].max())
    d = np.where(op, (depth - lo) / (hi - lo) if hi > lo else 0.0, 0.0).astype(np.float32)
    gy, gx = np.gradient(d)
    nz = np.ones_like(d)
    nx, ny = -gx * NORMAL_K, -gy * NORMAL_K
    norm = np.sqrt(nx * nx + ny * ny + nz * nz)
    nx, ny, nz = nx / norm, ny / norm, nz / norm
    rgb = rgba[..., :3].astype(np.float32) / 255.0
    mx, mn = rgb.max(axis=2), rgb.min(axis=2)
    span = np.maximum(mx - mn, 1e-6)
    sat = np.where(mx > 0, (mx - mn) / np.maximum(mx, 1e-6), 0.0)
    hue = np.where(mx == rgb[..., 0], ((rgb[..., 1] - rgb[..., 2]) / span) % 6,
                   np.where(mx == rgb[..., 1], (rgb[..., 2] - rgb[..., 0]) / span + 2, (rgb[..., 0] - rgb[..., 1]) / span + 4))
    hue = np.where(mn == mx, 0.0, hue) * np.pi / 3.0
    dist = cv2.distanceTransform(op.astype(np.uint8), cv2.DIST_L2, 3)
    edge = np.minimum(dist, EDGE_PX) / EDGE_PX
    lab, info = islands(op)
    in_island = (lab > 0).astype(np.float32)
    island_y = np.zeros((h, w), np.float32)
    height = np.zeros((h, w), np.float32)
    size = np.zeros((h, w), np.float32)
    yy = np.arange(h, dtype=np.float32)[:, None] * np.ones((1, w), np.float32)
    for i, (top, bottom, area) in info.items():
        m = lab == i
        island_y[m] = (yy[m] - top) / (bottom - top + 1)
        height[m] = (bottom - yy[m]) / h
        size[m] = min(area / float(h * w), 1.0)
    feats = np.stack([
        d, gx, gy, nx, ny, nz, rgb[..., 0], rgb[..., 1], rgb[..., 2], sat, mx,
        np.sin(hue), np.cos(hue), edge, in_island, island_y, height, size,
    ], axis=-1).astype(np.float32)
    feats[~op] = 0.0
    return feats, op
