# Experimental scenario rules

The `...` button beside Next on the scenario screen opens five independent switches.
Only the duplicate-item restriction is on by default for new games. Choices reset when
starting a new playthrough from the main menu.
Returning from hero selection preserves the selection. The switches cannot be changed
for an existing game through this menu.

- **All special bonuses simultaneously:** retain the unit type's ability and distinct
  equipped-item abilities. Repeated copies of an ability do not multiply its effect.
  PoisonS supersedes Poison, GodStrike supersedes GodAnger, and OldVampirsGist supersedes
  VampirsGist. Other distinct abilities use their existing battle phases and order.
  Stat modifiers from equipment continue using the existing calculation, independently
  of this switch. Negative abilities are retained too.
- **Multiple rings** and **multiple amulets:** allow those item types to occupy any of
  the four existing equipment slots. Class, magic school, holy-item, weapon and all other
  equipment restrictions remain in force. Each switch is independent of bonus stacking.
- **No duplicate items:** compare item IDs per character, not item categories. Different
  rings/amulets are permitted when their corresponding switch is on. Repeated copies of
  one item are refused. Existing old saves retain their previous rules.
- **At most two potions per character:** two successful drinks between army battles,
  including instant healing and revival potions. Failed attempts consume neither an item
  nor a count. The counter survives saving, loading and campaign carry-over, and resets
  alongside the existing potion block after a battle.

Rules apply to all armies, including opponents, garrisons and subsequently recruited units.
They live in an isolated per-playthrough Content copy so menu choices never mutate an
installation's shared definitions. `Game.experimental` is the persisted source of truth;
loading rebuilds the runtime Content copy from this field before setting up the AI.

Save format 11 stores the rules in Game and the campaign hand-over record, potion counters
and noncombatant marks in units. Missing rules in older saves use false values. Manual saves, autosaves, restarting the same scenario
and advancing to the next campaign map preserve the rules. Older Razdor builds reject
format 11 rather than silently dropping the experimental rules.

In battle, all active abilities survive fighter creation. FateGift consumes only itself
when triggered; Neutralize removes all current abilities. Existing phase order governs
combinations (for example, a fatal hit can trigger FateGift before Neutralize removes the
remaining abilities). Abilities still retain their attack-type and other applicability
conditions; enabling them does not make every ability trigger on every action.

The shared unit panel lists active abilities and their sources in a bounded region.
An overflowing list scrolls independently using the wheel over the region or its scrollbar.
Name, stats, equipment slots and the panel's outer bounds remain fixed. Hero class traits
and state lines share the region with unit abilities.

For reproducible screenshots, `RAZDOR_SCENE_EXPERIMENTAL=bonuses,rings,amulets` enables
rules in snapshot scenes; `RAZDOR_SCENE_RULES=1` opens the options over `scenarios`, and
`RAZDOR_SCENE_BONUS_ITEMS=1` equips available distinct bonus items on the hero in an army scene.

## Noncombatants

Advanced settings (the regular settings window's More button) has **Unit battle
participation**, off by default. With it enabled, select a living non-hero in the army
window and use **Exclude from battles** / **Return to battle** beside the dismiss button.
The card and unit panel identify a marked member. The hero cannot be excluded.

Only the player-side fighter list changes, both when attacking and defending. Marked
members remain in the squad, reserve their formation cells, require their usual wages,
retain their HP and remain ordinary living records for quest presence/death conditions.
They are absent from battle XP shares and the battle's starting participant count. Other
world systems and scenario-awarded experience retain their existing rules.

The preference is stored globally in audio.json, while each mark is saved with its unit
and carried to the next map with that unit. Disabling the preference restores everyone's
battle participation without deleting their marks. Marks apply to the hero's army; units
transferred into a world garrison use that garrison's ordinary troop records.

Snapshot scenes can mark a member using `RAZDOR_SCENE_BENCHED=<squad index>`; enable the
global preference in an isolated snapshot save directory to display its controls.

## Travel and Shift shortcuts

Player routes avoid all army cells, regardless of allegiance or patrol state. Remaining
routes are checked before each hero step and after the world advances. A blocking army
causes a route search to the same destination. If no bypass exists, the planner falls
back to a terrain-valid route and the hero approaches the blocking army for contact:
an enemy starts combat and a friendly army is met. Clicking an army directly still
selects it as the intentional target, including pursuit. These travel changes are always
active; they are convenience changes rather than original-game behavior.

Holding either Shift key while clicking a garrison/squad card transfers the unit to the
other formation's first available place. Existing restrictions on heroes, named units,
dead units and capacity still apply; retrieving an unpaid garrison member pays the normal
price immediately. Shift-clicking a backpack item equips it on the hero, or drinks it if
it is a potion. Shift-clicking worn gear returns it to the pack. Failed quick actions
keep the item and show the existing refusal. All equipment rules and potion limits apply.
