# Android without "All files access" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Razdor on Android stops asking for `MANAGE_EXTERNAL_STORAGE` (and, on Android 8–10,
`READ/WRITE_EXTERNAL_STORAGE`). Everything Razdor writes lives in the app's own folder; the game
archive, maps and saves come in and go out through the system file picker.

**Why:** since Android 11 (scoped storage) an app opens files by path only inside its own folders.
Razdor uses plain `std::fs` paths in the shared storage (`/storage/emulated/0/Razdor`,
`/storage/emulated/0/DiscordTimes`) and scans `Download`, `Games` and the whole internal storage,
so it needs the all-files permission. That permission is a scary settings page for players, and
Google Play allows it only for file managers, backup apps and the like.

**Architecture:**
- `platform::app_dir()` and `platform::unpack_dir()` on Android return folders under the app's
  external files dir (`Context.getExternalFilesDir(null)` → `/storage/emulated/0/Android/data/io.github.indicozy.razdor/files`),
  read once over JNI at start-up and kept in a `OnceLock`. Rust `std::fs` works there with no
  permission on every Android Razdor supports (min SDK 26).
- `platform::install_roots()` on Android returns only `unpack_dir()`: no scan of the shared
  storage. The install is always the one Razdor unpacked.
- The game comes in through the picker (`ACTION_OPEN_DOCUMENT`, already in
  `android/MainActivityInject.java`). Instead of copying the archive into the cache, the Java side
  opens the picked URI as a `ParcelFileDescriptor`, detaches the fd and hands it to Rust, which
  reads it as a `File` (`FromRawFd`) and unpacks straight into `unpack_dir()`.
- Maps and saves: import via the picker (exists for maps), export via `ACTION_CREATE_DOCUMENT`
  (new), so players can still move files in and out.
- Existing players get a one-time migration of the old shared-storage folders.

**Tech Stack:** Rust, macroquad/miniquad JNI (`ui::android`, `razdor::jni`), the Java inject
`android/MainActivityInject.java`, cargo-quad-apk (`Cargo.toml` `[package.metadata.android]`).

**Spec (to agree with the owner before Task 1):**
- No storage permission of any kind in the manifest (from release N+1; release N keeps it for the
  migration, Task 6). The not-found screen no longer sends the
  player to a settings page.
- The not-found screen offers «Выбрать архив игры…» (pick the 7z/zip); after the pick it unpacks
  with the existing progress UI. The patch archive (Community Update zip) goes the same way.
- Saves, the log, the editor's maps, the hub and the translation jobs move to the app's folder.
- New: «Экспорт сохранений…», «Экспорт журнала…» and «Экспорт карты…» write a file through the
  picker. Import of maps stays as it is; import of a save is new.
- One-time migration for players who already granted all-files access (details in Task 6).
- iOS and desktop do not change.

## Global Constraints

- Machine: every cargo and heavy command is `CARGO_BUILD_JOBS=12 nice -n 19 taskset -c 8-19 …`;
  never CPUs 0–7. Scratch dirs and the agent's `CARGO_TARGET_DIR` go under
  `~/.cache/razdor-scratch/android-storage/`.
- Work in an agent worktree. Commit there; **do not push** until the owner has seen it running on a
  phone (Task 8).
- Code, comments and commit messages in English; the changelog and the wiki ru page in Russian.
- Keep every JNI call's pending exception cleared and every local reference freed, as
  `ui::android` already does (a refused call must never end the process).

## What touches the storage today (inventory)

| Where | What |
| --- | --- |
| `src/platform.rs:56-84` | `ANDROID_ROOT` `/storage/emulated/0/Razdor`, `app_dir()` |
| `src/platform.rs:170-210` | `install_roots()` (DiscordTimes, Download, Games, whole storage), `unpack_dir()` `/storage/emulated/0/DiscordTimes` |
| `src/platform.rs:255-300` | `android_document_uri()` (opens Files at a shared folder) + its tests |
| `src/ui/android.rs` | `storage_granted()` (264), `ask_once()` (318), the all-files / app settings intents, `open_folder()` (140) |
| `src/ui/mobile.rs` | not-found screen: storage check (103, 118), install poll (150), archive search + unpack (217) |
| `src/ui/mod.rs:1225-1240` | the poll that waits for `storage_granted()` before loading |
| `src/dt/unpack.rs` | `find_archives` over folders, `unpack` into the install folder |
| `src/ui/game_import.rs:155,529`, `src/dt/game_import.rs` | «Обновить игру из архива…» (picker, unpack beside the install, swap) |
| `src/ui/map_import.rs:277` | map import via the picker (copies into cache) |
| `android/MainActivityInject.java:270-355` | `razdorPickFile` / `razdorPickOutcome`, copy into `getCacheDir()/razdor-import` |
| `src/main.rs:172`, `src/diag.rs:47-90`, `src/rules/save.rs:263`, `src/ui/saves.rs:986`, `src/editor/files.rs:26`, `src/hub.rs:648`, `src/translate/job.rs:475` | users of `app_dir()` / `unpack_dir()` (no change once the functions move) |
| `Cargo.toml:62-88` | manifest comment, `requestLegacyExternalStorage`, permissions |

## Tasks

## Release N execution order (rulings 2026-11-09)

This branch implements release N only: Tasks 1, 2N, 3, 4, 5, 6 (its release-N part), 7, in
that order. Task 2 proper (dropping the permissions from the manifest, deleting the grant code)
and Task 6's release-N+1 part (the tree-picker fallback) are a later branch. Task 8's release-N
checks are the owner's device check before the push.

### Task 2N: Stop waiting for the grant (release N)

- [ ] `ui/mod.rs` (~1218-1240) and `ui::mobile`: new players are never sent to the all-files
  settings page and the install load does not wait for `storage_granted()`: the install is
  "listed" when `unpack_dir()` holds one. Remove the start-up `ask_access_once()` call and the
  not-found screen's grant button; keep `storage_granted()` itself (Task 1's `starts_migrated`
  and Task 6 read the legacy folders only when they are readable). The manifest permissions stay
  in N.
- [ ] `Cargo.toml` `[package.metadata.android]`: add `hasFragileUserData = true` to the
  application (check cargo-quad-apk's key name for application attributes; if it has none, add
  it through the same mechanism the manifest already uses for `requestLegacyExternalStorage`).
  Android 10+ then asks on uninstall whether to keep the app's data.
- [ ] `update::phone`: confirm the APK download goes to the app's cache (`phone::cache_dir`), not
  the shared storage; change it if not.
- [ ] Tests: whatever logic moved is covered by existing ui::mobile tests; add one if a pure
  function changed. Build the APK once (see Global Constraints) to check the manifest.

### Task 1: The app's folder from Java

- [ ] In `MainActivityInject.java` add `public String razdorFilesDir()` returning
  `getExternalFilesDir(null)`'s absolute path, falling back to `getFilesDir()` when it is null
  (no external volume).
- [ ] In `ui::android` (or `razdor::jni`) read it once at start-up into a `OnceLock<PathBuf>`
  exposed to `platform` (e.g. `platform::set_android_files_dir`, set before anything calls
  `app_dir()`; `main.rs` already runs Android setup early).
- [ ] `platform::app_dir()` and `config_dir()` on Android → `<files>/Razdor`; `unpack_dir()` →
  `<files>/DiscordTimes`; `install_roots()` → `vec![unpack_dir]`. Keep `ANDROID_ROOT` only as the
  migration source (rename it `LEGACY_ANDROID_ROOT`; the legacy game folder is
  `/storage/emulated/0/DiscordTimes`, `LEGACY_ANDROID_GAME`).
- [ ] Release N gate (the migration, Task 6, flips it): a process-wide `AtomicBool` "migrated" in
  `platform`. At start-up, after the files dir is set: migrated = the marker
  `<files>/Razdor/migrated-from-shared` exists, OR the legacy root holds no Razdor data / cannot
  be read (fresh install: write the marker then). While NOT migrated, `app_dir()`, `config_dir()`
  and `unpack_dir()` return the legacy paths (`LEGACY_ANDROID_ROOT`, `LEGACY_ANDROID_GAME`) and
  `install_roots()` the legacy list of today; once migrated, the new ones. Expose
  `platform::android_migrated()` and `platform::set_android_migrated()` (Task 6 calls it after a
  successful copy). Every caller already calls `app_dir()` per use, so the flip takes effect at
  runtime; nothing caches the path (check `diag`, which has `reopen_log`).
- [ ] Keep the decision logic pure and testable: e.g. `fn android_paths(files: &Path, migrated:
  bool) -> AndroidPaths` and `fn starts_migrated(files: &Path, legacy_root: &Path) -> bool`,
  compiled on every target (not `cfg(android)`), so host tests cover them.
- [ ] Tests: the path functions for both states, and `starts_migrated` with a temp dir: marker
  present; legacy empty or missing; legacy with saves and no marker. `cargo test`.

### Task 2: Remove the permission (release N+1: NOT in this branch)

- [ ] `Cargo.toml`: drop `MANAGE_EXTERNAL_STORAGE`, `READ_EXTERNAL_STORAGE`,
  `WRITE_EXTERNAL_STORAGE` and `requestLegacyExternalStorage`; rewrite the storage comment.
  Keep `INTERNET` and `REQUEST_INSTALL_PACKAGES` (updates).
- [ ] `ui::android`: delete `storage_granted`, `ask_once`, the settings intents and `ASKED`.
- [ ] `ui::mobile` and `ui/mod.rs:1225-1240`: the poll no longer waits for a grant; the install is
  "listed" when `unpack_dir()` holds one.
- [ ] Check the update flow (`update::phone`): it must download the APK into the app's folder or
  cache, not the shared storage.

### Task 3: Import the game archive without a copy

- [ ] Java: `razdorPickArchive()` (or a mode of `razdorPickFile`) that, on the picker's answer,
  opens `getContentResolver().openFileDescriptor(uri, "r")`, calls `detachFd()` and returns
  `"ok\n<name>\n<size>\n<fd>"` (size from `OpenableColumns.SIZE`, -1 when unknown). The copy path
  stays for map import (small files).
- [ ] Rust: `dt::unpack` gains an entry that takes a `File` instead of a path
  (`unsafe { File::from_raw_fd(fd) }` on Android). 7z needs `Read + Seek`: a detached fd from
  `openFileDescriptor` on a local document is seekable. If `seek` fails (some cloud providers
  stream), fall back to the existing copy into the cache, then unpack from there.
- [ ] Classify the picked archive with the existing listing check (`install::listing_is_install`,
  `is_install_path`): a whole game unpacks into `unpack_dir()`, a patch goes over it. Unknown
  archive → a clear message, nothing written.
- [ ] Not-found screen: replace the folder search and «Проверить снова» with «Выбрать архив игры…»
  plus the existing progress UI. Keep the archive search code for desktop.
- [ ] Space check: before unpacking, compare the archive's uncompressed size with free space
  (`statvfs` on `unpack_dir()`) and show the needed amount when short.
- [ ] Tests: unpack from a `File` opened on a test 7z and zip (the existing `Scratch` helpers);
  a patch over a game; an unknown archive refused.

### Task 4: «Обновить игру из архива…» on the new folders

- [ ] `ui::game_import` / `dt::game_import`: use the fd entry from Task 3; the
  `<install>.new` / `.old` swap stays inside `unpack_dir()`'s parent (the app's folder).
- [ ] Existing tests keep passing; add one for the fd path.

### Task 5: Export and import through the picker

- [ ] Java: `razdorCreateDocument(String name, String mime)` with `ACTION_CREATE_DOCUMENT`; on the
  answer, copy a file Razdor names (passed in advance, e.g. `razdorSetExportSource(path)`) into the
  chosen URI on a thread; poll like `razdorPickOutcome`.
- [ ] Saves: «Экспорт сохранений…» writes one zip of `app_dir()/saves` (the `zip` crate is already
  a dependency); «Импорт сохранения…» picks a save or a zip and copies into `saves`, refusing
  names that would overwrite without asking.
- [ ] Log: «Экспорт журнала…» writes `razdor.log` (for bug reports in Discord).
- [ ] Maps: «Экспорт карты…» in the editor writes the map file.
- [ ] `open_folder()` / `android_document_uri()`: delete the Android uses that open the Files app at
  `/storage/emulated/0/Razdor` (Files cannot open `Android/data` on 11+); the export buttons replace
  them. Keep `android_document_uri` only if migration (Task 6) needs it.

### Task 6: Migration for existing players (two releases)

Existing installs have the game in `/storage/emulated/0/DiscordTimes` and saves, maps and the log
in `/storage/emulated/0/Razdor`. Dropping `MANAGE_EXTERNAL_STORAGE` (and on Android 8–10
`READ_EXTERNAL_STORAGE`) from the manifest revokes the grant with the update, so the release that
drops it can no longer read the legacy folders by path. The copy therefore happens one release
earlier, while the permission is still there.

**Release N (the migration release): the permission stays, the data moves.**
- [ ] Tasks 1, 3, 4, 5 and 7 ship in N; Task 2 (removing the permission) does not.
- [ ] When Task 1's gate says not migrated: a background copy of `LEGACY_ANDROID_ROOT` (saves,
  settings, hub, translate, editor maps, log; everything in it) into `<files>/Razdor`, started on
  the title screen, with a progress line there (the existing unpack progress UI); starting or
  loading a game waits until it is done. The paths stay legacy meanwhile (Task 1). Before copying, compare the total size with free space (`statvfs`) and show what is needed
  when short; nothing is switched then.
- [ ] The game: copy `DiscordTimes/` into `unpack_dir()` the same way (large: progress, space
  check), or, when space is short, skip it and let the not-found screen ask for the archive
  (Task 3). The player's choice is not needed: copy when it fits, otherwise ask for the archive.
- [ ] Copy, never move: write into `<dest>.part`, compare every file's size (and the count) with
  the source, then rename into place and write the marker `app_dir()/migrated-from-shared`
  (the date and the source paths), then `platform::set_android_migrated()`, rewrite the
  remembered install path (`dt::install`'s remembered folder in the copied config) from
  `LEGACY_ANDROID_GAME` to the new `unpack_dir()` when it pointed there, and
  `diag::reopen_log()`. A failed or interrupted copy removes `.part` and retries next start;
  the paths stay legacy.
- [ ] After a successful copy, one message: «Файлы Раздора перенесены в папку приложения. Старые
  папки Razdor и DiscordTimes можно удалить.» Never delete the legacy folders automatically.
- [ ] Permission not granted in N (a player who never granted it): nothing to copy by path; the
  app's folder is used from the start and the fallback below applies.

**Release N+1 or later (2–3 weekly releases after N, the owner decides when, looking at the
download stats from `tools/stats/downloads.py`): the permission goes.**
- [ ] Task 2 ships: the manifest drops every storage permission. The by-path copy code stays
  (harmless: the legacy root is simply not readable).
- [ ] Fallback for players who skipped N (e.g. 1.3.x → N+1) or whose copy did not finish: when
  the app's folder has no marker and no saves, and the legacy root is not readable, show one
  screen: «Раньше Раздор хранил файлы в папке Razdor во внутренней памяти. Выберите её, чтобы
  перенести сохранения.» with «Выбрать старую папку Razdor…» (`ACTION_OPEN_DOCUMENT_TREE`;
  Android 11+ refuses only the storage root, `Download` and `Android/data`, a top-level `Razdor`
  can be picked), «Выбрать архив игры…» (Task 3) and «Пропустить». The tree is copied through
  `DocumentsContract` queries (no AndroidX), with the same `.part` / size check / marker as above.
  «Пропустить» writes the marker so the screen does not return; the saves screen keeps
  «Импорт сохранения…» for later.

**Tests**
- [ ] Pure Rust with a fake legacy root and a fake app folder: a full copy writes the marker and
  switches the paths; an interrupted copy (simulated failure mid-way) leaves no marker and the
  legacy paths in use; a size mismatch is refused; the space check refuses when short; a second
  start with the marker copies nothing; the legacy folders are untouched after every case.
- [ ] Device-only (Task 8): the N upgrade from 1.4.x with the permission granted; N+1 straight
  from 1.4.x (tree picker); N+1 after N (nothing shown).

### Task 7: Docs

Release N wording: the files move into the app's folder and the game comes in through the
picker; the permission is still asked of nobody new but stays for the move; it goes in a later
release. Don't write "no storage permission" yet.

- [ ] `CHANGELOG.md` (ru): no storage permission; where files live now; export/import buttons; the
  migration.
- [ ] Wiki ru + en: the Android install page (pick the archive instead of copying folders; files
  are in the app; uninstall deletes them, so export saves first).
- [ ] `docs/flows/` for install and saves (whichever cover `platform`, `dt::unpack`, `ui::mobile`).
- [ ] Remove the "all-files" wording from `src/ui/mobile.rs` and `src/ui/android.rs` module docs.

### Task 8: Device check before the push

Release N (permission still in the manifest):
- [ ] Update from 1.4.x with the game and saves in the legacy folders and the permission granted:
  the copy runs, the marker is written, saves load from the app's folder, the legacy folders are
  untouched. Interrupt it once (kill the app mid-copy): the next start retries.
- [ ] Fresh install: the game comes in through the picker (7z, then the Community Update zip); play;
  save; export saves; «Обновить игру из архива…»; map import/export.
- [ ] Owner's yes, then push; release N.

Release N+1 (permission dropped), weeks later:
- [ ] Fresh install on Android 14+ and on Android 9 or 10 (or emulators): no permission prompt at
  all; uninstall (keep data, Android 10+) and reinstall: saves still there.
- [ ] Update N → N+1: nothing shown, everything in place.
- [ ] Update 1.4.x → N+1 directly: the tree-picker screen; pick `Razdor`; saves copied.
- [ ] Owner's yes, then push; release N+1.

## Review Focus

1. **The fd's lifetime and seekability:** the detached fd is owned by Rust's `File` (closed on
   drop, never twice); a non-seekable provider falls back to the cache copy instead of failing.
2. **Space:** unpacking a large game into the app's folder must check free space first; a failed
   unpack removes partial files (as `dt::game_import` does with `<install>.new`).
3. **No data loss in migration:** copy, never move; verify sizes before saying "done"; the legacy
   folders stay.
4. **Uninstall wipes the app's folder:** the UI must make export easy and the changelog must say it.
5. **Desktop and iOS unchanged:** `cfg(target_os = "android")` around every new branch; their
   tests still pass.

## The owner's decisions (2026-11-09)

- App folder only: no storage permission of any kind; no opt-in that keeps the shared folders.
  Saves live in the app's folder, with export/import through the picker. No auto-find of an
  archive in `Download` (the picker only).
- Added: `android:hasFragileUserData="true"` on the application (Task 2), so on Android 10+
  uninstalling asks whether to keep the app's data: the game and saves survive a reinstall when
  the player says yes. Mention it on the wiki page and in the changelog line.
- UI (the not-found screen, the export/import buttons, the migration screen): off-screen
  screenshots (desktop unchanged, phones) go to the owner before the push, with the device check.
- Order: start after the refresh-rate change to `android/MainActivityInject.java` is on `main`.
- Migration in two releases (Task 6): N keeps the permission and copies the data into the app's
  folder; a later release (2–3 weeks on, the owner's call) drops the permission, with the
  folder-picker fallback for players who skipped N.

## Open questions for the owner (answered above, kept for the record)

- Shared vs app storage for saves: keeping saves in the app's folder means uninstall deletes them.
  Acceptable with an export button, or should saves be written through a picker-chosen folder
  (`ACTION_OPEN_DOCUMENT_TREE` with a persisted grant) instead?
- Should the not-found screen still auto-find an archive the player put in `Download`? That needs
  the permission, so the plan says no (picker only).
- Play Store release planned? If yes, this change is required anyway.
