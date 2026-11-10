# Shadows: performance and memory analysis of real-time vs baked

Analysis for the cast shadows of the adventure map (2026-11-10). No product code changed. Numbers
are measured on the install's real sprites unless marked **est.** (estimate) or **not measured**.
Scratch data (probe source, depth maps, traced masks, CSVs, comparison sheet) lives outside git in
`/home/indicozy/.cache/razdor-scratch/atmo-shadows/analysis/`.

## The two options

- **A. Real-time silhouettes.** Per frame, each visible standing sprite (decoration, building or card)
  draws one sheared quad on the ground that samples a pre-blurred alpha silhouette (one R8 shadow
  atlas, same layout as the map atlas). Sun direction fixed, length from the sun's elevation, moves
  continuously.
- **B. Baked depth-traced shadows, 24 hours.** At first load, for each sprite and each of 24 hours, a
  heightfield shadow is traced from the sprite's depth map and stored; drawn per frame as one textured
  quad, two while crossfading neighbour hours. Needs the depth model on the device for every sprite.

## Method and what was actually run

1. **Sizes.** `probe sizes` on `Graphics/Objects/Objects.ugs` (engine decoder `decode_objects_ugs`,
   the engine's `pack_atlas` at width 2048, the same call as `DtArt::map_atlas`).
2. **Depth model.** The engine's own `DepthModel::depth` (tract 0.23, the shape-fixed fp16 Small file,
   `tools/depth/fix_onnx.py`, 266/518 input rule as the engine) on all 370 sprites, one at a time, then
   on 4 and 12 threads (wall time).
3. **Shadow trace.** A heightfield per sprite: each opaque pixel (alpha >= 204) is a column at
   X = x, height = h-1-y, depth Z = (1 - dn) * dz with dz = 0.5 x the sprite's larger side (**an
   assumption, not measured**; dz = 0 is the flat billboard). Sun azimuth 45 degrees (lower right, as the
   engine's relief), elevation from the engine's own curve (`ui::daylight` sun and moon: 11.5 to 66
   degrees by day, moon 34 degrees by night). Ground shadow by one sweep along the sun direction:
   `A(P) = max(H(P), A(P+s) - sqrt2 tan e)`, cell P shadowed when `A(P+s) - sqrt2 tan e > 0`. Exact on the
   grid for a 45-degree sun.
4. **Validation.** With dz = 0 the trace matches the closed-form flat shadow of A (sheared silhouette)
   up to the column fill: IoU 0.96 to 0.995 on three sprites and two hours. The difference is the
   heightfield's solid columns (a column is solid up to its top, the silhouette only where it is opaque).
5. **Per-frame counts.** Razdor's own frame probe (`RAZDOR_PROFILE=1`, `RAZDOR_SCENE_QUIET=1`, Xvfb, two
   5-second summaries per run; `sprites n=`, the standing sprites drawn per frame).

## Sizes (measured)

| item | value |
|---|---|
| sprites in `Objects.ugs` | 370 (285 decorations, 85 buildings); 2.97 Mpx of sprite pixels (1.92 + 1.05) |
| sprite pixels, unpacked RGBA | 11.9 MB |
| map atlas (2048 wide) | 2048 x 1684, RGBA8 **13.8 MB** (already in VRAM today) |
| shadow atlas A, same layout | R8 **3.45 MB**; RGBA8 13.8 MB; zero extra if it reads the map atlas alpha |
| B: 24 hourly bounding boxes, R8, dz 0.5 | **212.8 MB** total (6:00 alone 38.7 MB; 12:00 2.7 MB) |
| B: same, dz 0 (flat) | 157.7 MB total (6:00 34.2 MB; 12:00 0.9 MB) |
| B: 1 bit per cell (est.) | about 27 MB (dz 0.5) / 20 MB (dz 0) |
| shadow length at 6:00 (11.5 degrees, engine curve) | up to **4.8 sprite heights** (castle 285: 4.81 h) |

Note on the length: the task's "up to ~2.6 heights" is the 21-degree case. The engine's relief uses
0.20 rad (11.5 degrees) at dawn, so the real dawn shadow is about 4.9 heights. This spec uses the
engine's curve. A 2.6-height cap would shrink B's dawn boxes and A's dawn quads; not computed here.

## Depth model cost, B only (measured on this machine's CPUs 8-19, shared, load 6 to 12)

| run | result |
|---|---|
| model load (fp16 file upcast to fp32) | 0.21 s |
| all 370 sprites, one thread | **273.5 s**; 282 at 266 px, median 298 ms, sum 87.3 s; 88 at 518 px, median 1.94 s, sum 186.2 s |
| all 370, 4 threads (the engine's desktop worker count) | **86.2 s** wall |
| all 370, 12 threads | 63.0 s wall (4.3 times, not 12: uneven 518 sprites and the shared box) |
| trace, 370 sprites x 24 hours, one thread | **0.63 s** (dz 0.5), 0.47 s (dz 0) |
| phone | **not measured**. est. 18 to 27 min single thread (x4 to x6), about 11 min on the engine's 2 phone workers |

The 518 input takes 68 percent of the bake (88 sprites above 120 px). A 266-only bake would be about
111 s single thread on this desktop (est. 370 x 0.30 s). That is a real saving for B, but the quality
of 266 px on large sprites is untested.

## Per-frame counts (measured, standing sprites drawn per frame)

| scene (start, explored) | flat 1280x800 | 2.5D 1280x800 | flat phone 2400x1080 @2.625 | 2.5D phone |
|---|---|---|---|---|
| РК1-Начало пути | **55** (0.32 Mpx) | **37** (0.18 Mpx) | 65 (0.71 Mpx) | 42 (0.44 Mpx) |
| Устье Трейна | 4 (fogged start) | 6 | 156 | 125 |
| РК3-Столица | 8 (fogged start) | 8 | 14 | 9 |

Устье and РК3 start mostly under the fog, so only РК1 is representative. Its numbers are used below.
The desktop and phone views of the same fogged start differ a lot (4 vs 156 on Устье), so the start
view is not a stable measure; a mid-game save would be better.

## Per-frame cost (estimated from the counts and the traced footprints; not measured)

Per-frame fill is taken as the shadow boxes' area divided by the sprite pixels, applied to the on-screen
sprites. Ratios (ground box pixels per sprite pixel, all 370 sprites, per hour):

| hour | 00 | 05 | 06 | 07 | 09 | 12 | 15 | 17 | 18 | 19 | 21 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A (dz 0) | 1.47 | 2.99 | **11.5** | 2.57 | 0.68 | **0.32** | 0.68 | 2.57 | **11.5** | 2.99 | 1.47 |
| B (dz 0.5) | 2.20 | 3.88 | **13.0** | 3.41 | 1.31 | **0.90** | 1.31 | 3.41 | **13.0** | 3.88 | 2.20 |

Crossfade (B): 6:00 to 7:00 sums to 16.4 (dz 0.5) and 14.1 (dz 0); 12:00 to 13:00 sums to 1.8 and 0.7.

Applied to РК1 flat at 1280x800 (0.32 Mpx of sprites on screen):

| | A | B |
|---|---|---|
| extra draw calls, flat / 2.5D | +55 / +37 | +110 / +74 (2 per sprite, crossfade) |
| extra fill at noon | about +0.1 Mpx | about +0.3 Mpx |
| extra fill at 06:00 | about +3.7 Mpx (3.6 screens) | about +4.2 Mpx; 16 Mpx in the 6-to-7 crossfade |
| CPU per frame | not measured; one quad per sprite | not measured; two quads, hour switch |
| GPU per frame | not measured (Xvfb is software GL and does not represent the player's GPU) | same |

Dawn and dusk dominate both options. A's dawn cost is set by the 4.9-height reach; a 2.6-height cap
would cut it, and the cap is the cheapest lever for either option.

## Memory

| | A | B |
|---|---|---|
| RAM (persistent) | 3.45 MB shadow atlas (or none, sharing the map atlas alpha) | all 24 hours kept: 213 MB (dz 0.5) / 158 MB (dz 0), unacceptable on a phone. Only the current pair: 5 to 50 MB |
| RAM while loading | blur of the atlas: about 21 ms desktop (measured with PIL on an atlas-sized image) | model weights 99 MB fp32 (engine docs), plus about 130 MB for the 518 plan (engine docs). **Process RSS not measured.** |
| VRAM | 3.45 MB (R8), or 0 when sharing the map atlas | working set of two hours per sprite: about 5 MB at noon, **50 MB** at 05 to 06 and 18 to 19 (dz 0.5) |
| disk | none | the model file, **49.6 MB** download; masks if cached (213 MB raw, est. smaller compressed) |

## Quality (side by side, 7:00 and 12:00)

`/home/indicozy/.cache/razdor-scratch/atmo-shadows/analysis/compare_shadows.png`: rows are the castle
(285, 280 x 192), the tower (332, 120 x 159) and the village (292, 200 x 164); columns are A 7:00,
B 7:00, A 12:00, B 12:00. The sun falls from the lower right, so shadows go to the upper left, as the
engine's relief.

- **A** is the sprite's silhouette sheared along the sun direction (the flat billboard's projection).
  It is faithful to the picture's own outline, but a silhouette has gaps: the village's shadow breaks
  into separate pieces where its picture is open. It has no depth, so a house behind another is shaded
  as if it stood in front.
- **B** (dz 0.5) is solid: each column is filled up to its top, so shadows are heavier and longer than A
  (IoU with A only 0.09 to 0.38, cells 1.5 to 3 times A's). The depth spread is an assumption; with
  dz 0 B is A with the column fill. Neither version is checked against the original's painted shadows
  or against the engine's 3D truth. The shape difference is mostly the depth spread.

## Recommendation: A (real-time silhouettes)

1. **No model, no download.** B needs the 50 MB depth model on every install, base game included,
   because the shipped cards cover only the buildings' outlines. A needs nothing new.
2. **Load time.** A is about 21 ms of blur. B is 273 s single thread on this desktop (86 s on 4 workers)
   and an estimated 10 to 27 min on a phone.
3. **Memory.** A is 3.5 MB. B's 24 hours are 158 to 213 MB, and its working set is 50 MB at dawn.
4. **Smoothness.** A moves with the sun, continuously. B steps hourly and doubles quads while
   crossfading.
5. **Cost per frame.** A is one quad per sprite; B is two. Both are dominated by dawn and dusk fill.
6. **Quality.** B's solidity and depth spread are unvalidated; A is exact for the picture it has.

If depth-correct shadows are wanted later, B's depth map is already needed for the 2.5D cards on mods.
Then B's extra cost is the trace (0.6 s) and the 24 hours in memory, which A does not need.

Lever for either: cap the shadow reach at about 2.6 heights, which the task suggested. It cuts the dawn
fill and the boxes; not computed here.

## Not measured (and why)

- **Frame times of A or B** (CPU and GPU): no prototype was built. Xvfb uses Mesa's software renderer,
  which would not represent a player's GPU, and a headless EGL run on the RTX 4060 was not done.
  The per-frame figures above are fill and draw-count estimates.
- **Phone timings** (model bake and per-frame): no device; the x4 to x6 factor is the design's.
- **Process RSS and VRAM** actually allocated: only the sizes above, computed from the data.
- **Disk cache of B** after compression.
- **Depth quality**: the Small model at 266 and 518 px was used as the engine runs it; its depth was
  not checked against the shipped Large cut data for these shadows.
- **Mid-game counts**: the start views of Устье and РК3 are fogged; the counts for them are not used.
