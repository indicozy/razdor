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
