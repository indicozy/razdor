# Experimental scenario rules

The `...` button beside Next on the scenario screen opens five independent switches.
Only the duplicate-item restriction is on by default for new games (the switch
«Разрешить надевать одинаковые предметы» is off). Choices reset when
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
  the equipment slots: with either switch a unit has six cells (the four classic ones plus
  a third pair), otherwise four. Class, magic school, holy-item, weapon and all other
  equipment restrictions remain in force. Each switch is independent of bonus stacking.
- **Allow duplicate items** (default off): with it off, compare item IDs per character, not
  item categories. Different rings/amulets are permitted when their corresponding switch is on.
  Repeated copies of one item are refused. Turning it on lifts only that refusal. A weapon is
  still one per unit: the hint line «Оружие у бойца всегда одно.» says so in the menu.
  Old saves keep their previous rules: `RulesWire` maps `no_duplicate_items` to the inverse.
- **At most two potions per character:** two successful drinks between army battles,
  including instant healing and revival potions. Failed attempts consume neither an item
  nor a count. The counter survives saving, loading and campaign carry-over, and resets
  alongside the existing potion block after a battle.

Rules apply to all armies, including opponents, garrisons and subsequently recruited units.
They live in an isolated per-playthrough Content copy so menu choices never mutate an
installation's shared definitions. `Game.experimental` is the persisted source of truth;
loading rebuilds the runtime Content copy from this field before setting up the AI.

A save writes `allow_duplicate_items` and the inverse `no_duplicate_items` for older builds, which
read the restriction from that key; format 12 is unchanged by the switch.

Save format 11 stores the rules in Game and the campaign hand-over record, potion counters
and noncombatant marks in units. Missing rules in older saves use false values. Manual saves, autosaves, restarting the same scenario
and advancing to the next campaign map preserve the rules. Older Razdor builds reject
format 11 rather than silently dropping the experimental rules.

Save format 12 stores each unit's item cells as six (`items::MAX_SLOTS`); a format 11 save's
four load with two empty cells added (`items::deserialize_cells`). Which cells the rules use
is `items::slot_count(content)`: every search, equip and count reads only those, so the classic
rules never see cells 4 and 5.

In battle, all active abilities survive fighter creation. FateGift consumes only itself
when triggered; Neutralize removes all current abilities. Existing phase order governs
combinations (for example, a fatal hit can trigger FateGift before Neutralize removes the
remaining abilities). Abilities still retain their attack-type and other applicability
conditions; enabling them does not make every ability trigger on every action.

The shared unit panel lists the unit's description first, always whole, and under it the active
abilities in a bounded region of eight rows. Only that region scrolls, using the wheel over it or
its scrollbar. Abilities carry no source line.
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

Route avoidance is Razdor's advanced setting «Обход армий на пути» (`Settings::avoid_armies`,
`Game::avoid_armies`), off by default. Off, the route is the original's: stationary guards and
armies with a meeting waiting next to the hero close their cells, and other armies are walked
through and met on the step. On, every army's cell closes whatever its allegiance or patrol state;
the route is checked before each hero step and after the world advances, and a blocking army causes
a route search to the same destination. If no bypass exists, the planner falls back to a route that
walks up to the first army on the way: an enemy starts combat, a friendly army is met, and a
friendly army clicked on purpose is fought unless `friends_let_pass` lets him by. Clicking an army
directly still selects it as the intentional target, including pursuit. These are convenience
changes rather than original-game behavior, and they apply only while the option is on.

Holding either Shift key while clicking a garrison/squad card transfers the unit to the
other formation's first available place. Existing restrictions on heroes, named units,
dead units and capacity still apply; retrieving an unpaid garrison member pays the normal
price immediately. Shift-clicking a backpack item equips it on the hero, or drinks it if
it is a potion. Shift-clicking worn gear returns it to the pack. Failed quick actions
keep the item and show the existing refusal. All equipment rules and potion limits apply.
