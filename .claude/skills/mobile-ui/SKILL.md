---
name: mobile-ui
description: Use to check that phone screens are readable and easy to tap - "text too small", "font size", "мелкий текст", "шрифт мелкий", "маленькая кнопка", "не попасть по кнопке", "small button", "hard to tap", "touch target", "can't read it on the phone", "mobile UI check", "buttons grow with the text size", "кнопки под размер текста", after every phone UI change and before a release. Renders every phone screen off-screen on two phones in portrait and landscape, measures every text in sp and every touch target in dp, lists what is too small or crowded with marked screenshots, and on request turns the findings into per-screen specs for the owner.
---

# Mobile UI check (phones)

Players keep reporting text too small to read and buttons too small to hit («Нанять», the close
cross) on phones. Screenshots and the UI regression check only show that a screen changed; this
check measures every text and every touch target and names the small ones.

## How it measures

- `src/ui/text_probe.rs` (`RAZDOR_TEXT_PROBE=<file.tsv>`): every text drawn in the snapshot's
  frame: size, capital height, place, width, text. Hooked in `widgets::text_exact` and
  `chrome::engraved_centered`.
- `src/ui/touch_probe.rs` (`RAZDOR_TOUCH_PROBE=<file.tsv>`): every touch target's hit rectangle,
  whether it is enabled, and the screen code that asked for it (`file:line`, through
  `#[track_caller]`). On a phone that is the finger-sized hit area (`widgets::finger_hit`), not
  the drawing. Hooked in `widgets::button_state` and `widgets::tapped_in`, which most
  buttons go through (`button`, `small_button`, `pill_button`, `close_button`, …). A screen that
  tests the pointer itself (`mouse_in` + `clicked`) is not seen: those are the gaps (below).
- Snapshot runs only; the game never sets either variable.
- `tools/ui-regress/mobile_ui.py` builds its cases itself: `PROFILES` × portrait/landscape ×
  `SCREENS`. Profiles: **small** 1080×2400 at 2.75 px per dp (an ordinary modern phone), **big**
  1440×3120 at 3.5 (a dense large phone, the owner's OnePlus 15 class). A new phone is one line in
  `PROFILES`, a new screen one line in `SCREENS`.

Limits:

| what | too small (red, fails when new) | small (orange, reported) |
|---|---|---|
| text, capital height / dp / 0.7 | under 12 sp | under 14 sp |
| touch target, its shorter side | under 40 dp | under 48 dp (Android's minimum) |
| two targets side by side | | crowded: hit areas partly overlapping, or under 8 dp apart while one is under 48 dp |

## Run it

1. Build the silent binary the UI regression check uses, on CPUs 8-19:
   `CARGO_TARGET_DIR=/mnt/ai_models/razdor-scratch/ui-regress/target CARGO_BUILD_JOBS=12 nice -n 19 taskset -c 8-19 cargo build --release --no-default-features`
2. `python3 -I tools/ui-regress/mobile_ui.py run` (`--only <text>` for some cases, e.g.
   `--only barracks` or `--only big-portrait`; `--bin` for another binary). Output:
   `/mnt/ai_models/razdor-scratch/mobile-ui/<time>/`.
3. Read `report.md`: per case the counts, then each finding smallest first, `**NEW**` on a too-small
   one the baseline does not list. Exit 1: a case crashed, or a new too-small text or target.

After a UI change, the report is enough. Look at the pictures only for specs (below).

## Buttons grow with the text size

The owner's rule (2026-10-31): at «Размер текста» Normal (1.0) every button keeps the original's
size; at Large (1.25) and Largest (1.5) a button, its label and the line under it (a price) grow
with the text (`display::button_grow()`), on the desktop and on phones. Phones start at Large, so
their buttons are grown by default.

- `python3 -I tools/ui-regress/mobile_ui.py adapt [--only <text>]` renders the screens at 1 and 1.5
  and lists every labelled button whose label grows less than 1.2× as "does not grow"
  (`adapt.md` in the run folder). `run --text-size 1.5` measures a screen at Largest.
- A screen changed for this: `adapt` shows its buttons growing, `run --text-size 1.5` shows no
  new overlap or crowding, and the UI regression check shows **no** change at Normal.
- Growing a button: its height, width (if the label needs it) and label size times the factor; the
  layout around it makes room (move the neighbours, grow the box), never shrinks the label.
  Every screen grows its buttons (rolled out 2026-10-31), except the bottom bar's readouts
  (`game_bar.rs`, the original's panel art) and the market row (bound by the original row's
  widths; the owner's call). `small_button` and `toggle_button` grow their labels themselves; a
  screen grows their rects. A new button on any screen grows from the start.

## Specs (on the owner's request, and at the release check)

No local model writes them: Claude does, from the report, the code and the pictures.

1. Look at **every** `view/<case>.png` (the marked shot, shorter side 540 px): cyan outlines are the
   measured targets, red/orange too small/small, magenta lines crowded pairs. A control you can see
   with **no outline** is a gap: the probe missed it. Estimate its size from the full-size
   `marked/<case>.png` and say in the spec that the number is an estimate.
2. Read the code at each finding's `file:line`.
3. Write `specs/<screen>-<orientation>.md` in the run folder, one per screen and orientation (both
   profiles together): each finding with its source place, size now (dp, mm, both profiles), the
   target size, and the fix. Fixes, in order of preference:
   - for a `button_state` target, `widgets::finger_hit` already grows the hit area where there is
     room: a target still small has neighbours too close; give it room (layout) or move them;
   - grow the hit area only, the drawing as it is (as `chrome::touch_hit`, `widgets::tap_rect` and
     `saves::finger_hit` do);
   - change the layout (wrap, fewer columns, a scroll, more gap);
   - a phone-only size (`display::dp_px()` is Some; `display::text_base`/`text_for` for text).
   Never a fixed px, never the desktop's layout (it keeps the original's sizes, parity), never a
   shared widget changed for every screen. A gap's fix is to route its hit test through
   `button_state`/`tapped_in` first, so the probe sees it.
4. Give the owner the list of specs. Every one is a UI/QoL change: the owner decides. The ones the
   owner approves become entries in the reports backlog (reporter "mobile-ui check", status
   `triaged`), then are implemented as usual. The specs hold the game's texts: never in git.

## Fix

- Make room by the layout, not by shrinking: a `text_fit` that shrinks a label below 12 sp is a
  layout bug.
- A phone change is Razdor's choice; comment it as such.
- Run the check again, and the UI regression check (the pictures change).

## Baseline

`mobile-ui-baseline.json` in the goldens folder (`RAZDOR_GOLDENS`, `/mnt/ai_models/razdor-goldens`):
per case the too-small texts and targets (`file:line|label`) accepted. It holds the game's texts, so
never in git. `python3 -I tools/ui-regress/mobile_ui.py accept <run dir>` writes a run's too-small
items into it; accept only what the owner agreed to leave small (the bottom bar's labels, say).

## When

Every phone UI change, before a release, and when a player reports small text or buttons. A screen
not in `SCREENS` is not measured: add it.
