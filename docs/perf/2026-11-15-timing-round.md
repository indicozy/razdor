# Timing round, 2026-11-15: frame cost per milestone, re-measured in one sitting

One consistent re-measurement of the performance steps of 2026-11-11 to 2026-11-15, for the history page
("how frame time per render stage changed"). Every number here is from the same day, the same scene
definitions, the same measuring scripts and the same machine state rules. Nothing from earlier runs is mixed in.
Earlier ledger numbers come from loaded or mixed runs and are marked in `docs/perf/ideas-ledger.md`.

## Method

- **Build.** Eight milestone commits built in release with `--no-default-features` from detached worktrees
  (one shared target directory outside the repo; binaries copied per milestone). Milestones:
  M0 v1.5.1 (`c52a50d9`), M1 `c54de532`, M2 `7a6d798f`, M3 `1933b9eb`, M4 `386962f1`, M5 `5a25ef6d`,
  M6 `967e2683`, M7 `70222bed` (current main). Each build was checked for its switches; a switch that an old
  commit lacks is recorded as n/a, never backfilled.
- **Scene.** `Другой берег` from the install, explored (`RAZDOR_SCENE_EXPLORED=1`), `RAZDOR_SCENE_QUIET=1`,
  map animation off (`RAZDOR_MAP_ANIM=off`), no audio. Whole map = `RAZDOR_MAP_FIT=1`; close-up = the default zoom
  at the hero. Window 1600x720 for every scene; 1024x768 for two milestones. Flat = `RAZDOR_VIEW=flat`,
  2.5D = `RAZDOR_VIEW=25d`.
- **CPU measure (instructions).** Main-thread user instructions with `perf stat -e cpu_core/instructions/u`,
  pinned to P-cores 12-15 (the hybrid CPU's P-core type only). Per frame = (I400 - I100) / 300, so the first
  100 frames are excluded. Both on llvmpipe (Xvfb, own display and `XDG_RUNTIME_DIR`) and on the RTX 4060 (card by
  PCI `0000:05:00.0`, through VirtualGL). The 5060 Ti was not used.
- **Time measure (frame probe).** `RAZDOR_PROFILE=1` with `RAZDOR_PROFILE_FRAMES`, 400 frames, RAZDOR_GPU_FINISH=1
  on the 4060. `finish_ms` is not pure GPU time: `finish_wait` (`src/ui/gpu_probe.rs`) flushes macroquad's batch and
  then calls `glFinish`, so it is the submission of the frame's draws plus the wait for the GL queue. In every
  probe cell of this round `finish_ms - submit_ms` is 0.03 to 0.17 ms (checked per cell; the close-ups and the walk
  cells with a non-zero `flush_ms` are at the top of that range). Stage medians and p90 over frames 101-400 (299 frames).
  Total frame ms is the probe's `total_ms`. The probe's `work_ms` is the CPU side of the frame.
- **Machine state.** Before every run the one-minute load average had to be below 4 (the runs waited up to
  2 hours). The load at the start and the end of every run is in `results.csv`. Many runs started under 4 and
  finished above it, because another session ran ffmpeg and Chrome. The instruction counts were stable under
  that load (the gate below); the stage milliseconds were not always (see anomalies).
- **Walk.** The walk `walk:Другой берег:0,-12` moves the hero for frames 1-144 only (the longest reachable path
  found; `0,25`, `0,12` and `12,0` gave "no way there"), then the map is idle. The walk is therefore measured on
  its own: instructions per frame = (I140 - I10) / 130 (frames 11-140 all walking; the first 10 frames are skipped
  instead of 100, a deliberate deviation), and the probe stats are taken over all 144 walking frames.
- **Battle.** `battle:Другой берег:0` (army 0): 299 frames, kinds `battle` and `battle-effect`.
- **Pan.** There is no map-pan scene in `src/ui/snapshot.rs` (`RAZDOR_SCENE_PAN` pans windows), so no
  pan measure. The walk's camera follows the hero and stands in for scrolling.

## Gate (method stability): PASS

Three identical runs of M7, flat whole map, 1600x720, through the same script:

| Run | llvmpipe Minstr/frame | 4060 Minstr/frame | probe work ms (med) | probe total ms (med) | probe finish ms (med) |
|---|---|---|---|---|---|
| gate1 | 61.94 | 28.84 | 2.54 | 5.53 | 1.47 |
| gate2 | 61.97 | 28.27 | 2.55 | 5.51 | 1.47 |
| gate3 | 61.94 | 28.30 | 2.54 | 5.48 | 1.47 |

Spread: llvmpipe 0.05 %, 4060 instructions 2.0 % (28.27 to 28.84), stage
medians under 1 %. The brief's limits (2 % instructions, 5 % stages) are met.

What was changed to get here: the frame count is fixed (100 and 400 frames for instructions, 400 for the probe),
animation is off (`RAZDOR_MAP_ANIM=off`), the scene is set by one `RAZDOR_SCENE` string per run, and the
timing run is separate from the instruction run (the probe adds work, so the instruction runs run without it).
Found and fixed in the first matrix: the walk cells of that first pass measured idle frames (the walk ends at
frame 45 or 144 and the window started at frame 101). Those cells are kept in
`superseded-walk-idle/` for the record and replaced by the walk method above.

## Results

Milliseconds are medians of the probe's per-frame values (p90 in brackets). "n/a" = the switch or stage does not
exist in that build. Minstr = million instructions per frame, main thread.


#### Flat, whole map explored (fit zoom), 1600x720

| Milestone | llvmpipe Minstr/frame | 4060 Minstr/frame | work ms med (p90) | total ms med (p90) | finish ms med | gl_draws med | gl_bytes med (kB) |
|---|---|---|---|---|---|---|---|
| M0 | 111.3 | 95.0 | 5.08 (5.20) | 15.95 (20.88) | n/a | n/a | n/a |
| M1 | 82.4 | 48.6 | 4.23 (4.30) | 7.23 (7.38) | n/a | n/a | n/a |
| M2 | 84.8 | 51.1 | 4.40 (4.53) | 7.30 (10.43) | n/a | n/a | n/a |
| M3 | 85.5 | 52.6 | 4.68 (4.78) | 7.75 (10.67) | n/a | n/a | n/a |
| M4 | 86.0 | 53.0 | 4.71 (4.80) | 7.61 (8.91) | n/a | n/a | n/a |
| M5 | 79.3 | 45.5 | 4.26 (4.72) | 7.56 (10.30) | n/a | n/a | n/a |
| M6 | 61.0 | 27.2 | 2.41 (2.73) | 5.53 (6.39) | 1.50 | 137.0 | 2393 |
| M7 | 61.9 | 28.8 | 2.54 (2.83) | 5.53 (6.07) | 1.47 | 137.0 | 2393 |

#### 2.5D, whole map explored (fit zoom), 1600x720

| Milestone | llvmpipe Minstr/frame | 4060 Minstr/frame | work ms med (p90) | total ms med (p90) | finish ms med | gl_draws med | gl_bytes med (kB) |
|---|---|---|---|---|---|---|---|
| M0 | 134.2 | 107.5 | 10.94 (11.82) | 12.95 (16.21) | n/a | n/a | n/a |
| M1 | 111.5 | 86.4 | 9.10 (10.23) | 11.48 (14.83) | n/a | n/a | n/a |
| M2 | 113.0 | 88.6 | 9.46 (9.96) | 11.51 (14.23) | n/a | n/a | n/a |
| M3 | 107.8 | 82.4 | 9.34 (9.47) | 13.43 (13.76) | n/a | n/a | n/a |
| M4 | 107.8 | 83.1 | 9.40 (10.96) | 12.12 (15.12) | n/a | n/a | n/a |
| M5 | 106.9 | 82.0 | 9.41 (11.54) | 13.72 (18.87) | n/a | n/a | n/a |
| M6 | 107.4 | 83.7 | 8.36 (9.27) | 10.32 (13.63) | 0.49 | 138.0 | 2394 |
| M7 | 82.7 | 57.5 | 6.97 (7.74) | 9.50 (12.13) | 0.52 | 138.0 | 2394 |

#### Flat, close-up explored, 1600x720

| Milestone | llvmpipe Minstr/frame | 4060 Minstr/frame | work ms med (p90) | total ms med (p90) | finish ms med | gl_draws med | gl_bytes med (kB) |
|---|---|---|---|---|---|---|---|
| M0 | 17.6 | 18.0 | 0.84 (1.20) | 4.07 (5.32) | n/a | n/a | n/a |
| M1 | 13.5 | 12.3 | 0.78 (0.92) | 3.27 (3.76) | n/a | n/a | n/a |
| M2 | 13.7 | 12.8 | 0.69 (0.72) | 2.93 (3.03) | n/a | n/a | n/a |
| M3 | 13.8 | 12.7 | 1.23 (1.71) | 4.46 (5.70) | n/a | n/a | n/a |
| M4 | 13.9 | 12.9 | 0.74 (0.80) | 3.00 (3.11) | n/a | n/a | n/a |
| M5 | 13.2 | 12.0 | 0.64 (0.72) | 2.85 (3.23) | n/a | n/a | n/a |
| M6 | 11.6 | 10.6 | 0.56 (0.58) | 2.77 (2.87) | 0.84 | 111.0 | 265 |
| M7 | 11.8 | 10.8 | 0.58 (0.65) | 2.79 (3.03) | 0.83 | 111.0 | 265 |

#### 2.5D, close-up explored, 1600x720

| Milestone | llvmpipe Minstr/frame | 4060 Minstr/frame | work ms med (p90) | total ms med (p90) | finish ms med | gl_draws med | gl_bytes med (kB) |
|---|---|---|---|---|---|---|---|
| M0 | 18.0 | 17.7 | 1.73 (1.95) | 3.82 (4.47) | n/a | n/a | n/a |
| M1 | 17.2 | 16.3 | 1.44 (1.49) | 3.40 (3.61) | n/a | n/a | n/a |
| M2 | 17.3 | 16.3 | 1.48 (1.56) | 3.44 (3.60) | n/a | n/a | n/a |
| M3 | 16.7 | 15.8 | 1.40 (1.54) | 3.31 (3.62) | n/a | n/a | n/a |
| M4 | 16.7 | 15.7 | 1.59 (3.02) | 3.65 (6.42) | n/a | n/a | n/a |
| M5 | 16.6 | 15.9 | 1.37 (1.40) | 3.29 (3.34) | n/a | n/a | n/a |
| M6 | 16.7 | 15.9 | 1.42 (1.77) | 3.44 (4.19) | 0.61 | 98.0 | 210 |
| M7 | 14.9 | 13.9 | 2.42 (3.12) | 5.63 (7.13) | 0.84 | 98.0 | 210 |

#### Flat walk (frames 1-144 walking), 1600x720

| Milestone | llvmpipe Minstr/frame | 4060 Minstr/frame | work ms med (p90) | total ms med (p90) | finish ms med | gl_draws med | gl_bytes med (kB) |
|---|---|---|---|---|---|---|---|
| M0 | 15.9 | 16.8 | 0.66 (1.94) | 3.30 (4.72) | n/a | n/a | n/a |
| M1 | 12.5 | 12.5 | 0.69 (2.35) | 2.92 (7.46) | n/a | n/a | n/a |
| M2 | 12.7 | 12.6 | 0.91 (2.84) | 3.58 (7.57) | n/a | n/a | n/a |
| M3 | 13.1 | 12.6 | 1.08 (2.46) | 3.53 (5.53) | n/a | n/a | n/a |
| M4 | 13.0 | 12.8 | 0.77 (1.95) | 2.95 (4.36) | n/a | n/a | n/a |
| M5 | 13.0 | 12.9 | 0.75 (1.80) | 2.89 (4.16) | n/a | n/a | n/a |
| M6 | 12.1 | 12.0 | 1.35 (3.23) | 4.57 (7.83) | 0.95 | 80.0 | 191 |
| M7 | 12.2 | 11.3 | 0.74 (1.83) | 2.96 (4.55) | 0.78 | 80.0 | 191 |

#### 2.5D walk (frames 1-144 walking), 1600x720

| Milestone | llvmpipe Minstr/frame | 4060 Minstr/frame | work ms med (p90) | total ms med (p90) | finish ms med | gl_draws med | gl_bytes med (kB) |
|---|---|---|---|---|---|---|---|
| M0 | 16.2 | 16.6 | 1.26 (2.52) | 3.21 (5.01) | n/a | n/a | n/a |
| M1 | 15.0 | 15.2 | 1.24 (3.94) | 3.35 (8.47) | n/a | n/a | n/a |
| M2 | 15.0 | 15.1 | 1.21 (4.02) | 3.22 (7.90) | n/a | n/a | n/a |
| M3 | n/a | n/a | n/a (n/a) | n/a (n/a) | n/a | n/a | n/a |
| M4 | (not run) | | | | | | |
| M5 | (not run) | | | | | | |
| M6 | (not run) | | | | | | |
| M7 | (not run) | | | | | | |

#### Whole map at 1024x768, M0 and M7

| Milestone | llvmpipe Minstr/frame | 4060 Minstr/frame | work ms med (p90) | total ms med (p90) | finish ms med | gl_draws med | gl_bytes med (kB) |
|---|---|---|---|---|---|---|---|
| M0 flat whole | 114.2 | 95.1 | 4.96 (5.07) | 18.39 (19.28) | n/a | n/a | n/a |
| M7 flat whole | 64.5 | 31.4 | 2.41 (2.46) | 5.45 (5.58) | 1.98 | 223.0 | 2393 |
| M0 2.5D whole | 138.7 | 112.3 | 13.06 (13.26) | 18.73 (19.20) | n/a | n/a | n/a |
| M7 2.5D whole | 90.3 | 59.1 | 6.29 (7.16) | 8.61 (10.83) | 1.03 | 221.0 | 2342 |

#### M7 switch variants (1600x720)

| Milestone | llvmpipe Minstr/frame | 4060 Minstr/frame | work ms med (p90) | total ms med (p90) | finish ms med | gl_draws med | gl_bytes med (kB) |
|---|---|---|---|---|---|---|---|
| M7 default (gate run 1) | 61.9 | 28.8 | 2.54 (2.83) | 5.53 (6.07) | 1.47 | 137.0 | 2393 |
| RAZDOR_FLAT_FAST=0 (flat) | 80.5 | 46.9 | 4.36 (4.58) | 7.95 (10.62) | 1.55 | 137.0 | 2393 |
| RAZDOR_STATIC_DRAW=1 (flat) | 58.2 | 30.8 | 1.41 (1.48) | 5.98 (6.17) | 3.04 | 443.0 | 220 |
| RAZDOR_REPLAY_FLOOR=1 (flat) | 47.7 | 13.8 | 0.87 (0.93) | 3.72 (3.94) | 1.42 | 137.0 | 2393 |
| RAZDOR_CLEAR_TARGETS=1 (flat) | 62.0 | 28.1 | 2.44 (2.68) | 5.36 (5.73) | 1.45 | 137.0 | 2393 |
| M7 default (2.5D whole) | 82.7 | 57.5 | 6.97 (7.74) | 9.50 (12.13) | 0.52 | 138.0 | 2394 |
| RAZDOR_DEPTH_FAST=0 (2.5D) | 108.2 | 83.4 | 8.64 (9.75) | 10.78 (14.11) | 0.49 | 138.0 | 2394 |
| RAZDOR_TERRAIN_CHEAP=1 (2.5D) | 79.5 | 55.1 | 6.19 (7.02) | 8.30 (11.16) | 0.48 | 138.0 | 2394 |

#### Battle scene «Другой берег» army 0, 1600x720, M0 and M7

| Milestone | llvmpipe Minstr/frame | 4060 Minstr/frame | work ms med (p90) | total ms med (p90) | finish ms med | gl_draws med | gl_bytes med (kB) |
|---|---|---|---|---|---|---|---|
| M0 battle | 19.4 | 20.9 | 0.69 (0.71) | 3.93 (3.97) | n/a | n/a | n/a |
| M7 battle | 18.8 | 20.4 | 0.65 (0.70) | 3.99 (4.21) | 1.98 | 281.0 | 308 |


Reading the tables in order of the history: the largest step is M0 to M1 (the view-projection once per frame,
flat hills in one session, the flat fog box): flat whole map 111.3 to 82.4 Minstr on llvmpipe, 2.5D whole map
134.2 to 111.5. M1 to M2 (ground rework B1-B3) costs a little on the flat whole map (82.4 to 84.8, +3 %), and
M2 to M3 (E1, whole-pixel ground cache) is flat on the flat map and -4 % on llvmpipe for the 2.5D whole map.
M3 to M4 (FrameCtx) shows no change (flat whole 85.5 to 86.0, 2.5D whole 107.8 to 107.8). M4 to M5 (flat objects as
indices and the still-frame memo) is a -8 % step in the flat whole map (86.0 to 79.3) and -5 % in the flat close-up. M5 to M6 (option f, flat) is the big one for the
flat whole map: 79.3 to 61.0 Minstr on llvmpipe and 45.5 to 27.2 on the 4060, probe work 4.26 to 2.41 ms.
M6 to M7 (option f, 2.5D) takes the 2.5D whole map from 107.4 to 82.7 Minstr and probe work 8.36 to 6.97 ms,
but see the 2.5D close-up anomaly below.

## Switch variants on M7

Each switch is the same M7 build with one environment variable, flat whole map or 2.5D whole map, 1600x720,
the same protocol as the rows above. They show what each switch is worth now.

- `RAZDOR_FLAT_FAST=0` (old flat path): 61.9 to 80.5 Minstr on llvmpipe, probe work 2.54 to 4.36 ms. The option f
  flat path is worth about 23 % of the instructions and 42 % of the probe work on this machine.
- `RAZDOR_DEPTH_FAST=0` (old 2.5D path): 82.7 to 108.2 Minstr, probe work 6.97 to 8.64 ms.
- `RAZDOR_STATIC_DRAW=1` (A′, retained decorations): instructions drop (58.2 vs 61.9 on llvmpipe) and probe work
  drops (1.41 vs 2.54 ms), but the frame median gets worse: total 5.53 to 5.98 ms, `finish_ms` 1.47 to 3.04 ms,
  gl_draws 137 to 443. The `finish` rise is the submission of the 306 extra draw calls (`submit_ms` 1.39 to 2.97 ms;
  `finish - submit` stays at 0.07 ms), not work moved to the GPU. The probe shows the submit path getting more
  expensive, not the GPU getting more work. This confirms the A′ handoff's "no gain";
  on this machine it is a loss in total frame time. The switch stays off.
- `RAZDOR_TERRAIN_CHEAP=1` (cheap ground): 2.5D whole 82.7 to 79.5 Minstr, probe work 6.97 to 6.19 ms.
- `RAZDOR_REPLAY_FLOOR=1`: a lab measure of the replay floor (what a replay costs with nothing else), not a player
  path. Its 47.7 Minstr is not a speed-up for play.
- `RAZDOR_CLEAR_TARGETS=1`: no measurable change (62.0 vs 61.9 Minstr). Lab switch.
- Ground blend setting (Смешивание земли) off: not run in this round (the settings path and RAZDOR_TERRAIN_CHEAP
  were measured instead, see above).
- `RAZDOR_SWAP_INTERVAL`: not run (the brief did not list it for the round).

## Follow-up, quiet machine (same day, CPUs 12,14)

Done after the round, to settle the 2.5D close-up anomaly and to fill the 2.5D walk for M6, M7 and the old 2.5D path.
Same scene, window, probe and instruction protocol as above. Differences: the measured process is pinned to CPUs
12 and 14 (one thread on each of two physical cores; 12/13 and 14/15 are hyper-thread pairs, and the first pass used
12-15), and the runs are interleaved. Binaries: M6 `967e2683`, M7 `00ba26ef` (the code of `70222bed`; the commit
differs only in docs), and M7 with `RAZDOR_DEPTH_FAST=0`. M3 `1933b9eb` was rebuilt for the blip re-check only.
Every run started and ended at load1 below 4 (the per-run values are in the follow-up's progress log); the
machine ran at the default power mode during all runs. No run was retried for load. The first llvmpipe 100-frame
read of M6 round 1 was 9.75 G instructions against 6.80 G in every other read; a re-run gave 6.80 G, so the
first reading was discarded and the re-run is used.

### 2.5D close-up, interleaved (1600x720, frames 101-400, medians per run)

Order: M6, M7, M7 `RAZDOR_DEPTH_FAST=0` (called M7df0), three rounds. `work_ms` is the probe's CPU frame time;
the probe `world` stage is the scene's objects, `finish` includes `flush` and `glFinish`.

| run | llvmpipe Minstr | 4060 Minstr | work ms | total ms | world ms | flush ms | submit ms | finish ms |
|---|---|---|---|---|---|---|---|---|
| r1 M6 | 16.66 | 17.29 | 0.916 | 2.559 | 0.763 | 0.292 | 0.278 | 0.386 |
| r1 M7 | 15.02 | 15.67 | 0.899 | 2.541 | 0.747 | 0.296 | 0.282 | 0.392 |
| r1 M7df0 | 16.79 | 17.41 | 0.930 | 2.595 | 0.777 | 0.292 | 0.283 | 0.397 |
| r2 M6 | 16.66 | 17.30 | 0.922 | 2.582 | 0.768 | 0.294 | 0.277 | 0.385 |
| r2 M7 | 15.03 | 15.58 | 0.801 | 2.382 | 0.679 | 0.263 | 0.270 | 0.380 |
| r2 M7df0 | 16.76 | 17.60 | 0.845 | 2.460 | 0.721 | 0.266 | 0.270 | 0.378 |
| r3 M6 | 16.66 | 17.39 | 0.836 | 2.448 | 0.711 | 0.266 | 0.272 | 0.379 |
| r3 M7 | 15.00 | 15.72 | 0.812 | 2.428 | 0.685 | 0.266 | 0.269 | 0.376 |
| r3 M7df0 | 16.75 | 17.83 | 0.835 | 2.437 | 0.711 | 0.259 | 0.265 | 0.374 |
| **median M6** | 16.66 | 17.30 | **0.916** | **2.559** | 0.763 | 0.292 | 0.277 | 0.385 |
| **median M7** | 15.02 | 15.67 | **0.812** | **2.428** | 0.685 | 0.266 | 0.270 | 0.380 |
| **median M7df0** | 16.76 | 17.60 | **0.845** | **2.460** | 0.721 | 0.266 | 0.270 | 0.378 |

Verdict: M7 is not slower than M6 in the 2.5D close-up. Its work is 0.10 ms lower (about 11 %), its total 0.13 ms
lower, its instructions 10 % lower on llvmpipe and 9 % lower on the 4060, and the lowest `world` stage is M7's. The
stage that moves is `world` (0.76 to 0.69 ms); `flush`, `submit` and `finish` are the same within 0.03 ms. The
`RAZDOR_DEPTH_FAST=0` run sits between M6 and M7 on the frame time (0.85 ms work) and matches M6 on instructions, so
the drop from M6 to M7 comes from the new 2.5D fast path, as the instruction count says. The 2.42 ms of the first
pass is not reproduced; the anomaly is closed. The `perf record` comparison of the first plan was not needed and was
not run.

Note on the frames: close-up frames are kind `idle` (still frames), as in the first pass.

### 2.5D walk (`walk:Другой берег:0,-12`, frames 10-140 for instructions, 144 walking frames for the probe)

Three rounds for each (round 1 is the first run of the pass above, rounds 2 and 3 are new). Medians of the three rounds:

| variant | llvmpipe Minstr | 4060 Minstr | work ms | total ms | world ms | flush ms | submit ms | finish ms | gl_draws |
|---|---|---|---|---|---|---|---|---|---|
| M6 | 14.89 | 16.77 | 0.622 | 2.215 | 0.499 | 0.131 | 0.247 | 0.390 | 73 |
| M7 | 14.09 | 15.96 | 0.648 | 2.237 | 0.525 | 0.135 | 0.252 | 0.395 | 73 |
| M7df0 | 14.78 | 16.65 | 0.629 | 2.218 | 0.508 | 0.132 | 0.248 | 0.393 | 73 |

Per round, M6 work is 0.619, 0.622, 0.623 and M7 is 0.648, 0.648, 0.649 (no overlap). The walk is a small step the
other way from the close-up: M7 is about 4 % more probe work than M6 (+0.03 ms, all of it in `world`), with the same
total frame time (2.24 ms), 5 % fewer instructions on llvmpipe and the same GPU stages. M7df0 is in between on work.
On the 4060 the instruction counts vary by about 2 % between rounds, which is the noise of that backend.

### Flat walk and flat close-up (blip re-check, two rounds each)

| run | llvmpipe Minstr | 4060 Minstr | work ms | total ms |
|---|---|---|---|---|
| flat walk M6 (r1 / r2) | 12.23 / 12.36 | 13.20 / 13.36 | 0.435 / 0.436 | 2.106 / 2.103 |
| flat walk M7 (r1 / r2) | 12.40 / 12.50 | 13.27 / 13.37 | 0.443 / 0.441 | 2.116 / 2.118 |
| flat close-up M3 (r1 / r2) | 13.87 / 13.85 | 14.55 / 14.26 | 0.423 / 0.425 | 2.111 / 2.122 |
| flat close-up M6 (r1 / r2) | 11.62 / 11.63 | 12.27 / 11.96 | 0.350 / 0.349 | 2.069 / 2.067 |
| flat close-up M7 (r1 / r2) | 11.74 / 11.80 | 12.17 / 12.45 | 0.358 / 0.355 | 2.078 / 2.079 |

These are the cells the coordinator asked about. Their first-pass blips are gone on a quiet machine (see anomaly 4).
M3 has no GPU-finish columns (before M6). On the flat walk and close-up, M7 is 1 to 2 % above M6 in work
(0.44 against 0.44 ms; 0.356 against 0.349 ms), which is within the noise of this check.

Files: `/mnt/ai_models/razdor-scratch/timing-followup/results/summary.md` (the interleaved and walk tables, per run),
`summary-recheck.md` (blip re-check) and `finish-check.py` output (finish minus submit in all probe cells of the round).

## Anomalies and what is unresolved

1. **M7 2.5D close-up anomaly: not reproduced (resolved in the follow-up below).** The first pass showed M7 probe
   work 1.42 to 2.42 ms with identical draws. In the quiet interleaved re-run M7 is not slower: work 0.81 ms
   against 0.92 ms for M6 (medians of three rounds), total 2.43 against 2.56 ms. The first pass's M6 and M7 cells were
   run at different loads (2.5 and 3.4 at their start, not quiet), which is the explanation the data support.
   The absolute milliseconds of the first pass are not comparable with the follow-up's.
2. **M0 to M5 have no GPU finish.** `RAZDOR_GPU_FINISH` (and `RAZDOR_REPLAY_FLOOR`, `RAZDOR_CLEAR_TARGETS`) exists
   from M6. For M0 to M5 the `finish` column is n/a, and their probe totals do not include the GPU wait. Compare
   `total_ms` between M5 and M6 with that in mind; the instruction counts are unaffected.
3. **Walk frames are few.** The walk ends at frame 144 (see Method), so the walk rows describe 144 walking frames,
   not 300. The instruction window is 10 to 140 (a deviation from N1 = 100).
4. **Load during the round.** The instruction numbers held (gate: 0.05 % llvmpipe spread). Stage milliseconds of
   the cells whose `load-end` is above 4 in `results.csv` are less reliable; the M7 close-up above is one of them.
   The follow-up re-checked the single-value blips of the first pass (M3 flat close-up 1.23 ms, M6 flat walk
   1.35 ms, about 2 to 3 times their neighbours): with CPUs 12,14 and a quiet machine they are 0.42 ms (M3 flat
   close-up, two rounds) and 0.44 ms (M6 flat walk, two rounds). They were load and hyper-thread contention on
   CPUs 12-15 (two physical cores), not milestone costs. First-pass milliseconds on those cells should not be read.
5. **Battle** (M0 vs M7): instructions 19.4 vs 18.8 M on llvmpipe, probe work 0.69 vs 0.65 ms, total 3.93 vs 3.99.
   No real change in the battle frame.

## Not run, and why

- **Phone.** Nothing in this round ran on a phone. Never measured on a phone: option f (flat and 2.5D), A′
  (`RAZDOR_STATIC_DRAW`), the swap interval, and everything after the A04 lab run. The water EveryOther (995ec3a9)
  is phone-only and was not measured on desktop either.
- **Pan.** No map-pan scene exists (see Method).
- **1024x768** only for M0 and M7, whole map flat and 2.5D (the other milestones were not run at that window).
- **2.5D walk for M3 (llvmpipe only, partial), M4, M5, M6 and M7:** not reached before the round's time limit (the walk's
  machine was above load 30 for over an hour); those cells read "(not run)". The follow-up ran the 2.5D walk for M6, M7
  and M7 `RAZDOR_DEPTH_FAST=0` only (above); M4 and M5 are still not run, and M3's walk is not run. Flat walk, all milestones: run. Battle: M0 and M7 only.
- **Ground blend setting off** and **RAZDOR_SWAP_INTERVAL:** not run.

## Files

- `results.json` and `results.csv` (`/mnt/ai_models/razdor-scratch/timing-round/`): one record per cell and measure,
  with the load at start and end of each run.
- `scripts/` (build, run, matrix, walk, aggregate, tables) and `logs/` in the same folder: the full run log is
  `logs/matrix-progress.log`.
