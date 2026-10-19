# Flow: saves and loads

**Read this first.** A save (`.rzsave`) is bzip2-compressed JSON of `{meta, game}`: the
whole `Game` through serde, minus everything that comes from the scenario or the content
(map, texts, event scripts, unit/item/spell definitions). Loading reads the map again from
the install (it must be the same bytes: an FNV-1a hash is stored) and rebuilds the statics.
All code is in `src/rules/save.rs`; the windows are `src/ui/saves.rs`.

## Where saves live

- `save::default_dir`: the platform data folder (`razdor/saves`), or `RAZDOR_SAVE_DIR`
  (`save::DIR_ENV`; tests and profiling use a scratch folder). `audio.json` (settings) and
  `pinned.txt` lists live alongside.
- `save::games_dir`: one sub-folder per install (folder name + hash of its path), so a mod
  and the plain game do not mix saves; `claim_old_saves` moved older saves in once.
- `manual/` (player's saves and quick saves, `MANUAL_SAVES_KEPT`, `QUICK_SAVES_KEPT`) and
  `auto/` (autosaves, `AUTOSAVES_KEPT`).

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
      │
      ▼
save::write(dir, kind, name, game)          manual; quick → save::quick_save
save::write_autosave(dir, name, game, in_battle)   original's slot reuse (0x4b7410)
      │
      ▼
put(): save::meta_of → save::encode (serde_json + BzEncoder) → write .tmp → rename
       → remember_meta (the meta cache knows the new file at once)
```

- Autosaves: one before every battle (`saves::battle` saves the moment *before* the
  fight with the foe taken off, so loading it puts the hero next to the enemy) and one at
  the noon report (`Game::autosave_due`, named by `save::date_name`). The install's
  `OptValue8` can turn them off (`autosaves_on`).
- «Рестарт» of a battle keeps the encoded bytes of that moment in
  `BattleView::restart_state`, not a file. A save holds no AI level (`Game::improved_ai`,
  `max_ai` are the settings'): `App::restart_battle` and `App::load` call
  `main_menu::follow_settings` before the battle starts, so the restarted battle is fought by
  the same AI (#19; before, it fell back to the original's AI on its normal level).

## Loading

```
ui: saves::load_screen sets App::pending_load → App::load (after the frame)
      │
      ▼
save::load(path, demo, install) → save::decode (unpack, check_version, serde)
      → save::restore: re-read the map, check the hash, World::from_scenario,
        World::restore_statics, EventEngine::restore_statics, a fresh generator
        (Rng::save_load_with_music, as the original reseeds on load), AI set up again
        (Game::ai_init), format migrations (e.g. migrate_spells for version < 7)
```

Errors are `save::SaveError` (version, map missing or changed, mismatch, full, pinned) and
show in the load window.

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
- Never commit a player's save (it may embed map data): build fixtures in code, or gate a
  test on a file outside the repo existing.

## Traps

- `Content` is not saved: a save names its scenario (`ScenarioRef`), and the row width the
  game started with (`Game::wide_row`) is re-applied on load and holds for the session
  (`saves::session_content`).
- The generator is not saved (format 2+); anything that must survive a load and depends on
  randomness must be stored as state.
- Writes go through `.tmp` + rename; keep it so a crash never leaves half a save.

<!-- refs verified by: python3 -I docs/flows/verify.py docs/flows/*.md -->
