# The original map editor: map check, loading, saving and scoring

This file describes what the original scenario editor does when it checks a map for mistakes, when it
opens a map, and when it writes one: the rules of the map check window and the record each of its rows
leads to; how a file is read, what the loader repairs or fills in, and how it builds its cell grid; every
save format the editor knows (normal, uncompressed, demo and text dump), every change a save makes to the
data, and the header fields it writes; the playability score with its weights and its curve, and the
small text files the editor writes next to itself.

The source is `DTMapEdit.exe`, the map editor that ships with the Discord Times Community Update. It is a
32-bit Delphi program with image base 0x400000, and it was read statically: decompiled handlers,
disassembly and the decoded form resources. Rules are given in our own words; messages are described by
what they say, not quoted. Addresses are virtual addresses in that build and serve as evidence. Byte
offsets refer to the `.DTm` records in [dtm-format.md](../dtm-format.md), counted from 0. The record
windows that the check opens are in [records.md](records.md); the main window, its quick save and the
emergency save trigger are in [main-window.md](main-window.md).

Confidence tags:
- **code**: read in the editor's code, or in the form resources it loads.
- **data**: consistent with the shipped maps or the install's ini files, but not traced in code.
- **unknown**: not determined.

## 1. Conventions

- Ids of buildings, armies, points and events are 1-based list positions.
- "div" is integer division that truncates toward zero. "Round" is Delphi's rounding, which rounds
  halves to even in the default FPU mode. Where the code stores a float into an integer directly, it
  rounds the same way.
- **Mark.** The editor keeps a signed mark byte per cell, rebuilt by one routine (0x5a2ed4): the terrain
  code, replaced by minus the object class where an object stands, and by three negative values inside
  building footprints. The map check and the score read these marks. They are rebuilt on load, when the
  grid overlay is switched on and after a building is dropped, so other edits can leave them stale until
  then (code for the callers, the staleness is inferred).
- **Impassable cell** (used by the check and the score, code, 0x556d70, 0x5a23bb): the terrain code is 2
  (deep sea), 3 (lava), 9 (impassable swamp) or 15 (impassable snowdrifts), or the mark is −11 or one of
  −8..−5 (an object of class 11 or of classes 5 to 8).
- **Event title** shown in lists: the title up to its first `%`, so the flag script is not shown (code,
  0x4dc7b8).
- **Upkeep divisor** D: the `CostRecrutDiv` value of the install's global ini, read at start-up (code,
  0x59cbd4). The install sets it to 2 (data).

---

## 2. Map check

### 2.1 The window (code, 0x55653c, 0x556574, 0x556b48, 0x556c68)

The menu command creates the check window the first time and keeps it afterwards. Each time it runs the
whole check again, fills the table and shows the window modally. The table has four columns: what kind of
record the row is about (army, building, event or point, as four translated labels), the record's id, its
name, and the message. Rows appear in the order of §2.2: all armies by id, then all buildings, all events
and all points, each record's rules in the listed order. One record can produce several rows.

Below the table, a label gives the number of rows. The window grows with the rows up to 70 % of the screen
height and is centred vertically. When nothing is found the table shows one empty row and the go-to button
is disabled.

**Go to** (code, 0x558178). The button reads the selected row's kind and id and opens the matching record
window on that record: the army window, the building window, the point window or the event editor at that
event. The kind is recognised by comparing the label text, so in a translation where two kind labels were
equal the first one (army) would win. The table is not refreshed after the record window closes; the user
has to run the check again. There is no double-click action.

### 2.2 The rules (code, 0x556c68)

Before the rules, the check scans the artefact list of the install (the base list, not artefacts stored in
the map): the dearest price is the largest cost (at least 0), the cheapest price is the smallest absolute
cost among artefacts whose type is neither 9 nor 10.

The window holds 23 texts: the four kind labels and 19 messages. One message (missing description) serves
two rules, so there are 20 rules.

**Armies** (name column: the army name).

| # | condition | message meaning |
|---|---|---|
| 1 | the army's cell is impassable (§1) | the army stands on an impassable place |
| 2 | the army is inactive at start (byte 63 set) and no event activates it (results 121–122), starts a battle with it (147) or takes added units from it (142) | the army is never activated |
| 3 | the attitude towards the player (byte 65, signed) is 0 or more, the army is not a reserve, and no event that may fire many times (byte 141 = 0) has it as the met army (byte 74) | there is no meeting event |
| 4 | feudal style (byte 59 = 0), not a reserve, and upkeep is above income (below) | the upkeep, given as a number, is above the average income, given as a number |
| 5 | feudal style, not a reserve, and y + patrol radius (byte 61) is at least the map **width** | the patrol leaves the bottom edge of the map, which may crash |
| 6 | not a reserve and the description (third army string) is empty | there is no description text |

An army is a **reserve** when it is inactive at start and some event takes added units from it (result
142). Only rule 2 sets this, so an active army is never a reserve, and a reserve skips rules 3 to 6.

Upkeep and income for rule 4:
- Upkeep = the sum over the six troop slots with a unit of (unit cost div D) × count. The leader is not
  counted.
- Income = byte 80 × 10, plus the daily gold income (bytes 282–283) of every castle and fort (types 3–4)
  whose owner byte (292) is this army. For villages (type 2) the check sums income and maximum gold
  (bytes 284–285) and counts them; a village is counted when it lies within the patrol radius, measured by
  the game's distance function between the army's and the building's x/y, or when the army does not patrol
  (byte 60 = 0), and then any village counts. With n counted villages, income grows by
  ((incomes + maxima) div 2n) × min(n, 3).

Rule 5 compares with the width, not the height; the shipped maps are square (data).

**Buildings** (name column: the building name).

| # | condition | message meaning |
|---|---|---|
| 7 | village, castle or fort (types 2–4) with daily gold income 0 | no money income is set |
| 8 | the description (third string) is empty and the type is not a bridge (13, 14) | there is no description text |
| 9 | town, market or church (types 1, 6, 7) whose highest random price (bytes 335–336) is above the dearest artefact | the market's highest price is too large |
| 10 | castle or ruins (types 3, 12) whose garrison slots with a unit hold 0 units in total | the garrison is empty |
| 11 | town or castle (types 1, 3) whose barracks slots with a unit have a maximum count of 0 in total | no recruitment is set |
| 12 | town, market or church with none of the 64 words at 136–263 set, and either no random goods (byte 295 = 0), or a highest price above the dearest artefact, or a lowest price (333–334) below the cheapest artefact | the market is not set up |

**Events** (name column: the event title, §1). Rules 13 to 15 are exclusive by type.

| # | condition | message meaning |
|---|---|---|
| 13 | a local event (type 2) that is not subordinate (byte 140 = 0) and is in no point's and no building's event list | an unused local event |
| 14 | a quest (type 3) that no event completes (result 124–125) | a quest that is never closed |
| 15 | a rumour (type 4), not subordinate, in no building's list | an unused rumour |
| 16 | a subordinate event (byte 140 set) that no event chains to (138–139) | an unused subordinate event |
| 17 | the start time is the "relative only" value 1 036 800 000 and no event names it as its relative event (77–78) | an unused relative event |
| 18 | the event adds units (97–100) or gives artefacts (113–116) and its message (third string) is empty | there is no text for the reward |
| 19 | the artefact condition is on (byte 46), and for each of its three slots whose artefact is set, is not 135 and whose owner code (50–52) is 1 (the player): no event gives that artefact (113–116), no army carries it (50–52) and no building lists it among its 64 words at 136 | the artefact, named in the message, is not on the map, so the event can never fire |

Rule 19 gives one row per slot. Hero presets, building spells and garrison items are not searched.

**Points** (name column: the point's x and y joined by a dash).

| # | condition | message meaning |
|---|---|---|
| 20 | no attached events (byte 39 = 0) and radius 0 (byte 38) | an empty point |

---

## 3. Opening a map

### 3.1 Names and dialogs (code, 0x59ffdc, 0x5a6c20)

Opening asks to save a modified map first (yes, no, cancel). The open dialog offers the normal and the demo
map types; its filter entries map to file extensions by position: the first keeps the typed name, the
second asks for the text dump (`.DTD`) and the third for a demo map (`.DTS`). The loader then picks the
file to read from the extension, compared case-sensitively:
- `.DTD`: read `<name>.DTm`, then import the text dump (§5).
- `.DTS`: read `<name>.DTs`.
- anything else: read `<name>.DTm`.

The same loader runs at start-up for the last map and for a map handed over by a second launch.

Before it even checks that the file exists, the loader clears the modified flag, sets the current file name
and empties the name, owner and description strings of the buildings of the old map. If the file does not
exist it stops there and returns failure; the old map stays in memory without those strings. At start-up a
failure makes the editor start a new map instead. (code, 0x5a6d3c–0x5a6e0d)

### 3.2 Reading the file (code, 0x4d54c4)

A file shorter than 12 bytes or without the `AIpf` magic is read as a raw payload. Otherwise the reader
undoes the scramble named in byte 7, then decompresses by the code in byte 6: below 10 with zlib (inflate),
10 to 19 with bzip2. A size that does not match the header's sets a flag that nothing reads.

### 3.3 Header checks (code, 0x5a6e90–0x5a6fb7)

- The first nine bytes must be the signature up to and including the dot before the version digit, and
  width and height must both be below 801. Otherwise the editor shows its map-load error with the file
  name and stops.
- A width above 200 shows a warning about large maps and goes on.
- The version digit (byte 9) selects the upgrades of §3.7. A map whose signature is not exactly the
  version-4 one is marked modified at once.

### 3.4 Sections (code, 0x5a6fe0–0x5a7260)

Header byte 0x117 is the demo flag. With 0 the sections are read in file order: terrain, objects,
buildings, armies, points, events, then the custom-artefact section. With 1 the order is objects, custom
artefacts, points, terrain, events, armies, buildings. With any other value no section is read, though the
record counts are still taken from the sizes. Counts are size div record size; the custom-artefact section
(size at header 0x34, records of 230 bytes) is appended after the install's artefacts.

Then come the strings, in the order of the format reference, with two strings (name, description) per custom
artefact between the event strings and the named characters. Every string is trimmed as it is read: all
leading and trailing bytes of 32 or below (spaces, line breaks, other control characters) are removed, so a
string made only of such bytes becomes empty, and a later save writes the trimmed text (code, 0x5a6a98,
0x460f9c). The title and the named-character names are then cut to 64 characters. The scenario picture and
each event picture with a positive size follow.

### 3.5 Event list cut (code, 0x5a757b–0x5a76cb)

The loader looks for the first event, in id order, one of whose non-empty strings starts with a control
character (a byte below 32). If there is one, the event list is cut just before it, and after loading the
editor shows its map-load error with the file name. The strings of the later events were already read, so
the rest of the file stays in step.

The test runs on the strings after the trimming of §3.4, which has already removed every leading byte of 32
or below, so it can never succeed and no map is cut (code, 0x5a757b, 0x5a6a98). In the shipped map
"Устье Трейна", events 36 to 38 have a question that is only a line break (data); the editor opens all 211
events, with those three questions empty, and a save writes them empty.

### 3.6 Text import

See §5.2.

### 3.7 Old versions (code, 0x5a8424–0x5a87f0)

- **Version 1**: building event lists were 64 four-byte values; their low words become the 64 two-byte
  ids. The 128 bytes at 136 are cleared and the six goods slots are filled from the signed bytes 301–306.
  Point event lists are converted the same way (five ids).
- **Version 2**: a point of model 6 (byte 5) becomes model 8. An active army's map model (byte 5) becomes
  its byte 59 + 4 and an inactive army's becomes 7; byte 4 is set to the army's id.
- **Version 3**: an army garrison-strength byte 82 of 0 becomes 50, and any other value becomes 0.
- Version 4 maps are taken as they are.

### 3.8 Building the cell grid (code, 0x5a8810–0x5a945b)

The whole cell grid is cleared and the map size is set from the header.

1. **Terrain.** The run-length pairs fill the cells row by row. The renderer's terrain copy gets a
   one-cell border: the row above the map repeats row 0, the row below repeats the last row, and the
   columns left and right repeat the first and last columns.
2. **Objects.** Records of class 13 or more are dropped. A record of class 1–8 goes to the cell's first
   object slot, any other class to the second. A later record replaces an earlier one in the same slot. For
   classes 1–8 the footprint size n is the sprite index div 10 (the object covers the n × n cells ending at
   its x/y); if x or y is below n − 1 it is raised to n − 1, so the footprint stays on the map. In the
   shipped maps no cell holds two objects of one slot, no class is 13 or more and no footprint needs the
   raise (data).
3. The marks (§1) are rebuilt.
4. **Buildings.** A building beyond the right or bottom edge is moved to the last column or row. If its
   cell already holds a building, its picture goes to the cell one step right and down, and the building's
   x and y move there too (once, without a second test). The footprint (bytes 289–290) is set from the
   picture's size table, replacing what the file stored. Towns and ruins (types 1, 12) get byte 357 set
   to 1.
5. **Armies.** The cell holds the army's id and model. An experience correction (byte 71) of 0 becomes
   100.
6. **Hero starts and points.** For each hero preset whose start x and y are both non-zero and inside the
   map, the cell shows that hero's figure and a visibility circle of radius 5 is marked for the fog
   preview. Each point's cell shows its figure; a point with a radius that is active at start marks its
   circle.

Finally the minimap, the scroll bars, the building and army menus, the artefact order and the caption are
refreshed. The caption shows the stored playability score (§6.8); loading does not compute it.

---

## 4. Saving a map

### 4.1 Entry points (code, 0x5a0194, 0x5b3714, 0x5a5fa4, 0x5ab588)

- **Save** writes straight to the open dialog's folder plus the current file name, unless the map is
  still called `New.DTm`; then, and for **save as**, the save dialog appears. Its filter entries map by
  position: first, the typed name; second, the text dump (`.DTD`); third, uncompressed (`.DTZ`); fourth,
  demo (`.DTS`). After a dialog save the caption is refreshed.
- **F2** saves like save without a dialog (main-window.md §17.1).
- **Emergency save.** When the minimap or map drawing reports a DirectDraw error, the editor saves to
  `ErrorSave.DTm` in the map folder, shows the error and ends the program.

The new-map, open, create and close commands ask to save a modified map first. No other command saves.

### 4.2 Extension rules (code, 0x5a46b4–0x5a477f)

The demo flag (header 0x117) is cleared first. Then, comparing case-sensitively:
- `.DTD`: the dump is wanted; the map is written as `<name>.DTm`.
- `.DTZ`: no compression is wanted; the name is then changed to `<name>.DTm`, so the uncompressed file is
  written under the normal extension.
- `.DTS`: demo; the file is `<name>.DTs` and the demo flag is set.
- anything else: `<name>.DTm`.

So re-saving a map opened as `.DTs` writes a normal `.DTm`, because the current name's `.DTs` does not match
`.DTS`.

### 4.3 Changes a save makes to the data (code, 0x5a47c1–0x5a4c3f, 0x5a5028–0x5a5050)

In this order, on the map in memory (the changes stay after the save):
1. **Building type from picture.** A building of type 0 takes its picture type (byte 5). A house (type 8)
   whose picture variant (byte 4) is 2 to 4 becomes an obelisk (15); variant 5 or 6 makes it ruins (12).
2. **Home buildings.** For every army, in id order, that has a home building (byte 25) and is active at
   start (byte 63 = 0): that building's owner byte (292) becomes the army's id, and its faction (337) and
   four attitudes (338–341) are copied from the army (64, 65–68). With two armies on one home the later one
   wins.
3. **Goods list.** In each building, when the first goods slot (136) is empty, slots 2 to 6 move one place
   left and slot 6 is cleared. This happens at most once: the loop stops at the next empty slot, so a gap
   after a filled slot, or a second empty slot at the front, stays as it is. If the owner byte is 1 to 254,
   the faction and attitudes are copied again from that owner army.
4. **Custom artefacts are dropped.** The artefact count is reset to the install's list, so artefacts read
   from the map's custom section are gone from memory and are not written (header 0x34 is 0).

### 4.4 Rebuilding sections from the grid (code, 0x5a4c44–0x5a4fc0)

- **Terrain** is run-length coded row by row: a new pair starts when the code changes or the run reaches
  256 (at most 100 000 pairs).
- **Objects** are not taken from a list: the editor walks the cell grid row by row and writes the first
  object slot, then the second, of every cell that has one (at most 400 000). The result is sorted by
  (y, x) with at most two objects per cell.

### 4.5 Header fields written (code, 0x5a4fa0–0x5a50f2)

- The signature, version 4.
- The **save counter** (u16 at 0x124) is raised by one on every save.
- The section sizes and, after the sections and the text marker, the text offset.
- 0x34 = 0 (no custom artefacts).
- The named-character count (0xEE) and their unit classes (0xEF onward).
- Everything else is kept from memory: the generator seed (0x14), set by the new-map command to the
  current random seed and by the generator, the playability score (0x122) and quest count (0x126) of the
  last scoring (§6), the demo flag of §4.2.

The shipped maps have 0 at 0x122–0x127, so they were saved by an earlier editor or cleaned (data).

### 4.6 Section order and container (code, 0x5a50f2–0x5a5655, 0x4d5790, 0x4c2938)

The payload is the header, the sections, the 8-byte text marker, the strings, the scenario picture and the
event pictures. Normal and uncompressed saves use file order; a demo save writes objects, the empty
custom-artefact section, points, terrain, events, armies, buildings. In a demo save the next-map name gets
the `.DTs` extension. Then:

| save | file | container |
|---|---|---|
| normal | `.DTm` | `AIpf`, code 19 (bzip2 level 9), no scramble |
| uncompressed | `.DTm` | none: the raw payload |
| demo | `.DTs` | `AIpf`, code 9 (zlib deflate level 9), scramble mode 1 |

The editor's own reader opens all three (§3.2).

### 4.7 After the save (code, 0x5a5be4)

The modified flag is cleared and the current file name becomes the written file's name. A text dump is
written now if one was asked for (§5.1).

---

## 5. Text dump (`.DTD`)

### 5.1 Writer (code, 0x5a5660–0x5a5bdc, 0x5a45bc)

Next to the map the editor writes a plain text file named `<name>.Eng` when the title's first character is
an ASCII letter (or one of the six characters between `Z` and `a`), else, and for an empty title,
`<name>.Rus`. Every line is one
text; a line break inside a text is written as a space, `%` and `/`. The file has these blocks:
- `[Head]`, then the title, the description and, only if it is not empty, the campaign name. The next-map
  name is not written.
- For each building, `[B#<id>]` and its three strings.
- For each army, `[A#<id>]`, the name up to its first `%`, the leader name and the description.
- For each event, `[E#<id>]`, the title up to its first `%`, the question only if the question flag (76)
  is set, and the message only if it is not empty.
- `[I#<id>]` blocks for custom artefacts: never written, since the save has just dropped them.
- For each named character, `[U#<id>]` and the name.

### 5.2 Reader (code, 0x5a7930–0x5a8417, 0x5a6afc)

After reading the binary map, the loader opens `<name>.Eng` or `<name>.Rus`, chosen by the first character
of the title just loaded. It turns each of the four spellings of the line-break marker (with or without a
space on either side) back into a line break, and counts lines for its error message.
- The first line must be `[Head]`; otherwise the file is ignored. Then the title (cut to 64 characters),
  the description, and a campaign name if the next line does not start with `[`.
- The blocks must come in the writer's order. Each block type is read in a loop while lines start with its
  tag. The id is the number between `#` and `]`.
- In a building or army block the first line is taken, and each further string only while the next line
  does not start with `[`, so missing trailing strings keep their old value.
- In an event block the line replaces the title, and the old title's flag script (from its first `%`) is
  appended again. The question is read only when the event's question flag is set; then the message.
- A tag whose number cannot be read shows an error naming the line, once per file, and reads no
  further line, so the loop sees the same tag again and never ends (code, 0x5a7acf–0x5a7c63). A
  block numbered 0 stops with a range error.
- Ids are not checked against the record counts.

---

## 6. Playability score

### 6.1 When it runs (code, 0x5b3960, 0x5a0724)

Only the score button computes the score; saving and loading do not. The routine adds positive points s for
the map's content, subtracts penalties, then maps s to the score through a curve (§6.7). It also counts the
quests and appends a line to `MapData.Txt` (§6.9). W is the map width.

### 6.2 Content points (code, 0x5a0779–0x5a134a)

**Armies.** For each army: byte 80 div 50. For each of its three carried artefacts: take the cost; a
negative cost c counts as (−c) div 1000; then divide by 500 for artefact types below 8, by 2000 for others;
at most 10 each.

**Buildings.** By type (code, jump table 0x5a0945):

| type | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 15 | 0, 13, 14 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| points | 4 | 3 | 4 | 3 | 2 | 3 | 3 | 1 | 4 | 3 | 2 | 5 | 1 | 0 |

A building with random goods for sale (byte 295 ≠ 0) adds 2 + its highest price div 1500.

**Events.** Each event gets a value e:

| feature | points |
|---|---|
| type local / quest / rumour / global | 15 / 40 / 5 / 0 (each quest also counts towards the quest count) |
| a met army (74) | +10 |
| each of the conditions "event happened, yes" (56), "event not happened" (61), "event happened, no" (69) when on: first id set and second id empty / both ids set | +10 / +15 |
| a flag condition in the title's flag script (`=X` or `=/X`) | +15 |
| the question flag (76) | +20 |
| the artefact condition (46) | +10 |
| the building condition (29) | +5 |
| the defeated-armies condition (53) | +10 |
| the named-squad condition (36) | +5 |
| a relative event (77) | +15 |
| a first activated army (121) | +5 |
| a spell on the player (81) | +10 |
| each gained artefact (113–116): its value v, see below | +v, and +2 once if any |
| a positive gold change (85) | +5 |
| a negative gold change | +2 |

The value v of a gained artefact is its cost, a negative cost c counting as (−c) div 500, divided by 250 for
types below 8 or by 1500 for others, at most 15. Each v, and for a positive gold change the value
gold div 2000 + 1, also goes into a reward statistic (count, sum, sum of squares) used in §6.5.

Then two corrections:
1. **Near duplicates.** For every other event, the editor compares the two messages and the two questions
   by a word-level edit distance (punctuation and line breaks count as spaces). The distance d starts at
   −1 and adds the message distance when both messages are non-empty and the question distance when both
   questions are non-empty. When d is not −1 and below 2 the other event counts as a near duplicate. With k
   near duplicates, e becomes e div k. Exact copies with only a message give d = −1 and are not counted.
2. **Text length.** Unless the title starts with `-`: t = Round(√(question length + message length)) − 9.
   When t ≤ 0, e ≤ 10, and the event is neither the victory nor the defeat event, e becomes t if e was
   positive, else −10, and the event counts as weak. Otherwise e grows by t div 2.

s grows by e for each event. After all events: s += 5 × armies + 5 × (events − weak events) + points. A
first campaign map (header 0x10F = 1) adds 100, a later campaign map (2) adds 200.

### 6.3 Terrain variety (code, 0x5a1430–0x5a1d80)

For every cell at least 3 cells away from each edge, the editor counts how many cells around it differ from
it, in three square rings: the 8 cells at distance 1, the 16 at distance 2 and the 24 at distance 3 (offset
tables 0x5becdc, 0x5bed10). The loops take the x range from the height and the y range from the width, which
only matters for a map that is not square. The counts are cumulative. The cell is **uniform at level 1** when fewer than 2
of the first ring differ, at level 2 when fewer than 3 of the first two rings differ, and at level 3 when
fewer than 4 of all three differ.

Three families of counters are kept:
- **land** (L1–L3) and **water** (W1–W3) compare marks (§1), and the cell goes to water when its mark is
  0, 1 or 2 (shallows, coastal water, deep sea) and to land otherwise (including objects and buildings);
- **terrain** (T1–T3) compares terrain codes, for every cell. It is only reported.

The report (§6.9) gives each count as a percentage of (W − 6)². Then every count is divided by
(W div 50)², so a map narrower than 50 makes the routine fail with a division by zero. Penalties:

| counter | above | s loses |
|---|---|---|
| L1 | 400 | (L1 − 400) div 10 |
| L2 | 100 | (L2 − 100) div 3 |
| L3 | 50 | L3 − 50 |
| W1 | 800 | (W1 − 800) div 20 |
| W2 | 400 | (W2 − 400) div 6 |
| W3 | 200 | (W3 − 200) div 2 |

### 6.4 Building spacing (code, 0x5a1dc0–0x5a1ffc, 0x4db648)

For each building of types 1–5 and 7–12, the editor finds the nearest other building of those types. The
distance is (2 × max(|dx|, |dy|) + min(|dx|, |dy|)) div 2 between the footprint centres (x + size x div 2,
y + size y div 2); a building with no other building of those types gets W + H. The average of these
nearest distances (integer division; W when there are no such buildings) above 10 costs
(W div 50) × (average − 10)².

### 6.5 Reward spread (code, 0x5a2049–0x5a20a3)

From the reward statistic of §6.2 the editor takes the mean M and the standard deviation S (both 0 when
there are no rewards). When M + S is above 5, s loses Round((M + S − 5) × the sum of all reward values).

### 6.6 Penalties (code, 0x5a20a6–0x5a2b57)

| what | s loses |
|---|---|
| a castle with no gold income | 10 |
| a village with no gold income | 5 |
| ruins with no first garrison unit (314) and owner byte 0 | 3 |
| a town, market or church without random goods (295 = 0) | 5 |
| a starting building of any class (353–355) with gold income above 150 | income − 150, plus (income − 150) div 2 for each barracks slot whose unit costs more than 150 |
| an army on an impassable cell | 50 |
| an army without a leader (26 = 0) | 15 |
| a quest that no event completes (124) | 50 |
| a global event attached to a point | 25 for each attachment |
| a point on an impassable cell | 25 × its number of events |
| chain loops and deadly chains from points (below) | 25 per deadly chain, and Round(500 × loops / number of points) |
| on a standalone map (0x10F = 0), each hero preset with a start x: gold 0 / gold above 1000 | 100 / (gold − 1000) div 50 |
| no events at all | W |
| more than one quest per seven events | 50 × (quests − events div 7) |

**Chains from points.** For each event attached to a point, the editor follows the chained event (138)
step by step, writing the visited ids into a text separated by `~`. A step whose id is found anywhere in
that text, as a substring, ends the walk as a loop; so id 1 after id 12 already counts as a loop. A loop
whose repeated event (the one just found in the text) casts a spell with a negative fixed hit-point change
counts as deadly instead of as a loop.
A walk that ends on the defeat event also counts as deadly.

### 6.7 Curve (code, 0x5a2ba4–0x5a2c45)

- If s ≤ W the score is 0.
- Else v = √((s − W) / √W) × 11, kept with four decimals (rounded). Above 100, v becomes
  (v − 100) × 0.75 + 100.
- The score is Round(v), stored as a 16-bit value at header 0x122. The quest count goes to header 0x126.

### 6.8 Display (code, 0x5a04d8)

The status line shows the quest count and the score from the header. Its colour changes at the score
thresholds 1, 50, 100, 150, 200, 300 and 400 (red for 0, then orange, olive, two greens, blue, purple and
magenta).

### 6.9 MapData.Txt (code, 0x5a19c0–0x5a2b9f, 0x5a2c5f–0x5a2cf3)

Each scoring appends one line to `MapData.Txt` in the editor's folder (the file is created when missing).
Fields are separated by `|`; numbers are right-aligned to 5 characters:
1. the title, padded with spaces to at least 30 characters;
2. s after §6.2;
3. T1, T2, T3 as percentages with one decimal, side by side; then L1–L3; then W1–W3 (each group one field,
   counted before the division of §6.3);
4. s after §6.3;
5. s after §6.4;
6. s after §6.6, before the curve.

The score itself is not in the line.

---

## 7. Other files

- **Battle.Sav** (code, 0x57e3c4, 0x57e77c): the test battle's placement in the editor's folder, an ini text
  with one section per side (`Army1`, `Army2`) and one key per grid cell `U<row><column>` (rows 1–3,
  columns 1–6) holding the unit type's id, or 0 for an empty cell. Loading rebuilds the units in row order
  (testers.md).
- **Unit and artefact lists** (code, 0x55a430, 0x553a6c): the unit and artefact editors export
  `Rus_Units.New.Ini` and `Rus_Artefacts.New.Ini` in the ini layout the game reads, with one section per
  entry titled by its number and name, keys with value 0 left out, type, bonus and magic school written as
  names, and an empty numbered section for nameless slots; plus `Units.Rus` and `Artefacts.Rus` with the
  name and description per entry. Details are in records.md §5–6.
- **Editor options**: on close the editor writes the last map, the map folder and the building-place option
  to its own ini file (main-window.md).

---

## Razdor editor now → original

- **Map check.** Razdor's check (`src/editor/validate.rs`) tests file integrity: sizes, ids in range,
  dangling references, pictures, string encoding. It blocks saving on errors. The original's check is a
  separate window of 20 design rules (§2.2) that never blocks saving, and the original has no integrity
  check at all. None of the 20 rules exists in Razdor, and Razdor has no go-to from an issue to the record
  window.
- **Formats.** Now as the original (`src/editor/mapfile.rs`, `src/dt/container.rs`): the
  save dialog offers the four file types and the extension rules of §4.2 pick the file
  (`.DTZ` raw under `.DTm`, `.DTS` a zlib, scramble-1, demo-order `.DTs` with header 0x117 = 1,
  `.DTD` the map and its dump); the open dialog's three types follow §3.1, and the editor's
  stream reader takes raw, zlib and bzip2 files (§3.2). Normal saves always use code 19. The
  text dump is written and read as §5 (`src/editor/dump.rs`); where the original's reader would
  loop forever on a tag number it cannot read, or stop on block 0, Razdor stops the import and
  says at which line.
- **Round trip.** Now as the original: a save makes the changes of §4.3–4.5 in memory (one undo
  step), with objects written from the cell grid (`src/editor/grid.rs`) and the save counter
  raised; the shipped maps change only at 0x124 (and the trimmed strings of "Устье Трейна").
  Loading trims strings, cuts the title and named characters to 64 characters, clamps and
  moves objects and buildings, resets footprints from the install's pictures, sets byte 357 for
  towns and ruins and fills byte 71 = 100 (§3.8). Razdor's integrity check still refuses a file
  that would not read back. A missing or unreadable file leaves the open map untouched (the
  original first empties its building strings and renames it, §3.1).
- **Custom artefacts.** Now as the original: the loader reads the section of header 0x34 with
  two strings per artefact; a save drops them and writes 0x34 = 0.
- **Old versions.** Now as the original: versions 1–3 open with the upgrades of §3.7 (each
  digit only its own step) and are marked modified; the game itself still refuses them.
- **Playability.** Razdor has no score, no header 0x122/0x126 writing and no `MapData.Txt`.
- **Saving place and safety.** Razdor saves to the user's folder through a temporary file, asks before
  touching the game's folder, and has no emergency save. The original writes in place in its map folder
  and saves `ErrorSave.DTm` on a DirectDraw failure.
- **Strings with leading or trailing blanks.** Now as the original: trimmed on load, so the
  three questions of "Устье Трейна" that are only a line break become empty; all 211 events
  open (§3.5).

## Unknowns

- Whether the open dialog's visible filter entries match the three positions the code expects; the
  filter text is built from a translated label at start-up and was not decoded fully.
- What the stream reader does with compression code `C` (it is treated as 0) and why.
- The exact meaning of byte 357 that the loader sets for towns and ruins (the building window shows it as a
  box; see records.md).
- Why the version-3 upgrade clears a non-zero garrison-strength byte.
- Why demo maps use a different section order (protection of the demo build is a guess).
