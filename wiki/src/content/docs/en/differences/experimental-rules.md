---
title: Experimental Scenario Rules
description: Experimental scenario rule toggles in Razdor and what each one changes.
sidebar:
  order: 2
---

On the scenario screen, next to the "Next" button, there is a "..." button. It opens fifteen independent toggles that change the game rules. The first eight are Razdor's additions; they are not in the original game. The last six ("Bless once a turn", "Poison spares the Undead", "No spell book limit", "Merchant discount from an item", "Rear Service always −30%", "Store in your own buildings") cover rules where Razdor differed from the original: by default the game plays as the original does, and a toggle turns on Razdor's earlier rule.

:::caution
By default, for a new game, all fifteen are turned off. Your selection is remembered: it is saved in the settings when closing the window ("OK", Esc, Enter, or the close button) and upon starting a game, so when you next open the list of new games, the toggles will be as you left them, and will remain so after restarting the game. In the scenario list, an orange "Experimental rules enabled" label indicates that a toggle is on. Returning from the hero selection window preserves it. For an already started game, toggles cannot be changed through this menu.
:::

![Scenario Selection](../../../../assets/shots/scenarios.webp)

## Toggles

* **All special bonuses simultaneously:** the fighter retains their type abilities and the abilities of equipped items. Identical abilities do not stack: a duplicate copy does not enhance the effect.
* **Six equipment slots:** the fighter has six item slots instead of four (a third pair is added).
* **No item limits:** any wearable item can be worn by any living fighter while they have a free slot. The class (shield, melee and ranged weapon, staff), magic school and holy item restrictions are lifted, as are the one-weapon rule, the one-item-per-kind rule and the crown restriction (only the hero and noble warriors). Anyone can wear the crown. Potions and other goods still cannot be worn, and the dead wear nothing.
* **No more than two potions per character:** between army battles, you can drink no more than two potions (including healing and resurrection potions).
* **Free units:** the hero's army asks for no daily wages (neither gold nor mana). Hiring costs as usual.
* **Automatic promotion:** as soon as a fighter of the hero's army can be promoted, it is promoted on its own, along a random branch of its upgrade tree. The hero is not promoted this way.
* **Units don't die:** fighters of the hero's army killed in a battle come back with 1 hit point after it, if the army survived the battle.
* **Faster AI:** armies think faster on big maps (from 300×300 cells): there they search for a path only within a 64-cell square around themselves, and an army with a single target takes the shortest path to it. They move slightly differently from the original.
* **Bless once a turn:** a fighter receives a blessing at most once a turn, and a blessed ally is healed or blessed only when that does something. Off (as in the original): blessings can be repeated and their effects add up.
* **Poison spares the Undead:** the Undead are not poisoned by any poison. Off (as in the original): poison works on every fighter.
* **No spell book limit:** any spell in the book can be cast. Off (as in the original): only the first 15 spells of the book can be cast; a spell taught by an event past the 15th is known but cannot be cast. In the sanctuary, a book of exactly 15 spells cannot be added to (a bug of the original, kept).
* **Merchant discount from an item:** an item with the Merchant property in the hero's pack also gives the merchant discount. Off (as in the original): only fighters in the army with that property do.
* **Rear Service always −30%:** Rear Service always cuts wages by 30%. Off (as in the original): when the player has no income from towns, the cut is larger, about 70%.
* **Store in your own buildings:** in your castle, fort or town with the hero in it, a "Store" tab shows the loot your garrison could not wear; a click puts the item into the hero's pack. Off (as in the original): no tab, the loot stays in the building.
* **Death curse on counter blows:** when a counter blow or a preventive strike kills a fighter with a death curse (or a Ghost under its condition, as in a normal kill), the fighter who struck back dies too, and its remaining hit points count as its side's losses. Off (as in the original): a counter blow does not work like a normal kill, and the striker survives.

The "Razdor's rules" row above the toggles turns on at once all of Razdor's former rules and its fixes (the ones listed above, "Death curse on counter blows" included) and leaves the other toggles as they are. The row shows on when all of them are on; press it again to turn them all off. Afterwards each toggle can be switched on or off on its own.

On a phone, when the window does not fit, the list of toggles scrolls.

:::tip
The item and potion rules apply to all armies: your troops, opponents, and garrisons. "Free units", "Automatic promotion" and "Units don't die" apply to the hero's army only.
:::

## Warrior Participation in Battles

In the "Battle and army..." group of the settings window, there is a parameter called **"Disable fighters"**, which is off by default.

When it is enabled, select a living non-hero in the army window and tick the **"Does not participate in battle"** check box above the "Don't remind" line (top right of the army window). After it is ticked or unticked the warrior is deselected. To return a warrior to battle, untick it.

* The marked warrior remains in the squad, occupies their cell in the formation, and receives wages.
* They do not participate in battle, do not gain experience, and are not counted among the battle participants.
* A hero cannot be excluded.

## Army Deployment and Pathfinding

In the settings groups, there are two additional Razdor parameters:

* **"Preserve formation"** (on by default): after a battle, all warriors, including those who perished, return to the cells they occupied before the battle. If off — warriors remain in the cells where they ended the battle, as in the original.

## Quick Actions with Shift

* **Moving a warrior:** `Shift` + click on a garrison or squad card moves the warrior to an empty slot in another formation.
* **Equip item:** `Shift` + click on an item in the backpack equips it to the hero (or drinks a potion).
* **Unequip item:** `Shift` + click on an equipped item returns it to the backpack.

## "Maximum" Protects Mages

In the "Battle and army..." group, there is a parameter called **"Protects mages"**, which is on by default. When enabled, "Maximum" will not place their mage or healer where a single enemy strike would kill them, if a move exists that avoids this. The original AI does not do this; disable the parameter to make "Maximum" play as before.
