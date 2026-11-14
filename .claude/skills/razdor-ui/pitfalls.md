# Razdor UI: known pitfalls

Mistakes made in this project, with the owner's correction and the check that catches each.
Numbers are backlog ids (the backlog's place is in `CLAUDE.local.md`) and commits. Add an entry
when a UI change is rejected, reverted or reported broken.

## Process

| # | Pitfall | Rule | Example | Check |
|---|---|---|---|---|
| P1 | Pushed on your own review of the shots | The owner's yes on screenshots comes before the push, also for fixes and also under "work on your own" | 1.3.1's phone diary tab strip and settings pages shipped unseen and were reverted in 1.3.2 (1495, 3c4f3cba) | No push without a dated yes; the commit says "approved by the owner" |
| P2 | A revert brings the old bug back | Before reverting, list what the reverted commit fixed and keep or redo those fixes | The revert 3c4f3cba (report 1495) reopened the cramped advanced settings: 1489 came back as 1502 | Run the reverted commit's scenes after the revert |
| P3 | A fix that makes a worse look | A broken-UI fix is still a look change: show options, the owner picks | The scenario list's corner fix left empty bands and a short scrollbar (1397 → 1399) | Before/after pairs of the whole window, not the fixed corner |
| P4 | Showing shots with known faults silently | Say what is wrong and what you ask; do not make the owner find it | 1607 took several rounds of shots; the useful ones named the fault (labels 5.7-7.1 sp sideways) and asked a layout question | Self-review against this file before sending |
| P5 | A stale settings file in the snapshot's save folder | Each run gets a clean `RAZDOR_SAVE_DIR` | A leftover `audio.json` still sets its values, so a "debug off" shot can show the strip (the warning beside `RAZDOR_SNAPSHOT_DEBUG` in `snapshot.rs`) | New folder per run; the runners do this |

## Phones: size and layout

| # | Pitfall | Rule | Example | Check |
|---|---|---|---|---|
| S1 | A layout overhaul for phones | Same window, same rows, modestly bigger. No new component layer, no upright re-layout unasked | The adaptive-components overhaul was rejected by the owner and players (2026-11-01); the upright bigger-buttons variant was dropped too (2d1941f4) | Compare the phone shot with the desktop's: same structure |
| S2 | Giant phone controls | Text about 14 sp, buttons 44-48 dp, secondary rows 40 dp. Never full-screen windows or stacked full-width buttons | 1.4.0's sideways settings showed three sliders a screen (1569); a full-screen update window with stacked buttons was rejected; a close cross as its own 44 dp box was "too big": the hit area grew instead | Mobile check sizes; count the rows visible in the sideways shot |
| S3 | Stretched grid cells | A grid cell keeps one button's height; the freed space goes to the content | 1607: the 2×2 cells filled the old stack's height (57×76 dp squares) | Unit test the cell height in dp; look for square buttons in the shot |
| S4 | "Normal" size still too big for a secondary row | Start at the small end for secondary buttons | 1607: 48 dp cells with 17-18 sp labels were rejected; 40 dp and labels up to 14 sp approved (82912bc4) | Mobile check's label sp per target |
| S5 | A label squeezed to fit | Make room: drop key hints on phones, wrap whole words, share one size across the row. Under 12 sp is a layout bug | 1607 sideways: labels at 5.7-7.1 sp in a ~130 dp column, fixed by dropping " (Q)"/" (W)" and two-line labels | Mobile check red list; `--text-size 1.5` |
| S6 | Fixed px on a phone | Sizes in dp, text through the phone text base, pop-ups through the popup scale | The mod drop-down's 22 px rows caused mis-taps (1500); tiny search text (1496); a fixed-size rules window (1461); tiny spell tooltip (1465) | Mobile check; grep the new code for bare numbers not times `k` or dp |
| S7 | The sideways layout squeezed upright (or the reverse) | Check both orientations; upright may need rows of two, a scroll, or the picture on top | Noon report's four labels overlapped upright, now 2 × 2 (bf3d0b85); advanced settings spilled out of the window (1489) | Shots at `1080x2400` and `2400x1080` |
| S8 | Under the notch or a rounded corner | Keep text and crosses inside `display::safe_area()` | The cheat console's text and the minimap's cross were cut by screen corners; the bar's first digits too | A phone shot with `RAZDOR_SAFE_AREA="l,t,r,b,bl,br"` (physical px; `display::parse_safe_area`): the plain cases have no insets |
| S9 | A phone fix that moved the desktop | Phone branches only; the desktop keeps the original's sizes | ca2aff29 states "desktop unchanged (pixel check, 149 cases)": do the same | Pixel identity run on the desktop cases |

## Overlap, clipping, hit areas

| # | Pitfall | Rule | Example | Check |
|---|---|---|---|---|
| O1 | Two overlays in one corner | Anything pinned to a screen edge asks who else is there (FPS, debug strip, version, language link, minimap, compass) | The debug strip covered the FPS counter (1636, e1571945: the label now sits under `debug_banner::bottom()`) | A scene with both on (`RAZDOR_SNAPSHOT_FPS=1 RAZDOR_SNAPSHOT_DEBUG=1`) |
| O2 | Fine at Normal, overlapping at Large | Test text sizes 1.25 and 1.5; a grown thing that no longer fits scales down together or wraps | The title's grown ovals ran into each other (d6b66f0a); the settings' bottom buttons overlapped at large text (1511, aa8ae6ee) | `RAZDOR_TEXT_SIZE=1.5`; `mobile_ui.py run --text-size 1.5` |
| O3 | Drawn outside its scroll view | Everything in a scrolled list is drawn inside the clip and clicked only where it shows | A heading drawn after the scroll area lay over the subtitle (1511); half-visible marble buttons showed under «Ок» (1509, ca2aff29) | `RAZDOR_SCENE_SCROLL=<px>` shots, scrolled mid-row |
| O4 | A new row pushes the last ones out | A list that can grow scrolls; fixed buttons stay outside the scroll | Phone settings did not fit and could not scroll (1483, 1502; test `the_advanced_window_scrolls_by_what_runs_past_its_view`) | Shot the end of the list at 800x600 and on both phone orientations |
| O5 | Labels running under their buttons | The label wraps in the room left of the button, or goes on its own line above | Right-column labels cut by buttons (1505, 1511) | `labelled_row_help` does it; for hand-made rows, shots in ru and en |
| O6 | Hit area not where the drawing is | Derive the hit rect from the drawn rect; finger growth must not cross a neighbour | A checkbox drawn at the row's foot, tapped at its top (1432); a star's 48 dp area reached under the scrollbar's arrows (1444) | Mobile check `view/` outlines; a unit test on the rects |
| O7 | A control vanishing under a tooltip | Controls are drawn every frame; tooltips last | The victory window's OK disappeared while a loot card showed (1464, 5ec3f1a0) | A scene with `RAZDOR_MOUSE` on the hovered thing |
| O8 | A row that does nothing | Hide a row where it has no effect | «Размер нижней панели» shown upright, where the bar never changes (1570) | Try the row in each orientation's shot |
| O9 | The phone keyboard opening for a list | A drop-down or filter must not take text focus on open | The keyboard covered the mod list (1440) | `RAZDOR_KEYBOARD=<px>` scene; `RAZDOR_SCENE_MOD_LIST=1` as the model |

## Texts

| # | Pitfall | Rule | Example | Check |
|---|---|---|---|---|
| T1 | A text not in the catalog, or a dead catalog line | Every `tr`/`trf!`/`n_` literal has its line; removed code removes its line | Guarded by the catalog test rather than by a past report | `cargo test --profile fasttest --lib i18n::` |
| T0 | Checked in English only | The runners render English (no saved language, no `LANG`); the players' Russian is usually longer | No report yet: the goldens cannot catch a Russian label that is cut | A Russian shot (`LANG=ru_RU.UTF-8`) of every screen with a new or tight label |
| T2 | The English key too long for its button | The key is the English label: keep it as short as the Russian | "Rearrange the army" showed in full on the English screen; renamed to "Restart" (82912bc4) | An English shot (`settings.json` with `"language":"en"`) |
| T3 | One word for two different actions | Check the screen for an existing label with that word | «Рестарт» is both the deployment button and the exit window's restart (1607): raised with the owner before landing | Read the neighbours' labels in the shot |
| T4 | Game texts in git | Shots, specs and reports that quote the game stay outside the repo | The mobile check's specs and baseline live in the goldens folder for this reason | `git status` before the commit |
