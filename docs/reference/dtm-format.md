# Discord Times (Времена раздора) `.DTm` scenario format

This is a reverse-engineered description of the scenario map format from Discord Times (Aterdux, 2004), as saved by the
Community Update editor. It is written for interoperability, so an engine remake can load maps the user already owns.
It describes the format only. No game text or data is reproduced here.

Sources: the 15 shipped maps were diffed against each other, and fields were matched against the editor UI labels
(`Rus_MapEdit.ini`), the editor manual (`Help/Документация к Редактору Сценариев.doc`) and the community modders' byte
notes (`Дополнительные файлы/Блокноты с ID/*.txt`). The notes use 1-based offsets. **Every offset in this document is 0-based.**

Confidence tags:
- **C** (confirmed): the decoder parses all 15 maps byte-exactly with this layout, and the values cross-check. Either the
  semantics were validated against other data, or they come from the modder notes and are consistent with the data.
- **L** (likely): the values are consistent, but the meaning is inferred.
- **U** (unknown): the meaning is not known, or the bytes are always zero.

All integers are little-endian. Text is Windows-1251 (cp1251) and NUL-terminated.

Reference implementation: `dtm_decode.py`. It parses every shipped map with zero leftover bytes and emits JSON.

---

## 1. Container

| off | type | value | conf |
|---|---|---|---|
| 0 | char[6] | `AIpf\r\n` | C |
| 6 | u8 | compression code: 19 in all maps = 10 + bzip2 level 9. Below 10 the data is a zlib (deflate) stream at that level; the Community editor writes its demo maps (`.DTs`) with code 9 (DTMapEdit 0x4d5790, 0x4c2938) | C |
| 7 | u8 | scramble mode, 0 in all maps (1 = XOR byte i with i+1, 2 = XOR with a random byte). The editor's demo maps use mode 1 | C |
| 8 | u32 | size of the uncompressed payload | C |
| 12 | … | a bzip2 stream (`BZh…`) running to EOF | C |

The game (0x473188) only checks bytes 0, 2 and 3 (`A`, `p`, `f`); byte 1 = `I` selects this
layout, and bytes 4–5 are ignored. Its saves use the same reader with `AEpf` and a block layout
(original-mechanics/saves-data.md §9). Bytes 6–7 were described here as a u16 "version" before.

## 2. Payload layout

The payload consists of the following parts, in order:

```
Header            0x000 .. 0x12E   (303 bytes, fixed)
Terrain RLE       terrain_rle_size bytes
Objects           objects_size     bytes  (6 per object)
Buildings         buildings_size   bytes  (358 per building)
Armies            armies_size      bytes  (89 per army)
Points            points_size      bytes  (99 per lantern / event point)
Events            events_size      bytes  (171 per event)
Custom artefacts  header 0x34 bytes (230 per artefact; always empty)
Text marker       08 3E 2D 54 65 78 74 2D   ("\x08>-Text-")
Strings           NUL-terminated cp1251 strings (see section 9)
Scenario picture  scenario_picture_size bytes (optional, a LIT image)
Event pictures    each event's custom_picture_size, in event order (optional)
EOF
```

A demo map from the Community editor (header 0x117 = 1, saved as `.DTs`) stores the same sections in the order
objects, custom artefacts, points, terrain, events, armies, buildings; the editor writes and reads that order, and
also writes a payload with no container at all when asked for an uncompressed save (DTMapEdit 0x5a50f2, 0x5a6fe0) (C).

The following invariants hold for all 15 maps (C):
- `0x12F + sum(section sizes) + 8 == text_offset`.
- The file ends exactly after the last picture.

## 3. Header (0x000–0x12E)

| off | type | field | conf |
|---|---|---|---|
| 0x00 | char[12] | `MapLDV V.4\r\n`. Byte 9 is the version digit; the Community editor also opens versions 1–3 and converts them (DTMapEdit 0x5a8424) | C |
| 0x0C | u32 | width in cells (50 / 100 / 200) | C |
| 0x10 | u32 | height in cells (all shipped maps are square) | C |
| 0x14 | u32 | random-generator seed from the "generate map" dialog. Several maps share one value. | L |
| 0x18 | u32 | `text_offset`: absolute offset of the first string, just after the text marker. The game seeks here before reading the strings and never reads the marker | C |
| 0x1C | u32 | terrain RLE size | C |
| 0x20 | u32 | objects size (a multiple of 6) | C |
| 0x24 | u32 | buildings size (a multiple of 358) | C |
| 0x28 | u32 | armies size (a multiple of 89) | C |
| 0x2C | u32 | points size (a multiple of 99) | C |
| 0x30 | u32 | events size (a multiple of 171) | C |
| 0x34 | u32 | size of the custom-artefact section (230-byte artefact records after the events, each with a name and a description string). Always 0: the Community editor reads the section but always writes 0 and drops the artefacts (DTMapEdit 0x5a5028, 0x5a76d1). Not read by the game | C |
| 0x38 | u32 | scenario start time, in minutes (see *Game clock*). 0 means unset. | C |
| 0x3C | 3 × 50 B | starting-hero presets: knight, archmage, ranger (see below) | C/L |
| 0xD2 | u16 | victory event (1-based event id, 0 = none) | C |
| 0xD4 | 4 B | always 0. Possibly further victory slots. | U |
| 0xD8 | u16 | defeat event (1-based, 0 = none) | C |
| 0xDA | 4 B | always 0 | U |
| 0xDE | i8[4][4] | global faction relation matrix, rows and columns in the order player, ally, neighbour, enemy. Values −3..3. | C |
| 0xEE | u8 | number of named characters (именные персонажи), N | C |
| 0xEF | u8[32] | unit id (class) of each named character. Only the first N entries are meaningful; the rest can hold stale data. | C |
| 0x10F | u8 | scenario kind: 0 = standalone, 1 = first map of a campaign, 2 = later campaign map | C |
| 0x110 | u8[7] | values carried over from the previous campaign map: gold, gods' favour, fame, experience/level, personal artifacts, whole inventory, whole army | L (order follows the UI) |
| 0x117 | u8 | demo flag: 0 in all maps; 1 in the editor's demo maps (`.DTs`), whose sections are stored in another order (section 2) | C |
| 0x118 | 4 B | always 0 | U |
| 0x11C | u32 | size of the embedded scenario picture (0 = none) | C |
| 0x120 | u8 | scenario picture index (a built-in picture choice) | L |
| 0x121 | u8 | always 0 in maps. The game overwrites it at load with its wide-front-row option (6 units per row when set, else 4); in a save it stores that option and restores it | C |
| 0x122 | u16 | playability score, written by the Community editor's score button (DTMapEdit 0x5a2c45); 0 in all shipped maps | C |
| 0x124 | u16 | save counter: the Community editor adds 1 on every save (0x5a4fc8); 0 in all shipped maps | C |
| 0x126 | u8 | number of quests, written with the score (0x5a2c5a); 0 in all shipped maps | C |
| 0x127 | 7 B | always 0 | U |
| 0x12E | u8 | always 0 in maps; in a save the save kind (1 manual, 2 autosave) | C |

### Hero preset (50 bytes, at 0x3C + 50·k)

| off | type | field | conf |
|---|---|---|---|
| 0 | u32 | always 0 in shipped maps. Possibly fame or gods' favour. | U |
| 4 | u32 | always 0 | U |
| 8 | u32 | starting gold (the exe reads the low 16 bits, signed; the same for every class in the shipped maps) | C |
| 12 | u32 | starting mana (low 16 bits, signed; largest for the archmage). There is no starting experience (original-mechanics/experience.md §5) | C |
| 16 | u8 | starting building (1-based building id, 0 = none) | C |
| 17 | 2 B | always 0 | U |
| 19 | 6 × (u8 unit, u8 level, u8 count) | starting troops; the level is 0-based (0 = level 1 as the game shows it) | C |
| 37 | u16 | start x | C |
| 39 | u16 | start y | C |
| 41 | u8[3] | starting artifacts (artifact GlobalIndex), put into the pack | C |
| 44 | u8[6] | starting spells and prayers (1-based index into `Rus_Spells.ini`), learned | C |

The game offers a class on the new-game screen only when its start x or start y is non-zero
(0x4c1804). Troop triples with unit id 1–3 (the hero types) are skipped, here and in armies and
garrisons (0x4b44db). Bytes 0–7 and 17–18 are not read by the map loader. (C)

### Game clock (C)

Time values count minutes from year 0, month 1, day 1, 00:00. A day is 1440 minutes, a month is 30 days and a year is
12 months (360 days). Durations and repeat intervals in events use the same unit. For example, 1440 means one day.

This was checked against gameplay footage: the header of РК3 decodes to year 1204, month 5, day index 19, 09:00. The
game shows days 0-based (`5 месяц, 29 день` is followed by `6 месяц, 0 день`), so the screen reads `1204 год, 5 месяц,
19 день, 9 час` at the start, and the footage shows day 19 in its first minute.

## 4. Terrain grid (C)

The terrain is a flat stream of `(u8 value, u8 run−1)` byte pairs. Each pair expands to `run` cells, filling the grid
row by row: index = `y*W + x`, with row 0 at the top (north). Runs can cross row boundaries. The stream always expands to
exactly `W*H` cells.

Codes follow the order of the editor's surface palette. The order is L, but it is well supported: the frequencies are
plausible, rivers and lakes come out as codes 1 and 2, sand lines the banks as 10, and roads link the buildings as 4.

| code | surface (editor label) | seen in maps |
|---|---|---|
| 0 | shallows / fords (Отмели и броды) | yes |
| 1 | coastal water (Прибрежные воды) | yes |
| 2 | deep sea (Глубокое море) | yes |
| 3 | lava fields (Лавовые поля) | rare |
| 4 | road (Дороги) | yes |
| 5 | grass lowland (Травяная низина) | yes |
| 6 | grass plain (Травяная равнина). The most common code. | yes |
| 7 | dry plain (Засушливая равнина) | yes |
| 8 | marsh (Болотистая местность) | yes |
| 9 | impassable swamp (Непроходимое болото) | yes |
| 10 | sand and dunes (Пески и барханы) | yes |
| 11 | clay soil (Глинистая почва) | yes |
| 12 | stony soil (Каменная почва) | yes |
| 13 | scorched land (Выжженные земли) | yes |
| 14 | snowy ground (Заснеженная почва) | not in shipped maps |
| 15 | impassable snowdrifts (Непроходимые сугробы) | not in shipped maps |

The file stores a plain W×H array; the topology is not stated (U). The evidence points to **plain rectangular
32×22 px cells with 8 neighbours**, not a staggered (hex) layout (L):
- the editor's grid masks (`Graphics/Editor/Grid*.tga`) are unstaggered 32×22 rectangles, and the terrain textures are
  256×242 px = 8×11 such cells;
- the path arrows (`Windows/Way_Arrows.ugs`) and the map figures have 8 directions;
- roads are thin 4-connected strokes (76% of road cells have exactly 2 road neighbours among the 4 orthogonal ones,
  against 45% for 6 hex neighbours) with diagonal steps between them;
- 1×1 bridge pieces cross rivers diagonally, one column per row. No hex parity connects such a chain; in РК3 it is the
  only link to the capital. With 8 neighbours every building of РК1 and РК3 is reachable from every hero start.

The x/y orientation is confirmed: with `(x, y)` = (column, row), trees almost never stand on water (5 of 54,773). With
the axes swapped, about 7% would.

## 5. Objects: hills, mountains, trees, stones (C layout, L meaning)

Each object is 6 bytes: `u16 x, u16 y, u8 sprite, u8 class`. The records are sorted by (y, x), and one cell holds at
most two: the Community editor keeps one object of classes 1–8 and one of the other classes per cell and writes them
from its cell grid in that order, so a later record for the same group replaces an earlier one (DTMapEdit 0x5a4c44,
0x5a8c00) (C).

| class | count (all maps) | typical terrain underneath | sprite ids | likely meaning |
|---|---|---|---|---|
| 1 | 5 k | grass plain | 10–19, 20–27, 30–33, 40–43, 50–53, 60 | hills |
| 5 | 3.7 k | stony soil | 10–13, 20–23, 30–33, 40–43, 50–53 | mountains / rock massifs |
| 8 | 0.9 k | mixed, including shallows | 10–38 | rocks / stone scatter |
| 9 | 55 k | grass, lowland, marsh | 0–8, 12–20, 24–29, 36–41, 48–50, 60–65, 120–184 | trees / forest |
| 10 | 8.4 k | mixed, including scorched land | 108–116, 228–236 | dead / dry trees |
| 11 | 27 k | grass | 0–8, 12–20, 24–29, 36–41, 108–116 | dense thicket (impassable) |
| 2, 3 | 1–4 | – | 11–23 | stray and rare. Unknown. |

The sprite id appears to be grouped by tens (a style family) plus a variant digit. The ids index the editor's object
palette and `Graphics/Objects/Objects.ugs`: (class, sprite) is the (cat, idx) key of an Objects.ugs section-A
record (C: every object of every shipped map resolves). Exact passability per class is **U**.

## 6. Buildings (358 bytes each, C unless noted)

Buildings have 1-based ids in file order. Events refer to buildings by that id.

| off | type | field |
|---|---|---|
| 0 | u16 | x. This is the **bottom-right** cell of the footprint (L). Footprints anchored this way are free of trees and sit on roads. |
| 2 | u16 | y |
| 4 | u8 | picture variant within the type |
| 5 | u8 | picture type (almost always equal to the building type) |
| 6 | u8 | building type (table below) |
| 7 | u8 | always 0 (U) |
| 8 | u16[64] | local events available here (1-based event ids). Only the first `event_count` entries are used, and 0 marks a deleted slot. |
| 136 | u16[64] | fixed artifacts: market goods, or the ruin treasure (клад). Only the first 6 are ever used. Ids are artifact GlobalIndex values up to 146. |
| 264 | 6 × (u8 unit, u8 count_at_start, u8 max_count) | barracks: units available to recruit |
| 282 | u16 | gold income per day |
| 284 | u16 | maximum gold that can accumulate (villages) |
| 286 | u16 | always 0 (U) |
| 288 | u8 | `event_count` |
| 289 | u8 | footprint size x |
| 290 | u8 | footprint size y |
| 291 | u8 | always 0 (U) |
| 292 | u8 | owner army (army id; 0xFF = none, so the neutral owner applies) |
| 293 | u8 | linked building (1-based). For a village, the castle it belongs to. For a dungeon entrance, probably the tunnel target (L). |
| 294 | u8 | has barracks: 1 exactly when a barracks slot (264) holds a unit. The Community editor derives it when its building window stores the record (DTMapEdit 0x54c3fc); all 1,082 shipped buildings agree |
| 295 | u8 | number of random artifacts for sale |
| 296 | u8[6] | stale u8 copy of the artifact list. Often out of date; ignore it (L). The Community editor also reads and writes 296 (i8), 297 (u16) and 299 (u16) through three hidden controls of its building window (DTMapEdit 0x54c3fc) |
| 302 | 6 B | always 0 (U) |
| 308 | u8[6] | spells for sale (1-based spell index) |
| 314 | 6 × (u8 unit, u8 level, u8 count) | garrison (0-based levels) |
| 332 | u8 | extra garrison defence |
| 333 | u16 | minimum random-artifact price |
| 335 | u16 | maximum random-artifact price. For ruins, the treasure gold. |
| 337 | u8 | faction: 1 player, 2 ally, 3 neighbour, 4 enemy |
| 338 | i8[4] | attitude towards player, ally, neighbour and enemy (−3..3) |
| 342 | 8 B | always 0 (U) |
| 350 | u8 | mana income per day |
| 351 | u8 | maximum mana |
| 352 | u8 | always 0 (U) |
| 353 | u8 ×3 | starting building for knight, archmage, ranger |
| 356 | u8 | "all types" recruitment flag (allows bandits and similar) |
| 357 | u8 | garrison is AI-only |

Building types (byte 6), confirmed against the building names:

| id | type | id | type |
|---|---|---|---|
| 0 | palace (Дворец, unused) | 8 | smithy / house |
| 1 | town | 9 | shipyard / port |
| 2 | village | 10 | altar |
| 3 | castle | 11 | dungeon entrance |
| 4 | fort | 12 | ruins |
| 5 | tavern / inn | 13 | stone bridge |
| 6 | market | 14 | wooden bridge |
| 7 | church | 15 | obelisk |

Each building has three strings in the text section: name, neutral owner's name and description.

## 7. Armies (89 bytes each)

The army id is at byte 4, and it always equals the 1-based record index. Unit ids are the `GlobalIndex` values from
`Rus_Units.ini`, and artifact ids those from `Rus_Artefacts.ini`.

| off | type | field | conf |
|---|---|---|---|
| 0 | u16 | x | C |
| 2 | u16 | y | C |
| 4 | u8 | army id (1-based) | C |
| 5 | u8 | map model: 1 knight hero, 2 archmage, 3 ranger, 4 feudal, 5 bandits, 6 peasants, 7 inactive, 8 lantern, 9 event point, 10 necromancer, 11 ghosts, 12 zombies. Shipped maps use only 4–7. | C |
| 6 | u16 | tactical cost (editor value 1); not read by the game's map loader | L |
| 8 | u8 | 0..3. Possibly the leader archetype. Not read by the game's map loader | U |
| 9 | 4 B | always 0 | U |
| 13 | i8 | speed correction: speed = max(1, 5 − value − 1 if the leader is unit type 2) | C |
| 14 | u8 | "add experience like the player" flag: units the army hires start with XP from the player's army (experience.md §5) | C |
| 15 | 2 B | always 0 | U |
| 17 | i16 | starting gold of the army (not an income; corrected after reading the game's loader 0x4b2504) | C |
| 19 | u16 | bonus experience given to the units the army hires | C |
| 21 | 4 B | always 0 | U |
| 25 | u8 | home building (1-based, 0 = none) | C |
| 26 | u8 | leader unit id | C |
| 27 | u8 | leader level, 0-based | C |
| 28 | 6 × (u8 unit, u8 level, u8 count) | troops (0-based levels) | C |
| 46 | 4 B | always 0 | U |
| 50 | u8[3] | artifacts carried | C |
| 53 | 5 B | always 0 | U |
| 58 | u8 | named character (1-based index into the named-character list, 0 = none) | C |
| 59 | u8 | behaviour style: 0 feudal, 1 rogue, 2 peasant. It equals the model (4, 5, 6) in all 231 such armies of the shipped maps; model-7 armies carry it too | L |
| 60 | u8 | patrols | C |
| 61 | u8 | patrol radius | C |
| 62 | u8 | units carry no money | C |
| 63 | u8 | inactive at start: 1 exactly for the model-7 ("inactive") armies in every shipped map, 0 for all others | L |
| 64 | u8 | faction: 1 player, 2 ally, 3 neighbour, 4 enemy | C |
| 65 | i8[4] | attitude towards the four factions | C |
| 69 | i8 | aggression | C |
| 70 | u8 | respawn time in days | C |
| 71 | u8 | experience correction in percent (100 = normal): scales the XP the player gains by beating this army | C |
| 72 | u8 | ship type (0 none, then hero, pirate, merchant) | L |
| 73 | u8 | always 0 | U |
| 74 | u16 | tactical cost (editor value 2); not read by the game's map loader | L |
| 76 | u8 | ignored by AI | C |
| 77 | u8 | hunts only the player | C |
| 78 | u8 | no random targets | C |
| 79 | u8 | no socialising with other armies | C |
| 80 | u8 | base daily gold income ÷ 10 (the game stores value × 10) | C |
| 81 | u8 | no interest in buildings | C |
| 82 | u8 | garrison strength (default 50) | C |
| 83 | u8 | respawn the whole army, not only the leader | C |
| 84 | u8 | spell cast on the army (1-based spell index) | C |
| 85 | u8 | target-selection model: 0 standard, 1 aggressive, 2 passive, 3 hoarding, 4 trading | C |
| 86 | 3 B | always 0 | U |

The behaviour style (feudal, robber or peasant) is byte 59; the model byte (4–6) repeats it for armies on the map, and model-7 (inactive) armies need byte 59 (L).

Garrisons are not armies. They are stored inside the building records (section 6, offset 314).

Each army has three strings: army name, leader name and description. Placeholder names such as `#HERONAME`, and
archetype suffixes like `#рыцарь`, appear inside the strings.

## 8. Points: lanterns and event points (99 bytes each)

| off | type | field | conf |
|---|---|---|---|
| 0 | u16 | x | C |
| 2 | u16 | y | C |
| 4 | u8 | point id (1-based). Events refer to points by this id. | C |
| 5 | u8 | model: 8 = active lantern (reveals an area), 9 = event point or inactive lantern | C |
| 6 | u16 | running serial number. Events do not reference it. | L |
| 8 | u16[10] | local events attached to the point (the editor allows 5) | C |
| 28 | u16[4] | target priorities for green, blue, yellow and red | L (always 0) |
| 36 | u16 | active duration | L (always 0) |
| 38 | u8 | visibility radius at start (lantern radius, at most 24) | C |
| 39 | u8 | number of attached events | C |
| 40 | u8 | active at start | C |
| 41 | 58 B | always 0 | U |

## 9. Events (171 bytes each)

Events have 1-based ids in file order, and every cross-reference between events uses these ids. Every offset below
is C: besides the data and the modder notes, the Community editor's routine that stores its event window into the
record was read (`DTMapEdit.exe`; each control's published field, the value it reads and the byte it writes), so
the control behind each byte is known. Unless stated otherwise, a "check" byte is a checkbox that enables the
condition next to it. The editor holds at most 5000 events.

**Header**

| off | field |
|---|---|
| 0 | u8 group colour in the editor (0–5) |
| 1 | u8 type: 1 global, 2 local, 3 quest, 4 rumour. This is C: quests are the targets of "completes quest", and rumours occur only in buildings. |
| 2 | u32 start time (minutes). 1 036 800 000 (year 2000) is the editor's "relative time only" box: the event has no start of its own until a "relative event" result sets one. |
| 6 | u16 repeat period (minutes). The editor edits it in days (1–31) and stores days × 1440. |
| 8 | u16 active duration. The editor edits it in hours and stores hours × 60 (every shipped value is a multiple of 60), but the game's window check (0x4a7c3f) takes the stored number as hours, so a window the editor shows as 24 hours lasts 1440 hours in the game. |
| 10 | u8 hero archetype: 0 all, 1 knight, 2 archmage, 3 ranger |

**Conditions**

| off | field |
|---|---|
| 11 | i16 squad count (at most 12). The sign encodes ≥ or ≤: the editor's switch stores the value, or its negation when set to ≤. The same holds for 13, 19, 21 and 25. |
| 13 | i16 army strength |
| 15 | u8 army must be inactive |
| 16 | u8 army whose patrol changes (a result) |
| 17 | i8 patrol delta. The community patch overloads this as an opcode; see its guide. |
| 18 | u8 check "current stats" |
| 19 | i16 level |
| 21 | i16 gold |
| 25 | i16 holiness / mana |
| 29 | check: building ownership |
| 30 | u8[3] building ids |
| 33 | u8[3] owners: 0 none (the list's empty first entry), 1 player, 2–5 green, blue, yellow, red, 6 "not the player"; the same codes at 43 and 50. For buildings the game (0x4a815d) skips a slot with code 0, reads 1 as "owned by the player", 6 as "not", and 2–5 as "the building's faction (byte 337) is code − 1", whoever holds it |
| 36 | check: named squad in some army. Each slot needs its own unit (three alike: three units); see economy.md §6 |
| 37 | u8[3] unit ids |
| 40 | u8[3] named characters |
| 43 | u8[3] owners |
| 46 | check: artifacts |
| 47 | u8[3] artifact ids |
| 50 | u8[3] owners |
| 53 | check: player defeated armies |
| 54 | u8[2] army ids |
| 56 | check: event happened, answer yes |
| 57 | u16[2] event ids |
| 61 | check: event not happened |
| 62 | u16[2] event ids |
| 66 | check: army beaten by anyone |
| 67 | u8[2] army ids |
| 69 | check: event happened, answer no |
| 70 | u16[2] event ids |
| 74 | u8 meet army: the army being met right now (the game's 0x68dc7c, cleared after the scan that followed the meeting) |
| 75 | u8 army is active |
| 76 | u8 ask a confirming yes/no question. It correlates with a non-empty question text. |
| 146 | u8 army is in its home building |

**Results**

| off | field |
|---|---|
| 77 | u16 relative event |
| 79 | u16 relative event delay, in hours |
| 81 | u8 spell to activate on the player |
| 82 | u8 picture: 200 defeat, 201 victory, otherwise a unit id |
| 83 | i16 experience change |
| 85 | i16 gold change |
| 89 | i16 mana change |
| 93 | u8[4] spells learned |
| 97 | u8[4] units added |
| 101 | u8[4] named characters for the added units |
| 105 | u8[4] units removed (0xFE = "unit added by an event", 0xFF = "any unit") |
| 109 | u8[4] named characters for the removed units |
| 113 | u8[4] artifacts gained |
| 117 | u8[4] artifacts lost |
| 121 | u8[2] armies activated |
| 123 | u8 army deactivated |
| 124 | u16 quest completed (event id) |
| 126 | u16 delay in hours |
| 128 | u16[4] lanterns lit (point ids) |
| 136 | u8 army that removed units go to |
| 137 | u8 new hero class (unit id) |
| 138 | u16 chained (subordinate) event, executed immediately, without checking its conditions, window or once flag (0x4ab1ec) |
| 140 | u8 "subordinate event" flag. In memory this byte is the "done" flag (+0x8c): the check 0x4a7b80 skips the event, so it fires only through a chain |
| 141 | u8 **0 = may fire many times, 1 = once** (the inverse of the editor's "many times" box) |
| 142 | u8 army that added units are taken from |
| 143 | u8 move that army to the hero |
| 144 | u8 show army |
| 145 | u8 hero has only 1 HP |
| 147 | u8 start a battle with this army |
| 148 | u8 "no meeting with army" (also a community opcode switch): firing the event ends the current meeting (0x4ab286) |
| 149 | u8 repeat after a yes answer |
| 150 | u8 generate the battle army to match the player (the check box next to "start a battle with army"; always 0 in the shipped maps; the game's use is not traced) |

**Unknown and pictures**

| off | field |
|---|---|
| 23–24, 27–28, 87–88, 91–92 | U (small or rare values) |
| 151–162 | U. Byte 156 is a 0/1 flag in about 2% of events; the editor's window does not write it. |
| 163 | size of this event's custom picture, a u32 at 163–166 (the game reads all 32 bits; shipped sizes fit in the low u16). It is stored after the scenario picture: u16 width, u16 height, then width·height 16-bit pixels. The only example is 128×128, 32,772 bytes. The pixel format is probably RGB565 (L). |
| 165–170 | U |

**Strings.** Each event has three strings:
1. Title, optionally followed by a flag script (below).
2. Question text: the text of the yes/no prompt. Empty unless offset 76 is set.
3. Message text shown when the event fires. Empty means the event fires silently.

**Flag script (C).** The editor edits the title in three fields (the name, "flag" and "check flag") and writes
`name`, then `%` and the flag field if that starts with `+` or `-` (a flag field without a sign is dropped), then, if
the check field is not empty, `%` (when no flag was written) and `=` and the check field. So an event title may be
followed by `%` and a script of the form `[+X | -X][=X | =/X]`:
- `+X` sets flag X and `-X` clears it; these are results.
- `=X` requires X to be set and `=/X` requires X to be unset; these are conditions.

Examples of the syntax: `…%+Foo`, `…%-Foo=Foo`, `…%=/Foo`. Nowhere else stores flags.

## 10. Strings section

The strings follow the text marker, in this order (C):

1. scenario title
2. scenario description
3. campaign name
4. next scenario file name (`*.DTm`), empty if none
5. three strings per building: name, neutral owner name, description
6. three strings per army: name, leader name, description
7. three strings per event: title with flags, question, message
8. two strings per custom artefact (name, description), when header 0x34 is non-zero
9. N named-character names (N from header byte 0xEE; their classes are at 0xEF+i)

When the game loads the map (0x4b2504), every run of two or more spaces in the description and in
the building, army and event strings is collapsed to one space, and `#HERONAME` in the third
string of buildings and armies is replaced by the hero's name at that moment.

Victory and defeat conditions are not stored as text. They are the event ids at 0xD2 and 0xD8. Text escapes such as
`#HERONAME` are substituted at runtime.

## 11. Embedded pictures

**Scenario picture.** It is present when header 0x11C is non-zero (59,152 bytes in every shipped map that has one).
The data is a LIT image, the same format as `Graphics/**/*.lit`:

| off | field |
|---|---|
| 0 | `LIT\0` |
| 4 | u32 width (267) |
| 8 | u32 height (134) |
| 12 | u32 mode |
| 16 | compressed data |

In mode 2, the data after the header begins with 8×8 quantisation tables, which suggests a JPEG-like DCT codec. The LIT
codec itself is out of scope here (U).

**Event pictures.** These follow the scenario picture, in event order (see event offset 163).

## 12. How a loader should use this

- Guard the text marker: check that it sits at `0x12F + sum(sizes)`. The decoder raises an error if any section size is
  not a multiple of its record size, or if bytes are left over at the end.
- Cells: `terrain[y][x]`, plus zero or more objects per cell (the game keeps one object of classes 1–8 and one of the
  other classes per cell; a later record replaces an earlier one). Building footprints extend up and to the left of
  `(x, y)` by `size_x × size_y`, plus one extra row above when `size_x > size_y` (0x4b2f57).
- **Buildings.** Use type, owner, faction, garrison, barracks, market or treasure contents, income, and local events.
- **Armies.** Each is placed exactly at `(x, y)` (it is at sea when that cell is water and not a bridge); byte 63 alone keeps it off the map until an event activates it. `home_building` ties
  it to a building, and `named_character` indexes the named-character table.
- **Hero.** Pick one of the three presets by archetype. It gives the start position, gold, mana, troops,
  artifacts and spells.
- **Event engine.** On every tick, evaluate the conditions of all eligible events, as the editor manual describes.
  Global events can fire anywhere. Local events fire only in the building or point that lists them. Quests go to the
  journal. Rumours are optional local events.

## 13. Open questions

- Exact passability and speed factor for each terrain code and object class.
- The grid topology (see section 4; 8-neighbour squares is the working assumption).
- Army bytes 8, 59 and 80. Hero preset bytes 0–7 and 17–18. Header 0x120.
- Event bytes 151–162.
- Point priorities and duration (always 0 in shipped maps).
- The LIT image codec.
