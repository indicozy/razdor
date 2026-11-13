# Low-spec PC gameplay benchmark: 1.4.1 vs 1.5.1 vs main (2026-11-13)

Scope: live gameplay on a simulated weak PC, three builds, one round. Each cell is one run; the
numbers in the table are that run's medians, so there is no spread across rounds. The owner cut
the benchmark to one round, so treat differences of a few percent as noise: this round does not
measure run-to-run variation.

## Setup

Builds (release, `--no-default-features`, Mesa llvmpipe):

| build | source | version line |
|---|---|---|
| v1.4.1 | tag `v1.4.1` (31a85c96) | Razdor 1.4.1 |
| v1.5.1 | tag `v1.5.1` (c52a50d9) | Razdor 1.5.1 |
| main | dfdf5e12 | Razdor 1.5.1 (dfdf5e12) |

Each binary was built from its own checkout in a shared target dir, cleaning the razdor package
before each build (the three checkouts share one cargo hash for the crate).

Machine and run:
- Xvfb 1366x768x24 and the game pinned to CPUs 18-19 at nice 19; the driver on CPUs 8-17.
- Mesa llvmpipe: `LIBGL_ALWAYS_SOFTWARE=1 GALLIUM_DRIVER=llvmpipe LP_NUM_THREADS=2`.
- Live runs (no `RAZDOR_SNAPSHOT`): the game's own clock and decode queue. Environment:
  `RAZDOR_SCENE` (the cell's scene), `RAZDOR_SCENE_QUIET=1`, `RAZDOR_NO_AUDIO=1`,
  `RAZDOR_HUB=mock`, `RAZDOR_PROFILE=1` with `RAZDOR_PROFILE_FRAMES=<csv>`, and the install.
- Settings file (`audio.json` in the run's save folder): music and sound muted, `updates: off`,
  `display: borderless` (fills the 1366x768 screen), the view (`map_tilt` false for flat, true for
  2.5D), and for v1.5.1 and main the preset's values (below).
- The remote config fetch at start was left on (one small request, the same for every build).
- Window size is not set with `RAZDOR_SIZE`: that variable routes `display::follow_settings` to the
  snapshot branch, which ignores the settings file (light, shadows, mist, water, weather, living).
  Earlier runs with it were discarded; all numbers here come from the corrected setup.

Presets (v1.5.1 and main, written into `audio.json`; the values of `quality_preset::look`):
- «Оригинал» (orig): render scale 1.0, light Off, shadows Painted, mist Off, live water off,
  weather off, living map off, animation On. This is also the install default.
- «Высокое» (high): render scale 1.0, light GameClock, light quality High, shadows Sun, mist
  Clouds, live water on (rate EveryFrame, quality High), weather on, living map on, animation On.
- v1.4.1 has no presets and runs with its own defaults (the view is still set through `map_tilt`).

Cells, each with the same inputs for every build:
- **walk**: scene `walk:Другой берег:-36,-14` (the longest route all three builds accept), with
  `walk_speed` 0 (the slowest pace the original allows; a step takes 400 ms against 150 ms at the
  default). The window is from 3 s to the arrival. The map has no route longer than about 36 cells,
  so at the slowest pace the walk lasts about 12-13 s at 11-15 fps and about 26 s at 5 fps (the same
  route takes longer when frames are slower). The owner asked for at least 15 s; one destination on
  this map cannot give that at the slowest pace.
- **battle**: scene `battle:Другой берег:1` (a real fight against «Отряд фон Бигхолл»). W (watch) is
  pressed at 2 s, so the units act and fight live. The window is 3-30 s.
- **scroll**: scene `map:Другой берег` with the map explored (`RAZDOR_SCENE_EXPLORED=1`; unexplored,
  most of the panned view is fog). Left arrow held from 3 s to 13 s. The window is that hold.

Measurement: the frame probe CSV (`RAZDOR_PROFILE_FRAMES`) is flushed every 5 s, and each run
ends with SIGKILL at 32 s, so the window is the flushed rows inside it. FPS is frames divided by
the summed frame time in the window. Median and p99 are of the frame time (total, including the
present). "Work" is the game's frame time, "present" the rest (under llvmpipe, rasterisation is
charged to present).

## Verification

Every run was checked for the activity it measures and for a changed screen:
- Walk: walking frames in the window (`walk-step`: 124-185 per run) and the screen at 1 s differs
  from the screen at the end. Views: the 2.5D runs show `25d` in the CSV view column.
- Battle: battle-effect frames (285-332 per run, units striking) and a changed screen.
- Scroll: the Left hold moved the view: the screens at 1 s, 6 s, 9.5 s and the end all differ, so the
  pan is continuous; the frames are idle (no walk, no battle).
- Panic: none in any run. Errors: none.
- Preset applied: yes (v1.5.1 and main, high cells). Checks: (1) the first live frame of «Высокое»
  and «Оригинал» differ in 7.7% of pixels, with lit windows and a haze visible in the high frame;
  (2) the stage means go up: 2.5D walk, world stage 22 ms (orig) against 72 ms (high), terrain 20
  against 23; (3) `RAZDOR_SCENE_PRESET=high` with `audio.json` at «Оригинал» gives the same numbers
  as `audio.json` at «Высокое» (5.2 and 5.1 fps; world stage 68 and 64 ms), so the two routes agree.

## Results (round 1, one run per cell; medians within the run)

FPS, frame time (ms), 99th percentile (ms), share of frames over 50 ms, frames in the window.

| cell | view | preset | build | FPS | frame ms | p99 ms | >50 ms | frames |
|---|---|---|---|---|---|---|---|---|
| walk | flat | default | v1.4.1 | 13.7 | 72.3 | 79.0 | 100% | 166 |
| walk | flat | orig | v1.5.1 | 15.5 | 64.5 | 71.9 | 100% | 189 |
| walk | flat | high | v1.5.1 | 5.6 | 181.7 | 217.8 | 100% | 133 |
| walk | flat | orig | main | 15.6 | 63.6 | 71.8 | 100% | 187 |
| walk | flat | high | main | 5.6 | 182.3 | 218.0 | 100% | 133 |
| walk | 2.5D | default | v1.4.1 | 9.6 | 102.2 | 117.3 | 100% | 127 |
| walk | 2.5D | orig | v1.5.1 | 11.8 | 80.9 | 101.5 | 100% | 145 |
| walk | 2.5D | high | v1.5.1 | 5.1 | 197.2 | 238.8 | 100% | 136 |
| walk | 2.5D | orig | main | 11.7 | 81.3 | 111.3 | 100% | 142 |
| walk | 2.5D | high | main | 5.1 | 200.7 | 239.2 | 100% | 135 |
| battle | flat | default | v1.4.1 | 28.0 | 35.3 | 40.8 | 0.1% | 755 |
| battle | flat | orig | v1.5.1 | 26.2 | 37.1 | 57.6 | 2.0% | 705 |
| battle | flat | high | v1.5.1 | 26.8 | 36.9 | 43.1 | 0.1% | 722 |
| battle | flat | orig | main | 26.1 | 37.4 | 52.2 | 1.9% | 703 |
| battle | flat | high | main | 26.2 | 37.2 | 54.8 | 1.8% | 706 |
| battle | 2.5D | default | v1.4.1 | 27.8 | 35.5 | 40.7 | 0.1% | 749 |
| battle | 2.5D | orig | v1.5.1 | 25.5 | 37.1 | 70.8 | 5.5% | 686 |
| battle | 2.5D | high | v1.5.1 | 24.8 | 38.9 | 64.2 | 3.9% | 669 |
| battle | 2.5D | orig | main | 25.7 | 37.5 | 57.9 | 2.7% | 694 |
| battle | 2.5D | high | main | 26.3 | 37.3 | 46.1 | 0.7% | 709 |
| scroll | flat | default | v1.4.1 | 12.4 | 79.5 | 98.3 | 100% | 123 |
| scroll | flat | orig | v1.5.1 | 18.8 | 51.9 | 70.8 | 100% | 187 |
| scroll | flat | high | v1.5.1 | 6.5 | 152.3 | 167.8 | 100% | 65 |
| scroll | flat | orig | main | 18.8 | 52.1 | 70.2 | 100% | 186 |
| scroll | flat | high | main | 6.5 | 153.0 | 169.6 | 100% | 64 |
| scroll | 2.5D | default | v1.4.1 | 8.1 | 122.8 | 130.6 | 100% | 80 |
| scroll | 2.5D | orig | v1.5.1 | 10.9 | 89.6 | 108.8 | 100% | 109 |
| scroll | 2.5D | high | v1.5.1 | 4.9 | 201.0 | 225.9 | 100% | 48 |
| scroll | 2.5D | orig | main | 10.9 | 90.0 | 109.0 | 100% | 109 |
| scroll | 2.5D | high | main | 5.0 | 200.1 | 219.0 | 100% | 49 |

v1.4.1 has no presets: its row is its own defaults. The «orig» rows of v1.5.1 and main are the
install default look.

Stage means (ms per frame, the window's mean), the key cells:

| cell | build | work | present | terrain | world | battle window | draws/frame |
|---|---|---|---|---|---|---|---|
| walk 2.5D | v1.4.1 | 2.5 | 101.4 | - | 1.8 | - | - |
| walk 2.5D | v1.5.1 orig | 23.0 | 61.7 | 20.4 | 22.4 | - | 182 |
| walk 2.5D | v1.5.1 high | 72.5 | 123.6 | 22.8 | 71.8 | - | 188 |
| walk 2.5D | main orig | 23.7 | 61.7 | 21.0 | 23.0 | - | 181 |
| walk 2.5D | main high | 73.0 | 124.9 | 23.2 | 72.2 | - | 188 |
| scroll 2.5D | v1.5.1 orig | 4.9 | 86.7 | 2.4 | 4.5 | - | 177 |
| scroll 2.5D | v1.5.1 high | 6.6 | 196.9 | 2.8 | 6.2 | - | 233 |
| scroll flat | v1.5.1 orig | 0.9 | 52.4 | - | 0.6 | - | 248 |
| scroll flat | v1.5.1 high | 1.3 | 151.7 | - | 0.9 | - | 396 |
| battle 2.5D | v1.4.1 | 0.9 | 35.1 | - | - | 0.6 | - |
| battle 2.5D | v1.5.1 orig | 1.1 | 38.1 | - | - | 0.8 | 373 |
| battle 2.5D | v1.5.1 high | 1.3 | 39.1 | - | - | 0.8 | 372 |

## Reading

Answers to the owner's questions, from this one round:

1. **1.5.x against 1.4.1, on the default look (per cell).** Walking is faster: flat 13.7 to 15.5
   FPS (+13%), 2.5D 9.6 to 11.8 (+23%). Scrolling is faster: flat 12.4 to 18.8 (+51%), 2.5D 8.1 to
   10.9 (+35%). Battles are slightly slower: flat 28.0 to 26.2 (-7%), 2.5D 27.8 to 25.5 (-8%), and
   the frames are less even: the share of frames over 50 ms is 0.7-5.5% against 0.1% in 1.4.1 (p99 52-71
   ms against 41 ms). The walk and scroll gains are real on this round's scale; the battle loss is
   small but has the same single-run caveat.
2. **The «Высокое» preset on 1.5.x and main.** It costs a lot where the map is drawn: walking flat
   15.5 to 5.6 FPS (2.8x slower), 2.5D 11.8 to 5.1 (2.3x); scrolling flat 18.8 to 6.5 (2.9x), 2.5D
   10.9 to 4.9 (2.2x). In battles it costs 1-3%, because the battle window covers most of the
   frame. The walk and scroll slowdown is the 3x the earlier measurement showed; the earlier 2-3%
   came from the settings not being read (see Setup).
3. **What dominates.** Under llvmpipe the present (rasterisation of the frame) is 60-70% of frame
   time in every map cell, so frame time follows pixels and draw calls. Game work is small on the
   default look (walk 2.5D: 23 ms, most of it terrain and world drawing; scroll: about 5 ms). The
   high preset adds about 50 ms of world work to a 2.5D walk (the light and shadow shading: world
   stage 22 to 72 ms) and about 50 draw calls to the 2.5D scroll (177 to 233), and in flat scroll
   it more than doubles the draw calls (248 to 396).
4. **Main against v1.5.1.** No difference beyond 1% in any cell, for either preset, within this
   one round. Main shows no regression from 1.5.1 here. The round does not measure run-to-run
   spread, so a difference under about 3% should not be read as real.

Caveats:
- One round, one run per cell. The owner cut the benchmark to one round; no spread across rounds.
- The walk cell is 12-13 s of walking at the default-look builds and about 26 s at 5 fps, not the
  15 s the owner asked for: one destination on this map cannot give that at the slowest pace.
- v1.4.1 has no quality presets, so its row is not the same look as the 1.5.x presets.
- The remote config fetch at start ran in every run (the same for every build).
- Frames were measured at 1366x768 with llvmpipe on two E-cores. These are software-rasteriser
  numbers, not a player's GPU; they show the relative cost of the look, not its frame rate on a
  real low-spec PC.

No performance idea was tried in this change, so `docs/perf/ideas-ledger.md` gets no row.
