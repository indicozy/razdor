---
title: Cheat Console
description: A guide on how to use the developer console and the list of commands for Razdor.
sidebar:
  order: 3
---

## Cheat Console

The console allows you to enter commands to modify the gameplay. It can be opened both on the world map and during battle.

To open the console, press the **~** key (tilde, located to the left of the "1" key or "Ё" in Russian layout). After pressing it, a command line will appear at the top of the screen.

:::note
While the console is open, all key presses are sent to the console, not to the game.
:::

### Console Controls
* **PgUp / PgDC** or **Mouse Wheel**: scroll through the output history.
* **Up and Down Arrows**: return to previously entered commands.
* **~** or **Esc**: close the console.

### Typing Tips

As you type, a list of suggestions appears below the input line (up to 8 lines): command names, and after `item `, `spell `, or `unit `, the names of items, spells, and units in the current game. The command line shows its syntax (e.g., `gold N`) and its description in gray.

The Tab key completes the first suggestion; each subsequent press of Tab moves to the next line (Shift+Tab moves backward). The Up and Down arrows select a line in the list while it is open; the command history is not called during this time. Enter completes the selected line and executes it only if it is a command without arguments (e.g., `reveal`); in all other cases, Enter only completes the line, and it must be sent again. On mobile, simply tapping a line in the list will complete it, just like using Tab.

### Commands

Command names are written in English. Case does not matter. If you enter the name of an item, spell, or unit, the engine first looks for an exact match, then a match at the start of the word, and then a match within the word. If multiple objects meet the criteria, the console will notify you.

| Command | Effect |
| --- | --- |
| `help` | Shows a list of all commands with descriptions in the interface language |
| `gold N` | Gives N gold (a negative N subtracts gold) |
| `mana N` | Gives N mana (a negative N subtracts mana) |
| `reveal` | Uncovers the entire map |
| `heal` | Heals the army and revives fallen units |
| `xp N` | Gives N experience to every living warrior in the army (levels increase as they would after a battle) |
	| `level N` | Sets the hero's level to N |
| `item <number or name>` | Adds an item to your backpack |
| `spell <number or name>` | Adds a spell to your spellbook (maximum 15) |
| `unit <number or name> [level]` | Adds a warrior to the army (if there is space) at the specified level |
| `time H` | Skips time by H hours (affects noon, events, and AI) |
| `win` | Ends the battle with your victory |
| `lose` | Ends the battle with your defeat |
| `god` | Toggles invulnerability for your army in battle |
| `speed N` | Speeds up only the hero: for every step they take, N times less game time passes, meaning enemy armies will advance less during that time (1 — normal speed) |
| `noclip` | Toggles the ability to pass through obstacles |
| `peace` | Toggles peace mode (enemies will not attack the hero, but the hero can attack them) |
| `routes` | Toggles the display of enemy army paths on the minimap |
| `event N` | Triggers map event number N immediately; its conditions are not checked. The number is the same as in DTMapEdit (the original's map editor), starting from 1. Used for map testing. |

:::caution
The `peace` command does not affect garrisons, building guards, or battles scripted within scenario events.
:::

### Important Features
* **Saves**: Using cheats marks the save file. In the load list, the notation "(cheats)" will appear after the scenario name.
* **Log**: Every used command is recorded in the game log.
* **Battles**: During battle, the army, its experience, and time do not change. However, the `win` and `lose` commands work during combat, ending it with the appropriate result (results screen, experience, losses).
