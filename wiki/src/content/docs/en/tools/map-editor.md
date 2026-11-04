---
title: Razdor Studio
description: The Razdor package editor: opening or making a package, the map, the inspector, the script, checks, undo and saving.
sidebar:
  order: 1
---

Razdor Studio edits `.rzpack` packages: folders with a map, texts and a script. The studio is in
the Windows, Linux and macOS builds; there is none on phones. Everything you change is written to
the files of the package folder.

## Opening a package

In the main menu, press **Editor**. The studio opens the package you worked on last. If there is
none, a start page appears:

* **Open package folder…** — pick a folder that has a `package.toml` in it.
* **New package** — type a folder name, choose where to make it, and press **Create package**. The
  package gets one 50 by 50 map: a meadow, the hero in its middle, and a victory when all the enemy
  armies are beaten.

**Back to the menu** returns to the main menu.

## Workspaces

The studio has four workspaces. Switch between them with the buttons in the top bar or with
**Alt+1** … **Alt+4**:

* **Map** — the map, the stamps on the left, the minimap and Scene on the right.
* **Story** — the outline on the left (the tabs Story, Characters and Variables), the story or campaign graph in the centre.
* **Data** — on the left a choice of Units, Items, Texts, Package and Files; the chosen view in the centre. Files shows the file tree on the left and the code editor in the centre.
* **Test** — three views on the whole area, switched by the tabs Battle, AI and Scores: the battle test,
  the AI viewer on the map, and the package's two scores.

Each workspace has the Inspector on the right. The studio remembers each workspace's window layout;
**Reset layout** brings back the original one.

The top bar holds Save, Undo and Redo, **Check** (F7), **Play** (F5), the Package menu, the command field
«Commands and objects» (the palette), **Settings** (Ctrl+,) and Exit. The bottom has the status bar (the cell under the pointer, the tool's hint,
whether the package is saved) and the bottom panel with the tabs Problems, Output and Claude. The panel is
closed; it opens with Ctrl+J, and by itself when a new error appears.

## Keyboard shortcuts

Change them in **Settings** → **Keyboard shortcuts**: click a shortcut, then press the new key (Esc
cancels). A key that another command has is refused, and the page says which command has it. **Reset**
puts one command back, **Reset all** puts back all of them. The shortcuts are kept in the program's
settings, not in the package.

The default list:

| Action | Shortcut |
|---|---|
| Save / save all | Ctrl+S / Ctrl+Shift+S |
| Undo / redo | Ctrl+Z / Ctrl+Shift+Z (also Ctrl+Y) |
| Command palette / go to an object or file | Ctrl+Shift+P / Ctrl+P |
| Workspaces Map, Story, Data, Test | Alt+1 … Alt+4 |
| Check | F7 |
| Play | F5 |
| Bottom panel | Ctrl+J |
| Hide or show the side panels | Tab |
| Find in the current view | Ctrl+F |
| Open the selection in its file | Ctrl+Shift+O |
| Cancel, close a window, drop a floating paste | Esc |
| Settings | Ctrl+, |

On the Map, tools: V select and move; B brush; E eraser; G fill; I eyedropper; M marquee; L lasso;
W magic wand; S stamp; A army; U building; P point; H hand (R trigger zone, not available yet).

On the Map, the rest: [ and ] brush size; Shift+[ and Shift+] density by 10 %; 1–6 and 9 brush sizes;
Shift+click a straight line from the last point; Alt (hold) eyedropper while painting; X swaps the last two
picks; Space (hold), middle button or H pans; Ctrl+=, Ctrl+-, Ctrl+0, Ctrl+1 and Ctrl+wheel zoom; Shift and
Alt while selecting add and subtract (Tiled and Godot use Ctrl); Ctrl+A, Ctrl+D, Ctrl+Shift+I select all,
deselect, invert; Ctrl+C, Ctrl+X, Ctrl+V, Ctrl+Shift+V copy, cut, paste, paste in place; Enter commits a
paste; Delete and Backspace delete; Alt+drag (tool V) copies an object; Ctrl+Alt+S saves the selection as a
stamp; Alt+click a layer's eye shows only that layer.

In the Story view: F frames the selected node, Home the whole graph; Ctrl+F finds a node; Delete removes a
node; Ctrl+D copies a node. In Data: arrows move between cells; Enter or F2 edit; Esc cancels; Ctrl+C and
Ctrl+V copy and paste cells; Ctrl+F filters. In the Inspector, Ctrl+F searches the fields while the
Inspector has the focus. One key does different things in different views (Ctrl+F, Ctrl+D, Ctrl+C and
Ctrl+V): the one for the view you are in acts.

The interface scale (80 % to 150 %) is in the same Settings window. No text is drawn smaller than 12 pixels.

## Command palette and go to

**Ctrl+Shift+P** opens the command palette: every command of the studio, with its icon, name and shortcut.
Type part of a name (in Russian or English, or the command's id): the list narrows, the arrows pick a row,
Enter runs it, Esc closes. A click runs a row too.

**Ctrl+P** opens go to: the package's files, maps, armies, buildings, triggers, dialogs, units, items,
characters and variables. The choice opens in its own workspace: a map, army or building on the Map; a
trigger or dialog in the Story; a variable or a character in the Story's outline tabs; a unit or an item in
Data; a file in Data › Files, at its line.

The «Commands and objects» field in the top bar opens the palette with both commands and objects. While the
palette is open the studio's shortcuts wait: the keys go to its search field.

## Play (F5)

**Play** saves the open files and starts the game on the package's shown map in its own window: the game opens
the package as it does for a normal game with a package (the hero is picked first). The studio stays open, and
the status bar says «The game is running» while the game's window is open.

The game does not start, and the studio says why (in the Output, or in Problems for errors in the package):

* the game's folder is not set (`RAZDOR_DT_DIR`);
* the package has errors (run **Check**, F7, first: its errors are in Problems);
* no map is open.

## Windows

* **File system** — the files of the package. A click opens a file in the Script.
* **Scene** — the objects of the open map.
* **Map** — the map itself, drawn as the game draws it.
* **Script** — the text of the open file. The selection is linked to the map: the selected object
  is highlighted in the text, and the other way round.
* **Inspector** — the fields of the selected object or file. At the top, the name and the type, and
  under them a field search: it keeps the rows whose label or file key holds the query (Ctrl+F puts the
  cursor in the search while the pointer is over the Inspector). The fields are grouped in sections,
  which open and close and remember their state for each kind of object. A field's file key shows in
  its tooltip; in Settings → "File keys in labels" it shows under the label. A changed field of a unit
  or an item has a dot, and the reset icon on its right returns the base value. An army's troops, a dialog's choices
  and a list of units drag by the grip on the left to reorder; the buttons under a list add, duplicate
  and remove a row.
* **Problems** — the result of the checks. A click goes to the place of the error.
* **Output** — the log of the actions.

Windows can be dragged, split and opened as tabs inside their workspace.

## The map

The tools are **Select**, **Building**, **Army**, **Point**, **Object** and **Terrain** (a brush of
1 to 6 or 9 cells), with the pages described below. A tool places its object where you click; a
selected object can be dragged.

The check boxes are **Grid**, **Footprints** (the cells the buildings take) and **Reach** (the cells
the hero cannot walk to on the map's story). Placement errors, such as an object on water or
buildings on top of each other, show on the map at once; the details are in **Problems**.

### Map tools

A strip of tools runs down the left edge: one icon each; the tooltip shows the name and the key.

* **V** select and move; **B** brush (paints what the palette has picked); **E** eraser;
  **G** fill; **I** eyedropper (or Alt while painting with the brush); **S** stamp; **A** army;
  **U** building; **P** point; **H** hand (drag to move the view).
* **M** marquee, **L** lasso and **W** magic wand (see Selections below); **R** (trigger zone) is
  not available yet.

Above the map is the options bar of the active tool. For the brush: size (a slider, keys `[` and
`]`), the presets 1 to 6 and 9, round or square, and density (`Shift+[` and `Shift+]`, in steps
of 10 %) for forests and objects. Buildings, armies and objects pick their kind in the same row.

**Palette** (the Palette tab): the terrain kinds with their names, and the objects: hills,
forests, mountains. With the game installed, the objects are drawn with its sprites; without it,
with an icon and a name. The search finds by name. **X** swaps the last two picks.

**Layers** (the Layers tab): Terrain, Objects, Buildings, Armies and Points. The eye shows or
hides a layer, the lock locks it. Alt+click on an eye leaves only that layer shown. A hidden or
locked layer is not picked and not changed. The overlays (passability, grid, patrols, fog,
building footprints, reach) sit below the layers and only show data.

**Brush:** a stroke paints every cell along its path, so a fast drag leaves no gaps. Shift+click
draws a straight line from the last point. One stroke is one undo step. The cells the brush will
cover are shown under the pointer. An army, a building or a point shows a ghost until you click;
Esc or a right click cancels the placement.

**View:** Space with a drag, the middle button or the hand move the map; Ctrl+wheel zooms;
Ctrl+= and Ctrl+− zoom in and out, Ctrl+0 fits the map, Ctrl+1 gives 100 %. Tab hides the side
panels so the map takes the full width.

**Status bar:** the cell under the pointer, its terrain, the object on it and the tool's hint.

The **Move** mode: the first click takes an object (an army, a building, a point, an object), the
second puts it down. **Burn everything** (after a question) turns live trees dead and burns the
ground above water; it is one step and is undone in one step.

The **Minimap** shows the whole map with a frame for the view: a click or a drag moves the view.

### Selections

A selection is a set of map cells. It does not change files and it is not an undo step.

* **M** marquee: drag from one corner to the other. A click without a drag clears the selection.
* **L** lasso: draw round cells; the cells whose centre is inside are selected.
* **W** magic wand: a click selects the cells of the clicked terrain. The Contiguous box takes only the
  connected cells; without it every cell of that terrain on the map is selected.

**Shift** adds to the selection and **Alt** subtracts from it (the Photoshop convention). The buttons on
the options bar set the default mode: Replace, Add or Subtract. Commands: **Ctrl+A** select all,
**Ctrl+D** deselect, **Ctrl+Shift+I** invert. Dashed «marching ants» run round the selection.

A selection acts only on visible, unlocked layers: a hidden or locked layer (its eye and lock in Layers)
is not copied, cut or cleared. Terrain and objects whose cell (for a building, its bottom right corner)
is in the selection are copied. The hero is never copied or removed.

* **Ctrl+C** copies the selection; **Ctrl+X** cuts it (the copy and the clearing are one undo step);
  **Delete** or **Backspace** clears its contents. A cleared cell becomes plain grass, and objects are
  removed.
* **Ctrl+V** attaches the copy to the pointer: it is semi-transparent and follows the cursor. A click or
  **Enter** pastes it, **Esc** drops it. **Ctrl+Shift+V** pastes it where it was copied from. A paste is
  one undo step. Pasted objects get new numbers; an army with a name loses it, because a named
  character is one per package.
* **Only in selection** on the options bar: the brush, the eraser, the fill, the forest and hill scatter
  and the stamp act only on the selected cells. The box is on while a selection exists.
* **Ctrl+Alt+S** (or the button on the options bar) saves the selection as a stamp: you type a name and
  the stamp appears in the Stamps tab. Stamps are kept in the package's `.editor/stamps` folder; the game
  does not read them.

## Editing, undo and saving

* Changes stay in memory until you press **Ctrl+S** (save the open file) or **Save all**.
* **Ctrl+Z** undoes and **Ctrl+Y** or **Ctrl+Shift+Z** redoes. There is one undo for the whole
  package: changes on the map, in the script and changes to files from the disk.
* **Delete** removes the selected object from its file.
* When a file is changed in another program, a clean tab reloads by itself. A tab with unsaved
  changes asks: reload, keep yours, or show the difference.
* When the text of a file is broken, the map shows the last correct state and the line of the
  error. The other files stay editable.

## Scenario: package, map, campaign, characters, texts

- **Package settings** («Package» → «Package settings», or `package.toml` in the file tree): version, title, description, author, licence, base. The id is read only.
- **Map settings** («Package» → «Map settings», or the map's name in the scene): title, description, start date, the relations between the sides (−3 to 3), terrain colours and the victory (its kind and its argument).
- **Campaign** (the «Campaign» main screen): maps as nodes, next-maps as arrows with numbers (1 is tried first). Drag a node to move it; drag from its right edge to another node to add a next-map. Select an arrow to change its order, delete it or set its conditions. Dashed arrows are set by triggers (travel, victory with a next map, ending); they are read only here and are edited in Script.
- **Outline** (the Story tab on the left of Story): the triggers and dialogs by map; a trigger shows its condition under its name, a dialog its title. Search at the top. A click selects the node, opens it in the Inspector and centres the graph on it. The graph opens at a readable zoom, centred on the selected node (or the first trigger). Variables fold into one «Variables» node; «Each variable» above the graph shows them one by one. Keys in the graph: **F** frames the selected node, **Home** fits the whole graph, **Ctrl+F** finds a node in the outline, **Ctrl+D** duplicates a trigger, **Delete** removes the selected trigger or dialog when nothing names it. The selected node is highlighted, its edges are bright and the others faint.
- **Story** (the «Story» main screen): the map's dialogs, triggers and variables as nodes. A dialog's dots are its choices; drag from a choice's dot (or a trigger's) onto a dialog to wire it: the choice opens the dialog, or, when a `dialog_choice` trigger already answers the choice, a new trigger is made. Edges read «opens», «sets X», «if X». Select a node to edit it in the Inspector: a trigger (when, if, then, with each kind and its arguments from a list), a dialog (its title and text in each language, the speaker, its choices with their conditions and actions). «Variables» (a tab of the outline) shows where each variable is set and read, warns about the unused ones, and renames it across the files in one step.
- **Characters** (the «Characters» tab of the outline): where each character is given, required and removed. A warning when a character is needed before any map gives it. Renaming changes it in every file in one undo step.
- **Texts** (in Data, the «Texts» view): one row per key, one column per language. Red: used but defined in no language; grey: defined but never used; amber: a language lacks it. «Add language» creates `texts/<language>.toml` with every key empty.

Each change is one undo step (Ctrl+Z).

## Units and items

- **Units** and **Items** (in Data): the package's records as spreadsheets. Columns: the id (pinned on the left), the base, the experience to level 2, hits, attack, defence, speed, gold and the suggested price from the game's formula. A click on a header sorts: ascending, descending, then none. The filter is Ctrl+F. A number cell is edited by a double click, Enter or F2; Enter writes it, Esc cancels, and each edit is one undo step. The Apply button in a row writes its suggested price. The arrows move the cursor; Ctrl+C and Ctrl+V copy and paste a number. The picked record opens in the Inspector. **New unit** (or item) makes a record on a base game unit: its file `units/<id>.toml` and its name in every language.
- Pick a record to edit it in the Inspector. Ctrl+Shift+O (or «Open in file» at the top of the Inspector) goes to the object's line in Data › Files. Each field shows its override, with the base value next to it; **Reset** returns the base.
- **Suggested price** comes from the game's formula: the unit's strength, its tactical cost, the gold rounded down in bands; doubled for the three hero classes. The steps open under the price. **Apply** writes the price into the price field in one undo step. The formula needs the game's install; without it there is no price.
- An item gets the price of the gain it gives the units that can wear it. A price of 1 and potions are left alone.
- **Levels 1–5** (for a unit): the stats and the tactical cost in percent of the base. The gains per level come from the base.
- **Rename** changes the id in every file (references, the base and the name keys) in one step. **Delete** is refused while the record is named somewhere; the first place is shown.
- The package format does not hold what DTMapEdit has: gains per level, nature, magic school and direction, bonuses, icons, upgrade units, surrender, starting experience and the price multipliers. These come from the base; an item's stat changes come from its base too.

## New map (generator)

Package → «New map (generator)…» makes a map by the rules of DTMapEdit. The map comes from a
seed: the same seed and settings give the same map.

- **Size.** 50, 100 or 200 cells: a package holds maps of these sizes only.
- **Map type and orientation.** Land, lake, river valley, estuary, coast, skerries, island or
  archipelago; which side the sea or the river mouth faces.
- **Ratios.** Seven bars of panel widths, as in DTMapEdit: water, land, the coast band, hills and
  mountains, marsh, forests. Each bar shows its panels; you can type the width of each panel, and the last panel of a bar takes the rest.
- **Seed.** A number from 1 up. «Random» takes a new number.
- **World.** Three steps: buildings and roads, economy, armies and garrisons. The chances (how
  often a town, a castle, a village or a ruin is built, and so on) and the army budgets are in the
  folded sections.
- **Map.** An id (lowercase Latin letters, digits and dashes), a title and, if you want, a place in
  the campaign (at the end of the map list in `campaign.toml`).

«Create» makes the map in the background (a few seconds) and adds it to the package as one undo
step («Edit» → «Undo»): the map's files, its names and its campaign entry go together. The map
opens on the Map tab.

What the package does not hold, and what the studio does instead:

- The generator makes 400 and 800 cells; a package does not.
- Trade, the spell library, village mana, artefacts for sale and building events: a package does
  not store them, so the map has none.
- An army stands on the nearest free cell next to its building (a package keeps armies off
  buildings). There are no named leaders: a leader becomes the army's first unit.
- The victory is reaching a cell the hero can walk to over land.
- Patrols, aggression and respawn times of armies are not set in a package.

The Check button warns when a building or an army cannot be reached on foot from the hero's start:
as in the game's maps, a river is crossed only on a bridge.

## Battle test

Package → «Battle test». Two sides: A is the player's, below;
B is the enemy's, above. Each side is a map army or garrison, the hero's squad, or a custom army (unit
types with their levels, up to 12 units). The map's armies with troops and its garrisons are offered.

- **Battles.** How many battles to fight (100 by default); each battle takes its own seed 1…N. Both
  armies are played by the AI, as in the game's auto battle. «Improved AI» is the battle AI's improved level,
  the game's «Battle AI strength» setting.
- **Results.** The win shares of A and B, draws (the battle did not end before the turn limit), the
  average number of turns, and each side's losses: units and HP, average and range over all battles.
  A table per seed.
- **Watch the battle.** Choose a seed and press «Watch the battle»: the game's own battle screen opens in
  watch mode (both sides by the AI). Leaving it returns to the studio.

The battle uses no chance: damage is exact and the AI makes no random choice. So every seed fights the
same battle, and the studio says so under the results. A spread appears only when a seed changes the
battle itself; this one does not.

## AI viewer

The AI tab of the Test workspace shows the map read-only: you can move and zoom it, not edit it. The top
row has the number of days (30 by default) and **Run**. The map runs with the same calculation as
`razdor-pack sim`: the armies' AI, the hero where he starts, dialogs answered by the first choice.

- **The day slider** shows the armies at their cells on that day. Coloured trails show their last five days;
  the number on an army is its place in the map's order.
- **Story.** Triggers that name a cell (the hero enters a cell, an army arrives at one) are marked with a
  diamond and their id from the day they fired.
- **Log** under the map. Lines «day: what happened». A click moves the slider to that day and opens the object
  of the event: the trigger or dialog in the Story, the army or the cell on the Map.
- The game's files are needed (`RAZDOR_DT_DIR`); without them the panel says so. **Cancel** stops a run.

Moments without a cell (a dialog, a variable) are only in the log.

## Scores

The Scores view of the Test workspace shows two numbers for the open map, side by side, each at the full height with its breakdown.

- **Richness** is DT's playability score: the more content (events, armies, buildings), varied terrain and
  even rewards, the higher. Penalties come off for monotonous terrain, scattered buildings, castles and villages
  without income, empty ruins and quests that nothing completes. The number under the curve is the score.
- **Health** runs from 0 to 100. It is not DT's score but Razdor's own measure: 100 less the findings (a
  warning takes 10, a note 2). The findings are: places the hero cannot walk to; scripted fights the starting
  squad loses; the gold budget against the wages; dead ends (a once-only offer gated on gold, a «later» answer
  whose dialog never comes back, a named character who can be removed or killed); and the traps of the editor's
  guide (a hunting army with no goal, a meeting area that does not cover its army, a neutral army, a town as a
  capture goal).

A click on a line takes you to what it describes: an army or a building opens on the Map (its fields show in the
Inspector), a cell is highlighted in yellow on the Map, a file's line opens in Data › Files. The numbers update after each change; while they are counted the tab shows the
last result. Health only warns: it never stops a save or a play. The game's files are needed
(`RAZDOR_DT_DIR`).

## Not yet

The studio does not have route checks for every story yet. These come in later versions.

## .DTm maps

The studio works with packages, not with the old `.DTm` maps. `.DTm` maps are still edited in
Razdor 1.x and in the original DTMapEdit.
