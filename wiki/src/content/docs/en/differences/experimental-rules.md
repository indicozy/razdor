---
title: Experimental scenario rules
description: Scenario rules in Razdor — "DT rules" and "Razdor's rules", fixes and cheats, and what each switch changes.
sidebar:
  order: 2
---

On the scenario screen, next to the "Next" button, there is a "..." button. It opens the "Experimental rules" window with eighteen switches that change the game rules. They are in two groups: **"Fixes"** and **"Cheats"**.

The original game has none of these switches. For a new game all of them are off by default, so the game follows the original's rules.

:::caution
Your choice is remembered. It is saved to the settings when you close the window ("OK", Esc, Enter or its close button), and it is loaded when the game starts. Returning from the hero selection keeps it too. An ongoing game does not change through this menu. Each new playthrough starts from the saved choice.
:::

## DT rules and Razdor's rules

Two buttons sit at the top of the window:

* **"DT rules"**: every fix is off. The rules are the original's.
* **"Razdor's rules"**: every fix is on.

The pressed button shows which set is active. If you turned fixes on one by one, neither button is pressed, and the line below them says "Mixed".

The buttons change only the fixes. The cheats stay as they were.

## Fixes

Fixes change behaviour that the original shows as a bug or as something that does not work as designed. Each fix can be turned on on its own. A fix that is off gives the original's rule.

* **Faster AI:** on maps of 300×300 cells or more, armies search for a path only in a square around themselves. An army with one target goes to it by the shortest path. Armies move a little differently from the original.
* **Bless once a turn:** a unit gets a blessing at most once per turn. In the original, blessings stack.
* **Poison spares the Undead:** Undead cannot be poisoned. In the original, poison works on every unit.
* **No spell book limit:** any spell in the book can be cast. In the original, only the first 15 spells of a book can be cast.
* **Merchant discount from an item:** a Merchant item in the hero's pack gives the discount. In the original, only units in the army do.
* **Rear Service always −30%:** the wage cut is always 30%. In the original, with no income from towns, the cut is nearly 70%.
* **Store in your own buildings:** in your castle, fort or town where the hero stands, a "Store" tab appears with the loot that the garrison could not equip. In the original, the loot stays in the building.
* **Death curse on counter blows:** if a counter blow kills a unit with the death curse, the attacker dies too. In the original, the attacker survives.
* **No corpses in garrisons:** a fallen unit cannot enter a castle or fort garrison, and cannot be swapped with one. In the original, it can.
* **Strict initiative order:** the next to act is always the group with the highest initiative, even if it rose during the battle. In the original, a group whose initiative rose after its place in the queue had passed waits for its turn.
* **Experience kept on promotion** ("Experience on promotion kept", off by default): after a unit is promoted, it keeps its experience. In the original, the experience is reset. Part of the "Razdor's rules" preset.

## Cheats

Cheats give the player an edge or remove a limit. A cheat that is on marks the run yellow (see [completion marks](/razdor/wiki/differences/run-marks/)).

* **All special bonuses at once:** a unit keeps the abilities of its type and of its equipped items. Identical abilities do not stack.
* **Six equipment slots:** a unit has six item slots instead of four.
* **No item limits:** any item can go on any living unit while a slot is free. Potions and other goods still cannot be worn, and the dead wear nothing.
* **At most two potions per character:** between battles a character can drink at most two potions, healing and revival ones included. The limit resets after a battle.
* **Free units:** units of the hero's army pay no daily wage. Hiring costs the same as usual.
* **Automatic promotion:** a unit of the hero's army is promoted by itself, along a random branch of its tree, as soon as a promotion opens. The hero is not promoted this way.
* **Units don't die:** a fallen unit of the hero's army comes back with 1 HP after a battle, if the army survives the battle.

:::tip
The item and potion rules apply to all armies: yours, your opponents' and the garrisons'. "Free units", "Automatic promotion" and "Units don't die" apply only to the hero's army.
:::

## Warrior Participation in Battles

A living non-hero can be left out of a battle. In the army window, top right, above the "Don't remind" line, there is a check box **"Does not participate in battle"**. It is a normal option of the game, not a setting and not a departure from the rules: it does not turn the run's mark grey.

* The marked warrior remains in the squad, occupies their cell in the formation, and receives wages.
* They do not participate in battle, do not gain experience, and are not counted among the battle participants.
* To return a warrior to battle, untick the box. A hero cannot be marked.

There is also a check box for one battle only: after "Restart" (see "Quick battle"), click on your warrior and the left panel shows the check box **"Sits out this fight"**. While it is ticked, the warrior stands dimmed in its cell and does not fight in this battle. After the first move the check box is gone.

## Army Deployment and Pathfinding

On the settings tabs, there are two additional Razdor parameters:

* **"Preserve formation"** (on by default): after a battle, all warriors, including those who perished, return to the cells they occupied before the battle. If off — warriors remain in the cells where they ended the battle, as in the original.

## Quick Actions with Shift

* **Moving a warrior:** `Shift` + click on a garrison or squad card moves the warrior to an empty slot in another formation.
* **Equip item:** `Shift` + click on an item in the backpack equips it to the hero (or drinks a potion).
* **Unequip item:** `Shift` + click on an equipped item returns it to the backpack.

## "Maximum" Protects Mages

On the "Game and battle" tab, there is a parameter called **"Protects mages"**, which is on by default. When enabled, "Maximum" will not place their mage or healer where a single enemy strike would kill them, if a move exists that avoids this. The original AI does not do this; disable the parameter to make "Maximum" play as before.
