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

## Android (experimental)

The APK is built by `scripts/dist-android.sh` (cargo-quad-apk, pinned in its header; NDK 29;
see `[package.metadata.android]` in `Cargo.toml`). Per-platform paths live in one place:
`src/platform.rs` (`app_dir` for saves, log, picture cache, editor maps, translation jobs;
`config_dir` for the remembered install; `install_roots` for the search). On Android they are
`/storage/emulated/0/Razdor` (survives an uninstall) and the search starts with `DiscordTimes`
in internal storage; desktop keeps the platform's data and config folders.

Reading that storage needs the all-files access. `src/ui/android.rs` calls the activity's
methods through miniquad's JNI (`attach_jni_env`, `ACTIVITY`): the all-files settings page
(Android 11+: the app's own page, else the general all-files list, else the app's details) or
the runtime permissions (8 to 10), and `storage_granted`. The title screen's not-found block
(`screens::android_not_found`, scaled to the screen, its buttons at least 12% of its height)
opens the access page by itself once per run (`android::ask_once`) and has «Выдать доступ» and
«Проверить снова»; while it
is shown, `App::android_install_poll` (every 5 s once the permission is granted, and on the
button) loads a newly found install with `Assets::adopt_install` and the `ui/mod.rs` helpers
`dt_content_of` / `scenarios_of`, without a restart. Android has no self-update (`update::asset_name` is None) and
no Discord link (it needs a browser opener): both are hidden there. Touch: miniquad simulates
the left mouse button from a touch (`simulate_mouse_with_touch`), so a tap is a left click;
there is no long-press right click. Gamepads are off (`--no-default-features --features audio`).
The on-screen keyboard follows the text fields: `edit_line` / `edit_rows` drawn active ask for it,
and `widgets::sync_keyboard` (once a frame, before `next_frame`) shows or hides it with miniquad's
`show_keyboard`. miniquad's `MainActivity` swallows every key, so `quad.toml` at the repo root
injects `android/MainActivityInject.java`: the volume keys change the media volume there. The FPS
counter sits further in from the rounded corner.

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
(`items_view::HeroView`, `story::journal_lines`). Hint boxes
(`widgets::tooltip`, `panel_hint`) fade in over 300 ms as the original's.

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
group or the order starts the pack at its top; both are kept for the session (`GROUP`, `SORT` in
`items_view`). The market's list has the same eight group buttons under its titles
(`building_view::market`, a row of 24 px, the list below it; no search: the market keeps its own
filter line, `item_filter::field_with`): it hides the other groups' rows before the filter, and a
hidden pick is not the pick.

The book (`spellbook::frame`) lays its 15 cells out column by column (`ROWS` = 5, 4dad08). Razdor's
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
  the largest that fits; `chrome::bar_height` the bottom bar. A chosen scale is cut down to
  the fit (`display::scale`); `display::chosen_scale` is the setting as it is.
- The world map's scale at zoom 1 (`world_view::map_scale`, Razdor's choice) follows the
  interface scale setting, not the window: its 32 px cells are drawn at the chosen scale ×
  0.9375 (as large as the original's pictures in the windows), not cut down to the fit;
  Auto is the fit. The settings' «Масштаб карты» (`map_zoom`) multiplies it, the wheel and
  +/- step from that (×1.1 a notch) and 0 comes back; locked, the zoom stays `map_zoom`.
  Razdor's: the wheel and +/- zoom out past the settings' range (`ZOOM_RANGE`, 0.4) down to
  the whole map fitting the view (`world_view::zoom_floor`); a map that fits already at 0.4
  keeps 0.4 as its floor. The floor is passed as `lo` to `zoom_at` and `step_zoom`.
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

«Расстановка армии» (`unit_drag::Controls`, saved as `formation_controls`, pushed in by
`main_menu::apply_options` → `unit_drag::set_controls`) is a Razdor setting that does not
default to the original (the owner's decision after a player's report, 2026-10-17):
«Перетаскивание», the default also for older files, makes a press on a card in the army
window (`items_view::squad`), the building window's hero grid and the garrison tab only select,
or deselect the selected unit pressed again, as the original does (`unit_drag::grid_press_in`,
`building_view::garrison_click`); units move by dragging
(`unit_drag::update`, `unit_drag::follow`, the garrison's `building_view::garrison_act`) and
the card hints say "click to select" (`items_view::pointed_line`). «Как в оригинале» keeps the
original's press to pick, press to move (interface.md §9.6) with dragging on top. Battle is
not affected.

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
