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

## Windows

* **File system** — the files of the package. A click opens a file in the Script.
* **Scene** — the objects of the open map.
* **Map** — the map itself, drawn as the game draws it.
* **Script** — the text of the open file. The selection is linked to the map: the selected object
  is highlighted in the text, and the other way round.
* **Inspector** — the fields of the selected object.
* **Problems** — the result of the checks. A click goes to the place of the error.
* **Output** — the log of the actions.

Windows can be dragged, split and opened as tabs.

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

## Not yet

The studio does not have route checks for every story yet. These come in later versions.

## .DTm maps

The studio works with packages, not with the old `.DTm` maps. `.DTm` maps are still edited in
Razdor 1.x and in the original DTMapEdit.
