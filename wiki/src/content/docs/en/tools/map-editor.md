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
1, 3, 5 or 9 cells). A tool places its object where you click; a selected object can be dragged.

The check boxes are **Grid**, **Footprints** (the cells the buildings take) and **Reach** (the cells
the hero cannot walk to on the map's story). Placement errors, such as an object on water or
buildings on top of each other, show on the map at once; the details are in **Problems**.

## Editing, undo and saving

* Changes stay in memory until you press **Ctrl+S** (save the open file) or **Save all**.
* **Ctrl+Z** undoes and **Ctrl+Y** or **Ctrl+Shift+Z** redoes. There is one undo for the whole
  package: changes on the map, in the script and changes to files from the disk.
* **Delete** removes the selected object from its file.
* When a file is changed in another program, a clean tab reloads by itself. A tab with unsaved
  changes asks: reload, keep yours, or show the difference.
* When the text of a file is broken, the map shows the last correct state and the line of the
  error. The other files stay editable.

## Not yet

The studio does not have a story editor (triggers and dialogues), a campaign graph, a table of
texts or route checks yet. These come in later versions.

## .DTm maps

The studio works with packages, not with the old `.DTm` maps. `.DTm` maps are still edited in
Razdor 1.x and in the original DTMapEdit.
