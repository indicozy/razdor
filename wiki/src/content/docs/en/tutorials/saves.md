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

There are up to 30 autosave slots (12 in the original). When the slots are full, the oldest one is overwritten. An autosave on the same map replaces the previous one with the same name.

:::caution
If the install has the autosave option turned off (`OptValue8`), no autosaves are made.
:::

## Continuing after the game was closed

While playing, Razdor creates an additional autosave called "Last session". It updates approximately every 30 seconds, as well as when the game window loses focus (on a computer) or before the start of each battle.

If the game was closed without a proper exit (crash, power loss, swiping the app away from recent apps, or the phone unloading it from memory), a "Continue the game?" window with the map name and date will appear over the main menu upon the next launch. Clicking "Yes" will return you to that moment, while "No" will simply close the window. If you exited the game properly (via "Exit" or by returning to the main menu), this window will not appear.

:::note
"Last session" is located in the autosaves tab within the loading window. It can be loaded manually at any time. This save does not occupy one of the 30 slots and is available even if autosaves are disabled.
:::

If the game closed during a battle, "Last session" stores the moment before the battle: after loading, the hero will be standing on the map next to the enemy, and the battle will restart upon your attack. Open windows (town or hero) are not restored — the game will continue on the map. When the game goes to the background (on a phone) or the computer shuts down, "Last session" is written at once; if the hero was walking at that moment, the game continues from where he last stopped.

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
