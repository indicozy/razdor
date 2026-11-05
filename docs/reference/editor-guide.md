# The Hristos map-editor guide, checked against our specs, the code and the exe

Analysis of 2026-11-05. The guide's claims were checked against `docs/reference/` (the specs),
the code on `main` (`src/`), and the original's exe (the Community Update build, decompiled; the
decompile notes are local). Nothing was built or run. Claims marked **open** are unsettled and are
listed in `docs/OPEN_QUESTIONS.md` (the 2026-11-05 entries). This file is our own summary: it does not
copy the guide's text, and it omits the author's personal and payment details, the feedback form and
all links.

## What the guide is

- **Title:** «Картостроение. Всеобъемлющий гайд» (map building: a comprehensive guide), in Russian.
- **Author:** the handle Hristos. **Edition 1**, dated **2026-06-21**, **263 PDF pages**.
- **Chapters**, one line each (guide page ranges are PDF pages, approximate at the joins):
  1. Editor and map creation: map sizes, terrain and its passability colours, the generator, the basics of the scenario window (pp. 6-16).
  2. Scenario settings, factions and campaigns: hero presets, the alliance matrix, campaign carry-over (pp. 15-24).
  3. Buildings: types, their tabs, markets, barracks, income and maximums, garrisons (pp. 25-46).
  4. Armies: player and AI armies, behaviour styles, targets, respawn, loot, army settings (pp. 46-69).
  5. Events, theory and reference: scan order, conditions, results, flags and counters, pictures, the function table (pp. 70-138).
  6. Event examples: recipes for quests, timers, counters, ambushes, alliance swaps, resurrection (pp. 139-194).
  7. Subtleties and limits: map limits, counters, ghosts, ships, tribute, named characters, playability (pp. 195-224).
  8. Making a good map: landscape recipes, draw order, game design, balance, publication (pp. 225-257).
  9. Known bugs: the guide's own list of 16 bugs (pp. 258-262).
  - Page 263 is the author's final page (thanks, licence, contact, payment, feedback form). It is not analysed.
- **Source:** the post on the Discord Times server of 2026-06-21, with the guide's PDF (edition 1).
  A Google Docs online edition exists and is newer. It is not publicly exportable, so it was not
  checked: its differences from edition 1 are unknown.

## How to read this file

Each chapter has a summary and a table: guide page, the fact, a verdict, and the evidence (exe address,
our spec section, or our code path). The verdicts:

- **confirms**: our spec or code already says the same.
- **guide wrong (exe says X)**: the exe settles it against the guide; Razdor matches the exe unless a gap says otherwise.
- **our doc stale (fixed)**: our spec was wrong, the exe settles it; the spec was corrected on 2026-11-05.
- **new, exe**: not in our docs, and confirmed in the exe.
- **new, unverified**: not in our docs and not checked.
- **open**: unsettled. Listed in `docs/OPEN_QUESTIONS.md`.
- **unclear**: the guide's claim was not traced; no verdict is given.
- **advice**: design advice, not a checkable claim.

Notation: `0x4…` is a virtual address in the Community Update exe (a function entry or a branch, as the
analysis gave it); "@0x…" means "at". `events.md §2` means `docs/reference/original-mechanics/events.md`
section 2. `src/…:N` is a line on `main`. "Razdor gap" items are in the section "Razdor gaps found",
with the IDs used in the tables (E = editor, G = engine, D = deliberate).

## Chapter 1: Editor and map creation (pp. 6-16)

The editor starts a 50×50 water map. Passability colours on the terrain are the guide's main map-making
aid. The guide's terrain legend is correct and answers the open item in `dtm-format.md §13`, which was
stale and is fixed (2026-11-05).

Summary: map sizes, the generator seed and the terrain legend are right. The 400 and 800 size warnings
are unverified, and the editor-only behaviour is not traced.

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 9 | Sizes 50, 100, 200, 400 and 800 are offered | confirms (the first three) | All 15 shipped maps are 50, 100 or 200 square (`dtm-format.md §3`); Razdor offers 50, 100 and 200 (`src/editor/defaults.rs:9`) |
| 9 | 400 and 800 break the game and the editor (minimap, misdrawn buildings) | new, unverified | Not traced |
| 9 | "400×400 is 64 times larger" than 50×50 | confirms (arithmetic) | The shipped maps show the same sizes |
| 10-11 | The generator stores a seed; the "old" and "calculation" checkboxes regenerate on the same seed | confirms the seed | `dtm-format.md §3`, header 0x14; the generator UI is editor-only and new |
| 12 | Five palettes; green best, yellow and orange slow, red impassable; roads drawn green but the fastest | confirms | `world.md §1.1`: road cost 3, trees +4 (yellow), dead trees +6 (orange), thicket class 11 blocked (red) |
| 12 | Normal trees slow the hero; the grove variant is impassable | confirms | `world.md §1.1` (class 11) |
| 12 | Hills cost 2; mountains and rocks are blocked | confirms | `world.md §1.1` |
| 14 | The editor's Save sometimes does nothing; use Save as | new, unverified | Editor-only; not traced |
| 15 | Victory and defeat events end the map at once | confirms | `dtm-format.md §3` (0xD2, 0xD8) |
| 16 | Start dates in the V to XV centuries; dates after 2000 make relative events misbehave | confirms the mechanism | `events.md §2` item 3: the "relative only" start is the year-2000 sentinel (`src/dt/dtm.rs:30`); a later scenario start makes it earlier than now. Shipped starts run from 1142 to 1452 |
| 18-19 | A scenario picture is a LIT of 267×134; the guide gives a BMP-to-LIT conversion route and says other converters write LIT versions the game rejects | size confirms; "rejected" claim open | `dtm-format.md §11`; the picture decoder check is open (OQ "Scenario picture") |

## Chapter 2: Scenario settings, factions and campaigns (pp. 15-24)

Summary: the hero presets, the alliance matrix and the campaign hand-over match our specs. The one
guide claim that the exe contradicts is the carry-over of learned spells: they follow the experience
and level flag, not the pack flag. Our carry-over code already does this.

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 16 | Hero presets (knight, archmage, ranger): gold, artefacts, spells, troops, building; starting battle experience does nothing | confirms | `dtm-format.md §3` (hero preset); `experience.md` (preset: no XP) |
| 6, 43 | The hero's starting troops are paid daily | confirms | `economy.md §1`: starting army is wage kind 1 |
| 16, 43, 48 | The leader's start level is an added level, 0-based | confirms | `dtm-format.md §7` |
| 17 | Four factions; green is the player; relations set the default for every army and building; below 50% they fight and capture; keep the default alliances | confirms | `dtm-format.md §3` (matrix -3..3, faction codes 1-4); `world.md §4.2` (50 or more: no attack) |
| 17, 39 | Alliance gauges show 0, 16, 33, 50, 66, 83, 100 per cent: the seven steps -3..+3; the default matrix is given | confirms the steps | The exe's price steps (`economy.md §2`): 0 per cent is -3, 100 per cent is +3 |
| 17 vs 39 | The screenshot on p.39 shows green to blue as 93 per cent; p.17 shows 83 | unclear (guide inconsistency) | Not explained by the guide |
| 17 | Presets (default, allies, neutral, world war) set the whole matrix | new, editor-only | Not traced |
| 17, 66 | Global alliances are the only way to set the player's relations; custom alliances may crash | new, unverified | The crash claim is also in chapter 9 (9.14) |
| 20 | The first map of a campaign is "first scenario", later ones "normal"; all share the campaign name and name the next map; the campaign list takes the description and picture from the first map | the kind field confirms (`dtm-format.md §3`, 0x10F); the picture rule new, unverified | `dtm-format.md §10` strings 3-4 |
| 21 | Carry-over by header byte: gold (0), mana (1), fame (2, unused), experience and level (3), worn items (4), pack (5), army (6) | confirms | `script.rs:1288-1310`; `saves-data.md §6` (header 0x110..0x116). Byte 1 settles the mana "guess" (see Settled) |
| 21 | Learned spells are carried by the inventory (pack) flag | guide wrong (exe says the experience and level flag) | `saves-data.md §6` and the exe (0x4b5b64): the spell book is restored under the experience and level flag, the same flag that zeroes level and XP. Razdor matches (`script.rs:1155`, `:1298`) |
| 21 | Fame carries over | confirms | No code reads it (`script.rs:1149`) |
| 22 | The next map is looked up by file name in the campaign folder; a missing or misnamed file ends the campaign | confirms the lookup and the end | `next_campaign_map` (`src/ui/screens.rs`); the quote question is open (existing entry, "next-map name in quotes") |
| 22 | Testing a campaign by swapping the next map's file works; each hand-over reads the file as it is then | confirms | `saves-data.md §6` (0x4b5b64 loads the file at each hand-over) |
| 22 | Editing a map does not change a run in progress; a new game is needed | open | The guide says the game keeps a local copy; `saves-data.md §6` says the run re-reads the file by name on restart (OQ "hero's figure", and the campaign entries) |
| 23-24 | The editor saves Rus_Artefacts.New and Rus_Units.New; renaming them replaces the game's data for every map | new, unverified | Our loader reads these files from the install; not checked |

## Chapter 3: Buildings (pp. 25-46)

Summary: the building types, tabs, market rules, barracks, prices and income match our specs, with
a few exceptions the exe settles: potions, castle payouts, ruin treasure, and the "garrison serves
the AI only" checkbox, which the game never reads (gap E3).

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 27 | 15 building types, in the dropdown order and by number | confirms | `dtm-format.md §6`, ids 0-15 |
| 29 | The smithy is a development artefact that can cause bugs | new, unverified | Not traced |
| 29-30 | Town: no fighting inside; hire, heal and resurrect; garrison hidden (AI only); market, spells, rumours | confirms | `src/rules/town.rs:79-112`; the garrison tab only for the player's castles and forts (`town.rs:90`) |
| 29-30 | Castle and fort: hire and heal; garrison for the owner; rumours; passive income; a garrison defence bonus for armies inside | confirms | `economy.md §7` |
| 29-30 | Village: tribute; barracks AI only | confirms | No hire tab for villages (`world.rs:597-604`) |
| 29-30 | Church: hire, heal, resurrect; spells and market; rumours | confirms | `economy.md §2, §7` |
| 29-30 | Altar: spells, hire and heal; no market | confirms; our doc was stale | `economy.md §2` (the market is types 1, 6, 7 only) and §7 (no type test on the hire tab). Our `mechanics.md` said altars hire for the AI only: fixed 2026-11-05 |
| 29-30 | Ruins: an AI-only garrison that is never refilled, hostile; hire and heal; treasure; income that builds the treasure | confirms | `world.md §4.2` step 3; `src/rules/game.rs:2170` |
| 29-30 | Shipyard: ships only. Bridges and obelisk: no window. Dungeon entrance: rumours | confirms | `world.md`, `town.rs` |
| 29 | Capturing a town, castle or fort changes its alliances, which changes market prices | confirms | `world.md §4.2` (capture copies faction and attitudes); `economy.md §2` |
| 26-30 | The editor offers settings per type that the game ignores (for example the altar market) | confirms | `records.md` tab map (dt-editor) and `economy.md §2` |
| 26 | Right-click shows a neutral owner for every type except castle, fort and ruins | new, UI, not traced | |
| 27-28 | Changing a building's type after events refer to it can reset settings or crash the editor | new, unverified | Editor-only |
| 31 | Market: six editor slots for fixed goods, random goods, up to 12 places in total | confirms | `economy.md §2`; records (fixed 6, random 0-12) |
| 31-32 | Fixed goods stay until bought; a bought one frees a place that the next restock fills with a random good in the price range; random goods redrawn at midnight | confirms | `economy.md §2` (market stock) |
| 31-33 | Price range "from/to": 0 to 0 means any price except negative; a range with no matching artefact hangs the game at launch | confirms (both) | Zero cap: `economy.md §2` step 1, `world.rs:364-377`. The hang: `economy.md §2` step 6 (the exe loop never ends). Razdor stops after 10,000 rounds (`src/rules/economy.rs:132`): deliberate, D2 |
| 32 | Negative-price artefacts are never sold or random; as fixed goods they pay the buyer | confirms | `economy.md §2` "Buying" (exe 0x4b9e18: the sign of the cost is not taken off) |
| 32 | The church price window should top out at 245 to match vanilla churches | guide wrong (shipped data) | Amulet and potion items cost 25, 50, 75, 150 and 250 (ids 94, 95, 114). Shipped churches use 250, so the "vanilla" claim does not match the data |
| 33-34 | "Towns and churches always have several potion slots; their number depends on the total goods count" | guide wrong (exe says the potion step runs for towns only) | Exe 0x4be178 runs the potion step only when the building type byte is 1 (town). Churches get potions through the normal candidate list. The count is R div 5 + 1, where R is the random count less the fixed goods still present. Razdor matches (`src/rules/economy.rs:635-641`) |
| 33 | A market with no random places and only fixed goods gets no new goods once they are bought | partly wrong | For a town the potion step still runs when R ≤ 0 (0x4be178). Razdor matches (`economy.rs:636`) |
| 35 | The spell library lists some spells priced 0 or 1 with no icon: technical, not for sale | new, data, not checked | |
| 36-37 | Hiring: six unit slots per building; "there" is the count at the start, "max" the maximum; stock regrows on a random chance each day | confirms | `dtm-format.md §6` (barracks triples); `economy.md §2` (barracks regrowth, 0x4a1998) |
| 37 | "All types" must be ticked for the player to hire bandits or undead; AI armies hire them without it | confirms | `economy.md §7` (byte 356); AI has no such test (`src/rules/ai.rs:2873-2895`) |
| 37 | A building with barracks heals; a building can heal without hire through a bug (section 7.4) | confirms the first part; the bug is open | Heal lives in the hire tab (`economy.md §7`). See OQ "hire row" |
| 40 | Buy price factor by the building's attitude to the player: 0 per cent +70, 16 +45, 33 +25, 50 +10, 66 0, 83 -10, 100 -25; only the player's attitude counts | confirms | `economy.md §2` (table 0x4a03ec: attitude -3..+3 → 1.7, 1.45, 1.25, 1.1, 1, 0.9, 0.75); a building the player owns counts as +3 (`economy.rs:80-98`) |
| 40 | Sell, hire and spell prices do not depend on alliance | confirms | `economy.md §2` |
| 40 | "The base price table is in chapter 3.3" | guide error (editorial) | There is no price table in 3.3; pages 27-30 have only types and capabilities |
| 41 | Passive income: towns pay nothing at noon; the hire tab shows their nominal income | confirms | `economy.md §1` step 3, §7; `economy.rs:348` |
| 41 | Village tribute: one day's income is in the stock at the first noon; with max 0 the village pays once | confirms | `economy.md §3` (0x4b55f0); `world.rs:1094-1095`; `economy.rs:210-217` |
| 42 | Castle and fort: income and max must match, or the income is "phantom" (paid to nobody) | guide wrong (on "must match") | The castle's stock resets at every noon (0x4a41d8) and grows from 0 each midnight by income × √(1 − stock/max) (0x4a1998). The payout per day is min(income, max). The "phantom" part is right: the hire tab shows the nominal income. Shipped data: of 216 castles and forts, 135 have max above income, 60 equal it, 20 have max 0, 1 has max below income |
| 42 | Mana income and max work for villages | confirms | `economy.md §3`: only village mana is ever collected |
| 41 | Ruin treasure grows by its income each day up to its max, with the example 150+200, 150+200+200, 150+200+300 | guide wrong (exe says square root) | 0x4a1998: growth is income × √(1 − stock/max), and ruins have no noon payment. From 0 with income 200 and max 300: day 1 gives 200, day 2 gives 300 (200 + round(200·√(1−200/300)) = 315, capped at 300), not 400 |
| 45 | Ruin treasure reward is halved like the army loot | guide wrong | Exe 0x4c50ec, garrison branch: gold = stock + garrison gold (the treasure) + one day's income, with no division. Razdor matches (`src/rules/game.rs:2882`) |
| 45 | Ruin goods: the first five goods become the garrison's items and the treasure | confirms | `economy.md §3` (first five goods; 0x4b554e, 0x4a273c). The editor offers six slots: gap E5 |
| 43-44 | Garrison levels 0-9 (ten levels); garrison defence bonus slider 0-50 | confirms the levels and the slider | records (dt-editor) §3 |
| 43-44 | Shipped maps set 65, 75 and 100 on six buildings | new, data | The original editor clamps values above 50 to 50 on load and save (records §3). Razdor keeps them: gap E4 |
| 44 | Castle defence is not pierced by piercing strikes; the example (45 piercing against a militiaman behind 50 defence does 1) | confirms the rule (`battle.md` row 28); the arithmetic is not checked | |
| 44 | "Garrison serves the AI only" (byte 357) does nothing | guide right; our doc stale (fixed) | Exe 0x4bbc84 reads the hire flag at +0x164; no decompiled code reads +0x165 (byte 357). Razdor writes it and never reads it (gap E3). `dtm-format.md §6` row 357 corrected 2026-11-05 |
| 45-46 | Moving buildings and armies (a hidden tool): references are by number, so deleting an earlier object shifts later ones and breaks their events | confirms | `dtm-format.md §9` (1-based ids) |

## Chapter 4: Armies (pp. 46-69)

Summary: the army settings, behaviour styles, targets and checkboxes match our specs. The guide is
wrong on feudal respawn, on rogue income, and on the loot rule (the player's loot has no minimum).
The editor lacks a control for the daily income (gap E1).

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 47 | Player armies: one spawn point per class at most; a map with no spawn cannot be launched | confirms the spawn rule; the launch claim new, unverified | `dtm-format.md §3`; `dtm.rs:307-318` |
| 47-49 | AI army settings: name, hero name, leader class, start level (+1), starting gold, artefacts (three slots for the army), spell, home building; a ship texture on water | confirms (`dtm-format.md §7`, bytes 4, 17, 25, 26-27, 50-52, 84); the automatic ship texture new, unverified | |
| 49 | Artefacts belong to the whole army; the AI spreads them for the most strength; unused artefacts drop on defeat | drop on defeat confirms (AI loot pool); the spread unclear | |
| 49 | An army can buy and sell artefacts it does not use | unclear | Not traced |
| 49 | The army spell is permanent, on the listed units only; hired or joined units do not get it | confirms | `world.rs:1233-1235` (`MAP_ARMY_SPELL_END`) |
| 49 | The home building is not owned by the army but is used for respawn | confirms | `ai.rs:3593-3660` |
| 51-52 | Named characters: up to 32 per scenario, each with an id used by events; deleting one shifts later ids | confirms | `dtm-format.md §3` (0xEE, 0xEF); §7 byte 58 (1-based) |
| 52 | A carried-over named character keeps its id; if the next map's list has another name there, the name changes | confirms | `script.rs:1213-1241`; `saves-data.md §6` |
| 54 | The editor takes at most 12 units including the leader; the limit can be bypassed by adding the leader last | limit confirms; the bypass unverified | records §2 (AddUnit refuses above 12); the loader drops the overflow in order (`world.rs:122-144`) |
| 54 | A 13th unit makes the game drop the last one in reading order | confirms | `world.rs:122-144` |
| 54 | Levels above 10 are allowed for armies; levels near 100 crash the game | levels confirm; the crash unclear | |
| 54 | A battle XP total of about 2147 gives no XP | new, exe; the guide's reset claim open | The vanilla award is a 32-bit product divided by 10^6 (0x4c5676-0x4c569d), so a product above about 2147 wraps instead of capping. `experience.md:391` records the wrap. What the wrap yields is open (OQ "XP above 2147") |
| 54-55 | The tactical cost is two numbers, "approximate" | confirms as an approximation | records §2; `experience.md` (army strength) |
| 55 | The left number under "cost" is the hire cost; the right number the daily payment | unclear (the screenshot's figure does not reproduce) | The editor shows an estimate: the left is the sum of unit costs (0x547070) and the right the sum of cost ÷ CostRecrutDiv. The game's wage per unit is round(cost ÷ div × f), f = 0.25 to 1 by cost (`economy.md §1`, 0x4a163c). The guide's 332 does not come out of its own costs (sum 374, game rule 258) |
| 54-55 | Gold income: paid every day, from nothing; the editor shows it ×10 | confirms | `dtm-format.md §7` (byte 80, daily income ÷ 10); `ai.rs:210`. No editor control: gap E1 |
| 56 | Feudal armies: income from castles, forts and linked villages; hire ordinary units; buy artefacts; collect tribute; pay wages | confirms | `ai.rs:139-141` (lordly), `ai.rs:2586` (tribute, feudal), `ai.rs:3694` (wages, feudal) |
| 56 | Rogue armies: bandits; buy artefacts; no tribute; no wages; no building income | tribute and wages confirm; building income guide wrong | Exe 0x4a41d8 (the castle, fort and linked-village loop) has no style test: every army's noon pays them. Razdor matches (`ai.rs:3669-3690`; checked, not a gap). Shipped data: rogue armies own 6 castles, 15 forts and 23 ruins |
| 56 | Peasant armies: no income, hiring, artefacts, tribute or wages | confirms | `ai.rs:139-141` (lordly false, no artefacts); `ai.rs:2586`, `:3694` |
| 56 | The style comes from the texture (feudal, rogue or peasant); other textures come with other leaders | confirms | `dtm-format.md §7` byte 59 equals the model 4, 5 or 6 in all shipped armies; `Style::of` (`ai.rs:118-126`) |
| 57 | Target models: standard, aggressive, passive, hoarding, trading | confirms | `_Global.ini` priority lists (lines 69-87; columns in that order) |
| 57 | Aggressive: enemy armies and castles first; passive: less battle priority; hoarding: villages and tribute first; trading: markets first | confirms | `_Global.ini` (`MinVillageTarget` hoarding = 1; `MinPurchaseTarget` trading = 50, hoarding = 250; `MinAtackArmyTarget` aggressive = 1, passive = 100) |
| 58 | Aggression runs from -100 to 100; a huge strength gap makes even an aggressive army run; armies cannot be made fearless | range confirms; the fearless limit unclear | records §2 (-100..100 step 10); `ai.rs:1462-1463` |
| 58-59 | Five checkboxes: ignored by AI; hunts only the player; no random targets; no socialising; no interest in buildings | confirms | `dtm-format.md §7` (bytes 76-81); `ai.rs:217-221`, `:2147`, `:2224`. "Ignored" can still be met on the step (`world.md §4.2` step 1) |
| 59 | No random targets: after finishing a goal the army drops low-priority goals and waits longer | confirms | `ai.rs:2224` (`!no_random \|\| idle > IDLE_FOR_RANDOM`) |
| 60 | Patrol box around the post; radius 0 means the army stands still; it may leave the box to chase or visit a village | confirms | `ai.rs:427-429` (radius 0 is a stationary guard), `ai.rs:477-479` (pursuit is not limited) |
| 60 | A patrol near the bottom edge crashes when the route runs off the map | new, not reproduced | Razdor clamps the wander box to the map (`ai.rs:2004-2011`): deliberate, D1 |
| 61 | Speed correction -3 to 5; correction 1 makes an army as fast as the ranger; messengers use it | range confirms; the -1 for unit 2 is open | `world.md §2`: speed is max(1, 5 − c), minus one when the leader is unit 2 (the archmage). See OQ "speed bonus" |
| 61 | Inactive armies are placed inactive and activated by events; any active army can be made inactive | confirms | `dtm-format.md §7` (model 7, byte 63); `events.md` (activation) |
| 62 | Respawn: days (0 = never); the whole army or only the leader comes back; feudal armies come back in one of their own buildings, otherwise not; rogue and peasant armies come home even if it was captured; no home, no respawn; the patrol centre stays at the start | mostly confirms; feudal guide wrong | Exe 0x4a28d0 looks only at town (type 1), then castle (3), then fort (4), in building order, for feudal armies. Razdor matches (`ai.rs:3604-3616`). Rogue home rule: `ai.rs:3617-3620` (types 2, 9, 10, 12); patrol centre kept: `ai.rs:3634-3636` |
| 63 | Army hiring: "add experience" for hired units, up to 1000; "as the player" adds the player's average XP | confirms | records §2; `dtm-format.md §7` (bytes 14, 19); `ai.rs:2801-2827` |
| 63 | Garrison strength runs from 0 to 90; 0 means none; armies do not defend castles by default | range and 0 confirm; the default is open | `ai.rs:2603` (0 means none). The default for a new army is open (OQ "default garrison strength"); Razdor uses 50 (`src/editor/defaults.rs:89`) |
| 64 | Feudal income and wages: a suggested net of 10 to 20 gold a day | sources confirm (`economy.md §1`, `ai.rs:3669-3700`); the figure is advice | |
| 64 | The player's loot from an army: gold ÷ 2, and nothing if the army has fewer than 25 gold | guide wrong (exe says no minimum) | Exe 0x4c50ec divides by VictoryGoldDiv (the global, 2) with no threshold. MinVictoryGold applies only to AI-against-AI fights (`economy.md §3`). Razdor matches (`src/rules/economy.rs:972-976`); the 25 threshold is AI-vs-AI (`economy.rs:957-966`). Our `mechanics.md` said the minimum applied: fixed 2026-11-05 |
| 64-65 | "Units carry no money" (byte 62) removes the units' personal money from the loot and keeps feudal armies alive when they cannot pay | loot part confirms; the keep-alive part guide wrong | The flag is read in the loot code only (+0x3822, byte 62). The wage and desertion path has no such test (`ai_pay_wages`, `economy.rs:975`). The guide's "money proportional to tactical cost" is the wage bill (`economy.md §3`) |
| 66 | Army relations: the default is the global matrix; an army with a relation of 50 per cent or more does not attack that faction; the player can attack any army; an event can prevent it | confirms | `world.md §4.2` step 1 (on open ground every army is engaged on the player's step) |
| 66-68 | The army generator tabs (infrastructure, economy, armies); generation overwrites settings | new, editor-only, not traced | |

## Chapter 5: Events, theory and reference (pp. 70-138)

Summary: the event theory matches the engine: scan order, global and local events, quests and rumours,
flags, counters, subordinate and relative calls, and most result rules. The guide is wrong on a set of
points where the exe is clear. The largest group: a text-less event still applies gold, mana, units,
artefacts and spells in the exe, and the guide says it does not. The interval model is contested and
open (the window length); the sections below mark it.

Page numbers are the guide's printed PDF pages, checked against the page images. The screens are the
guide's own, so they show what the guide shows, not what the exe does; where the two differ, the verdict
follows the exe.

### 5.1-5.2 Theory, archetype, group (pp. 70-74)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 72 | Each tick the first event whose conditions hold fires; the check restarts from the top; "exact" for global events, "formal" for others | confirms | `events.md §1-2`; the scan at 0x4abfbc restarts after each taken event |
| 73 | The event name is the window title for plain text windows (p. 91, the Yes/No dialog; p. 101, a text example). For picture events the title is the fixed text "Произошло событие!" (pp. 132-135). The diary shows the name too (p. 81) | plain windows confirm; the picture title is a known difference (G13, OQ "Picture window title") | `events.md §6.3`; Razdor uses the event name as the title of every window (`src/ui/story.rs:41`) |
| 73 | Text after `#` in an event name (and an army name) is an editor comment | confirms for events; army names not traced | `events.md §6.3` |
| 74 | The group colour is editor-only | confirms (editor-only); the stored range is open (OQ "Group colour range") | `dtm-format.md §9` (byte 0) |
| 74 | The group colour drop-down offers 14 colours | the editor agrees with the screen (14 items); dtm says 0-5. Open (OQ "Group colour range") | `docs/notes/mapedit/out/forms/TINFOEVENTS.dfm.txt` (ComboBoxGrupp, 14 items); the colour routine near `decomp.c:239890` indexes a 14-entry table and wraps above 13 |
| 73-74 | The hero archetype (all, knight, archmage, ranger) restricts an event to one class; a class change does not change which archetype's events fire. The screen labels index 2 as the archmage, so the dfm caption "Священник" is stale | confirms | `events.md §3.6` (byte 10); the start class is read at load (`start_class`) |
| 74 | Deleting the last event crashes the editor (the guide's warning); the editor forces a new event on opening | the guide states it; editor behaviour not traced | |

### 5.3 Global and local events (pp. 75-78)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 76 | A global event can happen anywhere on the map once its conditions hold | confirms | `events.md §2` step 1 |
| 76 | A local event is tied to a point or a building and runs only when the hero is on that point or inside that building, with its conditions checked then | confirms | `events.md §2` steps 2-3 (points on every step; buildings only on entry, 0x4ed434) |
| 77 | Points accept events of any type, but "only local events and quests" work there; rumours do not | guide wrong (exe says any type) | The point loop at 0x4abfbc takes every listed event whatever its type. `events.md §2` step 2 agrees with the exe |
| 78 | Buildings accept any type, but only local events, quests and rumours work | confirms | `events.md §2` step 3 and §10: buildings fire local events; quests and rumours are listed in the main hall and taken by the player |
| 82 | Some buildings do not support rumours and quests properly. The guide defers to chapter 3.3 for which ones, and does not name villages or shipyards here | the village and shipyard detail is not in the guide's chapter 5 pages; removed from this table. Our rule stands: villages and shipyards fire every listed event on entry | `events.md §2` step 3, §10 |
| 77 | A point's "active at start" flag is only a lamp switch | confirms | `events.md §2` step 2 (the point's active byte is not tested; it only lights a lantern at load) |

### 5.4 Quests and rumours (pp. 78-81)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 78-79 | A rumour or quest in a building is listed in the main hall once its conditions hold, and does nothing until taken. Quests in the hall list are highlighted blue; rumours are not (pp. 79-80) | confirms | `events.md §10` (the hall list passes the full check) |
| 80 | Taking a quest adds a diary entry, recorded even when it is a repeat | confirms | `events.md §7.2` step 11, §10 |
| 81 | The diary entry shows the event name, its text and the time elapsed, e.g. "less than an hour" | the screen agrees; the diary text not checked in code | |
| 79 | A rumour's results run after the player takes it. The guide's "rumour with a price has a negative gold result" is not on pp. 70-138; removed from this table | the results rule confirms; there is no price claim to check | `events.md §10` (rumours have no price field) |
| 80 | A quest on a point fires like a local event when the hero stands on it | confirms | `events.md §2` step 2 |
| 80-81 | A quest or rumour whose conditions fail is not listed at all | confirms | `events.md §10` (the hall list needs the full check) |

### 5.5 Intervals, time and repeats (pp. 82-85)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 83 | The activation time defaults to the scenario start. The date field reads "hh h dd.mm.yyyy", the hour first (pp. 70, 83, 86) | confirms | `dtm-format.md §9` (offset 2, minutes); the field format is the screen's |
| 83 | "на N часов" (N hours): the window is N hours from the start, repeated every P days; defaults N = 24, P = 1 | open | Our `events.md §3.4` and the exe reading disagree on the window's length. The exe reading (0x4a7b80): the window ends at day start + stored × 60 minutes. So for a repeating event any stored N of 1 or more covers the whole cycle day, and only N = 0 cuts it short. The guide's N = 6 example would open all day. OQ "interval N" |
| 83 | N = 24 with P = 2 gives an event available every other day | confirms the cycle | `events.md §3.4` (only even cycle days open) |
| 84 | Day 0 of the calendar is the last day of the previous month (the guide's text) | open | The editor date formula gives day 0 as -1,440 minutes; the exe clamps a negative start to 0 (0x4a7b80). OQ "Editor day 0" |
| 84 | A start date before the scenario start means the event never fires on its own | guide wrong (exe says it fires) | Exe 0x4a7b80: a negative start is set to 0; only start > now is refused. A window still open at the start fires. Our `events.md §3.3` matches the exe. Settled (Settled list) |
| 84 | Interval settings do not apply to local events; local events are available any time after their start | guide wrong (exe says they apply) | 0x4a7b80 has no event-kind test; the scan 0x4abfbc passes point and building candidates through it. Local events obey P and N |
| 84 | A text-only event with N = 0 never happens | open | The exe reading: a stored 0 counts as one hour, so the event fires once in the first hour of each eligible day. Our `events.md §3.2` and `§3.4` agree with that reading. The guide's "never" needs a live test. OQ "N = 0" |
| 84 | The same N = 0 event with a victory or defeat result still fires; with other results the game crashes | unclear | Not traced |
| 85 | A multi-time event with a text window blocks movement, because the window reopens every tick | confirms | `events.md §3.2` (with a duration, the next tick fires again) |
| 85 | A multi-time event fires any number of times within each interval | confirms | `events.md §3.4` |
| 85 | Events fire once per playthrough unless "many times" is set | confirms the rule; the default of the box is open | `events.md §3.5` (byte 141). The "many times" box is unchecked on new events on the screens (pp. 73, 86), and records §9.2 say the default follows the shipped ini's option. OQ "many times default" |

### 5.6 Events and the player (pp. 85-92)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 87 | Dependencies: "happened with yes", "happened with no", "not happened"; two slots each | confirms | `events.md §4` rows 2-4 |
| 88 | Victory over an enemy: two slots; the listed armies were beaten by the player and have not come back | confirms | `events.md §4` row 1; the drop-downs show the army name, its leader and its value in dollars, e.g. "Army 1 / One Hero (40$)" |
| 89 | "Meeting an army" works only for global events and interrupts the battle start | confirms | `events.md §9`; the global-only limit follows from the scan (contacts read no point, buildings are scanned on entry only) |
| 90 | Current stats, in this order on the screen: level, gold, holiness and mana, squad count, army strength, then a 1-HP checkbox. Each numeric box has a grey toggle between "at least" and "at most", and the screens show "at most" | gold, squads, strength and 1 HP confirm; the toggle is new (screens) | `events.md §4` rows 9, 11; the sign of the stored value encodes the toggle (`dtm-format.md §9`, Conditions, offset 11) |
| 90 | "Current level" checks the hero's experience (the guide calls it a bug to avoid) | guide wrong (exe says level) | 0x4a80b4 → 0x4a7b40 compares the hero's 0-based level (`experience.md`). Razdor's `hero_level` is level − 1 (`script.rs:440-450`), which matches. The blue note on p. 90 says both this field and the mana one are repaired in an unofficial community fix (chapter 7.17); the exe checked here is the vanilla build, which has the bug |
| 90 | "Squads" counts the characters in the player's squad, at most 12, the hero included | the guide states it (p. 90); the exe adds that dead units are counted, which the guide does not say | 0x4a8102 and the count block: the count is the number of unit records, dead units included |
| 90 | "Holiness and mana" is not processed and is always true | guide wrong (exe compares mana). The unofficial-fix note above applies too | The vanilla numeric block at 0xc2875e compares mana with the same counter the finish adds to. Razdor's `mana()` (`script.rs:448-450`) matches |
| 91 | The answer box: a question shows Yes and No (the dialog's title is the event name and its buttons are "Да" and "Нет"); No counts as happened and runs no results; Yes runs them; "repeat after yes" decides whether a once-event closes | confirms | `events.md §6.2` (byte 149, xor) |
| 92 | The table of once, many-times and repeat-after-yes combinations | confirms | `events.md §6.2` (once = 149 xor 1; a No uses up a once-event) |

### 5.7-5.8 Events and heroes (pp. 92-98)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 94 | Building ownership per slot: the player, not the player, or a faction (codes 2-5). The test is on the building's faction byte, whoever holds the building | confirms, and the spec stands: the routine at 0x4a815a tests the faction byte (offset 337) for codes 2-5, and the owner-army byte (offset 292) for codes 1 and 6. The guide's "must belong" is loose wording | `events.md §4` row 12; `dtm-format.md §9` (row 33); `docs/notes/out/decomp.c` (FUN_004a815a) |
| 94 | The player always belongs to the green team | the guide states it (p. 94); not checked in code | |
| 95 | Artefacts held: for AI armies only the worn items count, so an item no unit can use never passes | confirms | `events.md §4` row 14 |
| 96 | Named units: the check ignores the unit's class, and dead units do not count | confirms | `events.md §4` row 13 (codes 1 and 6 search living units) |
| 97 | "Army already beaten by anyone": dead, whether beaten by the player or by an army, until it is revived | confirms | `events.md §4` row 7 |
| 98 | Active and inactive: active is on the map, inactive is off the map; a dead army can be either | outcome confirms; a destroyed army fails the active test in the exe | `events.md §4` rows 5-6 |
| 98 | "Army in its home building": an army with no home is never in it, so the event never fires | guide wrong (exe says it passes) | 0x4a7b80 tests the home only when it is above 0 (army +0x16B3). Our `script.rs:520-522` matches the exe |
| 98 | A deactivated army standing in its home building still counts as there | the guide says so (p. 98); the exe test is not traced | |

### 5.8 Results, first group (pp. 98-108)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 100 | Text limits of about 25,000 characters (Cyrillic) and 16,000 (Latin); more crashes | unclear | Not traced; Razdor sets no limit |
| 100 | Colour markers at the start of a paragraph: `*` white, `@` orange, `\|` blue; the last marker in a paragraph wins | guide wrong (exe has a fixed order) | 0x48e1cc tests the markers in the order `*`, `\|`, `@`, and each one found sets the colour, so `@` beats `\|`, which beats `*`, wherever they stand. Razdor matches (`src/dt/markup.rs:102-103`) |
| 100 | `^` centres a paragraph and combines with colour; put it first | confirms | 0x48e1cc (the `^` is found anywhere in the line and removed); `markup.rs:111` |
| 100 | `#HERONAME` is replaced by the hero's name. On a screen (p. 101) an unnamed hero shows as "(Нет имени)" | the replacement confirms (`events.md §6.4`); the placeholder text is unclear: the screen may be the editor's preview, and Razdor uses the class name for an unnamed hero (`game.rs:1007`) | |
| 100 | A text of only markers shows an empty window | confirms | `events.md §6.3` (markers are stripped) |
| 102 | Functions that need text (gold, mana, units, artefacts, spells, picture) do not work on an event without text; the event still counts as happened | guide wrong (exe applies them) | Artefacts and units: 0x4a8ae8 → 0x4a8e66, no message test. Spells: 0x4a9b75 → 0x49c144. Gold and mana: at the finish (0x4ab51b, +0x55 and +0x59), no text test. Razdor matches (`src/rules/events.rs:1575-1583`). The text-dependent case is a Yes on a question with an empty message, which skips the dialog-time results (`events.md §6.2`) |
| 101 | Experience works without text; a negative experience change does nothing to XP (p. 103) | experience confirms; negative part unclear | `events.md §7.2` step 4 |
| 103 | Gold does not work without text (p. 103) | guide wrong | 0x4ab51b (gold at +0x55, no text test); `events.rs:1575-1583` |
| 103 | Mana does not work without text (p. 103) | guide wrong | 0x4ab51b (+0x59 into the mana counter, no text test) |
| 104 | Units added: at most four slots; a named unit keeps its name; units taken from an army keep their state | confirms | `events.md §7.1`; `dtm-format.md §9` (offsets 97-104) |
| 104 | Units added by an event need no payment; units moved from an army pay; named units never pay | open | Our `events.md §7.1`: an event unit is wage kind 3 (never paid); a unit taken from an army keeps its whole record, wage kind included. The guide's split is not settled. OQ "wage kind" |
| 104 | A full army (12 units) when an event adds one: our rule dismisses the weakest of units 2-12. The guide does not say "weakest"; its pp. 119-120 rule ("the last unit that joined is replaced") is for squads moved by a result, a different case | our rule stands (`events.md §7.1`, exe 0x4a8fe5); no guide verdict | `events.md §7.1` |
| 105 | Spells learned: a full book does not block a spell; the guide says it does | guide wrong (exe has no capacity test) | 0x49c144 has a duplicate check (0x49c100) and no capacity test; the spell is appended. Razdor matches (`events.md §7.1`) |
| 105 | A spell the hero already knows is not learned again | confirms | `events.md §7.1` (duplicate check) |
| 106 | Artefacts gained: up to four per event. The guide's "a full pack loses the item silently" is not on pp. 105-106; removed from this table | the four-slot limit confirms; our own rule (lost silently, 0x49a85c) stands and is not a guide claim | `events.md §7.1`; `dtm-format.md §9` (113-116) |
| 107 | Delay: the hero waits N hours; a text event or a battle can interrupt the wait | delay confirms; interruption unclear | `events.md §8` (delay ticks; a chain behind a delay is dropped) |
| 107 | A subordinate event that also delays does not run | confirms | `events.md §8` |
| 107 | A rumour that delays keeps its window open after OK; Esc closes it | the guide states it (p. 107); not traced | |
| 108 | "Complete quest" removes the quest from the diary; completing a quest never taken is harmless; it works without text | confirms | `events.md §7.2` step 12, §10 |

### 5.9-5.10 Subordinate and relative events (pp. 108-111)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 109 | A subordinate event fires at once, unconditionally, once per call, even if it happened before; it is many-times automatically | confirms | `events.md §8`; `dtm-format.md §9` (offset 140 is the done flag) |
| 109 | A subordinate event is reached only through a chain | confirms | `events.md §3.1` |
| 110 | A relative call moves the target's start to now + hours (the 30-hour example works) | confirms | `events.md §7.2` step 19; 0x4ab51b writes the start field as now + 60 × delay |
| 110 | "Relative time only" stores the year-2000 start; the editor shows the default 01.01.2000 (screen). The event has no start of its own until a relative call sets one | confirms | `dtm-format.md §9` (offset 2 = 1,036,800,000); `events.md §3.3` |
| 109-110 | Subordinate and relative-only are mutually exclusive in the editor: setting one clears and greys the other (screens) | editor behaviour, new (screens); the stored fields are as `dtm-format.md §9` | |
| 110 | A relative call on a repeating event sets its duration N to 24 hours; P stays | guide wrong (exe writes the start only) | 0x4ab51b writes the start field only; Razdor matches (`src/rules/events.rs:1640-1643`) |
| 111 | A relative call with 0 hours re-bases the start and the event is then checked normally; a subordinate call runs at once | confirms | `events.md §7.2` step 19, §8 |
| 111 | A rumour with no text that calls a relative event with 0 hours shows visual artefacts inside buildings. The screen shows exactly this setup: empty rumour text, relative "Цикл слух", 0 h | unclear (not traced); chapter 9, 9.8 | |

### 5.11 Flags and counters (pp. 111-117)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 113 | Set a flag with `+NAME`, clear it with `-NAME`, test `NAME` (true) and `/NAME` (false); default false | confirms | `events.md §5` |
| 113 | Flag names need at least two characters | confirms | `events.md §5`: an action of two characters or fewer does nothing |
| 117 | Five-character names avoid the prefix bug | advice; the test is a substring, so length does not remove the trap | `events.md §5`; chapter 9, 9.16 |
| 114 | A counter's name must end in a digit to use counter logic. The same page's "+COUNT^" screen and p. 116's "+CASTLE^" with a "CASTLE9" test show the form on names that end in a letter | guide wrong (the guide contradicts its own screens; the `^` form works on any name) | The engine writes the digit itself (`+X^` appends X and 1, or raises the byte after X). Exe 0x4ab2a3 (the `^` branch) |
| 114-115 | `+NAME^` creates the counter at 1 or raises it by 1; `-NAME^` lowers it and removes it at 0; counters are not limited to 9 | confirms | `events.md §5` (counter form; the digit is the character after the name) |
| 115 | Two counters with the same name: the first created one is changed by `+` and `-` | confirms | `events.md §5` (first occurrence) |
| 115 | Flags and counters carry over between maps of a campaign | confirms | `events.md §5` (lifetime), §12 |
| 117 | The prefix bug: a flag test matches a flag that begins with the tested name (SAD matches SADDLE). The test is in fact a substring anywhere in the flag string (AB matches XAB) | confirms, and wider. The guide's narrower "prefix" is wrong for the exe (Settled list) | The test is a substring anywhere in the flag string (exe 0x4a8a22). `events.md §5` |
| 117 | A test for a counter's absence or presence uses the name without its digit | confirms | `events.md §5` (substring test) |

### 5.12 Results, second group (pp. 117-127)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 118 | Take units from the player, by name or by type; "squad goes to army" moves them with XP, artefacts and health; a full target army loses its last unit | confirms | `events.md §7.1` (removal; a full target overwrites its last unit) |
| 120 | With no room at all, nothing moves; with some room but not enough for all, none of the moved units is added | guide wrong (exe appends while there is room) | 0x4a8fe5: each unit is added while count < 12; once the army is full the copy overwrites record 12. Razdor matches (`script.rs:653-665`) |
| 119 | A unit removed from the squad and given to an army goes there with its record | confirms | `events.md §7.1` |
| 119 | The moved unit keeps its XP, artefacts and health | confirms | `events.md §7.1` |
| 121 | "Move to hero": the army is put next to the hero on the first passable cell, clockwise from the top left. The screen numbers the 8 candidate cells | guide wrong (exe takes the cheapest neighbour) | 0x4980d8 takes the lowest cost of the eight neighbours, ties to the first clockwise; +50,000 for a building cell, +100,000 for an occupied cell; no move when every cell costs 100,000 or more. Razdor matches (`script.rs:758-790`) |
| 121 | An army on water moves to a hero on water only, and one on land to a hero on land | unclear | 0x4980d8 uses the hero's cost map and tests no army medium. Razdor matches the exe |
| 121 | "Move to hero" leaves a phantom army on the old cell, which cannot capture or interact | new, exe; Razdor has no phantom | 0x4980d8 zeroes the old cell's logical occupant byte (+8) but not the drawn byte (+9). Razdor clears the cell correctly (`script.rs:758-790`). Whether to copy the phantom is open (OQ "phantom"; a decision) |
| 121 | "Move to hero" works without text | confirms | `events.md §7.2` step 14 |
| 122 | Taking artefacts works only with text | guide wrong | Artefacts are removed when the window opens, with no text test (0x4a8e66 → 0x49a8a8; Razdor `events.rs:1481-1493`) |
| 123 | Activating an army puts it on its start cell, even if it was deactivated elsewhere; it keeps its troops, XP and resources | confirms | `events.md §7.2` step 7 |
| 123 | A deactivated dead army stays dead; activating a dead army revives it early | confirms | `events.md §7.2` step 7, §4 row 6 |
| 123 | Activating and deactivating the same army in one tick can crash the game | open | No fault path in 0x4969b8, 0x496900, 0x496774 or 0x48eeb0 (chapter 6, F31). OQ "same tick" |
| 124 | "Change patrol": a negative result stops the army (radius 0); a patrol change on an inactive army can crash at the lower map edge (the guide ties this to chapter 4.6, not to a screen) | radius confirms; the crash unclear | `events.md §7.2` step 8 |
| 125 | "Change hero class" keeps level, XP and equipment | confirms | `events.md §7.2` step 16 (only the unit type changes) |
| 125 | The archmage and ranger class bonuses are not tied to the hero and stay after a change | archmage confirms; ranger is a gap | The exe fixes speed, sight and cast divisor from the start-class global (0x68dccc) at load (0x4b2504). The ranger's noon heal is the exception: the exe reads 0x68dccc, Razdor the current class (gap G3) |
| 125 | The knight bonus: the guide's wording differs between pp. 124 and 125; p. 125 says it is tied to the character, not the type. Our spec says the unit type | open (OQ "Knight bonus after a class change") | `battle.md` ("Which army is a knight army": the flag comes from the first unit's type, 0x49855c); the class-change case is not traced |
| 125 | Frequent class changes: "not recommended more than once a day" | advice, not a limit (the crash claim is unclear) | Not traced |
| 126 | "Start battle with army": the battle starts even if the army is inactive, and the army becomes active | confirms | `events.md §7.2` step 15, §8 |
| 126 | Only one battle can start per tick: the lower event number wins; a later battle event only activates its army | the guide states it as a rule; Razdor does not follow it (gap G2). The exe ordering is open | `events.rs` (battle path); OQ "two battles" |
| 126 | A battle in a building, or in the same tick as a text event, shows overlapping interfaces; loading an autosave cannot replay a battle that started far away | unclear (UI and save) | Not traced (chapter 9, 9.13) |
| 127 | "No meeting with army" ends the meeting; later meeting conditions of that scan fail | confirms | `events.md §7.2` step 1, §9 |
| 127 | "No meeting" has a "!" warning label next to the checkbox in the editor (the label appears when the event has a meeting-with-army condition). The mark is a warning label, not part of the checkbox; do not use the option | confirms; the label wording corrected | `events.md §9` (byte 148, which also switches the Community opcodes); the form's label is LabelMA |

### 5.13 Lamps, show army, spells (pp. 127-131)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 128 | A lamp (point) reveals fog of war; up to 24 cells of radius. The lamp dialog's default in the screen is 10 | confirms | `dtm-format.md §8`; `events.md §7.2` step 10; records §2 (a new lamp gets radius 10) |
| 129 | A radius above 24 crashes the editor and the game; a negative radius crashes the editor once | unclear | Not traced. Razdor does not clamp (`script.rs:789-794`): deliberate, not replicated |
| 129 | An activated lamp (a point with "active at start") works as a plain lamp | confirms | `events.md §2` |
| 130 | Several lamps in one event light in order; the lamp slots are a 2×2 block numbered 1-4 in row order; the same lamp may be named several times | confirms | `events.md §7.2` step 10; `dtm-format.md §9` (lantern slots 128-135, lit ids) |
| 130 | "Show army" scrolls to and reveals an army; a dead army shows the cell where it died; an inactive one shows its start cell | new, exe; gap G4; the inactive start-cell claim is open | 0x4ab51b reads the record's current cell (+0x1724, +0x1728) for any army, with no activity test. Razdor reveals only on-map armies (`script.rs:745-750`). OQ "show army" |
| 131 | "Activate spell": a self-target spell lasts exactly five times as long as when the player casts it; an enemy-target spell lasts as long as when cast | guide wrong (flat 5×) | Own-army event spells last 10 × TimeWork hours when TimeWork < 8, and 5× when TimeWork ≥ 8 (`magic-items.md §4.1`). Razdor matches (`magic.rs:215`, `:620`) |
| 131 | A lamp, show army and spell in one event (two or more of them): the first available event runs without some of its conditions, causing random events and crashes | open | Unverified. `events.md §8` describes the chain timer, not this effect. OQ "lantern, show or spell with another effect" |
| 131 | These functions work without text | confirms | `events.md §8` (lanterns, shown army and spell start the chain timer; no text needed) |

### 5.15 Pictures (pp. 131-136)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 132 | An event picture is none, a stock picture (victory or defeat) or a custom one. The victory and defeat stock pictures render badly in the game | stock values confirm (200 defeat, 201 victory); the bad rendering unclear | `events.md §6.3`; `src/ui/story.rs:57-58` |
| 132 | The picture drop-down starts with none, defeat, victory, then portraits, and has a scroll bar. The original's list also holds the install's global images from id 200 up (records §9.1). Razdor's editor offers none, 200, 201 and the portraits below 200 | the original list is wider than Razdor's; gap G5 | `src/ui/editor/events.rs:98-101`; records §9.1 (dt-editor). OQ "Picture 202" |
| 133-134 | Custom pictures: 32-bit uncompressed TGA with no semi-transparent pixels. The guide's route is a JPG, then a community TGA converter with H and W fields | unclear (not traced); the stored format is 16-bit ARGB4444 with width and height | `dtm-format.md §9` (offset 163), §11; gap E8 |
| 135 | A custom picture with a one-space text: 581 × 563 works (the screen on p. 135); the guide says a picture larger than that crashes, so the limit is above 581 × 563, not at it | unclear (the size limit is not traced); the wording corrected | gap E8; the open picture-size entry |
| 135 | Semi-transparent pixels crash the game and can stop the editor saving the map | unclear | Not traced |
| 136 | The picture's index breaks when an event is deleted or moved; add pictures last | new, explained by the format | Pictures are read by position in event order (`dtm-format.md §9`, §11); the editor re-links by index |
| 136 | A picture needs text to show | confirms | `events.md §2` (a text-less event opens no window) |

### 5.16 The function table (pp. 136-138)

The table (p. 137) marks which functions need text. The verdicts below are the table's rows.

- Rows that agree with the exe and with us: subordinate call, relative call, quest completion, experience, delay, flags and counters, lamps, show army, activate spell, active and deactivated armies, patrol, class change, battle start, move to hero (`events.md §7-8`).
- Rows the exe contradicts: gold, mana, army plus and minus, artefacts plus and minus, and spells learned. The table marks spell learning as needing text; the exe applies it on a text-less event (0x49c144). Guide wrong for all of them (see 5.8).
- "The result never decides whether the event fires; the conditions do" (p. 138): confirms (`events.md §4`, §6).
- "If a text-less event has both kinds of result, only the kinds that do not need text run" (p. 138): guide wrong, for the same reason as above.

### Added from the screenshots (pp. 70-138)

Facts the screens show that the text does not, and that matter for parity or for the file format. Editor-only
layout facts (lists, filters, captions) are left out.

- Window titles: a plain event window takes the event's name; a picture event takes the fixed title "Произошло событие!" (pp. 91, 101, 132-135). Razdor shows the event name for both (G13).
- The Yes/No dialog takes the event's name as its title, with the buttons "Да" and "Нет" (p. 91).
- Each numeric current-stat box has a grey ≤/≥ toggle, default "at most" (pp. 70, 86, 90). It is the sign of the stored value (`dtm-format.md §9`, Conditions).
- New spins start at 0 for experience, gold, mana, delay and relative hours (pp. 99, 103, 107, 110).
- Subordinate and relative-only are mutually exclusive (pp. 109-110), as above.
- Points: the point window header shows "№ N" (p. 130). Point priorities and active time have no function and are always 0 (pp. 77, 130; `dtm-format.md §8`, rows 28-36).
- A lamp's slots are a 2×2 block numbered 1-4 in row order (p. 130).
- Army activation block: two activated-army slots and one deactivated-army slot (p. 123).
- Patrol change example: a spin value of -5 (p. 124); the range is -120..120 in steps of 5 (records §9.1).
- The diary entry shows the event's name, its text and the elapsed time (p. 81).
- Quests in a building are highlighted blue in the hall list; rumours are not (pp. 79-80).
- Hidden control: "generate the battle army" is hidden and disabled in the form (dfm; records §9.1). It shows on no screen.
- The "!" next to the no-meeting checkbox is a warning label (p. 127).

## Chapter 6: Event examples (pp. 139-194)

Summary: the recipes work as written when the screenshot settings are used, with two caveats: a
duration of 0 closes a window after 60 minutes, and the randomness account is wrong on the mechanism.
The chapter's scan rules are the most useful part, since they settle the scan's stop rule. Two recipes are
broken as written: 6.17 corrupts a flag once its counter reaches zero, and 6.16.3 collides with GENSEED
by substring. Resurrection (6.14) credits a checkbox that does not revive the character.

### Scan, chaining and ticks (6.15.1, pp. 139-175)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 170 | Silent events finish at once and the scan starts again from the first global event; an event with a window holds the scan until OK | confirms | `events.md §2`; 0x4abfbc (the silent flag 0x674966; a window skips 0x4ab1ec) |
| 170-171 | "A subordinate call ends the tick's checks" | guide wrong (partly) | Only a chained event that is not silent stops the pass: one that opens a window, a delay, a spell, lanterns, a shown army or a battle. A silent chained event is finished in place, and when it has no chain of its own the scan reruns from the top (0x4ab51b tail calls 0x4abfbc). Our `events.md §8` lacks the stop rule (a doc addition, not made) |
| 141, 170 | Chained events skip their own checks: no flags, window, guard, once, class, place or conditions | confirms | `events.md §8`; 0x4ab51b tail |
| 141, 143, 191 | Subordinate events never fire from the scan or the hall; only a chain reaches them | confirms | `events.md §3.1`, §10 |
| 170 | One firing per pass: last-fired is set to now + 1; duration 0 also needs more than 60 minutes | confirms | `events.md §3`; 0x4a7b80 (guard), 0x00c28710 (sets last-fired) |
| 146, 170 | Idle reset: when a pass finds nothing, every last-fired in the future is set back to now; an event fired this minute can fire again on the next scan | confirms the reset; placement open | `events.md §2`; 0x4abfbc tail. Razdor resets at the end of `run` (gap G7) |
| 170-172 | The procedure pattern works: events run in array order, each restarting the scan; a START flag raised by a silent subordinate call runs the whole procedure in one scan; the last event removes START | confirms | `events.md §2`, §3, §8 |
| 171 | A delay inside a procedure is not silent: the scan stops and the rest runs next tick | confirms | `events.md §2` (silence test 0x4a7a40), §8 |
| 175 | Entering a building runs the scan over its local events; quests and rumours of the hall are listed, not fired by that scan | confirms, with nuance | `events.md §2`, §10 |
| 175 | Vanilla events draw no random numbers | confirms | The event engine calls no LCG function (0x4832fc); the finish's gold anti-cheat branch (0x4ab150) is the one draw reached |

### Windows, repeats and timers (6.4, 6.5, 6.6, 6.8, 6.17)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 145 | Cooldown rumour (6.4): duration 24 h, repeat 1 day, multi, self-relative 24 h; the window stays open and each firing re-arms the start at firing time + 24 h | confirms | `events.md §3` item 4; 0x4a7b80 (repeat branch). With duration 0 the window closes 60 minutes after the start; Razdor tests that (`events.rs:2433-2463`) |
| 146 | Merchant (6.5): a decline counts as happened and sets last-fired; no once unless the event is once; Yes sets once when "repeat after Yes" is unticked | confirms | `events.md §6.2`, §2 |
| 146 | The numeric gate: gold ≥ 750 needs "current stats" (byte 18); the other numeric fields are 0 | confirms | `events.md §4` row 11 |
| 148, 152, 183 | Relative re-arming drifts: the next start is the firing time plus the delay, and the firing time is the first scan after the start, so any lag carries into later cycles | new, derived | `events.md §7.2` step 19 and §3; the guide does not mention it. A doc addition for `events.md §3` or §7 (not made) |
| 152 | Timed quest (6.8): the expired event is once, relative-only, duration 24, repeat 1 day; completion excludes it by a not-happened test | confirms | `events.md §3`, §4 row 4, §7.2 step 19. Duration 0 would close the expiry window after an hour |
| 176 | Counter arithmetic (6.16.1): a plain `-SEED9` removes the entry; `+SEED^` appends SEED1 when absent or raises the byte; at digit 9 the byte becomes `:`, so the reset must run first | confirms | `events.md §5`; 0x4ab2a3 |
| 183-186 | Hourly relative calls (6.17): each feudal changes the counter at most once an hour, only if the event is open when he is home; with duration 0 the chain dies if he is not home in that hour | unclear | The time settings are not visible in the screenshots |

### Conditions and flags (6.9, 6.10, 6.12, 6.14, 6.19)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 153, 161, 167 | Flag tests are substring searches: `/QUEST` blocks any flag containing QUEST, such as QUESTX | confirms | `events.md §5` |
| 152-158 | "Happened" is times fired > 0 with the answer not 1; "not happened" is times fired = 0; a No counts as happened, which makes mutually exclusive dialogs work | confirms | `events.md §4` rows 2-4, §6.2 |
| 154-158 | The ambush (6.10): the Yes result takes gold, but the guide adds no gold condition; gold clamps at 0, so a player with no gold can say Yes for free | new, a gap in the recipe | `events.md §7.2` step 5 confirms the clamp. Not a Razdor bug; kept as the documented case in S13 |
| 191-192 | The artefact check searches the pack and the worn items of units 1-12; an item moved into a garrison is not found | confirms | `events.md §4` row 14 |

### Meetings and contact (6.1, 6.2, 6.3, 6.10, 6.11)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 139, 159 | Stepping onto a friendly army: on open ground the hero is engaged by any army he steps onto, friendly included; the scan runs with that army as met; an event that fires cancels the battle | confirms | `world.md §4.2` steps 1, 5; `events.md §9`; `src/rules/game.rs:2152` |
| 139 | A question mark shows on a friendly army when a meeting event waits and the army's attitude is 1 or more | confirms | `interface.md` (the meeting-waiting cursor); `magic.rs:384`, `ui/minimap.rs:631` |
| 141, 143, 159 | A friendly army inside a building cannot be met by walking onto it; a stationary one in a castle, fort or village never starts a meeting | new, exe; settled (Settled list) | `world.md §4.2` steps 1-2; `ai.md §8`. Razdor does the same (`game.rs:2152`, `:2160-2162`) |
| 154-158 | A stationary hostile ambush: a patrol-0 army does nothing until the hero steps into it; No counts as happened, so the second dialog is blocked and the second contact fights | confirms | `ai.md §2`; `world.md §4.2`; `events.md §4` row 4, §6.2, §9 |
| 159-160 | The messenger (6.11): yellow (friendly to all) at 100 per cent, the others at 66; peasant style; ignored by AI; hunts the player; a peasant never assaults a building | confirms the style; the slider mapping open | `ai.md §1, §3, §8, §9.1`; `ai.rs:516-519`. The guide's 66 per cent as +2 and 100 per cent as +3 is inferred; the exe-traced price steps give 66 per cent as +1 (OQ "attitude slider"). Both are above 0, so the messenger behaves the same |
| 159-160 | The messenger's patrol flag and radius are not visible; a stationary army never moves | unclear | `ai.md §2` |

### Activation, revival and ownership (6.6, 6.7, 6.12, 6.13, 6.14)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 148, 164-167 | Activating an off-map army revives all its units (HP full, paid now), clears destroyed and beaten-by marks, and puts it on its start cell; if that cell is its home building, the building takes its owner, faction and relations; nothing happens if the army is already on the map | confirms | `events.md §7.2` step 7; 0x4969b8 (unit loop; home test copies owner and relations; on-map test returns). Razdor `script.rs:307-364`, except for managed armies (gap G8) |
| 167 (6.14) | The "respawn all" checkbox revives the named character | guide wrong (exe says activation revives all) | 0x4969b8 has no flag test in its revival loop. The flag only changes the AI respawn (`ai.md §12`, `events.md §7.2` step 7) |
| 167 | The tech army must start off the map; if it is on the map when the event activates it, nothing happens and the character stays dead | confirms | `events.md §7.2` step 7 (off-map only). The guide's wording is unclear |
| 165-166 | Deactivating the temporary army leaves the building as it was: owner, faction and relations stay with the activation | confirms | 0x496900 and 0x496774 do not write the owner byte; `script.rs:730-742` |
| 164 | No vanilla alliance change: vanilla events have no faction or attitude result; only the Community opcodes 9 and 10 change them | confirms | `events.md §7` (no such result), §15; `ai.md §15` |
| 164-167 | An army activated and deactivated in one tick "crashes"; the 1-hour delay is the workaround | open | No fault path in 0x4969b8, 0x496900, 0x496774 or 0x48eeb0. The Community fix credited in the guide is not in our `community-patches.md`. OQ "same tick" |
| 148 | A killed ghost comes back at its next activation, on its start cell; deactivation clears its destroyed flag, so it does not respawn alone | confirms | `ai.md §12`; `events.md §7.2` step 7 |
| 150, 161-163 | A named character moved by an event moves with its whole record: level, XP, HP, items and spell effects | confirms | `events.md §7.1` ("taken from an army") |
| 161-163 | Named units cannot go into a garrison; they can be dismissed, which drops their worn items, personal ones included (not in the guide) | confirms; the dismissal loss is new to the guide | `economy.md` (garrison moves, dismiss) |
| 150 | Personal items cannot be taken off | confirms the behaviour | `interface.md` ("ItemI" hover). Our docs disagree on the offset of the unit record's +0x18 byte (OQ "+0x18") |
| 150, 161 | Spells the editor gives to AI armies run "for good": the map load writes the end minute 1,036,800,000 to each unit's spell slot | confirms | 0x4b2504 (`magic-items.md §4.2`) |

### AI, respawn, wages and levels (6.19, 6.16)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 192 | An "ignored by AI" army is not attacked by AI armies and is not fought as its owner's army | confirms | `ai.md §8`, §9.1 |
| 192 | "Army beaten by anyone" means destroyed or beaten-by | confirms | `events.md §4` row 7 |
| 192 | Friendly attitudes (0 or more) stop AI-vs-AI fights | confirms | `ai.md §3`, §8 |
| 192 | A respawn needs a home (feudal armies may fall back to a town, castle or fort) and the delay is in days; an activation revives at once | confirms | `ai.md §12` |
| 193 | The cost field: the second number is the daily wage bill; the gold income field is byte 80 × 10 | confirms | `economy.md §1`; `ai.md §1, §14` |
| 193 | A stationary army never earns income or pays wages; a moving army's noon runs at its first arrival after 12:00 | confirms the second (new caveat) | `ai.md §14` |
| 194 | AI units promote after each XP gain once their level reaches the next tier's requirement, and drop to level 0; only surviving units gain XP; hero units are promoted by hand | confirms | `ai.md §10` step 3, §11; `experience.md §4` (the player's promotion is manual) |
| 194 | Remedies: extra levels on units at the end of their branch, or keep the armies out of fights | confirms | `ai.md §8`, §11 |
| 175, 186 (6.16, 6.17) | "Market and army behaviour use a cache that depends on the player's actions" | guide wrong | There is no per-player cache. One LCG (0x4832fc) is seeded to 1 at map load and is not saved (`engine.md §3.2-3.4`, `saves-data.md §11`). Its draws come from midnight restocks (0x4be178), AI wander points (0x4a2550), promotions (0x4a4a7c), village offers (0x4bba40), each hero stop (0x4ad8a0) and the real-time music rotation (`src/ui/mod.rs:746`). The only cache files hold parsed list headers, with no gameplay caller; deleting them changes no game behaviour |
| 186 | Per-player divergence after hour 6 | unclear | Plausible from the specs (the player's stops and the music draws); not tested |
| 183-184 (6.17) | The decrement recipe corrupts a flag: the decrement has no lower bound; once the counter reaches zero and its entry is removed, the next `-SEED^` lowers the byte at position len("SEED") in the flag string | new, exe; the guide's "random walk" is this bug | 0x4ab2a3 (the `-X^` branch takes the position without a zero check). Razdor skips it (`events.rs:1054`): deliberate, D4. OQ "decrement" |
| 178 (6.16.2) | The increment guard prevents "a copy of the counter" | unclear | `+X^` already checks for presence; the missing check is the lower bound (see above) |
| 179-180 (6.16.3) | GENSEED contains SEED: a test for `SEEDn` is true whenever `GENSEEDn` is set; `+SEED^` raises the first SEED occurrence, which is inside GENSEED when GENSEED was added first | new, exe; Razdor reproduces it | 0x4ab2a3 (first-occurrence search) with `events.md §5` (substring test). `events.rs:998-1075`. A trap to document |
| 187-189 (6.18) | Fishing in a shipyard: shipyards list every event, rumours included, so the nine same-named events fire one per visit, and the flag army deactivates after one | confirms | `events.md §2` (shipyard rule), §10 |
| 190-191 (6.19.1) | Disconnected quests: a completion must test that the quest was taken | confirms | `events.md §4` row 2 |

## Chapter 7: Subtleties and limits (pp. 195-224)

Summary: most map limits and the counter arithmetic match the exe. The guide's playability section is
wrong about the algorithm (the editor's scorer is known from the exe, on the `dt-editor` branch) and
about several scoring terms. Tribute is wrong on the classes. Several limits are new to us and their
checks are open (the validator gap E8).

### Limits and multipliers (pp. 195-196)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 194 | A high-level AI unit promotes after any battle, so a shown army loses its strong units | confirms | `experience.md §4` (the AI promotion roll runs after every gain) |
| 195 | Events: 5000 (editor 1.3.14 and later), 1024 (1.3.13), 512 (1.3.12 and older) | 5000 confirms; the version split unclear | `src/editor/events.rs:17-18`, `src/editor/doc.rs:674` |
| 195 | Armies and event points: 255 each | confirms | `src/editor/validate.rs:72`; `doc.rs:563`, `:590` |
| 195 | An event point holds five events | open | The validator checks against the 10-slot array (`src/dt/dtm.rs:950`), so the guide's 5 is not what it enforces. Which is real is open (gap E8) |
| 195 | Named characters: 32 | confirms | `doc.rs:623`; `validate.rs:202` |
| 195 | Local events or rumours in one building: 64 | confirms (the validator checks it) | The building's array holds 64 (`dtm.rs:500`); `validate.rs:266` checks against it. Not a gap |
| 195 | Gold, XP and mana grants up to 32767 | confirms | `events.md §7.2` items 4-6 (signed 16-bit fields) |
| 195 | A flag name needs two characters at least; 20 or more characters can cause memory errors | minimum confirms; the memory claim new, unverified | The validator checks only empty names, `%`, `=` and spaces (`src/editor/events.rs:186-197`). The flag buffer size is to check (OQ "flag name length") |
| 195 | An event picture without text: 581 × 563 | new, unverified | The validator checks the data size and a 64 KB cap only (gap E8) |
| 195 | Player inventory: 253 | new, unverified | Gap E8 |
| 195 | XP per battle per unit: 2147 in vanilla, 5256 in Community | 5256 confirms; 2147 new, exe | 5256: `experience.md §4`, `src/rules/experience.rs:14`. 2147: the overflow bound of the 32-bit product (0x4c5676-0x4c569d). The reset claim is open (OQ "XP above 2147") |
| 195 | Characters, artefacts and spells: 255 each (modding limits) | new, unverified | |
| 196 | VictoryGoldDiv = 2 and MinVictoryGold = 25 | values confirm; the rule is guide wrong | The player's loot has no minimum (chapter 4). `mechanics.md` said it did: fixed 2026-11-05. The AI rule is in `economy.md §3` |
| 196 | ResurectConst = 300: resurrection costs three times the unit's cost | confirms | `economy.md §2` (resurrection) |
| 196 | HealingConst = 50: a full heal costs half the cost | confirms | `economy.md §2` (healing) |
| 196 | BattleEndTurn = 25; some mods use 15 | confirms | `mechanics.md` (`BattleEndTurn`); `src/rules/content.rs:721` |

### Counters (pp. 197-199)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 197 | Flags and counters are one string; a counter is a name plus one character; `+X^` raises that character | confirms | `events.md §5`; 0x4ab2a3 |
| 197 | 207 is the last value before the byte reaches 0xFF | confirms (arithmetic) | 48 + N; 0xFF is `я` in cp1251 |
| 197 | Past 207 the byte wraps to 0x00 (NUL), not to a further printable value | new, exe; Razdor matches | 0x4ab38e-0x4ab39d (the byte wrap); `src/rules/events.rs:1042` (`wrapping_add`) |
| 197-198 | A counter is removed when its digit goes from 1 to 0 | confirms | `events.md §5`; `events.rs:1056-1060` |
| 198 | Negative counters are not worth it | confirms (no path below 0) | No spec entry |
| 198 | `=` inside a result name is lost in the editor, so such a counter cannot be made | confirms the syntax; the editor erasure unclear | `events.md §5`; the validator rejects `%` and `=` in names (`src/editor/events.rs:190`) |
| 199 | A counter test "RAND^" needs an extra trailing `^` ("RAND^^") | guide wrong (exe) | The exe's flag test drops only the first `^`: "NAME^^" becomes "NAME^", which never occurs in the flag string, so the test never passes. "NAME^" tests any value of NAME (a prefix). Razdor matches (`events.rs:998-1013`). Settled (Settled list) |

### Healing without hiring, ghosts, mods (pp. 200-203)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 200 | An empty first hire slot lets a building heal but not hire (a bug) | open | The game's tab test opens the tab when any of the six slots holds a unit (0x4bbc84, slots at 0x105 + 3i), so an empty first slot alone does not hide it. The hire-row builder was not traced. OQ "hire row" |
| 200 | AI armies still hire in those buildings | confirms | `economy.md §8`; `src/rules/ai.rs:3169-3171` |
| 201 | An army can serve as a flag: active means on the map and not destroyed; inactive means off the map and not destroyed | confirms | `events.md §4` rows 5-6 |
| 201 | "Army in its home" does not depend on activity | confirms | `events.md §4` row 10 |
| 201-202 | Crash when one army is deactivated and activated in the same tick | open | OQ "same tick" |
| 202 | Counter-based procedures for many flags in one tick | advice | |
| 203 | The editor reads the game's config files; a map made on a mod works in vanilla only if it uses no mod content; installing a mod that renumbers characters silently re-targets event references | new, unverified risk | Our validator checks unit and artefact ids against the install's names (`validate.rs:121-135`); the renumbering risk is not documented |

### Ships, tribute, named characters (pp. 205-212)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 205 | The ship is the last entry of the leader list; its leader type is the marker 0xFE (254) | confirms | `world.md §8`; 0x4969b8 and 0x496900 compare the first unit type with 0xFE |
| 205 | Activating the army places the hero's parked ship; deactivating removes it | confirms | `world.md §8`; 0x496ec4 (place), 0x49704c (remove) |
| 205 | Re-summoning does not remove the earlier ship; a ship on a cell where another of his ships sits cannot move | new, exe; gap G9 | 0x496ec4 fills the single parked-ship record and sets occupancy bytes on the new cell, but never clears the old cell's bytes. Only 0x49704c clears them, so the old cell stays blocked. Whether two ships are drawn at once is unclear (OQ "second ship") |
| 205 | The summon is refused while the hero stands in a shipyard (type 9) | new, exe | 0x496ec4 (first test): the guard is "building type 9 and army index not 0" |
| 205 | Walking onto a summoned ship on land blocks movement | new, follows from the stale bytes | |
| 206 | Tribute is refused for heroes of the lowest bandit or undead classes | guide wrong (exe says Rogue only) | Only the Nature Rogue (value 3) is tested (0x4bbd69 and 0x4bbde6). Undead is not tested. Razdor matches (`economy.rs:770-778`). Settled (Settled list) |
| 206 | The hero's class can be changed mid-story to unlock tribute | advice | `events.md §7` (class change) |
| 207 | A named character moved into another army becomes its leader, shown in battle only | confirms the record; the map-side name unclear | `events.md §7` (units added; 0x4961d0). The army's name on the map is not traced |
| 208 | An army on a bridge is open to ship attack and cannot attack ships, for the player and the AI | unclear | No rule that ships engage bridge armies was found (`world.md:127`, `:322`). See the existing "bridge" entry |
| 210 | "Any unit" takes the last-joined unit that has no name | confirms | `events.md §7` (code 255 after the hero, no name); 0x4961d0 tests name zero and index above 1. The exe's code number is open (OQ "unit codes") |
| 210 | "Added unit" takes the unit whose class matches the last unit the events gave, and cannot take one that changed class | guide wrong (exe) | 0x4961d0 selects the last unnamed unit whose wage kind is 3 (an event unit). No class memory is involved. Our `events.md §7` describes the same search for code 254 (the event unit). The numbering differs: OQ "unit codes" |
| 211 | Taking the hero's unit out of his army can make the earliest-joined character the leader | unclear | The hero's leader choice is not traced |
| 211 | A named character given to the player becomes the hero's name, keeps its personal items, and its spells vanish if the hero dies | new, unverified | |
| 211-212 | Emptying the hero's army with an event crashes the game if the army window is then opened | new, unverified; open | OQ "empty hero army" |
| 212 | Named characters can exist without a class; the class can be chosen when the character is given | new | Not in our specs |
| 212 | Taking a classed named character takes the last one with that name, whatever its class; the named-presence check ignores class | confirms | `events.md §4` (named squads), §7 (named slot) |

### Playability (pp. 213-214)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 213 | Playability is an algorithm whose exact form is unknown | guide wrong (the scorer is reconstructed) | The editor's scorer was reconstructed from the exe: 0x5a0724 (scorer), 0x5a1430 (terrain counts), 0x5a2ba4 (curve), 0x5a04d8 (label). On the `dt-editor` branch only (`docs/reference/editor/mapcheck-files.md §6`) |
| 213 | Small maps reach high percentages much faster (density) | confirms | `dt-editor` mapcheck §6.3 (counts over (W div 50)^2; percentages over (W − 6)^2) |
| 213-214 | Growth is non-linear; each step needs more events | confirms | `dt-editor` mapcheck §6.7 (square-root curve) |
| 214 | A ceiling of about 300 per cent for most maps; 200-250 per cent as the quality mark | unclear | The curve has no hard ceiling (slope 0.75 above 100) |
| 214 | Events weigh heavily; events count as different when their conditions differ | guide wrong | The exe compares events by message and question text (word-level distance; near duplicates divide the event's points). Conditions are not compared (`dt-editor` mapcheck §6.2) |
| 214 | Quests weigh heavily; a quest no event completes costs points | confirms | `dt-editor` mapcheck §6.2 (quest type 40 points), §6.6 (uncompleted quest -50) |
| 214 | Landscape variety counts: tile and tree types | partly | Only the land and water uniformity counters are scored (mapcheck §6.3). The terrain-code variety counters are reported, not scored |
| 214 | Giving items and characters by event lowers the score | guide wrong | Each gained artefact adds its value (+2 once if any); gold gains add. There is no term for characters (mapcheck §6.2) |
| 214 | Armies raise the score even when inactive | confirms | mapcheck §6.2 (+5 per army, no activity test) |
| 214 | Longer texts help | confirms | mapcheck §6.2 (text term) |
| 214 | Building descriptions and named characters do not count | confirms | mapcheck §6.2 (no such term) |
| 214 | The garrison's makeup does not count | guide wrong | A ruin with no first garrison unit and owner byte 0 costs 3 points (mapcheck §6.6) |
| 214 | Example scores (for example Arnor 140 per cent) | new, data | Not verifiable here |

### Paid characters, early respawn, settings (pp. 215-224)

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 215 | Named characters never pay | unclear | Kind 0 (hero and AI leaders) and kind 3 (event units) are never paid. The named-character rule was not traced |
| 215 | An event-given character that then leaves another army pays | open, leaning guide wrong | The record copy keeps the wage kind (`economy.md §1`, `events.md:657`), and event units are kind 3 (`push 3` at 0x4a94b3). OQ "wage kind" |
| 215 | An event-given character given directly is free | confirms | `economy.md §1` (kind 3, 0x4a94b3) |
| 216 | An activated dead army comes back regardless of its respawn settings | confirms | `events.md §7` item 7; 0x4969b8 (each unit's HP reset from 0, paid time set) |
| 216 | It returns without its worn items | confirms | `ai.md §12` |
| 216 | It returns at its start cell even when it has a building of its own; the building matters only if it is the army's home | confirms | `events.md §7` item 7; 0x4969b8 compares the home with the start-cell building |
| 216 | Only the leader comes back unless "respawn whole army" is ticked | guide wrong on activation | The record keeps only the leader after a player win, unless byte 83 is set (`ai.md §12`, `dtm-format.md`). The event activation revives every unit left in the record (0x4969b8). The box concerns the record, not the activation (Settled list) |
| 217 | Editor settings: the event text font size (small preferred); "new events are multiple" unticked by default | new, editor-only | |
| 217-218 | Battle tester: 12 units per side; the front row 6 wide, the second row 4, the third 2 (reserve) | confirms | `dt-editor` testers §2.1 |
| 218 | Random armies cost 2100 or 1100 per side | confirms | testers §2.3 |
| 218 | The top army is always AI; the bottom army is the player's | confirms | testers §2.1, §2.3 |
| 218 | Battle tester check boxes: super AI, all AI, delay | confirms | testers §2.1 |
| 219 | Battle tester options: five switches ("counter attack", "ranged attack", "one defender", "attack one", "movement and initiative"); the screenshot shows all unticked; our defaults have three on | open | The form controls are cbAnswer, cbDist, cbOneDef, cbAtkOne and cbMovIni. Only cbAnswer and cbMovIni map clearly. On the `dt-editor` branch. OQ "battle tester" |
| 219 | Movement tester steps: 6 minutes, 1 hour (10 steps) or 6 hours (60 steps) | confirms | testers §3.2 |
| 219 | The events check box: only global events run, and text events open as windows | confirms | testers §3.4 |
| 220 | Choice events always answer Yes in the tester | confirms | testers §3.4 (the viewer never records an answer) |
| 220 | Two armies can be pushed into the battle tester from the movement tester | confirms | testers §2.1 |
| 221 | The hero's class can be chosen in the movement tester | confirms | testers §3.1 |
| 222-224 | Keep versioned copies of the map (sync software); Community update fixes; protection gate (a global event that checks for 32000 mana, which vanilla never reaches) | advice; the Community mana condition confirms | `community-patches.md` (mana condition added in Community, c25200). The vanilla "XP resets to 0 above the cap" claim is open (OQ "XP above 2147") |

## Chapter 8: Making a good map (pp. 225-257)

Summary: this chapter is mostly editor advice. Its checkable parts are the impassable terrain, the
draw order and the balance notes, and most of it is advice. The draw order of the game is unverified: we
draw buildings above trees and stones in the same cell, and the guide's screenshot suggests the opposite
for stones. The editor's draw order is a known difference (gap E7). The condensed advice is in the
section "Map design advice" below.

| Guide p. | Fact | Verdict | Evidence |
|---|---|---|---|
| 225-228 | Forests: use only 1×1 or 2×2 brushes; a larger brush gives a dense, uniform blob | partly wrong | The editor gives each cell a random member of the family when the size is above 1 (`dt-editor` main-window §10.3), so sprites differ. The real problem is the density of the stamp |
| 227-229 | Forest recipe: one to three species; a 2×2 pass of impassable variants for the outline; thin random segments; small passable variants; dead trees and stones at 1×1; vary the ground | advice | |
| 241 | Do not mix very different species (blue spruce with yellow trees) | advice | |
| 230-232 | Swamp recipe: 2×2 swamp base; impassable swamp, grass and water; reeds and graves at 2×2 (some reeds have no 1×1 variant) | advice; the no-1×1 claim is data, not checked | |
| 232-233 | Mountains: stamp 5×5 first, then 2×2 and 1×1, from the centre out; do not repeat identical pieces; ridges with bends and branches; hills, stones and firs at the foot | advice | |
| 234-235 | Water: a shore gradient or a noisy mixed texture; lakes as a gradient to a deep centre; rivers as short curved strokes; no deep water where a path is needed | confirms the impassable deep sea (terrain 2) | `dt-editor` mapcheck §1 |
| 236-237 | Plains: texture with low grass patches, little vegetation, a few hills and stones, a lake or rock in the middle of a large plain | advice | |
| 238-239 | Decoration: grass under villages becomes road; castle floors become stone; clay on rarely used roads (with gaps); dry grass for fields; lava under altars for blood | advice | |
| 240 | The game's draw order: ground, hills, buildings, mountains, trees, stones (later covers earlier); the editor's order may differ | unclear for the game; open | Razdor draws buildings above mountains, rocks and trees in the same cell (`src/ui/world_view.rs:1640-1655`, `:1682-1686`). The screenshot shows stones over a town. Gap G12, OQ "game draw order" |
| 240 | The editor's draw order: hills of classes 1-4 under everything; then per cell forest, mountains and rocks, buildings, figures | guide right; Razdor differs | The exe order (`dt-editor` main-window §8.1, editor 0x5ab588). Razdor's canvas sorts objects, buildings and armies by y only (`src/ui/editor/canvas.rs:278`): gap E7 |
| 241-243 | Bad examples: editor-made maps with too many hills and sand beaches; forests of a single 3×3-plus brush; empty maps with no obstacles and no points of interest | advice | |
| 244-245 | Good examples: mixed forests; ridges that cut the map; islands joined by ships | advice | |
| 246-247 | Game design: lanterns to guide the route; cut backtracking; reward side exploration; dose the story; avoid event density; manage army costs; avoid sudden deaths and long reload chains; keep text, battle and puzzle modes in balance | advice | |
| 247-248 | Story: the player must know who he is and what he wants; the world changes by activating and deactivating armies; characters speak with bias; do not use generated text | advice; the world-change method confirms `events.md §7` item 7 | |
| 249-250 | Balance: the tactical cost over-values regeneration, vampirism and magic resistance; keep a 500-1000 gap between story battles; initiative and damage are the key stats; 3-4 key threats per army; counterblow units answer flank strategies; healers can have extra levels; experience correction for overpaid armies; avoid invulnerable heroes and the blessing cross on hard maps; avoid unwinnable battles | the cost bias open; counterblow and flank confirm (`experience.md`); the rest advice | OQ "tactical cost" |
| 251 | Event management: colour categories by quest line, comments on the closing event, dash names for subordinate events | advice (editor UI) | |
| 252-253 | Technical maps: procedural spawning with text-less timed events; economy simulations; tower defence | idea list; the delay path confirms `events.md §8` | |
| 254-255 | The grammar pass: export text, edit, re-import; the game shows only a fixed glyph set; re-import needs the same file name and intact line breaks | confirms the glyph set (`engine.md §5`, up to 255); the export tool is new (editor) | |
| 256-257 | Publication: a description template (description, author, version, settings), a disclaimer event at the start, version numbers in the file name, first comments with hints | advice | |

## Chapter 9: Known bugs (pp. 258-262)

Summary: the guide's own list of bugs. Most are new to us and unverified; several are confirmed or
disproved by the exe. The events-without-text claim (9.15) is wrong, and the flag-prefix bug (9.16) is
ours too: it is the substring test, which we reproduce on purpose.

| Guide p. | Bug | Verdict | Evidence |
|---|---|---|---|
| 258 | 9.1 Stale building descriptions after opening another map (editor) | new, unverified | Editor-only |
| 258 | 9.2 Negative tactical costs crash the game; a hidden undead unit can join the battle | open | `experience.md §1` has no overflow note. A different mechanism from the "negative vampirism" entry |
| 258 | 9.3 A crash in battles with high-level units, fixed by replaying the battle | new, unverified | |
| 258 | 9.4 Activating an army on an empty map can crash | new, unverified | Open (crash group) |
| 259 | 9.5 A player army next to an allied army at the noon report can fight it | new, unverified | `ai.md` contact rules do not mention noon |
| 259 | 9.6 A crash from route memory on big maps with free-moving (unpatrolled) armies | new, unverified | Razdor has no memory limit to compare |
| 259 | 9.7 Random crashes, cleared by deleting the cache and extra saves | new; doubtful | The cache files hold list headers with no gameplay caller (chapter 6). Deleting them changes no game behaviour, so this fix is doubtful |
| 259 | 9.8 A rumour with no text plus a relative event at 0 hours causes visual artefacts inside a building | new, unverified | Same as 5.10 |
| 260 | 9.9 A lantern, army show or spell in the same event as another effect: the first runnable event fires without its conditions being checked | open | `events.md §8`; check 0x4ab1ec and 0x4af658. OQ "lantern, show or spell" |
| 260 | 9.10 A delayed event with a chained event: the chain is dropped | confirms | `events.md §8` ("a chain behind a delay is dropped") |
| 260 | 9.11 A wait plus a text window inside a building: only Esc closes it | new, visual | |
| 260 | 9.12 A patrol that leaves the bottom edge crashes when it routes | new, not reproduced | Razdor clamps the wander box to the map (`ai.rs:2004-2011`): deliberate, D1 |
| 261 | 9.13 "Start battle with army" in a building, or with a text event, overlaps the interfaces | new, UI; open | Chapter 5, 5.12 |
| 261 | 9.14 Custom alliances can crash | new, unverified | |
| 261 | 9.15 Events without text lose many functions | guide wrong | The exe applies gold, mana, units, artefacts and spells without text (5.8). `events.md §8` says silent events apply their results at once |
| 262 | 9.16 The flag check matches by prefix: "SAD" is found when "SADDLE" is set | confirms (the substring test) | `events.md §5`; the exe test (first occurrence); Razdor `events.rs:1005-1013` keeps the same behaviour on purpose. The guide's workaround (same-length names of at least five characters) does not remove the substring test, since AB matches XAB |

Chapter 10 (p. 263) is the author's final page. It is not analysed.

## Where the guide is wrong (exe-settled)

A quick list of the guide's claims that the exe settles against it. Each is detailed in its chapter.

- Church potions: only towns get the potion step (0x4be178). Churches get potions through the normal candidate list.
- Potion count: R div 5 + 1, where R is the random count less the fixed goods still present; not the total number of places.
- Castle and fort payout: min(income, max) (0x4a41d8, 0x4a1998); the max does not need to match the income.
- Ruin treasure: no halving on reward (0x4c50ec: stock + garrison gold + one day's income); growth is a square root (0x4a1998); no noon payment.
- Player's loot: gold ÷ VictoryGoldDiv with no minimum (0x4c50ec); MinVictoryGold applies to AI-against-AI fights only.
- "Units carry no money" (byte 62) keeps feudal armies alive: no; the flag is read in the loot code only.
- Feudal respawn: only a town, a castle or a fort (0x4a28d0), not any building.
- Rogue armies: they get castle, fort and linked-village income, like every army (0x4a41d8).
- Learned spells: carried by the experience and level flag (byte 3), not the pack flag (0x4b5b64).
- Start date before the scenario start: the event fires (0x4a7b80 clamps a negative start to 0).
- Local events: the interval (P and N) applies to them too (0x4a7b80; the scan 0x4abfbc).
- Points: rumours and every other type fire at a point (0x4abfbc).
- Relative call on a repeating event: writes the start only, not N = 24 (0x4ab51b).
- Move to hero: the cheapest neighbour, not the first passable cell (0x4980d8).
- Own-army event spell: 10 × TimeWork hours below TimeWork 8, 5× from 8 (`magic-items.md §4.1`); not a flat 5×.
- Full spell book: no capacity test, the spell is appended (0x49c144).
- Function table, spell row (p. 137): the guide marks learned spells as needing text. The exe applies them on a text-less event (0x49c144). Only "activate spell" works without text in the guide's table and in the exe.
- A counter's `^` form works on any name, not only names ending in a digit (0x4ab2a3).
- "Current level" compares the hero's 0-based level, not XP (0x4a80b4 → 0x4a7b40).
- "Holiness and mana" is not always true: the exe compares mana (0xc2875e).
- Current level and holiness and mana (p. 90): the guide's blue note says both are repaired in an unofficial community fix (chapter 7.17). The exe checked here is the vanilla build; Razdor matches the vanilla behaviour, so the note does not change the verdicts.
- An army with no home passes the "in its home" test (0x4a7b80 tail).
- Text-less events: gold, mana, units, artefacts and spells apply (0x4ab51b, 0x4a8ae8, 0x4a9b75, 0x4a8e66). Gold and mana need no text.
- Units moved into a full army: appended while there is room; then the copy overwrites the last record (0x4a8fe5).
- Colour markers: a fixed precedence, `@` over `|` over `*`, wherever they stand (0x48e1cc).
- "RAND^^": the exe's flag test drops only the first `^`, so `NAME^^` never matches.
- Tribute: refused for the Rogue nature only (0x4bbd69, 0x4bbde6); not for undead or bandits.
- The "added unit" removal takes a unit by wage kind 3, not by class (0x4961d0).
- The "respawn whole army" box does not revive a named character on activation; activation revives every unit left in the record (0x4969b8).
- The playability score is not unknown: the editor's scorer is known from the exe (0x5a0724 and its helpers; `dt-editor`). Giving items adds to the score rather than lowering it, and a ruin's garrison makeup does count.
- The ambush's "no gold" case: a Yes with no gold passes for free (the guide's recipe has no gold check; the result clamps at 0).
- Church price window: shipped churches use 250; the guide's "vanilla 245" does not match the data.
- The cross-reference "the base price table is in chapter 3.3" points to nothing.
- The randomness "cache": there is none; one LCG, seeded at load, draws from the listed sources (0x4832fc).
- The 6.14 "respawn all" checkbox revives nobody on activation (0x4969b8).
- The guide says the "same-tick" crash exists; no fault path was found in the four routines. Unverified, open.
- Claims the guide does not make, removed from chapter 5 (2026-11-05): a rumour with a price has a negative gold result; a full pack loses an artefact silently (the guide shows four slots only, pp. 105-106); the village and shipyard list for rumours and quests (the guide defers to chapter 3.3, p. 82); "the weakest unit" for a full army (the guide does not say it).

Our docs that were stale, now corrected (2026-11-05):

- `docs/reference/mechanics.md` (the building tabs): altars hire for everyone (the hire tab has no type test), not for the AI only; a building of attitude below 0 still hires and trades (the hire and tribute tabs have no attitude test).
- `docs/reference/mechanics.md` (rewards): the player's loot has no minimum; `MinVictoryGold` applies to AI-against-AI fights only.
- `docs/reference/dtm-format.md` §13: the passability open item is settled from the exe (`original-mechanics/world.md §1.1`).
- `docs/reference/dtm-format.md` §6, byte 357: the editor writes "garrison is AI only"; the game never reads it.
- `docs/reference/original-mechanics/events.md`, "Razdor now" row "When results apply": was "Matches"; now a difference (gap G1).
- `docs/reference/dtm-format.md` §8, byte 5 (the point model): 8 is the lantern (radius dialog), 9 the event point (point window, with "active at start"), 10 the AI target point (records §10 and the screens, pp. 128-129). The old "9 = event point or inactive lantern" was wrong.
- `docs/reference/dtm-format.md` §9, bytes 16-17: moved from Conditions to Results. They are the army whose patrol changes and its signed delta (the Результат 2 tab, "Изменить патруль армии", p. 124).

## Razdor gaps found

Each gap was checked against the current source on `main` (the cited line was grepped and says what the
report claimed). Two report claims did not hold, so they are not listed as gaps: the missing check for
64 local events per building (`validate.rs:266` checks it), and the 5-events-per-point limit, which is an
open question rather than a gap (the validator checks against the 10-slot array, `dtm.rs:950`). Each gap
to fix has an entry in `docs/OPEN_QUESTIONS.md` as a known difference (the 2026-11-05 entries). The
deliberate deviations (D1 to D5) are kept as they are; only D4 has an open entry, for the owner's decision.

### Editor (src/ui, src/editor, src/dt)

- **E1. No control for the army's daily gold income.** Byte 80 is the income in tens (`src/dt/dtm.rs:797-798`, named `unknown_80`), read by the AI as `extra_income` (`src/rules/ai.rs:210`). The troops tab (`src/ui/editor/props.rs:253-263`) has only "Starting gold". Add the control (display ×10, stored 0-250, step 10) and rename the field.
- **E2. Misnamed army field.** `src/dt/dtm.rs:739-740` `gold_income` is the starting gold (byte 17), bound to "Starting gold" at `props.rs:259`. Rename it with E1.
- **E3. "Garrison serves the AI only" does nothing.** `src/ui/editor/props.rs:149` shows it; `src/editor/defaults.rs:71` and `src/editor/doc.rs:1021` set it; `src/dt/dtm.rs:557, 609, 652` read and write byte 357. No code in `src/rules` reads it, and the exe reads no offset 0x165. Remove the checkbox or label it "no effect in the game". `dtm-format.md §6` row 357 is corrected.
- **E4. Editor ranges wider than the original's.** `props.rs:148` garrison defence 0-255 (the original's trackbar is 0-50, and its save clamps values above 50; six shipped buildings hold 65, 75 or 100). `props.rs:263` garrison strength 0-255 (original 0-90). `props.rs:274` speed correction -10..10 (original -3..5). `props.rs:269` aggression -128..127 (original -100..100 step 10). `props.rs:282` respawn 0-255 (original 0-30). `props.rs:284` experience correction 0-255 (original 10-250 step 5). `props.rs:273` patrol radius 0-255 (original 0-250 step 5). `props.rs:188` income per day 0-2500 (original 0-250 step 10). `props.rs:189` most kept 0-25000 (original 0-2500). The original's ranges are in the `dt-editor` branch's `records.md §2-3`. Clamp, or document that Razdor keeps the wider values on purpose.
- **E5. Ruin treasure shows six slots; the game takes five.** `src/editor/records.rs:36-37` (`GOODS = 6`) feeds the ruin slots at `src/ui/editor/props.rs:154-155`. The game uses the first five (`src/rules/world.rs:1127, 1131`, `goods(5)`). Show five for ruins.
- **E6. "Most kept (villages)" label.** `props.rs:189`. The stock maximum applies to any building with a max (`src/rules/economy.rs`, 0x4a1998; `economy.md §3`). Relabel.
- **E7. Editor draw order.** `src/ui/editor/canvas.rs:249-300` sorts terrain objects, buildings and armies by y only (the sort at line 278). The original editor draws hills of classes 1-4 first, then per cell: forest, mountains and rocks, buildings, figures (`dt-editor` main-window §8.1). A visible change to the editor's map view: UI, so the owner decides.
- **E8. Validator gaps** (`src/editor/validate.rs`, `src/editor/events.rs:186-197`): (a) no flag-name length rule; (b) no warning when a test is a substring of another flag name (the 9.16 trap; the runtime keeps the same behaviour); (c) no warning for a counter test ending in `^` (the "RAND^^" pitfall); (d) no check on a picture's size against 581 × 563 (the validator checks the data size and a 64 KB cap); no check on the inventory size (253); the point limit is checked against the 10-slot array (`dtm.rs:950`), while the guide says 5.

### Engine (src/rules, src/ui)

- **G1 (medium). A message event applies its finish-time results when its window opens.** `src/rules/events.rs:1455` (`open` calls `show`, then `finish`), `:1472-1474` (`show` sets `held`), `:1194-1195` (Yes with a message: `show`, then `finish`). The exe applies the finish-time results at OK (0x4c206c calls 0x4ab1ec). Affected: gold, mana and XP, activations and deactivations, patrol, lanterns, shown army, flags, journal, relative starts, and the victory or defeat latch. The battle, spell and delay start at `events.rs:1655, 1658, 1661` under the window, where the exe queues them until OK. Fix the path and add a test (events.md S2). Owner's call whether this goes on the experimental track, since it changes when a map's results land.
- **G2. A second battle in one tick overwrites the first.** `src/rules/script.rs:827-833` (`start_battle` sets `foe` and pushes the encounter) is called from `finish` (`events.rs:1654-1656`) while `run` (`events.rs:1277-1288`) goes on; `held` is set only for a message (`events.rs:1466-1473`). A battle-only event with no message does not stop the scan, so a later battle event replaces `foe`. The exe ordering is open (OQ "two battles").
- **G3. The ranger's noon heal uses the current class.** `src/rules/economy.rs:483-491` (`ranger_heal`) calls `hero_class()` (`src/rules/game.rs:1021-1023`). The exe reads the start-class global (0x68dccc) at the noon scan (0x4abfbc) and in the noon report (0x4ac427). After an event class change (`set_hero_class`, `src/rules/script.rs:818-824`, which only retypes the unit) the two differ. The doc comment at `script.rs:812-817` says the class bonuses are lost on a change; that is wrong for archmage and ranger, which keep the start class (`game.rs:1069-1085`, `magic.rs:368-373`).
- **G4. Show army does nothing for a dead or inactive army.** `src/rules/script.rs:745-750` reveals only an on-map army. The exe (0x4ab51b) reads the record's current cell (+0x1724, +0x1728) for any army, so a dead army shows its death cell. The inactive case is open.
- **G5. Picture 202 (the met army's leader) is not drawn, and the editor cannot pick it.** `src/ui/story.rs:53-63` treats any value other than 0, 200 and 201 as a unit id, so 202 gives no picture. The meeting caption (`story.rs:133-137`) shows the leader's name only. The editor's list (`src/ui/editor/events.rs:98-101`) holds none, the two stock pictures (200 defeat, 201 victory) and the portraits of units below 200. The original's list also holds the install's global images from id 200 up, and every unit's portrait (records §9.1; the screen on p. 132 has a scroll bar). So the original can offer ids above 201 that Razdor cannot pick or draw. Which global images exist, and whether any shipped map uses 202, is open: the install's `.ini` files hold no picture keys under that name, so the list is not located yet.
- **G13. Picture windows take the event name as their title.** The screens of pp. 132-135 show the fixed title "Произошло событие!" for an event with a picture. `src/ui/story.rs:41` uses the event title for every window. This is a UI difference, so any change is the owner's call (OQ "Picture window title").
- **G6. Chain depth cap.** `src/rules/events.rs:34` (`CHAIN_DEPTH = 32`), used at `events.rs:1672`. A deeper chain is dropped silently; the exe has no limit. Emit an outcome or log it.
- **G7. Idle reset placement.** `src/rules/events.rs:1293-1301` resets the last-fired values at the end of `run`. The exe resets in each idle pass, including the nested passes that chain tails call (0x4abfbc tail, 0x4ab51b tail), so it can reset events fired earlier in the same tick. A check with a test (S10's variant).
- **G8. Managed armies skip the revival.** `src/rules/script.rs:340-343` returns an army that `ai::managed` rejects (`src/rules/ai.rs:423-425`, which is `a.ai.enabled`) before the revival, the building copy and the wander points (`script.rs:344-366`). The exe has no such test (0x4969b8). The default is true (`src/rules/ai.rs:203`), so only the save-compatibility path sets it false.
- **G9. The player's ship summon is not special-cased.** `src/rules/script.rs:305-320` (`activate`) places any waiting army through `placement` (`src/rules/ships.rs:133`). The exe treats leader type 0xFE as the hero's single parked-ship record (0x4969b8 → 0x496ec4; dismissal 0x496900 → 0x49704c). The 0xFE handling in `src/rules` is for event units only (`src/rules/events.rs:78-86`). So re-summoning and the stale occupancy at the old cell are not reproduced (open: "second ship").
- **G10. The village gate reads the base nature.** `src/rules/economy.rs:770-775` tests the hero's base unit nature. The exe reads the hero's current nature byte (0x75aa61). The two differ only if an item or a spell changes the nature. Low risk.
- **G11. Stale comments.** `src/rules/world.rs:791-793` says the archmage test is "probably meaning the Ranger"; the test is unit 2, the archmage, as the spec says (the speed question stays open). `src/rules/script.rs:1147` says the mana carry-over is a "(guess)"; byte 1 settles it (Settled list). Fix the comments; keep the tests.
- **G12. Game draw order of buildings (unverified).** `src/ui/world_view.rs:1640-1655` (the layers: `LAYER_BUILDING` 2 above the object layers 0 and 1, by `object_layer`) and `:1682-1686`. Buildings draw above trees, rocks and mountains in the same cell. The guide's p.240 screenshot suggests the opposite for stones. Check 0x4c8864 and 0x4c9459, or footage, before changing anything.

### Deliberate deviations (kept)

- **D1. Patrol boxes are clamped to the map** (`src/rules/ai.rs:2004-2011`). The original's out-of-map crash does not happen.
- **D2. The market hang is stopped after 10,000 rounds** (`src/rules/economy.rs:132`, `STOCK_HANG`, checked at `:674` and `:680`). The original loops forever when no artefact fits the price range.
- **D3. The mana add is not byte-wrapped** (`src/rules/economy.rs:221-225`, `grow_mana`). The original wrapped it, which only matters with a maximum near 255.
- **D4. `-X^` with X absent is a no-op** (`src/rules/events.rs:1054`). The exe lowers the byte at position len(X) (0x4ab2a3), which corrupts a flag in the 6.17 recipe. Razdor is the safer choice; a decision for the owner (OQ "decrement").
- **D5. The loop guard** (`src/rules/events.rs:32`, `LOOP_GUARD = 256`, used at `:1281`). A safety net only; the exe would loop.

### Checked and matching (not gaps)

- The flag test and action (`src/rules/events.rs:998-1063`) match 0x4a8237 and 0x4ab2a3, including the substring test.
- The counter wrap at 207 (`events.rs:1042`) and the removal at `0` (`events.rs:1056-1060`).
- The named-character cap (`src/editor/doc.rs:623`).
- Activation's revival for managed armies (`script.rs:344-366`) matches 0x4969b8.
- The friendly-army-in-building rule (`src/rules/game.rs:2152`, `:2160-2162`) matches the exe. The limit is in both engines.
- The "move to hero" cost rule (`src/rules/script.rs:758-790`) matches 0x4980d8.
- The AI's castle, fort and village income for every army (`src/rules/ai.rs:3669-3690`, checked and kept) matches 0x4a41d8. Do not "fix" it to match the guide.
- The tribute gate (`src/rules/economy.rs:770-778`) matches the exe's Rogue test (see G10 for the nature byte).
- The 5256 cap and the `_Global` reads (`src/rules/content.rs:721`, `src/rules/economy.rs:956-967`).

## Regression scenarios

Scripted checks for the event engine (the `events.rs` test style), taken from the analysis's list. Each
gives the setup and the expected result. Where a scenario documents a known difference, it says so.

- **S1. Ally dialogue (6.1).** A friendly army (attitude +2) on open ground; a global multi-event with a meet-army condition and a message. Expect: stepping on it gives a meeting (no battle), the window opens, and a second meeting fires again. Variant: the same army stationary inside a castle; expect no meeting (the friendly-in-building rule).
- **S2. Finish timing (G1).** A message event with gold +5 and an activation of army X. Expect (exe): gold unchanged and X off the map while the window is up, and both applied at OK. Razdor applies them at open until G1 is fixed, so this test documents the gap.
- **S3. Quest chain (6.2, 6.3, 6.19.1).** A once meeting event that chains a subordinate quest; a once completion event that needs "happened" (the quest taken) and a kill. Expect: the journal entry is added at OK; completion does not fire before the quest is taken.
- **S4. Cooldown rumour (6.4).** Duration 24, repeat 1 day, self-relative 24 h. Expect: fires at t0, not at t0 + 12 h, and again at t0 + 24 h. Variant: duration 0; the window closes at start + 60 min, so a hall visit later finds nothing.
- **S5. Merchant (6.5).** A question; gold ≥ 750 with current stats; repeat-after-Yes off. Expect: No gives times = 1 and answer = 1, and the offer is listed again on the next visit. Yes gives once = 1, the sword at show, and gold −750 at finish. With 749 gold it is not listed.
- **S6. Timed quest (6.8).** A quest chains an expiry at +72 h (once, duration 24, repeat 1 day); completion needs not-happened(expiry). Expect: completion at +71 h completes the quest and the expiry never fires; expiry at +73 h fires and blocks completion. Variant: duration 0 on the expiry; it never fires (no scan in [start, start + 60]).
- **S7. Lifetime ghost (6.6).** An off-map ghost; a daily 20:00 event activates it and relays +24 h; a 06:00 copy deactivates it. Expect: a ghost killed at 14:00 is revived at 20:00 on its start cell with full HP and pay reset; after 06:00 it does not respawn.
- **S8. Resurrection (6.14).** A named character dead in the hero's army; removed by name (code 6; dead counts as absent); given to an off-map tech army; activation. Expect: all its units revived and paid. Variant: the tech army already on the map when activated; expect no revival (F28).
- **S9. Alliance swap (6.13).** A temporary army with a home castle on its start cell; one event activates it and deactivates the friendly original. Expect: the castle takes the temporary army's owner, faction and relations, with no battle. After a later deactivation of the temporary army the castle keeps them.
- **S10. Procedure (6.15).** START raised by a silent subordinate call from a message event after OK; procedure events P1 to Pn (multi, each guarded by START); the last event removes START. Expect: all fire in array order in one scan; a second scan does nothing. Variant: a window-opening chained event inside P2. Expect: the scan stops; after OK, P3 to Pn fire, and P1 and P2 do not refire in that minute (G7 applies).
- **S11. Counters (6.16.1, 6.16.3, 6.17).** (a) `+SEED^` from absent, then reset at SEED9 with a plain `-SEED9`: expect SEED1, then the entry is removed. (b) Flags "GENSEED3 SEED2", then `+SEED^`: expect the GENSEED digit changes and SEED does not (the 6.16.3 trap). (c) `-SEED^` with SEED absent: no change in Razdor (D4); the exe differs.
- **S12. Messenger (6.11).** A peasant, friendly, ignored by AI, hunts the player, activated far away; a meet-army event with deactivation. Expect: it walks to the hero, greets him (the event fires), then deactivates. It never assaults a building on the way. Variant: patrol radius 0 (stationary); it never moves.
- **S13. Ambush (6.10).** Two mutually exclusive once dialogs (each "not happened" of the other). Expect: a No counts as happened, so the second contact fights. Variant: Yes with 0 gold passes for free (the documented gap in the recipe).
- **S14. Multi-outcome quest (6.9).** Four completions, each with `+QUEST` and `/QUEST`. Expect: exactly one completes. A flag named "QUESTX" blocks all four (the substring test).

## Map design advice

Condensed from the guide's chapter 8, for an editor macro, a map-quality check or tutorial text. The
checkable items are marked; the rest are advice. Thresholds are suggestions to tune on real maps.

Brushes and density
- Forests: default brushes 1×1 or 2×2; larger strokes make blobs. The editor gives each cell a random family member above size 1, so a checker should measure density and runs of one family, not sprite identity.
- Blob check (suggested): flag any 3×3 window where one tree family covers more than about two thirds of the cells, and any run of single-family trees wider than four cells.
- Mixed forest: one to three species per patch; no very different species in one patch.

Variety and uniformity
- The exe's uniformity counters (`dt-editor` mapcheck §6.3) are a ready-made test: a cell is uniform at level 1 when fewer than 2 of its 8 neighbours differ, at level 2 when fewer than 3 of the 24 nearby cells differ, and at level 3 when fewer than 4 of the 48 differ. Large uniform areas cost score: the thresholds are 400, 100 and 50 (land) and 800, 400 and 200 (water), as percent of (W − 6)², normalised by (W div 50)².
- Tile-type variety (terrain codes T1 to T3) is reported by the exe but not scored: use it as a warning.
- Empty zones: warn on windows (for example 10×10) with no objects, events, armies or points. The guide shows this as a bad example (p.243).

Impassable terrain (checkable)
- Impassable in the exe: deep sea (terrain 2), lava (3), impassable swamp (9), impassable snow (15), objects of classes 5 to 8 (mountains and rocks), and class 11 (thicket) (`dt-editor` mapcheck §1). Warn when a point, a route or a bridge lies on one.
- Water: keep a shallow ring between deep water and the paths; warn on a deep-to-land edge with no shallow cell between.

Draw-order hazards
- Warn when a building footprint shares a cell with a tree, a rock or a mountain. The game's order is unverified (G12); the editor draws by row and class (E7), and either can hide the building.
- Hills of classes 1 to 4 are drawn first in the editor, so they never cover anything. No check needed.

Surface and decor (optional polish, not errors)
- Grass under villages becomes road; castle floors become stone; rarely used roads get clay with gaps; dry grass marks fields; lava under altars marks the blood.
- Plains: a noisy grass texture with low patches, a few isolated trees or bushes, and at most one landmark (a lake or a rock) in a large plain.

Recipes (for editor macros or tutorial text)
- Forest: an outline with 2×2 impassable variants; thin random segments; small passable variants; dead trees and stones at 1×1; then vary the ground.
- Swamp: a 2×2 swamp base; mix impassable swamp, grass and water; dry trees and reeds at 2×2; optional stones and firs.
- Mountain ridge: 5×5 first, then 2×2, then 1×1, from the centre out; no identical pieces; bends and side spurs; hills and stones at the foot.
- Lake: a shallow ring to a deep centre. River: short curved strokes that follow the flow.

Design and balance (review notes; not checkable)
- Make the route obvious (lanterns); cut backtracking; reward side exploration; dose the story text; avoid event density; keep army upkeep in mind; avoid sudden deaths and long reload chains.
- Keep a 500 to 1000 gap between story battles; check initiative and damage; keep three or four key threats per army; avoid unkillable heroes and battles that cannot be won without losses.
