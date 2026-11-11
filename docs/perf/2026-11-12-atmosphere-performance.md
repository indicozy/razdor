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
