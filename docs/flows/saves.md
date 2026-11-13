# Flow: saves and loads

**Read this first.** A save (`.rzsave`) is bzip2-compressed JSON of `{meta, game}`: the
whole `Game` through serde, minus everything that comes from the scenario or the content
(map, texts, event scripts, unit/item/spell definitions). A map save also carries its map's
bytes (the `map` field, base64; saves-data §12.2: the original's save is self-contained). Loading
takes the map from the install when its bytes hash to the stored FNV-1a hash, else from the
save's own bytes when those do, and rebuilds the statics. A save with neither is refused.
All code is in `src/rules/save.rs`; the windows are `src/ui/saves.rs`.

## Where saves live

- `save::default_dir`: the platform data folder (`razdor/saves`), or `RAZDOR_SAVE_DIR`
  (`save::DIR_ENV`; tests and profiling use a scratch folder). `audio.json` (settings) and
  `pinned.txt` lists live alongside.
- `save::games_dir`: one sub-folder per install (folder name + hash of its path), so a mod
  and the plain game do not mix saves; `claim_old_saves` moved older saves in once. The key is
  the install's path (on Android the app's own game folder).
- `manual/` (player's saves and quick saves, `MANUAL_SAVES_KEPT`, `QUICK_SAVES_KEPT`) and
  `auto/` (autosaves, `AUTOSAVES_KEPT`).

- Android: the saves folder is `platform::app_dir` (`<files>/Razdor/saves`, the app's own folder; no
  storage permission, see [app-and-ui.md](app-and-ui.md)). The saves of an older release come in
  through the folder picker (`ui::saves_move`, `transfer::move_old_release`). The picked folder is listed
  (its files counted), then copied into the cache's `old-saves` file by file; the window «Перенос
  сохранений» shows «N из M файлов» with a bar (`razdorOldSavesProgress`, the Java side writes the counts),
  and while the count runs an animated ellipsis without a bar. The import then runs on a thread behind the
  same window (the screen under it takes no input, the game's start waits), and the decision is written
  on the main thread when it ends (the decision is `done`; the App then reads the hub again, see below).
  Export and import go through the system's documents
  (`ui::transfer`, «Перенос…» in the Load window): one zip out, `.rzsave` or a zip in, nothing overwritten.

## Moving an older release (`transfer::move_old_release`)

- The old install's folder under the copy is named by the old install's path hash, so the new
  install's folder (`saves_dir_of`, the new path) never matches it: the moved saves were invisible
  (1622). The move therefore lists each old install folder (and a flat `manual`/`auto` at the
  copy's root, as the name `""`) in `saves/moved.txt` (`save::WAITING_FILE`, `save::add_waiting`).
- `save::adopt_moved(root, own, install)` places the waiting saves whose map the install plays into
  its own folder: the same map bytes rule as `claim_old_saves` (the demo's saves too), pins kept, a
  name taken by other bytes gets the next number (`transfer::numbered_name`), the same bytes are one
  save (`Adopted::duplicates`, the waiting copy goes). The rest stay in their folders and the names
  stay on the list; `save::games_dir` adopts for each install once per run (`adopt_moved_once`, a
  game's own folder is not created by it). A waiting folder with no saves left is removed.
- Maps and mods: `hub::import_old` copies each listed entry with its files (a mod's manifest too; the
  copy leaves out a mod's merged `game` and `archives`, and the hub's `cache` and `downloads`:
  `razdorHubSkip` in `android/MainActivityInject.java`). The move's end sets `saves_move::take_finished`,
  and the App's `saves_tick` runs `reload_hub_maps`: the mod choice (`App::mods`, read once at start
  before) and the new-game list show them without a restart (1621).
- The result line (`saves_move::outcome_text`) says what came into this game, what it already had and
  what waits for another map or mod; it never says «moved» for a save that is not in the folder.

## Limits and pins

- Own saves: at most `MANUAL_SAVES_KEPT` (50) **unpinned** ones, quick saves not counted; a
  save of a new name past that is `SaveError::Full` (nothing is rotated out), saving over an
  existing unpinned one always works.
- Pinned saves (`pinned.txt` per folder, `save::set_pinned`) are not counted anywhere and have
  no limit (the owner's deviation, 2026-10-17): pinning never fails, a pinned save is never
  overwritten (`SaveError::Pinned` as a target) nor rotated out, and the list scrolls to all.
- Quick saves: `QUICK_SAVES_KEPT` (5) unpinned, reused oldest first. Autosaves:
  `AUTOSAVES_KEPT` (30) unpinned, reused as the original (0x4b7410). Pinned ones of either
  are left out of the rotation.
- «Clear all» deletes the tab's unpinned saves only; deleting a save drops its pin.

## Writing

```
ui: saves::save_screen / App::quick_save / saves::autosave / saves::battle
      (save window's «Быстрое сохранение» → saves::quick_save_message → save::quick_save)
      │
      ▼
save::write(dir, kind, name, game)          manual; quick → save::quick_save
save::write_autosave(dir, name, game, in_battle)   original's slot reuse (0x4b7410)
      │
      ▼
put(): save::meta_of → save::encode (serde_json + BzEncoder) → write .tmp → rename
       → remember_meta (the meta cache knows the new file at once)
```

- The save window's bottom row (`saves::save_button_row`): «Быстрое сохранение» (the owner's
  request, not in the original; 8 px left of «Сохранить», which keeps the original's place), then
  «Сохранить» and «Отмена». The quick save is enabled when `hotkeys::allowed` says F5 may fire on
  the map (`App::frame_content`'s `quick_save_ok`: a game, no foe, no endless wait); it writes the same
  quick save and shows the same message as F5, then closes the window. On a phone the click area
  of the row's buttons is at least 44 dp tall (`saves::finger_hit`; drawing unchanged).
- The map's bytes: `Game::map_bytes` (not serialised; `Game::set_map` keeps them with the origin
  when a map game starts or a campaign map starts, `Game::restart` copies them). `encode` writes
  them as the `map` field only when they hash to the origin's hash. The field is optional: an
  older save has none, and an older Razdor reads a new save by ignoring it (no format bump).
- Autosaves: one before every battle (`saves::battle` saves the moment *before* the
  fight with the foe taken off, so loading it puts the hero next to the enemy) and one at
  the noon report (`Game::autosave_due`, named by `save::date_name`). The install's
  `OptValue8` can turn them off (`autosaves_on`).
- The noon autosave is written on a background thread (Razdor's; `saves::autosave_at_noon`,
  `save::write_autosave_in_background`). The frame takes the save's meta and its JSON
  (`encode_json`, about a millisecond); the thread (`spawn_autosave`) packs it (bzip2, the
  most of the cost), rotates the autosaves as `write_autosave` does (`store_autosave`) and
  writes the file. The file is the same bytes the synchronous path writes. The writer is held
  in `PENDING_WRITE`: `finish_pending_writes` joins it. It is joined before the next
  autosave (battle or noon), before the game quits (`exit_now`), before a save is loaded
  (`save::load`) and before a folder is listed (`list_files`, `list`, `quick_save_path`).
  The battle autosave stays synchronous.
- «Рестарт» of a battle keeps the encoded bytes of that moment in
  `BattleView::restart_state`, not a file. A save holds no AI level (`Game::improved_ai`,
  `max_ai` are the settings'): `App::restart_battle` and `App::load` call
  `main_menu::follow_settings` before the battle starts, so the restarted battle is fought by
  the same AI (#19; before, it fell back to the original's AI on its normal level).

## Map slot (continue a map)

Razdor's own (the owner's request): every playthrough keeps one latest save of its map, apart
from the load window's saves, so playing another map never loses it.

- What: the latest autosave of the map being played, the same bytes. The key is the map's own file
  name, `Game::origin` (set by `screens::start_game_with_rules` and by `screens::campaign_game` for
  each campaign map), so every campaign map has its own slot; `save::map_slot_key` is `None` for the
  demo. The key is lowercased and passed through `slug`. `Game::campaign` (the map the playthrough
  started on, carried to the next campaign map by `save::campaign_of`, kept by `Game::restart`) no
  longer keys the slot. A slot made before this, under a campaign's first map, is that map's slot.
- Where: `<games folder>/maps/<key>.rzsave` (`save::map_slot_path`), a sibling of `manual/` and
  `auto/`. Nothing lists `maps/`: `list`, `list_files`, `by_time`, the rotation, «Очистить всё» and
  `claim_old_saves` only walk the two kind folders.
- When written: at every autosave moment, the same bytes as the autosave. `save::write_autosave`
  and the noon background path (`spawn_autosave`, which takes the key computed on the frame) write
  it after the autosave. With the install's `OptValue8` off, `ui::saves::map_slot_only` writes it
  alone (`save::write_map_slot`, synchronous). Nothing else writes it (not manual or quick saves,
  not the session slot).
- Read: `save::map_slot(dir, key)` (the meta through `read_meta`; a missing or damaged file is
  `None`). The hero window (`new_game::class_select`, marble path) looks it up once when it
  opens (`HERO_SLOT`, cleared by `leave_hero_window`) and offers «Продолжить» left of «Начать»
  (no date line: the «Начать» question names the slot's date). The click sets `CONTINUE_REQUEST`, which
  `App::frame_content` takes and loads with `App::load`, as a save picked in the load window is
  (a failed load is a dialog on the main menu). «Начать» on a map with a slot first asks (`HERO_ASK`, `saves::question`): Yes starts
  the game as before, No returns to the window; the row takes no clicks meanwhile.

## Session slot (resume after the OS kills the app)

The phone's OS may end the app in the background, and the game may crash: the player then
loses what was played since the last save. The session slot is the answer (the owner's
decision, 2026-10-29; `ui::session`, `save::SESSION`):

- The slot is `auto/session.rzsave`, named «Последний сеанс» (`save::SESSION`). It is listed
  in the load window's autosave tab (with its name translated), but `write_autosave` leaves it
  out of the rotation and out of `AUTOSAVES_KEPT`, so it is never reused as an autosave.
  `save::session_json` builds it (`to_json`, the same JSON as a save), `save::write_session`
  packs and writes it (`.tmp` + rename) and then makes the marker `session.live` in the
  save folder (`save::session_marker`).
- The marker says a session is under way. It is removed when the game is left (the game
  is gone: the main menu, the editor, a custom battle), when the app closes (`App::shutdown`,
  after the pending writes), and at the launch.
- When it is written: the newest state is kept in `ui::session` (`LATEST`). It is looked at on a rest
  frame only (the hero stands and no step's window is playing), at most every 2 s of play, so a walk
  costs nothing and the first rest frame after a walk takes the look (the last stop's state). A look
  serialises only when a cheap key changed (the game clock, the hero's place and purse, the army's
  hit points, experience and levels; no allocation), and every 30 s regardless.
  Battle start (`saves::battle`, with the foe taken off) sets it and writes it at once. The slot is
  written every 30 s of play when the game changed (the hash is of the JSON from the `"game"` field on,
  so the meta's time alone does not count), and when the window loses its focus, even in a battle
  (the last savable state). Only where F5 may fire on the map (the quick save's guard of the frame),
  and never in a test play or a battle.
- Leaving the app writes at once (`ui::session::flush_now`, callable from any thread): Android
  `onPause` (the Java inject `android/MainActivityInject.java` declares `razdorOnPause` and calls it
  from `MAIN_ACTIVITY_ON_PAUSE`; the export is `Java_io_github_indicozy_razdor_MainActivity_razdorOnPause`
  in `ui/android.rs`); iOS's `UIApplicationDidEnterBackgroundNotification` and
  `UIApplicationWillTerminateNotification` (`ui/ios.rs`, an observer registered at start); the
  desktop's SIGTERM and SIGHUP (`install_exit_hooks`: the handler only stores the signal, a watcher
  thread writes the slot and `_exit`s with 128 + the signal, the marker stays so the next launch
  offers to continue; SIGINT is left alone). Windows: nothing extra, since the focus loss already
  writes and miniquad does not deliver the logoff or shutdown messages. The frame loop does not run
  in the background, so these hooks are the only writes at the moment of leaving.
- Locks: `WRITE_LOCK` is held around every write of the slot and every removal of the marker (the
  writer thread and the hooks), and taken before `LATEST` and `WRITTEN`. `WRITTEN` is the hash on
  disk, so a state already written is not written again. When the session ends (the game is left,
  `end_session`), `LATEST` and `WRITTEN` are dropped under the lock before the marker goes: a hook
  after that writes nothing and never brings the marker back.
- Cost: the look is the frame probe's `Stage::Session` (outside the tick; the CSV's `session`
  column). A walk frame does no look and no key computation (`session` is 0 on walk-step frames);
  a rest frame between looks does a time check and the focus check only. A look computes the
  cheap key (no allocation); the JSON and its hash run only when the key changed, or every 30 s.
  Measured on CPUs 8-19: the JSON and hash take 0.5 ms on the walk save, 1.3 to 1.7 ms on a fresh
  largest community map (Другой берег, community-update), and 0.7 to 0.9 ms on Evolution's.
- The writes run on one writer thread, in the order sent (`Write` writes `LATEST` when it is not on
  disk yet, `End`, `Flush`); the bzip2 packing is off the frame loop.
- The launch: when the marker is still there, the app ended without a proper exit. The
  marker goes, and when the slot exists a question «Продолжить игру?» (the map's title and
  the game date) is the first window. Yes loads the slot (a failed load is told in a
  plain window); No does nothing. Only the Yes button answers it: a key does not load a
  game (the scenario questions still take any key as Yes, `widgets::answer_of`). It is not a
  scenario question (`Dialog::resume`), and it is looked for again once a phone's install
  is adopted (`App::session_checked`).

## Loading

```
ui: saves::load_screen sets App::pending_load → App::load (after the frame)
      │
      ▼
save::load(path, demo, install) → save::decode (unpack, check_version, serde)
      → save::restore: the map (`save::map_bytes`: the install's copy when its hash matches, else the
        save's own bytes when they match, else `MapChanged` / `MapMissing`; the bytes go back
        into `Game::map_bytes`), World::from_scenario,
        World::restore_statics, EventEngine::restore_statics, a fresh generator
        (Rng::save_load_with_music, as the original reseeds on load), AI set up again
        (Game::ai_init), format migrations (e.g. migrate_spells for version < 7)
```

Errors are `save::SaveError` (version, map missing or changed, mismatch, full, pinned) and
show in the load window. A save of a map the install lacks loads from its own bytes, so a
player can load a save sent from another install.

Razdor's crash guard (1552, not the original's): a save made with another mod names units or
items the install lacks. `restore` checks the save's map (`map_refs::unknown`, before the world
is built from it) and the save's state (`map_refs::unknown_in_game`, before anything reads them
by id; the same places `Content::unit` would be asked for) and refuses it with
`SaveError::Lacking { name, unknown }`. The load window shows its text (the save's name and the
ids, `Unknown::text`) in its message box, and the mod button of `ui/lacking.rs` beside OK.

A save records the hub mod it was made with: `SaveMeta.played_mod` (JSON `mod`, `SaveMod { id, version }`,
optional, none for the base game). `save::set_played_mod` sets it (the App does, in
`App::record_played_mod` at start and on every `refresh_mods`); `meta_of` writes it. An older save
has no field and reads as none (`an_old_save_without_the_mod_field_reads_as_none`); no format bump. Moving on to the next campaign map still
needs that map in the install (the hand-over reads it from there, as the original does), and
the install is still needed for the content (`NoInstall` without it).

## The load window's list (progressive)

The book's column headings use the pale green interface label colour; save names and scenario
titles are yellow, while the save time is orange in SanSerif. The manual/autosave tabs switch
size and colour immediately with selection. An idle spine is grey at 80%, a held one at 90%,
and a selected spine is full size and drawn last across the book outline and steel line. Each
tab's idle and selected artwork choices are defined separately in `src/ui/saves.rs`.

The delete sign opens a confirmation with the captured save name in blue and Yes/No
buttons. The book beneath turns grey and ignores input until the confirmation closes;
only Yes deletes the captured file, while No or Escape keeps it.

Opening the list never reads a save on the UI thread (commits c04fa1b, b2ecf89):

- `save::list_files` lists the folder only (name, length, mtime → `ListedSave`, ordered
  pinned first then newest file first) and fills metas already in the cache.
- `save::want_metas` asks reader threads for the missing metas, rows on screen first
  (`request_metas` puts them at the front of `META_QUEUE`); up to `META_READERS` threads
  run `meta_reader`. bzip2 cannot unpack only the start of a file, so a meta read is a
  whole-file unpack — that is why it is off the UI thread.
- `META_CACHE` keys metas by path and `FileStamp` (length, mtime): a file written over is
  read again; `META_FAILED` remembers unreadable files so their rows go.
- Each frame `ListedSave::fill` picks up what is ready (`MetaState`); rows waiting show the
  file time dimmed. When all are read, `save::sort_listed` puts them in the order of the
  time saved once (a selected row stays selected).
- `save::warm_meta_cache` reads the install's folder at startup, newest first.

## The run's mark (1532; `rules/marks.rs`)

A run (a game from the map's start) carries its mark: green, grey (a gameplay setting that is not
the original's), yellow (an experimental rule on) or red (a cheat used), with the first cause as
its reason. It is `Game::run_mark` (`run_mark` in the save, `#[serde(default)]`), so it rises and is
saved with the run. It is brought up to date every frame from the settings
(`main_menu::follow_settings` → `Game::note_settings`; the classification is one function,
`ui::audio::Settings::gameplay_differs_from_original`), on every load, and by the cheat console
(`Game::note_cheat`). A load or a battle's restart in the same run keeps the worse mark
(`Game::keep_run_mark`); a new game, a campaign's next map and «Рестарт» of the map start a new run.
A save from before the mark has no `run_mark`: its history is unknown, and it reads green (the owner's
decision, 2026-11-13: a fair run), except where the save itself shows a cheat (`cheats.used`, red), an
experimental rule (yellow) or a changed setting (grey). Its tooltip still says the history is unknown
(`new_game::mark_text_in`). Records written grey for an unknown history are read as green (`Records::from_json`). A parity switch
that the save lacks (inherited, `ExperimentalRules::inherited`) is on but does not make the run yellow.

The player's best mark per map (and its date) is `marks.json` beside `settings.json`
(`marks::Records`, keyed by the map's file name without the extension, as the favourites are). A
victory writes it (`ui/mod.rs` at the scenario's end, next to the list's tick); a loss writes nothing.

## The chronicle (`rules/chronicle.rs`)

`Game::chronicle` (the «Летопись», `campaign-scenario.md` «Chronicle») is saved whole, chapter by
chapter; it is `#[serde(default)]`, and the hand-over carries it in `NextMap::chronicle`. A save from
before it has no record: `save::restore` starts one at the load with an «earlier days not recorded»
line (`Chronicle::begin_after_load`), and a save that has a record keeps it as it is. Its texts are
references (scenario event ids, catalog keys), so it stays small (`SIZE_BUDGET_300_DAYS`).

## The replay of a save (`recording/session.rs`)

A game records to a replay file beside its saves (`replays/` in the games folder; the setting
«Записывать игру»). A save made then carries its place in the file: `SaveMeta::replay` (`ReplayRef`:
the file's name and its length). Loading the save resumes that file there (the file is cut at the
place and a new segment starts from the loaded state; `save::restore` sets `Game::replay_resume`).
Deleting the last save that points into a replay deletes the replay file (`save::delete`). The
replay's format and what it records: `docs/flows/replays.md`.

## Compatibility rules when adding state

- New fields on anything saved (`Game`, `World`, `Army`, units, the event engine …) get
  `#[serde(default)]` (or a default function) so older saves load. Fields rebuilt from the
  scenario or content get `#[serde(skip)]` and are restored in `restore_statics`.
- If old saves need a conversion, bump `save::FORMAT_VERSION`, describe the version in
  its doc comment (the list there is the format's history), and migrate in
  `save::restore` keyed on `meta.version`. `OLDEST_VERSION` is the oldest still read.
- Add a test: a save of the previous shape still loads (see
  `a_version_6_save_moves_its_army_wide_spells_into_the_units_slots`,
  `the_journal_history_survives_a_save_and_old_saves_load_without_one`) and a round trip
  (`demo_roundtrip_after_walking_and_a_battle`, `rk1_roundtrip_after_walking_and_a_battle`
  with the install).
- A changed shape can sometimes read the old one without a version bump: the pack
  (`pack::Pack`) is saved as its cells with `null` for a hole, so an older save's plain list
  of items reads as the same items in the first cells (`Game::held_item` is
  `#[serde(default)]` and goes back into the pack on load).
- Never commit a player's save (it may embed map data; a map save carries the map's bytes): build fixtures in code, or gate a
  test on a file outside the repo existing.

## Traps

- `Content` is not saved: a save names its scenario (`ScenarioRef`), and the row width the
  game started with (`Game::wide_row`) is re-applied on load and holds for the session
  (`saves::session_content`).
- The generator is not saved (format 2+); anything that must survive a load and depends on
  randomness must be stored as state.
- Writes go through `.tmp` + rename; keep it so a crash never leaves half a save.

<!-- refs verified by: python3 -I docs/flows/verify.py docs/flows/*.md -->
