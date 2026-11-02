---
title: Razdor on Android
description: Installing Razdor on Android, accessing game files, and gesture controls.
sidebar:
  order: 1
---

## General Information

The Android version (**razdor.apk** file) is experimental. It requires Android 8 or newer on ARM processors. A new APK can be installed over an old one.

## Installation and Launch

1. Install **razdor.apk**. Allow installation from this source in your Android settings.
2. Place the "Discord Times" game folder into your internal storage in the `DiscordTimes` folder (`Internal Storage/DiscordTimes`) or in the `Download` folder. It must contain the `Maps_Rus` folder and the `Rus_Units.ini` file, just like the folder containing `DiscordTimes.exe` on a PC.
3. Grant Razdor **"All Files Access"**.
   * If the game is not visible, a **"Grant Access"** button will appear on the title screen. This will open the required settings page where access can be enabled.
   * Manually: "Settings" → "Apps" → "Special app access" → "All files access" → "Razdor". On Android 8–10, the button requests standard file permissions.
4. Return to Razdor and tap **"Check Again"**. The title screen also checks the game files every few seconds.

:::caution
Without "All Files Access," the game cannot be read: the title screen will remain stuck on the demo map.
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
* **Journal** — A separate window with tabs in a single ribbon format (similar to the PC journal: parchment style, with the selected tab in red and underlined), including **"Quests"**, **"Completed"**, **"Rumors"**, and **"Messages"**, featuring a list of entries and the text of the selected entry. Hero stats and the backpack are in the army window; money is on the bottom panel.
* **Market** — Item description at the top, followed by section icons and a list of goods below.
* **Save** — Three buttons in a single row at the bottom.
* **Main Menu** — If a mod is installed, a wide bar saying **"Mod: <name>"** is displayed above the four tiles; tapping it allows you to select a different mod.
* **Sound, graphics and gameplay settings** — A window with two tabs: **"Sound and speed"** (with sliders) and **"Game"** (with checkboxes and lines such as touchpad mode, screen orientation, text size, large windows, and update game from archive). **"Apply"** and **"Cancel"** buttons are at the bottom. Tapping **"Advanced…"** opens additional settings; you return to the previous tab from there.
* **Additional Settings** — A window (opened via the **"Advanced…"** button) with a single column of lines. Each line contains a name on the left and a button on the right with a description underneath. The list is scrollable; the **"OK"** button is always at the bottom.
* **Spellbook** — A **"Close"** button and an enlarged filter button are located at the bottom.
* **City** — Section buttons remain in place when switching between them.
* **Events and Dialogues** — The image is at the top, and the text occupies the full width of the screen.

When the phone is held horizontally, the windows are the same as on a PC (the journal is also a separate window: list on the left, text on the right).

## Razdor Updates

Razdor on Android updates itself just like on a PC: when a new version is released, a window offers the update along with a changelog. **"Update"** downloads the new APK, verifies it, and passes it to the Android installer; you then confirm the installation. An already downloaded APK will not be downloaded again, and old versions are deleted after installation. For more details: [Updates](/razdor/wiki/en/start/updates/).

:::tip
You can also update manually: download the latest **razdor.apk** release and install it over the old one.
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

Saves and logs are located in `Internal Storage/Razdor/saves`. This folder is not deleted along with the application, so reinstalling does not erase your saves.

The image cache is stored in the application's own cache (Settings → Apps → Razdor → Storage) and takes up no more than 256 MB. There is nothing important in it: it can be cleared using the "Clear Cache" button; saves will not be lost, but images may take slightly longer to load on the next launch.
