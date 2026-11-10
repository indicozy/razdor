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

Cards are ordered back to front, by foot y ascending.

## Running it

Needs the venv with torch and transformers, plus two small libraries installed into a scratch
folder (opencv-python-headless, mapbox-earcut; `uv pip install --no-deps --target <dir>` keeps the
venv untouched). Run it on the RTX 4060 only (the script aborts on any other GPU):

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
triangles and feet) and `contact_sheet.png` of the building sprites. Never commit previews.
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
- Cards are back to front (foot y ascending). Sprites with one region have no entry.
