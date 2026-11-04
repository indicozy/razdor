# Razdor Studio UX: build tracker

The build plan for the UX overhaul specified in
[`../specs/2026-11-04-studio-ux-design.md`](../specs/2026-11-04-studio-ux-design.md) (§14 lists each
milestone's full contents and snapshot options). This file is the status list; it is updated as each
milestone lands. There are no GitHub issues for these milestones (the owner's decision, 2026-11-04).

How each milestone runs: a coding agent builds it in the `studio-ux` worktree, iterating with dev
builds and finishing with one release build, the full test suite and off-screen screenshots; Claude
reviews the code and the screenshots; it is pushed to `v2-packages`. The owner reviews the
screenshots of all milestones at the end, which gates the v2 → `main` merge (the same policy as
phase 2 of the studio).

## Milestones

| # | Milestone | Delivers | Status |
|---|---|---|---|
| UX1 | Foundation | Defect fixes (Ctrl+C/X/V from clipboard events, the pointer no longer overridden, stable tab ids); theme tokens, Noto Sans with Cyrillic, Phosphor icons; the command registry with rebindable shortcuts and tooltips; the frame (top bar, status bar, bottom drawer) and the four workspaces, each with its own saved layout; the start screen; UI scale 80–150 % | done |
| UX2 | Inspector | The Godot-style widget set (header, field search, collapsible sections, label column, changed-field markers and resets, keys in tooltips or inline, drag-reorder lists, price block) on every form: army, building, point, object, trigger, dialog, unit, item, package, map settings, campaign | done |
| UX3 | Карта workspace | Tool strip, tool options bar, the layer model (data with ids; eye and lock; overlay group; click filtering), the palette (terrain swatches; object thumbnails from the install, name and icon without it), the brush (size and `[` `]`, round or square, footprint, gap-free strokes, Shift line, scatter and density, one undo per stroke), the Inspector column (own column with a selection; folds the layers column into tabs below 1600 px) | planned |
| UX4 | Selections | Marquee, lasso, magic wand (contiguous or global), add and subtract, painting limited to the selection, marching ants, copy, cut, floating paste, delete, save as stamp | planned |
| UX5 | Сюжет and Данные | The story outline with search and tabs, the story/campaign graph switch; unit and item spreadsheets, the texts table moved here, the package form, files with the code editor, «Открыть в файле» | planned |
| UX6 | Тест, «Играть», command palette | Бой / ИИ / Оценка at full size with jumps to objects; «Играть» (F5: save, start the game on the current map from its hero start, back to the studio on close); the command palette (Ctrl+Shift+P actions, Ctrl+P go to object or file) | planned |

## Later (not in this build)

The first-run tour (at the stable release), the light theme (a second token set), user-created layers
(the layer model allows them), pen tablet support. Out of scope: touch screens and phones.

## Log

One line per landed milestone: date, commit, notes and gaps found in review.

- 2026-11-04: spec written (`cc31e9a8`); planning closed.
- 2026-11-04: UX1 this commit — frame, four workspaces, commands and shortcuts (Settings), theme, fonts, icons, start screen, UI scale; gaps: Map tool strip and palette, Cut, Ctrl+F/Tab/Ctrl+P, Play, AI viewer still on the map; the desktop pointer and a language switch with a saved layout not checked at run time.
- 2026-11-04: UX2 this commit — the Inspector widget set (`inspector_kit.rs`: header, field search with Ctrl+F, sections, label column, changed dot and reset, file keys in tooltips or inline, drag-reorder lists, price card) on every form (army, building, point, object, hero, trigger, dialog, unit, item, package, map settings, campaign node and edge); gaps: changed markers and resets only where a base exists (units and items), the trigger and dialog row editors keep their own layout, a dialog choice cannot be duplicated (no edit yet), Ctrl+F and the tooltip snapshot not checked at run time (no key or pointer injection off-screen).
