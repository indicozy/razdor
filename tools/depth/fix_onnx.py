#!/usr/bin/env python3
"""Make the model file the engine loads: Depth Anything V2 Small, fp16 ONNX, shape-fixed. Offline,
once per release; the output is the file the release ships (see tools/depth/README.md for the
download and the sha256). Shape only: the weights stay fp16 (about 50 MB), and the engine upcasts
them to fp32 when it loads the file (src/depth/model.rs).

Two shape-only edits, both needed by tract 0.23 (the export's symbolic dims block it):
  1. the optional value_info is dropped (its dims like floor(height / 14) make tract fail);
  2. the input becomes 1x3xSIZExSIZE (default 266); the output stays symbolic and tract infers it.

Usage: fix_onnx.py <model_fp16.onnx> <out.onnx> [--size 266]
Prints the sha256 of the output. Needs the `onnx` package (any recent version).
"""

import argparse
import hashlib
import sys

import onnx
from onnx import TensorProto


def fix(src, dst, size):
    m = onnx.load(src)
    g = m.graph

    del g.value_info[:]  # 1: the symbolic value_info blocks tract

    inp = g.input[0].type.tensor_type
    dims = inp.shape.dim
    del dims[:]
    for v in (1, 3, size, size):  # 2: the fixed input shape
        dims.add().dim_value = v

    onnx.save(m, dst)
    fp16 = sum(t.data_type == TensorProto.FLOAT16 for t in g.initializer)
    print(f"input 1x3x{size}x{size}; fp16 initializers kept for the engine to upcast: {fp16}", file=sys.stderr)


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("src")
    ap.add_argument("dst")
    ap.add_argument("--size", type=int, default=266)
    args = ap.parse_args()
    fix(args.src, args.dst, args.size)
    print(f"sha256 {sha256(args.dst)}  {args.dst}")


if __name__ == "__main__":
    main()
