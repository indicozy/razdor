# OpenMW smoke harness

Off-screen check of the converter's output (`crates/openmw-world`) in OpenMW 0.51. It starts the
game under its own Xvfb display, looks at the start cell, and reports PASS or FAIL. Not part of
`cargo test`. Design: `docs/specs/2026-11-27-openmw-world.md`, section 5.

## One command: play.sh (M6)

```
tools/openmw-smoke/play.sh <map.DTm | map.json> [--name N] [--archetype …] [--shot png] [--camera …] [--window]
```

From a fresh folder `/mnt/ai_models/razdor-scratch/openmw/play/<name>/` it exports the `.DTm` with
`razdor-dtm-json` (skipped for a `.json`), converts with `razdor-openmw` (the Game Template from
`OPENMW_EXAMPLE`), writes the standalone config and runs OpenMW off-screen (run.sh). Exit 0 is PASS.
Everything goes into the folder: `map.json`, `out/` (the plugin, textures, meshes, Lua), `smoke/`
(the config, `openmw.log`), `shot.png`, the build logs. The folder is never reused.

- The default camera (`view.py`) stands two tiles north-west of the hero's start tile and looks at the
  map's first building, 1800 units up, pitched 0.3 rad. `--camera` overrides it.
- The picture hides the map's start window: the test-only `m5/m5_auto.omwscripts` answers every window
  (the M5 driver, loaded by play.sh only). `run.sh` alone does not load it.
- `--window` runs nothing. It writes the play config (`<folder>/window/`: no smoke scripts, the game's own
  settings) and prints the command for a real window on the desktop, for the owner to play by hand:
  `openmw --replace=config --config <folder>/window --skip-menu --start 0,0`.
- Build cache: `CARGO_TARGET_DIR` (default `/mnt/ai_models/razdor-scratch/openmw/target`).

## Run

```
tools/openmw-smoke/run.sh <converter out dir> [--name <run name>] [--cell cx,cy] [--seconds N]
                          [--shot <png>] [--probe x,y]... [--camera x,y,z,pitch,yaw] [--expect <file>]
```

- `--expect <file>` (m5/EXPECTED.txt): the log's `RAZDOR_EVENT` ids must equal its `events` line in order,
  the last `RAZDOR_VICTORY` id its `victory` line, and every `contains <text>` line must be in the log.
- `play.sh` passes `--probe` on to `run.sh` (up to four); facing is not passed (`run.sh` has no option for it).

- `<converter out dir>` holds `razdor_map.omwaddon` and `textures/`. Game-derived: keep it outside git.
- Everything a run writes goes to `/mnt/ai_models/razdor-scratch/openmw/smoke-run/<name>/`
  (`openmw.cfg`, `settings.cfg`, `openmw.log`, `xvfb.log`, the picture). Override the Template with
  `OPENMW_EXAMPLE`.
- `--probe x,y` teleports the player to a world point (units; a cell is 8192 wide) on the ground,
  one point after another from second 1 (2 s each, up to four). Each reading is a `RAZDOR_PROBE` line.
- `--camera` puts a static camera there (pitch and yaw in radians) for the picture. A static camera
  does not load the cells around it: OpenMW loads the cells around the player, so a picture of a
  far spot needs `--probe` at the same spot (on a passable tile, or the guard sends the player back).
- `--cyrillic-test` shows the Russian test message (off by default).
- `--play-config` writes the standalone config without the smoke scripts or the wider view distance,
  prints the folder and exits (play.sh `--window` uses it).
- `OPENMW_SMOKE_SCRATCH` replaces the scratch folder (default `…/openmw/smoke-run`).
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
- Smoke runs add `[Camera] viewing distance = 12000`: at the default the fog hides the land a few
  tiles away. Play configs keep the game's own value.
- Known noise, the same with or without the converter's files: about 16 `E` lines (missing
  `menu_*.dds`, and menu scripts that cannot start). None names a razdor file.

## M5 driver (`m5/`, test-only)

- `m5/make_m5_map.py <out.json>` writes the synthetic scenario (16 x 16 grass, two armies, a building zone,
  a point, sixteen events, victory 14, defeat 15). Synthetic input only.
- `m5/scripts/razdor_m5/player.lua` (`m5/m5_test.omwscripts`, loaded with `--content m5_test.omwscripts
  --extra-data m5`) answers the windows in order (Yes, No, then OK; Quit to the end window), walks the player
  onto the scenario's tiles, and kills army 2's bodies (`body.lua`, a CREATURE script). Its expected event
  order is `m5/EXPECTED.txt`: `RAZDOR_EVENT` lines in that order and `RAZDOR_VICTORY 14`.
- `m5/scripts/razdor_m5/auto.lua` (`m5/m5_auto.omwscripts`) answers every window (Yes, OK, Quit) for runs
  with nobody at the screen.
- One command: `m5/run_m5.sh [--name N]` (make the map, convert with `--time-scale 600`, run with the driver
  and `--expect m5/EXPECTED.txt`). PASS is the picture and the smoke line, the event order (`events` line),
  the victory (`victory` line) and each `contains` line of EXPECTED.txt, which must appear in the log.
- Event windows pause the world, so the driver's answers and the smoke script's delays run on real time
  (`--seconds` is real seconds in every run).

## m8: the 4 vs 4 fight (M8)

`m8/run_m8.sh` runs the synthetic fight end to end: `make_m8_map.py` writes a 12 x 12 grass map (no
events) and a units version 2 file (three made-up units: a melee, a shooter, a caster). The converter
runs with `--tile-units 512 --units`, so the armies stand 3 tiles apart (1536 units), both hostile to
the player (faction 4). Each army is a leader plus three troops (melee, shooter, caster). The test
driver `scripts/razdor_m8/player.lua` turns the player off as a target (side 0 is not built yet: the
smoke is army against army) and logs its health each second (`RAZDOR_FIGHT player hp=…`).

`run.sh` runs 120 real seconds. The checks are in `run_m8.sh` (not `--expect`, which needs the event
ids): PASS when the log holds a `kind=melee`, a `kind=shot` and a `kind=strike` hit, and a
`RAZDOR_FIGHT_END` within 120 s of the first health line. A second run stops at 20 s for the
mid-fight picture (`mid.png`); the end picture is `fight.png`. Both go to the scratch folder
`/mnt/ai_models/razdor-scratch/openmw/m8-lua/<name>/`. Heal and bless are not in the synthetic units,
so the check does not cover them.
