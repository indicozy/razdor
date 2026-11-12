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
