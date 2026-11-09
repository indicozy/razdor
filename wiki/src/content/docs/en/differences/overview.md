---
title: Feature Overview
description: What Razdor adds to the original "Discord Times".
sidebar:
  order: 1
---

Razdor plays the original game's scenarios using its original rules. Below is what it adds beyond the original. Bug fixes are not listed here.

## Interface and Screen

* **UI Scale** from 1× to 4×, "Auto" is the largest that fits within the window.
* **Screen:** window, borderless window, or full screen (via "Screen" settings).
* **Sharp graphics:** the original's pictures are not blurred when scaled up: every pixel stays a crisp square of the same size at any scale.
* **Minimap** can be resized and moved to any corner of the screen.
* **Closing windows:** a window opened from the bottom panel can be closed by clicking or tapping outside of it, or by pressing `Esc`.
* **Equipment Search** (`Ctrl+F`): filters items in your backpack and at the market by name, type, stats, and description.
* **Sorting** of goods at the market and shrine by name or price.
* **Combat:** warrior cards show stat changes relative to the start of the battle (blue for increase, red for decrease); spell icons on cards.
* **Uncovering locations:** a location revealed by an event unfolds from the fog in a circle, from the center to the edges.
* **Touch modes and gamepads:** gamepad support on Windows, Linux, and macOS; touchpad mode on mobile devices.

## Convenience

* **Armies:** reorder warriors by dragging; pursue a selected army until an encounter occurs; the path is shown on click, and a second click sends the hero on the journey.
* **Clicking on the map** while walking stops the hero; **waiting** can be interrupted by a click or a keypress.
* **Hero Speed:** the "Hero Movement Speed" slider goes up to 2× faster than the fastest speed in the original (the middle of the slider is the original maximum).
* **Hero Log:** four tabs — "Quests", "Completed", "Rumors", "Messages", with entries marked by in-game date.
* **Saves:** up to 50 manual saves and 30 autosaves instead of 12; pin important saves with a thumbtack; quick save `F5` and quick load `F9`.
* **Folder:** the «Folder» button in the load window opens the folder with your saves and maps, on desktop, Android and iOS (Razdor's own, not the original's).
* **Resuming a game:** if your phone clears the game from memory or the game crashes, upon the next launch, it will offer to resume from where you left off ([more details](/razdor/wiki/en/tutorials/saves/)).
* **Victory window:** without the "you won gold" and "trophies" lines, so the gold, mana and loot show at once, without scrolling.
* **Defeat screen:** the "Load last save" button loads the most recent save of any type (manual, quick, or autosave); "Exit to main menu" replaces the old "OK" button.
* **Backpack:** `Shift`+click equips an item or drinks a potion; `Shift`+click on an equipped item unequips it.
* **Custom Battle:** any two armies outside of a campaign, with levels and items; the battle code can be copied to a string and pasted for another player.
* **Quick Battle:** autobattle via `Q` or `Enter`; battle replay (`W`) with 1×, 2×, or 4× speed.
* **New Game:** campaigns are grouped into a single line; the list can be filtered and sorted; "Add map..." button.
* **Experimental Scenario Rules:** fifteen toggles for new games; selections are remembered between games (see [Experimental Rules](/razdor/wiki/en/differences/experimental-rules/)).
* **Save formation** in additional settings.
* **Experience on promotion** in additional settings: as in the original, a unit's experience is dropped when it is promoted to a new class; "Kept" keeps it.
* **Leader's name under the event picture:** when an event fires on meeting an army, its window writes the army's leader name under the picture; turn it off in the additional settings ("Leader's name").
* **Minimap corner** in additional settings: the minimap can be placed in any corner, and the "Reset" button will return it to its default size and position.
* **2.5D map view** in additional settings ("Map view", off by default): the map is tilted, and the figures stand on the ground. Q and E turn the map 45°; on a phone, the two-finger turn is switched on by the "Two-finger turn" setting. A compass button next to the bottom bar appears while the map is turned, and brings it back to north up. "Sprite size" (1×, 1.5× or 2×) is 1.5× by default.
* **Extend the map past its edges** in additional settings (off, as in the original, where the area past the edges is black): a short stretch of ground goes on past the map's edges, then the same black fog as the unexplored map. Only the view changes: rules and paths stay as they are.

## Sound

* Music changes based on context, just like in the original; volume settings are in the `Esc` menu.
* More details: [Sound and Music](/razdor/wiki/en/differences/sound-music/).

## Tools and Mods

* **Map Editor** with map search (`Ctrl+F`) and an event panel.
* **Map Translator** `razdor-translate` translates maps and game data using a language model.
* **Cheat Console** (key `~`).
* **Debug Layer** (`F3` on the map) shows events, lights, and tile passability.
* **Mods:** a game installed with a mod (custom `Rus_*.ini`, graphics, and maps) is playable with its warriors, items, and spells. The selected mod is visible in the main menu and can be changed directly there.
* **Updates:** Razdor checks for new releases on GitHub itself and offers updates.

:::note
For automatic updates on Windows, macOS, and Linux, the `curl` program is required.
:::

## Phones

* **Android** (trial version): `razdor-<version>.apk` file, touch controls, touchpad mode, screen orientation.
* **iPhone and iPad** (trial version): `razdor-<version>.ipa` file, touch controls.
* More details: [Razdor on Android](/razdor/wiki/en/phones/android/), [Razdor on iOS](/razdor/wiki/en/phones/ios/).

## Game AI

* **Improved enemy intelligence** in combat, as found in the original settings.
* **"Maximum (Razdor)"** — an experimental mode: the enemy calculates moves and responses several actions ahead. Selected in the "Battle and army..." group → "Battle AI strength".

:::caution
After using the cheat console, a save file will be marked with "(cheats)" in the load list.
:::
