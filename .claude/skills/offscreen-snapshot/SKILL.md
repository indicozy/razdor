---
name: offscreen-snapshot
description: Use to see or measure a Razdor screen without opening a window on the desktop - "take a screenshot", "render the window", "check how it looks", "snapshot", "profile", "why is it slow", "lag when a window opens", checking a UI fix at several resolutions. Covers Xvfb, the RAZDOR_SCENE scenes, RAZDOR_SNAPSHOT, RAZDOR_SCENE_OPEN, RAZDOR_PROFILE and perf.
---

# Off-screen snapshots and profiling

Running Razdor at all, even hidden, needs the owner's go-ahead (see `CLAUDE.local.md`).
Nothing appears on the desktop with this recipe.

## Build

`cargo build --release --no-default-features` (with the core pinning prefix from `CLAUDE.local.md`, if present; silent: no music on the speakers), into a
scratch `CARGO_TARGET_DIR` outside the repo if the main target is in use. For profiling add
`CARGO_PROFILE_RELEASE_DEBUG=true`.

## Run

```
XDG_RUNTIME_DIR=<scratch dir, mode 700> \
RAZDOR_LOG=<scratch>/razdor.log RAZDOR_SAVE_DIR=<scratch>/saves \
RAZDOR_DT_DIR=<install> RAZDOR_SIZE=1280x800 \
RAZDOR_SCENE=<scene> RAZDOR_SNAPSHOT=<scratch>/shot.png \
xvfb-run -a -s "-screen 0 1280x800x24" <target>/release/razdor
```

- Its own `XDG_RUNTIME_DIR`: Razdor allows one copy (`razdor.lock` there); while the owner's
  game is open a snapshot run would otherwise quit silently.
- Its own `RAZDOR_LOG` and `RAZDOR_SAVE_DIR`: the owner's log, saves and settings stay untouched.
- `RAZDOR_SNAPSHOT_FRAMES` (default 20): frames before the shot.
- `RAZDOR_UI_SCALE=<n>` (e.g. `1.25`): that interface scale instead of Auto (snapshots
  ignore the saved settings' scale).
- `RAZDOR_TEXT_SIZE=<1|1.25|1.5>`, `RAZDOR_LARGE_WINDOWS=1`, `RAZDOR_DP=<px per dp>` (a phone's
  density, e.g. `2.625`): the text size, the large pop-up windows and the phone layout.
- Check UI changes at 800x600, 1024x768 and 1280x800 at least.

## Scenes

Listed at the top of `src/ui/snapshot.rs` (the source of truth). Common ones: `title`,
`load[:manual|auto[:<row>]]`, `options[:advanced]`, `map:<map>[:x,y]`,
`building:<map>:<n>[:barracks|garrison|market|sanctuary]`, `army:<map>[:<n>…]`,
`journal:<map>`, `spells:<map>`, `spells-pages:<map>`, `menu:<map>`, `settings:<map>[:advanced]`,
`battle:<map>:<n>`, `custom-battle[:watch]`, `replay:<step>` with `RAZDOR_REPLAY`.
`<map>` is the install's map file name without `.DTm`. Helpers: `RAZDOR_MOUSE=x,y`,
`RAZDOR_SCENE_QUIET=1`, `RAZDOR_SCENE_SHOW`, `RAZDOR_SCENE_CONSOLE`, `RAZDOR_SCENE_FILTER`,
`RAZDOR_SCENE_SPELLS`, `RAZDOR_SCENE_POTION`, `RAZDOR_SCENE_DEBUG`, `RAZDOR_SCENE_SELECT=<n>` (squad member n selected in a `building:` window: its card), `RAZDOR_SCENE_LANGUAGE=1` (the language list open). A later campaign map started
as a new game may end at once by its own defeat event; that is not a bug.

No scene for what you need (a hover, a tab, a selection)? Add one to `snapshot.rs` rather than
skipping the visual check.

## UI regression check

For "did this UI change alter any other screen?": `tools/ui-regress/` renders the cases of
`tools/ui-regress/scenes.txt` (one per scene and size, the main windows at 800x600, 1280x800 and
1920x1080) and compares each shot with its reference in `/mnt/ai_models/razdor-goldens/`.

- Build the release binary (`CARGO_TARGET_DIR=/mnt/ai_models/razdor-scratch/ui-regress/target`,
  `--no-default-features`, core pinning as above), then
  `python3 -I tools/ui-regress/ui_regress.py run --bin <that binary>`. Needs `RAZDOR_DT_DIR`
  (environment or the repo's `.env`). About 3 minutes for the whole set.
- Look at `report.md` in the output folder (under `/mnt/ai_models/razdor-scratch/ui-regress/out/`)
  and at each `<case>.diff.png` (reference | new | changes in magenta). Accept only the changes
  that were meant: `accept --only <text> <outdir>`, or the whole run with `accept <outdir>`.
- A new case has no reference: it is listed as `new` and is not a failure until accepted.
- Never commit the references or the shots: they show the original game's art.
- Snapshots are deterministic (`src/ui/clock.rs`: the interface clock advances 1/60 s per frame
  when `RAZDOR_SNAPSHOT` is set; the event generator has a fixed seed). A case that differs
  between two runs of one build is a determinism bug to fix in the code, not a tolerance to raise.

## Profiling

- `RAZDOR_PROFILE=1`: frames over 100 ms are logged with their screen, and slow picture decodes
  as "slow load". `RAZDOR_PROFILE=<ms>` sets another threshold. Every 5 s the frame probe logs
  a `frame probe` line per kind (walk-step, walk, idle, other): percentiles of total, work,
  present and each stage, in ms. For a walk scene on a fresh map add `RAZDOR_SCENE_QUIET=1`,
  or the scenario's messages hold the hero. With `RAZDOR_SCENE_SAVE` leave it off: the day
  change's window is part of what the walk measures.
- `RAZDOR_PROFILE_FRAMES=<csv path>` (desktop): one CSV row per frame
  (`frame,kind,work_ms,total_ms,tick,terrain,world,fog,minimap,rebuilds`), buffered and written
  every 5 s and at exit. The `frame` column counts the probed frames from 1.
- `RAZDOR_SCENE_SAVE=<file.rzsave>`: a map scene (`walk:<map>:dx,dy`, `map:…`) takes its game
  from that save, as the Load screen loads it, instead of a fresh scenario; the scene's map
  name is then unused. A save that does not load (another install hash, a damaged file) is
  logged as `RAZDOR_SCENE=…: cannot load the save <path>: <reason>`, and the scene stays on
  the title screen.
- `RAZDOR_SCENE_OPEN=<frame>:<window>[,…]` (`load`, `army`, `spells`, `journal`, `menu`,
  `building:<n>`, `minimap`, `map`): opens a window at that frame and logs the frames after it.
  Its first open of the minimap or a building is what the game's start prepares (the decode
  queue and the textures): time it with `RAZDOR_PROFILE=1` and `RAZDOR_PROFILE_FRAMES`, without
  `RAZDOR_SNAPSHOT` (snapshot runs queue nothing).
- Many saves: point `RAZDOR_SAVE_DIR` at a scratch folder of copied saves (0, 85, 500).
- CPU: `perf record -F 999 -g` (or `--call-graph dwarf`) on the run, then
  `perf report --no-children`; I/O: `strace -c -f`.
- Timings under Xvfb use Mesa's software renderer: compare before/after on the same setup, and
  don't read render times as the player's GPU.
