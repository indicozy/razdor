# tools/depth: cut the map sprites into cards

An offline tool. It reads the sprites of `Graphics/Objects/Objects.ugs` in each install, estimates
a depth map of every sprite with Depth Anything V2 Large, splits the sprite into a few depth
regions (cards) and writes only numbers to `data/cards/objects.json`. The engine uses those numbers
for the 2.5D view: each card stands at its own ground point and sorts like a sprite.

## What it does

1. Decodes each sprite exactly as `src/dt/gfx.rs` (`decode_objects_ugs`, `ugs_pixel`).
2. Keys each sprite by an FNV-1a 64 hash over `u32le(w) + u32le(h) + RGBA bytes` (row-major,
   four bytes a pixel as decoded), written as 16 lowercase hex digits. The engine must hash the same
   way. Identical sprites in several installs are stored once.
3. Composites the sprite over grey (128), upscales it x4 (bicubic), runs the model (fp16, CUDA) and
   resizes the depth back to the sprite size. Higher depth = closer.
4. Takes the opaque pixels (alpha >= 204; shadows at alpha 153..187 are not part of any card) and
   skips sprites with fewer than 200 of them.
5. Splits the opaque pixels into regions: 4-connected, joined when their normalised depths differ
   by at most 0.06. Small regions (under max(60 px, 1.5 % of the opaque pixels)) merge into the
   touching region with the closest mean depth. More than 8 regions merge pairwise by mean depth.
   A sprite left with one region gets no entry; the engine keeps one card for it.
6. Finds each region's foot: the lowest row, and the mean x of its bottom pixels. A region with
   another region directly below 30 % or more of its bottom pixels is occluded; its foot is moved
   down along a ground line fitted through the unoccluded regions of the sprite.
7. Outlines each region (dilated by 1 px), simplifies the outlines (epsilon 1.0), triangulates them
   with holes (mapbox earcut).

### The bends pass (castles)

Castles are one continuous ring of walls whose depth changes gradually, so step 5 leaves them as one
region. The bends pass cuts such a sprite at its structural bends. It runs after the merge in step 5
and before the "one region" exit, and only when all of these hold:

- the sprite is in the building section (section 1 of `Objects.ugs`), and step 5 left exactly 1 region;
- it has at least 2500 opaque pixels;
- the p95 - p5 range of its normalised opaque depth is at least 0.25.

Then:

1. Smooths the depth over the opaque pixels only (normalised convolution: Gaussian sigma 2 of
   depth x mask, divided by the Gaussian of the mask) and takes Sobel gradients gx, gy of the result.
2. Per opaque pixel, the features `[2 d, gx / s, gy / s]`, with d the smoothed depth and s the median
   gradient magnitude over the opaque pixels (at least 1e-6, so no division by 0). For each K from 2 to
   5, k-means (scikit-learn, 10 inits, seed 0). A K is rejected when a cluster holds less than 8 % of
   the opaque pixels. Among the valid K, the one with the best silhouette score on a 3000-pixel sample
   (numpy seed 0) wins. No valid K: the sprite keeps one region, no entry, as before.
3. The clusters' labels split into 4-connected pieces; the pieces go through the same small-region
   merge as step 5, down to at most 5 regions (merging the pair with the closest mean depth).
4. Feet and ground fit: step 6, unchanged.
5. Overlap: a bends region's card area is its region dilated by 4 px (a 9x9 square), clipped to the
   sprite's silhouette (the opaque pixels dilated by 1 px). So a card reaches 4 px into the neighbours
   across a cut, never past the silhouette. Cards are drawn back to front, so the front card covers
   the overlap. Plain regions keep the 1 px dilation.

The data file format does not change (version 1). The bends pass only adds entries: a sprite that
already had cards gets the same cards as before.

The contact sheet of the bends-cut sprites is `bends_sheet.png` in the preview folder: region tints,
triangle outlines and the feet as red crosses.

Cards are ordered back to front, by foot y ascending.

## Running it

Needs the venv with torch, transformers and scikit-learn, plus two small libraries installed into a
scratch folder (opencv-python-headless, mapbox-earcut; `uv pip install --no-deps --target <dir>`
keeps the venv untouched). Run it on the RTX 4060 only (the script aborts on any other GPU):

```sh
CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=1 HF_HOME=/mnt/ai_models/hf OMP_NUM_THREADS=12 \
PYTHONPATH=/home/indicozy/.cache/razdor-scratch/atmosphere/depth/pylib \
nice -n 19 taskset -c 8-19 /mnt/ai_models/venvs/qwen-tts/bin/python tools/depth/cut_sprites.py \
  --out data/cards/objects.json \
  --preview /home/indicozy/.cache/razdor-scratch/atmosphere/depth/preview \
  "/home/indicozy/Games/Discord Times Community Update" \
  /mnt/ai_models/razdor-community/evolution/install
```

`--preview` is optional and writes scratch PNGs only: per sprite (sprite, depth, regions with
triangles and feet), `contact_sheet.png` of the building sprites and `bends_sheet.png` of the
bends-cut sprites. Never commit previews.
A run takes about a minute and a half for both installs.

## Licence

`depth-anything/Depth-Anything-V2-Large-hf` is CC-BY-NC-4.0. The model runs offline only, and only
its numbers ship: no model output images, no pixels. Do not run the model on the engine's side
with this checkpoint.

## The data file

`data/cards/objects.json`, one sprite per line:

```json
{"version":1,"model":"depth-anything/Depth-Anything-V2-Large-hf","alpha_opaque":204,"sprites":{
"<key>":{"w":W,"h":H,"cards":[{"foot":[x,y],"verts":[x0,y0,x1,y1,...],"tris":[i0,i1,i2,...]}, ...]},
...
}}
```

- `key`: the sprite's pixel hash (see above).
- `w`, `h`: the sprite's size in pixels.
- `foot`: the card's ground point, in sprite pixels (x from the left, y from the top; one decimal).
- `verts`: the card's outline vertices, integer sprite pixel coordinates, flattened.
- `tris`: triangles over `verts` (indices into this card's vertices), flattened.
- Cards are back to front (foot y ascending). Sprites with one region have no entry, unless the
  bends pass cuts them (its cards may overlap across a cut, see above).
