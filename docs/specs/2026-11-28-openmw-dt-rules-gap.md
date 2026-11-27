# OpenMW world: gap against DT's rules (inventory, 2026-11-28)

Why: the owner wants the OpenMW world to replay every DT rule that a map or mod can see, not only
battle ("all other rules like map rules as well"), so that any community map plays. This file lists
what DT does, what the port does today, and what is missing. It is an inventory, not a design: the
battle is M8 and M9 of `2026-11-27-openmw-world.md`, and is only pointed at here.

Sources. DT's rules: `docs/reference/original-mechanics/` (`world`, `economy`, `ai`, `events`,
`experience`, `magic-items`, `community-patches`, `evolution-mod`, `battle`). The port: the spec above
(§3.x, M4 to M9 notes), `crates/openmw-world/README.md`, and `crates/openmw-world/lua/*.lua` (read
2026-11-28, branch `openmw-2026-11-27`).

How to read the tables.
- "Map-visible?": yes when a map, a mod's ini data or an event script can observe the rule (it changes
  a flag, a gold or mana number, an army's place, a building's owner or a condition an event tests).
  A rule only a player sees on screen is "no".
- "OpenMW now": the first word is `done`, `partial` or `missing`, then the reference. `done` means the
  rule is played as DT plays it, as far as the Lua code and the spec say; I did not run the game.
- "Real time": how the rule meets OpenMW's clock (see "Time model"). `-` means no adaptation needed.
- Size: S under about a day of work, M a few days, L a week or more. All sizes are guesses.
- Every unmarked statement about DT is from the cited spec; every statement about the port is from the
  Lua file or spec named. Guesses are marked **[guess]**.

Two facts the whole file rests on.
1. **The data is not exported.** JSON v1 (`src/dt/dtm_json.rs`, read by `crates/openmw-world/src/input.rs`)
   gives a building only its index, faction, events, footprint and kind; an army no gold, income,
   speed, target model, AI flags or units' paid state; the hero no nature or level. The economy, the
   capture rules, the strategic AI and hiring all need a JSON v2 (building: owner, attitudes, income
   and max stocks, barracks slots, market goods, spells, garrison, treasure; army: the AI bytes of
   ai.md §1). This is the first blocker for most rows below.
2. **Armies exist only as actors in loaded cells.** Patrols run in a local script
   (`army.lua`), positions are read from actors (`global.lua` `check_meetings`), and an unloaded
   cell's army does nothing (M4 notes). DT's AI runs the whole map all the time (ai.md §2). Any
   army rule that acts away from the player needs a strategic layer that holds each army's tile
   and moves it by the clock, with actors only shown near the player. See "Time model".

## 1. Movement and time

| DT rule | Map-visible? | OpenMW now | Real time | Size |
|---|---|---|---|---|
| Hero step costs cost-of-cell × speed minutes, ×1.5 diagonal; speeds knight 5, archmage 5, ranger 4 (world.md §2.1) | yes (windows counted in days, event delays) | missing: the player walks at engine speed, terrain cost is not read; the clock is real time (spec §3.11) | Time model, option B or C | M |
| Time passes only while the hero walks, waits or casts (world.md §6.4) | yes | partial: owner chose real time (Q2); the world pauses while an event window is open (README "Pause") | decided deviation; keep | - |
| Waits of 1 h, 4 h and the F4 endless wait; a fired event ends the wait (world.md §6.1) | yes (events fire on ticks, noon ends a wait) | missing: no wait command | a wait key that raises the time scale for the wait **[guess]**; real time already ticks the scans | S |
| Calendar: start minute + 1, day shown 0-based (world.md §6.3) | yes (event `start_time`, time windows) | partial: DT minutes = `info.start_time` + game seconds / 60 (`global.lua` clock); the +1 is not checked | `-` | S |
| Event scan on each step, each 30-minute tick, after a window, on contact (events.md §1) | yes | done: step, 30 DT minutes, meeting, window close (spec §3.11); not done: scan after a building window closes (no building window) | each 30 DT minutes of game time | - |
| Event point under the hero: one cell; an attack scan reads no point (events.md §1) | yes | partial: point offered on its tile (§3.10); the "attack reads no point" rule is not modelled | `-` | S |
| 8-neighbour grid, LAND/MIXED/SHIP cost maps, 0 = closed (world.md §1.1) | yes (rivers, thickets, bridges gate quests) | partial: the guard returns the player from closed tiles and water (spec §3.5, `player.lua`); the costs themselves are not used | per-tile speed, Time model | M |
| Hero's planner and the click mask (world.md §1.2-1.3) | no | n/a: OpenMW walks freely; replaced by the guard | - | - |
| Footprints: stored cell is bottom-right, extra row above when wider than tall (world.md §7.1) | yes (building tests, event zones) | done: boxes and zones from the footprint (§3.7) | - | - |
| Entering a building: second footprint cell or end of walk; obelisk enters silently; a bridge is never entered (world.md §7.2) | yes (local events) | partial: stepping into a box runs the scan (Q6); the two-cell rule, obelisk and bridge cases are not checked | - | S |
| Hero start on the preset cell; start building and class buildings owned (world.md §7.1) | yes | partial: start tile done (§3.9); the start building is owned (`events.lua` line 139); the class-flagged buildings (byte 353+class) are not exported | - | S |
| Distance = max + min/2 (octile) used by contact and AI (world.md §4.1) | yes | partial: contact is Euclidean within 1 tile (`MEET_UNITS`, `global.lua`) | `-` | S |
| Hero class sets sight, speed, cast divisor; events cannot change them (Community: speed byte) (world.md §2.1) | yes | partial: `new_hero_class` stores only the unit id (README "Pause"); class speed and the cast divisor are not used | with step time | S |

Counts (done / partial / missing): 2 / 8 / 2 (13 rows, plus one n/a).

## 2. Contact and engagement (battle is M8 and M9)

| DT rule | Map-visible? | OpenMW now | Real time | Size |
|---|---|---|---|---|
| Hero's step onto a cell: any army on open ground is engaged, friend or foe, except town, tavern, church, smithy, shipyard, altar, dungeon or a friendly army inside a non-bridge building (world.md §4.2 item 1) | yes (a won battle sets "beaten") | missing: meeting only fires events (`check_meetings`); M9 plans it | hero tile change, not a poll | M |
| Guard of a village, castle, fort, ruins or bridge: the army standing inside; engaged when its attitude ≤ 0, always on a bridge (world.md §4.2 item 2) | yes | missing (M9 lists it) | as above | M |
| No guard: garrison engaged; an empty garrison means the building is captured; an unguarded village is captured by crossing it, with no window (world.md §4.2 item 3) | yes (building-owner conditions, income) | missing: `s.owner` is set only for the start building | as above | M |
| AI army reaching the hero: contact within 1 cell, relation rule, battle score > 0, talk counters, only at the end of the hero's step (world.md §4.3) | yes (meeting events, battles) | partial: meeting within 1.5 tiles when the leader's cell is loaded; hostile armies fight at fight 100 (spec §3.8); no relation rule, score or talk counter | AI arrival callback (Time model) | L |
| Friendly meeting: talk counter grows by relation+1 per AI step, greets when > 0, then -500 (world.md §4.3) | yes (meet events on friendly armies) | partial: any army in contact starts a meet scan once per approach (`contact[]`); the counter is not modelled | arrival callback | M |
| Event scan first on contact; an event that fires cancels the battle; "no meeting" clears the met army (events.md §9) | yes | done: meet scan, hostility hold while a meet event waits (spec §3.8, M5) | - | - |
| Relation: both ≥ 0 gives (a+b) div 2, a < 0 gives a, else -1; the army's own attitude vs the relation (world.md §4.3, ai.md §3) | yes | partial: hostile iff `info.relations[0][faction-1] < 0` (M4 notes); the two-sided rule is not used | - | S |
| Battle, wrap-up, loot, XP, surrender, turn limit | yes | missing now; M8 (fight) and M9 (rules around it) | frozen world (M8) | L |
| Army moved next to the hero (event), cheapest free of 8 neighbours (world.md §4.4) | yes | partial: first open tile, not the cheapest (README "Pause") | - | S |

Counts: 1 / 4 / 4 (9 rows; the battle row is counted missing).

## 3. Economy: income, villages, towns, shops, hiring, prices, mana

Almost nothing here exists; the events engine keeps `gold` and `mana` as counters, and the start kit
seeds them. Gold is also given as `gold_001` items (spec §3.9) and mana is the player's magicka (M8).

| DT rule | Map-visible? | OpenMW now | Real time | Size |
|---|---|---|---|---|
| Hero's noon at 12:00 (the next day's noon first), pay, autosave, noon report (world.md §6.4, economy.md §1, §9) | yes (gold, wages, desertion, events at the report) | missing | game-clock threshold, report is an event-style window | M |
| Income at noon: base income, castle and fort stocks ×F/100, villages linked to an owned building (economy.md §1 "Noon payment" 1-5) | yes (gold) | missing: no stocks exported or tracked | daily, at noon | M |
| Wages by kind: recruit Round(C/CostRecrutDiv × tier), mercenary, kinds 0 and 3 free; dead not billed; Rear Service; Elemental mana bill (economy.md §1) | yes (gold, unpaid units) | missing | at noon | M |
| Short gold: unpaid units sit out, pick the lowest-wage units, desertion after MaxTimeNotUpkeep (economy.md §1 items 9-10) | yes (army composition, strength) | missing | at noon | M |
| Unpaid units do not fight; attacker's unpaid sit out (economy.md §1 end) | yes | missing (needs M9 fight) | - | S |
| Midnight: markets re-roll, barracks grow, villages and buildings refill by `income × sqrt(1 - stock/max)`, garrisons heal, medics heal 10% (world.md §6.4, economy.md §3) | yes (stocks, shop goods, events' gold tests) | missing | game-clock midnight | M |
| Village entry: capture, offer chooser (innkeeper, priest, blessing, furs, witch) with exact Rand order, or the window that pays all gold and mana (economy.md §3) | yes (gold, mana, items) | missing | on entering, scan first | M |
| Town, tavern, market, church, smithy hall: Info (quests, rumours), Hire, Garrison, Market, Spells tabs (economy.md §7) | yes (hire, goods, spell shop feed gold, units, items, spells) | partial: hall list offers quests and rumours only (M7); no Hire, Market, Garrison or Spells | an OpenMW UI window, no clock | L |
| Price factor by attitude (1.7 to 0.75; own = +3), Merchant bonus, sell price formula, x87 rounding (economy.md §2) | yes (gold) | missing | - | S |
| Market stock: 12 goods, fixed goods by id, restock at midnight, buy to pack, sell list Cost > 1 (economy.md §2) | yes | missing: the JSON has no goods | midnight | M |
| Barracks hire: slots, growth chance `1/(MaxDayCountForNewUnit div max)`, wage kind 1, 12-unit army cap (economy.md §2, §7) | yes (army size, gold) | missing | midnight growth | M |
| Healing and resurrection prices (HealingConst, ResurectConst, ÷F, take no game time) (economy.md §2) | yes | missing | - | S |
| Spell shop: buy if not known, gold ≥ CostGold, book not full at 15 (magic-items.md §2) | yes | missing | - | S |
| Loot after a win: enemy gold / VictoryGoldDiv, wage bill, items, garrison stock + one day's income (economy.md §3) | yes | missing (M9 lists loot) | - | M |
| Mana pool: preset mana, stocks of villages, surrender mana, Elemental payment (economy.md §1, community-patches.md §12) | yes | partial: counter `s.mana`, written to magicka (M8); no income, no payment | - | S |
| Difficulty factor F (100 or 120) on heal, resurrect, sell, castle income (economy.md §0) | yes (gold) | missing | - | S |
| Ranger heals 15% each wounded unit at noon and 20% after the report (economy.md "Player's noon sequence") | yes (unit HP) | missing | at noon | S |

Counts: 0 / 2 / 15 (17 rows).

## 4. Buildings and capture

| DT rule | Map-visible? | OpenMW now | Real time | Size |
|---|---|---|---|---|
| Owner, faction and attitudes per building; capture copies the hero's (world.md §4.2, economy.md §3) | yes (conditions 12, AI hostility) | partial: `s.owner[i]` only for the start building (`events.lua`); faction is the static JSON value; no attitudes | - | M |
| Building types 0-15 and what each opens (economy.md §7) | yes (building type decides tabs, income and guard rules) | partial: the kind drives the mesh, the entry rule and "all events" for villages and shipyards (§3.7, `events.lua`) | - | S |
| Garrison records for towns, castles, forts, ruins; map-load garrison units skip hero types; ruins' treasure and first 5 goods (economy.md §3, §7) | yes | missing: garrisons not spawned (M4 notes) | - | M |
| Garrison fights as an army; won: building captured, hero stays outside (world.md §7.2) | yes | missing (M9) | - | M |
| Own castle or fort Garrison tab: move units between army and garrison, buy garrison units (economy.md §7, ai.md §9.6) | yes | missing | - | L |
| Bridges: always engaged by the guard, never entered; stone and wooden (world.md §4.2) | yes | partial: bridge decks built (§3.7); the guard and engagement rule missing | - | S |
| Altar, dungeon, church, smithy effects (spells, healing, resurrection) (economy.md §7) | yes | missing | - | M |
| Obelisk (types 13-15): local events fire on entering, no window (world.md §7.2) | yes | partial: "stands on an obelisk's tile" triggers the scan (§3.11) | - | - |
| Army "home building"; a won building returns to a respawned army (ai.md §12) | yes | missing: respawn only teleports home (§3.8) | - | S |

Counts: 0 / 4 / 5 (9 rows; the obelisk row counted partial, the rest missing).

## 5. Army strategic AI: targets, hunting, patrols, respawn, garrisons

Today every active army stands or wanders inside its patrol radius (`army.lua`, Wander package or
`RazdorArmyNext`), only while its cell is loaded. None of DT's goal choice exists.

| DT rule | Map-visible? | OpenMW now | Real time | Size |
|---|---|---|---|---|
| Per-army AI bytes: style (feudal, rogue, peasant), target model, aggression, patrol flag and radius, ignored-by-AI, hunts-only-player, no-random-targets, no-socialising, no-interest-in-buildings, garrison level, respawn-whole-army, hire XP (ai.md §1) | yes | partial: patrol flag, radius, home, respawn days and `behaviour` are exported; the rest is not | - | S (data) |
| Step clock: bank minutes, step when bank ≥ cost × speed, ×1.5 diagonal; speed byte, archmage leader -1 (ai.md §2, world.md §2.1) | yes (where and when armies arrive; events test "army at home") | missing: engine walk speed | Time model | L |
| Stationary guards (radius 0): no steps, no noon, only defend (ai.md §2) | yes | done: radius 0 stands (§3.8) | - | - |
| Patrol box = home ± radius clamped to the map; wander points; idle plans (ai.md §1-2) | yes | partial: random open tile in the radius (`global.lua` patrol destination); not DT's four wander points, not clamped to a path | per arrival | S |
| Army score by simulated battle; building scores; goal choice with seeds, obstacles, repulsion, flood and path (ai.md §4, §6, §7) | yes (armies go for castles, villages, the hero; maps rely on it) | missing | per arrival | L |
| Hunting the hero (own-army target models, AIDistance0..2, AIGetPathDistance, attack if score > 0) (ai.md §7.1, §8.1) | yes | missing: a hostile army in loaded cells attacks by fight 100 (OpenMW AI), not by DT's score | per arrival | L |
| Meeting other armies, AI against AI battles, loot pool and item handout (ai.md §8, §10) | yes (armies get beaten, buildings change hands) | missing | per arrival, off-screen sims | L |
| Arrival in a building: assault and capture, village gold, shopping, healing, hiring, garrison buying (ai.md §9) | yes (owners, gold, army sizes) | missing | per arrival | L |
| AI noon and midnight (income, wages, stock collection, medic heal 10%) (ai.md §14) | yes | missing | lazy at the first arrival after noon | M |
| Respawn: after `respawn_days`, at home or the first owned town, castle or fort; leader only unless the whole-army flag; beaten by AI = whole army (ai.md §12) | yes ("army active", "beaten" conditions) | partial: home respawn after N days with the whole army (`global.lua` line 468); no owner rules, no leader-only, no flag, no gold top-up | game clock days | S |
| Deactivate vs destroyed: a deactivated army never respawns; activation clears the beaten mark (ai.md §12, events.md §7.2) | yes | partial: activate and deactivate exist (M5); the destroyed-vs-deactivated difference and the cleared beaten mark are not checked | - | S |
| XP and promotion of AI units, hire XP (experience.md §4, §5; ai.md §11) | yes (army strength) | missing | with battles | M |
| Attitudes and factions never change except by event opcodes and capture (ai.md §15) | yes | partial: attitude read once at spawn; `start_battle_with` marks an army hostile; event opcode 10 missing | - | S |
| AI ships (ai.md §13) | yes | see section 9 | - | - |

Counts: 1 / 5 / 7 (13 rows, plus one pointer to section 9).

## 6. Hero: levels, classes, skills, artifacts, spells

| DT rule | Map-visible? | OpenMW now | Real time | Size |
|---|---|---|---|---|
| Unit XP table: `need(L) = round(Start × (mult/100)^L)`, several levels per gain (experience.md §2) | yes (level condition, strength) | missing (M9 lists XP shares) | - | S |
| Battle XP pool, shares, hero modifier, difficulty, 5256 cap, surrender gives mana (experience.md §3) | yes | missing (M9) | - | M |
| Event XP to the hero only, as written; hero level condition is 0-based (experience.md §5) | yes | partial: `experience` and `level` fields are read in `events.lua`; level is never raised, so level tests read the start value | - | S |
| Promotion: player free choice at level ≥ 1; AI's random pick; items stay worn (experience.md §4) | yes | missing | - | M |
| Unit strength (tactical cost) from stats × cost multiplier; side strength; weakest unit dismissed when an army has 12 (experience.md §1, events.md §7.1) | yes (strength condition, units add) | partial: strength = sum of price (units JSON `cost`) or 50, not the tactical cost (M7 notes); the 12-unit cap and weakest-dismissed rule are not checked | - | M |
| Hero is the first unit, alive or not; never promoted; never deserts (experience.md §6, economy.md §1) | yes | partial: party is a Lua list of bodies; the hero is the player, not a unit record (§3.9) | - | M |
| Hero classes (knight, archmage, ranger): start kit, speed, sight, cast divisor, ranger healing, 6 start spells (world.md §2.1, magic-items.md §2) | yes | partial: start kit seeded (M7); class event stores a unit id only | - | S |
| Artifact pack: 256 slots, holes, add to first empty, full pack loses it, remove from pack then worn (magic-items.md §5.1) | yes (artifact conditions) | partial: tracked list, add and remove played (README "Pause"); no 256 cap, no worn tracking, codes 2-5 and 0 fail | - | S |
| Equip rules: slots, wear test, who may wear what (magic-items.md §5.2-5.3) | yes (artifact effects, named squads) | missing: no equip screen | an inventory window | L |
| Item effects: stat rebuild order and rounding, potions, durations (magic-items.md §6, §7) | yes (battle numbers, events that test stats) | missing | - | L |
| Items and the AI: pack of 12, best-item handout (magic-items.md §8, ai.md §10.1) | yes | missing | - | M |
| Spell book: 256 entries, magic window shows 15, events can push past 15 (magic-items.md §2) | yes | partial: spells learned and added to OpenMW's magic menu (M8); no 15 limit | - | S |
| World casting: cost mana, wait TimeCast×2 ticks (half for archmage, ×0.8 Caster), world runs and scans during the wait, an event or an enemy cancels it (magic-items.md §3.2-3.3) | yes (timing, mana) | partial: M8 casts in real time through OpenMW's magic menu; no cast wait, no cancel rules | cast time as game minutes | M |
| Spell effect on an army: per-unit stat changes, durations 10×/5× for event casts (magic-items.md §4, §3.6) | yes | partial: M8 maps effects to OpenMW magic effects; the unit-level effects of §4 are approximated | - | M |
| Event-cast spell hits the player's own army, no mana (magic-items.md §3.6) | yes | partial: `cast_spell` applies the spell to the player (M8); to the party is not stated | - | S |
| Spell, item and unit data from the install and the mod (magic-items.md §1) | yes | partial: units and spells JSON (v2, v3 planned); the items JSON does not exist | - | M |

Counts: 0 / 10 / 6 (16 rows).

## 7. Events and quests

M5 and M7 cover the engine. The table lists what the events spec has that the port does not play.

| DT rule | Map-visible? | OpenMW now | Real time | Size |
|---|---|---|---|---|
| Scan order, once, guard, window, class, flags substring rule, chain, relative event, quest, rumour, hall (events.md §2-§10) | yes | done (spec §3.11, M5, M7; `run.sh --expect`) | game minutes | - |
| Conditions 1-8, 10, 12-15: defeated, happened yes/no, not happened, beaten, meet, army active/inactive/at home, buildings, named squads, artifacts, flag test (events.md §4) | yes | done, with the partials: named squads read the party's names only; artifacts condition codes 0 and 2-5 fail (README) | - | S |
| Condition 9: hero at exactly 1 HP (events.md §4) | yes (one shipped map chains defeat from it) | missing: no `hero_one_hp` in `events.lua` | - | S |
| Condition 11 numerics: level, gold, mana, squad, strength, "current stats" gate, patrol-delta interplay (events.md §4) | yes | partial: all read; level never rises, strength approximated (M7) | - | S |
| Results: message, gold, mana, XP, units add/remove (+named), artifacts, spells, activate, deactivate, patrol, show army, move to hero, battle, class, chain, relative, quest, no meeting (events.md §7) | yes | done or partial as the README lists; `show_army` activates instead of scrolling, `start_battle_with` marks hostile and does not fight (M9) | - | M |
| Units added: the 12-unit cap dismisses the weakest, joins at level 1, wage kind 3; "taken from an army" moves the whole record; "given to an army" (events.md §7.1) | yes | partial: add and remove played; cap, weakest rule, take-from and give-to armies missing (`units_from_army`, `removed_units_to_army` not in `events.lua`) | - | M |
| Event delay: the hero waits N hours, a chain behind a delay is dropped (events.md §8) | yes (no shipped event has one) | missing: logged only (README) | game minutes | S |
| Flag counters `+X^` and `-X^` (events.md §5) | yes (no shipped map uses them) | missing: logged only (`events.lua` line 180) | - | S |
| Engine flags `Sea` and `EnterShipyard` (events.md §5, world.md §8) | yes (maps test them) | missing: no sea state without ships | with ships | S |
| Lantern results: scroll and reveal (events.md §7.2) | no | decided: not lit (Q7) | - | - |
| Spare slots: battle report, noon report, village offer, garrison purchase run through the dialog code (events.md §14) | yes | missing | with economy | M |
| Quests: journal entry, completed entry removed; rumours (events.md §10) | yes | done: journal INFO 10 and 100, hall buttons (M5, M7) | - | - |
| Victory and defeat: window, hand-over to the next map, flags carry over, defeat by losing the army does not fire the defeat event (events.md §11, §12) | yes | partial: end window and Quit (§3.12); next map is named, not loaded (Q8); carry-over of gold, mana, book, pack, army, flags not done | - | L (campaign) |
| Tutorial hooks, `end_tutorial` title (events.md §13) | no | partial: the title test passes (`events.lua` 159); no ini write | - | - |
| Community opcodes 1-22 in opcode mode ("no meeting" set and patrol value non-zero) (events.md §15, community-patches.md §12) | yes (community maps use them; "no shipped map" refers to the base maps) | missing: the exporter does not name them and `events.lua` ignores them; the patrol value would be misread as a patrol delta | - | L |
| Opcode effects by group: byte pokes 1-5, items 6, type 7, speed/faction/attitude 8-10, spells 11, 14, 16, name 12, XP 13, next map 15, figure 17, random flag 18, army cell 19, hero teleport 20, named-squad class 21 | yes | missing | - | M each group |

Counts: 4 / 4 / 7 (15 rows, plus one decided deviation).

## 8. Victory and defeat

| DT rule | Map-visible? | OpenMW now | Real time | Size |
|---|---|---|---|---|
| Victory and defeat events end the map; no chain after them (events.md §11) | yes | done (§3.12) | - | - |
| Defeat by losing every unit or the hero (does not fire the map's defeat event) (events.md §11) | yes | partial: the player's death is OpenMW's own screen; party wipe is not treated as defeat (M9) | - | S |
| Hero at 1 HP when he falls while units survive (battle.md hero survival) | yes | missing (M9) | - | S |
| Next map load, carry-over by header bytes 0x110-0x116 (gold, mana, fame, level, worn slots, pack, army) (events.md §12) | yes | missing (Q8: one map per run) | - | L |

Counts: 1 / 1 / 2.

## 9. Ships

| DT rule | Map-visible? | OpenMW now | Real time | Size |
|---|---|---|---|---|
| Shipyard window buys a ship (`ShipCost`); one ship; planner switches to MIXED (world.md §8) | yes (`Sea` flag, water routes) | missing: ships "come later" (Q4) | - | M |
| At sea: water cheap, land ×5, bridges closed; landing parks the ship on the water cell (world.md §8) | yes | missing: the guard forbids water for the player | - | L |
| Ship armies: an army on water is a ship army for good; SHIP cost map; targets only ships, the player at sea, buildings (ai.md §13) | yes | partial: ship armies float on the water at z=0 and stand (§3.8); the exporter writes `patrols = false` for them (M7 notes) | per arrival | M |
| Ship army activate / deactivate places or removes the hero's ship (world.md §8) | yes | missing | - | S |
| Shipyard: landing in it sets the ship's home; `EnterShipyard` flag (world.md §8) | yes | missing | - | S |

Counts: 0 / 1 / 4.

## 10. Community patch and Evolution mod extensions

| DT rule | Map-visible? | OpenMW now | Real time | Size |
|---|---|---|---|---|
| Wide row formation, bonus tokens (52), Splash, Stun, FirstShot, Bleed, Flying, NoHeal, EternalGift, PreventiveStrike, ManaDrain, Evasion (community-patches.md §2-§11) | yes (unit data in mods) | missing (battle; the real-time fight cannot reproduce turns, M9 decides) | - | L |
| Elementals paid in mana; AddPayment (Rear Service); Caster ×0.8 cast time (community-patches.md §12) | yes | missing (economy) | - | S |
| Experience: victory award with HeroExpirienceModificator, difficulty, correction, 5256 cap (community-patches.md §12) | yes | missing (M9) | - | S |
| Gold and mana conditions use signed comparisons, a mana condition exists (community-patches.md §12) | yes | done: sign handled by the v1 conditions (M5) | - | - |
| Event opcodes (see section 7) | yes | missing | - | L |
| F-key endless wait, marker colours, AnimationSpeed, info-card lines | no | n/a: interface only | - | - |
| Load-time tripwires (artifact with `d-Manevres` 4 or `Cost` 32000, unit `LevelMultipler` 200 or 160) (community-patches.md §12) | yes (a map's data can hit them; they hang or crash the original) | missing: the exporter should refuse or warn | - | S |
| Evolution mod: older rule set (Stun 25%, Fortify +20%, Splash 70/30, no Evasion, 16 opcodes, 4 option rows, 161 units, 192 items, 40 spells, `BattleEndTurn` 20) (evolution-mod.md §1-§3) | yes | missing: the port has no rule set choice; the units JSON reads whichever install it is given | rule set per install **[guess]**: a `rules` block in the options JSON | M |
| Mana payment for Elemental units, events that change events, alliances and army speed in the mod's data (evolution-mod.md §3) | yes | missing (see opcodes) | - | S |

Counts: 1 / 0 / 7 (8 rows, plus one interface-only n/a).

## Time model

Facts. DT time is in centi-minutes and only advances while the hero walks, waits or casts. A hero step
costs cost × speed minutes (25 on grass for speed 5, 20 for a ranger), a day is 1440 minutes (about 58
grass steps), noon is 12:00 and midnight 00:00 (world.md §2.1, §6). AI armies bank minutes and take a step
when the bank covers their cost; a step ends in an "arrival" where all AI rules run (ai.md §2). The port
runs the OpenMW game clock at scale 120 (`setGameTimeScale`, `global.lua`), so a DT day is 24 game hours,
12 real minutes (spec §3.11, Q2). The M8 spell durations use "timescale 30" (spec M8): this disagrees with
the runtime's 120; check which is meant **[guess: a leftover of OpenMW's default]**.

Proposal, to be decided by the owner (all numbers are guesses to be measured):
- **One clock.** The OpenMW game clock stays the DT clock (DT minutes = game seconds / 60 + start). Noon,
  midnight, event windows, delays, respawn and garrison healing are thresholds on it. They run in the global
  script on `onUpdate`, scan like the 30-minute event scan does today, and pause with the world while a
  window is open.
- **Hero steps.** Today the hero crosses a tile in a few real seconds (a guess of about 6 game minutes at
  scale 120) and terrain is free. Option A: keep it, accept that a window counted in days is about 4 times
  as generous as in DT. Option B: charge DT's cost on each tile change (needs a way to add game time from
  Lua; not found in the Lua API notes, **[guess]** it may need a faster scale or a time-skip). Option C
  (recommended **[guess]**): slow the player by the tile's cost (grass 25 is the base; road faster, swamp
  and hills slower) with the speed attribute, and pick the time scale so a grass tile equals 25 game minutes
  at the resulting walking speed (about 500 for a 3-second tile). Then days, delays and windows match DT.
  The class speeds (5, 5, 4) scale the same way.
- **AI steps.** Keep a strategic layer in the global script: per army a tile, a bank of game minutes and a
  path. Each tick the bank adds the elapsed game minutes; a step is taken when the bank covers cost ×
  speed (ai.md §2). The arrival runs DT's arrival rules (planning, contact test, building arrival, noon).
  An army's actors are teleported to the layer's tile and walk to the next tile when their cell is loaded
  (a Travel package to the tile centre, timed to the step), and are removed or parked when it is not.
  Positions are then the same loaded or not.
- **Days.** Noon for the hero: first at the next day's 12:00, then each 24 hours after it is paid. Noon for
  an AI army: lazily at its first arrival after 12:00 (ai.md §14). Midnight: once per 24 game hours, in the
  order of world.md §6.4 (markets, barracks, stocks, garrison heal, medic heal, AI rescore). All use the
  game clock, so a long wait in the world (a battle freezes it, M8) skips nothing: a threshold passed while
  frozen fires on resume.
- **Waits and casts.** A wait key advances 1 or 4 DT hours by raising the time scale briefly **[guess]**
  while the scans keep running. A spell's cast time is TimeCast hours of game time (half for the archmage).
- **Battle.** The world is frozen during the fight (spec M8) and a battle takes no DT time in DT either; the
  wrap-up runs on the clock's resume (M9).
- Open risks **[guess]**: a 500 scale makes OpenMW's own timed effects (magic durations, fatigue, day-night
  light) very fast, so those should be set or neutralised; unloaded-cell armies need the strategic layer
  before any AI rule is added.

## Proposed order (M10 and later)

Most map-breaking first: a gap that makes a community map unplayable, softlocked or impossible to win comes
before one that makes it easier or different.

| | work | why this position | rows covered | size |
|---|---|---|---|---|
| **M10** | JSON v2 and the data bridge: building economy, owner, attitudes, garrisons, barracks, goods, spells; army AI bytes; items JSON; event opcode fields; the load-time tripwire check | every later rule reads it; nothing in sections 3 to 5 can start without it | 1, §3, §4, §5, §7 | L |
| **M11** | Event completeness: condition 9, `units_from_army`, `removed_units_to_army`, delay, counters, the 12-unit cap, community opcodes 1-22, Evolution's 16-opcode set | a map that needs one of these cannot be won or its script stalls; no data beyond M10's | §7, §10 | L |
| **M12** | Contact and capture: the hero-step engagement rule (§4.2 of world.md), guards, garrisons, village and empty-castle capture, owner and attitude copy, bridges | owner conditions and capture-gated quests are the most common unlock; also needs M9's battle | §2, §4 | M |
| **M13** | The time model: strategic army layer, step clock, arrival callback, terrain-costed hero speed, wait key, noon and midnight thresholds | patrols, hunting, respawn, income and wages all hang on it; today armies in unloaded cells do nothing | §1, §5 | L |
| **M14** | Economy: noon (income, wages, desertion, Rear Service), midnight (stocks, markets, barracks, garrisons, healing), difficulty factor | a map whose gold or mana is a gate (gold condition, rumour cost, hire) is unplayable without it | §3 | L |
| **M15** | Town and village windows: hire, market, healing, resurrection, spells, garrison tab, village offers, price factor, loot | the player's only way to turn gold into units and items | §3, §4 | L |
| **M16** | Hero progression: XP table, promotion, strength (tactical cost), level condition, items equip and the stat rebuild, potions | level and artifact conditions, and battle numbers, read these | §6 | L |
| **M17** | Strategic AI: army scores, goals, hunting, AI-vs-AI battles, building arrival, hiring, AI XP, respawn rules | maps still play without it (armies stay or patrol) but they do not take buildings or hunt | §5 | L |
| **M18** | Ships and sea: shipyard, MIXED movement, landing, `Sea` and `EnterShipyard`, ship armies' AI, ship events | only maps with water routes need it; the guard keeps the player on land until then | §9 | L |
| **M19** | Campaign carry-over and next-map load; Evolution's rule set choice | one map per run is accepted (Q8) | §7, §8, §10 | M |

Notes on the order.
- M8 and M9 (battle and its rules) come before M12: capture and garrisons need a won fight.
- M10 can be cut in two (economy data first, AI data second) if the first M14 map needs only gold.
- M11 can run in parallel with M10 on the Lua side once the opcode fields are in the JSON.
- The community smoke map list (`community_maps_smoke`, CLAUDE.md) is the check for each milestone:
  a port milestone is done when every map of those installs loads, plays 30 DT days and fires the same
  event order as Razdor's own engine **[guess: needs a Lua-side replay harness; today `run.sh --expect`
  covers one synthetic scenario]**.
