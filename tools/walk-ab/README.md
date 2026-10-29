# Walk A/B (desktop)

`walk_ab.py` runs two Razdor binaries through the same walk, off-screen, and compares their frame
times. It is the desktop side of the map-walk FPS work: a phone's walk frames are estimated from
the desktop run by a factor `k` (see below). Stdlib only.

## What a run does

- One Xvfb for the session, on its own display (`--display`, default 99), pinned to an E-core
  (`--xvfb-cpu`, default 16). Each Razdor run is pinned to another (`--razdor-cpu`, default 19)
  with `LP_NUM_THREADS=1`.
- Each run: `RAZDOR_SIZE` (default 1440x3168), `RAZDOR_SCENE=walk:<map>:dx,dy` with
  `RAZDOR_SCENE_SAVE=<save>` (the walk starts from the saved hero's tile), `RAZDOR_PROFILE=1`,
  `RAZDOR_PROFILE_FRAMES=<run>/frames.csv` (one row per frame), `RAZDOR_QUIT_AFTER=<frames>`,
  its own `RAZDOR_LOG`, `RAZDOR_SAVE_DIR` and `XDG_RUNTIME_DIR`, `RAZDOR_NO_AUDIO=1`. With
  `--quiet`, `RAZDOR_SCENE_QUIET=1` too. No other `RAZDOR_*` variable of the parent shell is
  passed on.
- `--quiet` is needed for a walk past the noon report: without it the report window stays
  open, the hero stops, and every later frame is `other` (not a walk frame). The midnight
  change raises no window, so it is measured with or without the flag.
- Runs alternate A, B, A, B for `--repeats` repeats (default 3).
- A run fails when the process exits non-zero, when the scene does not stage (its log line
  `RAZDOR_SCENE=…: <reason>`, e.g. a save from another install), or when no frames are written.

## Output

In `--out`: `runs/<A|B>-<n>/` (`frames.csv`, `razdor.log`, `razdor-play.log`, `stderr.txt`,
`xdg/`, `saves/`, and `perf.data` with `--perf`), `xvfb.log`, and `report.md` (also printed):

- The runs: the step count of each run, a fingerprint of the frames where the hero stepped
  (equal fingerprints mean the same walk), the first and last step frame, and the in-game dates
  the play log names. Runs of A against A should match.
- Per kind (`walk-step`, `walk`, `idle`): work p50/p90/p99/max, and each stage's p50/p99/max,
  for A and B pooled over the repeats. `B/A` is the ratio. `phone~ A` and `phone~ B` are the
  desktop times times `k`. The stages are the probe's: `tick` and its parts (`route`, `look`,
  `armies`, `script`, `day`, `snap`), `tick-rest` (the tick less its parts), the map (`terrain`,
  `world`, `fog`, `minimap`), `autosave` (outside the tick) and `rest` (the work less the tick, the
  map, the fog, the minimap and the autosave). Inside `armies`: `ai_move` (the AI's moves and plans)
  and, inside it, `ai_plan` (the replans); `armies-other` is armies less `ai_move` (the non-managed
  armies and the midnights). `ai_plan` is "of which", never added to a part. `tick-rest`,
  `armies-other` and `rest` are worked out here from the CSV, clamped at zero. A CSV without these
  columns (an older probe build) is refused.
- Where the walk-step work goes: each part's total over all walk-step frames of all runs, its share
  of the work, and the sum of the parts (it equals the work when nothing is clamped).
- The worst five walk-step frames, with their stage split and `phone~` (work times `k`).
- With `--detail 13,69` (frame numbers), the stage split of those frames in every run.
- `k = 3.0 / (A's walk-kind work p50)`: the phone's walk work p50 (about 3 ms) over the
  desktop's on the same walk. `k` is printed in the header.
- The worst 5 frames by work over all runs, with their kind and stage split.

## Running it

`RAZDOR_DT_DIR` must name the install (the save needs it). `--walk dx,dy` is the offset from the
saved hero's tile; `--frames` is the length of each run; `--map` is only a placeholder (the
scene's map name is not used with a save) and defaults to `-`.

```
RAZDOR_DT_DIR=<install> python3 -I tools/walk-ab/walk_ab.py \
  --a <razdor-A> --b <razdor-B> --save <file.rzsave> --walk -40,24 --frames 90 \
  --quiet --repeats 3 --out <scratch>/walk-ab
```

Build the binaries with `cargo build --release` (`CARGO_PROFILE_RELEASE_DEBUG=true` for symbols
to profile), pinned to the E-cores (`CARGO_BUILD_JOBS=4 taskset -c 16-19 cargo build --release`).
`--perf` wraps the first run in `perf record -F 999 -g --call-graph dwarf`, pinned as the rest.
Keep `--out` and the perf data in scratch (`/mnt/ai_models/razdor-scratch/`), not in the repo.

Use a save copy in scratch, never the player's. The script refuses a missing Xvfb, `perf` with
`--perf`, or a display already in use (`/tmp/.X<n>-lock`; pass `--display`).
