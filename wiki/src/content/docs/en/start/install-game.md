---
title: Game installation
description: How to connect your own Discord Times copy to Razdor.
sidebar:
  order: 1
---

## Your copy of Discord Times

Razdor is an engine for the scenarios of the original game *Discord Times* (Community Update). It reads data from **your own installed copy** while it runs. The Razdor repository contains no maps, data, texts or graphics, and nothing from your installation is copied or changed.

You need version 1.8 with Community Update 1.2 (game 1.8.1). If the game is missing, the main menu explains that you need your own copy and links to the [installation guide](https://indicozy.github.io/razdor/install.html) and to the Discord where it is obtained.

## How to connect the game

Razdor looks for the installed game in this order:

1. **`RAZDOR_DT_DIR`**: the path to the folder that contains `DiscordTimes.exe`. The variable can be set in the environment or in a `.env` file in the current folder or next to the program.
   ```sh
   export RAZDOR_DT_DIR="/path/to/Discord Times"
   ```
2. **Next to the game**: if you copy `Razdor.exe` (Windows), `razdor` (Linux) or `razdor-macos` (macOS) directly into the folder with `DiscordTimes.exe`, Razdor uses that copy. On a Mac the game is inside the Wine or CrossOver bottle.
3. **Previous run**: the folder Razdor remembered.
4. **Standard folders**: `~/Games`, `~/Downloads` and your home folder.

Razdor only reads the game files: `Rus_Units.ini`, `Rus_Artefacts.ini`, `Rus_Spells.ini`, `_Global.ini` and the maps `Maps_Rus/*.DTm`.

:::tip
The **Evolution** mod is supported (tested with version 9.0 on Community Update). Point `RAZDOR_DT_DIR` at the copy of the game that has the mod installed.
:::

## What to do if the game is not found

If the game is missing or Razdor does not accept it, the main menu explains that you need your own copy (1.8 with Community Update 1.2).

* Check that version 1.8 with Community Update 1.2 is installed.
* Check the path in `RAZDOR_DT_DIR`.
* Look at the `razdor.log` file: it records which paths were searched and why the folder was not accepted as a copy of the game. It is in `%APPDATA%\razdor\` (Windows), `~/.local/share/razdor/` (Linux) or `~/Library/Application Support/razdor/` (macOS). The previous log is called `razdor.previous.log`.

:::caution
Mods that change the `DiscordTimes.exe` file itself are not supported.
:::

## Font setup

Russian text needs a TrueType font with Cyrillic. Razdor finds a common system font by itself. If you see transliteration instead of names, point it to your own font:

```sh
export RAZDOR_FONT="/path/to/font.ttf"
```
