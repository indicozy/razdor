---
title: Razdor on Android
description: Installing Razdor on Android, moving saves, and gesture controls.
sidebar:
  order: 1
---

## General Information

The Android version (**`razdor-<version>.apk`** file) is experimental. It requires Android 8 or newer on ARM processors. A new APK can be installed over an old one.

## Installation and Launch

1. Install **razdor-<version>.apk**. Allow installation from this source in your Android settings.
2. On the first launch, choose one of the following options on the title screen:
   * "Download the game" — downloads the game archive (approx. 45 MB) from the community.
   * "Choose the archive…" — select a game archive (.zip or .7z) that is already on your phone.
3. If you played an older version, Razdor will offer at the start to pick the Razdor folder in the device storage: its saves move into the app's folder. You can also do it later: Settings → Other → "Move saves from the old Razdor folder…". The game is not installed by hand: Razdor downloads it itself. The old folders are not deleted.
4. Razdor does not ask for all-files access and does not use it: everything is kept in the app's folder. To move the old saves, pick the Razdor folder in the system folder picker; any other folder moves nothing, and Razdor says so.

:::note
Razdor does not request or use all-files access.
:::

## Controls

By default, your finger acts as a cursor.

* **Tap** — A click occurs where the finger is released.
* **Double Tap** — A click occurs at the first tap location; the hero's path will not change.
* **Drag one finger across the map** — Pans the map following your finger.
* **Drag through a list** — Scrolls a list or text within a window.
* **Hold and drag** — Drags an item or unit, scrollbars, minimaps, or windows. An item dragged out of a slot is placed where you release your finger: into a cell, a slot, or onto a unit.
* **Long Press** (finger remains nearly still for about a third of a second) — Right-click: shows information about an army or building and map tooltips. Item descriptions appear immediately as long as your finger is on the item; no long press is required for descriptions. The tooltip/description appears above your finger (to the side if there is no space at the top), not underneath it. Releasing the finger releases the button; no left-click occurs after the release.
* **Two-finger pinch** — Map zoom: spread fingers to zoom in, pinch to zoom out.
* **Two fingers together or one finger held while the other drags** — Map scrolling. Lists are scrolled using two fingers up or down, or via a scrollbar.

### Scrolling the Settings

The "Sound, graphics and gameplay settings" window scrolls with a finger or the mouse wheel: the right side of a tab scrolls, and so does the column of tabs on the left if it does not fit. The scrollbar on the right is the game's own, twice as wide as the one in the scenario list; you can drag its knob with a finger. The "Apply" and "Cancel" buttons stay at the bottom of the window.

### Touchpad Mode

Touchpad mode is disabled by default. It is enabled by checking the **"Touchpad mode"** box in the "Sound, graphics and gameplay settings" window (the last item in the list). The screen works like a laptop touchpad, and the game itself draws the cursor:

* Dragging one finger moves the cursor wherever your finger is;
* A tap performs a left-click at the cursor's position;
* Holding the cursor on an object for a third of a second shows info about an army or building on the map; item or spell descriptions in the victory or event windows, or item descriptions in the backpack and hero window, appear as soon as the cursor is over them;

### Screen Orientation

In the "Sound, graphics and gameplay settings" window, look for the **"Screen orientation"** line:

* **"Auto"** — The screen rotates with the phone;
* **"Landscape"** or **"Portrait"** — The screen does not rotate.

### Building Windows

In Portrait orientation: the "Barracks" and "Garrison" windows are taller. A picture of the building is at the top, similar to the "Great Hall." Tap a unit (or hover over one): its card replaces the picture at the top. When nothing is selected, the picture returns. Below the picture, a row of units (garrison or recruits), the hero's army, and tab buttons are always visible.

### Vertical Screen

When the phone is held vertically, windows span the width of the screen, and text and buttons are larger:

* **Scenario Selection** — A list of maps covering the full screen. Tap a map to open its card with a picture, name, and description; **"Back"** and **"Next"** are at the bottom.
* **Diary** — a window of its own with the **"Quests"**, **"Completed"**, **"Rumours"** and **"Messages"** and **"Chronicle"** tabs, the list of entries and the picked entry's text. The hero's stats and backpack are in the army window, the money on the bottom bar.
* **Market** — Item description at the top, followed by section icons and a list of goods below.
* **Save** — Three buttons in a single row at the bottom.
* **Main Menu** — If a mod is installed, a wide bar saying **"Mod: <name>"** is displayed above the four tiles; tapping it allows you to select a different mod.
* **Spellbook** — A **"Close"** button and an enlarged filter button are located at the bottom.
* **City** — Section buttons remain in place when switching between them.
* **Events and Dialogues** — The image is at the top, and the text occupies the full width of the screen.

When the phone is held horizontally, the windows are the same as on a PC (the journal is also a separate window: list on the left, text on the right).

## Landscape Screen

On the main menu lying sideways, the five buttons stay on the left, as before, and the four links ("Custom battle", "Maps and mods", Discord and the language) are big tiles stacked down the right edge. If a mod is installed, its "Mod: <name>" tile comes first.

The map's bottom bar has a size setting: **"Bottom bar size"** on the **"Screen"** tab of the settings window (held sideways only), with the values **"Normal"**, **"Medium"** and **"Large"**. The default is **"Large"**. With the large bar, the buttons are bigger and easier to press, the time slider is easier to press too, and the date on the bar is shown in full.

:::tip
The bottom bar size only changes the layout when the phone lies sideways. Held upright, the bar is the same at every value.
:::

## A weak phone

If the map stutters, these rows on the **"Map"** tab of the settings help:

* **"Sprite size"** (in the 2.5D view): **1×**. Trees and houses are smaller, less work for the GPU.
* **"Map resolution"**: **75%** or **50%**. The map is a little softer but drawn faster; the bars and text stay sharp.
* **"Frame rate cap"**: **30**. Frames come more evenly and the phone runs cooler.

Screen recording itself lowers the FPS a lot on a weak phone: check the speed without it.

## Razdor Updates

Razdor on Android updates itself just like on a PC: when a new version is released, a window offers the update along with a changelog. **"Update"** downloads the new APK, verifies it, and passes it to the Android installer; you then confirm the installation. An already downloaded APK will not be downloaded again, and old versions are deleted after installation. For more details: [Updates](/razdor/wiki/en/start/updates/).

:::tip
You can also update manually: download the latest **`razdor-<version>.apk`** release and install it over the old one.
:::

## Updating the Game from Archive

This is how you install the game with a mod (e.g., Community Update Evolution Mod) or a patch:

1. In the main menu, open the "Sound, graphics and gameplay settings" window.
2. Tap **"Update the game from an archive…"**.
3. Select a `.zip` or `.7z` archive.

* An archive containing the entire game folder will replace the game in the `DiscordTimes` folder after a confirmation prompt. Saves are preserved.
* An archive containing only some files (a patch or mod) is installed over the existing game.
* The archive is extracted next to the game, and the game files are only changed after successful extraction. If an error occurs, the game remains unchanged.
* This button is only available when the game is located in the Razdor `DiscordTimes` folder. A copy of the game in a different folder will not be changed.

## Data Storage

Saves (in the `saves` folder) and the log are stored in the app's own folder: `Android/data/io.github.indicozy.razdor/files/Razdor`. The game files are located alongside them in the `DiscordTimes` folder. This folder is not deleted when the app is updated.

In the loading window, the "Transfer…" button opens three actions:
* "Export saves" — all saves and pins are packed into a single .zip file together with the settings (`settings.json`, `audio.json`, `marks.json`); the file is saved via the system "Save as" dialog.
* "Import a save" — allows you to select a .rzsave file or a .zip file containing saves. Old saves are not overwritten: if the file is identical, it is skipped; if it differs, a new copy with a new number is created. Settings already on the phone are not overwritten: the settings in the file are skipped for them.
* "Export the log" — creates a game log file for submitting bug reports.

:::caution
When you uninstall the app, Android also deletes its data folder: saves, pins and settings. Always perform an "Export saves" before uninstalling. If Android asks whether to "Keep" app data during uninstallation, select "Keep": then the settings and saves remain.
:::

The image cache is stored in the app's own cache (Settings → Apps → Razdor → Storage) and does not exceed 256 MB. There is nothing critical in it: you can clear it using the "Clear cache" button; saves will not be lost, and images will simply take slightly longer to load upon the next launch.
