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

Fonts on a phone: the Modern faces by default (`audio::Settings` migration, once). A player who picks
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

`kit::Window`: where a window stands and at what scale.

- Desktop and phone sideways: today's helpers (`window_layout::title_rect`, `over_map_rect`,
  `sideways_window`, `main_menu::window`/`map_window`) with the design width and height.
- Upright phone: the full width less the margins and the safe insets, from the top safe inset to the
  title foot (`window_layout::title_bottom`) or above the bottom bar (over the map). Its scale is
  `width / kit::UPRIGHT_W` (360 design px), so a design px is about a dp and the design's 12–15 px
  texts are 12–15 sp before the text size. Its content is a `Flow` (it scrolls when it must).

## Where each screen stands

| Screen | Upright layout |
|---|---|
| Settings («Настройки», `main_menu::options_window`) | one column: sliders (label over the track), checkboxes (wrapped), the rows, the buttons at the foot |
| Advanced settings (`main_menu::advanced_window`) | one column: the left column's rules, then the right column's rows |
