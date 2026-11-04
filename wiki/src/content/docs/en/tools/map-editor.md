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
* **Story** — the characters and variables on the left, the story or campaign graph in the centre.
* **Data** — the package's files on the left, units, items, texts and the script in the centre.
* **Test** — the battle test and the package's scores.

Each workspace has the Inspector on the right. The studio remembers each workspace's window layout;
**Reset layout** brings back the original one.

The top bar holds Save, Undo and Redo, **Check** (F7), **Play** (F5, not working yet), the Package menu,
**Settings** (Ctrl+,) and Exit. The bottom has the status bar (the cell under the pointer, the tool's hint,
whether the package is saved) and the bottom panel with the tabs Problems, Output and Claude. The panel is
closed; it opens with Ctrl+J, and by itself when a new error appears.

## Keyboard shortcuts

Change them in **Settings** → **Keyboard shortcuts**: click a shortcut, then press the new key (Esc
cancels). A key that another command has is refused, and the page says which command has it. **Reset**
puts one command back, **Reset all** puts back all of them. The shortcuts are kept in the program's
settings, not in the package.

The interface scale (80 % to 150 %) is in the same Settings window. No text is drawn smaller than 12 pixels.

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

The tool bar's pages follow DTMapEdit's: **Terrain** (a brush of 1, 2, 3, 4, 5, 6 or 9 cells),
**Hills** (one hill per brush centre, sized by the brush; **Erase** takes the hills the brush
covers), **Forests** (one tree per cell of a chosen family, or a thicket; **Replace trees** re-rolls
only what the family replaces), **Buildings** (**Erase** removes the building that covers the
cell), **Items** (not yet: the package format has no items on the ground), **Rectangle** and
**Fill** (a rectangle, or the connected cells of one kind, takes the palette's kind).

**Copy** takes a rectangle: **Ctrl+C** copies it, **Ctrl+V** arms a paste that follows the pointer
and **Esc** stops. **Save stamp** keeps the copy in the package under a name; the **Stamps** tab
lists them, and a click arms one for pasting.

**Move** is a mode for every tool: the first click picks an object up (an army, a building, a point
or a terrain object), the next click drops it. **Burn everything** asks first; it turns live trees
into dead ones and scorches the ground above water, as one undo step.

The **Minimap** tab shows the whole map with the view frame; a click or a drag moves the view.

Overlays: **Passable** (the walk grid: hard, impassable, building, water), **Patrols** (the army
routes of the story) and **Fog** (what the hero and the lit lanterns see at the start), besides
**Grid**, **Footprints** and **Reach**.

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
- **Story** (the «Story» main screen): the map's dialogs, triggers and variables as nodes. A dialog's dots are its choices; drag from a choice's dot (or a trigger's) onto a dialog to wire it: the choice opens the dialog, or, when a `dialog_choice` trigger already answers the choice, a new trigger is made. Edges read «opens», «sets X», «if X». Select a node to edit it in the Inspector: a trigger (when, if, then, with each kind and its arguments from a list), a dialog (its title and text in each language, the speaker, its choices with their conditions and actions). «Variables» (bottom dock) shows where each variable is set and read, warns about the unused ones, and renames it across the files in one step.
- **Characters** (the «Characters» tab at the bottom): where each character is given, required and removed. A warning when a character is needed before any map gives it. Renaming changes it in every file in one undo step.
- **Texts** (the «Texts» main screen): one row per key, one column per language. Red: used but defined in no language; grey: defined but never used; amber: a language lacks it. «Add language» creates `texts/<language>.toml` with every key empty.

Each change is one undo step (Ctrl+Z).

## Units and items

- **Units** and **Items** (main screens): the package's records as a list with a filter. **New unit** (or item) makes a record on a base game unit: its file `units/<id>.toml` and its name in every language.
- Pick a record to edit it in the Inspector. Each field shows its override, with the base value next to it; **Reset** returns the base.
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

On the Map tab, tick «AI viewer». A strip appears with the number of days (30 by default) and «Run». The
map runs with the same calculation as `razdor-pack sim`: the armies' AI, the hero where he starts, dialogs
answered by the first choice.

- **The day slider** shows the armies at their cells on that day. Coloured trails show their last five days;
  the number on an army is its place in the map's order.
- **Story.** Triggers that name a cell (the hero enters a cell, an army arrives at one) are marked with a
  diamond and their id from the day they fired.
- **Log.** Lines «day: what happened». A click on a line moves the slider to its day.
- The game's files are needed (`RAZDOR_DT_DIR`); without them the strip says so. «Cancel» stops a run.

Not shown: moments without a cell (a dialog, a variable) are only in the log; buildings and garrisons
are not marked by day.

## Scores

The Scores tab, at the bottom beside Problems, Characters and Variables, shows two numbers for the open map.

- **Richness** is DT's playability score: the more content (events, armies, buildings), varied terrain and
  even rewards, the higher. Penalties come off for monotonous terrain, scattered buildings, castles and villages
  without income, empty ruins and quests that nothing completes. The number under the curve is the score.
- **Health** runs from 0 to 100. It is not DT's score but Razdor's own measure: 100 less the findings (a
  warning takes 10, a note 2). The findings are: places the hero cannot walk to; scripted fights the starting
  squad loses; the gold budget against the wages; dead ends (a once-only offer gated on gold, a «later» answer
  whose dialog never comes back, a named character who can be removed or killed); and the traps of the editor's
  guide (a hunting army with no goal, a meeting area that does not cover its army, a neutral army, a town as a
  capture goal).

A click on a line selects what it describes: an object, army or building, a cell (highlighted in yellow), or a
line of a file in the Script. The numbers update after each change; while they are counted the tab shows the
last result. Health only warns: it never stops a save or a play. The game's files are needed
(`RAZDOR_DT_DIR`).

## Not yet

The studio does not have route checks for every story yet. These come in later versions.

The studio is being rebuilt in steps. Not done yet: the new tool strip and the map palette, layers and
selections (copy, cut and paste of areas), the command palette
(Ctrl+Shift+P) and go to an object (Ctrl+P), the Play button (F5), Tab to hide the side panels, and the AI
viewer inside Test (it is still on the map).

## .DTm maps

The studio works with packages, not with the old `.DTm` maps. `.DTm` maps are still edited in
Razdor 1.x and in the original DTMapEdit.
