# Razdor UI: code patterns

Recipes taken from real commits. Names are as in the tree; when one has moved, grep for it and
fix this file.

## Drawing basics

- Immediate mode: a screen's function runs every frame, draws and reads input. Draw order is call
  order. Per-window state lives in the screen value, not in the thread-locals of `chrome` and
  `widgets` (reset each frame). Expensive work is cached or done ahead.
- Scale: `chrome::k()` is the interface scale (px per original pixel). The desktop's sizes are the
  original's pixels times `k`. A grown text size multiplies buttons by `display::button_grow()`.
- Phone: `display::dp_px()` returns `Some(px per dp)` on a phone (and in a snapshot with
  `RAZDOR_DP`). Branch on it for phone-only sizes; comment the branch as Razdor's phone choice.
- Safe area and keyboard on phones: `display::safe_area()`, `display::keyboard_inset()`
  (snapshots: `RAZDOR_SAFE_AREA`, `RAZDOR_KEYBOARD`).
- Input: `widgets::button_state(rect, enabled)` returns hover, down, pressed and grows the hit
  area to a finger on phones (`widgets::finger_hit`); `widgets::tapped_in(rect)` for a plain tap
  area. Both record themselves for the touch probe. Screens under a dialog must respect
  `widgets::input_blocked()` (the helpers do).

## Chrome: the original's frames and art

Windows are assembled from the install's own pieces, never from pictures in the repo and never
from flat colours:

- art: `chrome::win(name)` (the install's `Graphics/Windows/<name>.lit`), `chrome::art`;
- stretching without distortion: `chrome::three_slice` (the ends keep their aspect, the middle
  stretches: buttons, strips), `chrome::tile` (a texture repeated at a fixed scale: marble,
  parchment). There is no nine-slice helper: a frame is a tiled or sliced fill plus drawn lines;
- the original's lines: `chrome::window_frames` (the 2 px frame and the line under the title),
  `chrome::window_inner_frames`, `chrome::steel_frame`, `chrome::silver_frame`, in the
  original's pixels (`px` screen pixels each), so they stay crisp at every scale;
- fills by skin: `chrome::surface(rect, skin)`.

`chrome::window` does all of it for a normal window. Reach for the pieces only for a panel inside
one, and copy a neighbour's calls. See `docs/flows/assets-and-drawing.md`.

## Text

- Razdor's own texts: `tr("English text")`, `trf!("Gold: {gold}", gold)` with named
  placeholders, `n_("…")` for a text named in a table and translated where shown. The key must be
  a string literal: the catalog test reads the sources.
- The English literal **is** the English text and the key. There is no English catalog for the
  interface (`data/lang/en/game/` holds translated game data only). Russian goes in
  `data/lang/ru/*.txt` as `English = Russian`, under a `#` comment naming the screen:
  `game.txt` (menus, settings), `battle.txt`, `world.txt`, `common.txt`, `events.txt`.
- Renaming a label: change the literal everywhere and the catalog line's left side together.
- The guard: `cargo test --profile fasttest --lib i18n::`
  (`every_translated_text_is_in_the_catalog_and_every_catalog_text_is_used`: a missing line, an
  unused line or a placeholder mismatch fails).
- The original's own interface texts are read from the install, never copied into the repo:
  `chrome::ui_text`, `chrome::ui_markup`, `chrome::ui_line`, `chrome::options_value`.
- Measure with `widgets::measure` (widths change with the font mode); wrap with `widgets::wrap` or
  `chrome::wrap_label`; pick a face with `typography::Role::…with(|| …)` or `dt_font::with_face`.
  Drawing: `widgets::text`, `widgets::text_centered`, `chrome::shadow_text` (window labels),
  `widgets::text_fit` (shrinks to a width; never the fix for a phone label under 12 sp).
- Phone text sizes: `display::text_for(base, reference)` / `display::text_base`. The mobile
  check's sp is the capital's height / dp / 0.7, so a 14 sp label is not "14 × dp px": measure it.
- Help texts say what the player gets, in the tone of the neighbours. Player Russian, no internals.

## A settings row (Razdor's own setting)

Model: 7a6d798f («Смешивание земли»). Every step, in order:

1. `src/ui/audio.rs`: a documented field in `Settings` (what it does, Razdor's or the original's,
   its default). The struct is `#[serde(default)]`; a default other than the type's needs
   `#[serde(default = "…_default")]` and the value in `impl Default for Settings`.
2. `src/ui/main_menu.rs`:
   - a variant in `enum Row`, the same place in `Row::ALL` (bump the array length);
   - its place in `Group::rows` (Map, Battle, Screen, Controls, Other), next to related rows;
   - a `draw_row` arm: `labelled_row_help(c, geo, tr("Label"), value, Row::X.help_text())`
     returns `(clicked, rect)`; on a click step the setting. On/off values use `yes_no(…)`;
     a slider row uses `slider_row_at` / `tune_slider`;
   - a long explanation goes behind «?»: an arm in `Row::help_text` returning `Some(tr("…"))`.
     Never a hint line under the row;
   - a row that does nothing in some state (an orientation, the desktop) is hidden there in
     `Row::shown`.
3. `src/ui/display.rs` `follow_settings`: push the value to the module that uses it. The first
   branch is the snapshots' (it reads env switches, not the settings): give the setting a
   `RAZDOR_…` switch there so a scene can show its effect.
4. `data/lang/ru/game.txt`: the label and the help text.
5. «Качество графики» (`src/ui/quality_preset.rs`): decide whether the setting belongs to the
   presets. If not, say so there and pin it with a test (`the_ground_blend_is_in_no_preset`).
   `Settings::default()` must stay the «Оригинал» look.
6. Tests to add or update:
   - `audio.rs`: the default and a round trip (`…_defaults_on_and_survives_serialization`: `{}`
     reads the default, a changed value survives save and load);
   - `main_menu.rs`: `every_setting_row_belongs_to_exactly_one_group`, and
     `the_long_hints_are_behind_a_question_mark` (its list names every row with a help text);
   - `quality_preset.rs` as above.
   Run: `cargo test --profile fasttest --bins -- ui::main_menu:: ui::audio:: ui::quality_preset::`
   and `cargo test --profile fasttest --lib i18n::`.
7. Shots: `options:<group>` and `settings:<map>:<group>` (groups `map`, `battle`, `screen`,
   `controls`, `other`); `RAZDOR_SCENE_HELP=<Row variant>` for the «?» pop-up;
   `RAZDOR_SCENE_SCROLL=<px>` to see a row low in a long group. The pane scrolls, so a new row
   moves the ones under it: check the end of the list on the phone, both orientations, and 800x600.
8. Changelog line, the flow doc's settings paragraph, the wiki's settings page (ru + en).

Row geometry is `RowGeo`: the desktop keeps the original's 28 px rows and 14 px labels
(`RowGeo::desktop`); the phone has 44 dp rows and 14 sp labels (`RowGeo::phone_compact`, the
owner's design). Do not add a third geometry.

An option of the original's own settings window (its sliders and checkboxes) is not a `Row`: it
lives in `options_window` on the General tab and follows the original.

## A button

- The marble button: `widgets::button(x, y, w, h, label, enabled) -> bool`. With a set label
  size: `widgets::button_sized`; a label that may take two lines of whole words:
  `widgets::button_wrapped_sized`. Others: `widgets::small_button`, `widgets::toggle_button`,
  `widgets::red_button`, `widgets::checkbox`, `chrome::pill_button`, `chrome::close_button`.
- Size: take the height of the neighbours in that row. The desktop's is the original's; on a
  phone 44-48 dp for a main button, 40 dp at least for a secondary row. Grow it with
  `display::button_grow()` like every other button on the screen.
- Several buttons in one row share one label size, the largest that fits them all:
  `chrome::shared_button_label_px`, or `chrome::shared_button_label_lines_px` when labels may
  wrap. Each fitting its own label gives a row of mismatched sizes.
- When a label does not fit: first drop what the phone does not need (a key hint such as " (Q)"),
  then allow two lines, then give the button room. Not a smaller font, not a wider window.
- Put the rects in a pure function and test them (sizes in dp at both phone profiles, equal
  cells, gaps). Example tests: `on_a_phone_the_grid_cells_are_40_dp_and_the_gap_6_dp_at_both_profiles`,
  `on_a_phone_the_deployment_is_a_2x2_grid_of_equal_cells` in `src/ui/battle_view.rs`.
- A hotkey for it: `src/ui/hotkeys.rs` (`Place`), so F1 lists it.

### A grid of buttons

Model: 82912bc4 (the battle's deployment row, `Layout::with_deploy`).

- A cell keeps one normal button's height. The block gets shorter; the freed space goes to the
  content above it (there: the panel takes every pixel the grid leaves).
- State in the brief: columns × rows, the cell height in dp, the gap in dp, the label cap in sp,
  what stays put (there: the grid's bottom edge) and what takes the freed space.
- The owner's numbers for that grid (2026-11-14): 40 dp cells, 6 dp gaps, labels up to 14 sp.
  For a secondary button row start at the small end; 48 dp cells with 17-18 sp labels were
  rejected as too big.

## A window

Steps (also in `docs/flows/app-and-ui.md`, "How to add a window"):

1. A `Screen` variant in `src/ui/mod.rs` (or a field on an existing view for a pop-up of it).
2. `fn frame(…) -> Option<Screen>` in its module. Frame and title: `chrome::window(rect, title,
   skin, closable)` returns the inner rect and whether the cross was pressed; skins are
   `chrome::Skin` (`Marble` most windows, `Red` battle, `Paper`, `Brown` text boxes, `Strip`).
   Panels and boxes: `chrome::surface`, `chrome::text_box`, `chrome::parchment`,
   `chrome::divider`. Use the install's art through these; never draw a flat rectangle where the
   original has a frame.
3. Over the map: draw `world_view::window_backdrop` (or `backdrop_lit`) first and centre the
   window above the bar (`chrome::bar_height()`).
4. Route it in `App::frame`'s `match`. What must happen when it opens or closes goes in the
   screen-change block there, not in the window's function.
5. Close by the cross, Esc (`widgets::esc_pressed`) and its own button. On a phone the cross gets
   a finger-sized hit area (`chrome::touch_hit`), not a bigger drawing.
6. A message or a yes/no question is not a new window: use `dialog::Dialog`.
7. Phone: the same window. Let it take the popup scale (`display::popup_scale`) and the text
   size; keep it inside `display::safe_area()` and above the bar. If the content does not fit, it
   scrolls (`widgets::scrollbar_px`, `widgets::swipe_rows`) with the buttons outside the scroll;
   anything inside a scroll view is drawn and clicked only where it shows (`widgets::Clip`; a
   marble button ignores the scissor, see `scroll_button` in `main_menu.rs`).
8. A scene in `src/ui/snapshot.rs` (`try_stage`), its `scenes.txt` case and `SCREENS` line.

A tab: `widgets::tabs(x, y, w, labels, &mut selected)` for a horizontal strip in the original's
style; the settings window's own column is `TABS` / `tab_buttons` with `Group`. A new settings
group is a new `Group` variant (its `from_name`, `rows`, a tab label), which is an owner-level
change to the window: ask first.

## Tooltips and hints

- Hover text: `widgets::tooltip(&[(text, colour)])`; from inside a window, `widgets::tooltip_later`
  (drawn at the frame's end, over everything). On a phone it shows on a long press and opens away
  from the finger (`popup::placed`).
- The original's hint box at the foot: `widgets::panel_hint`, `chrome::hint_text`. It follows the
  «подсказки» option (`widgets::hints_on`).
- A settings row's explanation: the «?» button (`Row::help_text`), pinned by a click, shown on
  hover on the desktop.
- A tooltip never replaces or hides a control: draw the buttons every frame, the tooltip after.

## Flow doc and wiki

- `docs/flows/app-and-ui.md`: the screen's paragraph, the setting's group list. Keep code names
  in backticks so `docs/flows/verify.py` checks them.
- Wiki: the `wiki` skill. Settings and phone pages exist in ru and en with the same file names;
  a reverted feature's bullets are removed in the same commit.
