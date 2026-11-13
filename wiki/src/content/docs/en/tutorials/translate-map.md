---
title: How to translate a map
description: Step by step, how to make an English copy of a map or translate the game's data with razdor-translate, check it in Razdor, and share it.
sidebar:
  order: 10
---

`razdor-translate` makes translated copies of maps: `Name.DTm` becomes `Name_en.DTm` (for English) next to the original. A language model you connect yourself does the translating. The program ships in every Razdor release. For the general notes and all settings, see [Tools: Map translator](/razdor/wiki/en/tools/map-translator/); this page is the order of steps.

## What you need

* An installed copy of Discord Times (Community Update, or another mod with its maps) and Razdor. The program finds the game the same way Razdor does. If it does not find it, point the `RAZDOR_DT_DIR` variable at the game's folder.
* A language model with an OpenAI-compatible API. Two options:
  * **on your own computer**: Ollama or LM Studio. The address for Ollama is `http://localhost:11434/v1`, no key needed. Download the model first (in Ollama: `ollama pull <model>`);
  * **in the cloud**: OpenAI, OpenRouter, DeepSeek and others. You need the service's address and a key, and the map text is sent to their server.
* Disk space: the program saves its progress after every model answer, so you can stop a translation and continue it later.

## Step 1. List the maps

Without a window, the program lists the maps and campaigns of the installed game.

**Windows** (in the folder with `Razdor-translate-<version>.exe`, or with its full path):

```
Razdor-translate.exe --list
```

If the game is not found, point to its folder first. In PowerShell:

```
$env:RAZDOR_DT_DIR="D:\Games\Discord Times"
.\Razdor-translate.exe --list
```

In the `cmd` prompt, use `set RAZDOR_DT_DIR=D:\Games\Discord Times`, then `Razdor-translate.exe --list`.

**Linux** (in the folder with `razdor-translate`):

```
./razdor-translate --list
```

If the game is not found: `RAZDOR_DT_DIR="/path/to/the/game" ./razdor-translate --list`.

## Step 2. Translate a map

A campaign is translated together with all its chapters: the program takes the map and every map it links to.

The example below uses a local Ollama model. Replace `My map` with the map's file name or its start, and `<model>` with the model you downloaded.

**Windows** (PowerShell):

```
.\Razdor-translate.exe --map "My map" --endpoint http://localhost:11434/v1 --model <model> --language English --context 8192
```

**Linux**:

```
./razdor-translate --map "My map" --endpoint http://localhost:11434/v1 --model <model> --language English --context 8192
```

What the flags mean:

* `--map`: the map or the start of its name. The `.DTm` extension can be left out.
* `--endpoint`: the API address. For a cloud service, give its address (for example `https://api.openai.com/v1`) and a key.
* `--model`: the model's name on that server. The default is `gemma4`.
* `--language`: the language to translate into. The default is `English`.
* `--context`: the model's context window in tokens. It sets how much text goes out at once: a smaller window means shorter requests.

It is better to pass the API key in a variable, so it does not stay in the command history: `RAZDOR_LLM_KEY` (in PowerShell: `$env:RAZDOR_LLM_KEY="your-key"`), instead of `--key`.

Other useful flags:

* `--notes "text"`: what the model should know about the maps: who is who, the story, which names must not change.
* `--style "text"`: your wishes for the style, for example `"like a chronicle"`.
* `--no-write`: translate only, without writing files into the maps folder.
* `--fresh`: translate everything again and forget the old translation. Use it after changing the model, the style or the context: an old translation does not update by itself.
* `--show-prompt`: print the instructions the model gets, then stop.
* `--save-settings`: remember the address, model, language and other settings for the next runs.
* `--help`: the full list of flags.

The program prints its progress: which strings were sent, how many were accepted, and which ones the model did not accept (with the reason). An answer with an error is requested again. If something did not work, run the same command again: finished work is kept, and the failed strings are retried.

When everything is translated, the English copies are written into the game's maps folder, next to the originals. Older copies with the same name are replaced.

## Step 3. Or translate in the window

Start the program without flags (double-click `Razdor-translate.exe`, or run `./razdor-translate` on Linux). In the window:

1. Enter the API address and the model, then press "Check": the program shows whether the server answers and has the model.
2. In "Translate to", choose the language.
3. In "Letters", choose "Latin or Cyrillic" (see below).
4. If you like, fill in "Translation style" and "Context" (notes about the maps).
5. Pick a map or a campaign on the left and press "Translate".
6. When the work is done, press "Write the English maps".

For another language the program makes its own copies, for example `Name_german.DTm`. The English version is always `Name_en.DTm`.

## Step 4. Translate the game data (optional)

Names and descriptions of units, items, spells and the game's texts are translated separately. Without this step, the names of units, items and spells stay in Russian in the game; the maps are translated without it.

**Windows**: `.\Razdor-translate.exe --data --endpoint http://localhost:11434/v1 --model <model> --language English`

**Linux**: `./razdor-translate --data --endpoint http://localhost:11434/v1 --model <model> --language English`

Files `Eng_Units.ini`, `Eng_Artefacts.ini`, `Eng_Spells.ini` and `Eng_DiscordTimes.ini` appear in the game's folder. Razdor finds them at startup and adds the language to its language list. The original game does not read them.

## Step 5. Check it in Razdor

1. Start Razdor. You switch language with the link showing its name in the corner of the main menu, or with the **F2** key on any screen (it cycles through the language list). The list has **English** and any data translations you made.
2. Open "New game". The translated map or campaign appears in the list next to the original: its file is called `Name_en.DTm`.
3. Choose it and start playing. Check that the text reads well: names, dialogue, event descriptions.

If the text is missing or shows as unreadable symbols, see "Common problems" below.

## Share a translation

The translator does not upload anything to the "Maps and mods" catalog and does not publish translations itself. A finished copy is simply the file `Name_en.DTm` in the game's maps folder.

* Ask the map's author first: the translation uses their text, and the decision is theirs.
* Send the file `Name_en.DTm` (and the `Eng_*.ini` files, if you translated the data) to the map's author, to the community server on Discord, or to the map's discussion thread. Say that it is a translation and name the map's author.
* Someone who wants your translation puts `Name_en.DTm` into the maps folder of their own game.
* Do not add the translator's files to your own mod archive: when the catalog packs mods, it skips them.

:::caution
Translated maps are made from your game's files. Do not post them to shared repositories or put them into archives you distribute yourself.
:::

## Common problems

* **Garbled text.** The original game shows only the letters of the Windows-1251 code page (Cyrillic and Latin letters without diacritics). If "Letters" is set to "any script" (umlauts, Devanagari, Chinese characters), the map works only in Razdor: the original shows those texts garbled. For maps that must also open in the original, choose "Latin or Cyrillic".
* **A text was not accepted; the log gives the reason.** With "Latin or Cyrillic", the program does not write a string that has a letter Windows-1251 lacks. If the map is needed only in Razdor, choose "any script". Otherwise change the style or the notes so the model sticks to Latin letters, and run the translation again.
* **A markup or placeholder error.** The program checks placeholders such as `#HERONAME`, markup marks and line breaks in every model answer. If the model broke one, the answer is requested again. If it still fails, write in the notes ("Context" or `--notes`) that these marks must stay as they are.
* **Names that must not change.** Write the names that must stay as they are (hero names, a title you want kept) in the notes. The program tries to write one name the same way throughout a map, but a hint in the notes is more reliable.
* **Old translation after a change of model or style.** It does not update by itself. Run the translation with `--fresh` (in the window: "Translate again").
* **The model does not answer or there is no connection.** Check the address, that the server is running and that the model is downloaded (in Ollama: `ollama list`). The "Check" button in the window shows the same.
* **A long campaign.** A big campaign takes time. You can stop it: finished strings are saved, and the next run continues from where it stopped.
* **The map is not in the New game list.** Check that the program wrote the files (the `wrote …` line at the end of the run) and that they are in the maps folder of the game you start.

:::note
The translator changes only the copies. The original map file and the rest of the game's files are not changed.
:::
