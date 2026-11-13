---
title: Game installation
description: How to connect your own Discord Times copy to Razdor.
sidebar:
  order: 1
---

## Automatic Loading the game

If the game is not present, Razdor will download it automatically upon launch. This works on Windows, Linux, macOS, and Android. Automatic download is not available on iPhone.

When the game is not present, the main screen and the download window display actual game images: they are part of the program itself, so the first launch looks normal.

In the "Loading the game" window, the stages are shown sequentially: first "Checking the catalog...", then a progress bar with the text "Downloading the game: {MB} / {all} MB, {pct}%", followed by "Unpacking… {pct}%". When the game is in place, the window closes and the main menu opens.

"Cancel" stops the download. The already downloaded part is saved, and it can be resumed using the "Download the game" button.

## If Automatic Download Fails

If the download fails (no network, directory unresponsive, or archive fails verification), the reason will appear on the main screen in one line, along with buttons:

* **"Download the game"**;
* **"Pick the game archive…"** (On Android, this is "Choose the archive…", which immediately opens the system file selection window; on iPhone, it is "Unpack the game archive" in the "Files" app).

Manual Method:

1. Open the [file on GitHub](https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip) ("Open the link" or "Copy the link") and download it: one `.zip` archive.
2. Do not unzip the archive: Razdor reads the `.zip` itself.
3. On a computer, click "Pick the game archive…" and choose the `.zip`. On Android, click "Choose the archive…" on the title screen and choose `.zip` or `.7z`. On iPhone, click "Unpack the game archive": Razdor will find the `.zip` in the "Files" app (the "On iPhone" folder > Razdor or other app folders).

## Your copy of Discord Times

Razdor is an engine for the scenarios of the original game *Discord Times* (Community Update). It reads data from **your own installed copy** while it runs. The Razdor repository contains no maps, data, texts or graphics, and nothing from your installation is copied or changed.

:::caution
Mods that modify the `DiscordTimes.exe` file itself are not supported.
:::

## How to connect the game

Razdor looks for the installed game in this order:

1. **`RAZDOR_DT_DIR`**: the path to the folder that contains `DiscordTimes.exe`. The variable can be set in the environment or in a `.env` file in the current folder or next to the program.
   ```sh
   export RAZDOR_DT_DIR="/path/to/Discord Times"
   ```
2. **Next to the game**: if you copy `Razdor-<version>.exe` (Windows), `razdor-<version>-linux` (Linux) or `razdor-<version>-macos` (macOS) directly into the folder with `DiscordTimes.exe`, Razdor uses that copy. On a Mac the game is inside the Wine or CrossOver bottle.
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

## Restoring Game Files

If the game exists but is missing files in its folder, the main menu says "Missing Game Files" and offers "Restore Game Files": missing files are taken from the same archive, and other files are not changed. Files that you changed yourself are not overwritten.

## Font setup

Russian text needs a TrueType font with Cyrillic. Razdor finds a common system font by itself. If you see transliteration instead of names, point it to your own font:

```sh
export RAZDOR_FONT="/path/to/font.ttf"
```

:::note
The game files were made available for use in Razdor by the developers of "Aterdux Entertainment" on November 12, 2026, for non-commercial use. Their rights and license remain with the developers and are not part of the Razdor GPL. Details are on the [License](/razdor/wiki/en/license/) page.
:::

