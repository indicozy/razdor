# Flow: replays (phase 1, rules only)

**Read this first** if you touch what a replay needs: the full-state snapshot, the input stream,
the engine's and the player's records, the file, the replay header, a battle's start state, or
the playback. The design and its measurements are `docs/superpowers/specs/2026-11-11-replays-design.md`.
Everything is in `src/rules/recording/`, and nothing is attached by default: with no recorder and
no playback the rules run exactly as before. The game records when its session is attached
(`ui::recording::follow`, the setting «Записывать игру», on by default).

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

## Frame schedules (holes 9 and 10: closed)

Every frame schedule gives the states of the unbudgeted reference, day by day
(`every_frame_schedule_gives_the_same_states_day_by_day`: 2 to 240 fps, whole frames and split
budgets, РК1 and ДС2, 30 days). The causes found, each now fixed in the rules:

- A paused step was observed by the decisions made at rest. `waiting()` and `moving()` read idle
  while the last wait tick or the last walk's arrival is still paused, so a caller that began a
  wait then began a second one. The action entry points now finish a paused step first (see
  `map-step.md`, "A paused step").
- An event hand-over stamped every event with the stretch count after the tick, so a finished
  stretch's events were held while a later stretch was paused (`Game::stamp_last`, `Game::marks`).
- A message left by a finished step did not stop the next tick (`Game::tick_inner`, the early
  return for finished events that need reading).
- The hash read an undelivered event twice over (in `finished_events` and in `held`); the snapshot
  keeps the two lists as they are, and the hash reads them as one queue.
- The snapshot did not carry what a step's cost and the hero's walk display read: the path's step
  weights (`AiMind::path_weights`) and each army's `Walk` (`ArmyState`).

Holes 9 and 10 were one bug seen twice: the decisions were taken from a state a frame had left
half-played. The split run is the same as the unbudgeted one (the test), and so are the difftests
and the smoke test. Frame times longer than a step (below about 6.7 fps) are now as the reference.

## Open (found while building phase 1)

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

## Player actions (`recording/actions.rs`)

Each entry point the screen calls that changes the game records itself at the rules boundary:
its body opens `actions::scope(self, || PlayerAction::X{..})` (a `Game` method) or
`actions::battle_scope(|| BattleAct::X)` (a `Battle` method). The first scope on the stack records
(after a paused step is finished, so the stamp is the place the action came at); nested calls do
not; nothing records inside an engine scope (`hook::engine_scope`: a tick, a script run, a restore,
the battle AI's turns). `apply_game` and `apply_battle` call the same methods on playback.

Recorded (the screen's calls): walks (`set_destination`), waits and the endless wait, the look,
answers and window closes (`answer_question`, `event_window_closed`, `release_unshown_window`,
`force_event`, `take_hall_entry`), `reenter_building`, `enter_waiting_building`, spells read or cast
on the map (`begin_cast`, `cast`), `army_window_opened`, items (`press_pack`, `press_worn`,
`give_held`, `drop_held`, `drink`, `equip`, `unequip`), towns (`buy`, `sell`, `hire`, `heal`,
`resurrect`, `learn_spell`, garrison moves, `dismiss`, offers, tribute, `rent_ship`), the squad
(`set_unit_battle_disabled`, `set_formation`, `set_hero_name`, `move_unit`, `promote`), the battle
(`start_battle`, `resolve_battle`, and the battle's `move_card`, `move_active`, `pass`, `act`,
`own_cell`, `auto_step`, `hero_auto_step`, `hero_auto_play_to_end`, `max_play_to_end`, `force_end`),
and the rule settings (`set_rule_settings`: a change, recorded once; `ui::main_menu::follow_settings`
calls it every frame).

Guarded by `actions::tests::every_entry_point_that_changes_the_game_records_or_is_listed`: every
`pub fn ... (&mut self` of `impl Game` and `impl Battle` in the entry-point files either opens a
scope or is in `NOT_PLAYER_GAME` / `NOT_PLAYER_BATTLE` (the engine, the setup, the tools). A new
method fails the test until it is one or the other. Internal calls from the engine that are not in
a scope are kept out by the engine scopes; the scripted sessions (`tests.rs`) are what check that.

## The replay file (`recording/file.rs`, `recording/session.rs`)

`.rzreplay`: the magic `RZRP`, the format version (1), then blocks `[tag u8][length u32 LE][payload]`:
the header (JSON `ReplayHeader`); a keyframe (tag 2: the state's hash and its packed snapshot,
every 10 game days, a check in playback); a segment's start (tag 3: a keyframe that begins a new
segment, at a new game or a load); the entries of the day (tag 4, `stream::encode`); a battle's start
(tag 5, `BattleStart` JSON, for a cut-out). A segment is its start and the entries after it. The
frame serialises a keyframe (`snapshot::capture`: at rest, the JSON and the hash, 0.3 to 1 ms);
the writer thread packs it (bzip2, 4 to 46 ms) and writes every block in order. The entries of a
game day are written when the day changes (`Session::pump`, once a frame).

The session (`Session`) is attached to the game (`Game::replay`) and to the thread's hook (its
recorder: what the rules report goes to its pending entries). `Session::save_point` writes the day's
entries and syncs the file: the save's `ReplayRef { file, pos }` is the file and its length then.
`Session::resume` cuts the file at `pos` and starts a segment from the loaded game's state.

## Saves and the replay (`rules/save.rs`)

`SaveMeta::replay` (optional, JSON `replay`) is the save's place: `save::meta_of` takes it from the
game's session. `save::restore` sets `Game::replay_resume` from it; the next frame's
`ui::recording::follow` resumes that file there. Deleting a save (`save::delete`) deletes the file
when no other manual or auto save of the game points into it (`replay_referenced`). A replay belongs
to the game's saves, so it is kept while one of them exists. A save made with recording off has no
place, and loading it starts a new file.

## Playback (`recording/play.rs`)

`Player::advance_to(stamp)` takes the game to a recorded place: frames of the playback's own time
until the game stands there, at rest, with the held events delivered (the screen decides only then).
Two stops keep the place exact: `Game::replay_stop_before` (the time does not begin the stretch after
the recorded one) and `Game::replay_stop` (the recorded ask inside a stretch). `Player::apply` plays
one entry: a player action at its place by its own method; a battle's action on the battle under way,
then the enemy's turns as the screen plays them (`run_enemy`: «Максимум»'s recorded moves, else the
battle AI); a check compares the state hash (finish, snap, `state_hash`), and a difference is a drift
with its place. Music changes are drawn at their recorded places (`draw_music`), and the start of a
battle (`start_battle`) takes its recorded patch globals (the hook).

A file plays segment by segment: each starts from its own snapshot (`Snapshot::from_bytes(..).restore`)
and plays its entries; a load's segment starts from the state the load gave.

## What the tests pin

- `recording_leaves_the_states_and_the_game_alone`: a session with recording on gives the states of
  the same session with it off, and the same game bytes (the save's meta differs only by its place).
- `a_recorded_game_plays_back_to_the_same_states_each_day`: a 3-day session with «Максимум» and a
  battle, recorded from a snapshot, plays back to the same hash at each day's end and at the end.
- `a_load_truncates_the_replay_and_the_file_plays_back_across_it`: a save, play on, a load of the
  save (the file is cut at the save's place and a second segment begins); the first segment plays to
  the saved state, the second to the live game's final state.
- `deleting_the_last_save_of_a_replay_deletes_it`: the file stays while another save points into it.
- `a_thirty_day_replay_is_small_and_its_keyframes_do_not_block_the_frame`: sizes and keyframe times.

## Measured (30 game days, walks, waits, a battle with «Максимум»)

- РК1: 32 KB (238 entries, one segment); ДС2: 111 KB (241 entries). The keyframes are most of it
  (each carries a packed snapshot: 5.7 KB on РК1, 14 KB on ДС2 at the test's end); the entries
  themselves are a few KB.
- A keyframe on the frame: 0.26 ms (РК1), 1.03 ms (ДС2); the packing on the writer thread: 4.4 and
  46 ms.

## Open

- The replay watching (phase 3) and the battle view's controls: not built.
- A save's place and the file are written when the day changes and at a save: the last day's
  entries are lost if the game is closed without a save and the day has not ended (the session has
  no exit hook yet).
- A game restored from its save with recording off keeps no session; turning the setting on starts
  a new file from the game's state.
- The `ui::recording` setting row and the frame hook are UI changes: the owner's approval of the
  «Прочее» screenshots is needed before a push to main.
