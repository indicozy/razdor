# Razdor Studio UX: workspaces, Map tools, Inspector, shortcuts, milestones

Status: design agreed with the owner on 2026-11-04 (wayfinder #36; decisions in #37 to #43; the
prototype approved in #41 and #42). Not built yet. Milestones UX1 to UX6 follow the order below.

Related: the studio design [`2026-11-03-razdor-studio-design.md`](2026-11-03-razdor-studio-design.md)
(the layout, the files-are-the-truth rule and phase 2, which this spec reworks on screen), the
package format [`rzpack-format.md`](../../reference/rzpack-format.md) and the editor guide
[`rzpack-editor-guide.md`](../../reference/rzpack-editor-guide.md). Sources are listed at the end.

## 1. Goal and audience

The studio (`src/ui/studio/`, `src/studio/`) is the editor for Razdor 2.0 packages: map,
story, units and items, campaign, texts, tests. This spec defines how it looks, how a person
moves through it and which keys do what. It covers the screens of the whole tool, so every later
change to a screen can be checked against it.

Audience: map makers and scenario writers who know Photoshop, Krita, Tiled or Godot, and players
of Discord Times who know its names (Холмы, Леса, Постройки, brush presets 1 to 6 and 9). The
studio uses the art tools' conventions where the action exists there, and keeps DT's names for
DT's things. Input is desktop only: mouse and keyboard, with a 1280×800 minimum window. Touch
screens, tablets and phones are out of scope.

## 2. Decisions at a glance

| # | Decision | Section |
|---|---|---|
| U1 | Four workspaces, «Карта», «Сюжет», «Данные», «Тест», each with its own saved panel layout. | 4 |
| U2 | Photoshop-like Map workspace: tool strip left, options bar above the canvas, palette left of it, layers and minimap right, Inspector at the far right. | 5 |
| U3 | Fixed layers, one per content kind, with an eye, a lock and an overlay group; the layer model is data with ids. | 5.4 |
| U4 | Brush: size slider and `[` `]`, round or square, footprint preview, gap-free strokes, Shift+click line, density and scatter, one undo step per stroke. | 5.5 |
| U5 | Selections: marquee, lasso, magic wand; Shift adds, Alt subtracts; painting and fill stay inside the selection. | 5.6 |
| U6 | One custom dark theme built from tokens; one accent colour. | 8 |
| U7 | Noto Sans Regular (subset with Cyrillic) as the UI font; egui's default fonts are removed; Phosphor Regular icons for every icon. | 8.3 |
| U8 | Inspector in the Godot style: header, field search, collapsible sections, fixed label column, changed-field markers, drag-reorder lists. | 6 |
| U9 | Shared command registry: every action has a name, icon, default shortcut and context; bindings rebindable in Settings with a conflict check. | 9 |
| U10 | Command palette (Ctrl+Shift+P for actions, Ctrl+P to go to an object or file). | 10 |
| U11 | A start screen replaces the empty studio. | 11 |
| U12 | UI scale 80 to 150 % in 10 % steps. | 12 |
| U13 | Three pre-existing defects are fixed first (UX1). | 13 |
| U14 | Milestones UX1 to UX6, each pushed to `v2-packages` after review; the owner reviews the screenshots at the end. | 14 |

## 3. Principles

1. **Art-tool conventions, DT names.** A key, a tool or a gesture follows Photoshop, Krita, Tiled
   or Godot when the action exists there (`[` `]` for brush size, Shift+click for a straight line,
   Alt for the eyedropper and for subtracting from a selection). DT's own names stay in the UI
   (Холмы, Леса, Постройки, presets 1 to 6 and 9).
2. **Files are the truth.** Unchanged from the studio design: every visual edit patches the
   package's TOML in place, the code view shows the same files, and an outside change is seen
   through the watcher. A UX change never adds a second store of content. Editor-only state
   (layout of graph nodes, stamps, notes) stays in `.editor/` or in the user's settings.
3. **Visible help, rebindable keys.** Every command has a tooltip with its name and shortcut, and
   every shortcut can be changed in Settings. Defaults are the table in section 9.
4. **One accent.** The accent colour marks the selection, the active tool, changed-field markers
   and primary buttons. Nothing else is coloured.
5. **Context decides.** The same key may do different things in different workspaces or
   panels when the action differs (Ctrl+F finds in the current view, in a story graph it finds a
   node, in the Inspector it searches fields). The shortcut help lists the context of each entry.
6. **Desktop at 1280×800.** Every workspace must fit at 1280×800 at 100 % scale. Larger windows
   add space to the canvas; they do not add columns.
7. **Nothing is faked.** A control that the format cannot hold is not shown, or is shown disabled
   with its reason (the format gap is listed in the editor guide, not hidden in the UI).

## 4. The frame and the four workspaces

### 4.1 Frame (every workspace)

| Part | Content | Height or width |
|---|---|---|
| Top bar | workspace switcher (Карта, Сюжет, Данные, Тест, Alt+1 to Alt+4); Save; Undo and Redo; Проверка (F7); Играть (F5); the command entry (from UX6) | 36 px tall |
| Status bar | cell under the cursor, terrain under the cursor, the active tool's hint, the save state (saved, unsaved, saving) | 24 px tall |
| Bottom drawer | tabs Проблемы, Вывод, Claude; closed by default as a 24 px handle; opened with Ctrl+J or by a new error | 186 px tall when open |
| Inspector | the current selection's form, in the right-hand column of each workspace | 380 px wide, full height |

The drawer and the Inspector are the only panels every workspace shares. The Tab key hides
the side panels to give the canvas or the graph the full width.

### 4.2 Workspaces

Each workspace has its own dock layout, stored in the user's settings. «Сбросить раскладку»
restores that workspace's default. Panels stay dockable and tabbable inside their workspace.
Stable tab ids (section 13) keep saved layouts valid when the interface language changes.

| Workspace | Left | Centre | Right |
|---|---|---|---|
| Every workspace | none | none | Inspector (shows the current selection) |
| **Карта** | tool strip; tabs Палитра \| Штампы | map canvas | minimap; tabs Слои \| Сцена; Inspector |
| **Сюжет** | outline with search; tabs Сюжет \| Персонажи \| Переменные (a click centres the node in the graph) | a switch: story graph (current map) or campaign graph | Inspector (trigger and dialog forms; dialog text shown inline) |
| **Данные** | tabs Юниты \| Предметы \| Тексты \| Пакет \| Файлы | Юниты and Предметы as sortable spreadsheets; Тексты as a key × language table; Пакет as a form; Файлы as a code editor | Inspector |
| **Тест** | none | a switch: Бой (battle tester), ИИ (AI viewer), Оценка (both scores, full size) | Inspector |

Notes on the table:

- **Карта, right side:** the minimap and the Слои | Сцена column sit to the left of the Inspector
  when something is selected; the Inspector is then a full-height column of its own. With nothing
  selected the Inspector column collapses to a thin strip (section 5.8).
- **Сюжет, graph switch:** the story graph shows the current map's triggers and dialogs; the
  campaign graph shows the map graph and its `next` edges.
- **Данные, file jump:** «Открыть в файле» on any object jumps to Данные › Файлы at the object's
  line (Ctrl+Shift+O).
- **Тест, jump:** score items (Оценка) and entries in the AI log select their object in Карта
  or Сюжет.
- **Играть (F5):** saves every open file, starts the real game on the current map from its hero
  start in a separate process, and returns to the studio when that window closes.

### 4.3 Detailed look of the Story, Data and Test workspaces

The Story, Data and Test workspaces are fixed here by layout and behaviour only. Their detailed
look is designed in each build milestone (UX5 for Сюжет and Данные, UX6 for Тест), with off-screen
screenshots, using the Inspector widget set (section 6) and the theme (section 8).

Data spreadsheets (UX5):

- sortable columns;
- the first column (the id) is frozen while the other columns scroll;
- numbers edit inline (Enter or F2; Esc cancels); other fields edit in the Inspector;
- a suggested price column (from DT's cost formula, `records.md` §5.2 and §6.3) with a «use»
  action that writes the suggestion to the price field;
- a filter field (Ctrl+F in the sheet).

## 5. The Map workspace

The approved look is the prototype `studio-ux:map` (commit `179f04f4` on `prototype/studio-ux`).
The layout at 1280×800 with a selection:

```
[tool strip 44] [palette 200] [canvas] [minimap / Слои | Сцена 304] [Inspector 380]
```

With nothing selected the last column collapses to a thin strip. The canvas is the largest area.

### 5.1 Tool strip

Fourteen tools, one icon each (Phosphor Regular), shown top to bottom. The Russian labels are
catalogue keys set in UX1; the letters are the shortcuts.

| Key | Tool | Icon note |
|---|---|---|
| V | Select and move (drag moves; Alt+drag duplicates an object) | |
| B | Brush (terrain, Холмы, Леса and the other paint tools) | |
| E | Eraser | |
| G | Fill (bucket) | |
| I | Eyedropper (also Alt while painting) | |
| M | Marquee selection | |
| L | Lasso selection | |
| W | Magic wand (contiguous or global, an option in the options bar) | |
| S | Stamp (places a saved region) | the Штампы tab uses the `stack` icon |
| A | Army | |
| U | Building (Постройки) | |
| P | Point | |
| R | Trigger zone (for `hero_enters` areas) | the `polygon` icon |
| H | Hand (pan) | |

Space held down pans with any tool. Middle-button drag pans. Ctrl+wheel zooms.

### 5.2 Options bar

A single bar above the canvas. Its contents depend on the active tool:

- Brush: size slider with its numeric field; DT size presets 1 to 6 and 9 as toggle buttons;
  round or square; density (per cent, 10 % steps); «Только в выделении» (paint only inside the
  selection).
- Eraser and stamp: the same size and preset controls.
- Magic wand: contiguous or global.
- Army, building, point and trigger zone: the options of that object kind; the placement preview
  follows the cursor (section 5.7).

At 1280 px the bar keeps its compact form: sliders are narrow and numeric fields sit beside them.

### 5.3 Palette (left column, tab Палитра)

- **Terrain:** colour swatches with names, one for each terrain kind of the 16-kind palette.
- **Objects:** thumbnails made at runtime from the player's install (`RAZDOR_DT_DIR`). Nothing
  of the original's art goes into git. With no install, a row shows the name and the icon.
- **Штампы tab:** saved regions (section 5.6). Stamps are stored in `.editor/stamps/`, or in the
  user's settings when they are shared between packages (as decided in the studio design, P2).
- `X` swaps the last two palette items.

### 5.4 Layers

The layer model has ids. Each layer is a definition in data (its id, name and kind); each map
keeps an instance of each layer keyed by that id (LDtk's definitions-and-instances model). Only
the fixed layers exist now; user layers come later and need no change to the model.

| Layer | Holds | Clicks |
|---|---|---|
| Местность | terrain cells | paint, fill, select |
| Объекты | decorative and blocking objects (Холмы, Леса and the like) | select, move, erase |
| Постройки | buildings, by footprint | select, place |
| Армии | army starts | select, place |
| Точки | points (heroes, spawns, lanterns) | select, place |
| Зоны триггеров | trigger areas (`hero_enters`) | select, draw |

Each layer row has an eye (visible) and a lock (no edits, no clicks). Alt+click on an eye shows
only that layer. The overlay group holds view-only toggles from the studio design's P2: the
passability grid, the patrol-zone overlay and the fog overlay. The overlay group has no data of
its own and never takes clicks.

Click filtering: a hidden or locked layer does not respond to clicks, marquee, lasso or wand,
and paint and erase skip it. Selections act on visible, unlocked layers only (section 5.6).

### 5.5 Brush

- **Size:** a slider in the options bar, `[` makes the brush smaller and `]` larger. The DT
  presets 1 to 6 and 9 are the size presets (keys 1 to 6 and 9 while the brush, eraser or
  stamp is active).
- **Shape:** round or square.
- **Footprint preview:** the cells the brush will touch are outlined under the cursor before the
  first click.
- **Gap-free strokes:** a fast drag paints every cell along the path between two samples; a stroke
  never leaves holes.
- **Straight line:** Shift+click draws a straight line from the last point of the stroke.
- **Density and scatter:** density in 10 % steps (Shift+`[` and Shift+`]`). Objects scatter at
  random within the density, using the random variants of the object kind.
- **Undo:** one stroke is one undo step, however many cells it touched. Ctrl+Z undoes the whole
  stroke.
- **Eyedropper while painting:** hold Alt to pick the cell's terrain or object without changing
  tools.

### 5.6 Selections

- **Marquee (M):** drag a rectangle. **Lasso (L):** draw a free shape. **Magic wand (W):** click a
  cell; the option chooses contiguous cells of the same kind or every matching cell on the map.
- **Add and subtract:** Shift adds, Alt subtracts while selecting (the Photoshop convention; Tiled
  and Godot use Ctrl, and the shortcut help says so).
- **Limits:** painting and fill stay inside the selection when «Только в выделении» is on (the
  options bar). Selections act on visible, unlocked layers only.
- **Display:** a marching-ants outline, drawn as a dashed line that moves (egui's dashed line
  with an offset).
- **Editing:** Ctrl+C, Ctrl+X and Ctrl+V copy, cut and paste. A paste is a floating layer over the
  canvas until Enter commits it; Esc drops it. Ctrl+Shift+V pastes in place. Delete clears the
  selection's contents.
- **Stamps:** Ctrl+Alt+S saves the selection as a stamp (Ctrl+Shift+S is Save all). A stamp appears
  in the Штампы tab.

### 5.7 Object placement

Placing an army, building, point or trigger zone shows a transparent preview under the cursor
(the VCMI and Tiled behaviour). Esc or a right-click cancels the placement.

### 5.8 Inspector column behaviour

- A selection opens the Inspector as a full-height right column (380 px at 1280×800).
- With nothing selected the column collapses to a thin strip (its width is set in UX3).
- At 1280×800 with a selection, the four columns (tool strip, palette, layers column, Inspector)
  leave about 350 px of canvas. UX3 decides how this is resolved (the layers column narrows, or
  the palette hides with Tab) and shows the choice in its screenshots. The prototype's sizes are
  in section 8.2.

### 5.9 Status bar on the Map workspace

Cell coordinates under the cursor; the terrain under the cursor; the active tool's one-line hint;
the save state.

## 6. The Inspector widget set

The widget set was approved in #42 as shown in `studio-ux:inspector-army`. It is the single form
set for every object and file in the studio. UX2 applies it to every existing form (army,
building, point, object, trigger, dialog, unit, item, package, map settings, campaign).

- **Header:** icon, name and type of the selection (for example, an army).
- **Field search:** a text box at the top. It filters the fields and opens the sections that
  match (Ctrl+F when the Inspector has focus).
- **Sections:** collapsible, each with a name. A section's open state survives changes to the
  number of its items (the section's id is salted, so its state is not lost when the count
  changes).
- **Label column:** a fixed width of 112 px (the token `LABEL_W`) in both placements, so the values
  line up.
- **Changed fields:** a changed field shows an accent dot before its label and a reset icon in a
  fixed slot at the end (`arrow-counter-clockwise`, disabled when the field equals its default).
- **File keys:** a field's file key (for example `army.owner`) is shown in its tooltip. A setting
  «Ключи файла в подписях» shows each key under its label instead.
- **Lists:** drag to reorder, with a grip handle (`dots-six-vertical`). A drop line (3 px accent
  bar) shows where the item will land; the selected row has a 1 px accent border. Add, duplicate
  and remove are icon buttons under the list.
- **Price block (units and items):** the suggested price as a number with a short explanation,
  then the buttons «Применить» and «Оставить». UX5 gives the spreadsheet the same suggestion as
  a column.

## 7. Dialogs and popups

- A popup or a small window takes the window tokens (section 8.1 and 8.2): panel fill, a 1 px line
  stroke, a 6 px corner radius and a margin of 8 px.
- Esc closes a popup (section 9.3).

## 8. Theme

### 8.1 Colour tokens

The exact values from the approved prototype (`src/ui/studio/ux_proto.rs`, module `tok`):

| Token | Hex | Use |
|---|---|---|
| `BG` | `#15171b` | the window background behind the panels (the clear colour) |
| `PANEL` | `#1f2227` | panels, the top bar, the status bar, the drawer, popups |
| `RAISED` | `#292d34` | inactive widgets (buttons, fields) |
| `HOVER` | `#333841` | hovered and active widgets |
| `WELL` | `#131518` | input wells (text fields, the search field, the field frames) |
| `LINE` | `#353a43` | panel and widget strokes, frame borders |
| `TEXT` | `#e4e7ec` | body text, active labels |
| `DIM` | `#a0a7b2` | secondary text, icon hints |
| `FAINT` | `#6e7580` | hovered border, tertiary text, placeholders |
| `ACCENT` | `#4f8cff` | the one accent: selection, active tool, changed-field dot, primary buttons, selection outline |
| `DANGER` | `#e06c6c` | errors, destructive confirmations |

Selection text uses a 1 px white stroke on the accent background (`selection.stroke`).

### 8.2 Sizes and spacing

| Token | Value | Use |
|---|---|---|
| `TEXT_SIZE` | 13.5 px | Body and Button text styles |
| `SMALL` | 12.0 px | Small text style, status bar, drawer tabs, canvas labels |
| `ICON` | 16.0 px | icons in buttons and tabs |
| Heading | 16.0 px | Heading text style |
| Monospace | 12.0 px | the code view |
| `BTN_H` | 26 px | button and field height (`interact_size.y`) |
| `ROW_H` | 24 px | list and tree rows |
| `LABEL_W` | 112 px | Inspector label column |
| `TOP_H` | 36 px | top bar |
| `STATUS_H` | 24 px | status bar |
| drawer open | 186 px | bottom drawer when open |
| drawer closed | 24 px | bottom drawer handle |
| tool strip | 44 px | tool strip width |
| palette | 200 px | Map palette column |
| right column | 304 px | minimap and layers column |
| Inspector | 380 px | full-height Inspector column |
| item spacing | 6 × 4 px | `item_spacing` (x, y) |
| button padding | 8 × 3 px | `button_padding` (x, y) |
| scroll bar | 8 px | `scroll.bar_width` |
| widget corner radius | 4 px | all widget states |
| window corner radius | 6 px | popups and windows |
| small icon button | 24 × 24 px | reset and list icon buttons |

Frame inner margins used in the prototype (x × y):

| Frame | Margin |
|---|---|
| top bar | 8 × 4 |
| status bar | 8 × 2 |
| bottom drawer | 8 × 3 |
| tool strip | 5 × 6 |
| palette | 8 × 6 |
| right column | 8 × 6 |
| Inspector | 10 × 8 |
| field well | 8 × 6 |
| toggle buttons | 4 × 2 |
| palette search field and result rows | 8 × 5 (the row when active: hover fill and accent border) |
| window | 8 on all sides |

Toggle buttons (for example the DT presets) have a 1 px stroke: the accent when on, `LINE` when off.

### 8.3 Fonts and icons

- **UI font:** Noto Sans Regular (SIL Open Font License 1.1, no Reserved Font Name), re-subset with
  U+0400 to U+045F so the Cyrillic letters are present. The subset costs about the same bytes as
  the old one (research #38). It replaces egui's default fonts in the studio, so egui's default
  Ubuntu-Light (Ubuntu Font Licence; its GPL compatibility is not verified) no longer ships in the
  studio build.
- **Icons:** `egui-phosphor` 0.9.0, Regular weight (MIT). Phosphor is drawn as an icon font,
  about 489 KB. Every icon in the studio comes from this set; there are no custom pixel icons.
  Substitutes: `stack` for the layers and Штампы, `lock-simple-open` for unlock, `polygon` for the
  trigger zone.
- **Crispness** at 16 px and 24 px is checked in the UX1 snapshots (research #38 reasoned it but
  did not render it).

## 9. Command registry and shortcuts

### 9.1 The registry

Every action in the studio is a command with:

- an id (stable; the saved bindings refer to it);
- a name in the UI catalogue (`data/lang/ru/studio.txt`, `data/lang/en/studio.txt`);
- an icon from the Phosphor set, where it has one;
- a default shortcut and a context (global, Map, Story, Data, Test, Inspector, graph, sheet);
- a tooltip: the name and the shortcut, always.

Buttons, menus, the command palette and the shortcut help read the same registry, so a name or a
key is written once.

### 9.2 Rebinding

Settings (Ctrl+,) lists every command with its binding. A new binding that clashes with another
binding in the same context is flagged, and the flagged one is not saved until the clash is
resolved. «Сбросить» restores the default for one command, and restores all defaults (the table in 9.3).

### 9.3 Default shortcuts

The default bindings, as decided in #43. The table is the spec's default list; a player's
bindings override it.

**Global**

| Action | Key |
|---|---|
| Save / Save all | Ctrl+S / Ctrl+Shift+S |
| Undo / Redo | Ctrl+Z / Ctrl+Shift+Z (also Ctrl+Y) |
| Command palette / Go to object or file | Ctrl+Shift+P / Ctrl+P |
| Workspaces Карта, Сюжет, Данные, Тест | Alt+1 … Alt+4 |
| Проверка | F7 |
| Играть (save, start the game on this map) | F5 |
| Bottom drawer | Ctrl+J |
| Hide / show side panels | Tab |
| Find in the current view | Ctrl+F |
| Open the selection in its file | Ctrl+Shift+O |
| Cancel / close popup / drop floating paste | Esc |
| Settings | Ctrl+, |

**Map: tools**

| Tool | Key |
|---|---|
| Select / move | V |
| Brush | B |
| Eraser | E |
| Fill | G |
| Eyedropper | I |
| Marquee | M |
| Lasso | L |
| Magic wand | W |
| Stamp | S |
| Army | A |
| Building | U |
| Point | P |
| Trigger zone | R |
| Hand | H |

**Map: modifiers and actions**

| Action | Key |
|---|---|
| Brush size − / + | [ / ] |
| Density − / + 10 % | Shift+[ / Shift+] |
| DT size presets | 1–6, 9 (brush, eraser, stamp active) |
| Straight line from the last point | Shift+click |
| Eyedropper while painting | Alt (hold) |
| Swap the last two palette items | X |
| Pan | Space (hold) + drag, middle drag, H |
| Zoom in / out / fit / 100 % | Ctrl+= / Ctrl+− / Ctrl+0 / Ctrl+1; Ctrl+wheel |
| Selection: add / subtract | Shift / Alt while selecting (Photoshop; Tiled and Godot use Ctrl, noted in the help) |
| Select all / Deselect / Invert | Ctrl+A / Ctrl+D / Ctrl+Shift+I |
| Copy / Cut / Paste / Paste in place | Ctrl+C / Ctrl+X / Ctrl+V / Ctrl+Shift+V |
| Commit floating paste | Enter |
| Delete selection contents or object | Delete, Backspace |
| Duplicate object | Alt+drag (V tool) |
| Save selection as stamp | Ctrl+Alt+S (Ctrl+Shift+S is Save all) |
| Show only this layer | Alt+click its eye |

**Story graphs**

| Action | Key |
|---|---|
| Frame the selection / fit all | F / Home |
| Find node | Ctrl+F |
| Remove node | Delete |
| Duplicate node | Ctrl+D |

**Data spreadsheets**

| Action | Key |
|---|---|
| Move between cells | arrows |
| Edit cell | Enter or F2 |
| Cancel edit | Esc |
| Copy / paste cells | Ctrl+C / Ctrl+V |
| Filter | Ctrl+F |

**Inspector**

| Action | Key |
|---|---|
| Search fields (when the Inspector has focus) | Ctrl+F |

Context note: Ctrl+F, Ctrl+D and Ctrl+C/V appear in several contexts with different actions. Each
entry belongs to its context; the command registry resolves the one that applies to the focused
panel.

### 9.4 Keys handled from events, not keys

Ctrl+C, Ctrl+X and Ctrl+V are handled from egui-miniquad's Copy, Cut and Paste events, not from
key events: egui-miniquad turns those keys into the events, so a key handler never fires (research
#39). See section 13.

## 10. Command palette

- **Ctrl+Shift+P** opens the command palette for actions. Typing filters the commands by their
  names in the current language; the palette shows each command's shortcut and context.
- **Ctrl+P** opens «Go to»: objects (by id and name) and files of the package. Choosing one selects
  it and moves the workspace to it (Карта for a map object, Сюжет for a trigger or dialog,
  Данные › Файлы for a file).
- The palette is drawn over the workspace as a popup (section 7). Esc closes it.
- Its top-bar entry is the command field. It lands with UX6 (section 14).
- The palette window is 580 × 330 px, with a search field at the top, then rows of command or go-to results, each with its icon, name, group and shortcut. The active row has the hover fill and an accent border.

## 11. Start screen

Shown on `razdor --studio` without a folder, and after «Файл › Закрыть пакет».

- **Recent packages:** up to 8, each with its name, its map count and when it was last opened.
- **Actions:** «Новый пакет», «Открыть папку», «Открыть пример» (Пепельная Марка).
- **Links:** the wiki guide to the studio (the player wiki).
- **Footer:** the version.
- Dark theme (section 8), Phosphor icons.

## 12. UI scale

- A setting «Масштаб интерфейса» from 80 % to 150 %, in 10 % steps. It sets egui's zoom factor.
- The default is 100 % or, when the screen's DPI is known, the value that the DPI suggests.
- The 1280×800 minimum window is a requirement at 100 %.
- Text is never drawn below 12 px on screen. At 80 % the smallest text token (12 px) would draw at
  9.6 px, so the studio raises the small size logically to 15 px at 80 % (12 ÷ 0.8). The tokens of
  section 8.2 are the values at 100 %.

## 13. Pre-existing defects, fixed first (UX1)

Three defects are found in the code by reading it (research #39). They are fixed before any
new screen, because the later work depends on them.

1. **Copy, cut and paste.** Ctrl+C, Ctrl+X and Ctrl+V are handled from egui-miniquad's Copy, Cut
   and Paste events. They are not key events, so a key handler never sees them.
2. **Cursor override.** The studio sets the cursor every frame, so egui's own cursors (text
   editing, resize) are overridden. The studio stops overriding the cursor, and egui's text and
   resize cursors show.
3. **Tab ids.** Dock tabs take their id from their localised title, so a language change breaks
   every saved layout. Each tab gets a stable id through `TabViewer::id`, and layouts are saved
   only after this change.

The research observed these defects by reading the code. They are confirmed at runtime in UX1
(the copy and paste test, the cursor check and a language switch with a saved layout).

## 14. Build milestones

Each milestone is built on `v2-packages` in its own agent worktree, reviewed, and pushed. The owner
reviews the screenshots of all six at the end (the same policy as phase 2). Every milestone runs
the UI regression check (`tools/ui-regress`) and the offscreen snapshots (the `offscreen-snapshot`
skill) at 1280×800 and, for the layouts, at 1920×1080.

| Milestone | Delivers | Screenshots (off-screen) |
|---|---|---|
| **UX1 Foundation** | the three defect fixes (section 13); the theme tokens (8.1 to 8.2), the fonts and icons (8.3); the command registry, rebinding in Settings with the conflict check, tooltips with shortcuts (section 9); the workspace frame (section 4.1); four workspaces, each with its own saved dock layout, and «Сбросить раскладку»; the start screen (section 11); UI scale (section 12) | start screen at 1280×800 and 1920×1080; each workspace's empty frame; drawer open on Проблемы; scale 80 %, 100 % and 150 %; a Settings page with a binding conflict; a Cyrillic check of the new font; a language switch with a saved layout |
| **UX2 Inspector** | the widget set (section 6) applied to every existing form: army, building, point, object, trigger, dialog, unit, item, package, map settings and campaign | one screenshot per form at 1280×800, with one changed field and one open list; the inline-keys setting on and off |
| **UX3 Карта** | the tool strip (section 5.1); the options bar (5.2); the palette with terrain swatches and object thumbnails from the install, with the name-and-icon fallback (5.3); the layer model with eyes and locks, the overlay group and click filtering (5.4); the brush (5.5); the Inspector column behaviour (5.8) with the chosen width resolution; the status bar (5.9) | the map at 1280×800 with and without a selection; the palette with and without an install; a brush footprint; a stroke with a Shift line; a hidden and a locked layer; the minimap column; the collapsed Inspector strip |
| **UX4 Selections** | marquee, lasso, magic wand (contiguous and global); add and subtract; painting and fill limited to the selection; the marching-ants outline; copy, cut and paste with a floating paste; delete; save as a stamp (5.6) | each selection tool mid-use; a floating paste; a stamp in the Штампы tab |
| **UX5 Сюжет and Данные** | the outline with search and its three tabs; the graph switch (story and campaign); the spreadsheets (4.3); the texts table moved into Данные; the Пакет form; Файлы with the code editor; «Открыть в файле» | each workspace with a selection; a sorted spreadsheet with the frozen id column; the filter field; the texts table with a missing key marked; the code editor with a jump |
| **UX6 Тест, commands, Играть** | the Тест workspace: Бой (battle tester), ИИ (AI viewer: read-only canvas, day slider, log), Оценка (both scores, full size); jump-to-object from score items and AI-log entries; «Играть» (F5); the command palette and Ctrl+P go-to (section 10) | each Тест view; a jump from a score item into Карта; the palette over a workspace with a typed query; the Ctrl+P list |

The Тест views' contents (the battle tester, the AI viewer and the scores) follow the studio
design's P7 and P8. This spec fixes their place, their switch and their jumps, not their data.

## 15. Later, and out of scope

**Later (not in this spec's milestones):**

- the first-run tour, at the stable release;
- a light theme, as a second token set with the same names;
- user layers: the layer model already allows them, with no change to the model;
- pen tablets (pressure for brush size or strength).

**Out of scope:** touch screens, tablets and phones. The studio is a desktop tool; it is compiled
out of Android and iOS (the studio design, decision 5).

## 16. Testing and checks

- **Off-screen snapshots:** each milestone's screens are drawn off-screen with the
  `offscreen-snapshot` skill, at 1280×800 and at 1920×1080 for the layouts, and the owner
  reviews them at the end.
- **UI regression check:** `python3 -I tools/ui-regress/ui_regress.py run`. The changed references
  are reviewed and accepted per milestone.
- **Planned unit tests** on the command registry: every default binding has a unique key in its context;
  a rebinding that clashes is refused; a saved binding round-trips.
- **Layout tests:** a saved dock layout with a tab id survives a change of language; «Сбросить
  раскладку» restores the default.
- **Brush tests:** one stroke is one undo step; a fast drag leaves no gap between two samples.
- **Selection tests:** a magic wand contiguous and global pick; add and subtract; painting limited
  to the selection; a hidden or locked layer takes no selection.
- **Clipboard test:** Ctrl+C, Ctrl+X and Ctrl+V reach the studio from the Copy, Cut and Paste
  events (section 13).

## 17. Open points

These are not decisions. Each has a milestone where it is settled.

- **Width at 1280×800 with a selection (UX3).** The four columns leave about 350 px of canvas
  (section 5.8). UX3 chooses between a narrower layers column and hiding the palette with Tab,
  and shows the choice.
- **Collapsed Inspector strip width (UX3).** Not fixed in the prototype; UX3 sets it.
- **Small text at 80 % (UX1).** Section 12 raises the small size to 15 px logical at 80 %, so the
  12 px on-screen floor holds; UX1 confirms the result in the snapshots, since the token table
  (section 8.2) is the value at 100 %.
- **Tool labels (UX1).** The Russian names of the tools are catalogue keys and are set in UX1.

## Sources

Issues (`indicozy/razdor`):

- #36, the wayfinder map «Razdor Studio UX: the spec for a human-friendly editor»: the destination,
  the charting decisions and «Decisions so far».
- #37, how other editors do it (Карта, Данные, Тест; layers as data with ids). Resolution in the
  research file `docs/research/studio-ux-editor-patterns.md` on branch
  `research/studio-ux-editor-patterns` (commit `19de6485`).
- #38, icons and UI font (Phosphor Regular via egui-phosphor 0.9.0; Noto Sans with Cyrillic).
  Research file `docs/research/studio-ux-icons-font.md` on branch `research/studio-ux-icons-font`.
- #39, what egui 0.31 and egui_dock cover, and the three pre-existing defects. Research file
  `docs/research/studio-ux-egui-capabilities.md` on branch `research/studio-ux-egui-capabilities`.
- #40, the workspaces, their panels and behaviours (the table in section 4.2).
- #41, the Map workspace prototype, approved; the Inspector gets its full-height column (section 5).
- #42, the Inspector widget set prototype, approved (section 6).
- #43, the shortcut table (section 9.3).
- #44, this spec.

Prototype: branch `prototype/studio-ux` (commit `179f04f4`, file `src/ui/studio/ux_proto.rs`). A
throwaway; its theme tokens and sizes are taken from it (section 8). Not merged.

Existing design: [`2026-11-03-razdor-studio-design.md`](2026-11-03-razdor-studio-design.md) on
`v2-packages`, for the layout, the files-are-the-truth rule and phase 2 (P1 to P9).
