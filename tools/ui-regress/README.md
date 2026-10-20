# UI regression check

Renders a fixed set of screens off-screen (under Xvfb, never on the desktop), compares each with
a reference image, and reports what changed. Use it after any change to what is drawn.

- `scenes.txt`: the cases, one per line: `name | RAZDOR_SCENE | WxH | KEY=VAL;KEY=VAL`. The
  scene names are the ones listed at the top of `src/ui/snapshot.rs`. Maps are the install's
  files (`Maps_Rus`, without `.DTm`).
- `ui_regress.py`: the runner (Python 3 with Pillow; run it with `python3 -I`).

## Run it

Build the release binary first (silent, no ALSA):

```
CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/mnt/ai_models/razdor-scratch/ui-regress/target \
  taskset -c 16-19 cargo build --release --no-default-features
python3 -I tools/ui-regress/ui_regress.py run --bin /mnt/ai_models/razdor-scratch/ui-regress/target/release/razdor
```

`run` needs `RAZDOR_DT_DIR` (the install): from the environment, or from the repo's `.env`. It
runs 4 cases at a time, each pinned to cores 16-19, with its own X display, `XDG_RUNTIME_DIR`,
log and save folder, and a 60 s timeout. Options: `--only TEXT` (cases whose name contains the
text), `--out DIR` (default: a timestamped folder under
`/mnt/ai_models/razdor-scratch/ui-regress/out/`), `--goldens DIR`, `--tolerance N` (largest channel
difference that still counts as the same, default 12), `--threshold F` (changed fraction of
pixels that fails a case, default 0.0005).

The output folder holds:

- `shots/<case>.png`: the new renders;
- `<case>.diff.png`: for each changed case, three panels: reference | new | the new render dimmed
  with the changed pixels in magenta;
- `report.md`: a table of every case (status `ok`, `changed`, `new` (no reference yet) or
  `crashed`, the changed percentage, the bounding box of the changes, and the paths);
- `logs/`: the run's output and the game's own log per case.

The exit code is 1 when a case changed or crashed.

## Review and accept

Open the diffs of the changed cases and check each change is the one you meant. Then:

```
python3 -I tools/ui-regress/ui_regress.py accept <outdir>            # every shot of the run
python3 -I tools/ui-regress/ui_regress.py accept --only army <outdir> # the cases named army
```

`accept` copies the shots into the goldens. Run again afterwards: an unchanged build reports 0
changes.

## Where the references live, and why they are not in git

The references are the game's own art as drawn by Razdor, from the player's install: they
show the original game's pictures and maps. They are kept in `/mnt/ai_models/razdor-goldens/`
(override with `RAZDOR_GOLDENS`) and never committed, like every other asset of the game. Only
the scene list and the runner are in the repo. The check therefore runs on the owner's machine
only; CI has no install.

## Determinism

A snapshot must give the same pixels on every run. In a snapshot (`RAZDOR_SNAPSHOT` set):

- the interface's clock (`src/ui/clock.rs`) starts at 0 and moves 1/60 s per frame, so the
  cursor blink, the sways, the fades and the timed windows depend only on the frame count;
- the event generator is seeded with a fixed value (`Game::seed_events`), not from the clock;
- the update check never starts (no network), and the program's version shows as `0.0.0`
  (`update_view::shown_version`), so a version bump changes no picture.

Game rules are unchanged. A case that still differs between two runs of the same build is a
determinism bug: fix it in the code that draws it (route the time or the random draw through
the snapshot clock or seed), not by raising the tolerance.

## Adding a case

Add a line to `scenes.txt` with a unique name, then `run --only <name>` and `accept --only
<name>` once its diff is reviewed. A scene the snapshot does not have yet: add it to
`src/ui/snapshot.rs` first (see `docs/flows/testing-and-tools.md`).
