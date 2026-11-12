# Flow: the app, its screens and windows

**Read this first.** Razdor is one macroquad program (`src/main.rs`) over a library
(`src/lib.rs`: `dt`, `rules`, `difftest`, `editor`, …). The library holds every rule and
decoder and has no macroquad; `src/ui/` is the only layer that draws or reads input. One
`App` (`src/ui/mod.rs`) owns the game and a single `Screen` enum value; every frame
`App::frame` runs the current screen's function, which draws *and* handles input
immediate-mode style and returns `Option<Screen>` for the next one. Modal message windows
are a queue (`App::dialogs`) drawn over the screen. Nothing is retained between frames
except what the screen value, the `App` fields and a few `thread_local!`s hold.

## Startup

`src/main.rs`:

1. `conf` (before the window exists): `razdor::difftest::cli` (the headless `--replay`
   mode exits here), `razdor::diag::init` (the log `razdor.log`),
   `razdor::dt::install::load_dotenv` (`.env` next to the program or in the working
   folder), `single_instance` (a lock file), `razdor::update::init`, the window size
   (`ui::snapshot::size`, `RAZDOR_SIZE`).
2. `main`: one black frame (`next_frame` before any loading, so the shown window handles its
   first messages and presents a picture at once; the loading itself runs inside one frame),
   `ui::widgets::load_font` (TrueType faces bundled in `data/fonts/`),
   `ui::language::init` (the saved language, `razdor::i18n`), `Content::builtin` (the demo content),
   `Assets::load` (`src/ui/assets.rs`) which finds the install (`DtArt::from_env` →
   `razdor::dt::install::locate`) and, when there is one, calls
   `ui::language::set_install` (the install's data translations join the language list),
   `ui::chrome::set_install` and `DtArt::preload_all` (start decoding the install's pictures ahead,
   see [assets-and-drawing.md](assets-and-drawing.md)), `razdor::rules::save::set_install` and
   `razdor::rules::save::warm_meta_cache` (see [saves.md](saves.md)).
3. `App::new` (`src/ui/mod.rs`): the install's `Content` (`Content::from_dt`), every map of
   the install parsed into `ScenarioEntry`s, `Audio::new` (settings are loaded here).
4. `ui::snapshot::stage` (only with `RAZDOR_SCENE`), then the loop (each iteration first refreshes `ui::frame_state`, the window's size and DPI, which the windows and the map read instead of asking the window): `App::frame`,
   `App::draw_fps`, `ui::cursor::draw` (the pointer is drawn last, over everything), and
   `next_frame().await`. Quitting goes through `exit_now` (it skips C `atexit` handlers,
   which crashed on exit). Every quit (the main menu's «Выход», the game's «Выйти из игры»,
   the battle's leave window, the window's close button) sets the quit flag, and `exit_now`
   does, in order: on Android, `ui::display::hide_for_exit` first (the activity's
   `razdorHideForExit` moves the app to the background at once, on its UI thread, so the slow
   writes below happen off the screen; the activity's pause then flushes the session slot, which
   writes nothing already written); the frame probe's flush, the pending autosave's join
   (`finish_pending_writes`), `App::shutdown` (the session end, the audio's stop and settings),
   then on X11 `ui::display::hide_for_exit` (the window is unmapped at once, so the memory the
   kernel frees before the X connection closes is freed off the screen), the update restart,
   and the process ends at once (no teardown of the C libraries). Nothing after `App::shutdown` writes a save. Under `RAZDOR_PROFILE` each step
   is logged as `exit timing` (on Android its hide is logged as ~0 ms: it ran before the timer).

Install lookup order (`src/dt/install.rs` `locate_with`): `RAZDOR_DT_DIR`, the program's own
folder when it is an install, the remembered folder (`config_file`), then a search under
`search_roots`. Without an install Razdor runs the built-in demo.

Other builds of the game: the Community Update Evolution Mod (`docs/reference/original-mechanics/evolution-mod.md`)
loads with the same readers (`DtInstall::load`); its data differs in counts only, and its
four option rows are `Option0`…`Option3`, not `OptionSld0`…`OptionSld4`, with no battle-animation
row (`ui/main_menu.rs` `options_window`). Its tests run with `RAZDOR_EVO_DIR`
(`dt::install::evolution_mod`).

## Android and iOS (experimental)

iOS mirrors Android, with no permission. `scripts/dist-ios.sh` (on a Mac; the `ios` job of
`.github/workflows/release.yml`) builds `dist/razdor.ipa`: an `aarch64-apple-ios` binary in a
`Payload/Razdor.app` bundle whose Info.plist is written by the script, ad-hoc signed only so the
binary loads; the player's AltStore or Sideloadly re-signs it. The app icons (AppIcon60x60, 76x76,
83.5x83.5 in the bundle, listed in `CFBundleIcons` of the Info.plist) are resized from
`data/icon/razdor.png` with `sips` by the same script. Paths: `platform::app_dir` and
`config_dir` are `Documents/Razdor` (the app's sandbox `$HOME/Documents`), and `install_roots` are
`Documents/DiscordTimes` then `Documents`. `UIFileSharingEnabled` makes that folder show in the Files
app as «На iPhone → Раздор», so the player copies the game there; no permission is asked, and
`platform::MOBILE` (Android or iOS) is the one flag the UI reads for «a phone». The not-found block
(`screens::ios_not_found`: the copy instructions and «Check again», the Android block's layout) and
the poll (`App::mobile_install_poll`, `ui::mobile`: the `RECHECK` / `NEXT_POLL` helpers, the 5-s look for the install) are shared with Android; the JNI parts
stay in `ui::android` (the shared helpers in `src/jni.rs`). The on-screen keyboard is on for both (`widgets::set_keyboard`). The
update window (`ui/update_view.rs`) offers a release on the phones too: Android installs it through the system
installer (`update::poll_installer` takes its report each frame: a cancelled session brings the offer back, a failed one shows why; the APK is kept as `razdor-<X.Y.Z>.apk` in the cache folder, reused by the next try, removed after a successful install and at start when it is not newer than the running version), iOS opens the release page. On a phone (`display::dp_px`) it keeps the desktop's layout, one unit one dp, fitted to the screen less its safe area and the bottom bar, with a little larger text (`NotesStyle::phone`, about 14 sp) and taller buttons in the same row (`button_rects`, 40 dp at Normal); the desktop window is unchanged. The Discord link opens through `ui::mobile::open_url` (see the main menu above). `ui/script_font.rs` skips the fontconfig lookup on iOS, as on Android.

The APK is built by `scripts/dist-android.sh` (cargo-quad-apk, pinned in its header; NDK 29;
see `[package.metadata.android]` in `Cargo.toml`). The launcher icon is `android/res/mipmap-*/ic_launcher.png`
(Razdor's own icon, `data/icon/razdor.png` resized to 48–192 px; `res` and `icon` in `Cargo.toml`).
Per-platform paths live in one place:
`src/platform.rs` (`app_dir` for saves, log, editor maps, translation jobs; `cache_dir` for the picture cache;
`config_dir` for the remembered install; `install_roots` for the search). On Android they are all in the app's own
folder and nowhere else: `platform::android_paths` gives `<files>/Razdor` (saves, the log, `app_dir`, the hub) and
`<files>/DiscordTimes` (the game, `unpack_dir`), where `files` is `getExternalFilesDir(null)` (`ui::android::files_dir`,
`Android/data/io.github.indicozy.razdor/files`). `set_android_files_dir` stores it once, in `main`. Desktop keeps the
platform's data and config folders.

No storage permission: the manifest declares none (`Cargo.toml`), and no screen asks for one. The game's downloads and
imports (the automatic start, «Скачать игру», the picker, the repair) write to `unpack_dir()`, so they need nothing, and
a fresh install goes straight from the menu art to «Проверяем каталог…», the download bar and the menu. (Before this
release the automatic import wrote to the shared `DiscordTimes`, which needed the all-files grant.)

The saves of an older release (Android, `ui::saves_move`, `transfer::import_folder`): older releases kept Razdor's folder
in the shared storage, which the app cannot read now. The player picks that folder with the system's folder picker:
`ui::android::start_old_saves_pick` opens `razdorPickOldSaves` (`ACTION_OPEN_DOCUMENT_TREE`, `EXTRA_INITIAL_URI` at
`primary:Razdor`). The Java side copies the picked folder's `saves` folder with its `settings.json` and its `hub` folder (the installed maps and
mods; or the folder itself, when it holds `settings.json` or `manual`) into the cache's `old-saves` (`razdorCopyOldSaves`, at most 512 MB, depth 6; any other
folder answers «wrong») and the poll `razdorOldSavesOutcome` says `copied`. Then `transfer::import_folder` merges the copy
into the app's saves folder with the saves import's rules: an identical save is skipped, a differing one gets the next
number, a settings file that is there is kept, pins are merged; files the saves folder does not hold are ignored. The copy's `hub` folder merges into the app's hub
(`hub::import_old`): each entry of its `installed.json` comes in with its files when the local hub does not have that id and
no file of it exists locally (no duplicate entries, no overwritten file).
The popup «Сохранения старой версии» is offered once per run on the main menu (`saves_move::offer_if_due(loaded)`: only once a
usable install is loaded, never over the no-install screen or a download) while the
decision says existing. `saves_move::decide_at_start` (in `main`, before the threads write to the cache) takes it: files
in the private cache (`getCacheDir`, where older releases kept their picture cache and update files) mean an older
install, an empty one a fresh install. The decision is kept in `<files>/Razdor/saves-move` ("existing", "fresh", "done").
«Не сейчас» leaves the popup for the next start; a successful import writes "done", so it never comes back. The
settings' «Перенести сохранения из старой папки Razdor…» (Прочее, Android only, `Row::OldSaves`) is always there.
Blind spot: an older install whose private cache was cleared (Android's «Clear cache», low storage) reads as fresh, so
no popup; the settings row still works.

The scenario-select screen's not-found block
(`screens::android_not_found`, scaled to the screen) offers only «Проверить снова» and points to the main menu.
The main menu shows its not-found block while no install is found (`screens::menu_not_found`, drawn by
`main_menu::frame(no_install)`, whose buttons come from `screens::not_found_rows`: «Скачать игру», the archive picker
«Выбрать архив…» and «Проверить снова»;
in the column left of the buttons, stacked, over a dark panel, at the interface scale; the column is the
left half of the screen, in either orientation). The block is the same on the desktop (a Razdor extra, not
the original's): it says that Razdor plays the player's own copy of the game (1.8 with Community Update
1.2), gives «Скачать игру» (`game_import::request_download`: `gdrive::download_tree` into the staging folder beside the target, then `dt::game_import::import_folder`, the archive path's install check and swap; on a thread on the import's `Job`, with a progress window and Cancel; a failure is one line on the block, `game_import::download_failure`). On a failure, or with no download (iPhone), the manual path: the folder's link as text, «Открыть ссылку» and «Копировать ссылку» (`gdrive::folder_page`), and the steps by hand (one .zip, then the importer's button: «Обновить игру из архива…» on the desktop, «Выбрать архив…» on the phones). «Как установить» (`main_menu::install_guide`) and the secondary link «Помощь в Discord» (`main_menu::DISCORD_INVITE`).
Both links, and the bottom «Discord» link, go through `main_menu::open_link`: `xdg-open` / `explorer` / `open`
on desktop, `ui::android::open_url` (an `ACTION_VIEW` intent) on Android, `ui::mobile::open_url`
(`UIApplication openURL:`) on iOS. The two link buttons share a row when the stack does not fit the height.
While it
is shown, `App::mobile_install_poll` (`ui::mobile`; on the main menu and the scenario list; every 5 s, and on the button) loads a newly found install with `Assets::adopt_install` and the `ui/mod.rs` helpers
`dt_content_of` / `scenarios_of`, without a restart. Its Discord link opens through `open_url` like the desktop's (see the main menu above). A log that could not be opened is opened again by the poll (`diag::reopen_log`).
Game archives on the not-found block (`dt::unpack`, `ui::mobile`, every platform): `look_for_archives`
scans `platform::archive_dirs()` (the phones: the app's game folder only; the desktop:
the program's folder except on macOS, Downloads, the install folders) at once after «Check again» or the
unpacking's end, else every 5 s. `unpack::find_archives` lists each `.7z` / `.zip` there without extracting:
a game when the listing passes `install::listing_is_install` after dropping one top folder, a patch when it
holds some of `install::is_install_path`'s files, else ignored. `not_found_rows` offers «Распаковать архив
игры» while a game is found and nothing runs; `start_unpack` writes the archives on a thread into
`platform::unpack_dir()` (Android `<files>/DiscordTimes`, iOS `Documents/DiscordTimes`, macOS
`DiscordTimes` in `app_dir`, elsewhere next to the program), the game first, the patches over it. While it
runs, `mobile_install_poll` does not load the install; its end (`unpack_tick`) asks for it (`ask_recheck`),
and the same poll loads it (the desktop has no 5-s poll: that reload is its only one). Snapshot fakes:
`RAZDOR_SCENE_ARCHIVES`, `RAZDOR_SCENE_UNPACK`.

Saves in and out (Android only; `ui::transfer`, `transfer` in the library). On Android the Load window's
button is «Перенос…» (`saves::folder_label`, `saves::folder_pressed` → `transfer::open_menu`): the app's folder is
`Android/data/...`, which the Files app cannot open, so the menu goes through the system's documents. «Экспорт
сохранений» (`transfer::export_saves`) writes one zip into the cache and hands it to the document creator
(`android::start_document_export`). «Импорт сохранения» (`android::start_pick`) takes a `.zip` through
`transfer::import_archive` or a `.rzsave` through `transfer::import_save`: nothing is overwritten, an identical save
is skipped, a different one gets the next number, a root json file (the settings) is kept, and pins are merged.
«Экспорт журнала» hands over the diagnostic log for a bug report. Desktop and iOS keep «Папка»
(`saves::open_folder_notice`). The manifest sets `android:hasFragileUserData` (`Cargo.toml`), so Android 10 and later
asks on uninstall whether to keep the app's data: the saves are in that folder, so export them first or keep the data.

The game download (`gdrive`, `ui/game_import.rs` `start_download`, the no-install block's «Скачать игру»): the community's one archive on Google Drive (`gdrive::GAME`: the file id, size and SHA-256, the owner's constants) is downloaded into `platform::cache_dir()/game-download.zip` on a thread (`gdrive::download_game`: `update::download_watched` with a Range request to continue a `.part` copy, the warning page's form followed, a quota page or 403/429 an error with its reason; a file that does not verify (size or SHA-256) is refused as «the archive on Drive changed», a resumed one first downloaded again from the start). A verified archive is kept, so a retry does not download it again. Then `dt::game_import::examine` and `import` (the same path as «Обновить игру из архива…», below) install it and the archive is removed. The window shows the megabytes (`download_window`, Cancel keeps the partial copy), then the unpacking. A failure is one line on the screen and the manual path (`screens::manual_steps`: the file's page, «Открыть ссылку», «Копировать ссылку»). The picker «Выбрать архив игры…» (`game_import::request`, the desktop dialog or Android's document picker; none on iPhone) is the secondary option, on the same screen and in the failure state: it goes through the same `examine` and `import`. Snapshot fakes: `RAZDOR_SCENE_DOWNLOADING=<percent>`, `RAZDOR_SCENE_DOWNLOAD_FAILED`. The automatic start (desktop and Android, not iPhone; `game_import::auto_tick`, each frame from `tick`): with no usable install loaded (`tick(install, loaded)`, `App` passes `loaded = assets.dt.is_some()`: a located folder that did not load, broken or unreadable, counts as none and never blocks it; the folder written to is then the unpack folder) and no `RAZDOR_DT_DIR` set by hand (not on a repair or a snapshot), the hub's catalog is read once per launch on a thread (`ui::launch::start`, read by `game_import::auto_tick` through `launch::catalog`; `hub::Hub::fetch_catalog`, the same as the hub window; the menu shows «Checking the catalog…» in the block's place, `screens::checking_panel`). A catalog with a `game` (`hub::GameRef`: an https address or a path in the hub, its size, SHA-256 and a file name, checked by `Catalog::checked`) starts the same download at once (`start_archive` with `auto`, the archive kept at its address as `gdrive::Archive::at_url`, no Drive pages), with the same window and Cancel (a cancel keeps the partial copy for the button). Its end shows no notice: the install is reloaded and the main menu opens. No game in the catalog, no catalog (offline, an error), and a failed download, verify or import leave the prompt as it is (a failure is its one line). Once per launch. Snapshot fakes: `RAZDOR_SCENE_CATALOG=checking`, `RAZDOR_SCENE_UNPACKING=<percent>`.

The game's integrity (`dt/integrity.rs`, the manifest `data/game-manifest.json` made from the archive by `scripts/game-manifest.py`: paths, sizes and SHA-256, no content). `DtInstall::from_env` checks the base install before anything is read (`install::guard_game_files`, sizes only, cheap): a Community Update build (its exe is the manifest's, or most files are there at the archive's sizes) with MISSING files stops with `DtError::MissingGameFiles` and `Problem::Missing`, and the no-install screen shows «Не хватает файлов игры: …» with «Восстановить файлы игры» (`game_import::start_repair`: the archive is got as above, then `integrity::repair` writes only the missing files, each checked before its rename; the install reloads). A CHANGED file (a size that differs; a full hash check is `integrity::check(.., true)`, used by the e2e test) is only logged and kept: the repair never overwrites it. Another build (Evolution's own exe, an old install) is skipped and logged. Extra files (translations, user maps, saves) are ignored.

Updating the game from an archive («Обновить игру из архива…», the settings window of the main menu,
`ui/game_import.rs`, the pure part `dt/game_import.rs`). `main_menu::options_window` draws the row
through `game_import::settings_row`: the button, or why it is not offered (over a map, on iPhone, or
when the install is not the phones' unpack folder: `game_import::target` returns it only for that
folder or with no install yet, so a player's own copy is never written). The button sets a request;
`game_import::tick` (in `frame_content`, every frame) opens the platform's picker (Android: `razdorPickArchive`,
whose answer comes through `pick_outcome` as `fd\n<name>\n<fd>`: the descriptor is detached in Java and Rust owns it;
`is_seekable` decides: a seekable file is read in place (`Archive::Open`), a stream is copied into the cache on a
thread (`Archive::Copy`, deleted when the import ends). The map picker «Добавить карту…» still copies). The picked
archive is `dt::game_import::examine` (a path) or `examine_file` (an open file: `unpack::inspect_file`), listing only,
the budget `over_budget`: nothing of the game in it is a refusal, nothing written. The declared sizes are not trusted alone: `unpack::unpack` caps the bytes
written at the declared total or `MAX_UNPACKED_BYTES`, and `unpack::write_file` reads each entry no further
than its declared size, so a zip bomb stops with an error (the side folder is removed, the install kept). A whole game asks first (`Modal::Confirm`) when the install is
there; a patch goes straight in, and is refused without an install. `dt::game_import::import` (or `import_file` for an open file, `unpack::unpack_file`) first checks the room
(`check_space`: the archive's uncompressed total plus `SPACE_MARGIN`, 64 MB, against `free_space`, statvfs on the
folder's disk; a room that cannot be read is not refused): short, it is refused as «Not enough free space…» with
nothing written. It unpacks
into `<install>.new` (beside the install, the same disk), then `swap_in` renames the install to
`<install>.old`, the new folder to the install's name, and deletes the old one (a failed second rename
puts it back); a patch's files are moved over the install's (`move_over`). Any failure removes
`<install>.new` and leaves the install as it was. The job runs on a thread (`Job`, the progress
window); when it ends, `tick` returns true and `App::reload_install` reads the install again
(`preload::forget_ready` first: the decode store is keyed by path alone, and its loads started
before the call are not kept, the generation). The audio follows the install too (`Audio::set_install`,
also after a mod switch in `apply_mod` and the phones' poll): a folder other than the one its sounds came
from reads the effects and music again, the player's settings stay, and the menu's music starts on the next frame. The saves (`platform::app_dir`) are never beside
the install, so they are not read or written. `dt::game_import::recover` (`main`, before the install
is looked for) puts back an install an interrupted swap moved away. While the import's windows are up
the settings under them are only drawn (`main_menu::backdrop`, `game_import::busy`). Snapshot fake:
`RAZDOR_SCENE_IMPORT` (with `RAZDOR_SNAPSHOT`) shows the row as offered.

The first screen without a game (`ui/menu_art.rs`): with no install loaded, `main` gives the window art to the folder `menu-art/` (`RAZDOR_MENU_ART_DIR` alone when set, else next to the program, else `Resources/menu-art/` in a macOS bundle; Android: the APK's `assets` are copied once into the app's cache at the first run, `extract_from_apk`). The folder holds only the first screen's files (`FILES`: the title, its tiles and flames, the loading window's frames, the bitmap fonts), at their paths in the game (`Graphics/Windows/…`), and is never mixed with an install: the install replaces it when it loads (`announce_install`). The files come from `menu-art.zip`, made from the unpacked archive by the release scripts: `scripts/fetch-menu-art.sh` checks the zip against the size and SHA-256 pinned in `scripts/menu-art.env`, unpacks it into `dist/menu-art/` (desktop, macOS) and `menu-art/` (the APK, `assets` in Cargo.toml). The hub catalog's ad (below) is the only other thing that reaches the title.

The hub catalog is read once per launch (`ui/launch.rs`: a thread from the first frame, not in a snapshot), and that one result serves the game check with no install and the promo ad. The catalog's optional `ad` (`hub::AdRef`: id, version, picture with its size and SHA-256, link, captions, validated like `game`) is kept in the app folder (`platform::app_dir()/ads/<id>/`, `Hub::install_ad`, run each frame by `ads::sync_tick`): a copy whose `ad.ini` differs from what the catalog says (id, version, picture or text) is downloaded, checked and written (the picture first, `ad.ini` last, the other ad folders removed), then `ads::forget` makes the menu look again. Offline, no `ad` in the catalog or a failed download keep what is on disk. The promo's lookup order (`ads::ad_roots`): `RAZDOR_ADS_DIR` alone when set, else the app copy, `ads/` next to the program, and the install's `ads/`.

Debug mode and the log (Settings → «Прочее»). `audio::Settings::debug_mode` («Режим отладки», off by default) is read each frame
by the app (`diag::set_debug`): `diag::profiling()` is on while it is, next to `RAZDOR_PROFILE` and the `profile` file, so the
slow-frame log and the frame probe follow it at once (the main loop reads `profiling()` every frame). While it is on,
`ui::debug_banner::draw` draws a strip «Включён режим отладки — FPS может снизиться» at the top of every screen, below the safe
area; it is drawn last, before the pointer, and takes no input. «Отправить журнал…» (`main_menu::send_log_row`): on Android it
starts the same export as the Load window's «Перенос…» (`transfer::start_log_export`): one text file (`transfer::write_log_bundle`:
`razdor.log`, then `razdor-play.log` and the newest `razdor-bench-*.txt`, each under a heading) goes to the document creator; the
answer comes back through `transfer::settings_poll` and a note window. On the desktop it opens the log's folder (`open_folder`).

Thread locals: `local!` (`src/local.rs`), never std's `thread_local!`: std takes one TLS key per static
on Android, where a process has about 128, and the army window aborted with «out of TLS keys»; one
test fails if `thread_local!` comes back in `src/`. Touch (phones and tablets,
`touchpad::MOBILE`): two modes, with miniquad's simulated mouse off (`simulate_mouse_with_touch(false)`,
once). The default is the gestures (`src/ui/gestures.rs`, `GestureMapper`): the finger is the
pointer, a tap clicks where it lifts, a double tap clicks at the first tap's place, a dragged
finger holds the left button, a long press (still for `input::HINT_DELAY_SECS`, 0.35 s, the one hint
delay) holds the right one, two fingers pinch the wheel
(the map's zoom) or slide (the map's scroll, or the list's wheel). A long press keeps the pointer
where the press began while the right button is held (`GestureMapper::press_at`): the finger may
slide to uncover the view, and the info window (army, building) stays put; the lift releases the
button there too. Hints: `input::hint_hover()` gates every hover hint (`widgets::tooltip`, `widgets::tooltip_later`, the bar's hint, the map's tooltips), with the 0.35 s delay. Item descriptions use `input::item_hint_hover()` instead, at once with no delay (the owner's decision, 2026-10-31): the army and hero windows' pack and worn slots. The event and victory windows' item and spell icons (`dialog`) use `dialog::icon_hover()`: the same, but only in a phone's touch modes; on the desktop they show while a button is held, as the original. The card is drawn last, over the window's buttons, which are always drawn (a player's report, 2026-11-01: the card used to end the frame, and the OK button vanished on hover); a button the card covers is not pressable while the pointer is on the card (`dialog::tip_covers`). In gesture mode an item's description shows while a finger is down on it (no long press is needed there), in the touchpad mode while the cursor is over it, else as `touch_hover`. The other hints in gesture mode in the touchpad mode they show while the cursor rests (`rest::rested`), and
with no touch, as `input::touch_hover` (no hint after a lift until a finger comes down). `input::HINT_DELAY_SECS`
(0.35 s) is the one delay for both the long press and the rest. Swipe areas: each frame the screens register the rectangles a one-finger swipe acts on
(`input::swipe_area(rect, SwipeKind)`, gestures only; `widgets::swipe_rows` for a list or text box with
its row height). The map registers `Pan` over the map view (not while a dialog is open; the open minimap
is `Off`, a blocker that keeps its drag a drag). The registrations of a frame are the next frame's
(`input::begin_frame` swaps them), the last one registered over the touch acts. Item cells and worn slots that hold an item register `SwipeKind::Pull` (`pan::pull_source`; a cell's row height for the pack, none for a worn slot): a finger held still on one holds no long press, a slide pulls the item out of its cell (`item_pull`: released over a pack cell, a worn slot or a unit's card, the item is put, swapped or given there as a click would; released on its own cell it is put back), and a quick mostly vertical slide over the pack scrolls it. A plain tap or press picks the item up, as the original's click (`clicked() || item_pull::dropped()` in `items_view`). Drag sources: the unit grids (the army window, the building's army and garrison, the hire tab's hero grid) register their cards as `Off` (`pan::drag_grid`, `pan::drag_source`) while the canvas overflows and units drag, so a quick slide from a card drags the unit instead of panning the window (the window's own `Pan` area is registered first). A finger that moves past
the slop within 0.25 s of touching down, on a `Pan` or `ScrollRows` area, is a swipe: no click, the map's
`Frame::pan` (the finger's slide, applied 1:1 by `world_view::pan` on the idle map), or the list's wheel
steps (one per row, finger up = content up). A finger held still longer first, or outside any area, is
the held drag as above; taps, double taps and the long press are unchanged. The touchpad mode is an option (`Settings::touchpad`, off by default; below). Gamepads are off
(`--no-default-features --features audio`).
The screen turns with the device (Android `screenOrientation = fullUser` in `Cargo.toml`,
following the rotation lock; iOS portrait and landscape in `scripts/dist-ios.sh`'s Info.plist); a
turn is a resize, and the interface scale fits the new width and height (`chrome::fit_k`).
On a phone a turn also moves the pointer to the middle of the new screen (`input::cursor_after_resize`,
run by `input::begin_frame` when the size changed; the pointer is then the pad's, so a touch or a
mouse move takes it back). Without this a stale pointer in the new edge zone kept scrolling the map
(report 1316, Razdor's extra). On the desktop a resize never moves the mouse.

Finger-sized hit areas (phones only, a Razdor choice): `widgets::button_state` tests the pointer against
`widgets::finger_hit(r)`, not the drawn `r`: each side grown to `FINGER_DP` (48 dp) where there is room.
Room ends at its window (the smallest window drawn last frame that holds it, `chrome::note_window`, so a button
at a window's edge never grows onto the map or another window), at the screen, at the bottom bar's top (a target on the bar stays on it, one above stays above),
and at the midline to every other rectangle that `button_state`, `tapped_in` or `mouse_in` tested in the
last frame (`finger_frame`, swapped in `input::begin_frame`) and that does not overlap the target, so
neighbours never share a hit area and controls with their own pointer test keep their taps; space blocked
on one side goes to the other. The touch probe records this hit area. The desktop uses the drawn rectangles.
The orientation setting (Android only; the options window's «Ориентация экрана» row, the last one
on Android, `Settings::orientation: Option<audio::Orientation>`, `None` = Auto): Auto keeps the
manifest's `fullUser`, Landscape and Portrait lock the activity. `ui::android::follow_orientation`
runs from `Ui::frame` and calls `Activity.setRequestedOrientation(int)` over JNI with
`ActivityInfo`'s 13 (full user), 6 (sensor landscape) and 7 (sensor portrait) when the value
changes, so it applies at start and on each change. The last value asked is cached
(`audio::must_request`), so a frame asks again only after a change or after the activity resumed:
the cache is cleared then (`android::resumed_since_last`, its own input subscriber read each frame,
since miniquad reports the Resume as `window_restored_event` and the pointer's watch does not run on
a phone). Without it a minimised app came back in the system's orientation, not the setting's
(a phone choice, a player's report). iOS has no row (its rotation lock stays the system's).
The on-screen keyboard follows the text fields: `edit_line` / `edit_rows` drawn active ask for it
(`edit_line_kb` with `keyboard` false draws without asking). The save window's name and the new
hero's name are always active, so on a phone they ask only after a tap in the field
(`widgets::name_keyboard` decides, `widgets::tap_rect` is the 48 dp tap target; desktop always
asks); a window's opening does not show the keyboard. While the keyboard is up, the window that holds
the name field moves above it (`window_layout::keyboard_lifted`, the hero and save windows): the height
is `display::keyboard_inset`, the IME inset (`WindowInsets.Type.ime()`, API 30+) that
`mobile::refresh_keyboard_inset` reads every frame on Android; it is 0 on desktop and iOS (no reading
there yet: the window stays put). A window whose bottom is under the keyboard moves up to its top edge
(never above the safe area); a window taller than the space left moves so its field is at the top.
A keyboard the system hides (its own arrow, Back: the inset drops from above 0 to 0,
`display::keyboard_closed_by_system`) ends the field's typing (`NameTouch::Hidden`), so the strip goes and the
window returns. On a phone, Enter with that keyboard up is the keyboard's own key: `widgets::name_enter` clears the
field's flag (the keyboard goes) and Enter is not the window's OK (the hero's Start, the save's Save); with the keyboard
down, Enter is OK as on the desktop. Then `widgets::sync_keyboard` (once a frame, before `next_frame`) shows or hides it with miniquad's
`show_keyboard`. miniquad's `MainActivity` swallows every key, so `quad.toml` at the repo root
injects `android/MainActivityInject.java`: the volume keys change the media volume there, and on
API 30+ its `RESIZING_LAYOUT_ON_APPLY_WINDOW_INSETS` section pads by the system bars only, so the
keyboard floats over the full-size game instead of squashing it. On mobile, `widgets::draw_keyboard_strip`
draws the line being typed in a dark strip across the top (the keyboard may cover the field); iOS
needs the strip too, since UIKit does not resize the view.

Safe area (a player's report, 0.9.3: text under the rounded corners and the cutout). `platform::safe_insets()`
returns the margins (left, top, right, bottom) in screen pixels, zeros on desktop; `platform::set_safe_insets`
stores them. On Android `ui::android::read_safe_insets` reads the decor view's `getRootWindowInsets()`:
`getInsets(systemBars | displayCutout)` on API 30 and later, the system window insets before that, and each
side is at least 24 dp (`platform::safe_margins`: the rounded corners are not reported by every system).
On iOS `platform::ios_safe_margins` gives 44 points at the top and bottom in portrait, at the sides in
landscape. The area also holds the radii of the four rounded corners (`display::SafeArea`: `corner_bl`, `corner_br`,
`corner_tl`, `corner_tr`, physical px; Android reads `getRoundedCorner` at positions 0–3, so the landscape's top
corners are round too; a corner the system does not report takes 16 dp). `RAZDOR_SAFE_AREA="l,t,r,b,bl,br[,tl,tr]"`
overrides it for snapshots. `game_bar::on_screen` and `rect_on_screen` test a point or a rectangle against those
corners. The bottom bar's stat line (mana, gold, income, wages) moves each item inward by the least shift that keeps
its label's start and its value's end off the bottom corners (`game_bar::stat_centres`, a phone choice; the
original has no corners). `mobile::refresh_safe_insets` (once a frame in `App::frame`) reads them when the screen turns and
every 30 frames, since the insets settle after a turn; a turn the system has not laid out is read again on
the next frame. The keyboard strip (`widgets::draw_keyboard_strip`) and the FPS counter (`App::draw_fps`) all sit inside it.

Portrait (a phone taller than 4:3): the width sets the interface scale, so the 960×720 canvas is shorter
than the screen. `chrome::centred_top` centres it vertically, only on screens narrower than 4:3 (a 4:3 or
wider screen, the desktop's and the landscape phone's, gets 0 and is never moved). The title screens are
placed from `chrome::canvas_top()`: the logo and the menu (`main_menu`), the scenario list and the new game
and custom battle windows. The map and building windows are already centred over the view above the bottom
bar (`window_layout::window`, `main_menu::map_window`, the saves and dialogs), so they are not moved again.
The battle window held upright (`battle_view::Layout::upright`, pure, with unit tests): the window's scale comes from
`window_layout::game_scale` (the width less 8 px, above the bar); the unit's band (`unit_sheet::draw_band`) on top at
design y 76, then the quick buttons, the enemy's grid, the strip, the player's grid; the close cross is its title bar's, inside the taller strip (see the close crosses below). The hover box (`BattleView::hint`) is drawn after the quick buttons in that layout (`Layout::hint_after_buttons`), because the enemy's back row lies under them (a player's report); landscape and the desktop draw it before them, as before. The landscape phone window is placed below the safe top plus the strip's growth (`battle_view`'s `Layout::landscape`, `chrome::phone_title_grow`). The map, the bar and the exit window are drawn at the interface scale (`display::with_window_scale(None, …)`).
Stacked windows (experiment, main only): a phone held upright (`window_layout::portrait`: `dp_px` set and the
screen taller than wide) stacks the building, army and hero windows, each with its own layout
(`window_layout::Stack`, passed to `window_layout::scaled`). Building: the two panes on top at full width, each at the
landscape pane's height (`window_layout::lower_height`), then a row of the building's tabs
(`window_layout::building_tabs`) with Выход centred under it (`building_exit`), the window 746 design px
tall; the quest header row sits at the lower pane's bottom, above the tabs. Army and hero: the
unit's band under the title, 420 design px (`window_layout::army_band`, `unit_sheet::draw_band`: picture
left, stats right; the band is the least height where the stats and the hero's bonus paragraph fit at the
band's text size, which is never below the landscape column's; the picture is scaled to the band's height,
its proportions kept and centred in the picture half (`unit_sheet::band_figure_rect`), the frame picture (`UnitFrame`) stretched over that same half (`unit_sheet::band_frame_rect`, `chrome::unit_parchment_framed`), so it is about two thirds of the landscape size; the window is 1092 design px tall, width-limited
on a 1080x2376 phone),
the right side (pack, formation) under it, and a «Закрыть» button under the formation (`army_close`). On the band the unit's trait rows (its bonuses, state lines) that do not fit its place under the stats scroll (touch drag, wheel, the bar at its right; `unit_sheet::band_list_most`), as the landscape column's trait list does under the simultaneous bonuses rule; the description goes first, as before. The
right side moves by the layout's shift (`window_layout::place`, used by `at`, `point` and the panel helpers),
and the army seam is a 2 px line under the band (`window_layout::frame`, `window_layout::seam`). These windows are drawn inside
`window_layout::scaled` (`display::with_window_scale`), so `chrome::k()` is the stacked window's own scale
(`portrait_k_for`: the largest that fits the width and the height above the bar); the map and bar under them are
drawn at the interface scale (`world_view::window_backdrop`). Landscape and desktop are unchanged; the
spellbook keeps its landscape window (`window_layout::landscape_window`).
The title background is covered, not stretched, on a portrait screen whose aspect is more than 20% off the
picture's (`chrome::cover_source`: the picture's centred crop); every other screen stretches it as before.
The title menu held upright (`main_menu::portrait_menu`, pure, with unit tests): the five buttons are 1.6× the
landscape size in one column, its middle at the middle of the area between the top safe inset and the foot lines
(the owner's OnePlus, 1080×2376 at 2.75 px per dp: the column's top moved from ~1001 px to ~561 px, its middle
from ~1570 to ~1110); the logo sits 24 px above the
column, never above the safe inset (the column moves down if it must); the links (custom battle, maps and mods, Discord, language) are chips
in one row at the bottom, touch targets of 48 dp; the version and copyright lines and an installed update's
notice sit just above the chips; the not-found block (no install) sits above the logo. Sideways on a phone with
an install (`main_menu::landscape_menu`, pure, with unit tests): the five ovals keep the original's column
(`landscape_buttons`, unchanged size), the logo and the foot lines stay where they were, and the four links are
tiles stacked down the right edge, half the room right of the logo and the column wide, above the version line
(48 dp and more each; the owner's choice, 2026-11-09). Their labels keep 14 sp and wrap inside the tile
(`label_lines`; the icon shrinks to 22 dp when a label takes two lines). The tile layout is drawn only by `main_menu::frame` (`SIDE_TILES`), so the
other screens' backdrops keep the corner links. Landscape without an install and the desktop keep the original
column and the corner links (`corner_links`).
With a mod installed the title shows the mod played (the owner's request, 2026-11-02): a «Мод: <title>» link between «Карты и моды» and Discord (desktop), or a full-width tile above the four chips (upright, which lifts the foot lines and the column by its height and gap, `portrait_menu`'s `mod_row`). A click opens the same «Мод» list as the new-game window (`new_game::mod_choice`, `request_mod`, key `razdor:menu-mod`); the app plays the pick (`take_mod_request` after `main_menu::frame`). Without a mod nothing changes.
Title windows held upright (the owner's request, 1080×2376: the scenario list and the new-hero window, the load
window, the settings window and its «Ещё…» page, the keys window over it, the authors window and the custom battle
setup): `window_layout::portrait_title` (pure, with unit tests) gives the window's own scale and rectangle: the
largest scale that spans the screen's width less 8 px a side and fits between the top safe inset and the footer
(`main_menu::portrait_foot_top` less 16 px), never below the interface scale, centred between the two. Each frame
sets that scale after its `main_menu::backdrop()` with `window_layout::title_scale` (a guard that restores the
scale on drop); `backdrop` always draws at the interface scale, so the logo and the foot do not grow. The window's
placement takes `window_layout::title_rect` where it is portrait, else its old placement (the load window's
`book_top`, at video y 190, is above the logo on a phone: that is why it moved). The credits column
(`thanks_column`) goes under the window down to `window_layout::title_bottom` (the footer), since the window
leaves no side. Landscape: `title_scale` takes `sideways_title_k` (a phone choice, not in the original: one original px
per dp, fitted to the screen less the safe area and the height above the bar, the large windows setting on), and
`title_rect` centres the window as an exit window (`exit_rect`). Desktop and portrait-off-phone: `title_scale` keeps
the scale it has and `title_rect` is `None`, so nothing changes. The in-game windows over the map get the same treatment on a phone held upright, with their own
rectangle: `window_layout::portrait_game` (pure, with unit tests) fits the window between the top safe inset
and the top of the bottom bar (`chrome::bar_height`, at the interface scale, 8 px above it), not the footer;
`window_layout::game_scale` / `game_rect` are the guard and the placement of the windows over the map that have no
title twin (the game menu's art window: `saves::exit_window`, `exit_dialog`, at the art's 634×516 px scaled by `opx`;
the spellbook; the battle's stacked window). The windows over the map that have a title twin take `window_layout::over_map_scale`
/ `over_map_rect` (`window_scale` / `window_rect` pick them by `in_game`): the settings window (its guard is set in
`options_window` over the map), the settings window (one window with its tabs, `desktop_options` / `phone_options`; its panes scroll in `pane_begin`), the keys window (`keys_window`), the save window (`save_screen`,
the name field's keyboard lift applies to the scaled rect), the load window (`load_screen`), and `map_window`, which
centres them over the map. Sideways on a phone these take the title's rule (`window_layout::sideways_window`, the same
scale and rectangle as the main menu's window, a phone choice: the windows were small in landscape, playtest 1365).
Over the map they use `window_layout::sideways_game_window`: the same, but never over the bottom bar; where the title
rule's floor (the base scale) would make the window taller than the room between the top inset and the bar, it scales
down to that room and stays centred there (the settings window: 774 video px wide on the desktop, the room's size on a phone, as its tabs and rows need). The map and the bar under them are drawn at the interface scale
(`world_view::window_backdrop`, `backdrop_lit`). The help overlay (F1, «Клавиши» in game, `hotkeys::help_overlay`)
already spans the phone's width; on a phone with a bar it stands between the top inset and the bar (`has_bar`).
Desktop is byte-identical, and so is landscape for the windows with no title twin (`game_rect` is None there, the old placements stay).
The exit windows (the game's «Выход из игры» and the battle's `saves::battle_exit_dialog`) have their own
scale: upright on a phone, `window_layout::game_scale`; sideways on a phone, `window_layout::sideways_exit_k`,
the large-window scale (`display::popup_scale`, one original px per dp, fitted to the screen less the safe
area's sides and the bar), so the window and its text and buttons grow together (about 1.3 times the
interface scale on a 2400×1080 phone). `window_layout::exit_rect` centres them between the safe area's sides
and between its top and the bar. Their «Рестарт» question (`saves::question_box`) is drawn at that scale
too, centred between the safe area's sides. On the desktop all of it is as before (no insets, no phone scale).
The spellbook in portrait (`spellbook::frame`, `window_layout::upright_book_video`, a phone choice) is 560 px wide
(two columns of cards), as tall as the screen takes between the top inset and the bar, so its rows fill the height;
its rows are as many as keep every card at least the landscape card's 96 px tall (`spellbook::upright_rows`), 20 spells
a page on a 2400 px phone. Its card text takes the phone's text base (`display::text_for`, `dialog::TEXT_REF`).
Its bottom row is inside that rectangle.

Upright phone layouts after the owner's OpenAI-mockup review (2026-11-02; only a phone held upright changes unless said,
desktop and sideways pixel-identical, texts drawn under `dt_font::legacy_metrics_if` so their sp sizes stick):
- scenarios (`new_game::Portrait`): a list view (the top filter row, the list taking the rest, «...»/«Отмена») and a card
  view (`card_view`, opened by `pick_tap` on a startable entry; Esc/close back to the list): the map's picture as the
  hub's hero (`hub_view::hero`, shared with the hub's card) with the title over it, the description centred, scrolled
  by `hub_view::pane_scroll`, «Назад»/«Далее». `RAZDOR_SCENE_CARD=1` opens it in a snapshot;
- the hero window on a phone (both orientations) is the diary-only window «Дневник» (`items_view::phone_diary`,
  `phone_diary_areas`): four tab buttons, the entry list, the picked entry's text centred; upright the list over the
  text and «Закрыть» at the foot, sideways the list left and the text right with «Закрыть» in the tabs row. The
  desktop keeps the original hero window;
- market (`window_layout::market_panes`, `Stack::Market`): the item description strip on top (`ItemBox::MarketStrip`),
  the section icons, the list taking the height, one row of «Инвентарь», money, «Купить», «Торговая лавка»;
- the title's bottom links are four marble tiles (`main_menu`); the options/settings windows take the phone's width
  and height with compact rows (`general_phone`: each slider with its label then its track, 14 sp, 40 dp rows at a 44 dp pitch; a label over 60% of the row wraps to two lines (`option_slider_sized`); each checkbox 34 dp for one line); the panes
  scroll by finger, the wheel and their bar (`pane_begin`: `scrollbar_px`, 24 × k wide, twice the new-game
  list's 12 × k, `new_game::list_track`; `ADV_SCROLL`, `ADV_VIEW`: rows drawn and clicked only in the view). The tabs
  (`tab_column_rects`: a column at the left sideways, a 3 × 2 grid upright, 44 dp each, 14 sp) pick the pane (`phone_options`);
  «Принять» and «Отмена» (44 dp) stay at the bottom right; keys
  (`keys_window_upright`) and the F1 list in dp; save/load with their buttons in one row; the reward windows'
  OK 120 × 44 dp and larger text (`dialog::OK_W_DP`, `BIG_TEXT`); the learnt-spell window padded, its spell card in
  the empty text box's place (`learnt_slot`, `spellbook::spell_tip_card_padded`); the army window's «Закрыть» a
  normal 44 dp button; custom battle's quick buttons 44 dp.
- the town windows (the building, the barracks and garrison, the market; `window_layout::town_rect_in`, `town_k_in`, pure,
  with unit tests): one bottom edge (the market's old one, a few dp above the bottom bar) and one scale (the smallest of
  their three stacked scales), so the tab row and Выход stay where they are between sections (a player's report);
  the tab row and Exit are measured from the window's own bottom (`building_tabs`, `building_exit`);
- the spellbook held upright (`spellbook::phone_foot`, `controls`): «Закрыть» at the foot beside the filter button, both
  44 dp tall with 14 dp labels; the page arrows and "N / M" on a row above them, always reserved so the rows do not change
  with the page count; the corner cross stays; desktop and landscape keep the bottom row of arrows (`bottom_row`);
- the event and talk windows held upright with a picture (`dialog::event_frame` with `stacked`, `stacked_top`,
  `picture_x`, pure): the picture centred across the top at its own size, the text box under it across the full inner
  width, the window taller by the picture; the box gives up its height first when the window does not fit (`phone_fit`).
  Beside the text (desktop, sideways) nothing changes.

The mobile check (`tools/ui-regress/mobile_ui.py`) has the `update`, `diary` and `scenarios-card` screens.

The minimap (`minimap::window`) in
portrait stands inside the safe area (`in_safe_area`, `inset_view`). It has no close cross on a phone (the owner's request, 2026-10-28,
not in the original): the bottom bar's map button closes it, as M and Esc do. The F1 keys list (`hotkeys::help_overlay`) takes a cross on a phone and closes by it.
Close crosses on a phone (a phone choice, not in the original): every title strip is 1.5× the desktop's
(`chrome::phone_strip_height`, `PHONE_STRIP`) and its cross sits inside it at the right, sized by the desktop's formula
on the taller strip (`chrome::title_close_cross`). The taller strip grows upward into the room above the window
(`chrome::grown_upward`: the frame starts `phone_title_grow` higher), so the window's content and its `inner` rect are
where the desktop puts them; where the safe top leaves no room above, the window keeps its rect and the strip covers its
top, so the caller must place the window with that room (the landscape battle window does, `window_layout` for the
others). The cross's click area is a 44 dp square over it, never below the strip (`chrome::touch_hit`, `close_button_at`).
The title keeps clear of the cross (`room_clear_of`). The battle window's cross is its title bar's own. The desktop's
strip and cross are as they were.
Not covered: the building, army and hero windows (the stacked layout).
The market and the sanctuary scenes of the UI regression check use town 8 of РК3-Столица (the start's
castle has neither tab); the snapshot clears the start's due places, which otherwise make the building
window step aside for the map (`world_view::building_flight`).

Touchpad mode (`src/ui/touchpad.rs`; off by default, on only at `Settings::touchpad == Some(true)`
on mobile, set by the General tab's last checkbox there, `general_checks`): the screen is a laptop trackpad, instead of the gestures. `TouchMapper::step` is pure and turns the frame's touches (`TouchPoint`, from
`macroquad::input::touches()`) into a `gamepad::Frame`: one finger slides the pointer (1.6 px per
px, the distances scaled to the screen's height); a tap (down and up within 0.25 s, under 12 px
of slide) clicks where the pointer is, pressed on the lift and released the next frame; a second
tap within 0.3 s and 40 px of the first is held with the left button while its finger slides
(a double click when it does not); two fingers tapped are a right click, two still for longer
than 0.3 s hold the right button until one lifts; two sliding together scroll the map (the stick,
`map_scroll`) or turn the wheel (a step per 40 px, the content follows the fingers). A third
finger ends the gestures and lets a held button go. `input::begin_frame` runs it while the mode
is on, and merges its frame with the pad's (`merge`): its pointer starts at the screen's
centre and stays the pointer. `ui::cursor::draw` draws the pointer as a plain arrow, counting
the touchpad as focus, as a phone has no system pointer; in gesture mode it draws nothing (the
finger is the pointer). `input::begin_frame` picks the mode: touchpad, else gestures on mobile,
else no touch. The mappers' tests cover each gesture. In this mode the event and victory windows' item and spell icons show their description while the cursor is over them (`dialog::shown_preview`, `dialog::icon_hover`: at once, as the items' hover); the desktop keeps the original's, shown only while a button is held (0x4c1fec / 0x4c2048).
The rest is `ui::rest` (`rest::tick` in `App::frame`, `rest::rested()`: the pointer still for
`REST_SECS` = `input::HINT_DELAY_SECS`, 0.35 s, the gestures' long-press delay, with no button down). In this
mode every hint waits for it, through `input::hint_hover()`: the bar's and the widgets' hints and the map's tooltip of an army or building (`world_view`). On the world map the resting pointer's card hides the pointer only when the card covers it (on a phone it is placed clear of the finger); the right-button card hides it always, as the original. The item descriptions of the backpack and the hero window (`items_view`) and the icons above (`dialog`) do not wait: they use `input::item_hint_hover` and `dialog::icon_hover`, at once.

Phone placement (backlog 1449): every pop-up that follows the pointer goes through `popup::placed`. On the desktop (`display::dp_px()` is None) it returns the caller's own rectangle, unchanged. On a phone it calls `popup::place_popup`: above the finger with a finger's gap (`widgets::FINGER_DP`), else beside it (right, then left), kept inside `popup::usable()` (the screen above the bottom bar). The sites: `widgets::tooltip_content` (every `tooltip`, `tooltip_later` and `draw_later` box), `widgets::hint_box_always` (the spell-target hint), `world_view::draw_tooltip_at` (the map's army and building tooltip), `dialog` (the icon preview under a press), `items_view` (the item description in the army and hero windows), `spell_badges::flush` and `battle_view`'s preview. The event window's reward-item tooltip (fixed at the window's top) and the bottom bar's panel hint do not follow the pointer and keep their places.

## The frame

`App::frame` in order:

- a performance test's Esc (`bench::handle_escape`) returns at once, and its cell is set up before the screen draws (`bench::before_frame`); its windows are drawn last (`bench_view::modal`); see [testing-and-tools.md](testing-and-tools.md);
- per-frame resets: `input::begin_frame`, `chrome::begin_frame`, `cursor::begin_frame`,
  `widgets::track_held_key` (the original's one held key: any key up clears it, and the
  system's repeats of the newest key still down hold it again, 0x475820); Alt+Enter display mode (`display::hotkey_pressed`);
- settings pushed into the game each frame (`main_menu::follow_settings`: `g.improved_ai`,
  `g.max_ai`, `g.pursue`, `g.walk_pace`; animation
  speed, row width) — the settings live in the UI, the rules only read plain fields; a game
  just loaded or restored gets them at once, before the battle it may start (#19: a pending
  foe, a battle's «Рестарт»); `main_menu::apply_options` puts the
  scroll speed and the autosave where the interface reads the install's `[Options]`
  (`chrome::set_option_override`) and turns the hint boxes on or off (`widgets::set_hints`);
  the walking speed slider runs 0..200 (`rules::game::WALK_SPEED_MAX`): 0..100 is the
  original's `WalkSpeed` range, 100..200 is Razdor's extra speed up to 2× (the owner's request);
- the cheat console and the update window decide whether input is blocked:
  `widgets::set_input_blocked(true)` while a dialog, the F1 overlay, the console or the
  update window is on top. Screens still draw under them but must check
  `widgets::input_blocked()` (most helpers do it for you);
- `match (&mut self.screen, &mut self.game)` calls the screen's function, e.g.
  `world_view::frame`, `building_view::frame`, `items_view::squad`, `BattleView::frame`,
  `items_view::hero_window`, `spellbook::frame`, `saves::load_screen`, `main_menu::options_window`;
- global hotkeys (`hotkeys::allowed`, F1 help, F5/F9 quick save/load, F10 the autosave written last (Razdor's), language);
- the front dialog: `dialog::draw` returns `Some(Close)` when closed, then the closing is
  routed (village offers → `building_view::answer_offer`, questions →
  `Game::answer_question`, event windows → `Game::event_window_closed`; results go back
  through `world_view::handle_events`);
- a pending fight (`game.foe`) opens `saves::battle` once no dialog is open; script
  victory/defeat ends the game; the noon autosave (`game.autosave_due`);
- the screen change: bookkeeping that depends on *which* screen closed (window-closed
  events after side windows, `Game::army_window_opened`, map camera reset or not).

Trap: anything that must happen when a window opens or closes goes in that last block, not
in the window's own function (the function does not know it is the first frame).

## Screens and windows

`Screen` (`src/ui/mod.rs`) has one variant per screen. Windows over the map
(`Building`, `Squad`, `Hero`, `Spellbook`, `Menu`, `Settings`, `Save`, `Load`) draw the map
under them with `world_view::window_backdrop` / `world_view::backdrop_lit` (the bottom bar
stays live and lights the window's button). The settings window uses `world_view::settings_backdrop`:
only its own bar button is live, and a second press on it closes the window with its changes kept
(`main_menu::close_options_kept`, as «Принять»; the player's report, 2026-10-30). The bar's flag (`BarButton::Hero`) opens the
original's hero window (`Screen::Hero`, `items_view::hero_window`: the hero's panel, the pack,
the item description, the four resource lines and the diary); J opens the same window. The
diary keeps the original's look; in place of its list title it has five tabs (asked for by the
owner): the quests under way (the original's diary), Razdor's history of completed quests,
rumours and messages over the whole campaign, with dates and a heading per campaign map
(`items_view::HeroView`, `story::journal_lines`), and the chronicle («Летопись», Razdor's, the
fifth; `items_view::DiaryTab::Chronicle`). The chronicle's rows are `rules::chronicle_text::rows`
(the dry line of each entry, its full text for a text window): one line per entry under a heading
per chapter and per day («День N», `story::chronicle_lines`), the newest at the foot; the tab opens
scrolled to the end (`items_view::switched`). Its entry box shows a text window's whole text, else
the entry's line. «Итоги» stands at the top of its list (`summary::open`, `After::Diary`: the whole
story so far, the route of the map being played). The phone's diary has the same five tabs (three
and two held upright, `phone_diary_areas`) and the same «Итоги» row.

The «Итоги» page (`src/ui/summary.rs`, `Screen::Summary`; the data is `rules::chronicle::Summary`,
built by `Chronicle::summary`) shows the map as the fog stands (`minimap::picture`), the hero's route
over it (cool blue at the start, warm orange at the end, `summary::route_colour`), the battles
(crossed swords, red for a loss) and the captures (a flag) where they happened, and the hero's end (a
skull, after a defeat); the highlights (`chronicle_text::highlight_lines`: the biggest battle, the
most units hired at once, the richest noon report, the longest walk in one day) and the totals (days,
battles won and lost, losses on both sides, places taken, artefacts, quests done). The page fits one
screen: upright the totals sit at the foot and the highlights above them, the map takes the rest
(`summary::layout`, the text reserved first); the text shrinks in steps only when it still does not
fit (`summary::fit_size`). Its one button («Закрыть» from the diary, «Далее» after a map) and Enter
or Esc leave it: back to the diary, or on to the next map or the main menu (`App::victory_end`,
`App::defeat_end`, `App::finish_victory`; campaign.md). No share or export button. On a phone (`display::dp_px`), while a diary
entry is shown and no artifact is described, the artifact description box is not drawn: the
quest list moves up to its top and the entry takes its height and the gap
(`items_view::diary_column`, a Razdor phone choice; the justification of the entry is the
original's). The phone's entry also drops the quest's title and the «Описание:» heading (the
quest list above names the quest; the player's suggestion): it is the text, then the time
heading and the time; a completed quest keeps its date and text, rumours and messages keep the
title. The desktop keeps the original's lines (`items_view::entry_parts`, pinned by its tests).
On a phone the bottom row of the hero window holds «Весь список» (left, `HeroView::list_reader`, `items_view::diary_list_reader`:
every line of the tab, scrolled by drag and the wheel, a tap on a row opens its entry, closing the entry returns to the list),
«Закрыть» (centred) and «Развернуть» (right, `HeroView::reader`, `items_view::diary_reader`): grey without an entry
(`expand_entry_enabled`) or, for the list, without entries (`whole_list_enabled`). Sideways the row sits at the window's
foot, from the diary's left edge (`items_view::sideways_row`, off the hero's panel): the two stack under the diary's
column (a phone choice, not in the original, so the labels fit), «Весь список» above «Развернуть», each as wide as the entry
box. The entry box ends above the upper one (`items_view::entry_above_row`, `items_view::bottom_row_rects`). The entry's full text (`reader_parts`, the desktop's parts) at the phone's sizes, in a window over the screen
less the safe area and an 8 px margin (`diary_reader_rect`), scrolled by drag and the wheel, and closed by its
finger-sized «Закрыть», its cross or Esc. While it is open the window below takes no input (`set_input_blocked`),
and the reader swallows the frame's keys and clicks.
Hit areas and scrolling follow the boxes they are given. Hint boxes
(`widgets::tooltip`, `panel_hint`) fade in over 300 ms as the original's.
A tap outside a window opened from the bottom bar (Menu, Settings, Save, Load over a game, Hero, Squad, Spellbook; not the minimap, whose map around it stays live), and off the bar, closes it as Esc does (Razdor's choice, the owner's request, 2026-10-28; the original keeps it open). It is decided in `ui::mod` (`frame_content`, `bar_window_open`) from the press and the release (`widgets::outside_tap`: a tap, not a drag; 12 px on a phone, 6 px on the desktop) against the rects the windows and popups recorded last frame (`chrome::note_window`, `chrome::take_drawn_windows`); on that frame `widgets::esc_pressed` is true.

An item's description, wherever it is shown (the market, the hero window's box, the army
window's pop-up, the event window's item tooltip), is one box, `item_text::draw` in
`src/ui/item_text.rs`, laid out per window by `item_text::ItemBox`; its usage line, effect text
and bonus line are the original's (interface.md §9.2), built by `rules::item_text` from the
install's `[Skills]` labels (`spell_badges::label`). The pop-up (`ItemBox::Tooltip`, the army
window's and the event windows' pointer pop-ups) grows with «Размер текста» (Razdor's, the
owner's request, 2026-10-30): its text is `display::item_text_for` (1.2 times the normal size's
text at Large and Largest, `display::item_text_factor`) and its box is `item_text::tooltip_size`,
1.2 times the 272 px at the normal size, kept to the screen (6 px a side; the height at 4 px
above and below, the text drawn smaller if it still does not fit). The hero window's box and the
market's stay at the window's text size (`display::text_for`).

The pack is the original's cells with holes (`pack::Pack`, `Game::pack`): an item stays in
the cell it was put in, gains fill the first empty cell. On main it has no last cell (the
original's 256, a deviation): a gain past the last used cell opens a new one. In the hero and army windows items
move as the original's, by presses only (0x4c24f4): a press takes an item from a pack cell or a
worn slot of the unit panel onto the pointer (`Game::held_item`, drawn as the pointer by
`items_view::draw_held`); the next press puts it in a cell (`Game::press_pack`), a worn slot
(`Game::press_worn`) or, in the army window, on a unit's card (`Game::give_held`: a potion is
drunk there). `items_view::pack_view` returns the pressed cell, an empty one too. While an item is held, the
unit card under the pointer is lit as the original's (interface.md §9.7): blue where the unit
takes it, red where it refuses (`Game::held_refusal`, the same test as `Game::give_held`, without
giving), and the refusal's line shows at once, before any press. Leaving the
window by any way puts a held item back in the pack: `Game::drop_held` in `App::frame`'s
screen change, and `save::restore` for a save made with one held. The filter (Razdor's) lists
the matching cells packed; an empty row of it puts the held item in the first empty cell, and
Enter picks the first match up. An empty worn slot on the unit panel is the original's picture
(`chrome::empty_item_slot`), a worn item its picture alone.

The backpack's view (Razdor's, view only: the pack keeps its order) is one row at the top of the
army window's backpack (`items_view::squad`, y 31, x 258 to 826): nine equal buttons with small
even gaps (`item_filter::row_cells`), drawn on the green marble of the original's buttons
(`item_filter::row_face`: `Btn1`/`Btn2`/`Btn3` Up, Down while lit, three-sliced so the ends keep
their shape). The first eight are the group buttons (`item_filter::group_row`, in
`item_filter::ROW`: helms, armour, weapons, shields, rings, amulets, potions, other;
`rules::pack_order::ItemGroup`, each kind in exactly one). Each shows its group's picture
(`item_filter::representative`: the lowest id of the group, but a shield prefers an item named
«щит», so the shields button shows a shield, not the bracers) fitted inside, its name as a hint (`group_label`). A click on a
group shows only it and holds its button down (accent edge); a click on the lit one shows all
again (there is no "all" button). The ninth is the search (`item_filter::search_button`): idle it
reads «Поиск»; a click, Ctrl+F or a typed letter makes it the field, the text shows on its face
and it stays lit while a query is held; Esc clears it. Its hover hint gives the found and of
count, the order and «ПКМ — порядок». A right click on it goes on to the next order
(`PackSort::next`); a changed order is named small at its bottom (`sort_short_label`), in the
body face. The pack's cells are then listed through `pack_order::pack_view_order` as the filter
lists them, so the filter counts within the group and Enter takes the first shown. Changing the
group or the order starts the pack at its top; both are kept for the game (`GROUP`, `SORT` in
`items_view`): a new game, a campaign map's start and a loaded save reset them (`items_view::reset_pack_view`). The market's list has the same eight group buttons under its titles
(`building_view::market`, a row of 24 px, the list below it; no search: the market keeps its own
filter line, `item_filter::field_with`): it hides the other groups' rows before the filter, and a
hidden pick is not the pick.

The book (`spellbook::frame`) lays its cells out column by column (`ROWS` = 5, 4dad08; 3 columns, the 15 of
`BOOK_PAGE`; upright on a phone 2 columns of as many rows as the height keeps, see above). Razdor's
choice (the owner's, 2026-10-22): the book pages all its spells 15 at a time (the original shows only the
first 15, 0x4d261c). A second page turns by the wheel (up is the previous page), PageUp/PageDown or
Left/Right, and the two arrow buttons with "N / M" in the bottom row under the grid; with one page those
are not drawn. A filter button ("Все", "Волшебство", "Проклятия": all, the spells on the own army, the
enemy spells; `spellbook::BookFilter`, cycled by its click) lists a part of the book, and the pages count
that list; a new filter starts on page 1. `Screen::Spellbook` keeps the page, the filter and `selected`
(the book index). The bottom row is always drawn. A card's background is the paper less the state's colour (`spellbook::card_fill`, `card_paper`):
normal, hovered (the hover is the colour, no frame), or dark red over the hero's mana. Every card has
the 1 px steel line the original draws into all its card backgrounds (0x4dc118, 0x48d1d4). The event window's spell tooltip is the same card at the book's cell
size, scaled with the window (`dialog.rs`, the spell preview), so its cost and duration lines show.
On a phone its text is the item pop-up's text (`display::spell_tip_text_for`), never under 12 sp
(`display::SPELL_TIP_MIN_DP`), and its box grows with the text (`spellbook::spell_tip_size`, `spell_tip_card`);
on the desktop at the original's size it is the card's own scale, as before.
A spell's card, wherever it is shown (the spell book's cells, the sanctuary's description, the
event window's spell tooltip), has the same text, drawn by `spellbook::card_text` in
`src/ui/spellbook.rs` (`spellbook::spell_card` adds the box and picture): the name, then the
effect text of `rules::spell_hint::effect_text` (the same builder as the badge hint's), the
cost and the duration line, each word-wrapped as one paragraph (interface.md §9.3). On main the
effect entries are coloured one by one (`spell_hint::effect_entries`, `spell_hint::good_for_holder`,
drawn by `widgets::wrap_coloured` and `widgets::shadow_coloured`), in the cards and the badge
hint alike: blue when good for the unit holding the spell, yellow (red in the hint) when bad;
the original colours the whole effect by the spell's target (a deviation the owner asked for).

The custom battle's setup (`Screen::CustomSetup`, drawn by `custom_battle::frame` in
`src/ui/custom_battle.rs`, a Razdor extra) keeps its armies in `rules::custom::Setup`. Its left
column has two tabs, the unit types (`custom::unit_list`) and the items the picked unit may
put on (`custom::item_list`: by `custom::Category`, sorted by `custom::PriceOrder`, kept by
the inventory filter's match). Each list has the inventory filter line
(`item_filter::field_hinted`, started by a click or Ctrl+F only); the screen takes the typed
characters first (`widgets::typed_chars` while no field has the keyboard) and a letter jumps
to the next name starting with it (`custom::jump`). «Скопировать код боя» / «Вставить код
боя» (and Ctrl+C / Ctrl+V while no field has the keyboard) write and read the whole setup as
one line of text, `RZB1:` + base58 (`rules::custom_code`: `custom_code::encode`,
`custom_code::decode`, versioned, CRC-checked, with the content's fingerprint to warn about
another install or mod) through miniquad's clipboard (its window module's clipboard_set and clipboard_get:
X11, Wayland, Windows and macOS). A code that does not read changes nothing; unknown unit
types and items are left out and counted in the message.

Building a window (`src/ui/chrome.rs`):

- `chrome::k()` is the interface scale: screen pixels per pixel of the 960×720 reference
  video. Every size and offset is written in reference pixels times `k`. `chrome::fit_k` is
  the largest that fits; `chrome::bar_height` the bottom bar. The chosen scale is kept to the
  fit (`display::scale`, since 1.0.1; Auto is the fit): a scale above the fit, saved by 1.0.0, is
  kept to the fit too, so the settings always fit the screen (players got stuck with them
  off-screen in 1.0.0). `display::chosen_scale` is the setting as it is. Ctrl+0
  (`display::scale_reset_pressed`, `ui/mod.rs`) sets it back to Auto on any screen, with a notice
  (`display::notice_scale`), and swallows the key so the world map's 0 does not fire. On the desktop the
  bottom bar, the FPS counter and the keyboard strip use `chrome::bar_k`, the chosen scale cut to the fit
  (`display::fitted_scale`), so they stay as large as the fit allows. On a phone the bottom bar has its own
  scale, `chrome::bar_k` = `display::phone_bar_k` (Razdor's, backlog 1557): the scale at which its readouts
  reach 14 sp (`BAR_TEXT_SP`), kept to the width less the safe area with a 90 dp time panel.
  `game_bar::draw_with_time` draws the bar as the window scale (`display::with_window_scale`) at that scale;
  the bar's place is its width capped at the screen less the safe area. The FPS counter and the keyboard
  strip keep the fit (`chrome::strip_k`). On a phone held sideways the bar's scale is `chrome::bar_k` =
  `display::bar_size_k` of the setting «Размер нижней панели» (`display::BarSize`, `audio.json`
  `bottom_bar_size`, default Large): Normal is main's scale (the fit), Large the phone's text size
  (`display::phone_bar_k`), Medium halfway. The landscape date wraps and stacks in its pane (`stack_time`),
  so it is never cut. Upright and on the desktop the bar is main's, one row at the fitted scale: the owner
  rejected two other layouts (2026-11-08), so the upright bar does not change with the setting. Windows
  above the bar keep main's height (`chrome::bar_height`), so they keep their size and may overlap the bar's
  taller part. The map's bottom edge, the pan's bounds (`pan::begin_frame`) and the bar's place use
  `chrome::bar_top_height`, the drawn bar's top; the bar's cover (`pan::cover_bar`, the clips, the presses on
  the bar) uses `pan::bar_top`, the old line (`chrome::bar_height`); pop-ups (`popup::usable`) keep to the
  drawn top, so they never cover a bar button.
  `game_bar::bar_rect` is the bar's rect for what hangs off it (the compass).
  The minimap may stand in a corner beside a bar that does not span the screen, down to the screen's bottom
  (`minimap::bottom_edge`, backlog 1560).
- Buttons grow with the text size (Razdor's, the owner's decision of 2026-10-31; every screen
  except the bottom bar's readouts and the market row). At ×1 every
  button keeps the original's size; above it `display::button_grow` (the text size) multiplies
  the button, its footer or backing, the price line under a pill, and the label. A label is drawn
  at `g` times its size at ×1 (`display::grown_text`), under `dt_font::legacy_metrics_if` (the
  native metrics ignore the size asked for), the largest size from there that fits the button's
  width and never below ×1 (`display::grown_label_size`): a grown label is never cut. The
  event window's inner frames end above the grown footer (`chrome::window_inner_frames_footed`).
  `widgets::small_button` and `toggle_button` grow their labels themselves (`compact_label`, up
  to 0.75 of the button's height); each screen grows the buttons' rects and makes room.
  The market's labels are bound by the original row's widths. Checked by `mobile_ui.py adapt`
  (the `mobile-ui` skill).
- Text size and large windows (Razdor's, the owner's request: phones' text too small). The
  two settings are independent (players' reports, 1.0.3). «Размер текста» (`Settings::text_size`,
  `display::TEXT_SIZES`: ×1, ×1.25, ×1.5; on phones ×1.25 by default) multiplies the text base and
  alone sets the glyph size
  of the long wrapped texts (messages, building descriptions, village and shipyard texts, the
  diary entry, item descriptions, map tips), of the window titles (`chrome::title_bar_styled`,
  the strip grows with the size: `chrome::window_styled`) and of the tooltips' text and title
  (`world_view::draw_tooltip`, its glyphs read by `dt_font::sheet_size` under
  `with_legacy_metrics`, as the native metrics ignore the size asked for). The text base is
  the window's scale on the desktop; on a phone it is one original px per dp
  (`display::phone_text_base`, the 1.0.2 large-window scale, read for the message
  `dialog::TEXT_REF`), always, with the large windows on or off, so tooltips, titles and
  messages share one scale there (`display::text_base`, `display::text_for`). A box that cannot
  scroll takes the largest size that fits, stepping down from the chosen one
  (`display::text_o_fitting`: 1.5, 1.25, then the original's); a scrolling box (the event
  window, the diary) takes the chosen size as it is. «Крупные окна сообщений»
  (`Settings::large_windows`, on by default on phones) only lets the 634-px pop-ups grow (event
  window, village event, village and shipyard windows): `display::popup_o` gives the largest
  scale that fits the screen above the bar, from their smallest size. On a phone
  (`display::dp_px`: one original px per dp, read from the system's density, 1 on iOS) that is
  one px per dp, and the cap of twice the interface scale is off; on the desktop the cap stays.
  The width of a large event window is what its text needs (`dialog::text_fit_width`: the
  widest row plus the frame, at least 260 px of text, at most `dialog::event_width`: the
  screen less 12 px, 860 on a phone, 634 on the desktop), so a one-line message is narrow. With
  the large windows off the event window is the original's 634 px. A taller title strip
  (`chrome::title_grow`, the text's growth over the original's 26 px) is added to the event
  windows' height (normal and village), and their inner frame and content move down by it.
  The rewards on a phone (backlog 1453; Razdor's, the original's window grows instead): the
  event window fits the screen (`dialog::phone_fit`): the text box keeps its whole text while
  the rewards fit, else it gives up its text down to five lines (`TEXT_FLOOR_LINES`, or its
  whole text when shorter), and the rewards that still do not fit scroll in a clipped area
  under the text (`Clip`, the wheel, the swipe and `widgets::scrollbar_px`, `Dialog::rewards_scroll`).
  The desktop takes the old path, so its window is the original's, unchanged. `display::dp_px` is
  `RAZDOR_DP` in snapshots; `RAZDOR_SCENE_TEXT` and `RAZDOR_SCENE_TITLE` give the `message`
  scene its text. `Settings::layout_version` (`LAYOUT_VERSION`): a file saved before 1.0.2 is
  reset once on load (interface scale back to Auto, text size and large windows to the
  platform defaults), then saved again.
- The pan (`src/ui/pan.rs`, Razdor's, the owner's request; inert since 1.0.1, as the canvas never
  exceeds the screen): when the canvas (960k × 720k) is
  larger than the screen, the offset is a translation of macroquad's camera that shifts
  everything drawn in the frame. `pan::begin_frame` runs before `input::begin_frame`
  (`App::frame_content`) and clamps the offset with `pan::offset_range` (the canvas edges meet
  the screen's, no further). Its input: a middle-button drag (desktop), or the finger's pan of
  the frame (`input::map_pan`, from last frame's gesture: a swipe over a window's registered
  area, `pan::window_area`, called by `chrome::window_styled`). It resets when the screen or a
  dialog changes (the window opened is centred). `input::mouse_position` takes the offset off,
  so the hit tests follow the drawing. `pan::unpanned` keeps the bottom bar
  (`game_bar::draw_with_time`), the map under a window (`world_view::window_backdrop`,
  `backdrop_lit`), the title's background (`main_menu::background`) and the map alone where
  they are; `pan::end_frame` (`main.rs`) puts the FPS counter, keyboard strip and pointer back
  on the screen. Not panned: the editor, the map with no dialog open.
- The bar over a window (`pan::cover_bar`, called after the bar is drawn in `world_view::backdrop_lit`
  and `window_backdrop`): when the canvas overflows, the frame's drawing is cut off above the bar
  (a scissor, also in `widgets::Clip`) and the window's controls take no hover or press over it
  (`widgets::mouse_in`); a press on the bar is swallowed for the rest of the frame. The pan's
  clamp stops the canvas's bottom at the bar's top (`pan::Pan::settle`). `RAZDOR_SCENE_PAN=dx,dy`
  (`ui::snapshot`) holds the offset for a snapshot.
- The world map's scale at zoom 1 (`world_view::map_scale`, Razdor's choice) follows the
  interface scale setting, not the window: its 32 px cells are drawn at the chosen scale ×
  0.9375 (as large as the original's pictures in the windows), not cut down to the fit;
  Auto is the fit. The settings' «Масштаб карты» (`map_zoom`) multiplies it, the wheel and
  +/- step from that (×1.1 a notch) and 0 comes back; locked, the zoom stays `map_zoom`.
  Razdor's: the wheel and +/- zoom out past the settings' range (`ZOOM_RANGE`, 0.4) down to
  the whole map fitting the view (`world_view::zoom_floor`); a map that fits already at 0.4
  keeps 0.4 as its floor. The floor is passed as `lo` to `zoom_at` and `step_zoom`. The steps
  are refitted to the current floor and home zoom each frame (`world_view::fit_steps`), so a
  turned phone or a changed settings slider moves the zoom at once. The wheel, pinch and +/-
  zoom in up to ×4 (`world_view::ZOOM_WHEEL_MAX`, Razdor's, for tapping on phones), above the
  settings' ×2 (`ZOOM_RANGE`, the slider and the home zoom's clamp).
- `chrome::window(r, title, skin, closable)` draws background, frame and title bar and
  returns `(inner, closed)`. Skins: `chrome::Skin`.
- Placement: windows over the map are centred over the view above the bar (as the
  original's, over its 1024×682 view) — see `map_window` in `src/ui/main_menu.rs` (commit
  77cf546). Never hang a tall window from a fixed top: its foot runs over the bar.
- The original's layouts (army, building windows) use `src/ui/window_layout.rs`
  (`window_layout::window`, `window_layout::at`, panel rects).
- Buttons and text: `chrome::marble_button`, `chrome::pill_button`, `chrome::shadow_text`,
  `chrome::hint_text`, `widgets::button`, `widgets::text_field`, `widgets::scrollbar`,
  `widgets::tabs`. Clicks: `widgets::clicked` / `widgets::mouse_in` (they respect
  `input_blocked`). Sounds: `audio::cue(Cue::Button)`.
- The cheat console (`console::Console`, on the map and in battle) is scrolled on a phone by a one-finger slide over
  its scroll-back (`widgets::swipe_rows` over `console::scroll_area`, the wheel it turns is `input::mouse_wheel`). Its
  on-screen keyboard (a phone choice, not in the original) is wanted while it is open and after a tap in the input line
  (`console::keyboard_wanted`, the tap target `console::input_field`); the system hiding the keyboard turns the want off,
  so a slide or a tap on the scroll-back no longer brings it up. The desktop has no keyboard, as before.
- The console's completion (a Source-style choice, not in the original): as the line is typed, a list of up to 8
  rows shows under the input line (`cheats::suggestions`: the commands by their first word; after `item `, `spell `
  or `unit ` the game's names from `cheats::Names`, built by `Console::set_content` only when the game changes).
  Tab (Shift+Tab back) completes to the next row and goes on through the list (`cheats::cycle`); Up and Down pick a
  row (with a list up they no longer recall the history); Enter completes to the picked row and runs it only when it is
  a command with no argument (`reveal`); on a phone a tap on a row completes to it (`row_rects`, a finger's target). The
  list is made again only when the line is typed: a Tab's completion or a recalled line keeps the list it has.
- Text input: every field (`widgets::text_field`, `widgets::number_field`, the drop-down's
  filter, the inventory filter `item_filter::field`, the hero's name
  `screens::hero_name_field`, the save's name, the console) edits through
  `widgets::edit_keys` over a `text_edit::TextEdit` (caret and selection; pure, unit-tested)
  and draws with `widgets::edit_line` / `widgets::edit_rows` (the mouse places the caret and
  selects). Keys: typing replaces the selection, Backspace / Delete / arrows repeat while
  held (`widgets::key_repeat`, the original's typematic timing `widgets::repeats`), Shift
  selects, Ctrl+A / C / X / V. Never read macroquad's typed characters or Backspace's press in a field
  of your own: take `widgets::typed_chars` and `widgets::edit_keys`.
- Tooltips: `widgets::tooltip` (translucent, follows the mouse), `widgets::hint_box` (the
  original's hint box), `widgets::panel_hint` (bottom panel). Clip with `widgets::Clip`.
- Message windows: build a `dialog::Dialog` and push it on `App::dialogs` (screens get
  `&mut VecDeque<Dialog>`); `story::show` turns rules outcomes into dialogs.

Input goes through `src/ui/input.rs` (mouse/keys with gamepad emulation from
`src/ui/gamepad.rs`), never macroquad directly, so the gamepad and the snapshot pointer
(`RAZDOR_MOUSE`) work. Pointer shapes: `cursor::set(cursor::Shape::…)`; the map's choice is
`cursor::map_pointer`, the battle's `cursor::battle_pointer`. Per-screen keys and the F1 list:
`src/ui/hotkeys.rs` (`hotkeys::Place`, `hotkeys::screen_keys`).

The map's F3 debug overlay (Razdor's, `world_view::draw_debug`) also tints every visible cell
(`world_view::draw_walkable`, `walk_tint`): on foot green for passable (brighter on roads), red
for impassable; aboard the ship blue for sailable, red otherwise, all faint so the map shows through.
### Maps and mods (the hub; a Razdor extra)

A refused save (1552, `ui/lacking.rs`, `hub/save_mod.rs`): the load window's message box gets a mod button
beside OK when the save lacks units or items. The mod it needs is the one the save records
(`SaveMeta.played_mod`, see `saves.md`); an old save has none, so the installed mods are checked on a
worker (`check_installed`: each mod's merged folder via `hub::prepare_mod`, read as an install, must
have every missing id), and exactly one match is the need. `save_mod::offer` gives: installed (the
button runs `choose_mod`, then the save loads when `poll_mod_switch` lands), downloadable (the button
asks Yes/No with the size, then `Hub::install` on a thread and the same switch), or unavailable (the
button is off with its reason). The box's words, buttons and Yes/No are `saves::lacking_question_box`
and `load_failed_box`.

`main_menu` link (the corner row, or the chips held upright) → `Screen::Hub(HubView)` (`ui/hub_view.rs`) → `razdor::hub` on a thread:
`Hub::catalog()` reads `index.json` from `RAZDOR_HUB` (an `https://` address or a folder; unset:
`hub::DEFAULT_SOURCE`, the latest release of indicozy/razdor-community; `RAZDOR_HUB=mock`, or a
snapshot run, the mock hub `hub::mock::build` writes under the hub folder; a file's `path` that is
`https://` is fetched as it is, else under the source), `Hub::install()` installs the
entry after the mods it needs (`plan_install`), checking each file's size and SHA-256, into
`RAZDOR_HUB_DIR` (else `hub` in `platform::app_dir`, the same storage on the phones): maps to `maps/`,
a mod's files to `mods/<id>/files/<path inside the mod>` (a mod ships only what differs from the
base install), the record in `installed.json` (version, title, `requires`, files). `Hub::remove()` refuses a
mod still needed and removes a mod's whole folder. Each entry carries `status` (`full`, `partial`,
`broken`, `unknown`, with `status_note`) shown as the window's state line, an optional `preview` (a PNG
no bigger than 480x270, read from its header; macroquad decodes PNG and TGA only, so no WebP), cached
once in `<hub>/previews/<sha256>.png` and drawn at the top of the right panel (`ui/hub_view.rs`
`ensure_preview`), and, for a mod, `base` (the base it is made for; nothing checks it). An optional `picture`
(the map's own LIT picture in the `.DTm`, made by `razdor --map-picture`, `hub::export_cli`; the same checks and
caching as `preview`) is what newer clients show in its place; older ones ignore it. An optional `about` ({ru, en}: the map's own story, its `.DTm` description, a
campaign's first chapter's) is drawn first in the details (`details`), the post's text (`description`) after it under «Из поста автора»; without it the description alone, as before. An optional `rating`
({up, down}: the 👍/👎 on its Discord post, absent when it has none) shows in the row (`· +up / -down`) and in the
panel («Оценка в Discord»); the list is sorted by `hub::by_rating`: rated first by score (up minus down), then by
votes, unrated after, ties by the Russian title. A mod file
may be an `archive` (a zip, mods only): it is downloaded to `downloads/` (streamed, at most 256 MiB),
checked by size and SHA-256, unpacked into `files.tmp/` under the zip's own paths (a `\` counts as `/`;
the same name checks as a mod's file; at most 2 GiB and 20 000 entries, `hub::mods::unpack_zip`), and
`files.tmp/` then replaces `files/` in one rename. The window's downloads show «Загрузка…».
The install also keeps the verified zip as `mods/<id>/archives/<n>.zip` (`mods::archive_path`, `n` the file's place in the entry) and writes `mods/<id>/manifest.json` (`mods::Manifest`: each file's path, size and SHA-256, and the catalog files they came from; `write_mod`, `keep_archives`). `fetch_files` takes an archive from its kept zip when that still matches the catalog. **Damaged files** (an antivirus takes them): `mods::damaged_files` compares `files/` with the manifest (a mod with no manifest, installed before manifests, reports only the files that do not read); `Hub::repair_mod` writes the whole mod again from its sources (the kept zips, else downloaded and checked; with no manifest, the catalog's entry of the installed version) and drops its merged folder. `prepare_mod` refuses a damaged mod (`damaged: …`) and builds no folder from it. **Hub maps** get the same check: a map's record keeps its catalog files as `sources` (`InstalledEntry::sources`; older records have none and are checked for readability only, repaired from the catalog). `hub::damaged_map_files` lists the damaged map files, `hub_entries` leaves them out of the new-game list (never played damaged), and `start_map_repair` runs `Hub::repair_maps` on a worker at launch (`App::poll_map_repair` takes it: the restored maps join the list, a failure is said); a map that will not read is skipped with a log line.

**Search** (`ui/hub_view.rs`, Razdor's): a field (`widgets::text_field`, key `hub-search`, placeholder «Поиск…») with a square cross button (`chrome::close_button`),
above the list on a phone held upright, and in the bottom row between «Обновить» and «Установить» elsewhere (a phone
held sideways, the desktop), so the list keeps its rows. On a phone the hub's place line «Хаб: …» is not drawn; the
desktop keeps it under the list. The search narrows the current tab by `matches`: a Russian or English title, or the author, contains the query, ignoring case
(the rating order stays). A tap in the field types; the phone's keyboard comes only with that tap (the field takes
focus on a click, as the dropdowns' filter does), so opening the window never raises it. «Очистить всё» and Esc empty
the field (the cross or Esc clears the search and does not close the window while the field has focus); closing the window drops
the focus. Nothing matching shows «Ничего не подходит к поиску».

**Scrolling** (`ui/hub_view.rs`, Razdor's): the list and the details pane scroll by the wheel, a
one-finger swipe (`widgets::swipe_rows`), a drag (`drag_frame`: a press that moves more than 12 px is a
drag and picks nothing; a press released where it began without a drag picks that row, so a tap picks as
before) and the scrollbar beside them (24 dp wide on a phone). The pane holds the whole entry (picture,
status, the mods it needs, description, the reason an install is refused) and is clipped above the bottom
row, so no text reaches Install or Remove; words too long for its width (a link) are broken there
(`wrap_long`). The pane's scroll starts at the top for each new pick. On a phone held upright (`Lay::narrow`) the pane is a card (Razdor's): the picture (`shown_picture`: `picture`, else `preview`) spans the pane's width up to 45% of its height, its centre band cropped, with a dark fade over its bottom half and the title over it in the title font (`Face::Title`, the largest of 28, 24, 20 units that keeps it on one line, else two, else three lines; no picture: the title centered at the top); under it «Автор: …» between gold rules, tags (kind, version, date, rating, short status) in centred rows, the status note, a diamond rule, then the story and the post's text. Info lines are 15 units and the story 16, drawn with `dt_font::legacy_metrics_if` so the original font's native size does not snap them back. The list's arrows still move one row.

**Stacked mods**: a mod that requires others is laid over them: `layer_chain` orders its `requires` chain
(each requirement after its own, each once, like `plan_install`; a missing one or a circle is refused
with the reason), and `build` lays each layer in turn over one path map, a later layer's file replacing
the earlier one's at the same path (ignoring case). The stamp holds every layer's id, version and file
digests, so a change in any layer rebuilds the folders that lay it.

**The catalog cache** (`hub::write_cache`, `read_cache`, `Hub::cached_catalog`, `Hub::fetch_catalog`; the window's
`HubView::reload`, `show_cache`, `landed`, `keep_pick`). From an `https://` source the last catalog fetched is kept at
`<hub>/cache/index.json` as `{"source": <address>, "catalog": <the index>}`, written atomically by the fetch thread.
Opening the window puts the cached catalog on the screen at once (no «Читаю хаб…» wait) when it was fetched from the
same address and reads back whole (`Catalog::checked`); a missing, corrupt or other-address cache is ignored and the
hub is read as without one. The current catalog is fetched in the background (`fetching`): the same catalog as the one
shown changes nothing, a different one replaces the list and keeps the pick by id (`keep_pick`), the tab, the search
and the scroll. A failed fetch with a list on the screen keeps it and sets `offline` (the line under the list: «Нет
связи: показан сохранённый список»; a phone held upright shows it above the bottom row); with no list, the error is
the list, as before. «Обновить» fetches again and is off while a fetch runs. A folder source and the mock are read in
place and have no cache. Previews are cached per picture (`preview_path`, `<hub>/previews/<sha256>.png`, checked
against the SHA-256 when written and not fetched again); the window's texture follows the picture's SHA-256 too
(`preview_key`), so a refreshed entry's new picture is loaded.

**File names** (`hub::safe_name`, for maps and for each folder of a mod's file path, and the zip's names):
any Unicode except control characters, `/`, `\` and `< > : " | ? *`; not empty, `.` or `..`; no leading or
trailing space; no trailing dot; at most 255 bytes; not a Windows device name (CON, PRN, AUX, NUL, COM1-9,
LPT1-9, with or without an extension). A leading dot is allowed.

**Mods play as layers** (`hub/mods.rs`). `prepare_mod(local, base, id)` builds `mods/<id>/game/`:
every file of the base install (`DtInstall`'s `RAZDOR_DT_DIR`) hard-linked (a copy where links fail),
then each file of `files/` laid over the base file of the same path ignoring ASCII case per component,
keeping the base's spelling (`Data/Units.ini` replaced by `data/units.ini`); a file with no match is
added. A link is removed before its file is replaced, so the base is never written. The build goes into
`game.tmp/` and is renamed into place; `stamp.json` in the folder holds the base path, a digest of the
base's names, sizes and times, the mod's version and the SHA-256 of each mod file (the asked mod's digests come from its manifest, which `damaged_files` has just checked, so they are not hashed again; the requirements' are hashed), and a folder whose
stamp differs is rebuilt (a reinstall, a changed base or file list). The stamp also holds a digest of the folder's own files (`game_fingerprint`: names and sizes), so a folder that lost a file (an antivirus) is built again from the mod's files. A stamp written before fingerprints (1.3.13 and older) matches in everything else: the folder is adopted and its fingerprint written, not rebuilt (a rebuild is a full copy on a phone, and it ran at each start). `hub::active_install_dir(base)`
(called by `DtInstall::from_env` after `locate`, so the whole app reads it: assets, content, maps,
language, `save::set_install`) returns the active mod's folder, else `base`; the choice is
`<hub>/active.json` (`{"mod": "<id>"}`, none = the base). A choice whose mod is gone or does not build
is cleared and the base played. At most two mods' built folders are kept (the owner's decision, 2026-11-10):
`use_mod` writes an empty `mods/<id>/last_played` marker after a mod's folder builds (its modification time is
the record of when it was played), and `apply_mod` calls `hub::keep_recent_game_folders(local, 2)` after the
switch has landed (never before, so the folder in use is not removed). It keeps the two mods played most
recently (a mod with no marker counts as the oldest; ties by id) and always the active mod's folder, even when
it is not among them, so switching between two mods is instant; the base chosen keeps the two newest. A mod's
`files/` stay, so playing it again rebuilds its folder. Each built folder is a full copy of the base where the
hub is on another disk than the install (Windows: install on D:, app data on C:; a phone: about 128 MB), which
is why no more than two are kept. A mod's saves are not in its folder: `save::saves_dir_of(app saves root, game
folder)` keys them by the game folder's path, beside the other installs' saves, and that path is the same
after a rebuild, so removing `game/` never deletes them (test `keeping_recent_game_folders_removes_the_others_and_never_the_saves`). `install::base_dir()` is the base (the folder that imports write to:
the game's update, «Добавить карту…»). Saves: `save::games_dir` keys them by the install folder's path,
so a mod's folder (`…/mods/<id>/game`, named `game`) has its own save folder beside the base's; they
are not in the mod's folder.

**Switching** (the new-game window, `new_game::scenario_select`): when a mod is installed, a «Мод» drop-down
(`view_choice_at`, the extras row under the buttons (1458: `new_game::extra_top`, half the width beside «Добавить карту…», full alone; the hint is «Добавить карту…»'s tooltip); «Мод: нет» then
«Мод: <title>» per installed mod) sets
`MOD_REQUEST`; `App::choose_mod` starts `build_mod` on a worker (1441), which builds the folder first (`hub::use_mod`, one hash pass over the mod's files). Only when that fails is the mod repaired (`Hub::repair_mod`, the one network step) and the build tried again (`ModBuilt::repaired` gives the notice «…restored», `ModFault::Repair` the failure message, and the choice stays); `use_mod` writes `active.json`, then `DtInstall::from_env` and the scenario list are made there too. `App::poll_mod_switch`
(each frame) applies the result on the UI thread (`apply_mod`: `DtArt::load` → `Assets::adopt_install` → content →
scenarios). While it runs the window's mod row reads «Готовлю мод…» and takes no choice, and Next, Enter and
«Добавить карту…» wait (`new_game::start_allowed`): no game starts on a half-built folder.
A mod that does not build shows its reason in `App::message` and the base is played. The class select (`Screen::ClassSelect`, `App::frame`) does not start a map that names units or items the install lacks (`rules::map_refs::unknown`): `Content::unit` would panic on them ("no unit 104"), so the list stays with a message. This is what a mod with damaged files looks like without the repair. Titles come from
`installed.json` (`hub::installed_mods`), not from the catalog, so the picker reads no network.

**Maps by mod**: `hub::MapGate` (read in `ui::hub_entries`, the hub's maps only) gives a hub map its
`hub::MapGroup` by `hub::map_group` (the rule of 1445, 1445b). With no mod played: `Chosen` (the main
part) for the maps that need no mod, else hidden. With a mod played (its `layer_chain`: the mod and the
mods it is laid over): `Chosen` for the maps whose `requires` name one of them; `Base` for a map made for
the base game (no `requires`), which is not hidden but listed under its own group; the rest hidden. A hub
map with the same bytes as an install map (`hub::InstallMaps::has_same`: sizes first, then the bytes) is
hidden, never listed twice. `ui::hub_entries` sets `ScenarioEntry::base_game` for the `Base` maps.

**The install's maps under a played mod** (1445c, `ui::install_entries`): the install's `maps` are read from the
played folder, the mod's merged folder, which holds the base install's maps too. With a mod played, `hub::BaseMaps::origin`
gives each map `MapOrigin::Base` when the base install (`install::base_dir()`) has a map of the same file name
(ignoring case) with the same bytes, else `MapOrigin::Mod` (added or replaced by a mod). `MapGate::install_group` (by
`hub::install_map_group`) sends a `Base` map to the `Base` group and a `Mod` map to `Chosen`; with no mod played all
are `Chosen`. A group moves to «Карты обычной игры» only when every map in it (its chapters too) is `base_game`
(`new_game::is_base_group`), so a campaign moves as a unit and a campaign with a map the mod changed stays on top.

**The base game's group** (`new_game::scenario_select`, 1445b): the list is split into the main groups
and the `base_game` ones (`partition`), each arranged by the filter and the order. With a mod played and
`Base` maps listed, a header row «Карты обычной игры» (`base_header`, a triangle shut or open) follows
the main groups; a tap on it (`widgets::button_state`, the finger area on a phone) opens or shuts it
(`BASE_OPEN`, shut when the list opens and not kept). Shut, its rows are not in `list`; opened, they are
after the header. On a phone the header is at least 40 dp tall (`FINGER_DP`) and its text 13.2 dp (the
mobile check's 12 sp), in two lines when the label does not fit one. A pick of a `Base` map starts it with
the chosen mod as any listed map. Snapshot scene: `RAZDOR_SCENE_BASE_MAPS=end|open` (scroll to the group,
and open it). The install's maps are gated only under a played mod (1445c, above).

**Ticks by mod** (1456, Razdor's): a map's green tick (a campaign: its last map) is kept per mod, `language::Settings::completed_by_mod` (`Won`, the mod's id or `language::NO_MOD`), written by `language::mark_completed(App::active_mod, file)` on a victory. `new_game::marks` reads the whole map once per open of the list, and `facts_of` and the chapters' ticks look up the chosen mod only (`language::won_under`). The old flat list (`completed_scenarios`) is merged into «none» on load and still written, so an older version keeps its ticks.

Leaving the window after a change re-reads the hub's maps into `App::scenarios` (`reload_hub_maps`:
the install is read again while a mod is played, so its folder is rebuilt or the base is back), after
the install's, so they show in the new-game list. Snapshot scene: `hub[:<tab>[:<id>]]`.

## Text

- Razdor's own texts: `razdor::i18n::tr("English text")` and `trf!` with named
  placeholders; the Russian is in `data/lang/ru/*.txt` (`English = Russian` lines). Mark
  texts named in one place and translated elsewhere with `razdor::i18n::n_`. A catalog test checks every
  key is used and translated.
- The original's interface texts are read from the install, never copied:
  `chrome::ui_text`, `chrome::ui_markup` (with the original's `#\ * | @ ^` marks, drawn by
  `widgets::markup_rows`), `chrome::ui_line`, `chrome::options_value` (`[Options]`).
- The language list (`src/ui/language.rs`, Razdor's own): «Русский», «English», then each
  translation of the data in the install (`razdor::dt::english::discover`: a
  `<Prefix>_Units.ini` whose first line is `english::HEADER` and the language's name). The
  choice is a data language, the prefix of its files (`language::data_prefix`: `None` for
  Russian, `Eng`, `Hindi-native`); Razdor's own texts (`razdor::i18n`) are Russian for
  «Русский» and English for every other entry (`language::interface_of`). Saved in
  `settings.json` as "ru", "en" or the prefix (`language::code_of`, `language::initial`; a
  prefix whose files are gone becomes English, `language::resolve`). Nothing saved (empty
  `language`, the first start): the system's language decides (`language::system_lang`, the
  `sys-locale` crate; `language::lang_for_locale`: Russian for ru, uk, be, kk, English for the
  rest, and English when the locale cannot be read), with no cyrillic font English. Nothing is
  written for it, so a later change of the system language still applies until a pick is saved. The main menu's corner
  link and the Screen group's «Язык» button open it (`language::open_list`, the editor's
  drop-down list `widgets::open_list` drawn by `widgets::draw_popup` at the end of
  `App::frame`; while it is open the screen takes no input); `App::follow_language` takes the
  pick (`language::follow_pick`); F2 steps through it (`language::cycle`).
- Translated data texts (Razdor's own, made by `razdor-translate`): `razdor::dt::english`
  reads a language's files next to the Russian ones (`english::data_ini`); English is
  `Eng_*.ini`, else Razdor's copies in `data/lang/en/game/` (`english::bundled`; a file whose
  sections do not line up with the install's is not used). In another language than
  Russian, `chrome::ui_text` and `chrome::ui_markup` take that language's text first, then
  English; the install's unit, item and spell names and descriptions are swapped in by
  Russian text, that language's first, English for the rest (`Content::from_dt_in`,
  `DtInstall::dictionary`; on a change `App::follow_language` retitles the content and the
  game under way with `Content::retitled`, and a battle on the screen with
  `BattleView::retitle` / `Battle::retitle`, since a battle copies its fighters' names at its
  start). The Russian data is always loaded as it is:
  upgrades match units by their Russian name.
- Fonts: the settings' «Шрифт» (`dt_font::FontMode`): the install's bitmap sheets by default,
  or the bundled TrueType faces (`widgets::load_font`); see `assets-and-drawing.md` §Fonts.
  Pick a face with `dt_font::with_face`; measure with `widgets::measure`, since widths
  change with the mode.

## Settings

`ui::audio::Settings` (`src/ui/audio.rs`, saved as `audio.json` in the save folder, all
fields `#[serde(default)]`) holds every setting: volumes, display, scale, battle AI level,
row width, and Razdor's own options (`updates`,
map zoom, `leader_caption`: the leader's name under the event picture of a meeting, see
`story::leader_caption` and `EventOutcome::MeetingArmy`; `chronicle_summary`, «Итоги после карты», on
by default: the «Итоги» page after a map ends or the hero falls, in the Other group). The windows are `main_menu::options` / `main_menu::options_window` (the
original's: five slider rows, eight checkboxes, «Принять» keeps, «Отмена» restores what the
window opened with) and the five groups (`main_menu::Group`) as tabs of one settings window (`TABS`; the pane is `group_pane`): Razdor's own rows, each in its group (`Group::rows`, the `Row` enum; a test checks that every row is in exactly one group). The settings window (2026-11-09: one window, the owner's design) has its tabs in a column at the left (`tab_buttons`, the selected one drawn pressed): «Основное» (the General tab: the original's two volume sliders (`slider_row`) and eight checkboxes, `general_desktop`, and on a phone the touchpad mode after them, `general_phone`), then «Карта», «Бой и армия», «Экран», «Управление» and «Прочее» (the groups). `GROUP` is the tab shown (`None`: General). «Принять» and «Отмена» at the bottom right act on everything changed in any tab: Cancel (the close box, Esc) puts back what `OPENED_WITH` holds, Apply keeps it (`options_window`). `open_group` selects a tab, `open_keys` selects «Управление» and opens the keys window alone (as before). The «Размер нижней панели» row (`Row::BottomBar`) shows only on a phone held sideways (`bottom_bar_row_shown`: upright the bar is the main's size, so the row did nothing; a player's report). The long hints of a row (`Row::help_text`: the zoom, the map's edge, the battle AI, the army arrangement, experience on promotion, the language, the updates) sit behind a «?» button at the row's end (`help_button`): a click pins the pop-up (`HELP_PINNED`), the desktop's pointer over it shows it too (`HELP_HOVER`); a click elsewhere or Esc closes it (`help_frame_start`), and the game's hint box draws it (`help_popup`, `widgets::hint_box_always`). `RAZDOR_SCENE_HELP=<row>` pins one in a snapshot. The groups: «Карта» (map zoom and its lock, with the zoom's slider as a row of its own (`slider_row_at`, its value as the label), the map's scroll speed (`Row::ScrollSpeed`), 2.5D view, sprite size, two-finger turn, extending the map past its edges, battles on the map, minimap army list, leader's name, minimap corner and reset), «Бой и армия» (the hero's walking speed and the battle animation speed, the sliders moved from the General tab (`Row::WalkSpeed`, `Row::AnimSpeed`, backlog 1591), battle AI strength (`main_menu::AiStrength`, kept in sync with the `OptValue9` checkbox), unit battle participation, keep formation, max guards casters, army arrangement, experience on promotion), «Экран» (screen mode, interface scale, font, text size, large message windows, screen orientation on Android, language), «Управление» (keyboard shortcuts; the touchpad mode is on the General tab, on a phone), «Прочее» (updates and «Проверить», the game import). Razdor's map options «Битвы армий на карте» (`battle_marks`, on) and «Список у
миникарты» (`army_list`, off) are pushed into `Game::battle_marks` / `Game::army_list` by
`follow_settings`, like the rule options. The battles' mark and notice are drawn in
`world_view::map_frame` (`draw_clash_marks`, `clash_notice`, from `Game::take_clashes`); the
army list is `minimap::army_list`, drawn under the minimap window and clicked like its own
centring (`view.look`); it takes the room above the bar and scrolls with the wheel.
The world map's drawing resolution «Разрешение карты» (`map_render_scale`, 100%, 75%, 50%; `map_scale`, see
`assets-and-drawing.md`, «The map's resolution») is always shown, right after the map zoom row. The first row of the Map group is «Качество графики» (`Row::GraphicsQuality`, `ui/quality_preset.rs`): a click steps Оригинал, Низкое, Среднее, Высокое and back to Оригинал (from «Своё», to Оригинал). `quality_preset::set` sets the resolution, the light and its quality, the shadows, the mist, the water (on, its rate and quality), the weather, the living map and the map animation at once. Nothing stores the choice: `quality_preset::current` gives the first preset whose values all hold in the settings, or «Своё» (`None`) when none does, so a hand change shows «Своё» and an install keeps its settings. A fresh install has `Settings::default()`, which is the «Оригинал» look (a test checks it). «Ограничение FPS»
(`frame_cap`, `Settings::frame_cap`: off, 30 or 60; `frame_cap::next`) follows it. `apply_options` pushes it into
`frame_cap::set_setting`, and the loop (`main.rs`, right after the frame is presented) calls `Pacer::pace` with `frame_cap::current()`:
the frames start no closer than 1/cap s apart, the sleep is the rest of the period less 1 ms and the last 1 ms is
spun, yielding the thread, and a frame over the period is never slept. Snapshots and the trailer never sleep. The
profiling switch is env `RAZDOR_FRAME_CAP=30|60` (desktop) or the files `profile-frame-cap-30` / `-60` (Android).
On Android `set_setting` writes the cap to `frame-cap` in the app folder when it changes; the activity reads it
(`razdorFrameCap` in `android/MainActivityInject.java`) and asks the window and the surface for the cap's rate
(the lowest mode at least that fast) instead of the top one.
**Battery saver** («Экономия батареи», `Settings::battery_saver`, off by default, row in «Прочее»; `frame_cap::set_battery`
from `apply_options`). `world_view::note_idle_frame` (after each map frame, from `frame`) sets the idle flag: no dialog,
no walk or wait, no showing, glide, turn or toast, no press, key, wheel or pointer move this frame, and the camera and
pointer where the last frame left them. `frame_cap::current()` then gives `battery_cap`: the idle frame rate
`IDLE_FPS` (10) instead of the cap after an idle frame, so the map's animations tick slowly and the frames in between are
not drawn. Other screens never set the flag, so they keep their cap. The ui-regression and the mobile checks show the row.
With «Анимация карты» Выкл (`map_anim::off()`) the same idle rate applies without the saver: a still map needs no more.
The world map's 2.5D settings are in the same window, after «Имя предводителя»: «Вид карты»
(`map_tilt`, always shown), and «Размер фигур» (`map_sprite_size`) and «Поворот двумя пальцами»
(`map_twist`, `gestures::set_twist`) only with the 2.5D view on. `apply_options` pushes all three
every frame (see `assets-and-drawing.md`, the 2.5D view).
The option «impossible difficulty» is read at start only, as the original's (the content's
difficulty factor in `App::new`). The original's «no music and sounds» (`OptValue4`) is removed
(the owner's decision, 2026-11-10): `Audio::new` always loads the sound, volume 0 silences it.

«Расстановка армии» (`unit_drag::Controls`, saved as `formation_controls`: `clicks`, `drag`,
`both`; pushed in by `main_menu::apply_options` → `unit_drag::set_controls`) is a Razdor
setting, the owner's decision after players' reports, with three choices. «Оба», the default
also for older files (and the old `original` value, which had both), is the original's press
to pick, press to move (interface.md §9.6) with dragging on top: a press on a card starts a
drag (`unit_drag::press`, `unit_drag::follow`). «Щелчками» (`Controls::Clicks`) is the
original alone: `unit_drag::press` arms no drag, so nothing sticks to the pointer. «Перетаскиванием»
(`Controls::Drag`) makes a press on a card only select or deselect the selected unit
(`unit_drag::grid_press_in`, `building_view::garrison_click`); units move by dragging, and the
card hints say "click to select" (`items_view::pointed_line`). The same choice covers units in
the army window (`items_view::squad`), the building window's hero grid and the garrison tab,
and items. Battle is not affected.

### The window's display mode

`display::follow_settings` → `display::follow` (first thing in `App::frame`) puts the window in
the setting's mode through each system's own window (`display::sys`: X11, Win32, AppKit);
miniquad has no borderless mode. On Windows every step is logged to `razdor.log` ("display:
…": the window's rectangles, styles, visible / active), the only way to see a player's case.

- Windowed start on Windows: an OpenGL window never resized since it appeared may stay black
  under the compositor, so `display::NUDGE` grows it 1 px at 0.3 s after the first frame and
  puts it back at 0.8 s (`nudge_due`, one step per frame, frames presented at the grown size;
  skipped when the player maximized, minimized or resized it meanwhile).
- Borderless: a `WS_POPUP` over the monitor, one pixel taller (`popup_rect`), so Windows and
  the drivers do not take it for an exclusive full-screen application (task switching).
- Full screen: a `WS_POPUP` exactly over the monitor, topmost only while it is the active
  window; when another window becomes active (Alt+Tab, Win+Tab) it goes to the bottom, and
  back to topmost when activated again (`layer`, `sys::Window::keep_layer`, every frame).
- Alt+Enter cycles the modes on any screen (`hotkey_pressed`, `notice`).
- Android (`sys::Window::apply`, the fallback module): full screen calls `android::set_immersive`
  (`razdorSetImmersive` in `android/MainActivityInject.java`), which hides the status and
  navigation bars (a swipe from the edge shows them for a moment). Windowed hides the status
  bar only, as the fullscreen theme did in 1.0.3, and shows the navigation bar.
  The activity re-applies it when the window regains the focus. The activity also asks for the display's
  highest refresh rate (`razdorPreferTopRefreshRate`, on create and on each focus gain): in the phone's dynamic
  refresh mode Android gives an app 60 Hz unless it states a preference. The manifest marks Razdor as a
  game (`android:appCategory="game"`, Cargo.toml), and the drawing surface votes for the same top rate
  (`razdorWatchSurfaceFrameRate`, on each new surface; API 30+, plus the game category on API 35+), so
  adaptive refresh (OnePlus's ADFR) keeps 120 Hz without a touch. On a phone the «Экран» row
  cycles only Windowed and Full screen (`DisplayMode::next_on`); a saved Borderless is Full screen.

Rule: a parity setting reads the install's value until the player chooses
(`Option<bool>` = `None` → the install's `[Options]` value such as `OptValue9`, see `main_menu::expert_ai`). A Razdor
extra defaults to the original's behaviour and its label says which choice is the
original's ("Battle (original)").

## Keys

The shortcuts' keys live in `src/ui/keymap.rs`: an `Action` per rebindable key (the map's
time, the windows' keys, F1, F2, the saves, N), each with today's key as its default.
`keymap::key(action)` is the only read; a screen checks it where it checked the literal key
before, with its own guards. `keymap::set` swaps with a key another action has (the endless
wait and the quick save may share F5). The overrides are saved as (action, key) names in
`Settings::keys` (`audio.json`): `keymap::load` at start, `keymap::overrides` each frame. The
window «Клавиши» (`main_menu::keys_window`) opens from the Controls group: a click on a key
waits for the new one (`keymap::listen`); while it waits no shortcut fires. The F1 list fills
its `{Action}` tokens with the keys (`hotkeys::fill_keys`). Fixed keys (dialogs, arrows, +/-,
0, Space, the editor's) stay literal.

**The F1 list as buttons (an experiment, main only).** `hotkeys::tiles(place)` makes a tile of
each key of the list's rows: an `{Action}` (its bound key), or a literal key (Esc, Enter, Backspace,
a digit, a letter when the row is that letter alone); F1 and the endless wait make none. A row's
words split at " / " when it has as many parts as the key label, else the row's words serve each
tile. Mouse and typing rows (`hotkeys::info_rows`) stay text lines under the grid, and EVERYWHERE's
question keys (Y, Enter, N, Esc) are no tile. A tap closes the list and calls `input::tap_key(key)`:
the key is queued and on the next frame `input::is_key_pressed` and `get_keys_pressed` report it
once, so the screen under the list runs its action as if the key were pressed (the queue is
`input::TapQueue`, consumed in `begin_frame`). A tile the rules forbid now is drawn disabled
(`hotkeys::tile_enabled`, `allowed`). Phones have no F1: the game menu's «Клавиши» button
(`saves::ExitChoice::Keys`, drawn by `exit_dialog` on a phone only, `display::dp_px()`; the desktop
menu is the original's four buttons; the phone's five take 4 px between them) opens the list over
the map; `App::help_fresh` keeps the press that opened it from closing it, and `widgets::swallow_input` (set at both
opening points, the map's and the battle exit window's «Клавиши») keeps it from pressing a tile in the list. On a phone the list's
text is the phone's text (`display::text_for`, drawn under `dt_font::legacy_metrics_if`) and its
tiles wrap to three lines; the desktop keeps the interface scale. On a phone the keys that have a button on the
screen fold out of the grid (a phone choice, not in the original): on the map the wait, minimap, centre, journal,
spell book, hero and army buttons and Esc; in battle Space, Q, W and Esc (`hotkeys::has_screen_button`,
`hotkeys::split_screen_buttons`). They sit in a list under the grid, behind a full-width toggle button («Показать»
/ «Скрыть клавиши, для которых есть кнопка на экране»); the list is collapsed when the window opens and closes
with it (`SECONDARY_OPEN`, reset with `SCROLL`). The toggle counts as a control (a tap on it keeps the window open)
and runs no key. The desktop shows every key in the grid, with no toggle. Snapshot:
`RAZDOR_SCENE_OPEN=<frame>:keys`.

Esc is read through `widgets::key(KeyCode::Escape)` (the guarded read) or `widgets::esc_pressed()`
(raw, for the direct reads). Both also take Android's system Back (`KeyCode::Back`, as miniquad
delivers it; `widgets::is_escape`): a new Esc check goes through these, never a bare
`is_key_pressed(KeyCode::Escape)`. Back is never in the key tables (`keymap::bindable` is false for it)
and, like Alt and F10, never the held key (`widgets::is_system_key`).

## Photo mode (the player's, 2026-11-10)

Photo mode (`src/ui/photo.rs`) freezes the map and frees its camera for a picture. It starts from the
game menu's «Фото» (`saves::ExitChoice::Photo`, a fifth button of the menu's row on the desktop, and
on a phone the row of six, its labels wrapped by `chrome::marble_button_wrapped`) or from the photo key
(`Action::Photo`, P, rebindable). Either sets `photo::request()`; `world_view::map_frame` takes it on the
next map frame, with no dialog open, and `enter_photo` keeps the map's look and starts the camera where
the play's looks (its pitch and turn), so leaving gives the view back: the play's zoom, pitch and turn are
never written, only the look is restored (`photo::leave` returns it).

- **Nothing advances.** `map_frame` runs `photo_frame` instead of `world_frame` (the old body, unchanged),
  and `photo_frame` returns before any tick, event, wait or AI move. `photo::may_tick()` is false in the
  mode: the world frame's own tick gate checks it, and `App::frame_content` does not finish a paused AI
  step on a press in the mode (it would advance the game). The idle map's animations (wall clock) keep going.
- **The camera.** `Camera::looking_in` builds `TiltCamera::free` at the mode's pitch (8° to 89.9°, the camera's
  own top-down limit) and yaw, whatever the view setting: the flat map is tilted in the mode. The zoom is the
  mode's own, `photo::zoom_bounds` (from the play's whole-map floor halved to twice the wheel's top). Bar height is
  zero in the mode (`bar_h`), so the map fills the window. The fog, the sprites and the first-person and bent
  views are as in play; the mode never offers the bend.
- **Input.** `photo_frame` takes a left drag (pan, `photo_pan`), a right drag (yaw by `DRAG_YAW`, pitch by
  `DRAG_PITCH`), the wheel and +/- (zoom), Q/E and PageUp/PageDown held (yaw and pitch rates), the arrows held
  (pan), and on a phone the one-finger swipe (`input::map_pan`), the two-finger twist (`input::map_turn`, raw
  radians, from `gestures::GestureMapper`'s `Frame::turn`) and the two-finger slide (`input::map_pair`, the mean
  slide, `Frame::pair`). `gestures` sets `twist_on` in the mode whatever the setting. A tap (a left press and
  release without moving) brings a hidden strip back. Esc with the strip shown, P, and «Выход» leave; Esc with
  it hidden shows it again; Space and Enter (and «Снимок») shoot.
- **The strip.** `photo::layout`: a panel at the bottom left on the desktop, a row of three 48 dp buttons on a
  phone (labels at 15 sp, `draw_strip`). «Скрыть» hides the strip for a clean shot. The strip's rectangles are
  swipe areas that take no pan (`SwipeKind::Off`).
- **The shot.** The shutter's picture is read (macroquad's screen read, which flushes the frame first) after the world
  is drawn and before the strip and the notices. `photo::upright` reverses the rows (the screen read is bottom
  row first, as the recorder's `Sinks::write`) and makes the alpha opaque, and `photo::encode_png` (the `png`
  crate) writes it to `platform::app_dir()/photos/razdor-YYYY-MM-DD-HH-MM-SS.png` (`-2`, `-3` for a second shot
  in the same second). Local time on unix (`libc::localtime_r`), UTC elsewhere. The notice names the file and,
  on the desktop, the folder. On Android the folder is the shared `/storage/emulated/0/Razdor/photos` (no gallery
  export; the root's `.nomedia` keeps it out of the media scanner).
- **Snapshots.** `RAZDOR_SCENE_PHOTO=1` asks for the mode at a scene's start (`photo::snapshot_request`);
  `RAZDOR_SCENE_PHOTO_PITCH`, `_YAW`, `_ZOOM`, `_STRIP=0` and `_SHOT=1` set its start (`photo::snapshot_start`).
  The regression cases are `photo-*` in `tools/ui-regress/scenes.txt` (no shutter case: its notice has the time).
- **Tests.** `ui::photo::tests` (the limits, the drag's slop and its turn, the strip's phone targets, the stamp,
  the file name, the flip, the PNG round trip, the entry and leave with the look given back, the pause gate).

## How to add a window

1. Add a `Screen` variant (or a field on an existing view) in `src/ui/mod.rs`.
2. Write `fn frame(...) -> Option<Screen>` in its own module: draw with `chrome::window`
   (over the map: centre above the bar), handle input with `widgets`, return the next screen.
3. Route it in the `match` of `App::frame`; add open/close bookkeeping in the screen-change
   block if the original does something there.
4. Hotkeys in `hotkeys.rs` (`Place`), its texts in `data/lang/ru/`.
5. A `RAZDOR_SCENE` for it in `src/ui/snapshot.rs` so it can be seen offscreen
   ([testing-and-tools.md](testing-and-tools.md)).

## How to add a setting

Add a `#[serde(default)]` field to `ui::audio::Settings` (and to its `Default`), a checkbox
or slider in `options_window` (an original option) or a row of a group window (`Row`, `Group::rows`; a
Razdor one), a translation line, and push it into `Game` at
the top of `App::frame` if the rules need it. Old `audio.json` files keep loading.

## Traps

- Draw order is call order: anything drawn after `dialog::draw` is over the dialog.
- `thread_local!` state in `chrome`, `widgets`, `cursor`, `input` is reset per frame by
  their `begin_frame`; do not keep per-window state there — put it in the screen value.
- A screen function runs every frame; expensive work (decoding art, reading saves,
  path-finding) must be cached or done ahead (see the profiling recipe in
  [testing-and-tools.md](testing-and-tools.md)).

<!-- refs verified by: python3 -I docs/flows/verify.py docs/flows/*.md -->
