# Experimental scenario rules

The `...` button beside Next on the scenario screen opens ten independent switches.
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
- **Six equipment cells** («Шесть ячеек снаряжения», default off): a unit has six item cells
  (the four classic ones plus a third pair), otherwise four. The count is `items::slot_count`
  and follows only this switch. It is fixed for a playthrough; the menu edits a new game's
  rules only, so no unit can wear items in cells 5–6 while the switch is off.
- **Multiple rings** and **multiple amulets:** allow those item types to occupy any of
  the equipment cells the unit has (four, or six with the six-cell switch). Class, magic
  school, holy-item, weapon and all other equipment restrictions remain in force. Each switch
  is independent of bonus stacking. Neither changes the cell count.
- **Allow duplicate items** (default off): with it off, compare item IDs per character, not
  item categories. Different rings/amulets are permitted when their corresponding switch is on.
  Repeated copies of one item are refused. Turning it on lifts only that refusal. A weapon is
  still one per unit unless the multiple-weapons switch is on: the hint line «Оружие у бойца всегда одно.» says so in the menu, «Можно надеть несколько оружий.» when it is on.
  Old saves keep their previous rules: `RulesWire` maps `no_duplicate_items` to the inverse.
A save without `six_cells` (before the switch) gets `six_cells = multiple_rings || multiple_amulets`,
so a playthrough that already had six cells keeps them and no worn item is lost.
- **Allow multiple weapons** («Разрешить несколько оружий», default off): `items::slot_for` skips
  its one-weapon refusal (`SecondWeapon`); the class checks (blow, shot, staff) and one item of
  each type still apply. Every worn weapon's bonuses count like any worn item's: the battle picks
  no «the» weapon (`NoHeal` marks and the cannon sound follow the unit's base attacks, not a
  worn item), so nothing else changes.
- **Free units** («Бесплатные юниты», default off): `Game::unit_wage` returns no wage for the
  player's units, so `bills`, `wage(i)`, `daily_wages`, `daily_mana_wages`, the noon payment, the
  short-of-money check and the cards all read 0. Hiring, healing, resurrection and refunds keep
  their prices (they never read `unit_wage`). Garrisons and AI armies were never billed through it.
- **Automatic promotion** («Автоматическая прокачка», default off): `Game::auto_promote_unit`
  takes every open option (`Unit::promotions`) of a squad member other than the hero at once, to
  an option drawn by the game's RNG (`self.rng`, one draw per promotion, so saves and replays keep
  their stream), and looks again while one is open (at most 8 promotions, a guard against a cyclic
  tree). It runs for every squad member after a battle (`settle_battle`, after the XP and the
  deaths are settled, before the result is reported) and after an event's XP to a player's unit
  (`unit_gains`, script.rs). The «Не напоминать» switch hides only the reminder and does not stop
  it; a corpse is not promoted (it waits until it lives again), and the hero never is. The level-up report still lists the
  level the battle reached. The promotion shows in the army window only (no separate message).
- **Units don't die** («Юниты не умирают», default off): `Battle::player_results` gives a dead
  fighter of the player's 1 HP while the army survives (any living player fighter at the end),
  the same way the hero does; the classic rule leaves it at 0 (`0x4906a0` for the hero). A wiped
  army keeps today's result. The battle has no summoned units: the engine's fighters are the
  squad's living units and the enemy's, and the only non-kept death mark (`kept_death`) belongs to
  AI troops. Corpses from an earlier battle are not fighters, so they stay dead. The revived unit
  keeps its spells and XP; the battle's `lost` count does not include it.
- **At most two potions per character:** two successful drinks between army battles,
  including instant healing and revival potions. Failed attempts consume neither an item
  nor a count. The counter survives saving, loading and campaign carry-over, and resets
  alongside the existing potion block after a battle.

Rules apply to all armies, including opponents, garrisons and subsequently recruited units.
They live in an isolated per-playthrough Content copy so menu choices never mutate an
installation's shared definitions. `Game.experimental` is the persisted source of truth;
loading rebuilds the runtime Content copy from this field before setting up the AI.

A save writes `allow_duplicate_items` and the inverse `no_duplicate_items` for older builds, which
read the restriction from that key; it also writes `six_cells`. Format 12 is unchanged by the switches.

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

For reproducible screenshots, `RAZDOR_SCENE_EXPERIMENTAL=bonuses,rings,amulets,...` enables
rules in snapshot scenes; `RAZDOR_SCENE_RULES=1` opens the options over `scenarios`, and
`RAZDOR_SCENE_BONUS_ITEMS=1` equips available distinct bonus items on the hero in an army scene.

## Map spells reach any army (not a switch)

Razdor's own (the owner's decision, 2026-11-02): a map spell may aim at an army on any explored
cell, the water and the bridges included, from land and back. The original refuses those
(0x4cbf20, magic-items.md §3.5: the cell must be passable on the hero's movement map). The
check is `Game::spell_can_reach`, used by `Game::curse_aim`, `spell_targets` and the
target-mode ring. Only the terrain test is lifted: the cell must be on the map and explored,
and range, ownership, the hero's own cell, buildings and the friends' meeting rule are as before.
There is no option for it.

## Noncombatants

Advanced settings (the regular settings window's More button) has **Unit battle
participation**, off by default. With it enabled, select a living non-hero in the army
window and tick the **Exclude from battles** check box (`Не участвует в боях`) above the «Не напоминать» row, both in the block above the dismiss box (it grows upward when both do not fit). After it is ticked or unticked the unit's selection is cleared.
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

## Formation after battle

«Сохранять расстановку» (`Settings::keep_formation`, `Game::keep_formation`) is Razdor's
deviation, asked for by the owner ("why is the army's layout a bit different at every new fight?"),
on by default, in «Ещё…» beside «Обход армий на пути». The original rewrites the army's formation
after each battle from the cells the battle left (4988c0), after its start fix (4d2141) and its
collapses (489f50): survivors keep the cells they ended on, the rest take free ones. On, every
unit that was in the army before the battle goes back to its cell from before it, the dead too.
A cell that is outside the formation or taken falls back to the original's rule. HP, XP, deaths,
items and the rest are unchanged. Off, the original's write-back. The difftests and replays
build `Game` with the flag off (`Game::new`), so they keep the original's rule.

## Travel and Shift shortcuts

Route avoidance is Razdor's advanced setting «Обход армий на пути» (`Settings::avoid_armies`,
`Game::avoid_armies`), off by default. Off, the route is the original's: stationary guards and
armies with a meeting waiting next to the hero close their cells, and other armies are walked
through and met on the step. On, every army's cell closes whatever its allegiance or patrol state;
the route is checked before each hero step and after the world advances, and a blocking army causes
a route search to the same destination. If no bypass exists, the planner falls back to a route that
walks up to the first army on the way: an enemy starts combat, a friendly army is met, and a
friendly army clicked on purpose is fought even with `friends_let_pass` on (only an unclicked step is let by). Clicking an army
directly still selects it as the intentional target, including pursuit. These are convenience
changes rather than original-game behavior, and they apply only while the option is on.

Holding either Shift key while clicking a garrison/squad card transfers the unit to the
other formation's first available place. Existing restrictions on heroes, named units,
dead units and capacity still apply; retrieving an unpaid garrison member pays the normal
price immediately. Shift-clicking a backpack item equips it on the hero, or drinks it if
it is a potion. Shift-clicking worn gear returns it to the pack. Failed quick actions
keep the item and show the existing refusal. All equipment rules and potion limits apply.

## The rules window (new_game.rs `experimental_menu`)

The choice is remembered (the owner's yes, 2026-11-02): when the window closes (OK, Esc, Enter or its close button) and when a game starts, the ten switches are written to `settings.json` (`language::Settings::experimental_rules`, all off on an older file). `open_list` starts from them, so the scenario list's «Экспериментальные правила включены» label shows a remembered choice too.

Opened from the scenario list's «Экспериментальные правила» button. Its scale is `display::popup_o`
fitted to the screen. Its ten rows and OK are touch targets (40 dp on a phone); its texts are one
px per dp on a phone, grown with the text size (`button_grow`) and never under Normal; the hints
wrap to its width and the window grows with them. The rows take one, two or three columns: the
fewest whose height fits the screen (a phone's landscape takes two or three), and the window takes
the screen's width. The hint lines of the free-units and units-don't-die rules show only while those are on (they say «армии героя»: both rules touch the player's units only). Every label is drawn at the same size; a label too wide for its column wraps to two lines, and every row is then two lines tall. On an upright phone the window is at most 60% of the screen high and has one column: the rows and hints scroll between the title and the OK button (a swipe, the wheel, the bar at the right; `RULES_SCROLL`, kept until the game ends, `RAZDOR_SCENE_SCROLL` for a shot). A row toggles only on a press inside the visible view. Any window that overflows the screen scrolls the same way. Snapshot:
`RAZDOR_SCENE=scenarios RAZDOR_SCENE_RULES=1`, with `RAZDOR_SCENE_EXPERIMENTAL` keys
`bonuses,rings,amulets,potions,cells,weapons,free,auto,undying`; the mobile check's case «rules».

## Battle AI: Caster guard (Максимум)

Advanced settings (the «Ещё…» window) have **«Бережёт магов»**, on by default (the owner shipped it without the poll, 2026-10-31)
(`Settings::max_guard`, saved in the settings file, not in a save; `Game::max_guard` and
`Battle::set_max_guard` are set from it when a battle starts, as `Game::max_ai` is). It is a
gameplay change of the «Максимум» AI; the switch stays so a player can turn it off.
With it on, «Максимум» does not move a mage or healer of its side into a cell where one action
of an opposing unit kills it, while another move leaves none exposed (`battle-ai.md` §8,
`Battle::guard_moves`). Off, «Максимум» plays as before (the `max_bench` baseline).
