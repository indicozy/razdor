# Flow: replays (phase 1, rules only)

**Read this first** if you touch what a replay needs: the full-state snapshot, the input stream,
the engine's hooks, the replay header, or a battle's start state. The design and its measurements
are `docs/superpowers/specs/2026-11-11-replays-design.md`; the recorder, the file and the player
are later phases and are not here yet. Everything is in `src/rules/recording/`, and nothing is
attached by default: with no recorder and no playback the rules run exactly as before.

## Snapshot is not a save

A save (`docs/flows/saves.md`) is the serialised `Game`, and a load does what the original does:
it reseeds the main generator, sets the event generator from the clock, and resets the AI's
records (`save::restore`, `ai_init(from_save)`). A loaded game therefore draws differently from the
live one. A replay needs the live state, so `Snapshot` (`recording/snapshot.rs`) holds what the
serialised game leaves out as well:

- the generators (`rng`, `event_rng`), `music_wait`, the patch globals (`battle::patch_globals`);
- the AI's per-army runtime fields (`idle_offset`, `idle_restarts`, `fallen`), not `contact` or the
  test-only `scripted` (a snapshot is taken at rest);
- the step and wait bookkeeping (`stretches`, `wait_ticks`, `reading`, `queued_casts`, `step_base`,
  `step_from`, `battle_troops`, `click_buildings`, `waiting_entry`, `formation_before_battle`,
  `walk_flags`, `held`, `ai_events`, `finished_events`), and the arrival counter `asks`;
- the eight settings the rules read that a save does not hold (`Settings::of`), with `walk_pace`.

The map is referenced by its hash and read from the install (`install_map`); it is never embedded.
`Snapshot::take` refuses a game mid-step (`ai_pending`), with an event's flight held, or with an
arrival still being played: the screen finishes a paused step before it observes the game
(`Game::finish_pause`), as it does before a save. `Snapshot::restore` runs no reseed, no reset and
no draw. Timings (`step_elapsed`, `since_step`, `stretch_minutes`, `snap_due`, `snapped`,
`hero_glide`) are kept in the file and left out of the hash.

`state_hash` (`recording/snapshot.rs`) is FNV-1a over the JSON body without the timings and
without `asks` (a frame's budget changes how many arrivals it asks, not the rules). Callers hash
at a day's end with the step finished and any stop's snap done (`finish_pause`, then
`armies_snap`): the snap's idle draws come at the next moving tick, so a hash taken before them
compares states at different points.

## The input stream

`recording/stream.rs`: `Entry { at: Stamp, input: Input }`. A `Stamp` is the stretch of time
(`Game::stretches`: a walk step or a wait tick) and the arrivals asked for inside it (`asks`), so
a draw made between two frames lands at the same place when played again. `Input` is a
`PlayerAction` (a compact enum: clicks, waits, answers, battle presses, shop and army actions) or an
`EngineInput`:

- `GameStart { load, seed, patch }`: a new game or a load starts, with the event generator's seed
  and the patch globals it starts with (the globals outlive a game, so they are recorded too);
- `Music { pick }`: a map music change, drawn at its stamp (`Game::music_rotate`);
- `Max { battle, decision, code }`: the enemy's «Максимум» move (`MaxCode`);
- `Patch(PatchRecord)`: the patch globals at a battle's start.

Encoding: a version byte, the count, then per entry a zigzag stretch delta, the ask count and the
tagged payload, all LEB128 varints. About 6 bytes an action with the stamp (the spike's 3.6 bytes
had no stamp). `decode` refuses what `encode` could not write.

## Hooks

`recording/hook.rs` is per thread (the `local!` table, like the patch globals):

- `install_recorder` / `take_recorder` attach a `Recorder` (a trait: `record(Entry)`; a `Vec<Entry>`
  is one). `record_at` reports an engine input at a stamp, nothing without a recorder.
- `install_playback(&entries)` gives the records back: `game_start` takes the next start record,
  `playback_max` the recorded move of `(battle, decision)`, `playback_patch` a battle's globals,
  `playback_music_due(stamp)` says a music change falls at that stamp (the driver then calls
  `Game::music_rotate`; the draw is the rules' own, the record gives only its place).
- `Game::replay_stop` (set by a playback driver) makes the frame stop at a given ask, so a music
  change that fell mid-step lands there. `Game::tick_within` counts the asks and wraps `tick_inner`.

Where they sit: `Game::with_world` and `save::restore` call `hook::game_start` (`game.rs`,
`save.rs`); `Game::music_rotate`, `Game::tick_within`, `Game::start_battle`; the battle's
`Battle::apply_max` reports a decision the battle really plays, and `MaxThinker::poll` returns a
recorded one instead of searching (`battle/max_ai/ponder.rs`). `Battle::ordinal` (the game's battle
number) and `Battle::decisions` name the decisions. The UI's `battle_view` plays the thinker's move
through `apply_max`, so a recorder sees the moves the screen played.

## Header and battle start

- `ReplayHeader` (`recording/header.rs`): the format, the Razdor version, `rules_fingerprint` (the
  version and `ExperimentalRules`), `install_hash` (the install's map names and sizes), the map's
  file and hash, the platform, the `Settings`, the experimental rules.
- `BattleStart` (`recording/battle_start.rs`): what `Game::battle_start` reads before the fight
  (attacker, both sides' units with their squad index, the bench cells, the experience correction,
  the flags, both building defences, the patch globals). `BattleStart::rebuild` makes the same calls
  in the same order as `start_battle`.

## Holes (the spec's list) and where each is closed

1. Loaded game is not live: `Snapshot` (above); a load stays as it was.
2. Event seed: `hook::game_start` (`GameStart`), at new game and load.
3. Music timing: `Game::music_rotate` records; `Game::replay_stop` and `playback_music_due` place it.
4. «Максимум» clock: `Battle::apply_max` records, `MaxThinker::poll` plays the record.
5. Eight settings: `Settings::of`, in the snapshot and the header.
6. Patch globals: recorded at a battle start and a game start, in the snapshot, set by `rebuild`.
7. Transcendental floats: not closed (see Open).
8. Battle start: `BattleStart` includes the enemy's building defence of an army in a hostile
   building (the garrison defence of the building it stands in), which the spike's rebuild left out (armies 1 and 6 of the biggest map).

## Open (found while building phase 1)

- **Split frames are not transparent.** A budgeted frame (`tick_shown_within`) that pauses a step
  gives different states from unbudgeted ticks: the same session at the same frame times differs at
  stretch 16 (an AI army one cell behind), from day 1. Split runs with different budgets agree with
  each other, so the screen's result depends on whether a step was split. Replays cannot assume
  split-independence until this is fixed. `diag_first_split_difference` (ignored) shows it.
  `settle_all` at a pause end is not the cause (tried and reverted).
- **Frame times longer than a step** (a step is 0.15 s, so below about 6.7 fps) change the states
  by day 2 even unsplit (dt 0.2 s against dt 1/60 s). Not tested or fixed.
- **Floats (hole 7)**: `rng::plant_hash` takes `trunc` of f64 `sin`/`cos`, which can differ in the
  last bit between platforms; that moves a plant's seed only when the value sits at an integer
  boundary. Not measured on another platform.

## Traps

- The hash is a rules hash: it changes when the rng changes (`the_state_hash_follows_the_rules...`).
- `asks` is in the snapshot and not in the hash; `held` is in the hash (events waiting for a window).
- A test's game must be seeded through a playback (`start` in `recording/tests.rs`): the opening
  events of `from_scenario` draw from the event generator before any `seed_events` call.
- The patch globals persist from one game to the next on a thread (nothing resets them between
  battles, as in the original). A replayed game starts from the recorded globals (its `GameStart`
  sets them); a test that plays two games on one thread relies on the same.
