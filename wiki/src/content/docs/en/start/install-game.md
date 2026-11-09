---
title: Game installation
description: How to connect your own Discord Times copy to Razdor.
sidebar:
  order: 1
---

## Your copy of Discord Times

Razdor is an engine for the scenarios of the original game *Discord Times* (Community Update). It reads data from **your own installed copy** while it runs. The Razdor repository contains no maps, data, texts or graphics, and nothing from your installation is copied or changed.

You need version 1.8 with Community Update 1.2 (game 1.8.1). If the game is missing, the main menu explains that you need your own copy and offers to download it (see «Download the game» below), and it links to the [installation guide](https://indicozy.github.io/razdor/install.html) and to the Discord for help.

## Download the game

If the game is missing, the main menu downloads it itself from the community's Google Drive (the «ИГРУ КАЧАТЬ ЗДЕСЬ» thread on the Discord server): one `.zip` file of about 45 MB. Press «Download the game» on the main menu. While it runs, the megabytes done are shown. «Cancel» stops it; the part already downloaded is kept, and the next press goes on from it. The file is checked by its size and checksum, then unpacked into the game folder. When the game is in place, the main menu opens as usual.

If the archive is already on your disk, press «Pick the game archive…» (on a phone, the system's file window opens) and choose the `.zip`: it is installed the same way as the downloaded one.

If the download does not work (no network, Google Drive does not give the file, or refuses it because too many people downloaded it), the same screen shows the reason and the manual way:

1. Open the [file page](https://drive.google.com/file/d/1xKIpVbDIuCWh7LFgtFn59D9t5RPkiH0y/view) («Open the link» or «Copy the link» on the screen) and download the file: one `.zip` archive.
2. Do not unpack it: Razdor reads the `.zip` itself.
3. On a computer, press «Pick the game archive…» and choose the `.zip`. On a phone, press «Unpack the game archive»: Razdor finds the `.zip` in the Downloads folder (Download) on Android or in the Files app on iPhone; on Android «Pick the game archive…» works too.
4. The game is in place when the main menu opens without the «Discord Times not found» message.

If the game is there but some of its files are missing, the main menu says «Missing game files» and offers «Restore the game files»: the missing files are taken from the same archive, the other files stay as they are. Files you have changed yourself are not overwritten.

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

:::caution
Mods that change the `DiscordTimes.exe` file itself are not supported.
:::

## Font setup

Russian text needs a TrueType font with Cyrillic. Razdor finds a common system font by itself. If you see transliteration instead of names, point it to your own font:

```sh
export RAZDOR_FONT="/path/to/font.ttf"
```
