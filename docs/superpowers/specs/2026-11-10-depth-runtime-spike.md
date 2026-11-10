# Depth runtime spike: findings (2026-11-10)

Research spike for `2026-11-10-atmosphere-design.md` §2: can the pure-Rust ONNX runtime `tract`
run Depth Anything V2 Small on the player's machine, how fast is it, and does it match PyTorch?

Short answer: yes. tract matches PyTorch closely (Pearson 0.9997 or better for fp32 and fp16 at
266 px end to end) and runs one 266 px sprite in about 0.3 to 0.5 s on one desktop thread with
tract 0.23. Two things need a decision first: tract 0.23 needs rustc 1.91 (the repo pins 1.90),
and the stock ONNX file must be prepared offline (fixed shape; see below). No engine code changed.
The spike project lives outside git (scratch folder `razdor-scratch/atmo-model/spike`).

## 1. Model and licence

| file (onnx-community/depth-anything-v2-small) | size | use |
|---|---|---|
| `onnx/model.onnx` (fp32) | 99.1 MB | reference, tested |
| `onnx/model_fp16.onnx` | 49.6 MB | shipping candidate (upcast at load) |
| `onnx/model_quantized.onnx` (int8) | 27.3 MB | tested, not recommended |
| `onnx/model_q4.onnx`, `model_q4f16.onnx`, `model_bnb4.onnx` | 19 to 27 MB | not tested |

Licence: Apache-2.0, on the ONNX repo and on the PyTorch repo `depth-anything/Depth-Anything-V2-Small-hf`
(both card metadata). The design's "believed Apache-2.0" is now verified for V2 Small.
tract is MIT OR Apache-2.0.

## 2. Which runtime works

Tested versions: `tract-onnx` 0.23.10 (latest, rustc 1.91 MSRV, stable 1.99 here) and 0.22.5
(last release for rustc 1.90, the repo pin).

- **Loading the export as is fails** on both fp32 and fp16, with the symbolic dims of the
  export ("Impossible to unify Sym(batch_size) with Val(1)", then `Div(Sym(height), 14)` vs 19).
  Fix (offline, shape only, with the `onnx` Python package): drop the optional `value_info` (its
  dims like `floor(height/14)` block tract), set the graph input to `1x3xSxS`. The output stays
  symbolic and tract infers it. Done for 266 and 518. The shipped file must be pre-fixed this way.
- **fp32, tract 0.23**: loads and runs (1083 nodes in the unoptimised graph; ops include ConvHir, MatMul,
  Softmax, Erf, Resize (linear and one cubic), ConvTranspose). Output `[1, S, S]` depth, higher
  = closer, as the design assumes.
- **fp16 native, tract 0.23**: loads, but runs at 23 to 36 s per 266 px image (not usable).
- **fp16 with weights and casts upcast to fp32** (334 initializers, 7 casts; a shape-only
  rewrite): loads, runs at fp32 speed (see §3). File stays 49.6 MB on disk; RAM holds fp32 (99 MB).
- **int8 (`model_quantized`)**: loads, runs 0.7 to 1.9 s at 266 px (slower than fp32 in tract).
  Parity lower (§4).
- **tract 0.22.5**: fails to build the one cubic `Resize` (the position-embedding interpolation):
  `mode: cubic` unsupported. Workaround that works: since the input size is fixed, evaluate that
  one node offline (onnx reference evaluator, input zeros, output `1x384x19x19` at 266) and store
  it as a constant. Parity stays the same (0.99999994, §4). But tract 0.22 is about 3 to 4 times
  slower on this graph (§3).
- **ort** (ONNX Runtime): not needed, tract works. Not tried.

## 3. Timings

Desktop: Intel i7-12700K (8P + 4E, 20 threads). Other sessions were using the machine, so
load average was 13 to 36 during these runs (shown as "load"). Treat the numbers as ranges. Each
is after 1 to 2 warm-up runs; medians of 10 unless noted. Input is the 64x50 sprite (`s0_c11_i0`)
composited over grey, resized to the square input.

| tract | model | pinning | median (min) | load |
|---|---|---|---|---|
| 0.23 | fp32 266 | CPUs 8-19 | 304 ms (300) | 15 |
| 0.23 | fp32 266 | CPUs 8-19 | 473 ms (308) | 18 |
| 0.23 | fp32 266 | CPUs 8-19 | 493 ms (338) | 16 |
| 0.23 | fp32 266 | CPUs 8-19 | 802 ms (443) | 24 |
| 0.23 | fp32 266 | CPU 8 only (single core) | 1649 ms (1236) | 25 |
| 0.23 | fp32 266, cubic folded | CPUs 8-19 | 488 ms (306) | 13 |
| 0.23 | fp16 upcast to fp32, 266 | CPUs 8-19 | 496 ms (341) | 21 |
| 0.23 | fp32 266, same time as the row above | CPUs 8-19 | 548 ms (367) | 21 |
| 0.23 | int8 266 | CPUs 8-19 | 711, 910, 1912 ms (median; min 705 to 1038) | 16 to 18 |
| 0.23 | fp16 native 266 | CPUs 8-19 | 23 to 36 s per image (5 images, one run each) | ~30 |
| 0.23 | fp32 518 | CPUs 8-19 | 3845 ms (2000, n=8) to 5475 ms (3538) | 14 to 24 |
| 0.23 | fp32 518 | CPU 8 only | 1997 ms (1868, n=5) | 15 |
| 0.22.5 | fp32 266, cubic folded | CPUs 8-19 | 1358 ms (975) and 1388 ms (1043) | 14 to 19 |
| 0.22.5 | fp16 upcast, cubic folded, 266 | CPUs 8-19 | 1416 ms (1349) | 19 |

Observations:
- tract's inference ran on one thread (checked mid-run): one image is one core's work, and
  parallelism must come from the engine (several sprites at once).
- Pinned to one core, the time was about 1.6 s at load 25, so the spread is mostly the shared
  machine, not the model. On an idle desktop expect the low end (~0.3 s at 266).
- Model load plus optimisation: fp32 0.2 to 0.6 s; fp16 upcast 0.2 s (reads the 99 MB graph);
  int8 0.9 to 3.6 s.
- tract 0.23 is about 3 to 4 times faster than 0.22.5 on this graph (same load, same pinning).

Binary size (tract-onnx 0.22.5, rustc 1.90, default release profile, x86_64, one binary):
without tract 489 KB (390 KB stripped); with tract 21.9 MB (17.6 MB stripped). So tract adds about
17 MB stripped on desktop. Mobile size is not measured. The repo has no custom release profile.

## 4. Parity with PyTorch

PyTorch reference: `depth-anything/Depth-Anything-V2-Small-hf`, fp32, CUDA on the RTX 4060,
transformers pipeline inputs (square bicubic resize, ImageNet mean/std). Five building sprites
composited over grey 128, decoded from `Objects.ugs`: 67x58, 60x59, 64x50, 47x38 and 310x256.
Correlation is on the depth map at model resolution (266x266 or 518x518).

| run | 266 px: Pearson (5 sprites) | 518 px: Pearson (5 sprites) |
|---|---|---|
| tract 0.23 fp32, same input tensor as PyTorch (runtime only) | 1.000000 for all five; MAE 0.0001 to 0.0003 | 1.000000 for all five |
| tract 0.23 fp32, end to end (Rust resize and normalisation) | 0.99959, 0.99995, 0.99997, 0.99979, 0.99999 | 0.99997, 0.99998, 0.99999, 0.99973, 0.99999 |
| tract 0.23 fp16 native, end to end | 0.99992, 0.99993, 0.99995, 0.99967, 0.99998 | not run |
| tract 0.23 int8, end to end | 0.99773, 0.99746, 0.99964, 0.99507, 0.99784 | 0.99865, 0.99859, 0.99979, 0.99819, 0.99928 |
| tract 0.22.5, fp32 folded, same input tensor (one sprite) | 0.99999994 | not run |
| tract 0.22.5, fp16 upcast + folded, end to end (one sprite) | 0.9999993 | not run |

The end-to-end gap (1 minus the correlation, up to 0.0004 on fp32 at 266 px) comes from the resize
filter (Rust CatmullRom versus PIL bicubic); it is not a runtime error. The runtime itself matches PyTorch
to 6 decimals.

Not checked: sprites with other background colours (only grey 128 tested), non-square sprites
(all inputs are squashed to a square, same as the PyTorch reference), and visual quality (only
numbers).

## 5. Android and iOS (check only, no APK, no link)

- `aarch64-linux-android`: `cargo +1.90.0 check --release --target aarch64-linux-android` passes
  with tract-onnx 0.22.5, using the repo's NDK 29 clang (`aarch64-linux-android24-clang`, `llvm-ar`)
  as `CC_`/`AR_` for the build scripts.
- `aarch64-apple-ios`: fails on Linux. The `tract-linalg` build script uses `cc-rs`, which asks
  `xcrun` for the iOS SDK. With an `xcrun` shim (answers with the host clang) and an empty
  `SDKROOT`, `cargo check` passes. This is only a check: a real iOS build needs macOS and Xcode.
- tract-onnx 0.23 on Android and iOS: not checked (needs rustc 1.91; stable has no mobile targets
  installed).

## 6. Recommended design (for the atmosphere spec §2)

1. **Runtime**: `tract-onnx`. Owner decision needed on the toolchain:
   - (a) raise the pinned Rust to 1.91 or newer and use tract-onnx 0.23: about 3 to 4 times faster
     in this test, no fold step; or
   - (b) keep 1.90 with tract-onnx 0.22.5 and the offline cubic-Resize fold (parity unchanged):
     about 1.4 s per sprite on desktop, which is slow for phones.
   This is a release-pipeline change (`rust-toolchain.toml`, the dist scripts and CI), so it is the
   owner's call.
2. **Model file**: the fp16 ONNX (49.6 MB download, matching the design's "~50 MB"), prepared
   offline: shape-fixed to `1x3x266x266`, weights and casts upcast to fp32, and for 0.22 the cubic
   Resize folded. The engine loads it as fp32 (99 MB resident weights; phone RAM untested).
3. **Input**: 266x266 square, bicubic resize, ImageNet mean/std. The 266 px choice is cheap
   enough (see below); 518 px is 5 to 10 times slower and was not needed for the parity test.
4. **Precision**: fp32 compute. Do not ship native fp16 in tract (23 to 36 s per image). Do not
   ship int8 (slower and lower correlation).
5. **Threads**: each inference is single-threaded in tract, so run N sprites in parallel (a thread
   pool with one plan per worker, or shared `Arc` plans in 0.23's `run`). Pick N from the phone's
   big cores when measured.
6. **Expected times** (extrapolated from the desktop figures, not measured on a phone; the design's
   desktop-to-phone factor of 4 to 6 applied):
   - desktop, 266 px, one thread (0.23): 0.3 to 0.5 s per sprite;
   - phone, one thread: 1.2 to 3 s per sprite;
   - 30 mod sprites: about 40 to 90 s on one thread, roughly a quarter of that on 4 threads;
   - a whole 370-sprite install (for reference): about 2 to 3 min on a desktop thread and 15 to
     25 s on 8 threads; on a phone about 7 to 18 min on one thread and 2 to 5 min on 4 threads. The
     design's "phone 3 to 4 min" therefore needs about 4 parallel workers. With tract 0.22 these
     figures are 3 to 4 times worse.

## 7. Risks

- **Toolchain**: tract 0.23 needs rustc 1.91; the repo pin (1.90) forces 0.22.5 and the cubic fold,
  which is about 3 to 4 times slower (§3). The owner decides between speed and the pin.
- **Quality (not tested)**: the base install's cards come from Depth Anything V2 Large at 4 times
  upscale; mods would use Small at 266 px. The two maps are not the same; the spike only compared
  Small with PyTorch. A visual check of the cards built from Small output is needed before shipping.
- **Squashed inputs**: non-square sprites (for example 64x50 to 266x266) are distorted. The
  PyTorch reference used the same squash, so parity does not test this; the quality check above
  must.
- **Memory**: fp32 weights take 99 MB while the model is loaded. Phones not measured.
- **Binary size**: about 17 MB stripped on desktop (default release profile). Mobile size not
  measured.
- **Cross-device floats**: NEON, AVX and other kernels round differently, so depth for the same
  sprite can differ slightly between machines. Cached results are keyed by sprite hash, so a
  cache made on one machine may differ from another. Expected to be small (fp32 parity 0.9999+).
- **Tract's fp16 path**: native fp16 is slow in 0.23. Upcasting avoids it; a later tract may change this.
- **Load and speed variance**: the desktop timings varied up to 2 times with machine load. Phone
  timings are an extrapolation until measured on a device.
- **iOS**: check only on Linux; needs a macOS build before shipping.

## 8. Reproduce (scratch, outside git)

- Model downloads: `hf_hub_download` from the ONNX repo (`onnx/model.onnx`, `model_fp16.onnx`,
  `model_quantized.onnx`) and `snapshot_download` of the PyTorch repo, under `HF_HOME` (scratch).
- Shape fix and fold, the upcast and the cubic-fold scripts are in
  `razdor-scratch/atmo-model/spike/onnx-fixed/` (`fix_shape.py`, `upcast_fp16.py`, `fold_cubic.py`).
- Rust spike: `razdor-scratch/atmo-model/spike/rust` (tract-onnx 0.23, stable) and
  `razdor-scratch/atmo-model/spike/rust-190` (tract-onnx 0.22.5, rustc 1.90; `resolver = "3"` and
  `rust-version = "1.90"` so the resolver picks 1.90-compatible releases). Commands: `run` (one
  image or raw tensor), `bench` (warm-up and median), `info`.
- PyTorch reference and comparison: `razdor-scratch/atmo-model/spike/parity/ref.py` and
  `compare.py`.
- Nothing from the game is in the repo. The sprites used are the spike's own scratch copies.
