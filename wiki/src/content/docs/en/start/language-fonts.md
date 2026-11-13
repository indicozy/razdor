---
title: Language and Fonts
description: Instructions for switching the interface language and configuring fonts in the Razdor engine.
sidebar:
  order: 4
---

## Language

You can select a language in two ways:
1. Click the "Language" button on the "Screen" tab of the settings.
2. Use the **F2** key on any screen to quickly cycle to the next language in the list.

The following are available in the list:
* **Russian** (the default game language).
* **English**.
* Any other translations you have added to the game folder (files in the format `LanguageName_Units.ini`, etc.).

:::note
The native Razdor interface (buttons, windows, tooltips, battle log, map editor, and demo) works only in Russian and English. If "Russian" is selected, the interface will be in Russian; if any other language is selected, it will be in English.
:::

The names and descriptions of warriors, items, and spells are pulled from the files of the selected language. If the selected translation lacks a specific string, the engine will display the text in English. Scenario texts depend on the maps selected in New Game mode.

The language selection is saved in the `settings.json` file in the saves folder. On the first start the language follows the system language: Russian for Russian, Ukrainian, Belarusian and Kazakh, English for the others. Until you pick a language in the list, a change of the system language changes the game's language too. If no suitable font is found for Russian, the game launches in English.

## Fonts

In the Sound, Graphics, and Gameplay settings window, there is a "Font" item. It changes the font of the entire interface at once.

### Font Modes

* **Original** (default): uses raster fonts from your game copy. The text remains pixel-perfect because letters are only scaled by integer values. If the original font lacks certain characters (e.g., «», —, ×, №), they will be replaced by the PT Sans font.
* **Modern**: uses built-in fonts.
    * For body text — **PT Serif**.
    * For headings — **Becker Medium**.
    * For buttons and names — **Kurale**.

### Text size

The "Text size" item in the same settings: Normal, Large and Largest. At Normal everything is as in the original. At Large and Largest the buttons grow with the text throughout the game: in the menus and settings, the scenario list and saves, the army, hero and battle windows, the buildings ("Hire" and "Heal" with their prices), event windows and the map editor. Phones start at Large. Only the bottom bar's readouts and the market buttons stay as they are: the original's frame bounds them.

### Character Support

The engine can display characters from different languages, even if they are not present in the primary font. If a required character is missing, Razdor searches for it in the following order:
1. The currently selected font.
2. PT Sans.
3. **Noto Sans** (for Greek, Vietnamese, and extended Latin/Cyrillic).
4. **Noto Sans Devanagari** (for Hindi).
5. System fonts (for Chinese, Japanese, Korean, Arabic, and other languages).

:::tip
The engine correctly renders writing rules for Hindi and Arabic, properly handling ligatures and diacritics.
:::
