# The original map editor: record editors

This file describes the windows of the original scenario editor that edit single records: armies, buildings,
unit types, artefacts, named characters, the scenario parameters with the hero presets, events, points, the
editor options and the small number dialog. For each window it lists every field, the field's range and
default, what the editor computes when the record is saved, which inputs it checks or limits, and how a change
spreads to other records.

The source is `DTMapEdit.exe`, the map editor that ships with the Discord Times Community Update. It is a
32-bit Delphi program with image base 0x400000, and it was read statically: decompiled handlers, disassembly
and the decoded form resources. Rules are given in our own words. Controls are named by what they do, not by
their captions. Addresses are virtual addresses in that build and serve as evidence. Byte offsets refer to the
`.DTm` records in [dtm-format.md](../dtm-format.md), counted from 0.

Confidence tags:
- **code**: read in the editor's code, or in the form resources it loads (the ranges of the controls).
- **data**: consistent with the shipped maps, the install's ini files or the manuals, but not traced in code.
- **unknown**: not determined.

Conventions:
- Ids of buildings, armies, points, events and named characters are 1-based list positions.
- "Round" is Delphi's rounding, which rounds halves to even in the default FPU mode.
- A spin control with no limit in its form resource accepts any number. Values that do not fit the byte or
  word they are stored in make the editor raise a range-check error when it stores them (0x402c68), and the
  save stops there. This document calls that "a range error on save".
- A spin control with limits keeps its value inside them, also when a window loads a stored value into it.
  A stored value outside the range therefore comes back clamped when the window is saved (**data**: the
  shipped maps show this for building byte 296, §13).
- In almost every list control, the first entry is an empty "none" entry, and it is stored as 0.

---

## 1. Shared behaviour

**code**
- **Unit lists.** Lists of units come from the editor's sorted unit table (0x5bf4a4). A list of troops (army
  troops, garrison, barracks, starting troops) starts at the fourth sorted entry, so the three hero classes
  never appear in it. The leader list and the event lists include every unit (army FormCreate 0x54431c;
  building FormCreate 0x54b4d8; scenario FormCreate 0x53edac).
- **Artefact lists.** Artefacts are listed in the editor's artefact order (0x5bf3fc): grouped by type, then
  by price, dearest first. The order is rebuilt when the artefact editor closes (0x5b2038).
- **Linked controls.** Each unit combo points to its count spin, and also to its level spin. Picking "none"
  sets the count to 0 and disables both spins. Picking a unit enables them and raises a count of 0 to 1. Setting
  a count to 0 clears the unit (army 0x54697c / 0x546af0; building 0x54eb50 / 0x54ec60; scenario
  0x541340 / 0x54145c).
- **Faction and attitudes.** The army, building and scenario windows edit attitudes on four gauges (−3..3)
  with up/down buttons, one step per click (0x544c4c, 0x54e81c, 0x5415c4). In the army and building
  windows, picking a faction reloads the four gauges from that faction's row of the scenario relation
  matrix (header 0xDE). Hand-made attitudes are lost when the faction is changed (0x546888, 0x54e97c). If no
  faction is picked, the faction saved is 4 (enemy) (0x544dac, 0x54c3fc).
- **Modified flag.** Every save button sets the main window's "map modified" flag (main form +0x58c).
- **Event filter.** Event pickers in the building and point windows share the main window's filter: a type
  (all, global, local, quest, rumour) and a colour group. Double-clicking an event adds it (0x54e730,
  0x551400).

## 2. Number dialog (lantern radius)

**code** (0x528bd0; form resource of the dialog)
- This is a one-value dialog: a caption, a label, a spin, OK and Cancel. It opens at the cursor when a
  position is given. It returns the spin's value on OK, and the default it was given on Cancel.
- Its only use is the radius of a lantern (point model 8), at most 24, at least 0:
  - when a lantern is placed, the record first gets radius 10 and "active at start" on. The dialog then opens
    with 10 as its default (place routine 0x595390, call at 0x5973e8);
  - clicking an existing lantern opens it with the current radius (0x5aea7a). The result is stored in point
    byte 38, and the lit area is redrawn.

## 3. Army window

**code** (open 0x54333c, load 0x54347c, create 0x54431c, save 0x544dac, cost 0x547070)

The window opens on its own when an army is placed (0x595390). It also opens from the army menu and from the
map check. A new army record is all zeros, except for:
- its position and id;
- the model chosen in the menu (4 feudal, 5 rogue, 6 peasant, 7 inactive);
- experience correction 100;
- for model 7: style 0 and "inactive at start" on; otherwise style = model − 4.

It gets a default name ending in its number; the leader name and description are empty. At most 255 armies.

### 3.1 Fields

| Field (byte) | Control and range | Notes |
|---|---|---|
| Name, leader name, description | text, text, memo | the leader-name box is disabled while a named character is used |
| Leader unit (26) | combo: none, every unit, then one extra blank entry at the end | the extra entry is stored as 255 (§3.3) |
| Named character (58) | check box + combo of the named characters | picking one sets the leader unit to the character's class and the leader name to its name (0x547a84) |
| Leader level (27) | spin, no limit | shown as stored, 0-based |
| Starting gold (17, i16) | spin, step 50, no limit | |
| Home building (25) | combo: none + every building of type town, village, castle, fort, church, shipyard, altar or ruins | §3.2 |
| Artefacts carried (50–52) | three artefact combos | |
| Spell on the army (84) | combo: none + every spell | |
| Ship (72) | up/down 0..3 with a picture | 0 = none |
| Troops (28–45) | six unit combos (troops only), level spin and count spin each, no limit | at most 12 units with the leader (§3.3) |
| Base gold income (80) | spin 0..2500, step 10 | stored divided by 10 |
| Bonus experience for hired units (19, u16) | spin 0..1000, step 10 | |
| "Hired units start like the player's" (14) | check box | |
| Garrison strength (82) | spin 0..90, step 5 | |
| Behaviour style (59) | three-way choice: feudal, rogue, peasant | also sets the model (§3.4) |
| Inactive at start (63), patrols (60) | check boxes | |
| Patrol radius (61) | spin 0..250, step 5 | |
| Speed correction (13, i8) | spin −3..5 | |
| Aggression (69, i8) | spin −100..100, step 10 | |
| Target-choice model (85) | five-entry combo | |
| AI flags (76–79, 81) | five check boxes | |
| Respawn time (70) | spin 0..30 days | |
| Respawn the whole army (83), units carry no money (62) | check boxes | |
| Experience correction (71) | spin 10..250 %, step 5 | |
| Faction (64), attitudes (65–68) | four faction buttons, four gauges | §1 |

Previous/next buttons and the PageUp/PageDown keys save the army, step to the neighbouring army with
wrap-around, centre the map on it and reload the window (0x547c58, 0x547df4, 0x548000).

### 3.2 Home building

Choosing a home building copies three things into the army (0x5462c4):
- the building's neutral-owner name into the leader name;
- the building's faction into the faction buttons;
- the building's four attitudes into the gauges.

The window also shows the building's picture and the centre of its footprint.

### 3.3 Size limit and the special leader entry

- **Size limit.** Every change of a unit, level, count, artefact, spell or leader recomputes the cost
  (0x547070). That routine builds a scratch army with the leader and every troop unit, one unit at a time.
  More than 12 units makes it fail, and the count that was just raised drops back by one (0x546f94,
  0x546af0). The limit is therefore 12 units with the leader included.
- **Special leader entry.** The extra entry at the end of the leader list (stored as 255) changes the window
  (0x546c68):
  - it disables the start-parameter group, the spell, the named character, the home building and the
    artefacts;
  - it hides the troop page and the AI page;
  - it ticks "inactive" and "patrols", and sets the radius to 0.

  What the game does with leader 255 is **unknown**.

### 3.4 Computed on save

**code** (0x544dac)
- Byte 8 is always written 0.
- The model byte (5) is 7 when the army is inactive at start, otherwise the behaviour style + 4. The army's
  cell code on the map is rewritten to match.
- The two stored tactical costs come from the cost routine, not from the user. Each is capped at 65,000:
  - byte 6 holds the sum over all units of each unit's current tactical cost (the battle core's evaluator in
    its "current" mode, 0x57faf8);
  - byte 74 holds a value that the battle core's side evaluation (0x58af74, 0x4f61f4) leaves in the side
    record. Its exact meaning is **unknown**.
- Shown but not stored (0x547070):
  - the army's gold cost, which is the sum of the unit types' prices;
  - its upkeep: the sum of price ÷ d (integer division) over every unit except the first one added. The
    divisor d is the recruit-cost divisor read from the game's global ini (0x5bf1b8; 2 in the shipped ini),
    not a constant. The first unit is the leader when there is one, otherwise the first troop unit.
- The spell on the army takes part in the cost: each unit gets it with a "never expires" time. The three
  artefacts are equipped on the first unit only (the leader when there is one), not on every unit
  (0x547600–0x5476d4).

## 4. Building window

**code** (open 0x54b1ac, load 0x549210, create 0x54b4d8, save 0x54c3fc)

A building is placed with these defaults (0x595390, mode 3):
- faction 3 (neighbour), with that faction's row of attitudes;
- garrison defence by type: town 20, village 2, castle 15, fort 10, ruins 5, all others 0;
- a random name and description from the type's lists, seeded by the position;
- the footprint of the chosen picture.

A new building can be placed only while fewer than 254 exist. Clicking a building on the map finds it through
its footprint (0x54af70) and opens this window.

### 4.1 Pages by type

| Page | Shown for types |
|---|---|
| Events | all |
| Troops group (barracks and garrison sub-pages) | 1–4, 7, 8, 10, 12 |
| Barracks | 1–4, 7, 8, 10, 12 |
| Garrison | 1–4, 12 |
| Treasure | 12 (ruins) |
| Trade group with market | 1, 6, 7, 10 |
| Library (spells) | 1, 7, 10 |
| Faction, income and links | all |

(0x549210.) The page that opens first is Events. Inside the troops group, Barracks is chosen if it is shown,
otherwise Garrison. Inside the trade group, Market is chosen if it is shown, otherwise Library.

### 4.2 Fields

| Field (byte) | Control and range |
|---|---|
| Type (6) | combo of the types; the type is saved as the combo position + 1 |
| Picture (4–5) | previous/next buttons; a check box switches them from cycling the variants of the type to cycling the types 1–14 (0x54ed78) |
| Name, neutral owner name, description | editable combos offering the type's name lists, and a memo |
| Owner army (292) | combo: none + every army; none is stored as 255. A link jumps to the owner on the map (0x54fe08) |
| Starting building of each hero class (353–355) | three check boxes |
| Recruits of all kinds (356), garrison serves the AI only (357) | check boxes |
| Barracks (264–281) | six troop combos, each with a starting count 0..9 and a maximum 0..9 |
| Garrison (314–331) | six troop combos, each with a level 0..9 and a count 0..9 |
| Garrison defence (332) | slider 0..50, inverted: the byte saved is 50 − the slider position, and loading sets the slider to 50 − the byte (0x54c3fc, 0x549210). A stored value above 50 (six buildings in the shipped maps, **data**) puts the slider at 0 and is saved back as 50 |
| Treasure (ruins only) | five artefact combos (136–145) and gold 0..50,000 in steps of 50 (335) |
| Market (other types) | six fixed artefacts (136–147); random count 0..12 (295); lowest price (333) and highest price (335), each 0..50,000 in steps of 500 |
| Spells for sale (308–313) | six spell combos |
| Gold income per day (282) | 0..250, step 10 |
| Most gold kept (284) | 0..2,500, step 10 |
| Mana income per day (350) | 0..250, step 10 |
| Most mana kept (351) | 0..2,500, step 10, though the byte only holds 255: a range error on save above that |
| Linked building (293) | village: none + every castle; dungeon entrance: none + every dungeon entrance, itself included (0x54b1ac) |
| Faction (337), attitudes (338–341) | §1 |
| Local events (8–135, count 288) | ordered list picked from all events |
| Community limits (296 i8, 297 u16, 299 u16) | three spins (0..12, and two unbounded in steps of 10) that the form resource leaves hidden and disabled. No code shows them, so the values are loaded and written back, except that byte 296 is clamped to 0..12 on the way (see §13 for what the shipped maps hold there) |

### 4.3 Computed and checked on save

**code** (0x54c3fc)
- The footprint (289–290) is taken from the picture table. The picture code is also written into the map
  cell.
- Byte 294 ("has barracks") is set to 1 exactly when any barracks slot holds a unit.
- Bytes 136 onwards hold the treasure for ruins and the fixed market goods for every other type. For ruins,
  byte 335 is the treasure gold. For other types it is the highest price, byte 333 is the lowest price and
  byte 295 the number of random goods.
- The event area (128 bytes) is cleared and then rewritten from the list.
- **Local events.** An event can be added twice: there is no duplicate check. Adding an event appends it, or
  inserts it at the selected line. More than 64 entries cause a range error. Double-clicking an entry removes
  it, and the list closes up (0x54e404, 0x54e66c).

### 4.4 Shown only

- **Garrison strength.** This is recomputed on every garrison change (0x54f03c). The garrison units, with the
  defence bonus applied and the ruins' treasure items equipped, are rated by the battle core. The window shows
  their summed tactical cost and the side strength. Nothing is stored.
- **Market test.** This needs a highest price of at least 50 and at least the lowest price. It runs the game's
  own restock (0x581a84) on a scratch copy of the building, with the fixed goods marked as fixed. The 12 goods
  that come out are shown with their prices (0x54f9f4). The test is enabled only when the random count is above
  0 (0x54fdd4).

## 5. Unit editor

**code** (open 0x558d74, show 0x558e44, form to record 0x559c20, store 0x55a268, cost 0x55c860,
export 0x55a430)

The window lists every unit type, heroes included, coloured by nature. It edits the in-memory unit table of
this session: there is no add, copy or delete. Edits change every cost shown by the editor afterwards, but they
are never written into a map. The only way to keep them is the export (§5.3). Choosing another unit in the
list stores the current one first.

### 5.1 Fields

| Field (unit-type offset) | Range |
|---|---|
| Name, description | text, memo |
| Icon (+0) | previous/next, cycling through as many icons as there are unit types |
| Nature (+0x2c), magic school (+0x41), magic direction (+0x46), bonus (+0xc5) | combos; the bonus list comes from the ini. The bonus is also copied to +0x63 |
| Hits | 10..250, step 5 |
| Attack and defence against blows and shots, magic power | 0..250, step 5 |
| Initiative, manoeuvres | 1..250 |
| Protections (life, death, elements), vampirism, regeneration | 0..99, step 5 |
| Per-level gains of every stat | one decimal, no limit (stored rounded) |
| Cost multiplier (+0x20) | no limit, form default 110 |
| Gold divisor (+0xc7) | 1..9 |
| Surrender (+0xc6) | 0..1000, one decimal |
| Starting experience, level multiplier | no limit |
| Up to three upgrade units with their levels | unit combos (every unit), level spins |

### 5.2 Cost formula

**code** (0x55c860, constants read in the disassembly at 0x55c8fb–0x55ca91)

The window recomputes the cost after every change of a stat:
1. S = the shared strength evaluator applied to the unit's base stats (0x57f4a8; the same evaluator the
   game uses).
2. Tactical cost T = Round(S × cost multiplier ÷ 100). If T < 1, both costs show 0 and nothing else is
   computed.
3. Gold G = Round((T ÷ 2.2 + 21) ÷ gold divisor). This uses the tactical cost T (already scaled by the
   cost multiplier), not S: the value loaded at 0x55c95d is the one stored after the rounding at 0x55c914.
   G is kept as a 16-bit value, so a G above 65,535 is a range error.
4. G is rounded down in bands:
   - below 100, to a multiple of 5;
   - 100–250, to a multiple of 10;
   - 251–500, to a multiple of 20;
   - 501–5,000, to a multiple of 50;
   - above 5,000, unchanged.
5. G is doubled for the three hero classes (unit index 1–3).
6. When the unit is stored, G becomes its price (+0x1c).

The window also shows a table of the tactical cost at levels 1 to 5, each as a percentage of T. The stats
at level L are base + gain × L, with the gains taken as the rounded whole numbers that are stored. The five
stats limited to 0..99 (the three protections, regeneration and vampirism) grow differently (0x55c558, used
for all five by 0x55c5e8):
- start from r = 100 − base;
- repeat L times: reduce r by gain × r ÷ 100 (single-precision arithmetic);
- the result is 100 − Round(r), capped at 99.

### 5.3 Export

**code** (0x55a430)
- The export first stores the current unit. It then writes a new unit ini into the program folder, under its
  own name next to the game's file, so the game's file is not overwritten. A second file holds the localised
  name and description of each unit.
- Each unit gets a section headed by its index and name. The section lists every non-zero key: identity,
  texts, costs, nature, magic school and direction (written as names), surrender, experience, icon, bonus,
  the upgrade units by name with their levels, the stats and the per-level gains. A cost multiplier of 0 is
  written as 100.
- A unit with no name gets an empty section.
- A random-order option exists in the code, but its check box is hidden in the form resource.

## 6. Artefact editor

**code** (open 0x552024, show 0x552230, copy 0x552e34, save 0x552f14, delete 0x5538b4, price 0x555bc8,
export 0x553a6c)

The window works like the unit editor: it edits the session's in-memory artefact table and keeps nothing in
the map. Each row shows the artefact's icon, name and price.

### 6.1 Fields

| Field | Range |
|---|---|
| Name, description | text, memo |
| Icon | previous/next |
| Price | spin, step 25, no limit |
| Type, magic school, bonus | combos |
| Three blocks of stat changes | spins with no limit. Each block covers hits, attack and defence against blows and shots, magic power, initiative and manoeuvres, and the third block also has protections, regeneration and vampirism. The three blocks are the item record's three stat groups (the ini's f-, d- and p- keys) |

### 6.2 Copy, store and delete

- **Copy** makes a new artefact at id = count + 1 with the copy marker added to the name. It becomes real only
  when stored. Ids stop at 255.
- **Delete** moves every later artefact down one id. Nothing in the map is renumbered: armies, buildings,
  hero presets and events keep their old artefact ids and so point at different items (0x5538b4).

### 6.3 Automatic price

**code** (0x555bc8)
1. Nothing happens when the price is exactly 1 or the type is 8, 9 or 10. Otherwise the artefact is stored
   first.
2. Every unit type, heroes included, is tried as a fresh unit wearing the item (0x58b5d8). For every unit
   type that can wear it (count n):
   - g = the unit's tactical cost with the item minus its base cost (0x57faf8, mode 2);
   - v = Round(g ÷ 2);
   - if v > 0, add v × Round(√v + 2) to a sum and count the unit in m.
3. The price is Round(sum ÷ m) + 10, but at least 20. It is 20 when no unit type can wear the item.
4. The price is rounded down in bands:
   - below 151, to a multiple of 5;
   - below 501, to 10;
   - below 1,001, to 25;
   - below 2,501, to 50;
   - below 6,001, to 100;
   - below 15,001, to 250;
   - below 50,001, to 500;
   - below 500,001, to 1,000.
5. A label shows n and the share m ÷ n.

The counter m is never set to 0 before the loop in this build. Only the counter n is. The divisor therefore
starts from whatever value is left on the stack (code, 0x555c60 / 0x555d4d). Whether this is 0 in practice is
**unknown**.

### 6.4 Export

**code** (0x553a6c)
- Like the unit export, this writes a new artefact ini plus a second file with the localised names and
  descriptions.
- Each artefact lists its identity, texts, icon file, price and type, then its magic school and bonus when
  set, then every non-zero stat key.
- The icon file name is the letter A, a three-digit icon number and the picture extension.
- Type, school and bonus are written as names, not numbers.
- A nameless slot gets an empty section.
- The random-order check box is hidden here too.

## 7. Named characters

**code** (open 0x52e63c, add 0x52e928, change 0x52ea20, delete 0x52eac8)
- A grid lists each character with its portrait, name and class. A text box and a class combo (every unit)
  are used to add a character, or to change the selected one. At most 32 characters; names are cut at 64
  characters.
- Delete removes the row and moves the later ones up. The window builds an old-to-new index table. Only the
  event window uses it, and only for the eight named-character combos of the event being edited (0x53ad48).
- Army byte 58 and the named-character bytes of stored events are **not** renumbered. After a delete they
  point at the next character, or past the end of the list (every user of the table, 0x5beffc, was checked).
- The OK button writes the names and classes back; Cancel discards the changes.

## 8. Scenario parameters

**code** (open 0x53df3c, create 0x53edac, save 0x53f604, preset page 0x53fd3c / 0x540864)

### 8.1 General

| Field (header) | Control |
|---|---|
| Title | text, cut at 64 characters |
| Description | memo |
| Map size | read-only |
| Start date (0x38) | masked text: hour, then day, month and a four-digit year (§8.4) |
| Victory event (0xD2), defeat event (0xD8) | combos: none + every event |
| Income summary | two read-only sums of daily gold: castles plus forts, and villages (towns are not counted) |

### 8.2 Hero presets

There is one page per class: knight, archmage, ranger. The window opens on the page of the class asked for.
Each page shows the class portrait and these fields:

| Field (offset in the 50-byte preset) | Range |
|---|---|
| Experience (6, i16) | 0..32,000 |
| Gold (8, i16) | 0..32,000 |
| Mana (12, i16) | 0..32,000 |
| Starting building (16) | combo: none + every town, village, castle, fort, church, altar or ruins, with its picture and position |
| Six starting troops (19–36) | troop combos with level 0..9 and count 0..9 |
| Three artefacts (41–43) | artefact combos |
| Five spells (44–48) | spell combos; byte 49 has no control |

Rules for the presets:
- The six troop counts may total at most 11, so 12 with the hero. A count that would pass 11 drops back by one
  (0x54145c).
- The start position (37–40) is not edited here. It is set by placing the hero figure on the map.
- Leaving a class page stores it back (0x540864, read in the disassembly). It writes exactly the bytes above:
  6, 8 and 12 as words, 16, 19–36 and 41–48. Bytes 0–5, 10–11, 14–15, 17–18, 37–40 and 49 are left as they
  are.

### 8.3 Alliances

The page has a 4×4 grid of gauges (−3..3) with up/down buttons, for player, ally, neighbour and enemy. The
gauges are header 0xDE to 0xED, row by row. Four buttons load preset matrices (rows are player, ally,
neighbour, enemy; columns in the same order):

| Preset | Row 1 | Row 2 | Row 3 | Row 4 |
|---|---|---|---|---|
| Default (0x541b44) | 3, 2, 1, −2 | 2, 3, 1, −2 | 1, 1, 3, 1 | −2, −2, 1, 3 |
| Allied (0x541c84) | 3, 2, 2, 1 | 2, 3, 2, 1 | 2, 2, 3, 2 | 1, 1, 2, 3 |
| Neutral (0x541dc4) | 2, 0, 0, 0 | 0, 2, 0, 0 | 0, 0, 2, 0 | 0, 0, 0, 2 |
| War (0x541ee0) | 1, −1, −2, −3 | −1, 1, −2, −3 | −1, −1, 1, −1 | −3, −3, −2, 1 |

The war preset is not symmetric: row 1 col 3 and row 3 col 1 differ, and so do row 3 col 4 and row 4 col 3.

### 8.4 Start date and its effect on events

- **Reading the date.** The date is read as hour × 60 + (day − 1) × 1,440 + (month − 1) × 43,200 + year ×
  518,400 minutes (0x5941d0). The digits are not checked against any range, so a day of 31 or a month of 13
  simply adds more minutes.
- **Showing a stored value.** The day and month are shown 1-based (0x593fb0).
- **Shifting the events.** When the window closes, the editor compares the start date with the one it had on
  opening. If the date changed, every event's start moves by the same number of minutes. Events in "relative
  time only" mode keep their marker value (0x53df3c, tail). Cancel never moves anything, because only the save
  button writes the new date.
- **Redrawing the lit area.** After closing, the editor clears the map's lit area and recomputes it: radius
  5 around each class's start, when its position is set, and each lit lantern's radius around that lantern.

### 8.5 Campaign and picture

- **Scenario kind (0x10F).** A three-way choice: standalone, first map of a campaign, later campaign map.
  "None picked" is stored as standalone.
- **Campaign name.** A text box.
- **Next map.** A file chooser; only the file name is kept.
- **Seven carry-over check boxes.** They map to 0x110–0x116 in their order on the page (the form resource).
- **Built-in picture (0x120).** An index 0..5 that cycles with a spin button. 0 means none.
- **Custom picture.** Loaded from a file and kept as the file's raw bytes (size at 0x11C). The editor checks it
  by decoding it for the preview. If decoding fails, the picture is dropped and the size is set to 0
  (0x53ddd0). A remove button clears it.

## 9. Event editor

**code** (open 0x5324ec / 0x532654, create 0x533728, form to record 0x534d20, load 0x536c80, store
0x536618, new 0x538fe4, copy 0x539214, delete 0x5396ac, move 0x539ca0)

The byte behind each control is listed in [dtm-format.md](../dtm-format.md) §9, which was written from the same
routine (0x534d20). This section adds the ranges, defaults and list operations. The editor holds at most
5,000 events.

### 9.1 Ranges and rules not in the format notes

- **Type and group.** The type is global unless one of the four type buttons is set. The group is the colour
  chosen.
- **Title.** The title is stored together with the flag text and the check-flag text, by the rule in the
  format notes.
- **Start date.** Uses the same masked field and rule as the scenario (§8.4).
- **Relative time only.** This stores the marker start, disables the date, and clears and disables the
  subordinate box (0x53ac54). Turning it off restores the scenario start and enables both again.
- **Subordinate** (0x53aa10). When on, the date, repeat and duration are 0, and the "many times" box is
  ticked and locked, so the event is saved as repeatable (once byte 0). The relative-time box is cleared and
  disabled. When off, the date, repeat and duration get the scenario start, 1 day and 24 hours, and "many
  times" is unticked (once byte 1); both boxes are enabled again.
- **Repeat and duration.** Repeat is 0..31 days, stored × 1,440. The active duration is 0..99 hours, stored ×
  60. "Many times" is stored inverted as "once".
- **Thresholds.** Each threshold is a value with a "at least / at most" switch, stored as + or − the value
  (the format notes give the bytes). Ranges:
  - level 0..99;
  - squads 0..12;
  - army strength and mana: no limit;
  - gold: up to 99,999, but the value is stored as a signed 16-bit number: "at least" accepts up to
    32,767 and "at most" (stored negated) up to 32,768; anything larger is a range error on save (0x534d20).
- **Result changes.**
  - Experience, gold and mana: −32,000..32,000, step 10.
  - Relative-event delay: 0..5,000 hours, step 6.
  - Patrol change: −120..120, step 5.
  - Hero wait: no limit; it is stored as a 16-bit word.
- **Building lists.** The building pickers list villages, castles, forts, churches, altars and ruins only.
  Towns are not offered (0x532654).
- **Pictures.** The standard pictures are none, the global images of the install's ini (ids 200 and up) and
  every unit's portrait. A custom picture can be loaded from a file.
- **Hidden control.** The "generate the battle army" check box is hidden and disabled in the form resource.
- **Units in conditions.** The extra entry at the end of the condition unit lists is stored as 255. In the
  removed-units lists, the two extra entries are stored as 254 (the unit added by an event) and 255 (any unit).
- **Named characters and units.** Picking a named character sets its unit combo to the character's class.
  Changing the unit clears the character (0x53b2dc, 0x53b39c). A button opens the named-character window and
  renumbers the open event's selections afterwards (§7).
- **Text options.** The text size and bold option (§11) apply to the message and question boxes.

### 9.2 New, store, leave

- **New** (0x538fe4). Prepares slot count + 1 without counting it yet. The defaults:
  - title: a default word followed by the event number;
  - strings: empty;
  - record: zeroed;
  - start: the scenario start;
  - repeat: 1 day (1,440);
  - stored duration: 1,440;
  - "once": the inverse of the "new events repeat" option (§11).

  New, copy and delete stay disabled until the event is stored.
- **Store** (0x536618). Writes the record and strings and counts a new event. The lists are rebuilt.
- **Leaving an edited event** (0x53a364). Picking another event compares the form with the stored copy and
  asks whether to store it. Answering no to a copy that was never stored deletes the copy.

### 9.3 Copy, move, delete

- **Copy** (0x539214). Works while there are fewer than 5,000 events.
  - Naming: if the name contains a hash sign, the character after the first hash is raised by one character
    code (so a 9 becomes the next character, not 10); otherwise a hash and 1 are appended.
  - Placement: the copy is appended, then, unless the original is the last event, moved (as for move, with
    renumbering) to the position of the event on the next row of the event list. The record and texts are
    then taken from the event just before the copy's new position. That is the original when the list shows
    every event; with a filter that hides the events right after the original, the copy gets the record of
    another event under the original's numbered name (0x539214).
  - The custom picture is not copied.
- **Move** (0x539ca0). Started with the move button or by Ctrl-clicking an event, then clicking the target
  row. This reorders the events. Every reference listed below is renumbered: the moved event takes its new
  position, and the events in between shift by one.
- **Delete** (0x5396ac). Asks for no confirmation.
  - Inside events, these references are renumbered: the happened-yes, happened-no and not-happened
    conditions (two each), the relative event, the quest completed and the chained event. So are the
    header's victory and defeat events. A reference to the deleted event becomes 0; references to later
    events drop by one.
  - In building and point event lists, the matching entry becomes 0 and later ids drop by one, but the list
    is **not closed up**. The count drops by one whenever the counted part holds a 0 after this pass, whether
    or not the deleted event was in it (the per-entry helper 0x539658 reports "entry is 0", not "entry
    matched"). A deleted event in the middle of a list therefore leaves a 0 inside the counted part and pushes
    the last real entry outside it, and every later event delete, of any event, shortens that list by one
    more.

## 10. Points

There are three kinds of points (place routine 0x595390, mode 4). At most 256 points.

| Model | Kind | Window |
|---|---|---|
| 8 | lantern | the number dialog for its radius (§2) |
| 9 | event point | the point window (§10.1) |
| 10 | AI target point | the target window (§10.2) |

### 10.1 Point window

**code** (0x550a7c)
- Fields:
  - radius (byte 38): a spin with no limit in the form;
  - five words at bytes 28–36: the priorities for the green, blue, yellow and red side, and an active time;
  - "active at start" (40);
  - a list of local events, at most 5 (the add handler stops at 5, 0x551220).
- On OK, bytes 8–17 are cleared and the list is written back with its count (39). The model byte is left
  as it is.

### 10.2 Target window

**code** (0x55026c)
- This edits the same five words (bytes 28–36) and nothing else. It opens next to the cursor.

## 11. Options

**code** (open 0x55d7b0, save 0x55d984; section of the editor's own ini)
- **Text size.** 8, 10 or 12 points. An unknown stored value shows as 8.
- **Bold text.** A check box.
- **New events repeat.** A check box, default on in the shipped ini (data). It decides whether a new event
  starts as "many times" (§9.2).
- Values are stored as Y or N. The two text settings apply to the event window's message and question boxes.

## 12. How deletes on the map spread

**code** (delete brush 0x597588 in the main window; included here because it decides how records stay
consistent)

- **Deleting a building.** Later buildings move down. Only the building conditions of events (bytes 30–32)
  are renumbered. Armies' home buildings (25), buildings' links (293) and the hero presets' starting
  buildings are not.
- **Deleting an army.** Later armies move down. Building owners that pointed at it become 255 (none), and
  later owners drop by one. In events, the army bytes 15, 54–55, 67–68, 74, 75, 121–122, 123, 136, 142 and
  144 are renumbered. The patrol-change army (16) and the battle army (147) are not.
- **Deleting a point.** Later points move down. The lantern references of events (128–135) are renumbered.
- **Deleting an artefact** (§6.2) **or a named character** (§7). Nothing on the map is renumbered.

## 13. Notes for the format reference

These points refine [dtm-format.md](../dtm-format.md):
- **Buildings.**
  - This build reads and writes bytes 296 (i8), 297 (u16) and 299 (u16) through three hidden controls named
    as Community limits on a building's garrison: units, cost and strength (§4.2). The shipped maps do not
    hold such limits there, though (**data**): 68 of their 1,082 buildings have any of bytes 296–301 set. In
    64 of them bytes 297–300 equal the low bytes of goods 2–5 (bytes 138–144), and byte 296 equals the low
    byte of good 1 or is exactly 12, which is what clamping it to the hidden 0..12 control on a re-save
    produces; byte 301 equals the low byte of good 6 in 7 of the 8 buildings that have a sixth good. So the
    format note's "stale byte copy of the goods" describes the shipped data. Byte 301 is not touched by this
    editor.
  - Byte 293 of a dungeon entrance is the target entrance (code, §4.2). The shipped maps are thin evidence:
    of five entrances, three hold 0, one points at itself and one at a village, the last explained by building
    deletes not renumbering this byte (§12).
  - Byte 294 is derived on save (§4.3); all 1,082 shipped buildings agree (**data**).
- **Armies.**
  - Byte 8 is written 0 by this editor on every army save. Its world generator writes 4 instead (worldgen
    notes). The shipped maps hold 1–3 in 25 of 403 armies, none of them led by the matching hero class, so its
    meaning is **unknown**; it is not tied to the leader unit.
  - Byte 5 is derived from bytes 59 and 63 (§3.4); all 403 shipped armies agree (**data**).
  - Bytes 6 and 74 are computed (§3.4); no shipped value exceeds 65,000 (**data**).
  - Leader 255 is the special entry (§3.3).
- **Points.** Model 10 is an AI target point (code; no shipped map has one). Bytes 28–36 are edited by the
  point and target windows.
- **Hero presets.** Bytes 6–7 hold an experience value written by the editor (code; 0 in every shipped preset). Gold and mana are written as
  16-bit values.
- **Header.** Byte 0x120 is the built-in picture index 0..5 (shipped maps use 0–5).
- **Events.** Bytes 15 and 75 are army ids: the map's army delete renumbers them. Every non-zero value in the
  shipped maps is a valid army id.

---

## Razdor editor now → original

| Area | Razdor's editor now | Original editor |
|---|---|---|
| Unit editor | none | edits the session's unit table, live gold and tactical cost (§5.2), exports a new ini |
| Artefact editor | none | edits, copies and deletes artefacts, automatic price (§6.3), exports a new ini |
| Army model byte | chosen freely from 12 models | derived: 7 if inactive, else style + 4 |
| Army byte 8 | kept as loaded | always written 0 |
| Army tactical costs (6, 74) | shown read-only, never recomputed | recomputed on every save, capped at 65,000 |
| Army size | six slots of 0..255 each | at most 12 units with the leader, edit rolled back |
| Army spin ranges | aggression −128..127, speed −10..10, respawn 0..255, XP correction 0..255, garrison strength 0..255, leader level 1..10 | −100..100, −3..5, 0..30, 10..250, 0..90 (step 5), level unbounded raw |
| Home building choice | sets only the id | also copies the building's faction, attitudes and owner name |
| Faction change | separate "attitudes from row" button | picking a faction reloads the attitudes |
| New army | model 4, patrol on, faction 4 with the enemy row, garrison 50 | zero record, model from the menu, XP correction 100, window opens |
| Building byte 294 | manual check box | derived: any barracks unit |
| Building footprint | editable 1..12 | always taken from the picture |
| Building bytes 296–300 | overwritten with a byte copy of the goods | Community limits, kept unchanged |
| Ruins treasure | six artefact slots | five slots |
| Building income ranges | gold 0..2,500, max gold 0..25,000, defence 0..255 | gold 0..250, max gold 0..2,500, defence slider 0..50 saved as 50 − position |
| Building event list | no duplicates, closes up | duplicates allowed, up to 64 |
| Garrison rating, market test | none | shown in the building window |
| Event pickers of buildings | every building | towns not offered |
| New event | repeat 0, duration 0, once on | repeat 1 day, duration 1,440, once from the option |
| Copy event | clone at the end, picture kept, name kept | inserted after the original, references renumbered, name numbered, custom picture dropped |
| Event order | cannot be changed | move with renumbering |
| Event delete | closes up building and point lists, asks when the event is referenced | zeroes the entry without closing up, no prompt |
| Event duration range | 0..1,092 hours | 0..99 hours |
| Start date change | events stay | every event start shifts by the change |
| Alliance presets | allied 2 / neutral 0 / war −3, self 3 | the four matrices in §8.3 (neutral self 2, war not symmetric) |
| Hero presets | gold and mana 0..32,767, six spells, no experience, start position editable | experience, gold and mana 0..32,000, five spells, start army at most 11 units, position from the map |
| Scenario picture | index 0..255 only | index 0..5 cycled, plus import of a custom picture |
| Lantern radius | inline spin 0..24, new lantern radius 5 | number dialog 0..24, new lantern radius 10 |
| AI target points | absent (model 10 rejected) | target window for bytes 28–36; the point window edits them too |
| Options | none | text size, bold, new-events-repeat |
| Named character delete | remaps armies and events | only the open event's combos; stored bytes not remapped |
| Map deletes | renumbers homes, links, presets, patrol and battle armies | leaves those references untouched (§12) |
| Limits | 255 buildings, armies, points | 254 buildings, 255 armies, 256 points |

## Unknowns

1. What the game does with army leader 255 (the extra leader entry, §3.3).
2. The meaning of the second tactical cost (army byte 74; the side evaluation 0x4f61f4).
3. Whether the uninitialised counter of the artefact price (§6.3) is 0 in practice.
4. (Resolved.) The hero-preset store-back 0x540864 is missing from the decompile but was read in the
   disassembly: it is the inverse of 0x53fd3c (§8.2).
5. How the game reads condition unit 255, which the editor labels as the added unit.
6. Whether any other build exposes the Community limit spins of the building window (§4.2).
7. A newly placed army has faction 0 and zero attitudes until its window is saved. Closing the window without
   saving keeps that record. How the game treats faction 0 has not been checked.
