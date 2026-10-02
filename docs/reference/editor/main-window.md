# Discord Times map editor: the main window and its tools

This file describes the main window of the original scenario editor: how it starts, its menus and
toolbar, the keyboard and mouse, scrolling, how the map is drawn, the minimap, and every editing
tool. The tools cover painting terrain, hills and forests, and placing buildings, armies, lanterns,
event points and hero starts, with every default value they write. The file also covers moving
objects, the Info mode, the bulk "burn" tool, the hidden tools, quick save, the buildings and
armies submenus, and the order and look of the lists the record editors share.

The source is `DTMapEdit.exe` of the Discord Times Community Update (32-bit Delphi, image base
0x400000), read statically only. Everything here is in our own words. Addresses are virtual
addresses in that build, given as evidence. Controls are named by what they do, not by their
on-screen captions. The record editors (building, army, point, events, scenario options), the
file format, the new-map dialog, the world generator, the map check and the AI and battle tests
have their own specs. Here they appear only where the main window calls them. The file layout is
in [dtm-format.md](../dtm-format.md).

Confidence tags: **code** = read in the code; **data** = consistent with the install's files, not
traced; **unknown** = not determined.

Conventions:
- A *cell* is one map square. On screen a cell is 32 × 22 pixels.
- *W* × *H* is the map size. The *view origin* is the top-left map cell shown in the view.
- Per map cell the editor keeps:
  - a terrain code (0–15);
  - a *mark* byte: the terrain code, a negative object class, or a building-footprint value;
  - an *object layer* for hills, mountains and rocks (classes 1–8);
  - a *forest layer* for classes 9–12;
  - a building-anchor word;
  - a *figure word*: the kind in the high byte, the record index in the low byte;
  - a reveal counter used for fog.
- Item kinds, as the palette numbers them:
  - 1 knight start, 2 archmage start, 3 ranger start;
  - 4 feudal army, 5 robber band, 6 peasant crowd, 7 inactive army;
  - 8 lantern, 9 event point;
  - 10 target place (no palette cell, see §16.3).
- `Rand(n)` is the editor's linear congruential generator, which returns 0..n−1 (0x4dc6b8).
- Integer division truncates toward zero.

---

## 1. Start-up

### 1.1 Single instance (code, entry 0x5b3fa4)

1. The program sets its application title, then walks the top-level windows.
2. Another unowned window with the same title and the same application window class means an
   editor is already running. In that case:
   - If the first command-line argument contains `.DTM` (case-insensitive), that path is put on
     the clipboard.
   - The running editor is brought to the front and restored.
   - The new process exits.
3. Otherwise the main window is created and the message loop runs.

The running editor picks the path up when it regains focus (§17.3).

### 1.2 Checks, in order (code, main form creation 0x59d684)

1. **Colour depth:** the screen must be 16 or 32 bits per pixel, otherwise a message appears and
   the editor exits (0x59d740).
2. **Install check:** the language ini `DTmapEdit_Rus.Ini` and `Graphics\Editor\E1.Tga` must exist
   next to the exe, otherwise the editor shows an anti-copy message and exits (0x59d7a4).
3. **Cursors:** six custom cursors are loaded as cursor ids 100–105 (§6.2).
4. **Game data:** units, spells and artefacts are read, and the unit and artefact sort orders are
   built (§19).
5. **Version:** the `Ver` key in section `[Label]` of the language ini must equal the exe's file
   version, otherwise the editor exits with a message (0x59dd7e).
6. **DirectDraw:** if a `DDraw.Dll` sits next to the exe, its size must be one of 2 835 968,
   3 580 928 or 3 602 944 bytes. Any other size shows a warning and the editor exits (0x59de3a).
   The language ini's text names the matching wrapper builds (**data**).
7. **Screen size:** a screen wider than 1920 or taller than 1440 pixels raises a warning. Only the
   abort answer exits (0x59de9d).
8. **Command line** (case-sensitive, first argument only, 0x59df01):
   - `NewMap` runs the new-map dialog without a main window, then exits.
   - `Battle` builds or loads a cached sheet of unit portraits (`Graphics\Editor\UnitMax.Bmp`,
     made from `Icons.Ugs` when missing, 0x598730), opens the test battle, then exits.
   - A path containing `.DTM` becomes the start map (step 13).
9. **Captions:** captions and hints come from the language ini section `[Menu]`, keyed by
   component name. All separators share one key. Toolbar buttons store their key in their hint
   (0x59dfad). The two mode check boxes take the hints of their menu items.
10. **Name pools:** sections `[Names]` and `[Heros]` are read as numbered lists. A line `#n`
    starts list *n*, the list for building type *n* (§10.4).
11. **Texts:** about twenty texts come from `[Label]`. The five tool-page buttons and the three
    panel titles get their captions from one semicolon-separated key.
12. **Defaults:**
    - map 50 × 50, view origin 0,0, Info tool;
    - minimap in its "simple colours" mode (§9);
    - the engine is set up and the object and building pictures are loaded; failure exits.
13. **Settings:** `DTmapEdit.Ini` must exist next to the exe, otherwise the editor exits. From its
    `[Option]` section:
    - `FindBuildingPlace`: true when its first four letters are TRUE, in any case. It sets the
      building-place check (§14.2).
    - `PathMap`: the map folder.
    - `WorkMap`: the full path of the last map.

    Then:
    - A `.DTM` argument replaces `WorkMap` and its folder becomes the map folder.
    - If the folder is still empty it becomes the exe folder plus `Maps_Rus\`. When the exe path
      contains the word "Ulmo" it is `Maps\` instead (0x59ec5d).
    - If `WorkMap` contains a folder, the map folder is set to it.
    - The map is loaded from the folder plus `WorkMap`'s file name. If loading fails, a new map
      is made (§3.1).
14. **Final steps:**
    - The terrain page is selected (§4.1) and keyboard focus goes to the map.
    - Hint timing is set: the palette and map hints stay up 1.5 s (Application.OnShowHint
      0x5984d4).
    - The toolbar is laid out.
15. **Word check:** every unit description is searched for seven encoded words. A hit shows a
    message and the editor exits (0x59f052).

The `[Option]` keys `NewEventsRepeat`, `TextFontSize` and `TextFontBold` belong to the options
window and the events editor. `[MakeMap]` belongs to the new-map dialog. They are not covered
here (**code**, 0x55d7b0, 0x532654).

### 1.3 Closing (code, 0x5a3a84)

1. If the map is modified, the editor asks yes/no/cancel. Cancel keeps the window open; yes saves
   as the plain Save does (§3.1).
2. Redrawing stops and the test-AI state is freed.
3. Two keys are written to `DTmapEdit.Ini` `[Option]`:
   - `WorkMap`: the save dialog's file name, or the open dialog's when the save dialog has none.
     Nothing is written if both are empty.
   - `FindBuildingPlace`: the current check flag, written as a boolean word.

   `PathMap` is never written.

---

## 2. Window layout (code, form resource; 0x59f87c)

The design size is 1024 × 670 client pixels, and the window can be resized.

- **Top:** a toolbar of 28-pixel buttons. It also holds the scenario label and the playability
  label with its compute button.
- **Right:** a 272-pixel panel, top to bottom:
  - the minimap title (clickable, §9);
  - the minimap panel;
  - the tool panel, holding:
    - the brush-size title (clickable, §16.1);
    - a row of buttons: sizes 1–6, Delete, Info;
    - the palette title with two small check boxes for the two modes (§14);
    - the five tool-page buttons;
    - the palette grid, three columns of 80 × 91-pixel cells.
- **Left, the map area:**
  - a top ruler of column numbers and a left ruler of row numbers;
  - a small corner button that gives keyboard focus back to the map;
  - the map view;
  - a vertical and a horizontal scrollbar.

The view shows `min(room ÷ 32, W)` columns and `min(room ÷ 22, H)` rows. Rulers and scrollbars
are resized to match. The scrollbars run from 0 to W − columns and 0 to H − rows. A 10 ms timer
drives the preview and the minimap hover frame.

---

## 3. Menus and toolbar

### 3.1 Menu (code, form resource; handlers in brackets)

| Menu | Item | Action |
|---|---|---|
| File | New (Ctrl+N) | Asks to save if modified. Clears the map at the **current** size: terrain 0, no records, the default title. Sets the file name to `New.DTm` and refreshes the submenus (0x5a4274). |
| File | Generate (Ctrl+G) | Asks to save, frees the test-AI state, runs the new-map dialog and generator (0x5a2db0). |
| File | Open (Ctrl+O) | Asks to save. The open dialog offers the normal format and the demo format (0x59ffdc). |
| File | Save (Ctrl+S) | Saves straight to the map folder plus the current file name, without a dialog. A map still called `New.DTm` gets the dialog instead (0x5a0194). |
| File | Save as | Always shows the save dialog. Its filters are normal, uncompressed and demo (0x5a0194, Tag 1). |
| File | Editor options | Opens the options window (0x5b38fc). |
| File | Show errors | Opens the map check (0x5b38e4). |
| File | Exit (Ctrl+Q) | Closes the window (§1.3) (0x59ffc0). |
| Edit › Object | five items | Each selects a tool page: terrain, hills, forests, buildings, items (0x5b19b8). |
| Edit › Size | 1×1 … 6×6 | Each presses that size button, if enabled (0x5b1728). |
| Edit | Info | Presses the Info button. |
| Edit | Move | Move mode (§12) (0x5b1df0). |
| Edit | Delete | Presses the Delete button. |
| Edit | Grid | Toggles the passability grid. Turning it on rebuilds the marks (§8.4) (0x5a37a8). |
| Edit | Patrol zones | A check item with **no handler**, so clicking it does nothing. The toolbar button does the toggling (0x5a37f4). |
| Edit | Fog | Toggles the fog overlay (0x5a382c). |
| Edit | Tree replacement mode | Toggles the mode, kept in step with its check box (0x5b2590). |
| Edit | Ignore mountains mode | Toggles the mode, kept in step with its check box (0x5b25e8). |
| Edit | Burn everything | §15 (0x5b29d4). |
| Scenario | Scenario options | Opens the scenario options window (0x5a4558). |
| Scenario | Events | Opens the events editor with no event selected (0x5afc0c). |
| Scenario | Named characters | Opens the named-characters window (0x5b1c70). |
| Scenario | Buildings › 12 type groups | Built at run time (§18). |
| Scenario | Armies › 4 groups | Built at run time (§18). |
| Scenario | Artefacts | Opens the artefact window (0x5b166c). |
| Scenario | Unit stats | Opens the unit window (0x5b16dc). |
| Help | Manual | Starts Microsoft Word through OLE automation and opens the editor manual in the `Help` folder (0x5b1c88). |
| Help | About | Shows the splash form with a close button and no progress bar (0x5acf28). |

### 3.2 Toolbar (code, form resource)

From the left:
- new, open, save, generate;
- fill the scenario with buildings and armies (the world generator, 0x5b1684);
- scenario options;
- events, artefacts, units;
- grid, patrol zones, fog;
- the scenario label;
- the playability compute button and its label (0x5b3960; scoring belongs to the map-check
  spec).

At the right end: test AI (0x5b169c), test battle (0x5b16bc) and exit. A hidden ninth button
(§16.2) sits between them.

The save button and the save menu item are enabled only while the map is modified. They are
refreshed on every redraw (0x5ab588).

The **window caption** reads: the main caption from `[Label]`, the exe version, the file name, and
the format number taken from the map's signature (0x5a0388).

---

## 4. Tool state

The editor's whole tool state is:
- the **page** (0 terrain, 1 hills, 2 forests, 3 buildings, 4 items);
- the **brush** (−1 Delete, 0 Info, 1–6 a size);
- the **move-mode** flag;
- the **held object**, if any;
- the palette selection of each page.

### 4.1 Choosing a page (code, 0x5aa0f8)

Choosing a page does the following:
1. It leaves move mode and selects the first palette cell.
2. It sets the brush and presses a size button:

   | Page | Brush and pressed button |
   |---|---|
   | 0 terrain | size 1 |
   | 1 hills | size 3 |
   | 2 forests | size 3 |
   | 3 buildings | size 1 |
   | 4 items | size 1 |

3. On the terrain page the Delete button and menu item are disabled; elsewhere they are enabled.
4. On the buildings and items pages the Size submenu and sizes 2–6 are disabled; only size 1
   stays.
5. The chosen page's button is drawn bold, and the palette grid moves under it.

### 4.2 Size, Info and Delete buttons (code, 0x5a4100, 0x59514c)

- These buttons work as one radio group. Pressing one leaves move mode and sets the brush to its
  value.
- On the terrain page, pressing one also gives keyboard focus back to the map.
- On the hills page the size picks which hills the palette lists: hills are grouped by footprint
  size (§10.2).
- On the forests page:
  - sizes 2–6 list one entry per tree family;
  - size 1 lists single sprites.

### 4.3 Palette clicks (code, 0x5aaa7c)

The palette cell index is row × 3 + column. A click selects:

| Page | Selection |
|---|---|
| 0 terrain | terrain code = index; only cells 0–15 count |
| 1 hills | the index-th hill of the current size group |
| 2 forests | a family representative (sizes 2–6) or a single sprite (size 1) |
| 3 buildings | the index-th building picture. The brush becomes that picture's anchor width in cells, used for the preview and the fit test. |
| 4 items | item kind = index + 1. The grid has nine cells, so kinds 1–9. |

If Info or Delete was pressed, a palette click switches to size 1. On the hills page that click
does not change the selected hill, because the hill lookup needs a size above 0 and runs before the
switch; a second click selects it (0x5aaa7c).

### 4.4 Palette hints (code, 0x5aa528)

- **terrain:** the terrain's name from the language ini. The last terrain (code 15) has no hint
  because of an off-by-one test.
- **hills and forests:** the class name and the sprite code.
- **buildings:** the type name.
- **items:** the kind name.

The palette draws each building with its footprint size and each item with its figure and a
short label (0x5a986c).

---

## 5. Keyboard (code, 0x5b3714, 0x5b3914, 0x5b3934)

The form handles keys only while the form itself has focus:

| Key | Action |
|---|---|
| Arrow keys | Move the view one cell. They work only when no control has focus. |
| Space | Info tool and leaves move mode. It does not drop a held object. |
| F2 | Quick save (§17.1). |

- The scrollbars swallow every key, so after clicking a scrollbar the arrows do nothing.
- The small corner button above the left ruler gives focus back to the map.
- Menu shortcuts: Ctrl+N new, Ctrl+G generate, Ctrl+O open, Ctrl+S save, Ctrl+Q exit.
- There is no undo, no zoom and no other key.

---

## 6. Mouse (code, 0x5ad0c4; mouse-down calls the same routine, 0x5aedf4)

### 6.1 Cells and the brush centre

- Outside the map view (over the rulers or scrollbars) the cursor is the normal arrow and the
  preview is hidden.
- Inside, the hovered view cell is column = (x − left edge − 1) ÷ 32 and row = (y − top edge − 1)
  ÷ 22. The two rulers highlight that column and row.
- The **brush centre** is the hovered cell plus brush ÷ 2 in both axes. Every tool works on the
  square of side *brush* that ends at the centre (its bottom-right cell):
  - odd sizes are centred on the hovered cell;
  - even sizes reach one cell further right and down.

### 6.2 Cursors

| Id | When |
|---|---|
| 100 | Info, or painting |
| 101 | Over an object in Info mode, outside move mode |
| 102 | Over an object in move mode |
| 103 | Delete tool |
| 104 | Holding an object |
| 105 | Forbidden spot (§14.2, §10.5) |

### 6.3 Buttons

- Only exact button states count. A left press with Shift, Ctrl or Alt held does nothing.
- The right button only picks up objects (§12).
- **Dragging** with the left button held keeps acting only on the terrain and forest pages. On the
  other pages each press acts once.

The left button does:

| State | Left press |
|---|---|
| Info tool, nothing held | Opens the object under the cursor (§13) |
| Size 1–6, nothing held | Paints or places (§10), marks the map modified, starts a stroke |
| Delete tool | Deletes at the hovered cell (§11) and marks the map modified, even when nothing was deleted |
| Holding an object | Drops it (§12) |

Releasing the button after a stroke redraws the minimap. The map is redrawn whenever the hovered
cell changes.

### 6.4 Preview

While the cursor rests on the map, the cell under it shows a ghost of what a press would do: the
brush frame, the object, the building or the figure. The ghost disappears ten timer ticks after
the cursor stops moving (code, 0x5a3ff8, 0x5ab588).

---

## 7. Camera and scrolling (code, 0x5a3864, 0x59f87c)

- The view scrolls in whole cells. There is no zoom and no smooth scrolling.
- The scrollbar positions are the view origin. The engine's camera sits one cell further in,
  because the terrain grid has a one-cell border.
- Every scroll relabels the rulers with absolute cell numbers, moves the minimap frame and
  redraws.
- Scrolling methods:
  - the arrow keys and the scrollbars;
  - clicking or dragging on the minimap (§9);
  - the buildings and armies submenus, which centre the view on the object (§18).
- There is no edge scrolling and no drag-to-pan.

---

## 8. Drawing the map (code, 0x5ab588)

Nothing is drawn while the window is closing or the minimap is being rebuilt. Each frame draws
these layers in order.

### 8.1 Layers

1. **Terrain:** blended terrain textures from the engine (0x4dd368, see the engine notes).
2. **Low hills:** object-layer classes 1–4, drawn for every visible cell plus a margin of four.
3. **One painter's pass:** rows and columns from 8 before the view to 4 after it. In each cell,
   in this order:
   - the forest-layer object, with a small position-dependent offset (0x4dc700). The offset is a
     hash of the cell that reseeds the shared random generator as a side effect;
   - mountains and rocks (object classes 5–8);
   - buildings. The building is drawn from the diagonal cell that matches its footprint height,
     so it sorts with the objects in front of it. Bridges get a small extra offset;
   - the figure (§8.2).
4. **Preview** (§6.4):
   - terrain: the brush frame;
   - forests at a size above 1: a scatter of family sprites. The scatter uses its own fixed seed
     per cell, so it does not match what a click will place;
   - other objects: the selected one;
   - buildings: the picture;
   - items: the figure;
   - while holding an object: the held building, army or point.
5. **Fog** (when enabled): a dark tile on every cell whose reveal counter is 0. Hero starts and
   lit lanterns raise the counter around them (§10.6).
6. **Patrol zone** (when enabled and an army is under the cursor): the same dark tile on every
   cell outside the square of the army's patrol radius around it. An army with radius 0 darkens
   the whole view.
7. **Passability grid** (when enabled): one of seven tiles per cell (§8.4).

The frame is then flipped to the window and the minimap is redrawn.

**Emergency save:** if the display reports an error, the editor saves the map as `ErrorSave.DTm`
in the map folder, shows the error and exits (0x5ab588 tail; the same in the minimap, 0x5a5fa4).

### 8.2 Map figures (code, 0x5ab0f8)

| Kind | Figure |
|---|---|
| Hero start | Its class picture; a ship on water (terrain 0–2) |
| Army | Its kind's picture. On water: a ship type taken from the army record, or the default ship. On land, the leader can change it: an undead-type leader gives the undead figure, and two small sets of leader unit ids give two more special figures. |
| Inactive army | Its own figure |
| Lantern | Its own figure |
| Event point | One of four figures: with a radius and events, events only, radius only, or neither |

### 8.3 Marks (code, 0x5a2ed4 and the brush)

The mark byte drives the grid and the building-place check. It is:
- the terrain code by default;
- minus the object class on cells covered by a hill or forest object;
- a footprint value on building cells.

A hill's footprint is its sprite number ÷ 10 cells square, anchored at its bottom-right cell.
Marks are rebuilt in full when the grid is turned on, after a building is dropped, and after hills
are deleted. Painting writes them directly.

### 8.4 Grid colours (code, 0x5ac9c3–0x5acd9a)

| Value | Meaning | Where |
|---|---|---|
| 0 | normal | road, all plains, sand, clay, stone, scorched land, lowland |
| 1 | hard | marsh and snow; or normal ground under a low hill, tree or dead tree |
| 2 | harder | marsh or snow under a low hill, tree or dead tree |
| 3 | impassable | lava, bog, deep drifts, mountains, rocks, thickets |
| 4 | building | any cell covered by a building footprint |
| 5 | shallow water | shoals and coast |
| 6 | deep sea | deep sea |

1. The starting value comes from the terrain.
2. Object marks then adjust it:
   - mountains, rocks and thickets (classes 5–8 and 11) set 3;
   - low hills, trees and dead trees (classes 1–4, 9, 10) add one step, but only to a value below
     2, so water and impassable cells keep their value;
   - bushes (class 12) leave the value unchanged.

   A cell holds one object mark, so it gets at most one step.
3. Lava is always 3.
4. A building footprint overrides everything with 4.

---

## 9. Minimap (code, 0x5a61b0, 0x5a3db4, 0x5a3f48, 0x5a5ea8, 0x5b16fc)

- The minimap is a 257 × 257 image. Its frame matches the view: its width is
  columns × 257 ÷ W and its position is origin × 257 ÷ W, both rounded, and the same with rows and
  H.
- Moving the mouse over it shows a second, lighter frame centred on the mouse and clamped inside.
  The frame fades after ten timer ticks.
- Holding the left button drags the view. A click sets the view origin to size × frame ÷ 257.
- Clicking the minimap title switches between two modes:
  - **simple colours** (the start-up default): terrain colours, with object colours painted over
    their footprints, and buildings tinted by type: castles and forts red, villages yellow, ruins
    pale yellow, others grey;
  - **marks**: a colour per mark value.
- The minimap is rebuilt after each paint stroke ends, after a building is dropped, and on load,
  new and generate.

---

## 10. Painting and placing (code, brush routine 0x595390; checks in 0x5ad0c4)

All placement works on the square that ends at the brush centre (§6.1). The routine receives the
centre's view column and row and adds the view origin to both (0x59541f–0x59543c).

### 10.1 Terrain (size 1–6)

For every cell of the square inside the map:
- the terrain code is set;
- water and lava (codes 0–3) remove the forest object;
- deep sea (code 2) also removes the hill object;
- the mark takes the terrain code, unless it holds an object or building mark.

Dragging paints continuously.

### 10.2 Hills, mountains and rocks (object classes 1–8)

- The whole square must lie inside the map, otherwise nothing happens.
- One object is placed, anchored at the centre cell. Its footprint size equals the chosen size
  group.
- Every cell of the square gets the object's mark.
- It acts only on a press, not while dragging.

### 10.3 Forests (classes 9–12)

The square must lie inside the map.

**Size 1:** the selected sprite is placed on the centre cell. Dragging keeps painting. Tree
replacement mode does not apply at this size.

**Larger sizes:** every cell of the square gets a random member of the selected sprite's family:
1. The sprite is chosen with Rand(family size) from the family's group of twelve.
2. If the sprite has a "+120" alternate picture, a further Rand(5) = 0 picks the alternate.

The calls are made in that order, cell by cell, row by row from the bottom-right: right to left
along the bottom row, then the row above, and so on.

**Tree replacement mode** (§14.1): when on, only cells that already hold a tree of a compatible
family are re-rolled. Live and dead trees count as one family; thickets and bushes each count only
with themselves. Empty cells are left alone.

Every cell of the square gets the selected class's mark, including cells that tree replacement
mode left alone (code, brush routine 0x59560f–0x595be0).

### 10.4 Buildings

A placement needs all of:
- a building picture selected;
- fewer than 254 buildings;
- no building anchored at the centre cell;
- the brush square inside the map.

Nothing else is checked: overlap with other buildings, water, mountains or figures is allowed
(§14.2).

The new record is zeroed and then filled:

| Field | Value |
|---|---|
| Position | the centre cell (the bottom-right anchor) |
| Picture | the selected type and variant |
| Type | the picture type. Exceptions: house pictures 2–4 become obelisks (type 15) and house pictures 5–6 become ruins (type 12). |
| Footprint | from the picture table |
| Faction | 3 |
| Attitudes | the scenario's attitude row of faction 3 |
| Defence | by picture type: town 20, village 2, castle 15, fort 10, ruins 5, all others 0. House pictures turned into ruins keep 0. |
| Owner army | 0 (none) |

The footprint cells lose their forest objects and get the building mark.

**Names:** before naming, the random generator is reseeded with
x·11 + y·7 + variant·3 + picture type. The same spot always gives the same names. From the type's
name pool (§1.2 step 10), and from its owner pool where one is used:
- **Most types** (town, fort, tavern, market, church, shipyard, dungeon entrance, bridges,
  obelisk): a random name and a random owner name.
- **Village:**
  - variant 3 picks one of two "settlement" or "village" title pairs;
  - variant 4 picks one of two "mill" or "farm" pairs;
  - other variants use the "village" pair.

  The pair's first word goes before the name and its second before the owner (the word pairs come
  from `[Label]`).
- **Castle:** a random name and owner, each prefixed with its part of the "castle" pair.
- **Houses:**
  - picture 5 takes one of three fixed ruin names (entries 16–18 of the ruins pool);
  - picture 6 takes entry 19;
  - other houses get no name.
- **Altar:** variants 0–1, 2 and 3–4 each draw one of three names from their own slice of the
  pool.
- **Ruins:** each variant group draws from its own slice of the pool. No owner name.

The description is left empty.

The code ends with a test that would switch back to the Info tool after a building. The test can
never be true, so the building tool stays active (0x596cd9).

### 10.5 Items

Items are placed on the centre cell, with size always 1.

| Kind | Limit | What is written |
|---|---|---|
| Hero start | — | The hero's preset start moves here. The old cell's figure is cleared and its fog reveal undone, a radius-5 reveal is made here, and the figure is written. The old cell is cleared even when the start was never placed (0,0), so a figure standing on cell 0,0 is wiped from the map. |
| Army | 255 armies | The new army's index is the new count. The record is zeroed except: position; id word (kind and index); experience correction 100; behaviour style = kind − 4 (feudal 0, robbers 1, peasants 2). The inactive kind gets style 0 and "inactive at start". The name is a fixed English word and the number (0x59754c); the leader name and description are empty. The army editor then opens. |
| Lantern | 256 points | The record is zeroed except: position; id word; radius 10; lit. A number dialog then asks for the radius at the cursor (default 10), and its area is revealed. |
| Event point | 256 points | Zeroed except position and id word. The point editor then opens. |

- After any item the tool returns to Info, so each placement needs a fresh palette click.
- Lanterns and event points are meant to be refused on a cell that already holds a figure: the
  forbidden cursor shows and the press is ignored. The test, however, reads the cell at the hovered
  *view* position counted from the map's top-left corner, without adding the view origin
  (0x5ad4da). It is right only while the view origin is 0,0; once scrolled, it checks an unrelated
  cell, so a lantern or point can overwrite a figure, and an empty cell can be refused.
- Hero starts and armies have no such check. They overwrite the cell's figure word, which leaves
  the earlier record off the screen.

### 10.6 Reveal radius (code, 0x594e04)

The reveal routine adds to, subtracts from, sets or toggles the reveal counter of every map cell
within the radius. Hero starts use radius 5 and lanterns their own radius. Hovering an event point
that has a radius shows that radius, under fog, until the cursor leaves.

---

## 11. Delete (brush −1; code, 0x597588)

Delete acts on the hovered cell, according to the page. It is disabled on the terrain page.

| Page | What is deleted |
|---|---|
| Hills | Every hill whose footprint covers the cell, searched up to 8 cells right and down. The marks are then rebuilt. |
| Forests | The forest object of that cell; the mark returns to the terrain. Dragging keeps deleting. |
| Buildings | Every building whose footprint covers the cell (overlaps are allowed, so there can be several), searched up to 8 cells right and down. For each, later buildings move down one index. Event conditions that name buildings are renumbered: the deleted one becomes 0, later ones drop by one. The footprint marks return to the terrain. **Army home buildings are not renumbered.** |
| Items: event point or lantern | Later points move down. The point references in events are renumbered the same way. |
| Items: hero start | The preset's start is set to 0,0. |
| Items: army | Buildings it owned become unowned; owners above it drop by one. Later armies move down. Every army reference in events is renumbered. |

The buildings and armies submenus are rebuilt after a building or army is deleted. A target place
(kind 10) is mistakenly handled as an army. Deleting a lit lantern or a hero start does not undo
its fog reveal.

---

## 12. Pick up, move and drop (code, 0x5ad0c4, 0x5b1df0)

**Pick up:**
- A right press in Info mode picks up the object under the cursor.
- In move mode (§3.1) a left press does the same.
- The editor tries, in order: an army, then a building covering the cell, then a point.
- Hero starts cannot be picked up; they are moved by placing them again.

**While held:**
- The object leaves the map at once.
- The brush becomes the building's footprint width (or 1), and the preview shows the held object.

**Drop:** the next left press:
1. writes the brush centre as the new position;
2. writes the figure or anchor back;
3. rebuilds the marks and the minimap for a building, or moves a lantern's reveal area;
4. marks the map modified and returns to Info. Move mode stays on, so in move mode the next left
   press picks up again. The size buttons are not redrawn.

A drop checks nothing: any terrain, other buildings, other figures and the map edge for wide
footprints are all allowed.

---

## 13. Info mode (brush 0; code, 0x5ad0c4)

### 13.1 Hover hint

Outside move mode, hovering builds a hint on the map area:

| Object | Hint shows |
|---|---|
| Building | Index, name, type and picture codes; the owning army (index, name, leader name) or the neutral owner's name; income and mana; a marker if it has a description |
| Army | Index, name and its two tactical-cost numbers |
| Point | The number of attached events and up to five event names; the lantern radius if any |
| Hero start | A fixed label |

Hovering an army also selects it for the patrol overlay (§8.1).

### 13.2 Left press

| On | Opens |
|---|---|
| Empty cell | The building editor, if a building covers the cell |
| Hero start | The scenario options window, on the tab of that hero class (start kind k opens tab k). The menu item always opens it on the first hero's tab (0x5ae9cc, 0x5a4558). |
| Army | The army editor |
| Lantern | The radius number dialog. The old area is un-revealed and the new one revealed. |
| Event point | The point editor |
| Target place | The target editor |

---

## 14. The two modes (code, 0x5b2590, 0x5b25e8, 0x5b2654)

### 14.1 Tree replacement mode

A menu check item and a check box, kept in step. It is read only by the forest brush (§10.3).

### 14.2 Ignore-mountains mode and the building-place check

- One flag, saved as `FindBuildingPlace`, enables the check.
- When the check is on and a building is being placed or held, a mountain mark (object classes
  5–7; rocks, class 8, are not tested) shows the forbidden cursor. The tested area is the footprint
  plus one extra column on the left and one extra row on top: from the centre minus the footprint
  width to the centre, and the same for height (0x5ad3b4–0x5ad4d2). **The press is still
  accepted**: the check only warns.
- At start-up the check box shows the opposite of the flag. When the setting is true, "ignore
  mountains" starts unchecked and the check is on.
- Every later toggle of the menu item or check box copies the check box state straight into the
  flag. After the first toggle, therefore, "ignore mountains" checked means the check is **on**.
  The meaning flips relative to start-up.
- The flipped flag is saved on exit, so the next start agrees with it again.

---

## 15. Burn everything (code, 0x5b29d4)

No confirmation is asked. The map is not marked modified, and nothing is redrawn until the next
normal redraw.

**Buildings:**
- Every building gets a ruin picture:
  - town variant 3 and every village each have a fixed ruin;
  - castle variants 0–5 map to six ruins;
  - fort variants 1–5 take one of two ruins at random;
  - taverns and house 0 share one ruin, and house 1 has another;
  - church variants 0–1 take one ruin and variant 4 another;
  - markets and the remaining variants keep their picture.
- The anchor cell is updated, but the building's **type does not change**.
- In each building name, nine nouns are replaced by their ruined forms (the pairs are in the exe
  at 0x5b358c–0x5b3700).
- The description is cleared.

**Terrain:** every cell with terrain above 3 (anything but water and lava) becomes scorched land
(13).

**Hills:** green, steppe and sand hills become scorched hills, and green mountains become
scorched mountains. Each keeps its sprite number.

**Trees:**
- Live trees become random dead trees.
- Thickets are re-rolled from their late sprites.
- Either may then take the "+120" alternate, with a one-in-five chance.

---

## 16. Hidden tools

### 16.1 Brush-size title click (code, 0x5b1a60)

Clicking the brush-size title re-rolls three placeholder forest sprites over the whole map, each
to one of five neighbours, using Rand(5). This affects a common tree, its "+120" alternate and a
common thicket. The map is neither redrawn nor marked modified.

### 16.2 Invisible ninth toolbar button (code, 0x5b1e18)

When clicked, it adds 1 to every event index in every building's and every point's event list,
then hides itself. It is invisible in the form resource, so it cannot be reached. It was a one-off
renumbering fix.

### 16.3 Target places (code, 0x55026c callers)

The target-place kind has an editor and opens from a map click, but the item palette has only nine
cells, so it cannot be placed.

---

## 17. Quick save, emergency save, hand-off

### 17.1 F2 quick save (code, 0x5b3714)

F2 saves straight to the open dialog's folder, a backslash and the current file name. There is no
dialog and no prompt, even for a map still called `New.DTm`. Like any save it clears the modified
flag, but the window caption is not refreshed.

### 17.2 Emergency save (code, 0x5ab588, 0x5a5fa4)

See §8.1.

### 17.3 Clipboard hand-off (code, 0x5b285c)

Each time the editor becomes the active application, it repaints and reads the clipboard text. If
the text contains `.DTM` and names an existing file:
1. The editor clears the clipboard.
2. It asks to save if the map is modified; cancel aborts.
3. It loads that file and makes its folder the map folder.

A second launch with a map path uses this route (§1.1). A path the user copies by hand is opened
too.

---

## 18. Buildings and armies submenus (code, 0x5aee20, 0x5af524, 0x5af960, 0x5afadc)

**Buildings submenu:**
- Rebuilt after load, generate, painting, size changes, deletion and the building editor.
- One item per building of types 1–12, captioned with the building's name and filed under one of
  twelve type groups. Bridges and obelisks are not listed.
- A group shows only if it has items. The two separators show only when groups exist on both
  sides of them.
- The whole submenu is disabled when the map has no buildings.

**Armies submenu:**
- An army whose leader unit is undead goes to the undead group.
- Otherwise it is filed by behaviour style: feudal, robbers or peasants. Other styles are not
  listed.

Items keep record order. Choosing one:
1. centres the view on the object, clamped to the map (a building is centred on its footprint);
2. opens its editor.

---

## 19. Shared lists in the record editors (code, 0x5afc24–0x5b23e4)

The record editors attach drawing and filling routines that live in the main window.

### 19.1 Unit lists (0x5b22a8, 0x5b23e4, 0x5b0680)

Units are sorted ascending by a key built from four parts:
1. **Group:**
   - heroes, and the three hero classes: first;
   - then normal, rogue and undead units, in that order;
   - every other nature last.
2. **Role:** units with no role first, then melee, then shooters, then casters.
3. **Casters:** by magic school (life, elemental, death).
4. **Cost.**

Each entry shows the portrait, the cost and the name. Lists meant for army troops skip the three
hero classes.

### 19.2 Artefact lists (0x5b2038, 0x5b03a0)

Artefacts are sorted by item type, then by absolute cost, dearest first. Each entry shows the
icon, the cost and the name.

### 19.3 Spell lists (0x5b0ae4)

Spells keep file order. Each entry shows the icon, the gold cost and the name.

### 19.4 Army lists (0x5b0d78)

Each army is summarised in one entry:
- whether it has a leader, and the leader's cost;
- the troop count and their total cost;
- the number of artefacts carried.

There are special texts for:
- a ship (no leader and no troops);
- an empty army;
- an impossible troop count above 12.

### 19.5 Event lists (0x5affe8, 0x5afc24)

- Events are listed in index order, filtered by type and group.
- Each event's text colour comes from its group, with a lighter colour when selected, on a black
  or white background.
- Quests are bold and rumours italic. Global events use a different typeface.

### 19.6 Group-colour lists (0x5b0174)

Group-colour lists draw a swatch for each colour.

---

## 20. Razdor editor now → original

Razdor files: `src/editor/*` (model) and `src/ui/editor/*` (screens). "Matches" means the same
behaviour, not the same look.

| Behaviour | Original | Razdor | Status | Razdor file |
|---|---|---|---|---|
| Entry | Separate exe; `NewMap`, `Battle`, `.DTM` arguments | `--editor` flag or the menu link; no map argument | differs | `src/main.rs`, `src/ui/mod.rs` |
| Single instance | Window-title check plus clipboard hand-off of a map path | A lock file, and the second copy exits; no hand-off | differs | `src/main.rs` |
| Start-up checks | Colour depth, install files, version, DDraw size, screen size, word list | None | differs (by design) | — |
| Settings file | `DTmapEdit.Ini` `[Option]` `WorkMap`, `PathMap`, `FindBuildingPlace` | `DTMapEdit.Ini` in Razdor's editor folder: `WorkMap` and `FindBuildingPlace` written on close, `PathMap` never | matches | `src/editor/options.rs` |
| Reopen last map | `WorkMap` loads at start; a new map on failure | The same | matches | `src/ui/editor/mod.rs` |
| Default map folder | Exe folder `Maps_Rus\` (or `Maps\`) | User data folder; install maps read-only | differs (by design) | `src/editor/files.rs` |
| Close with unsaved changes | Yes/no/cancel prompt | Prompt on Exit only; closing the window skips it | differs | `src/ui/editor/mod.rs`, `src/main.rs` |
| Menu bar | File / Edit / Scenario / Help | None; a toolbar only | differs | `src/ui/editor/mod.rs` |
| New map | Keeps the current size, terrain 0, no dialog | The same (seed from the generator, our own title); Razdor's sized dialog is "New of size" | matches | `src/editor/defaults.rs`, `src/ui/editor/mod.rs` |
| Generate / world filler | Yes | No | missing | — |
| Save without dialog | Ctrl+S to the map folder; F2 quick save | Ctrl+S; F2 saves under the map's name without a dialog (Razdor's folder rules and confirmations stay) | matches | `src/ui/editor/mod.rs` |
| Save formats in dialog | Normal, text dump, uncompressed, demo | The same four, by the same extension rules | matches | `src/editor/files.rs`, `src/editor/mapfile.rs` |
| Options, units, artefacts, named-characters windows | Yes | Named characters is a settings tab; the others are missing | differs / missing | `src/ui/editor/settings.rs` |
| Test AI, test battle, help, about | Yes | Test play only | missing | `src/ui/editor/mod.rs` |
| Undo / redo | No | 200 steps | differs (extra) | `src/editor/doc.rs` |
| Tool model | 5 pages × brush (−1/0/1–6) + move mode | The same, plus Razdor's flood fill and rectangle on the terrain page | matches | `src/editor/tools.rs` |
| Brush sizes | 1–6, square ending at centre (even sizes lean down-right) | The same | matches | `src/editor/geometry.rs` |
| Fill and rectangle terrain | No | Yes | differs (extra) | `src/editor/tools.rs` |
| Drag behaviour | Drag paints only terrain and forests | The same, once per cell entered | matches | `src/editor/tools.rs` |
| Modifier keys | A press with Shift, Ctrl or Alt does nothing | The same | matches | `src/editor/tools.rs` |
| Keyboard | Arrows (one cell), Space, F2, Ctrl+N/G/O/S/Q | Arrows (one cell, repeating), Space, F2, Ctrl+N/O/S/Q; plus Ctrl+Z/Y, Delete, Esc, zoom keys | matches (Ctrl+G: no generator yet) | `src/ui/editor/mod.rs` |
| Camera | Whole-cell scroll, scrollbars, rulers, no zoom | Smooth pan, zoom 0.12–2.5, no scrollbars, no rulers | differs | `src/ui/editor/canvas.rs` |
| Cell size | 32 × 22 | 32 × 22 at zoom 1 | matches | `src/ui/editor/canvas.rs` |
| Draw order | Low hills; then per cell: forest, mountains, buildings (diagonal sort), figures | Objects, buildings and armies sorted by y; bridges first | differs | `src/ui/editor/canvas.rs` |
| Tree position jitter | A per-cell hash offset | Not checked | unknown | `src/ui/editor/canvas.rs` |
| Fog overlay | Yes, with reveal radii for starts and lanterns | The same reveal counters (load, placements, drops, the lantern dialog; deletes leave them) | matches | `src/editor/cells.rs` |
| Passability grid | 7 coloured tiles from terrain and marks | The same 7 kinds; marks rebuilt when switched on, written by the brushes | matches | `src/editor/cells.rs`, `src/ui/editor/canvas.rs` |
| Patrol overlay | Hovered army only; everything shaded when its radius is 0 | The same (the selected army's radius is outlined too) | matches | `src/ui/editor/canvas.rs` |
| Brush preview | Ghost for 10 ticks; forest scatter preview | Brush, footprint and ghost always shown | differs | `src/ui/editor/canvas.rs` |
| Minimap | 257 × 257, two colour modes, hover frame, drag | Aspect-kept, terrain overview plus dots, click or drag | differs | `src/ui/editor/canvas.rs` |
| Terrain brush side effects | Water and lava clear forests; deep sea clears hills | The same, marks as the original writes them | matches | `src/editor/brush.rs` |
| Hill placement | One object at the centre; size = footprint group | The same; whole square inside the map | matches | `src/editor/brush.rs` |
| Forest placement | Random family member per cell, one-in-five "+120" alternate | The same draws, row by row | matches | `src/editor/brush.rs` |
| Tree replacement mode | Yes | Yes, with the family rules | matches | `src/editor/brush.rs` |
| Building fit | Anchor cell free and square inside the map; nothing else | The same | matches | `src/editor/brush.rs` |
| Building limit | 254 | 254 | matches | `src/editor/brush.rs` |
| Building defaults | Faction 3 with its attitudes, defence by type, owner 0, seeded random names | The same, names from the install's `DTMapEdit_Rus.Ini` | matches | `src/editor/brush.rs`, `src/editor/naming.rs` |
| House picture → type | Pictures 2–4 obelisk, 5–6 ruins (also at save) | The same | matches | `src/editor/brush.rs` |
| Forest cleared under a building | Yes | Yes | matches | `src/editor/brush.rs` |
| Army defaults | Zeroed; style = kind − 4; inactive kind; experience correction 100; numbered name; editor opens | The same; the panel opens | matches | `src/editor/defaults.rs` |
| Army kinds in palette | Feudal, robbers, peasants, inactive | The same | matches | `src/ui/editor/palette_panel.rs` |
| Lantern | Radius 10 then a radius prompt; lit | The same; the dialog's radius is revealed | matches | `src/editor/brush.rs` |
| Event point | Zeroed; editor opens | The same | matches | `src/editor/defaults.rs` |
| Point limit | 256 | 256 | matches | `src/editor/brush.rs` |
| Occupied-cell check | Lanterns and points refused on figures (but tested at the unscrolled view cell); armies and starts overwrite | The same, bug included | matches | `src/editor/tools.rs` |
| Hero start | Placed from the item palette; reveals radius 5 | The same (the settings' button picks it on the palette) | matches | `src/editor/brush.rs` |
| Back to Info after an item | Yes | Yes (not when the list is full, as in the original) | matches | `src/editor/tools.rs` |
| Delete | Per page at one cell; event refs renumbered; home buildings not renumbered | The same delete brush (target place deleted as an army); the Delete key and panels also delete a record | matches | `src/editor/brush.rs`, `src/editor/refs.rs` |
| Pick-up order | Army, building, point; right press, or left in move mode | The same | matches | `src/editor/brush.rs`, `src/editor/tools.rs` |
| Drop checks | None | None | matches | `src/editor/brush.rs` |
| Info hover hint | Building, army and point details | The same details, in Razdor's words | matches | `src/editor/tools.rs` |
| Info click opens editors | Yes, plus a lantern radius dialog | The same (panels, the dialog, the scenario window on the hero's tab) | matches | `src/editor/tools.rs`, `src/ui/editor/mod.rs` |
| Ignore-mountains check | Warning cursor only, with the inverted toggle | The same (red preview) | matches | `src/editor/tools.rs` |
| Burn everything | Yes | Yes, without the nine renamed nouns (the original's own text) | matches (names) | `src/editor/brush.rs` |
| Hidden tools | Tree re-roll, renumber button | No | missing (by design) | — |
| Buildings / armies submenus | Grouped by type or style, centre and open | The same, as two toolbar lists | matches | `src/editor/menus.rs` |
| Unit list order | Sorted by group, role, school, cost; portraits | The same order; text only | matches (order) | `src/editor/menus.rs`, `src/ui/editor/form.rs` |
| Artefact list order | By type, then dearest first; icons | The same order; text only | matches (order) | `src/editor/menus.rs` |
| Army-troop unit list skips heroes | Yes | Yes (troop, garrison, barracks and preset slots) | matches | `src/ui/editor/form.rs` |
| Event list look | Group colour text, bold quests, italic rumours, filters | Group swatch, kind initial, filters, search | differs | `src/ui/editor/events.rs` |
| Caption | Main caption, version, file, format number | Fixed app title; name shown in the toolbar with `*` | differs | `src/ui/editor/mod.rs` |
| Save enabled only when modified | Yes | Yes | matches | `src/ui/editor/mod.rs` |
| Playability label | Yes | Quest count and score at the foot of the tool column, in the original's colour bands | matches | `src/ui/editor/mod.rs` |

---

## 21. Unknowns

- The exact meaning of the three Application hint fields set at start-up (+0x74 = 1000,
  +0x78 = 0, +0x80 = 0).
- The leader-id sets that pick the two special army figures (0x5ab0f8), and the unit field that
  makes a leader "undead-looking".
- How the hills-page palette rows are counted on the first switch to that page: the count is taken
  before the brush becomes 3, and may not be refreshed until a size button is pressed.
- Whether the forbidden cursor over mountains was meant to block placement. It does not.
- Why a held building's forbidden test looks for a held-kind value that never occurs (0x5ad1e0
  region). The palette building's footprint is used instead.
- The exact ruin-name pairs and the per-variant name-pool slices are game text and are not
  restated; see the exe at 0x5b358c–0x5b3700 and the brush routine 0x5959a0–0x5961f0.
- Whether any shipped map uses target places (kind 10), which the palette cannot create.
