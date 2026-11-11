# Trailer shots: the format and how to render one

A **shot** is a text file that plays a scripted camera over a Razdor map, with the hero's walks,
days passing, and the game's own events and battles answered automatically. The run renders
every frame off-screen (the UI and the fog hidden, the armies moving) and writes an MP4 with the
game's sound. Nothing here changes the game for players: every switch is dev-only and needs
`razdor --trailer` (or its environment variable).

Developer notes (how it is built, the traps): `docs/flows/trailer.md`.

## Quick start

```sh
# 1. Check the file and see what happens (no pictures; prints the hero, the armies, the events)
razdor --trailer shot.shot --plan
# 2. A cheap preview: 640 px wide, every 10th frame, as a small MP4
razdor --trailer shot.shot --preview --out preview.mp4
# 3. The render: 1920x1080 (or the shot's `size`), every frame, with sound
razdor --trailer shot.shot --out shot.mp4
# Optional: PNG frames (--frames dir), and a hash line per frame (--hashes file, for determinism)
```

Off-screen: run it under Xvfb, with its own runtime folder and log, as
`.claude/skills/offscreen-snapshot/SKILL.md` describes. The GPU for encoding is the RTX 4060
(`CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=1`, set by the run for ffmpeg). `--encoder libx264`
uses the CPU instead. The shot's size must fit the Xvfb screen (`xvfb-run -s "-screen 0 5120x2160x24"`
for 5K).

**Renders and the save folder.** A battle met on the map writes its autosave (and the session's) to the save
folder, as in play. A render sets `RAZDOR_SAVE_DIR` to a scratch folder so that the player's saves stay untouched.

**Frames on the RTX 4060 (no window on the desktop).** Under plain Xvfb the frames come from Mesa's
llvmpipe, which draws artefacts the GPU does not. The run is therefore wrapped in VirtualGL, which
draws the window's GL on the 4060's EGL device and reads the picture back to the Xvfb display:

```sh
# Xvfb on its own display (never :0), with the 5K screen; the 4060 is /dev/dri/card2
# (PCI 0000:05:00.0: check with `ls -l /sys/class/drm/card*/device`, the GPU is the 4060 by name).
XDG_RUNTIME_DIR=<scratch, mode 700> Xvfb :77 -screen 0 5120x2160x24 +extension GLX -nolisten tcp &
DISPLAY=:77 VGL_DISPLAY=/dev/dri/card2 VGL_EGLLIB=/usr/lib64/libEGL.so.1 VGL_GLLIB=/usr/lib64/libGL.so.1 VGL_LOGO=0 \
  CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=1 \
  vglrun razdor --trailer shot.shot --out shot.mp4 --hashes shot.hashes
```

The log's `trailer: renderer:` line names the GPU that drew the frames (`NVIDIA GeForce RTX 4060`; a
Mesa name means the frames came from the software renderer). `VGL_DISPLAY` must be the 4060's device file:
the 5060 Ti is `/dev/dri/card3`. Two runs of one shot on the 4060 give the same `--hashes` file.

Errors come out before anything starts, one per line, with the line and the field:

```
shot.shot:
  line 4: pitch: 95: from 0 to 90 (90 is the top-down view, 0 to 5 the eyes view)
  line 7: walk: the route has no cell (write x,y x,y ...)
```

## The file

One item per line. `#` starts a comment. Every line is optional except `map` and a timed line.

| Line | Meaning | Default |
|---|---|---|
| `map <name>` | An install's map by its file name, without `.DTm`. Spaces are fine: `map Другой берег`. | required |
| `save <file.rzsave>` | Start from a save instead of the map. | — |
| `hero knight\|archmage\|ranger` | The hero's class (a new game). | `knight` |
| `seed <n>` | The event generator's seed. Same seed, same events. | `24301` (0x5EED) |
| `size <w>x<h>` | The picture's size. Width 16 to 5120, height 16 to 2160, even numbers. | `1920x1080` |
| `fps <n>` | Frames per second (1 to 120). Times in the file are seconds of this clock. | `60` |
| `music on\|off` | The map's music (as the game's jukebox plays it). | `on` |
| `battles map\|show` | `map`: a battle is fought on the map and the camera stays on it. `show`: its battle screen is drawn. | `map` |
| `event <id> <choice>` | How an event's question is answered: `0` is the first button (Yes), `1` the second (No). Events not listed take `0`. The id is the scenario's event number (the one the original's and Razdor's logs print). | `0` |
| `duration <s>` | The shot's length. Without it the length is the last timed line's time. | the last time |
| `at <t> <key>=<value> ... [ease=<e>]` | A camera keyframe at `t` seconds. See below. | — |
| `walk <t> <x>,<y> <x>,<y> ...` | At `t`, the hero walks to each cell in turn, at play speed (the game plans each leg). | — |
| `pass <t> <n>h` or `<n>d` | At `t`, time passes as a wait does: armies march, days and events happen. Days are 24 h. | — |
| `reveal <t> <x>,<y> <radius>` | At `t`, the cells within `radius` of the cell are explored. | — |
| `hour <h>` | The game clock's hour at the start (0 to 24, fractions allowed: `6.5`). No time passes: the armies stay and no event fires. | the map's own |
| `clock run\|hold` | `hold` keeps the light at the hour of the start for the whole shot (the game's own time still runs); `run` follows the game's clock. | `run` |
| `ui on\|off` | `on` draws the interface as a player sees it (bottom bar, minimap, the map's widgets); event windows are still answered, not drawn. | `off` |
| `fog on\|off` | `on` keeps the fog of war (black, or the look's mist). | `off` |
| `view flat\|25d` | `flat` is the classic flat map with the player's camera: only `target` and `zoom` may be set (the 2.5D keys are errors). `25d` is the tilted camera. | `25d` |
| `look <key>=<value> ...` | The look settings for this run only, never written to the settings file. `preset` («Качество графики»: `original` or `оригинал`, `low`, `medium`, `high`) is applied first, then the keys: `lighting` (off, on, cycle, fixed), `shadows` (painted, sun), `water` (off, on), `water_rate` (every, other, third), `water_quality` (high, medium, low), `weather` (off, on), `living` (off, on), `mist` (off, flat, clouds), `anim` (on, near, off). | the run's settings |
| `battle <army id>` | The hero's army fights the army with this id (the number `--plan` prints as `A<id>`) at frame 0. With `battles show` its battle screen opens at once; with `battles map` the fight is resolved on the map at the first frame it is free. The ground is the hero's own cell, as in play (the rules have no ground). An unknown id is an error. | none |
| `battle_speed <x>` | The pace of a battle on its screen, as a multiple of the game's animations (0.05 to 8; 1 is the game's pace). The AI plays both sides on the screen at that pace, to the battle's end. Needs `battles show`. | none: the battle ends at once |
| `battle_speed <t> <x>` | A timed pace: it ramps linearly from the pace before to `x` at `t`, and holds before the first and after the last line. A timed line at `0` replaces the header's pace. Same rules. | — |

Cells are map cells, `x,y`, counted from the top-left of the map as the game counts them. Use
`--plan` to check a cell: the planner prints where the hero stands.

### Camera keyframes

`at <t>` sets the camera at time `t`. Between two keyframes every value is interpolated; before the
first keyframe that sets a value it holds, and after the last one it holds.

| Key | Meaning | Range | Default (no key) |
|---|---|---|---|
| `pitch` | The view line below the horizontal, in degrees. `90` is the classic top-down map; 0 to 5 is the eyes view's (`eye=1`). | 0 to 90 | the player's pitch (35 to 75) |
| `fit` | The eye's distance as a multiple of the distance the view fits to the zoom. `1` is the classic scale at any window size. Use this, not `dist`. | above 0 | — (`1` once set) |
| `dist` | The eye's distance in world units, absolute. It depends on the window's height, so the same value frames differently at another size. Wins over `fit`. | above 0 | from `fit` |
| `fov` | The vertical field of view in degrees. | 5 to 120 | 35 |
| `bend` | The far ground rolls up: the arc's full angle. `0` is no bend. | 0 to 89 | 0 |
| `flat` | The ground ahead of the target that stays flat, in world units. | 0 or more | 3 |
| `radius` | The arc's radius in world units. | above 0 | 8 |
| `width` | Half the ground drawn across the view, in world units. Make it large enough for the bottom of the frame. | above 0 | 40 |
| `ahead` | Ground drawn ahead of the target, in world units. | 0 or more | 90 |
| `behind` | Ground drawn behind the target. Needed for the bottom of the frame at low pitch. | 0 or more | 8 |
| `near` | Standing sprites nearer the eye than this fade out (world units; 0 is off). | 0 or more | 8 |
| `yaw` | The view's turn in degrees (0 looks north; 90 looks west). Interpolated the short way round. | any | the player's yaw |
| `target` | What the camera looks at: a cell `x,y`, the hero `hero`, or the hero plus an offset in cells `hero+dx,dy`. An army: `army:<id>` (its id as `--plan` prints it, without the `A`; its displayed place, so the view follows it as it marches) or `army:<id>+dx,dy`. `--plan` and the render warn when the army is gone at a keyframe; the camera then has no target there. | — | the map's centre |
| `ease` | The easing of the segment that **ends** at this keyframe: `linear`, `in`, `out` or `inout`. | — | `inout` |
| `eye` | The eyes view («Глазами героя», the player's first-person camera from the hero's eyes): `1` stands the camera on the target's ground at `height`, looking along `pitch` and `yaw`; `0` is the chase camera. A 0/1 key switches at the middle of its segment. | 0 or 1 | `0` |
| `height` | The eye's height above the ground, in cells (eyes view only). | 0.1 to 5 | `0.7` |
| `zoom` | The flat view's zoom (`view flat` only; other shots use `fit`): 1 is the classic map, 0.4 to 2. Locked for the run, as the zoom setting is when locked. | 0.4 to 2 | `1` |
| `horizon` | The eyes view's far ground: `1` curves it down to the horizon (a sky above it; give `bend=60 radius=60`), `0` rolls it up to the top-down map (`bend=78 radius=8`). A 0/1 key switches at the middle of its segment. | 0 or 1 | `0` |

Notes:

- `pitch 90` is the top-down view (rendered at 89.9; the rendering needs a tilt).
- The eyes view (`eye=1`): the target is the eye's ground (use `target=hero`, which follows the hero's
  displayed place as he walks). `near` is not used: standing sprites within 2.5 cells of the eye fade out, as in
  play. `width`, `ahead` and `behind` are not used either: the ground is the sheet's footprint. A shot with no `yaw`
  keys turns with the hero's walk, as in play; a `yaw` key overrides it. The hero's figure, his route and his ring
  are not drawn from his eyes. `bend` and `radius` shape the far ground; `flat` is the flat stretch ahead of the eye.
- Low views need `bend`, `width`, `ahead` and `behind` together: `bend=78 width=120 ahead=200 behind=60`
  is the look of the treatment's descent. A larger `width` and `behind` fill more of the frame.
- `--plan` prints the fitted distance (`fitted distance` on each keyframe line, for the window
  it runs at). It differs per window size, which is why the shot uses `fit`.
- The view is kept on the map: near a map edge the target is pushed inward, so the hero is not
  always in the middle of the frame.

### Timing

- Frame `i` is at `i / fps` seconds. A shot of `duration` seconds renders `round(duration × fps)`
  frames. Nothing depends on the wall clock: the same file gives the same frames.
- A `walk` runs at play speed: a cell takes the game's step time (about a quarter of a second).
- A `pass` takes the game's wait time: two wait ticks an hour, about 0.3 s an hour. `pass 1s 4d` runs
  for about 29 s. The game stops a wait at each day report; the run takes the wait up again
  until the full span has passed.

## What happens without you

- **Events** open their windows as in play. The run answers each question with the shot's `event`
  choice (default the first button) and closes each message. Nothing is drawn.
- **Battles** are fought to the end by the battle AI on both sides (the game's own quick-battle
  rules, with a fixed step limit, so the result is the same on every run). With `battles map` the
  camera stays on the map and the result is reported as the game reports it. With `battles show` the battle
  screen is drawn and played to its end: at once, unless the shot paces it (`battle_speed`), when the
  AI plays it on the screen frame by frame at that pace (the same AI, the game's own animations). A run
  logs the battle's state once a second (`battle at <t>s: round …`): two sizes of one shot give the same lines.
- **The UI and the fog** are hidden in every run. The pointer is not drawn.
- **Armies** move in every frame, as in play.
- **Sound**: each sound effect the screens ask for, and each music track as the game's jukebox
  would start it, is logged with its frame. After the frames, the sound is mixed (48 kHz stereo)
  and muxed into the MP4. An effect that the game plays on a click is not recorded (no click
  happens in a shot). Battles on the map play no battle sound. `music off` keeps the effects
  and drops the music.

## Outputs

| Option | Output |
|---|---|
| `--out <file.mp4>` | H.264 with NVENC (h264_nvenc, the 4060), `--encoder hevc_nvenc` (the 4060's HEVC, up to 8K), or `--encoder libx264`. The sound is in the same file. |
| `--frames <dir>` | One PNG per frame: `frame_000000.png` ... |
| `--hashes <file>` | One line per frame: `frame sha256`. Two runs of one shot on one machine give the same lines. |
| `--preview` | The picture at 640 px wide, every 10th frame (`--every <n>` changes it): a quick check of the composition. |
| `--plan` | Nothing rendered. Prints the keyframe report and warnings to stdout (and the log). |

Frames come out exactly one per `1 / fps`, whatever the drawing takes: a slow frame costs real
time, not frames.

### 5K encoding: measured (item 7, 2026-11-12)

The 2 s, 120-frame descent at 5120×2160, on the RTX 4060 through VirtualGL:

| Output | Time a frame | Size of the 2 s |
|---|---|---|
| `--encoder hevc_nvenc --out` | 111 ms | 12.6 MB |
| `--encoder libx264 --out` (the fallback, as H.264 is over 4096 px) | 157 ms | 46 MB |
| `--frames` (PNG frames, no encoder) | 148 ms | 833 MB (about 7 MB a frame) |

So for 5K, `--encoder hevc_nvenc` is the faster encoder and the smaller file. The PNG master costs more than the
HEVC encode and takes about 70 times the disk; it is not used. The time is the run's own (its `render:` line).

## Fly mode (for finding the camera)

`RAZDOR_TRAILER_FLY=1 razdor` (dev-only, live, no shot; `RAZDOR_TRAILER_FLY_MAP=<map name>` picks
its map, the first campaign map by default): `W A S D` move the target on the ground
(hold `Shift` to go faster), `Q` and `E` change the distance, a right-drag turns (horizontal) and
tilts (vertical), the wheel changes the field of view. `F8` prints the camera as an `at` line
(`at <t>s pitch=… yaw=… dist=… fov=… target=x,y`): copy it into a shot. (`dist` there is absolute:
the printed line is for a look, use `fit` in the file.)

## Examples

The three shots below are the files behind the rendered examples. Copy one, change its map and
times, and run `--plan` before a render.

### 1. The Act II descent (16 s): from the map to a low view behind the hero

The camera starts on the classic top-down map, the hero walks a road, and by 16 s the camera
is low behind him with the far ground rolling up. (The treatment's 0:14 to 0:30.)

```
# Act II descent (treatment 0:14-0:30): from the classic top-down map the camera drops to a low
# view behind the hero on his road; the far ground rolls up to face the camera. Phase 2 acceptance.
map РК1-Начало пути
hero knight
seed 1204
reveal 0s 30,38 14
walk 0.5s 36,41 33,38 28,36 24,33
at 0s pitch=90 fit=1 target=hero
at 6s pitch=72 fit=0.95 target=hero+0,-2
at 11s pitch=40 fit=0.8 bend=30 width=120 ahead=200 behind=60 target=hero+0,-4
at 16s pitch=12 fit=0.6 bend=78 width=120 ahead=200 behind=60 target=hero+0,-5
duration 16
```

### 2. Flyover of a town (12 s): pitch 55, a full turn round the hero

```
# Flyover (example 2): the town at pitch 55, the camera turning a full circle round the hero.
map РК1-Начало пути
hero knight
seed 7
reveal 0s 36,40 12
at 0s pitch=55 fit=1 yaw=0 target=hero
at 12s pitch=55 fit=1 yaw=360 ease=linear target=hero
duration 12
```

### 3. Time-lapse (30 s): from the top, four days pass and the armies march

```
# Time-lapse (example 3): from top-down, four days pass and the armies march.
map РК1-Начало пути
hero knight
seed 3
at 0s pitch=90 fit=1 target=34,40
at 30s pitch=90 fit=1 target=34,40
pass 1s 4d
duration 30
```

### 4. The eyes view on РК1 (about 20 s): from the hero's eyes, the far ground curves to a horizon

The hero walks from his castle through the village, up the western track and along the top road, and back.
The camera is at his eyes (0.7 of a cell up) and turns with his walk; «Горизонт» is `horizon=1`. The route
is the game's own (it plans each leg over the explored map). Example 5 is the same walk with «Вверх».

```
# A. The first map (РК1-Начало пути) from the hero's eyes, «Горизонт».
map РК1-Начало пути
hero knight
seed 1204
reveal 0s 24,24 26
walk 0.5s 36,41 33,38 28,36 24,33 22,30 18,9 42,8 30,8
at 0s eye=1 height=0.7 horizon=1 pitch=2 fov=55 bend=60 radius=60 flat=2.5 target=hero
duration 18.5
```

### 5. The same walk, «Вверх» (the far ground rolls up to the top-down map)

```
# B. The same route on РК1 from the hero's eyes, «Вверх».
map РК1-Начало пути
hero knight
seed 1204
reveal 0s 24,24 26
walk 0.5s 36,41 33,38 28,36 24,33 22,30 18,9 42,8 30,8
at 0s eye=1 height=0.7 horizon=0 pitch=2 fov=55 bend=78 radius=8 flat=2.5 target=hero
duration 18.5
```

## Limits (known, not bugs)

- The flat view's camera keeps the map on the window: when the window is wider than the map from the target
  (the 5120×2160 frame at zoom 1 on РК1), the map's left edge sits at the window's left edge, so the target is not
  centred. The cell scale is the same at every size (PSNR 34.4 dB between the 4:3 frame and the matching part of
  the 5K frame at the same scale). Open: whether a flat trailer run should centre the target (the edge rule would
  need a run-only switch).

- A timed hour (`hour <t> <h>`, a sweep from day to night in one shot) is not built: the shadows
  step once an hour (`daylight::casts_stepped`), so a sweep of a few seconds would show the steps.

- The target is pushed inside the map near its edges (the view does not show past the map).
- Battles on the map play no battle sound; `battles show` plays the battle screen's sounds.
- Music is the jukebox's own choice for the screens: a track change follows the game's rules,
  and the mix does not fade a track out at its end (the next track starts on its own).
- Every shot starts from the game's new-game state at the map (the hero's class and the
  campaign's own start events), with the settings of the run's save folder.
