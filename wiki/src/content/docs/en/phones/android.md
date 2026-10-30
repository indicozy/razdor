---
title: Razdor on Android
description: Installing Razdor on Android, access to the game files, and gesture controls.
sidebar:
  order: 1
---

## General information

The Android version (the **razdor.apk** file) is experimental. It needs Android 8 or newer on ARM processors. A new APK is installed over the old one.

## Installing and starting

1. Install **razdor.apk**. Allow installs from this source in the Android settings.
2. Put the "Discord Times" game folder in internal storage, in the `DiscordTimes` folder (`Internal storage/DiscordTimes`) or in the `Download` folder. It must contain the `Maps_Rus` folder and the `Rus_Units.ini` file, as the folder with `DiscordTimes.exe` does on a computer.
3. Give Razdor **"All files access"**.
   * If the game is not visible, a **"Grant access"** button appears on the title screen. It opens the right settings page, where access is switched on.
   * Manually: Settings → Apps → Special app access → All files access → Razdor. On Android 8–10 the button asks for the normal files permission.
4. Go back to Razdor and press **"Check again"**. The title screen also checks for the game itself every few seconds.

:::caution
Without all-files access the game cannot be read: the title screen stays with the demo map.
:::

## Controls

By default, your finger works as the cursor.

* **Tap** — a click where the finger lifts.
* **Double tap** — a click at the place of the first tap; the hero's route does not change.
* **Drag one finger over the map** — moves the map with the finger.
* **Drag over a list** — scrolls the list or the text in a window.
* **Hold and move** — drags an item or a unit, scroll bars, the minimap and windows.
* **Long press** (the finger almost still for about a third of a second) — the right mouse button: details of an army or a building, tooltips on the map and item descriptions. Lifting the finger releases the button; there is no left click after it.
* **Pinch with two fingers** — map zoom: spread to zoom in, pinch to zoom out.
* **Two fingers together, or one held while the other moves** — scrolls the map. Lists scroll with two fingers moved up or down, or with the scroll bar.

### Touchpad mode

Touchpad mode is off by default. It is switched on with the **"Touchpad mode"** checkbox in the "Sound, graphics and gameplay settings" window (the last item in the list). The screen then works like a laptop touchpad, and the game draws the cursor:

* dragging one finger moves the cursor, wherever the finger is;
* a tap is a left click at the cursor's position;
* the cursor resting for a third of a second over an item or spell icon in the victory or event window shows its description;
* resting the cursor over an army or building on the map for a third of a second shows its information;

### Screen orientation

In the "Sound, graphics and gameplay settings" window, the **"Screen orientation"** row:

* **"Auto"** — the screen turns with the phone;
* **"Landscape"** or **"Portrait"** — the screen does not turn.

### Building windows

Portrait: the Barracks and Garrison windows are taller. The top is the building's picture, as in the Main hall. Tap a unit (or point at it): its card replaces the picture at the top. When nothing is selected, the picture returns. Below it there is always the row of units (the garrison or the recruits), then the hero's army and the tab buttons.

## Razdor updates

Razdor on Android updates itself, as on a computer: when a new version is out, a window offers it with the list of changes. **"Update"** downloads the new APK, checks it and hands it to the Android installer; you confirm the installation. An APK already downloaded is not downloaded again, and old ones are removed after installing. More: [Updates](/razdor/wiki/en/start/updates/).

:::tip
You can also update by hand: download **razdor.apk** from the latest release and install it over the old one.
:::

## Updating the game from an archive

This is how a game with a mod (for example, the Community Update Evolution Mod) or a patch is installed:

1. In the main menu, open the "Sound, graphics and gameplay settings" window.
2. Press **"Update the game from an archive…"**.
3. Choose a `.zip` or `.7z` archive.

* An archive with the whole game folder replaces the game in the `DiscordTimes` folder, after a confirmation. Saves stay.
* An archive with some of the files (a patch or a mod) is installed over the game.
* The archive is unpacked next to the game, and the game changes only after a successful unpack. On an error the game stays as it was.
* The button appears when the game is in the `DiscordTimes` folder used by Razdor. A copy of the game in another folder is not changed.

## Data storage

Saves and the journal are in `Internal storage/Razdor/saves`. The folder is not deleted with the app, so reinstalling does not erase your saves.

The picture cache is in the app's own cache (Settings → Apps → Razdor → Storage) and takes at most 256 MB. Nothing important is in it: "Clear cache" is safe, your saves stay, and pictures just load a little longer on the next start.
