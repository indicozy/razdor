---
title: Saving and loading
description: Saves, autosaves and the folder where Razdor keeps them.
sidebar:
  order: 3
---

## How to save and load the game

Progress is saved in several ways:

* **Bottom bar:** the "Save" and "Load" buttons.
* **Title screen:** "Main menu (Esc)" and "Load a game".
* **Keys:**
  * `F5` — quick save: a manual save named "Quick save" that replaces the previous one;
  * `F9` — load the quick save.

The load window has two tabs: your saves and autosaves. New entries are on top. Each entry shows the scenario, the hero and the game date.

:::tip
An autosave is made automatically before each battle and at the daily report at 12:00. Its name contains the date, for example "1204.06.03, 12 h".
:::

## Autosaves

There are up to 12 autosave slots, as in the original. When the slots are full, the oldest one is overwritten. An autosave on the same map replaces the previous one with the same name.

:::caution
If the install has the autosave option turned off (`OptValue8`), no autosaves are made.
:::

## Where saves are kept

Saves are kept in the data folder, not in the game folder:

* **Windows:** `%APPDATA%\razdor\saves`
* **Linux:** `$XDG_DATA_HOME/razdor/saves` (usually `~/.local/share/razdor/saves`)
* **macOS:** `~/Library/Application Support/razdor/saves`
* **Custom folder:** the path in the `RAZDOR_SAVE_DIR` variable.

Each game install has its own subfolder (for example `Discord Times-1a2b3c4d`), so the saves of a mod and of the plain game do not mix.

:::note
A scenario save does not contain the map itself. It stores the map's file name and a hash of its bytes. On loading, the map is read again from `RAZDOR_DT_DIR`. If the map is missing or has changed, the load fails. Demo saves do not need an installed game.
:::
