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
   `ui::chrome::set_install` (starts decoding window art ahead, see
   [assets-and-drawing.md](assets-and-drawing.md)), `razdor::rules::save::set_install` and
   `razdor::rules::save::warm_meta_cache` (see [saves.md](saves.md)).
3. `App::new` (`src/ui/mod.rs`): the install's `Content` (`Content::from_dt`), every map of
   the install parsed into `ScenarioEntry`s, `Audio::new` (settings are loaded here).
4. `ui::snapshot::stage` (only with `RAZDOR_SCENE`), then the loop: `App::frame`,
   `App::draw_fps`, `ui::cursor::draw` (the pointer is drawn last, over everything), and
   `next_frame().await`. Quitting goes through `exit_now` (it skips C `atexit` handlers,
   which crashed on exit).

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
the poll (`App::mobile_install_poll`, `ui::mobile`: the `RECHECK` / `NEXT_POLL` helpers, the 5-s
look for the install, `storage_granted` = always true on iOS) are shared with Android; the JNI parts
stay in `ui::android` (the shared helpers in `src/jni.rs`). The on-screen keyboard is on for both (`widgets::set_keyboard`). The
update window (`ui/update_view.rs`) offers a release on the phones too: Android installs it through the system
installer (`update::poll_installer` takes its report each frame: a cancelled session brings the offer back, a failed one shows why; the APK is kept as `razdor-<X.Y.Z>.apk` in the cache folder, reused by the next try, removed after a successful install and at start when it is not newer than the running version), iOS opens the release page; the Discord link opens through `ui::mobile::open_url` (see the main menu above). `ui/script_font.rs` skips the fontconfig lookup on iOS, as on Android.

The APK is built by `scripts/dist-android.sh` (cargo-quad-apk, pinned in its header; NDK 29;
see `[package.metadata.android]` in `Cargo.toml`). The launcher icon is `android/res/mipmap-*/ic_launcher.png`
(Razdor's own icon, `data/icon/razdor.png` resized to 48–192 px; `res` and `icon` in `Cargo.toml`).
Per-platform paths live in one place:
`src/platform.rs` (`app_dir` for saves, log, editor maps, translation jobs; `cache_dir` for the picture cache;
`config_dir` for the remembered install; `install_roots` for the search). On Android they are
`/storage/emulated/0/Razdor` (survives an uninstall) and the search starts with `DiscordTimes`
in internal storage; desktop keeps the platform's data and config folders.

Reading that storage needs the all-files access. `src/ui/android.rs` calls the activity's
methods through miniquad's JNI (`attach_jni_env`, `ACTIVITY`): the all-files settings page
(Android 11+: the app's own page, else the general all-files list, else the app's details) or
the runtime permissions (8 to 10), and `storage_granted`. The title screen's not-found block
(`screens::android_not_found`, scaled to the screen, its buttons at least 12% of its height)
has «Выдать доступ» and «Проверить снова». The first frame of a run without an install opens the
access page by itself, once (`mobile::ask_access_once` → `android::ask_once`). The main menu shows the
same block while no install is found (`screens::menu_not_found`, drawn by `main_menu::frame(no_install)`
in the column left of the buttons, stacked, over a dark panel, at the interface scale; the column is the
left half of the screen, in either orientation). The block is the same on the desktop (a Razdor extra, not
the original's): it says that Razdor plays the player's own copy of the game (1.8 with Community Update
1.2), gives the steps, and offers «Как установить» (`main_menu::install_guide`: the website's install guide,
`install.html` or `install-en.html` by the interface language) and «Где взять игру» (`main_menu::DISCORD_INVITE`).
Both links, and the bottom «Discord» link, go through `main_menu::open_link`: `xdg-open` / `explorer` / `open`
on desktop, `ui::android::open_url` (an `ACTION_VIEW` intent) on Android, `ui::mobile::open_url`
(`UIApplication openURL:`) on iOS. The two link buttons share a row when the stack does not fit the height.
While it
is shown, `App::mobile_install_poll` (`ui::mobile`; on the main menu and the scenario list; every 5 s
once the permission is granted, and on the button) loads a newly found install with `Assets::adopt_install` and the `ui/mod.rs` helpers
`dt_content_of` / `scenarios_of`, without a restart. Its Discord link opens through `open_url` like the desktop's (see the main menu above). The log cannot be opened before
the access (the folder is not writable), so the poll calls `diag::reopen_log` once it is granted.
Game archives on the not-found block (`dt::unpack`, `ui::mobile`, every platform): `look_for_archives`
scans `platform::archive_dirs()` (the phones: the install folders, Download, the storage root; the desktop:
the program's folder except on macOS, Downloads, the install folders) at once after «Check again» or the
unpacking's end, else every 5 s. `unpack::find_archives` lists each `.7z` / `.zip` there without extracting:
a game when the listing passes `install::listing_is_install` after dropping one top folder, a patch when it
holds some of `install::is_install_path`'s files, else ignored. `not_found_rows` offers «Распаковать архив
игры» while a game is found and nothing runs; `start_unpack` writes the archives on a thread into
`platform::unpack_dir()` (Android `DiscordTimes` in the storage, iOS `Documents/DiscordTimes`, macOS
`DiscordTimes` in `app_dir`, elsewhere next to the program), the game first, the patches over it. While it
runs, `mobile_install_poll` does not load the install; its end (`unpack_tick`) asks for it (`ask_recheck`),
and the same poll loads it (the desktop has no 5-s poll: that reload is its only one). Snapshot fakes:
`RAZDOR_SCENE_ARCHIVES`, `RAZDOR_SCENE_UNPACK`.

Updating the game from an archive («Обновить игру из архива…», the settings window of the main menu,
`ui/game_import.rs`, the pure part `dt/game_import.rs`). `main_menu::options_window` draws the row
through `game_import::settings_row`: the button, or why it is not offered (over a map, on iPhone, or
when the install is not the phones' unpack folder: `game_import::target` returns it only for that
folder or with no install yet, so a player's own copy is never written). The button sets a request;
`game_import::tick` (in `frame_content`, every frame) opens the platform's picker (the same one as
«Добавить карту…»: Android's `pick_outcome`, the Java copy by streaming into the cache). The picked
archive is `dt::game_import::examine` (listing only, the budget `over_budget`): nothing of the game in
it is a refusal, nothing written. The declared sizes are not trusted alone: `unpack::unpack` caps the bytes
written at the declared total or `MAX_UNPACKED_BYTES`, and `unpack::write_file` reads each entry no further
than its declared size, so a zip bomb stops with an error (the side folder is removed, the install kept). A whole game asks first (`Modal::Confirm`) when the install is
there; a patch goes straight in, and is refused without an install. `dt::game_import::import` unpacks
into `<install>.new` (beside the install, the same disk), then `swap_in` renames the install to
`<install>.old`, the new folder to the install's name, and deletes the old one (a failed second rename
puts it back); a patch's files are moved over the install's (`move_over`). Any failure removes
`<install>.new` and leaves the install as it was. The job runs on a thread (`Job`, the progress
window); when it ends, `tick` returns true and `App::reload_install` reads the install again
(`preload::forget_ready` first: the decode store is keyed by path alone, and its loads started
before the call are not kept, the generation). The saves (`platform::app_dir`) are never beside
the install, so they are not read or written. `dt::game_import::recover` (`main`, before the install
is looked for) puts back an install an interrupted swap moved away. While the import's windows are up
the settings under them are only drawn (`main_menu::backdrop`, `game_import::busy`). Snapshot fake:
`RAZDOR_SCENE_IMPORT` (with `RAZDOR_SNAPSHOT`) shows the row as offered.

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
button there too. Hints: `input::hint_hover()` gates every hover hint and item description (`widgets::tooltip`,
`widgets::tooltip_later`, the bar's hint, the backpack and hero window's item descriptions). In gesture mode
they show only while the long press holds; in the touchpad mode and with no touch, as `input::touch_hover`
(no hint after a lift until a finger comes down). Swipe areas: each frame the screens register the rectangles a one-finger swipe acts on
(`input::swipe_area(rect, SwipeKind)`, gestures only; `widgets::swipe_rows` for a list or text box with
its row height). The map registers `Pan` over the map view (not while a dialog is open; the open minimap
is `Off`, a blocker that keeps its drag a drag). The registrations of a frame are the next frame's
(`input::begin_frame` swaps them), the last one registered over the touch acts. Drag sources: the unit grids (the army window, the building's army and garrison, the hire tab's hero grid) register their cards as `Off` (`pan::drag_grid`, `pan::drag_source`) while the canvas overflows and units drag, so a quick slide from a card drags the unit instead of panning the window (the window's own `Pan` area is registered first). A finger that moves past
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
the next frame. The keyboard strip (`widgets::draw_keyboard_strip`), the active spells line on the map
(`world_view`, the notes under the minimap) and the FPS counter (`App::draw_fps`) all sit inside it.

Portrait (a phone taller than 4:3): the width sets the interface scale, so the 960×720 canvas is shorter
than the screen. `chrome::centred_top` centres it vertically, only on screens narrower than 4:3 (a 4:3 or
wider screen, the desktop's and the landscape phone's, gets 0 and is never moved). The title screens are
placed from `chrome::canvas_top()`: the logo and the menu (`main_menu`), the scenario list and the new game
and custom battle windows. The map and building windows are already centred over the view above the bottom
bar (`window_layout::window`, `main_menu::map_window`, the saves and dialogs), so they are not moved again.
The battle window held upright (`battle_view::Layout::upright`, pure, with unit tests): the window's scale comes from
`window_layout::game_scale` (the width less 8 px, above the bar); the unit's band (`unit_sheet::draw_band`) on top at
design y 76, then the quick buttons, the enemy's grid, the strip, the player's grid; the close cross is its title bar's, inside the taller strip (see the close crosses below). The landscape phone window is placed below the safe top plus the strip's growth (`battle_view`'s `Layout::landscape`, `chrome::phone_title_grow`). The map, the bar and the exit window are drawn at the interface scale (`display::with_window_scale(None, …)`).
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
the right side (pack, formation) under it, and a «Закрыть» button under the formation (`army_close`). The
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
column, never above the safe inset (the column moves down if it must); the links (map editor, custom battle, Discord, language) are chips
in one row at the bottom, touch targets of 48 dp; the version and copyright lines and an installed update's
notice sit just above the chips; the not-found block (no install) sits above the logo. Landscape and desktop keep
the original column and the corner links (`main_menu::landscape_buttons`, `corner_links`).
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
`options_window` over the map), «Ещё…» (`advanced_window`), the keys window (`keys_window`), the save window (`save_screen`,
the name field's keyboard lift applies to the scaled rect), the load window (`load_screen`), and `map_window`, which
centres them over the map. Sideways on a phone these take the title's rule (`window_layout::sideways_window`, the same
scale and rectangle as the main menu's window, a phone choice: the windows were small in landscape, playtest 1365).
Over the map they use `window_layout::sideways_game_window`: the same, but never over the bottom bar; where the title
rule's floor (the base scale) would make the window taller than the room between the top inset and the bar, it scales
down to that room and stays centred there (the advanced settings, 900×650, at 2400×1080 and 2.625 px per dp). The map and the bar under them are drawn at the interface scale
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
Its bottom row is inside that rectangle. The minimap (`minimap::window`) in
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
on mobile, set by the options window's last checkbox there, which makes the window 28 px taller
on mobile): the screen is a laptop trackpad, instead of the gestures. `TouchMapper::step` is pure and turns the frame's touches (`TouchPoint`, from
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
else no touch. The mappers' tests cover each gesture. In this mode the event and victory windows'
item and spell icons show their description while the cursor rests over them (`dialog::shown_preview`),
as the backpack's hover; the original shows it only while a button is held (0x4c1fec / 0x4c2048).
The rest is `ui::rest` (`rest::tick` in `App::frame`, `rest::rested()`: the pointer still for
`REST_SECS`, 0.35 s, the gestures' long-press delay, with no button down); the map's tooltip of an
army or building (`world_view`) and the icons above both wait for it in this mode.

## The frame

`App::frame` in order:

- per-frame resets: `input::begin_frame`, `chrome::begin_frame`, `cursor::begin_frame`,
  `widgets::track_held_key` (the original's one held key: any key up clears it, and the
  system's repeats of the newest key still down hold it again, 0x475820); Alt+Enter display mode (`display::hotkey_pressed`);
- settings pushed into the game each frame (`main_menu::follow_settings`: `g.improved_ai`,
  `g.max_ai`, `g.friends_let_pass`, `g.pass_on_bridges`, `g.pursue`, `g.walk_pace`; animation
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
stays live and lights the window's button). The bar's flag (`BarButton::Hero`) opens the
original's hero window (`Screen::Hero`, `items_view::hero_window`: the hero's panel, the pack,
the item description, the four resource lines and the diary); J opens the same window. The
diary keeps the original's look; in place of its list title it has four tabs (asked for by the
owner): the quests under way (the original's diary), and Razdor's history of completed quests,
rumours and messages over the whole campaign, with dates and a heading per campaign map
(`items_view::HeroView`, `story::journal_lines`). On a phone (`display::dp_px`), while a diary
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
install's `[Skills]` labels (`spell_badges::label`).

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
  (`display::notice_scale`), and swallows the key so the world map's 0 does not fire. The bottom bar, the FPS counter and the keyboard strip use `chrome::bar_k`, the chosen
  scale cut to the fit (`display::fitted_scale`), so they stay as large as the fit allows.
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
  windows' height (normal and village), and their inner frame and content move down by it. `display::dp_px` is
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
  turned phone or a changed settings slider moves the zoom at once.
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
  prefix whose files are gone becomes English, `language::resolve`). The main menu's corner
  link and the advanced settings' «Язык» button open it (`language::open_list`, the editor's
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
  game under way with `Content::retitled`). The Russian data is always loaded as it is:
  upgrades match units by their Russian name.
- Fonts: the settings' «Шрифт» (`dt_font::FontMode`): the install's bitmap sheets by default,
  or the bundled TrueType faces (`widgets::load_font`); see `assets-and-drawing.md` §Fonts.
  Pick a face with `dt_font::with_face`; measure with `widgets::measure`, since widths
  change with the mode.

## Settings

`ui::audio::Settings` (`src/ui/audio.rs`, saved as `audio.json` in the save folder, all
fields `#[serde(default)]`) holds every setting: volumes, display, scale, battle AI level,
row width, and Razdor's own options (`friends_let_pass`, `pass_on_bridges`, `updates`,
map zoom). The windows are `main_menu::options` / `main_menu::options_window` (the
original's: five slider rows, eight checkboxes, «Принять» keeps, «Отмена» restores what the
window opened with) and `advanced_window` («Ещё…»: Razdor's own options, the screen, scale, the battle AI strength (`main_menu::AiStrength`, kept in sync with the `OptValue9` checkbox),
font and language). Razdor's map options «Битвы армий на карте» (`battle_marks`, on) and «Список у
миникарты» (`army_list`, off) are pushed into `Game::battle_marks` / `Game::army_list` by
`follow_settings`, like the rule options. The battles' mark and notice are drawn in
`world_view::map_frame` (`draw_clash_marks`, `clash_notice`, from `Game::take_clashes`); the
army list is `minimap::army_list`, drawn under the minimap window and clicked like its own
centring (`view.look`); it takes the room above the bar and scrolls with the wheel.
Two options are read at start only, as the original's: «no music and
sounds» (`Audio::new`) and «impossible difficulty» (the content's difficulty factor in
`App::new`).

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
  The activity re-applies it when the window regains the focus. On a phone the «Экран» row
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
window «Клавиши» (`main_menu::keys_window`) opens from the advanced settings: a click on a key
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
or slider in `options_window` (an original option) or a button in `advanced_window` (a
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
