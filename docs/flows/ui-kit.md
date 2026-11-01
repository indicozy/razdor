# UI kit: adaptive text, rows, buttons and windows

Razdor's own (the owner's decision, 2026-11-01): every window is built from a few shared
components (`src/ui/kit.rs`) so that on a phone no text is under 12 sp and no touch target under
40 dp, and every window has a real upright layout, while the desktop draws exactly what it drew
before (the UI regression goldens prove it).

## The contract

- Phones are the `mobile-ui` skill's profiles (`tools/ui-regress/mobile_ui.py` `PROFILES`),
  upright and sideways. `display::dp_px()` is `Some` on a phone, `None` on the desktop.
- Text on a phone: never under 12 sp at the phone's default text size (1.25); a larger text size
  only makes it larger (`kit::min_cap_px`). sp is measured as the probe does: capital height / dp / 0.7.
  Safety net for every screen, kit or not (Razdor's own, the owner's decision, 2026-11-01): `widgets::phone_floor`
  raises the size in `text_exact` (drawing) and `measure_exact` (measuring, so a layout that measures makes room),
  and the engraved title path floors its whole pixel size. `widgets::without_floor` is for the floor's own measurement.
- Touch targets on a phone: at least 40 dp on the shorter side, 48 dp (`widgets::FINGER_DP`)
  where there is room (`kit::touch_h`).
- The desktop (and any `dp_px() == None` run) keeps today's pixels: every kit function returns
  exactly what the hand-written code computed (`design * k`).
- Overflow: a window declares its landscape layout (today's coordinates) and its upright one; inside
  them text wraps to the width and the column scrolls as the last resort. Nothing shrinks below the
  floor, nothing is cut with "…" on a phone.

## Metrics

- `kit::phone() -> Option<f32>`: `display::dp_px()`.
- `kit::upright() -> bool`: `window_layout::portrait()` (a phone held upright).
- `kit::min_cap_px() -> Option<f32>`: on a phone `12 sp × 0.7 × dp × max(1, text_size / 1.25)`.
  Pure twin: `min_cap_at(dp: Option<f32>, text_size: f32)`.
- `kit::text_px(design: f32) -> f32`: the size of a text that the design gives `design` px:
  desktop `design × k`; phone `floored(design × k × text_size)`.
- `kit::floored(size) -> f32`: the size raised so that the current face's capital height ("Н",
  `widgets::measure_exact`) reaches `min_cap_px`; unchanged on the desktop or when already big
  enough. Pure twin: `floored_at(size, cap_at_size, min_cap)` = `size × min_cap / cap` when `cap < min_cap`.
- `kit::touch_h(design_h) -> f32`: a button's height: `design_h × k × button_grow()` and, on a
  phone, at least 44 dp. Pure twin `touch_h_at(design_h, k, g, dp)`.

Fonts on a phone: the Modern faces by default (`audio::Settings` default and `migrate_font`, once). A player who picks
the original's sheets on a phone gets them at any zoom: kit text on a phone is drawn with
`dt_font::legacy_metrics_if(true, …)`, so the size asked for is the size drawn.

## Text

`kit::Text` (builder): the string, the design size, the colour, the face (`dt_font::Face`, default the
current one), shadowed (default yes, `chrome::shadow_text`) or plain, and the line pitch in design px
(default `design × 17 / 12`, the settings' hints). `size()` is `text_px(design)`; `pitch()` is
`max(pitch × k, size × 1.4)` on a phone, `pitch × k` on the desktop; `lines(width)` wraps
(`widgets::wrap`); `height(width)` = lines × pitch; `draw(x, baseline, width)` draws the lines from a
first baseline. On the desktop it calls exactly `chrome::shadow_text`/`widgets::text` with
`design × k`, so the native metrics apply as before.

## Flow (a measured column that scrolls)

`kit::Flow::begin(area: Rect, id: &'static str)`: a cursor that stacks rows from `area`'s top, each
as tall as the larger of its design height (× k) and what its content needs. Rows:

- `gap(design)`: moves down `design × k`.
- `text(&Text)`: wrapped text, as tall as its lines.
- `row(design_h, |r: Rect| …)`: any content in a rectangle `max(design_h × k, …)` tall.
- `setting(label, value, design_h) -> bool` (the settings' row): the label left, a button on the
  right; where both do not fit side by side (upright) the label goes on its own line and the button
  takes the full width under it.
- `finish()`: stores the content's height. When it ran past `area` (the previous frame's height),
  the column scrolls: the wheel, a finger's swipe (`widgets::swipe_rows`), a thin bar at the right
  (`widgets::scrollbar_px`), drawing clipped to `area` (`widgets::Clip`); the scroll offset is kept
  per `id`. The desktop's windows never overflow, so nothing changes there.
- Inputs in rows scrolled out of `area` are ignored (a button's hit rectangle is cut to `area`).

## Buttons

`kit::button(r, label, enabled) -> bool`: `widgets::button_state` (the finger's hit area, the touch
probe) + `chrome::marble_button` + the button cue, as the settings' button. Checkboxes and sliders
route their hit tests through `button_state`/`tapped_in` so the probe sees them.

## Windows

- Desktop: today's helpers (`window_layout::title_rect`, `over_map_rect` under the window scale,
  `main_menu::window`/`map_window`), unchanged.
- Phone, held upright or sideways: a kit window is at one design px per dp (`kit::phone_window`,
  pure twin `kit::phone_window_at`), so its scale is the phone's `dp`. It is as wide as its design
  (594 px for the settings, 640 for the advanced settings) or the screen allows, and as tall as its
  design (grown by the button growth) or the room it has: from the top inset (8 px down) to the
  title's foot upright (`window_layout::title_bottom`), to the bottom inset sideways, or to the bar's
  top over the map. Its content is a `Flow` that scrolls when it must.
- A row cut by the flow's edge is drawn but cannot be pressed until it is whole (its hit area is
  never half a row). Rows of a phone's column keep 4 px (checkboxes) or 6 px (settings) between them
  and are at least `kit::finger_h` (48 dp) tall, so the touch probe finds no crowded targets.
- The phone's layout code runs only when `kit::phone()` is Some; the desktop never reaches it.

## Rows on a phone

- `kit::check(r, label, on, ink, box_design)` / `check_in(flow, …)`: a checkbox row, the box 32 design
  px square at the left, the label wrapped beside it; `kit::check_h` gives its height.
- `kit::setting(flow, label, value, button_design_w) -> (bool, Rect)`: the label on the left and the
  button on the right. When the value does not fit beside the label at its own size, the label goes on
  its own line and the button takes the full width under it: a phone's value is never shrunk to an
  ellipsis.

## Where each screen stands

| Screen | Phone layout |
|---|---|
| Settings («Настройки», `main_menu::options_phone`) | one scrolling column: five sliders (the label above the track when narrow), eight checkboxes and the touchpad's, the rows (orientation, text size, large windows, the import); a footer of «Ещё…», «Принять», «Отмена» (one row at 440 design px and wider, two rows below) |
| Scenario list («Сценарий для Новой Игры», `new_game::scenario_phone`, `list_phone`) | upright: the picked map's picture (full width, at most 25% of the height), its name and its description in a box that scrolls (at most 22%), the list below with its own scroll (group heads and chapter rows at least a finger tall, names and titles wrapped), then the footer in three rows: the views «Все сценарии» and «Как в игре» in halves, «Добавить карту…» (and «Мод») in the next, «…», «Далее», «Отмена» at the foot; sideways: the list on the left (55%), the picture over the name and the scrolling description on the right, the footer in one row (two rows when it does not fit) |
| Experimental rules (`new_game::experimental_menu`) | the window at the phone's scale (one design px per dp, its title and close button touch-tall), six rows 50 dp tall 8 dp apart, the labels shrink together to fit the row |
| New hero («Стартовые характеристики Героя», `new_game::class_phone`, `hero_phone`) | upright: the three portraits across the width, the full-width name field under them, then the class's name, description, bonus line and the stats in two columns, in a column that scrolls; the footer «Старт» and «Отмена» in halves; sideways: portraits and name on the left half, the column on the right, the footer at its bottom right |
| Drop-down lists (`widgets::open_list`, `view_choice_at`) | on a phone each row is a finger tall (48 dp) and the list shows as many rows as fit in 90% of the screen (at least three), so a long list scrolls |
| Advanced settings (`main_menu::advanced_phone`) | one scrolling column: the left column's rules with their hints, then the right column's rows, «Ок» at the foot; no «Updates» row (the phone has its own update window) and no «Keyboard shortcuts» row (no keyboard) |
| Save («Сохранение активной игры», `saves::save_phone`) | upright: the name field on top (a text box with the caret), the list of saves as cards (name 15, scenario and in-game date 13, real time 13; at least 56 dp, 6 dp apart) scrolling, then «Быстрое сохранение» across the foot and «Сохранить», «Отмена» in halves; sideways, or upright with fewer than three cards: the list at 65% and the three buttons stacked on its right |
| Load («Загрузка сохраненной игры», `saves::load_phone`) | the same column: the two tabs («Личные», «Авто-сохр.») in halves on top, the cards with the play sign on the picked one and its pin and delete signs (48 dp), «Очистить всё» across the foot, «Загрузить», «Отмена» in halves; the same sideways stack |
| Leave windows («Выход из игры», `saves::exit_phone`; the battle's «Варианты выхода из битвы») | the picture (the battle's keeps its red buttons), the version line, the warning in a box, and the buttons: a row of five sideways, two columns with «Отмена» across the foot upright; the middle scrolls; the question boxes (`saves::phone_box`) are centred, their text at 13 px and their buttons touch-tall |
| Army («Характеристики и состав армии Героя», `items_view::squad`, `army_phone`) | upright: the band (the picture and the full stats, the level and the experience on their own lines), the nine filter buttons in two rows (five and four, 8 dp apart), the pack, the hint strip, the army's cards, «Закрыть» at the foot; sideways: the filter row across the top, the band over «Закрыть» on the left, the pack, the strip and the cards on the right; a unit's cards are pictures only (the band has the stats) |
| Hero («Личные характеристики и инвентарь Героя», `items_view::hero_window`, `army_phone`) | upright: the band, the pack, the resources in a 2 × 2 grid, the diary (the description box, the tabs over the list, the list, the entry), the buttons in a row at the foot; sideways: the band over the pack on the left, the right column (the resources, the tabs and the list with three touch-tall rows, the entry, the buttons) scrolls as one by its wheel and bar when it does not fit; the description box takes the entry's place only while an item is described |
