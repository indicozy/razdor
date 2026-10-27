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

On Windows: `%APPDATA%\razdor\saves`. On Linux: `~/.local/share/razdor/saves`. On macOS: `~/Library/Application Support/razdor/saves`. You can set your own folder with `RAZDOR_SAVE_DIR`. See [Saving and loading](/razdor/wiki/en/tutorials/saves/).

## Android cannot see the game folder. What do I do?

Give Razdor "All files access": the title screen has a "Grant access" button for this. Put the game folder in `DiscordTimes` and press "Check again". See [Razdor on Android](/razdor/wiki/en/phones/android/).

## How do I update Razdor?

Razdor checks GitHub for new releases itself and offers an update when you are not in battle. "Update" downloads the new version, which starts the next time you open the game. It needs the `curl` program. The mode is set in "Advanced…" → "Updates". See [Updates](/razdor/wiki/en/start/updates/).

## macOS will not run razdor-macos. Why?

The program is not signed by a developer. Run `xattr -d com.apple.quarantine razdor-macos`, or open it with right-click → Open in Finder. See [Installing Razdor](/razdor/wiki/en/start/install-razdor/).

## How do I change the language or the font?

The language is changed by the language name in the corner of the main menu, by the "Language" button in "Advanced…", or with the F2 key. The font is chosen in the "Sound, graphics and gameplay settings" window. See [Language and fonts](/razdor/wiki/en/start/language-fonts/).

## How do I report a bug?

Write to the [Discord](https://discord.gg/cdTtYz5dH) or open an issue on [GitHub](https://github.com/indicozy/razdor/issues).