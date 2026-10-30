---
title: Cheat Console
description: A guide on how to use the developer console and the list of commands for Razdor.
sidebar:
  order: 3
---

## Cheat Console

The console allows you to enter commands to modify the gameplay. It can be opened both on the world map and during battle.

To open the console, press the **~** key (tilde, located to the left of the "1" key). After pressing it, a command line will appear at the top of the screen.

:::note
While the console is open, all key presses are sent to it rather than to the game.
:::

### Console Controls
* **PgUp / PgDn** or **Mouse Wheel**: scroll through the output history.
* **Up and Down Arrows**: cycle through previously entered commands.
* **~** or **Esc**: close the console.

### Suggestions While Typing

As you type, a list of up to 8 suggestions appears under the input line: command names, and after `item `, `spell ` or `unit ` the names of items, spells and units in the current game. Each row shows the command's usage, with its description in grey.

Tab completes the first suggestion, and each further Tab moves to the next row (Shift+Tab goes back). Up and Down arrows select a row while the list is open; the command history is not recalled then. Enter completes the selected row and runs it only if it is a command that takes no arguments (for example, `reveal`); otherwise Enter only completes the line, and you press Enter again to run it. On a phone, tapping a row completes to it, as Tab does.

### Commands

Command names are written in English. Case does not matter. If you enter the name of an item, spell, or unit, the engine first looks for an exact match, then a prefix match, and finally a substring match. If multiple objects meet the criteria, the console will notify you.

| Command | Effect |
| --- | --- |
| `help` | Shows a list of all commands with descriptions in the interface language |
| `gold N` | Gives N gold (a negative N subtracts gold) |
| `mana N` | Gives N mana (a negative N subtracts mana) |
| `reveal` | Reveals the entire map |
| `heal` | Heals the army and raises its fallen |
| `xp N` | Gives N experience to every living warrior in the army (levels increase as they would after a battle) |
| `level N` | Sets the hero's level to N |
| `item <number or name>` | Adds an item to your backpack |
| `spell <number or name>` | Adds a spell to your spellbook (up to 15 in the book) |
| `unit <number or name> [level]` | Adds a warrior to your army (if there is space) at the specified level |
| `time H` | Skips time by H hours (affects noon, events, and AI) |
| `win` | Ends the battle with your victory |
| `lose` | Ends the battle with your defeat |
| `god` | Toggles army invincibility in battle |
| `speed N` | Speeds up the hero only: each of his steps takes N times less game time, so enemy armies move less meanwhile (1 — normal speed) |
| `noclip` | Toggles the ability to pass through obstacles |
| `peace` | Toggles peace mode (enemies will not attack the hero, but the hero can attack them) |
| `routes` | Toggles the display of enemy army paths on the minimap |

:::caution
The `peace` command does not affect garrisons, building guards, or battles scripted within scenario events.
:::

### Important Notes
* **Saves**: Using cheats marks the save file. In the load list, a "(cheats)" tag will appear after the scenario name.
* **Log**: Every used command is recorded in the game log.
* **Battles**: During combat, the army, its experience, and time do not change. However, the `win` and `lose` commands work during battle, ending it with the corresponding outcome (results window, experience, losses).
