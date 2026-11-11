# Flow: replays (the rules, the player and the watching screen)

**Read this first** if you touch what a replay needs: the full-state snapshot, the input stream,
the engine's and the player's records, the file, the replay header, a battle's start state, the
playback, or the watching screen (`ui/watch.rs`, the last section). The design and its measurements
are `docs/superpowers/specs/2026-11-11-replays-design.md`. The rules are in `src/rules/recording/`,
and nothing is attached by default: with no recorder and no playback the rules run exactly as
before. The game records when its session is attached (`ui::recording::follow`, the setting
«Записывать игру», on by default).

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
every 10 game days, a check in playback; a day keyframe waits for 30 s of wall time since the last one, a battle's is not held back); a segment's start (tag 3: a keyframe that begins a new
segment, at a new game or a load); the entries of the day (tag 4, `stream::encode`); a battle's start
(tag 5, `BattleStart` JSON, for a cut-out). A segment is its start and the entries after it. The
frame serialises a keyframe (`snapshot::capture`: at rest, the JSON and the hash, 0.3 to 1 ms);
the writer thread packs it (bzip2 at level 2, `save::pack_with`; the `.rzsave` keeps its own level) and writes every block in order. The entries of a
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
with its place. A music change is drawn where its record is (`Player::apply`'s `Music` arm, see
"The player" below), and the start of a battle (`start_battle`) takes its recorded patch globals (the hook).

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

## The player (`recording/watch.rs`)

`ReplayPlayer` opens a file (`open`, or `new` for a file already read): it reads the segments and
builds the timeline's `marks` (one pass that restores each checkpoint: the days, the battles'
starts with the two sides' unit names from the battle block, and the towns taken, the hero's fall
and the victory from the keyframes' states, so those last three are at keyframe precision).

- `advance()` plays the next entry: a segment starts from its own snapshot (its start hash is
  checked), the entries go through `Player::apply`, a segment's end starts the next one (a load).
  A checkpoint that does not match stops the playback: `Drift { day, version, at, reason }` with the
  recording's Razdor version (the message «Запись сделана в версии {v} и дальше расходится») and
  `last_good()`, the place the game stands at after the last matching entry.
- `seek_day(day)`: the latest checkpoint (segment start or keyframe) at or before `day`, restored,
  then played until the game's day is `day` or later; `seek_place(place)` goes to a place the same way.
  Days are the game's clock days (`first_day()` is the file's first day).
- The thread's playback is installed for each public call and taken away at its end
  (`arm`, `take_playback`), so several players on one thread do not meet, and a live game is not
  left with records.
- Nothing is drawn here: the watching screen is `ui/watch.rs` (below).
- A budgeted pump (`pump(budget, watch_battles)`): plays up to `budget` game seconds (any split gives
  the same states), and stops at a battle's start when the caller watches battles: `Pump::Battle`, the
  battle made and begun (`take_battle`). `take_battle_actions` hands the battle's recorded actions to
  its view in order (the «Максимум» moves and patch globals are looked up by key, not played), and
  `return_battle(battle, rest)` takes the view's battle back: the actions it did not play are played
  from its state, then the battle is resolved (a battle that is not over at its resolve is a drift).
  The engine's records in a battle are skipped by position (`battle_entry`).
- Seeks are states: `begin_seek_day` / `begin_seek_place` restore the checkpoint before the target
  (a seek its checkpoint already reaches is none), and the pumps play on until the target. The
  marks of battles are their `StartBattle` entries, so a seek to one stops at its choice.
- The events a step raises are kept (`take_events`): `apply_game_events` gives the window and
  message events of the recorded actions, the frames' events are collected by `tick_toward`.
- The last segment's end keeps the game (`game()` after the end is the final state).

`Player` (`play.rs`) places each recorded entry: a player action or a check at the **end of its
stretch** (the paused step is finished there, as the action does; the arrivals taken inside the
stretch are not compared), a music change at its **exact arrival** (`advance_exact`: the time stops at
that ask and the change is drawn, so it lands in stream order), and a battle's write-back into the
armies after each enemy step and before each check (the screen does it each frame). An arrival counts
when it is taken, not when it is asked for: a frame's refused ask is not a place, so the count is the
same whatever the frame budgets were.

## Recorded by the screen (found by playing the difftest lists back)

Besides the entry points that changed the game already: `Game::event_chord` (one draw of the
generator, made by the screen as a window opens: `PlayerAction::Chord`) and `Game::window_closed`
(the events a closed window allows: `PlayerAction::MapWindowClosed`). Both were engine-only and
drifted a playback. `actions::scope` finishes a paused step at each outermost action whether or not a
recorder is attached, and sets the hook's stamp to the action's place, so an engine record made by
the action (the battle's patch globals) carries the place it came at.

## Closing a session (the exit hook)

`ui::recording` holds the live session and closes it when the game ends: a return to the menu, a new
game, a load (before the new file opens, as a resume truncates it), the setting turned off, and the
app's quit (`App::shutdown`). `Session::close(game)` finishes the paused step and the snap, flushes
the day's entries, writes the game's state as a last keyframe (when it can be taken), and syncs the
file. A return to the menu has no game left to keyframe: it writes the entries only.

## Tests and where they stand

- `every_difftest_list_plays_back_from_its_recording` (`difftest/tests.rs`): each list of
  `tools/difftest` is recorded (a check after each action) and played back from the snapshot after
  its new game; all nine must end in the live state.
- `a_recorded_campaign_plays_to_its_end_and_seeks_to_any_day` (РК1 and ДС2, 25 days with a battle):
  plays to its end, seeks to days, and matches the straight playback at each place.
- `a_random_session_of_sixty_days_plays_back_day_by_day`: seeded random play (walks to towns and to
  armies, purchase attempts, days) for 60 game days on РК1 and ДС2, every day's end checked.
- `closing_a_session_keeps_its_pending_entries_and_its_end_state`: a close mid-day keeps the entries.
- `a_corrupted_checkpoint_stops_the_playback_with_the_version_message`: the drift stop and its place.
- `a_replay_of_other_rules_still_plays`: a 3-day replay with another rules fingerprint plays to its end.
- `ram_while_recording_a_long_session`: ignored; prints the resident memory of 60 days with and without
  recording next to the file's size (RSS is allocator-dominated; the file is about 51 KB for 60 days).

## Open

- The battle export (phase 4): not built. The watching screen is in (the last section).
- The random session's purchase path and its ДС2 battles (see `docs/OPEN_QUESTIONS.md`).

## What a checkpoint compares (and what the screen does at each)

A checkpoint (a keyframe, a `Check` entry, a day end) is the hash of the state after the stop's
paused step is finished and its snap is done (`finish_pause`, then `armies_snap`). A keyframe is
written only when no snap is due (`Session::pump`), so the snap of the next compare is a no-op there
and the keyframe equals it. The screen's card press is `Battle::act_with` (recorded as
`BattleAct::ActWith`); the event queue is taken with `Game::drain_events` (recorded as
`PlayerAction::DrainEvents` when events wait). A contact step is zero-time: `Game::zero_time_next`
lets a stop before the next stretch pass it, and `Game::chase` reads the route a chase would set, so
the check does not change the game.

A check is a look, so it does not play a zero-time contact either: the live driver can take a hash
right after a walk is set (before its first step), and the state there still has the contact to
come. `Player::apply` therefore stops a `Check` with `reach_stretch(.., contacts = false)`; an action
plays the contact first (`contacts = true`), as the live hero's next frame does. Playing it before a
check drifted the 30-day sample at day 14 (`a_thirty_day_scripted_session_plays_back_to_the_same_state_every_day`).

### The state machines of the map step (drift hunt, 2026-11-11)

Each machine, the variables its transitions touch, the input that triggers it, and whether playback
reproduces it. "Stop" means the place a check or action stands at (stretch end, ask).

| Machine | Transition | Variables | Trigger / record | Playback |
|---|---|---|---|---|
| Hero walk | idle → planned | `path`, `goal`, `talk_to`, `click_buildings`, `wait_ticks`, `endless_wait`, `reading` | `Walk` (`set_destination`) | the action, at its stretch |
| | planned → stepping (paused mid-stretch) | `pause`, `step_elapsed`, `since_step` | frame budget only | frames; `finish_pause` at each stop |
| | step → arrived / blocked / chase ends | `pos`, `path`, `goal`, `talk_to`, `step_flag` | frames (`begin_walk`, `follow_army`) | frames of the playback's own dt |
| | zero-time contact (army or garrison ahead) | `foe`, `path`, `talk_to`, `point_zeroed` | none: a frame with no time | action: played before its place; check: not played (fixed) |
| Events | queued → held → shown | `pending`, `finished_events`, `held`, `marks` | frames; screen glide | `held` must be empty at a stop |
| | shown → answered / closed | `held`, `held_shown` | `answer_question`, `event_window_closed`, `DrainEvents` | the action, at its place |
| Battle | pending (`foe` set) → started → fought → resolved → written back | `foe`, battle state, patch globals | `StartBattle`, `Battle(act)`, `Max`, `ResolveBattle`, `Patch` | the battle's own actions; enemy turns by record or AI |
| Waiting | begin → tick per stretch → window ends it | `wait_ticks`, `endless_wait` | `Wait` | frames to the stretch |
| AI armies | planning (`plans`) → arrivals (`ai_move_*`) → snap (idle draws) | `plans` (empty at rest), army fields, `rng`, `idle_*` | none; the snap at each stop | `armies_snap` at each check, as the driver's `hash` |
| Clock | midnight / noon | `clock`, daily flags | time | frames |

Every transition that changes a state the hash reads is either an input (recorded), a frame of the
playback's own time, or a zero-time contact. The last kind is the only one a stop can split from the
live order, which is why a check and an action place it differently.
- A game restored from its save with recording off keeps no session; turning the setting on starts
  a new file from the game's state.
- The `ui::recording` setting row and the frame hook are UI changes: the owner's approval of the
  «Прочее» screenshots is needed before a push to main.

## The watching screen (`ui/watch.rs`, phase 3 UI)

`Screen::Watch(WatchView)`. `WatchView` holds the `ReplayPlayer`, the camera (`MapView`), the mode
(`Mode`: Playing, Seeking, Choosing, Battle, Ended, Drifted), the messages, the battle being watched
and the screen «Выход» returns to. The state machine draws nothing: `tick(real_dt)` runs the
playback at the speed (a seek at 60 game seconds a real second), `choose(Choice)` answers a battle's
choice, `skip_battle`, `seek_day` and `seek_place` (the timeline), `watch_to_drift`. `frame` draws
the map (`world_view::watch_map`: the world as the map is drawn, the camera on the hero while
«Следить за героем» is on, the wheel and keys zoom, the arrows and a drag pan, the 2.5D keys turn it)
over the area above the strip (`world_view::set_watch_area`), then the strip, the messages and the
drift window.

- The strip (`strip_layout`, pure): one row on the desktop (play, the speeds 1× to 16×, the day, the
  timeline, «Следить за героем», «Выход»); stacked on a phone (the timeline on top, the play and the
  speeds, then the day, the follow button and the exit). Under 1000 dp wide it stacks. Buttons are
  44 dp or more.
- The timeline: `fraction_of` / `day_at` map days to the bar; `mark_at` hits a mark within 9 px. A
  mark's hover tooltip names a battle's two sides' units, a town taken, the hero's fall (the army's,
  `Game::army_fallen`) or the victory. A click on a mark seeks to its place; a click elsewhere to its
  day. While seeking the bar says «Перемотка…».
- A battle: the playback stops at its start (`Pump::Battle`), the strip offers «Смотреть бой» and
  «Пропустить бой»; after 3 s the default is taken (`default_choice`: skip at 4× and faster, watch
  below). Watching builds `BattleView::replay` (both sides by the view's own turns; the player's
  turns from the recorded queue, the enemy's «Максимум» moves from the records through
  `ReplayPlayer::in_playback`; no search runs); its strip (pause, speed, «Пропустить») is the
  screen's. «Пропустить» gives the rest of the battle to the rules at once.
- Messages: a scenario's dialog (through `story::show`) and the events `describe` names close
  themselves after 2 s at 1×, scaled by the speed, not under 0.6 s (`toast_secs`).
- Drift: a mismatch stops the playback (`Mode::Drifted`): the window names the version that
  recorded the record and offers «Смотреть до этого места» (the last good place, paused) and «Выход».
- Entry points: «Смотреть запись» in «Итоги» (`summary::Exit::Watch`; the page is the back screen),
  in the diary's foot on the desktop (`items_view`, left of «Закрыть») and on the phone's diary (`PhoneDiary::record`),
  and the load window's «Запись» (`saves.rs`, the chosen save's file when it is there). `watch::open_record` saves
  the live file's day (`Session::save_point`) and opens it.
- The snapshot scene `watch:<steps>` (`RAZDOR_WATCH=<.rzreplay>`; steps `play`, `battle`, `drift`,
  `corrupt`, `watch-battle`, `skip-battle`, `day=<n>`, `speed=<i>`) renders the screen for the shots.
  A sample record comes from the ignored test `write_a_sample_record_for_the_screenshots`
  (`RAZDOR_SAMPLE_RECORD`).

Tests: the pure layout and time functions (`ui::watch::tests`), and
`the_screen_plays_a_record_through_its_states` (play, the choice, a watched battle skipped, a seek to a
battle's mark, a seek by day, a drift and its window), which needs `RAZDOR_WATCH_RECORD`.

## Open (the watching screen)

- The map music during a replay (the watch plays no music yet).
- The recorded battles' pacing is the battle view's; the enemy's turns animate as the battle view plays them.

## The interface language (found by the watching screen's drift, 2026-11-11)

The rules write texts into the game (the journal's entries, `tr("Event")`), so the states depend on the
interface language. The header records it (`ReplayHeader::language`) and a playback runs its rules calls
in that language on its thread (`i18n::set_override`, set in `ReplayPlayer::arm`, restored in `disarm`;
nested calls keep the first one's). A record made in Russian plays back with the interface in English
(`a_record_made_in_russian_plays_back_with_the_interface_in_english`). Open: a language change during
a session makes the later texts differ from the record's language; the record keeps the start's language.

## Open (the watching screen)

- «Смотреть до этого места» from a drift seeks back into the file and drifts again on the corrupted
  sample (`ui::watch::tests::the_screen_plays_a_record_through_its_states`, run with `RAZDOR_WATCH_RECORD`).
