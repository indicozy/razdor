# Trailer shots: the developer flow

A dev-only path that renders a scripted camera over a map, frame-exact, with sound. Players
never reach it: every entry point needs `razdor --trailer`, `--plan`, `--preview` or
`RAZDOR_TRAILER_FLY=1`, and with those unset nothing here runs. The format, the examples and
the rendering commands are for agents and the owner: `docs/trailer-shots.md`.

## The call chain

1. **Start** (`src/main.rs`, `conf`): `ui::trailer::configure(&args)` reads the shot (parse
   errors print and exit 2), sets the window's size (`window_size`) and starts the snapshot clock
   at the shot's fps (`ui::clock::set_shot`). The clock is then the frame count (`clock::frame_index`).
2. **Before each frame** (`main.rs`, loop): `ui::trailer::before_frame`. The first call starts
   the map (`start`: a new game from the scenario with the shot's seed, or the save), opens the
   render's sinks (`open_sinks`: an ffmpeg pipe, PNG folder, hash file) and the sound
   (`sound_for`). Each frame it samples the camera (`src/trailer.rs` `sample_values`,
   `sample_target`), stores it for the world (`Runtime::cam`, `target`), issues the timed
   actions (`start_due_actions`: walk, pass, reveal), and steps the walk (`continue_walk`) and
   the pass (`continue_pass`, which takes the wait up again after a day report). A timed hour
   (`hour <t> <h>`, `sample_hour`) sets `daylight::set_hold` each frame, so the light follows it; `start` sets
   `daylight::set_sweep` only in a run that has timed hours, so the light skips its easing and the sun's
   casts follow the hour continuously (`casts_at`). Fly mode (`fly_frame`) replaces the shot's camera with keys and mouse.
3. **The frame**: `app.frame()` runs the game as in play, so events, walks and waits are the
   game's own rules. The camera reaches the world through four hooks in `ui/world_view.rs`:
   `trailer()` (the camera; a shot's camera overrides the phase-1 env camera), `Camera::looking_in`
   (the target replaces the centre), `tilt_view` (always on in a run) and `bar_h` (zero in a run,
   so the map fills the window). `trailer_nofog` and `trailer_clean` hide the fog and the UI.
   `main.rs` skips the pointer's draw in a run.
4. **Events** (`ui/mod.rs`, the dialog block): a run draws no dialog. `trailer::auto_close` answers
   a question with the shot's choice (0 is Yes) and closes a message with OK. The events themselves
   come from the game as in play (`handle_events`).
5. **Battles** (`ui/mod.rs`, where a fight starts): `trailer::battle_on_map` fights it at once with
   `Battle::auto_play_to_end` and resolves it as `BattleView::close_won`/`close_lost` do. With
   `battles show` the battle screen opens and `BattleView::play_to_end_for_trailer` plays it out, at once, unless
   the shot paces it (`battle_speed`): then `BattleView::set_trailer_pace` makes the AI play both sides on
   screen at the pace of the frame (`Shot::battle_pace`), and `after_frame` logs `BattleView::state_line` once a
   second. A `battle <army>` line sets `Game::foe` at the start (`start`): on the screen the battle opens at
   frame 0 (`Game::start_battle`, without an autosave), on the map `battle_on_map` resolves it.
   `layout phone` (`ui::trailer::phone_layout`, read each frame by `display::follow_settings`) sets the
   phone's density (the short side as 393 dp), text size 1.25 and large windows over the player's settings.
   `fps_counter on` turns the game's «Показывать FPS» on for the run; `ui::mod` `draw_fps` then counts real time
   (`trailer::wall_seconds`), because the shot's clock is the frame count. `lang` goes through `ui::language::set`.
6. **After the frame** (`main.rs`, after the cursor): `ui::trailer::after_frame` captures the
   picture (macroquad's screen read, made opaque), writes it to the sinks, and at the last frame
   finishes: closes ffmpeg, mixes the sound and muxes it into the MP4 (`finish`, `mux_sound`).
   A plan writes its keyframe report instead (`report_keys`).

### Sound

`ui/audio.rs` `Audio::frame` hands the cues of the frame to `trailer::record_sound` (and returns:
nothing plays). `record_sound` logs each effect's file (`_Sounds.ini` through the install's sound
table) and runs the game's `Jukebox` with the screen's mood as the audio player does, logging
each music track that starts. `Audio::set_map_track` forwards the map's rotation (`map_track`).
The mix is `src/trailer.rs` `mix_wav` (48 kHz stereo 16-bit; each effect whole, each music track
until the next one). Decoding is `dt::sound::read_sound`.

## Pure parts (no window)

`src/trailer.rs` holds the format (`parse`, the errors per line and field), the keyframe sampling
(`sample_values`, `sample_target`, `lerp_angle` for yaw), the easing (`Ease::apply`), the frame
clock (`frame_count`, `frame_time`), the sound log (`SoundLog`) and the mixer (`mix_wav`). Its unit
tests run with `cargo test --profile fasttest --no-default-features --lib trailer::`.

## The render's outputs and the determinism check

The run's GL renderer is logged at the first frame (`gl_renderer`, the `trailer: renderer:` line). The
render on the RTX 4060 without a window is VirtualGL over Xvfb (`docs/trailer-shots.md`): the code draws
the same frames there, so nothing in the flow depends on it.

Frames are one per `1 / fps` of the frame count. Each frame's hash is the SHA-256 of its RGBA
bytes (`Sinks::write`); two runs of one shot on one machine must give the same hash file. The
known sources of difference are the ones the flow would add: a wall-clock read in the world, an
unseeded random draw, a map-dependent cache that changes with the window's size. The planner
(`--plan`) runs the same frames at a 64 px window, so its report is the render's own.

## Traps (found by the first renders)

- **Render targets leak.** `world_view::bent_target` kept one target per size; the region changes
  with the camera, so memory grew by tens of MB a frame (11 GB by frame 210). It keeps one target
  now. macroquad frees a render target when its last clone is dropped: never keep clones of every
  size.
- **The fitted distance depends on the window's height** (`trailer_camera` reports it; `--plan`
  prints it at the plan's 64 px window, where it is about 6.8). An absolute `dist` therefore frames
  differently at 1080p and 5K. Shots use `fit`, a multiple of it.
- **The view is clamped to the map** (`Camera::looking_in` clamps the origin): near a map edge the
  target is pushed in. The descent's hero sits low in the frame for that reason on РК1. A shot's `edge free`
  skips the clamp (`trailer::edge_free`, read by `looking_in`), so the target stays in the middle.
- **A window that hides the bar** still reserves its height unless `bar_h` is zero in a run, so
  the bottom of the frame was black until `bar_h` changed. Keep the three hooks together.
- **The game's wait stops at each day report**, as for a player: `pass` must be taken up again
  (`continue_pass`), or a 4-day pass stops at day 10.
- **Walks plan over the explored map only.** A walk into unexplored cells is blocked; a shot that
  walks must `reveal` its route first.
- **The MP4 was upside down.** macroquad's screen read is bottom row first: its PNG export flips it for the
  PNG frames, but the ffmpeg pipe took the rows as read (every `--out` and `--preview` MP4 was flipped). `Sinks::write`
  now writes the rows in reverse. The PNG frames and the hashes were right all along.
- **`pitch 90` is the top-down view** but the camera's own range stops at 89.9 (look_at degenerates
  at 90); the shot's parser accepts 90 and the camera clamps it.

## Tests

- `src/trailer.rs` tests: the parser (defaults, errors with line and field, targets, sorting), the
  frame clock, easing, the sampling of values and targets (yaw the short way), the sound log and
  the mixer (placement, gain, clipping, cut clips, resampling).
- `src/ui/trailer.rs` tests: the camera a shot's values give (the player's defaults, pitch 90
  clamped, yaw and bend carried).
- The timed hour: the parser (`hour <t> <h>`, sorted, errors by line and field), `sample_hour` (held, eased,
  midpoint), and `daylight` (`a_timed_sweep_shows_the_held_hour_at_once_and_casts_the_shadows_at_it`).
- The phase-1 tests (`view25`, `tilt_tests`) pass unchanged.

## Recipes

- **A new camera key**: add it to `CAMERA_KEYS` and `check_range` in `src/trailer.rs`, the
  `Values` field and its `slot`, sample it in `sample_values`, and map it in `camera_of`
  (`src/ui/trailer.rs`) into `view25::Trailer` (add the field there, read it in `trailer_camera`).
- **A new timed action**: add a variant to `Action`, its parse in `apply_line`, and its start in
  `start_due_actions`. Keep the action's effect the game's own (call the game's API, do not
  change a rule).
- **A new sound source**: nothing to do if the screens cue it through `audio::cue`. Anything that
  plays straight from the screen must be sent to `record_sound` the same way.
