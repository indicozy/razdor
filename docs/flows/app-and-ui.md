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
2. `main`: `ui::widgets::load_font` (TrueType faces bundled in `data/fonts/`),
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

## The frame

`App::frame` in order:

- per-frame resets: `input::begin_frame`, `chrome::begin_frame`, `cursor::begin_frame`,
  `widgets::track_held_key`; Alt+Enter display mode (`display::hotkey_pressed`);
- settings pushed into the game each frame (`g.improved_ai`, `g.friends_let_pass`,
  `g.pass_on_bridges`, `g.pursue`, `g.walk_pace`, animation speed, row width) — the settings
  live in the UI, the rules only read plain fields; `main_menu::apply_options` puts the
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
font and language). Two options are read at start only, as the original's: «no music and
sounds» (`Audio::new`) and «impossible difficulty» (the content's difficulty factor in
`App::new`).

Rule: a parity setting reads the install's value until the player chooses
(`Option<bool>` = `None` → the install's `[Options]` value such as `OptValue9`, see `main_menu::expert_ai`). A Razdor
extra defaults to the original's behaviour and its label says which choice is the
original's ("Battle (original)").

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
