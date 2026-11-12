---
title: FAQ
sidebar:
  order: 0
---

## Razdor cannot find my game. What do I do?

You need your own copy of *Discord Times* version 1.8 with Community Update 1.2. Razdor looks for it through `RAZDOR_DT_DIR`, next to its own program, in the remembered path, and in `~/Games`, `~/Downloads` and the home folder. The reason is written to `razdor.log`. See [Game installation](/razdor/wiki/en/start/install-game/).

## How do I install a mod?

In the "Sound, graphics and gameplay settings" window, press "Update the game from an archive…" and choose a `.zip` or `.7z` archive. An archive with the whole game replaces it; an archive with some of the files is installed over it. On a computer you can also point `RAZDOR_DT_DIR` at a game install that has the mod. See [Installing mods and patches](/razdor/wiki/en/mods/install-mod/).

## Where are my saves?

On Windows: `%APPDATA%\razdor\saves`. On Linux: `~/.local/share/razdor/saves`. On macOS: `~/Library/Application Support/razdor/saves`. You can set your own folder using `RAZDOR_SAVE_DIR`. See [Saving and Loading](/razdor/wiki/en/tutorials/saves/). On Android, saves are located in the app's private folder: `Android/data/io.github.indicozy.razdor/files/Razdor/saves`, and they can be exported or imported using the "Transfer…" button in the loading window (see the Android page).

## Android cannot find the game. What should I do?

Razdor on Android stores the game and saves in its own app folder and does not request "All files access." If the game is not found, tap "Download the game" or "Choose the archive…" on the title screen. Saves of an older version move through the folder picker: Razdor offers it at the start, or in Settings ("Other").

[Razdor on Android](/razdor/wiki/en/phones/android/)

## How do I update Razdor?

Razdor checks GitHub for new releases itself and offers an update when you are not in battle. "Update" downloads the new version, which starts the next time you open the game. It needs the `curl` program. The mode is set on the "Other" tab → "Updates". See [Updates](/razdor/wiki/en/start/updates/).

## macOS will not run `razdor-<version>-macos`. Why?

The program is not signed by a developer. Run `xattr -d com.apple.quarantine razdor-<version>-macos`, or open it with right-click → Open in Finder. See [Installing Razdor](/razdor/wiki/en/start/install-razdor/).

## How do I change the language or the font?

The language is changed by the language name in the corner of the main menu, by the "Language" button on the "Screen" tab, or with the F2 key. The font is chosen in the "Sound, graphics and gameplay settings" window. See [Language and fonts](/razdor/wiki/en/start/language-fonts/).

## How do I report a bug?

Write to the [Discord](https://discord.gg/wmMksmWFDM) or open an issue on [GitHub](https://github.com/indicozy/razdor/issues).