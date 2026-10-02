# Map editor: the world generator and random armies

This is a description of the random world generator in `DTMapEdit.exe`, the map editor that comes with the
Discord Times Community Update. It also covers the random army builder that the generator uses. The generator
has three steps, each run on its own by the user: **buildings and roads**, **economy**, and **armies and
garrisons**. The aim is parity: a reimplementation that follows this text, with the same random generator
state and the same game data, should build the same world, including its quirks.

Confidence tags: **code** means read from the disassembly or decompile (an address is given as evidence),
**data** means taken from the install's files, and **unknown** means not established. Addresses are virtual
addresses in `DTMapEdit.exe` (image base 0x400000).

Terms used here. A *cell* is one map square. Its *terrain* code is the one the map format gives
(`dtm-format.md` §4: 0 shallows, 1 coastal water, 2 deep sea, 4 road, 5 grass lowland, 6 grass plain,
7 dry plain, 8 marsh, …). A cell also has an *object layer* (hills, rocks), a *forest layer* (trees,
thickets), a *building code* (picture type and variant, set on a building's anchor cell only), and an
*occupancy mark*. The mark holds the terrain code on a free cell, minus the object class on a cell under an
object or tree, and a fixed negative value on every cell of a building footprint. A building's *anchor* is
the bottom-right cell of its footprint (`dtm-format.md` §6). Its *centre* is the anchor minus half the
footprint, rounded down, on each axis. `W` and `H` are the map's width and height in cells. *R(n)* is one
draw of the random generator in §1 (0 when n = 0).

## 1. The random generator

- **code** (0x4dc6b8) The editor uses the game's linear congruential generator: seed ← seed × 214013 +
  2531011 (mod 2³²); R(n) = (bits 16–30 of the seed) mod n, or 0 when n = 0. The seed moves on even when
  n = 0. For a negative n the result is between 0 and |n| − 1, because the remainder takes the dividend's
  sign.
- **code** (0x4dc6b0) Randomize sets the seed from the low 32 bits of the CPU time-stamp counter. Only the
  buildings-and-roads step calls it (0x57112b), and it does so after that step has already made its first
  draws (§3.1).
- **code** (0x595f9d) **Every building placement reseeds the generator.** That includes towns, villages,
  junction buildings, ruins and each bridge piece. The new seed is 11·x + 7·y + 3·variant + picture type,
  where x, y is the anchor. The placement then makes its name draws (§3.5). So everything after a successful
  placement depends only on the last building placed and on the draws made since. The time-stamp seed decides
  just the draws up to the first placement.
- For parity, take "the seed" to mean the value Randomize produced. The draws made before it (§3.1, road
  clean-up) use whatever state the editor already had.
- **code** The generator code is compiled with range checking. An array index outside its declared range
  raises a run-time error that aborts the step half-way, leaving whatever it had already written. The cases
  that matter here are noted where they occur (§3.1, §3.2, §3.6, §3.9, §4, §5.4).

## 2. Settings and their defaults

**code** (form TMakeWorld, VMT 0x56dafc; FormCreate 0x56e858; Run button 0x5769cc). The window has three
tabs. Run executes only the step of the active tab.

- **Chance lists.** Ten drop-downs, five on the first tab (towns, castles, villages, ruins, other) and five
  on the third (armies in towns, castles, villages, ruins, other buildings). All ten have six entries (**data**:
  the editor's language ini) and start on the third entry. The chance is (5 − index) × 20 %, which gives
  100, 80, 60, 40, 20 and 0; the default is 60 %. A roll succeeds when R(100) < chance (**code** 0x5703b2,
  0x57466c).
- **Income grid** (economy tab). One row with four columns: town gold 100, castle gold 50, village gold 75,
  village mana 75. If a cell is not a number, the economy step uses 100, 50, 75 and 75 in its place (0x573262).
- **Trade grid.** Three rows (town, market, church) of (minimum price, maximum price, number of goods). The
  defaults are town 25 / 3000 / 10, market 125 / 2000 / 8 and church 25 / 250 / 6. The fallbacks for a
  non-number cell are town 100 / 5000 / 8, market 100 / 2500 / 5 and church 50 / 200 / 5.
- **Library grid.** Two rows (town, church) of (minimum price, maximum price, number of spells). The defaults
  are town 250 / 5000 / 5 and church 100 / 2000 / 3, which are also the fallbacks.
- **Budget ranges** (armies tab). Sixteen spin boxes, from 0 to 50000 in steps of 100 (**data**: form). Their
  defaults depend on W, with W/2 rounded down:

| range | for | low | high |
|---|---|---|---|
| T | town armies | 2W + 500 | 3W + 500 |
| C | castle armies | W + 200 | 2W + 200 |
| V | village armies | 200 | W/2 + 200 |
| R | ruin armies | 2W + 500 | 4W + 1000 |
| O | armies of forts, markets, churches | W + 500 | 4W + 500 |
| Tg | town garrisons | 2W + 1500 | 3W + 1500 |
| Cg | castle garrisons | 2W + 250 | 3W + 500 |
| Rg | ruin garrisons | 3W + 500 | 6W + 1000 |

- **Minimum point** (0x576988). Ten toggle buttons laid out like a numeric keypad, plus a button for no point,
  which is the default. Button 7 is the top-left corner (0, 0), 8 is (W/2, 0) and 9 is (W, 0). Button 4 is (0, H/2),
  5 is the centre and 6 is (W, H/2). Button 1 is (0, H), 2 is (W/2, H) and 3 is (W, H) (**code** 0x57474b;
  **data**: button tags and positions).
- **Two check boxes** on the armies tab, called here the *unowned-only* box and the *enemies-only* box (§5).
- **Counters.** When the window opens it counts the existing buildings by type and shows the counts
  (0x56e3f8). The two bridge cases in that count can never match, so existing bridges are not counted.

## 3. Step 1: buildings and roads

**code**, function 0x56ffa4 unless another address is given. This step throws away all buildings and the
road layer, then builds new ones. Armies are left alone, so their home and owner links go stale.

### 3.1 Clearing the old layer
1. All ten counters are set to 0, the marks are built again from the map (0x5a2ed4, called at 0x5700b2, the
   routine of `mapcheck-files.md` §1), and the "generating" flag is switched on (0x5700b7).
2. Every road cell inside an existing building's footprint becomes grass plain (6), and its mark is set to 6.
   Buildings go in table order, each footprint x outer and y inner. The cell reads are range-checked, so a
   footprint that reaches past the left or top edge (a building placed at the edge, see §3.5) aborts the step
   at that cell.
3. Every interior cell is visited, x from 1 to W−2 in the outer loop and y from 1 to H−2 in the inner loop.
   For a **road cell**:
   - If more than 4 of its 8 neighbours have a forest-layer object, the step draws a direction R(8) until it
     finds a neighbour with a forest object (0x5705a7), and copies that object onto the cell. The directions
     are numbered 0..7 in the order NW, N, NE, E, SE, S, SW, W.
   - The cell takes the terrain of the cell below it if that one is not road. If it is, the cell takes the
     terrain of the cell to its right if that one is not road; failing that, the cell below and to the right.
     If all three are road, the cell stays road. If the mark was 4, it becomes the new terrain.

   For every interior cell, road or not: its building code is cleared. The **placement mask** is then set if
   the terrain is 6 or 7 and the mark is not negative, and cleared otherwise. Border cells are never written
   by this pass, so their mask keeps any older value.
4. **Erosion**, five passes in the same cell order, updating in place. A masked cell is unmasked if fewer
   than 3 of its 8 neighbours are masked. It is also unmasked if its north and south neighbours are both
   unmasked, or if its east and west neighbours are both unmasked. After the five passes, the mask of the four
   map corners is cleared.
5. The building table is emptied (count 0, all 256 records zeroed). The view is scrolled to (0, 0), because
   placements are given relative to the view (0x5a3864). Randomize is called (§1).

The buildings standing when the step starts keep their footprint marks from item 1, which count as negative,
so a second run cannot place buildings where earlier ones stood, even after a load (which rebuilds the marks,
`mapcheck-files.md` §3.8). Only their former road cells are freed, by item 2 (**code**). Stale marks of
earlier edits, on the other hand, do not count, since item 1 replaces them.

### 3.2 Sectors and quadrants
- The map is cut into sectors of 50 × 50 cells, with n = W div 50 sectors per side. The **same n is used for
  rows**, whatever H is. The step visits k = 0 to n², which is **n² + 1** sectors. Sector k has row k div n,
  column k mod n and centre ((k mod n)·50 + 25, (k div n)·50 + 25). The last visit (row n, column 0) lies below
  the grid. Its searches start at y = 50n + 12 or later and only move down from there (§3.3), so on the
  editor's own square sizes (W = H = 50n) it can never place anything. It still makes all of its draws. It can
  place buildings only on a loaded map whose height reaches past 50n + 12.
- If W < 50, then n = 0 and the first modulo divides by zero. The step stops with an exception (**code**; what
  the user then sees is **unknown**).
- For each sector, **R(12)** picks one of the 12 ordered (town, castle) pairs of distinct quadrants. The two
  villages take the other two quadrants, the lower-numbered one first. Quadrant q starts the search at
  (centre x − 13 + (q mod 2)·25, centre y − 13 + (q div 2)·25), so the four starts sit 12 or 13 cells from the
  centre. The table, as (town, castle, village, village): 0 (0,1,2,3), 1 (0,2,1,3), 2 (0,3,1,2), 3 (1,0,2,3),
  4 (1,2,0,3), 5 (1,3,0,2), 6 (2,0,1,3), 7 (2,1,0,3), 8 (2,3,0,1), 9 (3,0,1,2), 10 (3,1,0,2), 11 (3,2,0,1).
- Then come, in order, the town, the castle, the first village and the second village. For each one:
  - a picture: R(number of pictures of the type) + the first picture number of that type;
  - a chance roll: R(100) < the chance from that type's drop-down;
  - on success, the spiral search of §3.3 with radius 30.

  **The picture is drawn even when the roll then fails.** A town that is placed is recorded in a 16 × 16
  sector table, at index row + 16·column. The town, castle and village counters go up on every successful
  search.
- The table index is range-checked on both axes (0..15; **code** 0x57147f). For n < 16, a town placed by the
  extra visit lands in a slot (column 0, row n) that the road pass never reads (§3.6). For n = 16 (W of 800 or
  more) its row index 16 is out of range, so a town placed there would abort the step with a range error. On
  an 800 × 800 map this cannot happen, because the extra visit places nothing.

### 3.3 The "spiral" search
**code** (0x56f410, test 0x56f36c). The footprint (sx, sy) comes from the picture. A step counter k starts at
4. Each round makes k div 4 single-cell moves in direction k mod 4, where 0 is +x, 1 is +y, 2 is −x and 3 is
−y. Every cell reached is tested, and then k goes up by 1. The search stops at the first cell that passes, or
when k reaches 4 × radius.

The moves are 1, 1, 1, 1, 2, 2, 2, 2, 3, …, so the path traces **closed squares of growing size that have the
start as their top-left corner**, up to side radius − 1. Each square's walk ends back on the start, and that
cell is tested too. So the first tests are (x+1, y), (x+1, y+1), (x, y+1) and then the start itself, and the
start is tested again at the end of every later square. The search covers only the square that reaches toward
+x and +y from the start.

A cell passes when every footprint cell, from x − sx + 1 to x and from y − sy + 1 to y, is inside the map and
masked. On success the building is placed at that anchor (§3.5). The mask is then cleared over the box
x − sx − 1 … x + 2 by y − sy − 1 … y + 2, with no bounds check. The search reports success even if the
placement itself was refused.

### 3.4 Cost grid
**code** (0x56f5d0). The grid holds one value per cell. In the first pass:

- Each cell starts at 1000 plus the cost of its terrain: 100, 150, blocked, blocked, 1, 8, 5, 7, 15, blocked,
  10, 8, 8, 10, 12, blocked for codes 0 to 15 (table 0x5beb1c; blocked is −32). The temporary footprint
  terrain 16 (§3.5) reads past that table and gets 1.
- An object adds its class cost over its footprint square, which reaches up and to the left of its cell. The
  costs for classes 1 to 12 are 2, 2, 2, 3, −32, −32, −32, −32, 4, 6, −32, 4.
- A forest-layer object adds its cost on its own cell: 1, 4, 4, 4, 6, −32, −32, −32, −32, 8, 10, −32, 6 for
  classes 0 to 12.
- A stone bridge sets its footprint to the road cost and a wooden bridge to the grass-plain cost, each plus
  1000.

The reads are bounds-checked (0 off the map) but the writes are not (0x4da2ec): a square reaching past the left
edge writes 0 + its cost at the end of the row above. Two cells before the grid hold its own width and height,
so a square reaching cell (−1, 0), (−2, 0), (W − 1, −1) or (W − 2, −1) would change the grid's size for every
later read and write. No object's square ever leaves the map (the hills brush keeps it inside, and a load moves
it inside, `mapcheck-files.md`), so only a bridge code left on the border by an earlier map could do this.

The second pass subtracts 1000, raises negative values to 0 and sets every border cell to 0. A value of 0 is
**blocked**, and **100 or more counts as water**. So shallows and coastal water can be crossed at a high cost,
and deep sea cannot.

### 3.5 What placing a building does
**code** (0x595390, building mode). The placement is refused unless all of these hold: there are fewer than
254 buildings, the anchor cell has no building code, the anchor is inside the map, and the anchor is at least
(current brush size − 1) cells from the left and top edges. Nothing checks that the footprint fits.

If it is accepted:
- the anchor cell gets the building code;
- a zeroed record gets the type, the picture, the position and the footprint. The type is the picture type,
  except that pictures of type 8 with variants 2–4 make a type-15 record and variants 5–6 make a type-12
  record (0x595da0). The generator never places type 8. The reseed and the name draws below use the picture
  type;
- the faction is set to neighbour, with the map's neighbour attitudes;
- the extra garrison defence is set by type: town 20, village 2, castle 15, fort 10, ruins 5;
- the owner byte is **left at 0**, not at "none";
- every footprint cell gets the building mark. While the generating flag is on, footprint cells of picture
  types below 13 also get the temporary terrain 16 (0x596c10; the picture type, so a house made an obelisk
  counts). While it is off, their forest layer is cleared instead. The footprint loop is range-checked and
  starts at the top-left cell, so a footprint reaching past the left or top edge writes none of its cells
  and aborts with a range error, after the record and its names are made.

Then comes the reseed of §1, followed by the name draws. These depend on the picture type and variant:
- **Default case** (towns, castles, forts, taverns, markets, churches, bridges, …): R(size of the type's name
  list), then R(size of its owner-name list).
- **Villages:** variants 3 and 4 first draw R(2) to choose a name prefix; then come the same two draws as the
  default case.
- **Ruins:** a single draw. It is R(7) for variants 0, 4 and 5, R(2) for variant 1, and R(3) for variants 2,
  3, 6, 7 and 8. Other variants draw nothing. The draw picks from a fixed band of the ruin-name list (entries
  counted from 0): 3–9 for variants 0, 4 and 5, 10–11 for variant 1, 0–2 for variants 2, 6 and 7, 15–17 for
  variant 3 and 12–14 for variant 8.
- **Smithies and altars** make 0 or 1 draws by variant. The generator never places them.

**The name lists** (**code** 0x59e1b3–0x59e355, loader 0x4d45b8). At start-up the editor reads
`DTMapEdit_Rus.Ini` from its own folder. It keeps every line, trailing spaces trimmed and blank lines
included. A section runs from the line after its `[...]` header to the line before the next header. The
`[Names]` section fills the name lists and `[Heros]` the owner-name lists, both in the same way:

- lines are read in order, and reading **stops at the first empty line** or at the end of the section;
- a line starting with `#` opens group g, where g is the number in the next two characters (spaces trimmed),
  1 to 15. The rest of that line is a comment and is not an entry;
- every other line is one entry of the group opened last. Duplicates are kept, and the list is not sorted.

Group g serves picture type g, so the size of group g is the n of that type's R(n) draws. **data**: the
shipped file gives these sizes (name list / owner-name list): town (1) 15 / 8, village (2) 46 / 23, castle (3)
60 / 77, fort (4) 7 / 7, tavern (5) 24 / 1, market (6) 6 / 1, church (7) 11 / 8, ruins (12) 20 / 1, stone
bridge (13) 1 / 1, wooden bridge (14) 1 / 1. The install's `Rus_MapEdit.ini` holds a near-copy of both sections (it differs
only in group 9), but the editor does not read it.

### 3.6 Roads: which pairs are linked
**code**. A road is built from a start centre to a goal centre (§3.7). First the path maps and the planner
are set up and the cost grid is built (0x571713–0x571747).

1. **Town roads.** The outer loop runs over columns 0..n−1 and the inner loop over rows 0..n−1. For each
   town in the sector table:
   - the next town further down the same column, skipping empty sectors, gets a road from its centre to this
     town's centre;
   - then the next town to the right in the same row gets one the same way.

   A table entry is the building count right after the search, so a search whose placement was refused
   (§3.3) names the building placed before it. When none was placed yet the entry is 0, and reading record
   0 − 1 aborts the step with a range error. With the default brush size this needs a refused anchor cell;
   a brush wider than a town's distance from the top or left edge refuses every placement.
2. **Nearest-neighbour roads.** The building count is taken once at this point; bridges built in item 1 are
   already part of it. For each building i of that count that is not a town (type > 1, bridges included):
   - take the building j ≠ i with the smallest distance from anchor to anchor, scanning all current buildings
     and keeping the first of equals; build a road from j's centre to i's centre;
   - take the next nearest, excluding the one just used, and build a second road.

   The distance is the larger axis difference plus half the smaller, rounded down (0x4db648). Towns can be
   chosen as j.

### 3.7 Building one road
**code** (0x56fa60). There are up to 6 attempts. Each attempt works as follows:

1. Clear the seeds and set every cost multiplier to 1. Put one seed of value 0 at the goal and flood from the
   start, stopping early when the start is reached. The flood is the game's planner flood (`ai.md` §7.5: steps
   cost 2 × the cell cost orthogonally and 3 × diagonally, frontier batches, early stop).
2. Read the path back from the start. Each step goes to the neighbour with the strictly lowest flood value
   (directions 0..7, the first of equals wins). Entry 0 is the start cell.
3. Scan entries 1 onward for the first cell costing 100 or more (water), and then for the first cell below 100
   after it (land).
   - **No water:** every path cell except the start whose terrain code is above 1 becomes road (4) and loses
     its forest object. This includes the goal and footprint cells with the temporary terrain 16. Done.
   - **Water:** go to §3.8. If it gives up, the road is abandoned and nothing is paved.
4. Rebuild the cost grid after every attempt. Each attempt that meets water places one bridge and tries again.
   On the 6th attempt the bridge is still placed, but the loop then ends without paving. So a road can be
   paved across at most 5 bridges, and a road that needs more leaves up to 6 bridges and no road.

### 3.8 Bridges
**code** (0x56fa60).
- Take the midpoint of the first water cell and the first land cell after it (each coordinate halved and
  rounded down). If the crossing is not mainly horizontal, meaning |dy| ≥ |dx| between those two cells, **the
  road is abandoned**: there is no north–south bridge.
- Otherwise there are three candidate axes: d = 1 (NE–SW), d = 2 (E–W) and d = 3 (SE–NW). For each, walk from
  the midpoint in both directions, starting one cell out, until a cell costs less than 100. Blocked cells and
  cells off the map also count. The east end of each axis is called A and the west end B.
- The axis with the smallest x-extent from B to A wins. Ties are broken this way: if axis 1 is shorter than
  axis 2, choose 1 when it is also shorter than 3, else 3; otherwise choose 2 when it is shorter than 3, else 3.
- The pieces come from the stone-bridge picture group (base = its first picture):
  - west end: base + 7 − d, placed at B;
  - east end: base + 6 + d, placed at A;
  - middle: base + 4 − d, on each cell from B toward A, excluding both ends.

  Each piece is a building placement, so each one reseeds the generator and makes its two name draws. The
  ends stand on the first non-water cells. The bridge counter goes up by 1 for each bridge, not for each
  piece, and the road is tried again.

### 3.9 Buildings on road junctions
**code**. First the generating flag is switched **off** (0x5720e8). Junction buildings and ruins are therefore
placed the interactive way: their forest is cleared and their terrain is kept.

Every cell is visited, x from 0 to W−1 in the outer loop and y in the inner loop. A cell qualifies when it is
masked, is road, and has **exactly 3** road cells among its 8 neighbours. For each qualifying cell:
- t = R(4) + 4 picks the type: 4 fort, 5 tavern, 6 market, 7 church (0x57226b);
- that type's counter goes up **before** the roll, so these four counters count candidates, not buildings;
- if R(100) < the other-buildings chance: a picture is drawn with R(pictures of t) + first(t), **after** the
  roll this time, and the building is placed with this cell as its anchor (0x57234a), with no search and no
  fit test. Then the mask is cleared over x − sx − 1 … x + 2 by y − sy − 1 … y + 2, even if the placement was
  refused. A failed roll clears nothing.

The neighbour reads and the mask box are range-checked (0x572499), unlike the box after a spiral search. A
junction at x ≤ sx or y ≤ sy, where the box reaches column or row −1, aborts the step with a range error
(§1). So would a masked road cell in column 0 or row 0, whose neighbour read reaches index −1. Border masks are
never recomputed by §3.1, so this depends on the stale mask state (see Unknowns).

### 3.10 Finishing, and ruins
**code**.
1. Every cell with terrain 16 becomes grass plain (6). Footprint cells that roads crossed stay road. The
   planner is freed.
2. A new mask is set on every cell that is grass lowland (5) or marsh (8), or has a forest object, or has an
   object of class 1 to 4.
3. For every building (bridges and junction buildings included), the mask is cleared over the box of
   ±(2 × footprint size + 3) around its centre, on interior cells only.
4. Five erosion passes run as in §3.1, without the corner reset.
5. For k = 0..n² (again n² + 1 sectors): a picture R(ruin pictures) + first ruin picture, then a roll
   R(100) < the ruins chance (0x572e2e). On success the spiral search of §3.3 runs with radius 30, starting
   from the **sector centre** itself.
6. The marks are built again from the new map (0x572eaa), and the step redraws the map and shows the
   counters.

### 3.11 Draw order of the step, summed up
1. Road clean-up: R(8) repeats, cell by cell.
2. Randomize.
3. Per sector: R(12); then, for the town, the castle and the two villages, the picture draw and R(100), each
   followed on success by the placement's reseed and its name draws.
4. Roads draw nothing, except that each bridge piece reseeds and draws its names.
5. Junctions, cell by cell: R(4) and R(100), then on success the picture draw, the reseed and the names.
6. Ruins, per sector: the picture draw and R(100), then on success the reseed and the names.

The sector-loop draws read the pictures-per-type counts (0x5bf448) and assume that a type's pictures are
numbered consecutively from its first one. If the graphics loader ever numbered them out of order, the draw
would land on another type's picture (**unknown** whether that can happen).

## 4. Step 2: economy

**code**, 0x573224. The step does nothing when there are no buildings. Let spread(x) = x div 10 and
δ(s) = R(s) − R(s), so δ(s) ranges over −(s−1)…(s−1). The bases come from the income grid (§2): town gold Gt,
castle gold Gc, village gold Gv, village mana M.

Buildings are visited in table order. For each one:

1. **Towns, villages and castles only** (in this order of draws):
   - village: mana per day = M + 5·δ(spread M);
   - daily gold income = the type's base + 5·δ(spread of that base);
   - maximum gold, from a fresh r = δ(spread of the type's base):
     - town: 10 × (Gt + 5r);
     - castle: 5 × (Gc + 5r);
     - village: 2 × (**M** + 5r). This uses the mana base, not Gv, which looks like a slip.
   - village: maximum mana = 2 × (M + 5·δ(spread M)), stored in a byte.

   The step also counts these three types, but never uses the counts.
2. **Every building:** faction = R(3) + 2 (ally, neighbour or enemy, equally likely), with that faction's
   attitude row from the map header. Owner = none (0xFF).
3. **By type.** Only the fields listed are written; everything else keeps its old value.
   - **Town.** Barracks, as (unit, count at start, maximum): (4, 4, 9), (19, 3, 9), (24, 0, 3), (40, 0, 3),
     (26, 1, 5), (30, 1, 5); barracks on. Goods: count, minimum price and maximum price from the town trade
     row. Spells: from the town library row (§4.1).
   - **Castle.** Barracks (4, 4, 8) and (19, 2, 4); barracks on. Garrison slots 1 to 3: 1 × unit 8, 3 × unit 4,
     4 × unit 19 (levels are not touched).
   - **Fort.** Barracks (4, 4, 9); barracks on.
   - **Market.** Goods from the market trade row.
   - **Church.** Barracks (26, 1, 5) and (30, 1, 5); barracks on. Goods from the church trade row. Spells
     from the church library row.
   - The unit numbers are unit ids as in `Rus_Units.ini`.
4. **Every building whose goods count is not 0:** count = count + R(3) − 1, so it moves by −1, 0 or +1
   (0x5741e2). Then any count above 12 is set to 12.

The words written here are range-checked: a negative daily income or maximum gold (possible only with
non-default grid values) aborts the step (§1).

### 4.1 Spells for sale
**code** (0x573094). The step does nothing if the game has no spells. The count is capped at 6. For each slot
it makes up to 31 tries:
- draw spell = R(number of spells) + 1;
- accept it if its gold price lies within [minimum, maximum] and it is not already chosen;
- store accepted spells in order.

A slot that finds nothing in 31 tries is skipped, so the building ends up with fewer spells.

## 5. Step 3: armies and garrisons

**code**, 0x57466c (shared tail at 0x5762a7).

### 5.1 Who is considered
- **Unowned-only box off:** every existing army is deleted first. Their map references are cleared,
  the army count goes to 0 and the table is zeroed. Every building is considered.
- **On:** existing armies stay. A building is considered only if its owner byte is 0xFF. Only the economy step
  writes 0xFF; a freshly placed building has 0. So with this box ticked, buildings that never went through the
  economy step are skipped. A skipped building gets nothing at all: no draw, no garrison, and its owner byte is
  left as it was.

Buildings are visited in table order, over the count taken at the start. Rolls exist only for towns
(town-army chance), villages, castles, forts, taverns, markets and churches (all four use the other-buildings chance)
and ruins. **Other types make no draw** and go straight to the garrison path, as does any building that fails
its roll or meets a full army table (255 armies).

### 5.2 Budgets and the minimum point
For a (low, high) budget range, with no minimum point the budget is R(high − low) + low, which lies in
[low, high − 1].

With a minimum point P, the budget is low + d·(high − low) div W. Here d is the anchor distance of §3.6 from P
to the building's anchor, and integer division truncates. Since d can reach about 1.5 W, **budgets can go past
high** in the far corner. Note also that the division is by W, even along the y axis.

**Town armies use the castle's low value C1 as the base** when a minimum point is set, while still using the
town range T for the slope (0x574fc5). The bug is reproduced here deliberately.

### 5.3 Garrison path (no army)
- town: budget from Tg, theme Normal;
- castle: budget from Cg, theme Normal;
- ruins: budget from Rg, then R(2): 0 gives the Rogue theme, otherwise Undead;
- other types get no garrison.

The garrison is **merged** into whatever the building already holds (§6.4). Nothing is cleared first, so
running the step twice piles units up. In every case of this path, the building's owner becomes none (0xFF).

### 5.4 Army path
The army is placed at the building's centre in army mode. It gets the editor's default numbered name, an
experience correction of 100 and byte 8 set to 4 (meaning **unknown**), and its starting gold is the
building's maximum gold. Then, by type:

| building | name | style | patrols | radius | aggression | gold | budget | theme | leader name |
|---|---|---|---|---|---|---|---|---|---|
| town | budget prefix + cut owner name | 0 feudal | no | 0 | −25 | town max gold | T | Normal | owner name |
| castle | same | 0 | no | 0 | −25 | castle max gold | C | Normal | owner name |
| village | the peasants name | 2 peasant | yes | 15 | 25 | village max gold | V | Piesant | owner name |
| fort | the robbers name | 1 rogue | **no** | 50 | −10 | fort max gold (0 after the economy step) | O | Rogue | owner name |
| tavern | the traveller name | 1 | yes | 25 | −50 | 500 | none | none | empty |
| market | the assassins name | 1 | **no** | 50 | −10 | 500 | O | Assasin | empty |
| church | the holy-host name | 1 | yes | 25 | 10 | 500 | O | HolyArmy | empty |
| ruins | vampires (1 in 3) or undead name | 1 | yes | 25 | 25 | = budget | R | Vampires / Undead | empty |

- **Town and castle names.** Start from the building's owner name. Check three nobiliary particles in a fixed
  order (a "von" form and two "de" forms). Whenever one is found, everything before it is cut, so the name
  starts at the particle; each check works on the result of the one before. A prefix then goes in front by
  budget, one of three words for a small, a middle and a large force: the first for 1–1000, the second for
  1001–3000 and the third for 3001–9000. **Any other budget (0 or less, or above 9000) leaves the default
  numbered name** (**code**; the words are the game's own and are not given here).
- **Town and castle extras.**
  - Daily base income: let v = Σ over troop slots of wage(unit price) × count, minus 50, minus the building's
    daily income. If v > 0 it is stored as v div 10 in the army's byte at offset 0x50; a v of 2560 or more
    overflows that byte's range check and aborts the step (0x5752af). The leader is not counted. The wage is
    Round(price / CostRecrutDiv × f), with f = ¼ for prices up to 50, ½ up to 100, ¾ up to 150 and 1 above.
    CostRecrutDiv is 2 in `_Global.ini`, and Round rounds half to even (0x581504).
  - With the enemies-only box ticked, the building becomes an enemy with the enemy attitude row before it is copied
    to the army.
  - Garrison slots 4 to 6 of the building are emptied.
- **Tavern.** The leader is unit 74 or 75, chosen by R(2) (0x576015). There are no troops and no budget draw.
- **Ruins.** R(3) is drawn after the budget (0x5761d4). The army's gold, the budget, is written after its
  troops.
- **Gold.** The army's gold is a signed 16-bit field and both writes are range-checked: a building's maximum
  gold of 32768 or more aborts the step right after the placement, before any budget draw (0x574c3d), and so
  does a ruin budget outside −32768…32767 once the troops are in (0x57628f). Both need values past the
  defaults (grid cells, spin boxes).
- **Shared tail, for every army.**
  - model = style + 4 (4 feudal, 5 bandits, 6 peasants);
  - home = the building;
  - respawn = R(5) + 2 days, so 2 to 6 (0x576346);
  - faction and attitudes copied from the building;
  - the army's map reference is written, and the building's owner becomes the army.

### 5.5 Draw order, per building
1. The chance roll, if the type has one.
2. **Garrison path:** the budget draw (none with a minimum point), the ruin theme draw if any, then the army
   builder's draws (§6).
3. **Army path:** the budget draw (none with a minimum point, none for a tavern), the vampire draw for ruins,
   the army builder's draws (none for a tavern, which draws its leader instead), then the respawn draw.

## 6. The random army builder

**code** (builder 0x57fe6c, unit picker 0x57fc48, theme loader 0x582ac0).

### 6.1 Themes
**data**: `_Global.ini`, section `[AIArmyGeneration]`. It is read at start-up together with the other global
options (0x59cbd4). Each key is a theme, and its value is a list of unit ids. The shipped lists are Normal (44
ids), HolyArmy (12), Piesant (3), Rogue (19), Assasin (4), Undead (19), Hero (23) and Vampires (5).

**code**: the theme numbers are fixed: 1 Normal, 2 HolyArmy, 3 Piesant, 4 Rogue, 5 Assasin, 6 Undead, 7 Hero,
8 Vampires. Each list is read until the first zero. There is room for 100 ids per theme, and nothing checks
that limit.

### 6.2 Leader and number of units, for a budget B
1. Leader: pick a unit from slot 0 with the window [B/5, B/4], both truncated. Theme Normal takes its leader
   from **Hero**; every other theme uses its own list. Slot 0 doubles its window (§6.3), so the leader really
   costs 40–50 % of B.
2. rest = B − the leader's cost.
3. If rest < 1: rest = 50 and n = 1. Otherwise n = |R(8) − R(3)| + 2, which ranges over 2–9.
4. share = rest × 3 div (4n). The window is [share/2, share × 3/2], and its low end drops to 0 when it is under
   40.
5. Units 1 to n are picked with that window from the army's own theme. The army then holds n + 1 units: full
   HP, paid, level 0.

### 6.3 Picking one unit (slot s, theme, window [lo, hi])
- Let m = (s + 1) mod 4. When m is 1 or 3, both lo and hi are doubled. That covers slot 0 and every even slot:
  2, 4, 6, 8.
- Repeat:
  1. Draw r = R(theme list size) and take that unit at level 0.
  2. Compute its cost (§6.5).
  3. In window = lo ≤ cost ≤ hi.
  4. **Role filter.** It applies only when the unit is in the window, s > 0 and the unit has a role. The roles
     are melee (melee attack above both ranged attack and magic), shooter (ranged above both others) and caster
     (magic above both melee/2.5 and ranged/2.5; this test wins). A unit with tied stats has no role and is
     accepted without a draw. The filter draws once:

| m | slots | melee kept | shooter kept | caster kept |
|---|---|---|---|---|
| 0, 2 | 1, 3, 5, 7, 9 | 3 in 4 | 1 in 4 | 1 in 4 |
| 1 | 4, 8 | 1 in 8 | 7 in 8 | 1 in 8 |
| 3 | 2, 6 | 1 in 4 | 1 in 4 | 3 in 4 |

  5. Every try counts. After the 26th try without success since the last widening, the window widens:
     lo = lo × 80 div 100 and hi = hi × 120 div 100, and the try count goes back to 0.
  6. Stop at the first unit accepted.

  The slots therefore lean melee, caster, melee, shooter, melee, caster, melee, shooter, melee, and the even
  slots get double the budget.
- Widening cannot raise a high end of 4 or less, because hi × 120 div 100 = hi for hi ≤ 4 (the low end
  still falls toward 0). A window that starts with hi ≤ 4, after any doubling, therefore never grows, and the
  loop never ends unless some unit of the theme costs that little. For the leader this happens when B ≤ 11.
  For the troops it happens when share ≤ 3 (share × 3/2 ≤ 4) on an odd slot, or share ≤ 1 on an even slot,
  which a large leader cost relative to B can cause (**code**; untested).

### 6.4 Into the map records
**code** (0x5742bc, 0x5744a8).
- **Army.** The leader unit id goes into the leader byte. Each further unit is merged into the six troop
  triples: it goes to the first slot that holds the same unit or is empty, adding 1 to the count. A unit that
  finds no slot (six other kinds already there) is dropped. Levels stay 0.
- **Garrison.** All n + 1 units, the leader included, are merged into the building's garrison triples in the
  same way, without clearing the garrison first.

### 6.5 Unit cost
**code** (0x57faf8, 0x57f4a8). The cost is the unit's strength × its CostMultipler div 100. It is computed from
level-0 stats with no items and no building defence. The strength is the editor's copy of the game's formula
(`experience.md` §1, game 0x49fc50). It is the vanilla version, without the Community hook that turns 0 into 1.
It divides by 3 when the ranged attack reaches ShotWeaponRange (60 in `_Global.ini`) and the unit is not
Artillery (bonus 14), and a result of 0 becomes 1.

**It is not a bit-exact copy of the game's** (**code**, an instruction-by-instruction comparison of 0x57f4a8
with game 0x49fc50). The two differ in two ways:
- every non-integer constant of the formula (1.17, 30.3, 1.07, 1.15, 1.7, 1.4, 0.8, 0.2, 1.2, 0.15, 3.2 and
  1.1; table 0x57fa40–0x57faec) is stored at full 80-bit precision in the editor. The game holds the same
  values rounded to 64-bit precision (game 0x4a01e8–0x4a0294). The results can therefore differ in the last
  bits, and so can a rounding that lands exactly on a half;
- the game's copy has two Community patches inside the formula, at its two bonus tests (game 0x4a0025 and
  0x4a0069). The editor keeps the vanilla tests there.

For parity, use the editor's version: vanilla bonus handling and 80-bit constants.

## 7. Quirks to reproduce

All **code** unless noted.

1. The "spiral" grows squares anchored at the start toward +x/+y only. It tests the start 4th, and again
   at the end of every square (§3.3).
2. The sector count comes from W alone. There are n² + 1 sectors; maps narrower than 50 divide by zero. On
   square maps the extra visit only makes draws; at n = 16 a town placed by it would hit a range error (§3.2).
3. Every placement, bridge pieces included, reseeds the generator from its position and picture (§1).
4. Sector and ruin pictures are drawn before the chance roll; junction pictures after it. The junction counters
   count candidates (§3.9).
5. Junction buildings ignore the footprint test. Junction buildings and ruins are placed with the generating
   flag off (§3.9).
6. A mostly vertical water crossing abandons the road. A road is paved across at most 5 bridges; one that
   meets water on its 6th attempt gets a 6th bridge and is never paved (§3.7, §3.8).
7. Shallows and coastal water are roadable at high cost; deep sea is not (§3.4).
8. Village maximum gold uses the mana base (§4).
9. Town army budgets with a minimum point start from C1 (§5.2), and distance scaling can exceed the high end.
10. Army names outside budgets 1–9000 keep the default numbered name (§5.4).
11. Fort and market armies get a patrol radius but no patrol flag. The fort's gold is the fort's maximum gold,
    normally 0 (§5.4).
12. Garrisons accumulate over runs. The economy step's castle garrison stays under the generated one (§5.3).
13. The unowned-only box relies on the owner byte 0xFF, which only the economy step writes (§5.1).
14. A building placed by the generator has its owner byte at 0 until the economy step runs (§3.5).
15. Old footprints stay unplaceable within the session, and border mask cells are not recomputed (§3.1).
16. The leader of a Normal army comes from the Hero list at 40–50 % of the budget (§6.2).
17. The economy step moves a goods count by −1, 0 or +1, never 0 to +2, and then caps it at 12 (§4).
18. A cost window whose high end is 4 or less never widens, so the builder can hang (§6.3).
19. Range checks abort a step half-way in a few edge cases: an old footprint past the top or left edge, a
    first town refused after its search, a junction near the top or left edge, an army income byte above 255,
    army gold above 32767, negative economy values (§1, §3.1, §3.6, §3.9, §4, §5.4).

## Razdor editor now → original

| Topic | Razdor now (`src/editor/worldgen/`) | Original | Status |
|---|---|---|---|
| The window | toolbar: World; three tabs, Run runs the tab shown (one undo step), made anew at every opening | TMakeWorld, §2 | matches (Razdor's layout) |
| Chances, grids, budgets, minimum point, the two boxes | §2: the six entries of every drop-down, the grids as text with the step's fallbacks for a cell that is not a number, the sixteen budgets from W, the keypad, the boxes | §2 | matches |
| Counters | the open map's counts on opening (bridges never counted), the step's after a run | §2 | matches |
| Random generator | the game's (`rules/rng.rs`), the editor's one stream; Randomize from the clock after the road clean-up | §1 | matches |
| Reseed on placement, name draws | the shared placement of the buildings brush (`brush::place_building_in`, `naming.rs`), with the generating flag (terrain 16, trees kept) | §1, §3.5 | matches |
| Clearing, mask, erosion | §3.1; the marks built again at the start and the end; the mask is the cells' scratch byte, which the new-map generator's flags fill and a load clears | §3.1 | matches |
| Sectors, quadrants, spiral | §3.2, §3.3, n² + 1 visits | §3.2, §3.3 | matches |
| Cost map | §3.4, unchecked writes past the left edge wrapping to the row above as the original's do, writes before the grid skipped; rebuilt after every attempt (only the changed cells are computed again, each as the whole build leaves it) | §3.4 | matches (a write into the grid's own size, which only a stale border bridge could make, is not followed) |
| Road flood and path | the game's planner (`TileMap::flood_maps`, `descend`) | the editor's copy of it | matches |
| Roads, bridges, junctions, ruins | §3.6–§3.10 | §3.6–§3.10 | matches |
| Economy | §4 and §4.1 | §4 | matches |
| Armies and garrisons | §5, the army placed with the items brush's placement; names our own words (the budget's three words, the fixed army names), the owner name cut at the Russian particles | §5 | matches (Razdor's own texts) |
| Army builder, roles, widening | §6 with the themes of `[AIArmyGeneration]` up to the first 0 | §6 | matches (a non-number in a list is skipped, where the original's reader may stop) |
| Strength | §6.5 in software 80-bit precision with the editor's constants and Delphi's `Exp`; `f2xm1` modelled correctly rounded | x87 | matches (an x86 processor's `f2xm1` is a last bit off for about 1 % of arguments; no unit's strength of the install changes) |
| Range errors (§3.1 footprint, §3.2 n = 16, §3.6 record 0, §3.9, §4, §5.4 income and gold) and the divide by zero (W < 50) | the step stops there and says why; what it wrote stays, but terrain 16 goes back to grass plain | the step aborts half-way, terrain 16 and the generating flag left | Razdor stops cleanly |
| Endless builder loop (§6.3) | the step stops when the window can no longer widen and holds no unit of the theme | loops for ever | Razdor stops cleanly |
| Undo | one step per run; afterwards the cell state is built again as a load builds it | no undo | Razdor's own |

## Unknowns

- Whether the pictures of a type are always numbered consecutively, which the picture draw assumes (§3.11).
- What the scratch mask holds after the editor's own new map without its generator (a load clears the whole
  cell grid, `mapcheck-files.md` §3.8; the generator leaves its flags there).
- What the user sees after the divide-by-zero on maps narrower than 50 cells.
- The meaning of army byte 8, which the generator sets to 4.
- Which units, if any, get a different strength in the editor than in the game because of the constant
  precision (§6.5).
- Whether the flood's special case at map cell (0, 0) (`ai.md` §7.5) can ever matter for roads. That cell is
  always blocked, as part of the border.
