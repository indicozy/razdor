# Atmosphere release: performance measurements (2026-11-11/12)

Measured on the owner's desktop: RTX 5060 Ti (the monitor's GPU), 2560×1440 full screen, vsync off,
the game pinned to CPUs 8–19. The monitor caps at 180 FPS (5.55 ms), so where a build hits the cap
the CPU time per frame (the frame probe's `work_ms`) is the comparable number. Harness:
`/mnt/ai_models/razdor-scratch/gpu-prof/` (`gpu.py`, `full4.py`, outside git). Medians of 2–3 runs.

Builds: v1.3.14 and v1.4.1 (release downloads), main `a0b301ab` (atmosphere batches 1–7 + the CPU
pass), branch `cpu-leaks` `c3697b09` (the CPU-leak fixes and the «Качество графики» row).

## What the work changed, in order

| Step | Where | Effect |
|---|---|---|
| Sun shadows baked into a world-space map, stepped once per game hour | first person | 33 → ~15 ms (30 → ~65 FPS) |
| Sprite batching (draws 3767 → 337 in first person, 636 → 185 in 2.5D) | first person | → 11.2 ms (~88 FPS) |
| Relief (groove) shading removed | 2.5D lighting | terrain stage 43 → 16 ms (software) |
| Fog redesigned (one pass instead of four, baked noise) | all | −44…−55% fog cost (software); ~1 ms on the 5060 Ti |
| Water: noise table, per-map bake, zoom LOD | all | first person 7.7–7.9 ms both before and after the new look (no cost) |
| CPU pass (view-projection once per sprite, form sun once, no per-sprite Vecs) | 2.5D whole map | 14.2 → 12.0 ms |
| CPU-leak fixes (FrameCtx, jitter per object, chunked culling, mesh reuse, fog counter, frame state) | see below | up to +25% |
| Tried and dropped: a cached map picture | flat whole map | slower (6.3 → 16–31 ms): tree sway and water refill it 24×/s |
| Tried: «Анимация карты» Выкл, «Разрешение карты» 50% | zoomed out | no FPS change (the cost is CPU, not pixels); Выкл stills the map (battery) |

## Final table (FPS; "180 (x ms)" = capped, x = CPU per frame)

### «Другой берег», explored, zoomed out to v1.4.1's limit

| View | v1.4.1 | main | branch |
|---|---|---|---|
| Flat, effects off | 180 (1.2) | 180 (1.7) | 180 (1.5) |
| Flat, effects on | — | 180 (2.4) | 180 (1.5) |
| 2.5D, effects off | 150 | 180 (4.0) | 180 (3.2) |
| 2.5D, effects on | — | 166 | 180 (4.5) |
| Third person, off | 152 | 127 | 151 |
| Third person, on | — | 102 | 127 |

### Whole map on screen (only main/branch reach it)

| View | main off / on | branch off / on |
|---|---|---|
| Flat | 171 / 131 | 180 / 180 |
| 2.5D | 116 / 83 | 142 / 104 |
| Third person | 68 / 57 | 83 / 71 |

### Start of РК3 (a light scene all four builds open)

All at the 180 cap; CPU per frame 1.0 (v1.3.14) / 1.0 (v1.4.1) / 1.2 (main) / 1.2 (branch) ms.

## Memory, load, storage

| | v1.3.14 | v1.4.1 | main / branch, effects off | main / branch, effects on |
|---|---|---|---|---|
| RAM (peak) | ~180 MB | 170–270 MB | 190–280 MB | 200–285 MB |
| VRAM | 116–130 MB | 36–63 MB | 54–93 MB | 129–167 MB |
| Load to first frame | 0.32–0.39 s | 0.31–0.36 s | 0.33–0.44 s | 0.46–0.55 s |
| Binary | 16.1 MB | 16.3 MB | ~47 MB (38.8 stripped) | same |

The binary grew mainly from the built-in depth-model runtime (tract) used to cut mod buildings. Shipped
building data adds ~360 KB (`data/cards`, `data/lights`). The picture cache on the owner's machine is
178 MB (shared by all builds); a per-build fresh-cache measurement is still to do.

## Known remaining costs

- Flat zoomed out with effects off still uses ~0.3 ms more CPU than v1.4.1 (both at 180 FPS).
- Zoomed-out 2.5D/third person is CPU-bound (thousands of sprites); next steps: retained sprite
  batches while the camera is still, depth-sort reuse, the remaining per-sprite settings reads.
- Correctness note: the off-screen software renderer (llvmpipe) shows artefacts the GPU doesn't
  (missing first-person sprites on РК3, a dark ring and a crosshatch in the fog) — judge looks on a GPU.

## Final check: main `32993b2c` vs v1.4.1 vs the original (2026-11-12)

Same setup as the earlier original-vs-Razdor run: РК3-Столица start, 1024×768, uncapped, the RTX 4060
through VirtualGL under Xvfb, fresh settings (so main runs the «Оригинал» preset, effects off), 40 s runs
with the first 5 s dropped, two runs each (harness `/mnt/ai_models/razdor-scratch/fps-final/run2.sh`).

| View | v1.4.1 FPS (CPU ms) | main FPS (CPU ms) | Change |
|---|---|---|---|
| Flat | 626 (0.18) | 612 (0.21) | −2% FPS, +0.03 ms CPU |
| 2.5D | 604 (0.26) | 539 (0.42) | −11% FPS, +0.16 ms CPU |

- The flat view (the original's look) is within 2% of v1.4.1.
- 2.5D close up costs +0.16 ms of CPU a frame: the world stage 0.16 → 0.31 ms, terrain 0.004 → 0.03 ms.
  The likely cause is the folded building cards (more sprites per building), which are the fix for floating
  buildings. Zoomed out, 2.5D is faster than v1.4.1 (see the tables above).
- The original DiscordTimes.exe: the only figure is 176 FPS (2026-11-04, flat, same map and size). The
  harness could not re-measure it (it reads a constant at `0x4c7488`, not a live counter). That run's
  VirtualGL setup also gave v1.4.1 only 222 FPS against 626 here, so the two are not directly comparable.
  On that day's setup v1.4.1 was 1.26× the original, and main's flat view is within 2% of v1.4.1.

### Re-run on main `7dd62351` (after the trailer work)

| View | v1.4.1 FPS (CPU ms) | main FPS (CPU ms) |
|---|---|---|
| Flat | 626 (0.18) | 604 (0.21) |
| 2.5D | 604 (0.27) | 536 (0.42) |

The trailer work changed nothing measurable (it is dev-only).

### Why 2.5D close up costs more (code reading, 2026-11-12; not yet measured per function)

The extra time is in the `world` stage. Draw batches are about the same, so the cost is extra CPU work
per item. The suspects, unranked:

1. **Card meshes rebuilt every frame.** `layered_cards` / `card_mesh_form` (`src/ui/world_view.rs`)
   clone each card's vertices and triangles and compute the sun per card. The ground cards (~378) are
   also redrawn every frame. Fix: cache the meshes per building, keyed by camera, or bake the static
   ground cards into the ground cache.
2. **Three passes over all locations in 2.5D** (ground cards, shadows, main). Each pass repeats the fog
   check and the card lookup. Fix: compute the seen locations once per frame.
3. **The view-projection matrix rebuilt per sprite** in `picture_on_screen` → `view_proj`
   (`src/ui/view25.rs`). Fix: compute it once per frame and keep it in `Tilt`.
4. **Terrain +0.025 ms** in the cached branch of `draw_ground`: confirm with `perf` first.

Next step: `perf record` on the 2.5D scene for both builds to get each function's share, then fix 1–3.

## 2.5D close-up cost: profile and fix (2026-11-12, perf-fix)

Profile (РК3-Столица start, 2.5D, RTX 4060 via VirtualGL): `perf record` works only as the parent with the
`cpu-clock` software event (no hardware PMU, no call chains under this kernel policy), so the shares are self
time, main thread (94% of the samples). In main (before): `Iterator::any` closure (the fog scan in
`location_seen`) 8.0% in 2.5D against 3.9% flat; the remaining top entries are libc memmove and mutex unlock
(GL/VirtualGL call overhead, the same in flat). Frame-timer counters (temporary, not committed): `location_seen`
441 calls a frame in 2.5D (three passes over 147 locations) costing ~267 µs with the timers on; flat makes 147.
Card work (`layered_cards`, `draw_building_ground`, `card_mesh_form`) was about 12 µs a frame: only one card
building was in view.

Fix: the location test checks the drawn rows and columns before the fog scan (both pure), and the 2.5D passes
and the living smoke share one list of the locations seen this frame. Pixels: the eleven snapshots (flat, 2.5D,
third and first person, effects off and on, zoomed out) were byte-identical before and after.

Building cards off by default (owner, 2026-11-12): `RAZDOR_BUILDING_CARDS=1` turns them on (dev only). With
cards off the card path, the depth worker and the settings row are inactive.

Measured on a shared machine (other sessions' builds and game runs, load 5–17): the medians are not usable, so
only the best run of each configuration is quoted, probe `work_ms`:

| build | flat best | 2.5D best |
|---|---|---|
| v1.4.1 | 0.191 | 0.346 |
| main before | 0.209 | 0.453 |
| location fix (cards on) | 0.195 | 0.298 |

Not re-measured on a quiet machine: the target (2.5D back near 0.27) is not confirmed. Terrain (+~0.025 ms):
the cached terrain path draws the cache and then an overlay pass (needed on water cells), where v1.4.1 drew
one quad; left as designed.

## After the fix: main `69212f33` vs v1.4.1 on the owner's monitor (2026-11-12)

`69212f33` turns the folded building cards off (buildings whole, as in v1.4.1) and runs the fog check once per
location per frame (it ran 441 times a frame in 2.5D). Measured with `tools/perf/bench.py --display :0`
(RTX 5060 Ti, 2560×1440, vsync off, 3 runs each, effects off). Medians; CPU = the frame probe's `work_ms`.

| Scene | v1.4.1 frame / CPU ms | main frame / CPU ms |
|---|---|---|
| РК3 start, flat | 0.42 / 0.16 | 0.43 / 0.16 |
| РК3 start, 2.5D | 0.47 / 0.21 | 0.47 / 0.21 |
| «Другой берег» zoomed out, flat | 7.65 / 1.29 | 2.07 / 1.03 |
| «Другой берег» zoomed out, 2.5D | 7.22 / 6.95 | 2.58 / 2.25 |
| «Другой берег» zoomed out, third person | 7.13 / 6.85 (v1.4.1 has no third person: 2.5D) | 5.58 / 5.26 |

The 2.5D close-up cost is gone: main matches v1.4.1 there, and zoomed out it is 3× faster. RSS is +7% and VRAM
+10 MB at the РК3 start. `tools/perf/baselines/v1.4.1.json` now holds these measured v1.4.1 numbers (it held numbers
copied from this doc before). The 4060 headless check could not run: the card hung (Xid 119) while the bench
polled `nvidia-smi` every second; the bench now reads VRAM once per run.

## Sprite pass: profile split (backlog 1605, step 0; 2026-11-12)

Question: where does the world pass's CPU go on the zoomed-out map, so that the next step targets it? Measured on
`ade8f733`'s code plus `df9c305a` (HEAD, no pixel change), the RTX 4060 through VirtualGL under a private Xvfb, 1024×768,
«Другой берег», 30 s per run, the frame probe's CSV (`work_ms`, `world`, `draws`, `verts`) and its 5 s summary lines.
The machine was shared (load 8–12 on 20 CPUs), so the absolute numbers move between runs; the ratios inside one run
are the usable part. Medians of the probe's per-window p50s; the first 100 CSV frames dropped.

Whole-pass numbers (HEAD, unprobed, one run each):

| Case | work ms | world ms (Stage::World) | draws | verts |
|---|---|---|---|---|
| zoomed out (`RAZDOR_MAP_ZOOM=0.05`), 2.5D | 5.68 | 5.19 | 180 | 16,348 |
| zoomed out, third person (`RAZDOR_VIEW=first`) | 8.80 | 7.15 | 183 | 26,096 |
| whole map (`RAZDOR_MAP_FIT=1`), 2.5D | 7.40 | 7.03 | 216 | 54,408 |
| whole map, third person | 13.91 | 11.61 | 213 | 58,492 |

Split of the world pass (temporary `frame_probe::time`-style stages, not committed; the timers are in the item build,
`standing_at` and the item loop of `draw_world_in`):

| Case (probe run 3) | work | items (seen tests, pushes, sort) | placement (`standing_at` before its draw) | sprite draws (its callback) | per-object prep (picture, detail, fog shade) | item loop, total | outside the loop (ground, fog, sky, the hills' pass) |
|---|---|---|---|---|---|---|---|
| zoomed out, 2.5D | 3.2 | 0.2 | 0.6 | 0.8 | 0.3 | 2.1 | ~1.1 |
| zoomed out, third person | 7.2 | 0.6 | 1.1 | 0.8 | 0.6 | 3.7 | ~3.5 |
| whole map, 2.5D | 8.3 | 0.5 | 1.9 | 2.6 | 0.6 | 6.0 | ~2.3 |
| whole map, third person | 15.3 | 1.4 | 3.6 | 2.7 | 1.3 | 9.5 | ~5.8 |

The first split run (before the timers were scoped to the item build) read 0.9 ms items and 4.8 ms placement for
zoomed-out third person: the same order, noisier. The loop is not the sum of its parts: placement excludes the
time of a sprite culled before its placement starts (the bend's horizon, the fade to zero), and the building and
army branches' own lookups (`building_foot`, `atlas_src`) are not in any stage.

What this says:

- **The item build and the sort are small**: 0.2–1.4 ms of 3–15 ms of world work (5–10%). Step 1 (sorting once per
  map and turn angle) therefore saves at most that share; its measured gain will be well under 1 ms zoomed out.
- **Per-sprite placement is the largest cost** (`standing_at` → `standing_model`, `picture_quad`, the off-window
  test, the eye test), 0.6 ms zoomed out 2.5D and up to 3.6 ms whole map third person; with the per-object prep
  and the fog shade it is about half of the loop. Sprite draw callbacks come next (0.8–2.7 ms).
- **Outside the loop** (the ground's overlays, the fog, the sun map, the hills' session): 1–6 ms, the rest of the pass
  and not yet split.

So the premise of the backlog plan (sorting is the main zoomed-out cost) does not hold on this measurement; the
per-sprite placement and the draws do. Steps 1 and 2 (the cached order and the chunks) can remove only the item
build and the sort, so their gain is at most about 1 ms of the zoomed-out third-person frame. Step 3 (GPU vertex
buffers per chunk, the camera as uniforms) is the one that removes the placement, which is the cost to attack. The
step order is the owner's; this section is the evidence for choosing between them.

Not measured here: the 96-PNG matrix, the invalidation tests, `tools/perf/check.sh`, and the Android split. No code
was changed by step 0; the instrumented build was reverted before this commit.

### Correction: the step 0 split ran on the RTX 5060 Ti (2026-11-12, after the reboot)

The step 0 numbers above were measured with the hardcoded `/dev/dri/card2` in the run script. After the reboot
(machine up since 13:04) `card2` is PCI 0000:01:00.0, the RTX 5060 Ti; the RTX 4060 is `card1`, PCI 0000:05:00.0
(nvidia-smi index 1). So every number in the step 0 table was measured on the 5060 Ti, the monitor's card, not the
4060. The CPU shares are still the same kind of measurement, but they are not the 4060's. The run script now finds
the card by its PCI address and exits if the 4060 is not there.

### Lazy PICKS measurement on the RTX 4060 (`card1`, backlog 1605, step 3 preparation)

Each `standing_at` call's parts, medians of the probe's 5 s windows (ms of world work; the 4060 under load 8–12):

| Case | work | whole `standing_at` | draw callback | `standing_model` | `picture_quad` | PICKS push | placement (whole − draw) |
|---|---|---|---|---|---|---|---|
| zoomed out, 2.5D | 5.6 | 3.4 | 1.3 | 0.4 | 0.7 | 0.0 | 2.1 |
| zoomed out, third person | 6.6 | 3.35 | 0.8 | 0.5 | 0.6 | 0.0 | 2.55 |
| whole map, 2.5D | 9.1 | 6.1 | 2.6 | 0.7 | 1.2 | 0.0 | 3.5 |
| whole map, third person | 14.7 | 8.1 | 2.4 | 0.9 | 1.1 | 0.0 | 5.7 |

(Third person has 1–2 summary windows only; the others 5.)

What this says:

- **The PICKS push costs nothing measurable** (0.0 ms). Lazy PICKS as first described (record the sprites and
  build the quads on demand) would save about 0 ms: the quad is computed once and the push only copies it.
- **The quad is needed by the cull**: `standing_at` projects the picture to decide whether the sprite is on the
  window at all, and a sprite off the window is not drawn. So the quad is in the draw decision, not only in the pick.
  Skipping it would need a cheaper conservative bound (the foot's projection and the picture's largest extent),
  falling back to the exact quad only near the window's edges. That would be a change to the draw path, with its
  own byte-identity proof. Its ceiling is the quad's share, 0.6–1.2 ms (about 10% of the world work); the
  `standing_model` share (0.4–0.9 ms) would stay.
- **Most placement is neither the quad nor the model** (2.1–5.7 ms minus 1.0–1.6 ms): `push_cpu_model` and the
  magnification, the eye test, the fade's near band, `overlaps`, and the sharp perspective set-up. Not split further.

Not done: no pick test, no matrix run for a lazy-PICKS change (there is no change: the measurement build was
reverted). The cheaper-cull variant is a candidate for the coordinator's decision, not a landed change.
## Atmosphere effects: the fog test per location (item 4, 2026-11-12)

What changed: the location fog test (a cell of the footprint explored near the feather) was scanned for every
location in the point lights, the building windows and the drawn-locations test, every frame. It is now kept per
location and rebuilt only when the fog's change key moves (`location_explored`). The answer is a pure function of
the explored set, so the cache cannot change a picture.

Pictures: 24 snapshot cells (РК3-Столица and «Другой берег», 2.5D, third person and flat, hours 6, 13, 19, 23,
effects on, frames 30 and 90) rendered by `ade8f733` and by the fog cache: all 48 PNGs byte-identical.

Timing: not measured on a quiet machine. The RTX 4060 box was shared (load average 13 to 15): interleaved runs of
the same binary moved by a factor of 2 to 3 between rounds (`work_ms`, РК3 start, 2.5D, effects on: 0.41 to
1.10 ms; the baseline's effects-off round read 0.84 ms in one round and 0.33 ms in another). The per-section timers
that located the cost were run on the wrong card (see the reboot note in the session log) and are discarded, so no
gain figure is claimed here. Re-measure with `tools/perf/check.sh` on an idle machine.

Item 8 (background preparation at load) was tried and dropped: the first-frame bake and the textures moved to
frames 2 and 3 as stalls of 65 and 109 ms, which the owner rejects. The load path is unchanged.

## Map sprites cut to their boxes (item 6, the trim; 2026-11-12)

Each map sprite's picture is drawn over the box of its opaque texels (plus one texel), not the whole sprite;
the atlas keeps its layout, so every sampled texel is the one it sampled before. Trees, sun-shaded and night-lit
pictures keep the whole sprite (their sway, form shading and lit windows read the sprite's full frame).

Pictures: 48 shots (card1, «Другой берег», flat and 2.5D, sun and night, zoomed out and near), main against the trim:
every shot within 2 levels, no pixel above 7, minimum PSNR 77.2 dB.

Thirty-day test: `thirty_days_on_every_map` alone, `RAZDOR_PERF_BUDGET_SCALE=4`: passed (97.5 s).

GPU (`RAZDOR_GPU_STAGES=1`, card1, interleaved main and trim, two rounds, 40 s each; the medians of the frame
rows, per round):

| scene | main gpu sum p50 | trim gpu sum p50 | sprites p50 (main / trim) | draws |
|---|---|---|---|---|
| flat, zoomed out (round 1 / 2) | 5.85 / 5.83 ms | 5.85 / 5.87 ms | 0.24 / 0.24 ms | 940 both |
| 2.5D, zoomed out (round 1 / 2) | 4.29 / 4.31 ms | 4.29 / 4.43 ms | 0.49 / 0.51 ms | 216 both |

No measurable GPU saving on the RTX 4060 for these scenes: the sprite pass is under 0.5 ms here, so the fill
saving is inside the noise. (An earlier unpaired run showed large differences; it ran under load and is discarded.)
The trim is kept for the weaker GPUs of phones, which this card does not stand in for; not measured there.

## Map sprite copies in the atlas (item 7, branch `sprite-lod`; 2026-11-12)

The half and quarter copies of the map sprites sit under the full picture in the map atlas's texture; a decoration
draws the copy below 12 (1/2) and 6 (1/4) screen pixels a cell. Not on main: the look changes at whole-map zoom.

- Texture: «Другой берег» atlas 2048×1684 (13.8 MB RGBA) becomes 2048×2298 (18.8 MB): +5.0 MB (+36%) of VRAM.
  The stacked texture is checked against `GL_MAX_TEXTURE_SIZE` (32768 on the 4060; a phone with 4096 fits it);
  over the limit, the full atlas alone is used and the draws keep the full sprites.
- Draw counts unchanged: 940 flat, 216 2.5D, before and after (`RAZDOR_GPU_STAGES=1`, card1, whole-map zoom).
- GPU, sprite pass p50, interleaved, two rounds: flat 0.40 / 0.30 ms before, 0.14 / 0.14 ms after; 2.5D 0.76 / 0.51 ms
  before, 0.39 / 0.39 ms after. The totals swung with the machine's load (12.9 to 28 ms per frame on the same
  scene) and are not used. The Samsung lab run measures the real gain (the target is the phones' GPUs).
- Look: at whole-map zoom the canopies and rocks are softer (PSNR about 28 dB against main on the flat and 2.5D
  shots; the buildings and water change little). At 0.45 and 0.6 zoom the copies are not used (the cells are 12 px
  or more) and the shots differ by at most 2 levels (the atlas's height changed the texel rounding).
- Not done: an Android build (GLES2 has the same `GL_MAX_TEXTURE_SIZE` enum; the query runs once when the atlas is
  made).

### Sprite pass, steps 1+2: the static order in 16×16 chunks (backlog 1605; 2026-11-12, DROPPED, not pushed)

What it did: steps 1 and 2 together. Objects in 16×16-cell chunks, each sorted by (key, index), and locations sorted;
a heap merges the visible chunks, then the locations, then the figures. Non-turned views only. Pixels: byte-identical,
96/96 PNGs against origin/main (`67c91112`, rebased). Tests: the static order and the chunked rectangle walk equal the
per-frame sort (flat and 2.5D at yaw 0, several visible sets and rectangles, across chunk borders and past the edge).

Speed: slower. World pass CPU, card1 (RTX 4060), base and combined build interleaved (round 1; round 2 of the second
build was loaded for one case and is not quoted):

| Case | base work / world ms | steps 1+2 work / world ms |
|---|---|---|
| «Другой берег» zoomed out, 2.5D | 2.90 / 2.74 (round 2: 2.97 / 2.79) | 3.32 / 3.11 |
| «Другой берег» zoomed out, third person | 5.86 / 4.70 (round 2: 6.02 / 4.81) | 7.47 / 6.22 |
| «Другой берег» whole map, 2.5D | 9.28 / 9.08 (round 2: 9.46 / 9.21) | 10.33 / 10.01 (round 2: 10.26 / 9.97) |
| «Другой берег» whole map, third person | 14.29 / 12.33 (round 2: 14.30 / 12.35) | 15.84 / 13.79 (round 2: 15.88 / 13.85) |
| РК3 close-up, 2.5D | 0.65 / 0.51 (round 2: 1.88 / 1.36, loaded) | 0.71 / 0.55 (round 2: 1.93 / 1.39) |

Why it does not pay: the walk still visits every visible object each frame (the chunks only narrow the set), and the
heap merge over the visible chunks costs more than the per-frame sort it replaced. Decision: dropped, per the rule that
a step lands only with a measured gain. The commits are kept on the local branch `dropped/1605-steps-1-2`.

### Correction to step 3a (2026-11-12): the earlier timing compared against the wrong baseline

The step 3a section above compared 3a with a base built from `67c91112`. origin/main had moved on by then (the map
sprite copies, `3fbd5109`, and others), so that comparison mixed in other changes. The pixel comparison was redone
against `a220850b` (the commit 3a was built on): 96/96 byte-identical. The timings were redone interleaved against
`a220850b`, on the 4060, two rounds (other sessions loaded the machine in round 2):

| Case | base `a220850b` work (round 1 / 2) | 3a work (round 1 / 2) |
|---|---|---|
| «Другой берег» zoomed out, 2.5D | 2.74 / 2.64 | 2.47 / 2.83 |
| «Другой берег» zoomed out, third person | 5.68 / 5.46 | 5.12 / 5.53 |
| «Другой берег» whole map, 2.5D | 8.00 / 7.04 | 6.66 / 8.77 |
| «Другой берег» whole map, third person | 11.58 / 11.56 | 10.98 / 13.17 |
| РК3 close-up, 2.5D | 0.63 / 0.62 | 0.59 / 0.61 |

Round 1 is faster by 6–12 %, round 2 is slower by up to 14 %. The rounds disagree, so there is no measured gain.
Step 3a is dropped, not landed (the commit is on the local branch `dropped/1605-step-3a-viewproj-cache`). The earlier
numbers in the section above are not a claim.

## Map sprite fog in the shader (backlog 1605, step 4)

What changed: the fog and the reveals' visibility of the map sprites (decorations, buildings, hills) is no longer
computed per sprite on the CPU. `ui/sprite_fog.rs` keeps one 16-bit visibility per map cell as a texture, read by the
sharp shader at each sprite's foot cell (`sharp::FOG_GLSL`), and the CPU reads the same table for the cull and the
pick. Armies, the hero, spells, the smoke and the lit windows keep the CPU shade. The table is made again only when
the shown fog (`minimap::shown_stamp`) or the reveals move (every frame while a showing opens) and uploaded only when a
value changed. Purpose: the sprites' vertices no longer depend on the fog, which step 5 (GPU vertex buffers) needs.

Pictures (RTX 4060, VirtualGL, private Xvfb, 1024×768; base = `origin/main` `49e9540b`, built from `git archive`):

- The 48-run matrix, explored map (`matrix.sh`), 96 PNGs: 96/96 byte-identical (max 0 per channel, PSNR inf).
- The 48-run matrix, partly explored map (no `RAZDOR_SCENE_EXPLORED`, so the fog is in view), 96 PNGs: 96/96
  byte-identical.
- Fog reveal mid-run (`RAZDOR_SCENE_SHOW=40,40,6` on «Другой берег», partly explored; frame 90 is mid-reveal), 2.5D and
  third person: identical. The first version of the table did not follow the reveal while it opened (max 255 on the
  village that opened; PSNR 25 dB); the reveal is now made again every frame while a showing is live.

Two things made the pictures identical and are worth keeping in mind: the shader truncates the table's visibility to 8
bits, as the CPU's colour alpha did (without it, 3-level differences on the trees at the fog's edge: the old colour
alpha is truncated, not rounded); and the table reads the unrounded shown darkness, as `Shade::at` did.

Timing (world pass, the frame probe's `world` column, medians of frames after 100, two interleaved rounds, 30 s per
run, RTX 4060 through VirtualGL; the machine shared with other agents, so only the pairs compare):

| Case | base world ms | new world ms | base work ms | new work ms | draws (both) |
|---|---|---|---|---|---|
| «Другой берег», zoomed out (0.05), 2.5D | 2.163 | 2.167 | 2.329 | 2.343 | 173 |
| «Другой берег», zoomed out (0.05), third person | 3.378 | 3.438 | 4.845 | 4.888 | 172 |
| «Другой берег», whole map (fit), 2.5D | 7.256 | 7.045 | 7.472 | 7.270 | 215 |
| «Другой берег», whole map (fit), third person | 10.188 | 10.325 | 12.689 | 12.787 | 212 |
| РК3-Столица, 2.5D (close-up start) | 0.480 | 0.486 | 0.617 | 0.628 | 164 |

No measurable gain: the change is within ±3% (the sign varies by case). It removes the per-sprite fog arithmetic, but
placement and the draw calls, which dominate the pass (see the step 0 split above), are unchanged. It lands because
step 5 needs the sprites' vertices to stay static while the fog changes, and it is not slower beyond that noise.

Checks: `cargo clippy --release --all-targets -- -D warnings` clean; `cargo test --release` with the real install and
`RAZDOR_PERF_BUDGET_SCALE=4` all green (1714 + 1050 tests; `thirty_days_on_every_map` passed); the Android cross-compile
(`cargo check --release --target aarch64-linux-android --bin razdor`, NDK 29) passes; the UI regression check: 162 ok, 4
changed, all the «title-noinstall» cases, whose only difference from the baseline is the binary's path printed on the
no-install screen (the same four change against the goldens on `origin/main`).

## Sprite placement on the GPU: stage 1, the harness (backlog 1605, step 5)

Question: can the GPU place a map sprite the way the CPU does, to within the tolerance the pictures need? Stage 1
answers it with a test, no shader yet. `ui/sprite_gpu.rs` is the pure port (no macroquad drawing): the billboard's
terms (`Billboard`: the ground's right and the camera's up, and the size, the shader's uniforms) and the picture's
corners as world offsets from the foot (`quad_rel`). `Billboard::point` is `foot + size·rel.x·right − size·rel.y·up`.

Why it is that small. A map sprite's base is its foot (`standing_map`, `standing_picked_map`, and the objects'
`standing_map(cam, foot, foot, …)`), so `billboard_model_about` has no base term. The CPU's model maps a picture pixel
`p` to `ground + right·k·(p.x − foot.x) − up·k·(p.y − foot.y)` with `k = size/scale`; `p − foot` is `scale` times a
world offset (the cell geometry plus `plant_shift / PX` and the sprite's own pixels over `PX`), so the scale and the
origin cancel. The world offsets are the same for every zoom, and the zoom-dependent part is only the sprite's pixel
size (`zoom = scale/PX`), which the GPU takes from the camera.

Test: `ui/world_view/gpu_placement_tests.rs` (`the_gpu_placement_matches_the_cpu_on_every_sprite`, skipped without
`RAZDOR_DT_DIR`). The packing (`deco_packing`, `building_packing`) reads the map and the sprite sizes only. For each of
the first three maps of the install: every decoration and every building, under flat and under 2.5D (pitches 35, 55,
75; yaws 0, 37, 90, 180, 271), at scales 1.6, 6.4, 16, 32 and 48 (zoom 0.05 to 1.5). The CPU's corners are
`standing_model` + `picture_quad` on the tilted space, as the draw places them; the GPU's are the port's. The foot's
packed world point is also checked against the CPU's foot.

Result (fasttest, RAZDOR_DT_DIR, 2,646,000 sprite-camera pairs): worst corner error 0.0027 px (2.5D, scale 48, pitch 35, yaw 90,
a class 5 decoration); no pair above the 0.01 px bar.

Two things the test had to handle, both about the comparison, not the port:
- A sprite near the eye's depth plane projects to millions of pixels (the first run: 316,302 px and 38,810 pairs over
  0.01 px, all far off the window). There the CPU and GPU agree to f32 rounding amplified by 1/w, and their boxes do
  not meet the window. The test counts a pair only when either box meets the window with the cull's 16 px margin
  (`OFF_WINDOW_MARGIN`): only those can change a pixel. A pair that is on the window in one and not the other is
  always an error.
- The foot is compared in world units, scaled by the camera, so the 0.01 px bar is in the same pixels as the draw.

What stage 1 does not cover: the shader itself (macroquad's vertex stage, `Projection` as `window_matrix · view_proj`),
the sway, the bent views (`bend`: the sheet's `Bend::point`), the eyes view (`eye_at`), the first person's near fade
and the trailer's magnification. Those are stages 5b and 5c.

Stages 5a–5c are not started in this commit. What the stage 1 work says about them, for the next session:
- The vertex data needed per corner in 5a is the foot (2 floats), the corner offset (2), the atlas uv (2), the sharp
  scale per texel (2, unitless, times the zoom), the detail visibility (1), the sway weight (1), and the form shading's
  corner index (2 bits); the fog cell is not needed as an attribute, because it is the foot cell's and the shader can
  read it from the foot's world point. macroquad's `Vertex` has 3 + 2 floats, 4 bytes of colour and 4 floats of
  `normal`, so these fit only with packing (the colour bytes for the small values).
- The draw order is the risk, not the placement. Batching by chunk (one draw per chunk, in chunk order) can change the
  order of overlapping sprites from different chunks (reasoned from the sort key `sprite_key`, not measured). The order
  that keeps the pictures is the global sorted order, with a draw per run of consecutive decorations between two
  non-decoration items (buildings, armies, the hero, spells, ships). Runs are short where buildings are dense.
- macroquad queues its draw calls and submits them at the end of the frame, so a custom miniquad draw with a persistent
  buffer would run out of order. The only route inside macroquad is a `Mesh` per run, copied each frame: the saving is
  the per-sprite math (the billboard, the cull, the fog and shade, the form colours, the sharp draw set-up), not the
  copy. Whether that saves the 2.1 ms of zoomed-out placement is what 5a has to measure.

## Sprite placement on the GPU: stage 5a-i, the simple case (backlog 1605, step 5)

What changed: the plain map decorations (trees, rocks, plants: the `Drawable::Object` items of `draw_world_in`, drawn by
`draw_object` through `draw_picture_form` or `draw_leaning`) are placed by a vertex shader when `RAZDOR_GPU_SPRITES=1`
(off by default). Each maximal run of consecutive decorations in the sorted item list becomes one `Mesh`, filled this
frame: `world_view::gpu_object` appends four corners per sprite, `sharp::draw_billboard_mesh` draws the run with
`sharp::SHARP_VERTEX_BILLBOARD`, and any other item flushes the run first, so the draw order is the list's.

Packing (`SHARP_VERTEX_BILLBOARD` documents it): `position` is the sprite's foot in world units (y 0, from
`deco_packing`, the stage 1 packing moved out of the test); `normal.xy` is the corner's offset from the foot in world
units (`rel_tl + (off + c·size)/PX`, so a box is the picture's box and a whole tree is its whole picture); `normal.zw` is
the sprite's fog cell (`sprite_fog`, as the CPU's `FOG_CELL`); `texcoord` is the corner's texel; `color0` is white
(full visibility; the colour is bytes, so the shader divides by 255, as every sharp vertex stage does). The billboard's
right and up and the sprite size are uniforms set once per draw (`sprite_gpu::Billboard`, the same terms the stage 1
harness checks), and `ScaleU` is the zoom (the CPU's sharp `Scale`: screen pixels per texel, times the raster factor).
The shader is `Projection · Model · (foot + size·rel.x·right − size·rel.y·up)`; the tilted pass's projection is the
same `window_matrix · view_proj` that the CPU's sprites go through, so no matrix is computed per sprite.

Eligible (anything else keeps the CPU path, byte for byte as before): the 2.5D view, not bent (the trailer, the first
person's `bend` and the eyes view keep the CPU: their per-sprite near fade and magnification are CPU maths); the map
animation off (`ctx.anim == 0`); no sun colour (`ctx.sun`); no lit material (`lit` is none: the light is off); no night
whole-sprite switch; per sprite: the detail visibility is 1 (a sprite in the fade band, 2 to 3 px, is the CPU's; a
sprite at 0 is not drawn, as on the CPU), no smaller LOD copy used (`decoration_lod`), and the box at 1:1 texels (so the
CPU's sharp scale is the zoom). A fogged cell that hides the sprite draws nothing, as on the CPU.

Two packing details that the proof found:
- The fog cell is the CPU's: `map_fog` of the same 2D foot (`object_foot`), not of the packed world point. A foot on a
  cell's edge (y = 25.78125 on «Другой берег») lands on the other side of the edge after float rounding, and whole
  trees were then hidden or shown (max 172 levels, PSNR 43.8 dB on the partly explored map before the fix).
- A run is at most 800 sprites (4800 indices). Macroquad's batcher clamps one geometry to its draw call's index capacity
  (5000) and then the indices read past the vertices (the first try drew red slabs over the frame).

The shader's colour attribute: `Byte4` is not normalised; the first try multiplied by the bytes (white blobs). The
test `the_billboard_vertex_stage_is_gles2_glsl_and_reads_the_byte_colour_normalised` pins the `/ 255.0` and GLSL ES 1.00.

Pictures (RTX 4060 for the matrix too, VirtualGL, private Xvfb, 1024×768, anim off; base = `origin/gpu-sprites` `2a8e4230`
built from `git archive`; new = this commit with `RAZDOR_GPU_SPRITES=1`). Matrices: `matrix5a.sh` (the perf-retain
matrix, 48 runs × 2 frames each, the runs split over both cards by half). `cmp_matrix.py` pairs the PNGs by name:
- Explored map, 96 PNGs: 88 identical; 7 at max 2 per channel (PSNR 73–79 dB); one shot, `rk3-25d-z05-off` frame 60,
  max 13 on 17 pixels out of 786,432, all on one row of a sprite's edge (PSNR 72.8 dB). The brief's bar is max 2; this
  shot is over it by 11 levels on 17 pixels. Its cause is a sub-texel edge: the packed corners differ from the CPU's
  by the stage 1 bound (0.003 px), and a sprite edge on a texel boundary takes the other texel. Not fixed; the
  pixels are reported, not hidden.
- Partly explored map (no `RAZDOR_SCENE_EXPLORED`, the fog in view), 96 PNGs: 90 identical; the other 6 at max 2 (PSNR
  78.7 dB or more). The fog-cell fix above is what made this one pass.
- Mid-run reveal (`RAZDOR_SCENE_SHOW=40,40,6` on «Другой берег», partly explored; frames 30 and 90): 2.5D max 2 (PSNR
  81.7 dB); third person identical (not eligible).

Picks (`RAZDOR_PICK_DUMP=1`, a developer switch: after the tilted pass, a digest of `PICKS` and of `front_pick` on a
4-px grid of the window, to stderr): six scene and zoom cases (both scenes; default, zoom 0.05, zoom 0.05 fit), 2.5D,
the GPU path off and on, two frames each with 45 digests per run: 90 of 90 identical in every case. Decorations are
never picked (`Pick`: hero, armies and locations only), so the pick records are not touched by the path; the digest
shows it. The flat view draws no pick record and has no GPU path.

Coverage (the frame's count of decorations, `gpu_object` placed vs the CPU's draws; a temporary counter, not in the
commit): «Другой берег» zoom 0.05, 2.5D: 4,169 placed, 0 on the CPU per frame. «Другой берег» whole map (fit), 2.5D:
0 placed, 12,805 on the CPU (every sprite is in the fade band, 2 to 3 px). РК3-Столица, 2.5D default: 909 placed, 0 on
the CPU.

Timing (world-pass ms and work ms, the frame probe's CSV, medians of the frames after the first 100, 30 s per run,
interleaved, two rounds (round 2 in the reverse order), RTX 4060 through VirtualGL, anim off in both; the machine was
shared: another session's trailer render and encoder ran during round 1, load 15): the controls (third person, whole
map, no GPU decorations) move by 30–40 % between runs, so the spread is about the size of any effect here.

| Case | base world ms (r1, r2) | new world ms (r1, r2) | base work ms | new work ms | draws base → new |
|---|---|---|---|---|---|
| «Другой берег», zoom 0.05, 2.5D (4,169 placed) | 4.17, 4.63 | 5.41, 6.21 | 4.62, 5.07 | 5.96, 6.74 | 173 → 221 |
| «Другой берег», whole map (fit), 2.5D (0 placed) | 18.01, 12.32 | 17.01, 22.97 | 19.11, 12.76 | 17.63, 23.54 | 215 → 215 |
| РК3-Столица, 2.5D (909 placed) | 0.60, 0.60 | 1.15, 0.63 | 0.73, 0.73 | 1.62, 0.89 | 156 → 169 |
| control: «Другой берег», zoom 0.05, third person | 3.91, 6.33 | 5.03, 5.97 | 5.74, 10.04 | 10.09, 9.43 | 172 → 172 |
| control: «Другой берег», whole map, third person | 16.88, 14.34 | 16.94, 16.76 | 30.08, 20.21 | 30.14, 29.93 | 212 → 212 |

Verdict: no measured gain, and the zoomed-out 2.5D case is slower in both rounds (+30 % and +34 % of the world pass).
The sprites' placement math is now on the GPU, but the run structure costs more: the item list alternates decorations
with buildings, armies and the hero, so the runs are short and each one is a draw call with its own uniform sets and
texture binding (173 → 221 draws in the zoomed case). The per-sprite work that is left (the cell lookup, the foot, the
packing) is about what the CPU's placement cost was once the draw calls are counted. The whole map is not on the GPU at
all: its sprites are in the detail fade, which is the CPU's in this stage. The controls are not the point of the
measurement and show the noise that makes a 30 % difference unresolved on this machine; the zoomed-out slowdown is
consistent across both rounds but has not been reproduced on a quiet machine.

Left for the next stages (not done here):
- 5a-ii, the detail fade: the sprites at 2 to 3 px (`detail_visibility`, the whole map's case), as a per-vertex
  visibility (one more packed float, the colour alpha) with the same 8-bit truncation the CPU has.
- 5a-iii, the LOD copies (`decoration_lod`: a different texel rectangle with the full sprite's size, per vertex) and
  the night's whole-sprite switch (`lit_whole`).
- The sun's colour per corner (`draw_picture_form`'s `form_colour_in`, four colours per sprite: the colour attribute
  can carry them) and the lit material (`LIT_MAIN`'s painted-shadow alpha in the shader, with `ShadowK`).
- The sway of the trees (`draw_leaning`'s lean, 5b, with the phase and the sway amplitude in the vertex).
- The bent views and the eyes and first-person views (5c): their sheet `Bend::point` and their per-sprite near fade and
  magnification; those need the stage 1 harness extended to the bent and eyes cameras first.
- The run structure is the real limit: the saving is only worth taking when the runs are long, which needs the sorted
  list grouped by texture and by layer (a different draw order is not allowed: the pictures must stay).

Checks: `cargo clippy --release --all-targets -- -D warnings` clean; the fasttest targeted tests (the stage 1 harness on
the install, and the new shader test) pass; the Android cross-compile (`cargo check --release --target
aarch64-linux-android --bin razdor`, NDK 29 clang) passes with the one unused-variable warning in `src/update.rs`, which
is not this change's; the full `cargo test --release` with the real install and `RAZDOR_PERF_BUDGET_SCALE=4`: 1714 + 1052
passed (the placement harness and the shader test among them); `cargo clippy --release --all-targets -- -D warnings`
clean on the final tree.

Timing note: the timings ran while another session rendered on the same machine; the owner then started playing, so
the coordinator moved the jobs to CPUs 8–19 at nice 19 (the RTX 4060 is the only GPU used for pictures and timings).
