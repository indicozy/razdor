# Ground classifier: from the owner's labels to the cards

The 2.5D view cuts each building sprite into cards: pieces that stand, and pieces that lie flat
on the ground (courtyards, fields, paths, the land between houses). The data is
`data/cards/objects.json`: each card has a polygon (`verts`, `tris`), a foot (`foot`) and a
`ground` flag. This flow says how the flags are labelled by hand, learned, applied to the base
data, and how the same classifier must run for mods in the engine (not implemented yet).

The classifier is one logistic regression over 18 per-pixel features (`tools/depth/ground_features.py`,
FEATURE_NAMES). Every feature is computable on a player's machine: the built-in depth model
(Depth Anything V2 Small, the shape-fixed ONNX, `tools/depth/fix_onnx.py`), the normals and
gradients derived from its depth, the colour, the alpha edge distance and the island geometry.
No SAM, no normal model, no Marigold.

## The steps

### 1. Paint the labels in the click tool

The tool is a local page with a tiny server (`server.py` in the click tool's folder, stdlib only, not in
git). It shows one sprite at a time, with a per-pixel overlay: green is ground (lies flat), red stands,
no colour is transparent. The overlay starts from the current labels (the card flags of
`data/cards/objects.json`).

```sh
cd /mnt/ai_models/razdor-scratch/folded/clicktool
taskset -c 8-19 nice -n 19 python3 server.py    # serves on 127.0.0.1:8765
# open http://127.0.0.1:8765/index.html
```

- Brushes: «Земля» (G, green) and «Стоит» (R, red); the brush size with the slider or `[` / `]`.
- Eraser to the AI (E): restores the prefilled label under the brush. Fill (F): a connected area of the
  same label. Undo/redo: Ctrl+Z / Ctrl+Shift+Z. Overlay toggle (H) and opacity slider.
- Zoom: fit, 2× and 4× buttons; pan with space + drag or the middle mouse.
- Only opaque sprite pixels take a label.
- «Проверено» marks the sprite as reviewed. Only reviewed sprites are used for training. Check a
  sprite only when every pixel on it is right: an unchecked sprite is ignored, a checked one is trusted.
- ←/→ move to the previous or next sprite; Shift+←/→ to the next unchecked one.
- The page saves each stroke by itself (debounced). Each sprite's mask is written to
  `labels/<hash>.png` (the label in the R channel: 0 none, 1 ground, 2 standing), and
  `labels.json` holds the checked flags. Both stay in the click tool's folder, never in git.

The older label file, `flips.json` (per-card flips, `{hash: {flips, checked}}`), is still read by the
trainer and the applier for the sprites that have no mask.

### 2. Export the labels

The masks are the labels; nothing to download. The training input is the folder `labels/` and the file
`labels.json` next to the server. Copy them somewhere safe if you want to keep them, for example
`/mnt/ai_models/razdor-scratch/folded/labels-<date>/`.

### 3. Train

Needs the sprite PNGs (the click tool's `sprites/` folder), the depth model, and the venv with
`numpy`, `scikit-learn`, `onnxruntime`, `opencv` and Pillow (`cv2` and `onnxruntime` are in the
scratch environment; see step 0 below).

```sh
PYTHONPATH=<scratch pylib> python3 tools/depth/train_ground.py \
  --labels /mnt/ai_models/razdor-scratch/folded/clicktool/labels \
  --labels-json /mnt/ai_models/razdor-scratch/folded/clicktool/labels.json \
  --data data/cards/objects.json \
  --sprites /mnt/ai_models/razdor-scratch/folded/clicktool/sprites \
  --onnx /mnt/ai_models/razdor-scratch/folded/models/da2-small/depth-anything-v2-small-fp16-266.onnx \
  --out data/cards/ground_model.json
```

`--labels` and `--labels-json` are the paint tool's; a checked sprite with a mask trains on its pixels.
The older `--flips flips.json` is still accepted (for the sprites with no mask).

Expected output (the two-sprite smoke test prints the same shape of lines):

```
3ba36b870a37ee0a: 35298 labelled pixels, ground 7600
ad4c6735da164222: 49932 labelled pixels, ground 17949
train sprites 1, pixels 49932; test sprites 1
train: pixels 49932, accuracy 0.801, ground recall 0.851, standing recall 0.772
test: pixels 35298, accuracy 0.780, ground recall 0.007, standing recall 0.992
per sprite (test):
  3ba36b870a37ee0a: accuracy 0.780
wrote data/cards/ground_model.json
```

The split is by sprite: every fifth checked sprite (by hash order) is held out. Look at the
per-category lines: `ground recall` (courtyards, fields, land) and `standing recall` (walls,
houses, towers, roofs). Trust the numbers only with enough sprites (at least about 20 checked
sprites, with some villages, castles and towers among them).

The output `ground_model.json` holds numbers only: the feature names, the standardisation
(`mean`, `scale`), the 18 `weights`, the `bias`, the threshold, the depth model's sha256 and the
list of training sprites.

### 4. Evaluate

The `test` line is the held-out figure. If it is low on the ground class, label more ground
sprites (fields, courtyards) and train again. The training run itself is cheap (seconds per
sprite on the CPU).

### 5. Apply to the base data

```sh
PYTHONPATH=<scratch pylib> python3 tools/depth/apply_ground.py \
  --data data/cards/objects.json \
  --model data/cards/ground_model.json \
  --sprites /mnt/ai_models/razdor-scratch/folded/clicktool/sprites \
  --onnx <the same .onnx> \
  --labels-json /mnt/ai_models/razdor-scratch/folded/clicktool/labels.json \
  --out data/cards/objects.json
```

Expected output: `sprites kept (owner-checked): 2; cards whose flag changed: 281; wrote ...`.
Every sprite that is not checked gets its card flags from the model: a card is ground when at
least half of its opaque pixels have a ground probability of 0.5 or more. Polygons, feet and card
order do not change. The checked sprites keep the owner's labels exactly.

Then the usual checks: `cargo test --release --bin razdor`, `cargo test --release --lib cards`,
`cargo clippy --release --all-targets -- -D warnings`. Commit the data and `ground_model.json` (both
are numbers), never the sprite PNGs, the depth model or the flips file (see `CLAUDE.md`, sprite cut data).

### 0. Setup (once)

- The depth model: `curl -L` the ONNX from the pin in `tools/depth/README.md` (the sha256 of the source
  is `2df6223f…`), then `python3 tools/depth/fix_onnx.py model_fp16.onnx depth-anything-v2-small-fp16-266.onnx --size 266`.
  The output's sha256 must be `897f1b87…`. Keep it out of git.
- Python packages go into a scratch folder with `uv pip install --target <dir> --no-deps`
  (`opencv-python-headless`, `onnx==1.23.2` with `ml-dtypes`, `onnxruntime`). Add it to PYTHONPATH.

## The engine: what the runtime must do for mods

Mods (and community maps) with buildings the base data does not know get their cards from the
engine's own cut: `src/depth/`. Today that cut returns standing cards only (`src/depth/cut.rs`,
the `Card` it builds has `ground: false`). To give mods the same ground flags:

1. **Model file.** `src/depth/model.rs` already loads the shape-fixed ONNX (SMALL input 266) and runs it;
   the file is fetched by `src/depth/fetch.rs` (the pinned URL and sha256). Use the same session.
2. **Features.** Port `tools/depth/ground_features.py` to Rust, in src/depth/features.rs (new). The
   order of FEATURE_NAMES is fixed. The steps: the composite over grey and the ImageNet
   normalisation are in `model.rs` already; the depth normalised over the opaque pixels; the
   gradients (`np.gradient`: central differences, one-sided at the edges); the normal with NORMAL_K
   = 8; the colour and HSV; the squared-distance transform of the opaque mask (Euclidean, as
   `cv2.distanceTransform` with DIST_L2 and mask size 3, so port the two-pass chamfer exactly or
   use the same 3x3 metric and say so); the 3x3 opening and the 4-connected labels of the opaque
   pixels, with each island's top and bottom rows and area.
3. **Evaluate.** Read data/cards/ground_model.json (add it with `include_str!` beside
   `data/cards/objects.json` in `src/dt/cards.rs`, or a loader like `builtin()`). Compute
   `sigmoid(sum_i w_i · (f_i − mean_i) / scale_i + bias)` per opaque pixel; a pixel is ground at
   `threshold`.
4. **Split the cards.** The cut builds cards from regions (`build_cards` in `cut.rs`). Set each
   card's `ground` flag by the majority rule of step 5 of the tutorial above (at least half of its
   opaque pixels ground). The polygons and feet of the cut stay as they are.
5. **Keep the old cut as the fallback.** When the model file is missing or its feature list
   differs, the cut returns standing cards only, as now.

Tests to add, in the same commit:

- A fixture: run `ground_features.py` on one sprite (for example the `780e4112a5e12150` sprite),
  dump its features to a small numbers file under `src/depth/` (not the sprite), and assert the Rust
  features match within 1e-3 for the same depth input.
- A test that the Rust classifier reproduces `apply_ground.py`'s flags on that sprite.
- The existing bundled-data tests must still pass (`bundled_buildings_stand_on_their_ground`).

Performance budget: the cut runs on background workers (`src/depth/work.rs`, at most four threads on
desktop, two on phones). The feature pass is linear in the sprite's pixels (a few hundred thousand
at most per building) and must not add more than a small fraction of the depth run's time. Measure
with a one-sprite timing before and after, on the desktop build, and keep the phone build within the
same budget (`RAZDOR_PERF_BUDGET_SCALE` is the test knob for the perf tests).

## Where things live

| What | Where | In git |
|---|---|---|
| Per-pixel features (Python, reference) | `tools/depth/ground_features.py` | yes |
| Trainer | `tools/depth/train_ground.py` | yes |
| Applier | `tools/depth/apply_ground.py` | yes |
| The trained weights | data/cards/ground_model.json | yes (numbers) |
| The owner's pixel labels | the click tool's `labels/<hash>.png` and `labels.json` (and the older `flips.json`) | no (local) |
| The sprite PNGs | the click tool's `sprites/` | no (game art) |
| The depth model | `fix_onnx.py` output, `…-266.onnx` | no (fetched by the engine) |
| Engine cut for mods | `src/depth/cut.rs`, `model.rs`, `fetch.rs` | yes |
