# OpenMW smoke harness

Off-screen check of the converter's output (`crates/openmw-world`) in OpenMW 0.51. It starts the
game under its own Xvfb display, looks at the start cell, and reports PASS or FAIL. Not part of
`cargo test`. Design: `docs/specs/2026-11-27-openmw-world.md`, section 5.

## Run

```
tools/openmw-smoke/run.sh <converter out dir> [--name <run name>] [--cell cx,cy] [--seconds N]
                          [--shot <png>] [--probe x,y]... [--camera x,y,z,pitch,yaw]
```

- `<converter out dir>` holds `razdor_map.omwaddon` and `textures/`. Game-derived: keep it outside git.
- Everything a run writes goes to `/mnt/ai_models/razdor-scratch/openmw/smoke-run/<name>/`
  (`openmw.cfg`, `settings.cfg`, `openmw.log`, `xvfb.log`, the picture). Override the Template with
  `OPENMW_EXAMPLE`.
- `--probe x,y` teleports the player to a world point (units; a cell is 8192 wide) on the ground,
  one point after another from second 1 (2 s each, up to four). Each reading is a `RAZDOR_PROBE` line.
- `--camera` puts a static camera there (pitch and yaw in radians) for the picture.
- Exit 0 is PASS. It fails when the `RAZDOR_SMOKE` line is missing, the picture is one flat colour
  (top 560 rows), or the log has an `Error` line naming razdor. The Xvfb display and OpenMW are
  stopped on exit.
- The machine's prefix (`ai-resources prefix`) is used for everything. A run takes about 20 s.

`probe_orient.py` writes synthetic maps (`json`, `json-sub`) and patches one raised vertex into a
converter plugin (`patch`). It needs no game files.

## Test-only scripts (`smoke/`)

- `smoke/smoke.omwscripts`: GLOBAL `scripts/razdor_smoke/global.lua` (teleports the player to an
  exterior point with `onGround`), PLAYER `scripts/razdor_smoke/player.lua` (the lines, the
  Russian message, the static camera, `core.quit()`).
- `run.sh` writes `scripts/razdor_smoke/config.lua` into the run's own data folder.

## openmw.cfg notes (0.51)

- Do not list `content=builtin.omwscripts`: OpenMW loads it itself and aborts with "Content file
  specified more than once".
- `replace=data` is what keeps the global config's `vfs-mw` out. Without it the Morrowind data dir is
  added and the run logs "Failed to load meshes/...nif" errors (not razdor files). `replace=config` and `replace=content` are kept as in the
  spec; they were not tested one by one.
- The log is `<run dir>/openmw.log` (the config dir), not under `userdata`.
- In `settings.cfg`, `[Video]` takes `fullscreen = false`. `window mode = Windowed` makes OpenMW
  abort on "Failed to parse number".
- The Template's `settings.cfg` is copied as is (its `[Models]` and `[Game]` lines).
- Known noise, the same with or without the converter's files: about 16 `E` lines (missing
  `menu_*.dds`, and menu scripts that cannot start). None names a razdor file.
