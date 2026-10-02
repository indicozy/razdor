# Discord Times map editor: the new-map generator

Source: `DTMapEdit.exe`, the map editor of the Discord Times Community Update, read statically
only (it was not run). This file describes the random map generator behind the editor's
"create map" dialog (the TMakeMap form, code 0x51c578–0x5289c4, generator 0x51f2ac). The aim is
that a reimplementation builds the **same map** for the same seed, options and installed graphics,
so the order of random draws and every rounding step are spelt out, together with the original's
quirks. The rules are in my own words; addresses are given so they can be checked.

Confidence tags: **code** = read in the code (decompiler and disassembly agree), **data** = taken
from the form layout, the install's settings file or the game data, **unknown** = not settled.

Conventions:
- `a div b` is integer division toward zero; `a >> 1` is a halving that rounds down (the sizes are
  positive, so it equals `div 2`).
- `Round(v)` is the x87 conversion with the default control word: round to nearest, **ties to
  even**, with the whole computation in 80-bit extended precision. Where a value is a quotient or
  a product of floats, the formula gives the order of the operations.
- `R(n)` is one draw of the generator in §2 with range `n`.
- The map is `W × H` cells (always square here); a cell is `(x, y)`, x = column, y = row, row 0 at
  the top (north). "Row-major" means y ascending, then x ascending.
- Terrain codes and object classes are those of `docs/reference/dtm-format.md` (§4, §5).

---

## 1. The dialog and its options

**Opening** (0x51cbd8, 0x51cc28): code. The main window's create command opens the dialog. The
dialog keeps the current map size so it can restore it, allocates its own work grid (801 × 801
records) and reads twelve integers from the `[MakeMap]` section of the editor's settings file
(`DTMapEdit.Ini`). A missing key reads as 0 (0x4d4d58).

**Controls**: data (form layout), code (use).

| control | values | default |
|---|---|---|
| map size | 50, 100, 200, 400 or 800 (square), §1.2 | 50 |
| map type | land, lake, river valley, estuary, coast, skerries, island, archipelago (index 0–7) | land |
| orientation | north, south, west, east (index 0–3): the side the sea or river mouth faces | north |
| ratio bars | seven bars of coloured panels split by draggable splitters, §1.1 | from the settings file |
| blur | slider 0–5, step 1 | 2 |
| seed | a number field | 0 |
| "keep" check box (cbOld) | use the typed seed and allow reuse | off |
| "rebuild relief" check box (cbRun) | with "keep": still recompute the relief | off |
| create / exit buttons | | |

Two more check boxes exist but are hidden and always on (a full-map switch and a grey-relief
switch for a preview mode, §10.4).

### 1.1 Ratio bars and their default widths

Every share in the generator is a **panel width in pixels**. Each bar is 106 pixels wide inside,
and each splitter takes 3 pixels. Some panels are stretched to fill the rest, so their width is
derived; two right-hand panels have a fixed width of 25 that is never saved. data (form,
settings file); the arithmetic is the toolkit's alignment (code for the formulas below).

| bar | panels (left to right) and meaning | width rule | default widths |
|---|---|---|---|
| 1 | W0 water, L0 land | L0 = 103 − W0 | 27, 76 |
| 2 | W1 shallows, W2 coastal water, W3 deep sea | W2 = 100 − W1 − W3, W3 = 25 | 35, 40, 25 |
| 3 | SL coast band (marsh), L1 the rest | L1 = 103 − SL | 20, 83 |
| 4 | LL plain, LH hills, LM mountains, LR stony soil | LM = 97 − LL − LH − LR | 53, 32, 11, 1 |
| 5 | SL1 sand, Sl2 lowland, SL3 marsh (shares of the band) | Sl2 = 100 − SL1 − SL3, SL3 = 25 | 54, 21, 25 |
| 6 | Sw1 marsh kept, Sw2 impassable swamp (shares of the marsh) | Sw2 = 103 − Sw1 | 44, 59 |
| 7 | F0 no forest, F1 forest, F2 dense forest | F1 = 100 − F0 − F2 | 39, 39, 22 |

The settings file stores W0, W1, W2, SL, LL, LH, LR, SL1, SL2, Sw1, F0 and F2 (the install's
values: 27, 35, 40, 20, 53, 32, 1, 54, 21, 44, 39, 22). W2 and SL2 are stored but not used on
load, because those panels are stretched. All twelve are written back when the dialog is left
with the exit button (0x528438).

### 1.2 Sizes

The size list sets the global width and height to 50, 100, 200, 400 or 800 at once (0x51d0c4).
The generator always makes square maps. code.

### 1.3 The exit button

code (0x528438). While a generation runs, the exit button is the break button. It only raises a
break request; the generator checks it in its long loops and stops with an internal error that
is swallowed (§10.3). The button has no close result in the form, so **before the first
generation it does nothing at all**. After a generation it closes the dialog and applies the new
document defaults (§11).

---

## 2. Random numbers and the seed

**Generator** (0x4dc6b8): code. It is the same as the game's (see
`original-mechanics/engine.md` §3.1): state `S ← S × 214013 + 2531011` mod 2³² on every call, also
for `n = 0`; the result is 0 for `n = 0`, otherwise `((S >> 16) and 32767) mod n`. The editor has
its own state (0x5c82cc). Seeding from the clock (0x4dc6b0) sets the state to the low 32 bits of
the processor's time-stamp counter.

**Seed choice** (0x51f3dc): code.
- If the seed field is 0, or the "keep" box is off, the state is seeded from the clock, and that
  value is written into the seed field and into the map header (offset 0x14).
- Otherwise the state is set to the field's value.

So a typed seed is honoured only with "keep" on, and seed 0 can never be replayed.

**Second seeding** (0x5210f7): code. After the relief is finished (§3 to §6) the state is set
**again** to the seed field's value. Everything from the terrain classes on (§7 to §9) replays from
the seed. A reimplementation therefore needs the relief draws only to shape the heights; nothing
after §6 depends on how many draws the relief made.

---

## 3. The relief

The relief is computed only when "rebuild relief" is on or "keep" is off (0x51f4ad). Otherwise
the work grid of the previous run is reused (§10.1). code.

### 3.1 Start

The map's cells and the work grid are cleared (0x51f4db). Every cell's working value starts at 0.

### 3.2 Midpoint fractal (0x51d7d8): code

One procedure is called on the rectangle `(x1, y1)–(x2, y2)` = `(0, 0)–(W−1, H−1)`, with a
**mode** (1 for the relief, 2 for the forest field of §9). On each call:

1. The **level** depends on `d = x2 − x1`: 1 for d ≤ 2, 2 for 3–5, 3 for 6–13, 4 for 14–29, 5 for
   30–59, 6 for 60–119, 7 for 120–252, 8 for 253 and more.
2. The **amplitude** `A` comes from a table (data, 0x5bb214 and 0x5bb234):

   | level | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
   |---|---|---|---|---|---|---|---|---|
   | mode 1 | 0 | 200 | 400 | 400 | 600 | 600 | 600 | 600 |
   | mode 2 | 100 | 200 | 300 | 200 | 100 | 0 | 0 | 0 |

   A **jitter** is `R(A) − (A >> 1)` (0x51d758). A draw happens even when `A = 0`.
3. Four values are formed in this order, each with its own jitter drawn first:
   top = jitter + (v(x1,y1) + v(x2,y1)) div 2; bottom = jitter + (v(x1,y2) + v(x2,y2)) div 2;
   left = jitter + (v(x1,y1) + v(x1,y2)) div 2; right = jitter + (v(x2,y1) + v(x2,y2)) div 2.
   The centre is (top + bottom + left + right) div 4.
4. With `mx = (x1 + x2) div 2` and `my = (y1 + y2) div 2`, the values are written to (mx, y1),
   (x1, my), (mx, y2), (x2, my) and (mx, my), in that order, **each only if that cell still holds
   0**. A cell whose value happens to be exactly 0 can be overwritten later. The four corners of
   the map are never written and stay 0.
5. If both `x2 − x1 ≠ 1` and `y2 − y1 ≠ 1`, the procedure calls itself on the four quarters in this
   order: top-left (x1,y1)–(mx,my), bottom-right (mx,my)–(x2,y2), bottom-left (x1,my)–(mx,y2),
   top-right (mx,y1)–(x2,my).

The recursion depth starts at 1; calls at depth 6 also update the progress bar and check for a
break. That has no effect on the result.

### 3.3 Blur (0x51f633): code

With slider value `b`: if `b ≥ 1`, one pass with a 3 × 3 window; then `b − 1` passes with a 5 × 5
window. Each pass runs row-major **in place** (cells to the left and above already hold their new
value). For a cell, the sum is its own value **plus** the values of all in-map cells of the window
(the cell itself included again), and the count is 1 plus the number of in-map window cells; the
new value is sum div count.

Then the working value is copied to the height of every cell, and the height classes are
computed (§4, mode 1).

---

## 4. Height classes: rescale, histogram and cut (0x51df40)

code. The same procedure serves the relief (mode 1, 8 classes) and the forest field (mode 2,
3 classes).

1. **Rescale.** With `max` and `min` of all heights (the search starts at −30000 and 30000), every
   height becomes `Round((h − min) × 999 ÷ (max − min))`, so heights run 0…999. In the same pass
   the cell's working value, feature kind and feature weight are cleared, and a 1001-bin
   histogram is filled. `N` = W × H. (A flat field divides 0 by 0 and aborts the run, §10.3.)
2. **Weights** (integer products of panel widths, §1.1), lowest heights first:

   | class | mode 1 (relief) | weight | mode 2 (forest) | weight |
   |---|---|---|---|---|
   | 1 | deep sea | W0 × W3 | no forest | F0 × 100 |
   | 2 | coastal water | W0 × W2 | forest | F1 × 100 |
   | 3 | shallows | W0 × W1 | dense forest | F2 × 100 |
   | 4 | coast band | SL × L0 | | |
   | 5 | plain | (LL × L1 × L0) div 97 | | |
   | 6 | hills | (LH × L1 × L0) div 97 | | |
   | 7 | mountains | (LM × L1 × L0) div 97 | | |
   | 8 | stony soil | (LR × L1 × L0) div 97 | | |

   With the default widths the relief weights are 675, 1080, 945, 1520, 3446, 2080, 715, 65
   (sum 10526): about 6.4, 10.3, 9.0, 14.4, 32.7, 19.8, 6.8 and 0.6 % of the cells.
3. **Targets.** With `S` = the sum of the weights, `cum₀ = 0` and
   `cumᵢ = cumᵢ₋₁ + Round((N ÷ S) × wᵢ)`. Only the last one is capped: if `cum_k > N` it becomes N.
4. **Bounds.** `high₀ = −1`. Walk the histogram bins v = 0…1000 upward, skipping empty bins. Before a
   bin, `prev` is the number of cells counted so far; after adding the bin, `c` is the new count.
   For each class i = 1…k in turn, if `prev ≤ cumᵢ ≤ c`, then `lowᵢ = highᵢ₋₁ + 1`, `highᵢ = v`, and if
   `highᵢ < lowᵢ` then `highᵢ = lowᵢ`. At the end `high_k = 1000`. A class whose target equals the
   count of the previous bins matches twice and keeps the later bounds. A class that never matches
   keeps bounds 0–0.
5. A cell belongs to every class whose `[low, high]` holds its height. Where bounds overlap, the
   class tested last (highest index) wins (§7, §9).

---

## 5. Water by map type

code (0x51f586–0x5200d9). The map types are: 0 land, 1 lake, 2 river valley, 3 estuary, 4 coast,
5 skerries, 6 island, 7 archipelago.

### 5.1 Water stamps

For types 1 and 3–7, cells are marked as **sea features** with a radius `r`:

| type | stamps |
|---|---|
| 1 lake | the cell (W >> 1, H >> 1), r = W >> 1 |
| 3 estuary, 4 coast | a line along the oriented edge (north: row 0; south: row H−1; west: column 0; east: column W−1), r = W div 5 |
| 5 skerries | the same edge line, r = W >> 1 |
| 6 island | four edge lines, r = W div 5, and four corner points, r = W div 3, in this order: north edge (0,0)→(W−1,0), point (0,0), east edge (W−1,0)→(W−1,H−1), point (W−1,0), south edge (W−1,H−1)→(0,H−1), point (W−1,H−1), west edge (0,H−1)→(0,0), point (0,H−1) |
| 7 archipelago | (W div 50)² + 1 points, each `x = R(W)`, then `y = R(H)`, r = W |

Every stamp writes the kind and the radius of each cell it marks, **replacing** what an earlier
stamp wrote there (0x51d1a8, 0x51d640): a cell marked twice keeps the later radius. For the island
this means the north-west corner (0,0) ends with the edge radius W div 5 (the west edge, drawn
last, ends on it), while the other three corners keep W div 3. code.

Lines (0x51d1a8): code. With `ax = |x2 − x1|`, `ay = |y2 − y1|` and `sx`, `sy` the signs of the
differences, the first cell (x1, y1) is marked, then:
- if `ay < ax` (x-major): `e = ax − 2·ay`; repeat: `x += sx`; if `e ≥ 0` then `e −= 2·ay`, else
  `e += 2·ax − 2·ay` and `y += sy`; mark (x, y); until `x = x2`;
- otherwise (y-major, including `ax = ay` and a zero-length line): `e = ay − 2·ax`; repeat:
  `y += sy`; if `e ≥ 0` then `e −= 2·ax`, else `e += 2·ay − 2·ax` and `x += sx`; mark (x, y);
  until `y = y2`.

So both ends are marked; the decision tests `e` before it is updated, which fixes the exact cells
of a slanted river. A switch branch for a middle line of radius 2 under type 2 exists but cannot
run (§10.2).

### 5.2 Lowering the sea (0x51e88c, kind "sea")

1. The marked cells are listed row-major.
2. For every listed point `(px, py, r)` and every cell of the square `px ± 2r`, `py ± 2r` (clamped to
   the map): the distance is `d = max(|dx|, |dy|) + min(|dx|, |dy|) div 2`. With `D = 10000·d` (an
   exact integer), if `D < 20000·r` the weight is computed in this order (0x51ee69–0x51eeba):
   `f = 1 ÷ (D ÷ (10·r) + 10000)`, then `w = Round(((10000 − D ÷ (2·r)) × f) × 10000)`. In value this is
   `10000 × (1 − t) ÷ (1 + 0.2 t)` with `t = d ÷ 2r`. A cell keeps the **largest** weight over all
   points (a later point replaces it only if strictly larger).
3. Every cell with a weight above 0 gets `h ← Round((10000 − w) × h ÷ 10000)`.

Then the height classes are computed again (mode 1). The lower bound of class 2 (coastal water)
from this computation is kept as the river level `T`.

---

## 6. Rivers

code (0x52010c–0x5210d4). Rivers are lines of **river features**. All steps use
`dx = Round(cos(a·π ÷ 180) × len)` and `dy = Round(sin(a·π ÷ 180) × len)` with the heading `a` in
degrees (y grows southward). After a step `(nx, ny) = (x + dx, y + dy)`; the walk is *inside* if
`0 ≤ nx < W` and `0 ≤ ny < H`; the end is clamped to the map, the segment from `(x, y)` to the clamped
end is drawn, and the walk continues from there while it was inside.

### 6.1 River valley (type 2)

1. `a = R(360)`.
2. Three rivers. Each starts at the centre (W >> 1, H >> 1) with `base = R(80) + a + 60`, `a = base`,
   step length `L = 10`, pending turn `p = 0`. The heading carries over from the end of the previous
   river.
3. Each step:
   - `drift = (a − base) div 10`.
   - If `p = 0`: `v = R(17) − 8 − drift`, and v sets p and L:

     | v | 0 | ±1 | ±2 | ±3, ±4 | ±5, ±6 | ±7 | ±8 … ±36 | other |
     |---|---|---|---|---|---|---|---|---|
     | p (sign of v) | 0 | ±1 | ±2 | ±4 | ±6 | ±15 | ±10 | unchanged |
     | L | 20 | 10 | 5 | 4 | 4 | 3 | 3 | unchanged |
   - If `p > 0`: `a += 10`, `p −= 1`; if `p < 0`: `a −= 10`, `p += 1`.
   - `len = (R(3) − 1) × (R(L) div 3) + L` (the two draws in this order).
   - Step as above, radius 10.

### 6.2 Estuary (type 3)

1. A **stem**, radius 20, from the centre toward the land side by H div 7 (or W div 7): north
   orientation ends at (W >> 1, (H >> 1) + H div 7); south at (W >> 1, (H >> 1) − H div 7); west at
   ((W >> 1) + W div 7, H >> 1); east at ((W >> 1) − W div 7, H >> 1).
2. Six rivers i = 0…5. The start is on the centre line: for north, x = W >> 1 and y = (H >> 1) +
   H div 7 for i = 0, 1, 5 or H >> 1 for i = 2, 3, 4; then i = 1, 5 subtract `R(H div 7)` and i = 2, 4
   add `R(H div 14)`. South mirrors it (base y = (H >> 1) − H div 7; i = 1, 5 add, i = 2, 4 subtract).
   West and east do the same on x with W (west: base (W >> 1) + W div 7, i = 1, 5 subtract, i = 2, 4
   add; east: base (W >> 1) − W div 7, i = 1, 5 add, i = 2, 4 subtract). Start headings:

   | orientation | i = 0 | 1 | 2 | 3 | 4 | 5 |
   |---|---|---|---|---|---|---|
   | north | 90 | 225 | 240 | 270 | 300 | 315 |
   | south | −90 | 45 | 60 | 90 | 120 | 135 |
   | west | 0 | 135 | 150 | 180 | 210 | 225 |
   | east | 180 | 315 | 330 | 360 | 390 | 405 |
3. Each step first turns the heading: river 0 by `5 × R(13) − 30`, the others by `5 × R(7) − 15`;
   then a step of length 20. Rivers 0 and 3 have radius 20, the others 10.

### 6.3 Skerries (type 5)

W div 3 short segments: a random point in the quarter band at the oriented edge (north: `x = R(W)`,
`y = R(H >> 2)`; south: `x = R(W)`, `y = H − 1 − R(H >> 2)`; west: `y = R(H)`, `x = R(W >> 2)`;
east: `y = R(H)`, `x = W − 1 − R(W >> 2)`), then `R(2)`: 0 moves the start 5 cells west, else 5 cells
north; a segment of radius 20 is drawn. These segments are **never carved** (below), and the seed is
reset right after, so they change nothing.

### 6.4 Carving (kind "river")

Only for types 2 and 3: weights as in §5.2, then every cell with a weight above 0 and `h > T` gets
`h ← T + Round((10000 − w) × (h − T) ÷ 10000)`. A river's centre line thus reaches the coastal-water
level.

Then (for every type, and also when the relief was reused) the height classes are computed again
(mode 1) and the generator is re-seeded (§2).

---

## 7. Terrain classes

code (0x52112d). Every cell is first reset: terrain 6 (plain) and ground 6, no hill or mountain
object, the scratch flag cleared (the tree object slot is **not** cleared). Then classes 1…8 are
tested in order (§4.5):

| class | terrain | ground |
|---|---|---|
| 1 | 2 deep sea | 2 |
| 2 | 1 coastal water | 1 |
| 3 | 0 shallows | 0 |
| 4 | 8 marsh (the coast band, split in §8) | 8 |
| 5 | 6 plain | 6 |
| 6 | "hill" marker | 6 |
| 7 | "mountain" marker | 12 stony soil |
| 8 | 12 stony soil | 12 |

"Ground" is the terrain under objects; the markers are replaced by objects in §9.

---

## 8. The coast band split

code (0x5211..0x525482). The band (class 4) is divided into sand (10), grass lowland (5), marsh (8)
and impassable swamp (9). All steps use:
- a **list** of cells of one terrain, built row-major unless stated;
- the **12 neighbours** in this order: (+1,0), (0,+1), (−1,0), (0,−1), (+1,+2), (+1,−2), (+2,+1),
  (−2,+1), (−1,+2), (−1,−2), (+2,−1), (−2,−1) (four orthogonal steps, then eight knight moves);
  data (0x5bb1fc, 0x5bb208);
- a neighbour counts only if it lies **strictly inside** the map (`0 < x < W−1`, `0 < y < H−1`);
- a **sweep**: visit the list in order; for a cell whose flag is clear, try the neighbours in order
  until the cell is flagged. Unless stated, flagged cells change terrain only after the sweep.

Let N be the band size. Targets: sand `Sₜ = Round(SL1 × N ÷ 100)`, lowland `Lₜ = Round(Sl2 × N ÷ 100)`,
and `M = N − Lₜ − Sₜ`.

1. **Lowland from the water.** Sweep the band list: a neighbour that is water (0–2) or lowland (5)
   flags the cell and turns it into lowland **at once** (so the growth runs on within the sweep).
   Repeat until a sweep changes nothing. With `G` cells converted, set `M ← G − Lₜ − Sₜ`.
2. **More lowland**, if M < 0: list the remaining band cells; sweep with "a neighbour is lowland or
   plain, and M < 0" (each hit: M += 1), then turn every flagged cell of the map into lowland;
   repeat until M = 0. If nothing can grow, this never ends (only the break button stops it).
3. **Sand.** List the lowland cells; s = 0; two sweeps, "a neighbour is shallows or sand" (s += 1),
   flagged cells become sand after each sweep. Sand grows only from shallows (code 0), not from
   deeper water.
4. **Erosion.** List the sand cells; three sweeps, "a neighbour is lowland" (s −= 1), flagged cells
   become lowland after each sweep. This always runs.
5. **Sand to target.**
   - If s < Sₜ: list the lowland cells **bottom row first** (y from H−1 down to 0, x ascending);
     repeat sweeps "a neighbour is sand and s < Sₜ" (s += 1), flagged cells become sand, until s = Sₜ
     or a sweep grows nothing.
   - Otherwise: list the sand cells; repeat: a sweep "a neighbour is lowland and s > Sₜ" (s −= 1),
     flagged cells become lowland; if that sweep did nothing and s > Sₜ, draw `i = R(n)` until list
     cell i is not lowland, turn it into lowland (s −= 1). Stop when s = Sₜ or nothing changed.
6. **Marsh**, if M > 0: list the lowland cells; repeat until M = 0: draw `i = R(n)` until list cell i
   is not marsh, turn it into marsh (M −= 1, flagged); then repeat sweeps "a neighbour is marsh and
   M > 0" (M −= 1), flagged cells become marsh, while a sweep grows. If all lowland is already marsh
   and M > 0, this never ends.
7. **Impassable swamp.** List the marsh cells (n of them); target `Q = Round(Sw2 × n ÷ 100)`. Two
   sweeps flag the marsh cells next to water (0–2); c counts them (the second sweep cannot add any).
   - If c < Q: the flagged cells become swamp; then, `Q − c` times, draw `i = R(n)` until list cell i
     is unflagged and turn it into swamp (flagged).
   - Otherwise: every list cell's flag is inverted (now the water-side cells are the clear ones);
     then draw `i = R(n)`, and if cell i is clear, turn it into swamp, flag it and lower Q; repeat
     while Q > 0. At least **one** draw is made even when Q = 0, and if that draw hits a clear cell
     it still becomes swamp (Q goes to −1).
8. Cells that are lowland, swamp or sand copy that code to their ground. The flags are **not**
   cleared afterwards (a mistaken index clears one stale list cell instead): the swamp flags, and in
   the second case of step 7 the flags of the marsh away from water, stay set into §9. (The editor
   also fills its passability grid from the ground codes here.)

---

## 9. Objects

### 9.1 Variant counts: data

The editor counts, while loading the object graphics (0x59a718): for hills (class 1) and
mountains (class 5), the loaded sprites per size group `k` = sprite id div 10; for classes 9–11, the
loaded sprites per family `f` = sprite id div 12 (ids below 120). The generator assumes a group's
variants start at its first id. The counts depend on the installed graphics.

### 9.2 Mountains (0x525524): code

For block sizes k = 5, 4, 3, 2, 1; for every top-left corner, rows y = 0…H−k and, inside, x = 0…W−k:
if every cell of the k × k block is a mountain marker with a clear flag, all its cells are flagged and
the block's **bottom-right** cell gets the mountain object `class 5, sprite 10k + R(count₅[k])`. Every
mountain cell ends up in some block, the 1 × 1 blocks taking the rest.

### 9.3 Hills (0x525aa5, 0x525efc): code

List the hill-marker cells (n).
1. Sizes k = 6 and 5: `W div (7 − k)` tries (W, then W div 2). Each try draws `i = R(n)`; list cell
   (px, py) is used only if `px ≥ k` and `py ≥ k` (so blocks touching row 0 or column 0 are never
   tried). The block of size k ending at (px, py) must have all k² cells hill markers and all k²
   flags clear; then the cells are flagged and (px, py) gets `class 1, sprite 10k + R(count₁[k])`.
2. Sizes k = 4, 3, 2, 1: scan every top-left corner as for mountains, counting hill markers `h` and
   clear flags `c` in the block. k = 1 needs h = c = 1. For k = 2…4 the block is taken unless
   `k ÷ 2 + c < k²` or `k ÷ 2 + h < k²` (single-precision floats), i.e. it needs h and c of at least 3
   (k = 2), 8 (k = 3) or 14 (k = 4). A taken block is flagged and its bottom-right cell gets
   `class 1, sprite 10k + R(count₁[k])`, even if that cell is not a hill marker.
3. Unflagged hill markers become plain (none remain after k = 1); plain cells that were flagged
   inside a taken block become hill-covered.

### 9.4 Forest (0x5265b1–0x5278d7): code

1. The working values are cleared, the fractal of §3.2 runs in **mode 2** from the current generator
   state, the result is copied over the heights (the relief is lost, §10.1), and the classes of §4
   are computed in mode 2.
2. Cells whose ground is lowland, plain, dry plain, marsh or sand (5–8, 10) get forest class 1–3
   from their value (others get none).
3. The **family list** holds every family 0–9 that has at least one loaded variant in class 9, 10
   or 11, in ascending order.
4. The **pool** lists the cells of forest class 2 or 3 (row-major, flags cleared). With `n₀` its
   size now, a progress step `q = n₀ div ((W div 50)² × 30)` is computed (0x526b68; the divisor was
   set at the start of the relief, §10.1). While the pool is not empty, one cluster:
   - `i = R(n)`; cell i is flagged. Budget = W.
   - One sweep over the pool in pool order: every **flagged** pool cell (including cells flagged
     earlier in this same sweep) flags each of its 12 strictly-inside neighbours, in the order of
     §8, that is clear and in forest class 2 or 3, while the budget lasts (budget −1 each). The
     loop is meant to repeat but its exit test makes it a single sweep.
   - `f = family list[R(family count)]`; families 6 and 7 become 0.
   - For every flagged pool cell, in pool order:
     - object class: 11 (dense thicket) if forest class 3, else 9 (trees) for f < 8, else 10;
       variant family v = f;
     - ground marsh: `R(6)`: 0–2 → v = 9, 3–4 → v = 5, 5 → v = 4; ground lowland: `R(3)`: 0–1 → v = 4,
       2 → v = 5; ground sand: `R(3)`: 0–1 → v = 5, 2 → v = 4; other ground: f = 4 or 5 → v = 0;
     - v = 4 forces class 9;
     - sprite = 12v + `R(count[class][v])`;
     - if that sprite is loaded: it becomes the cell's tree object; then `R(5)`, and on 0 the
       sprite + 120 replaces it if loaded; the terrain becomes the tree marker (forest class 2) or
       the thicket marker (class 3) and the cell leaves the pool. If the sprite is not loaded, nothing
       is drawn further and the cell **stays in the pool**.
   - The progress bar is advanced by an integer division by `q` (0x5278bb), then the pool is
     rebuilt (flags cleared) and a break is checked.
5. **Small pool abort** (code): if `1 ≤ n₀ < (W div 50)² × 30` (fewer than 30, 120, 480, 1920 or
   7680 pool cells for sizes 50…800), `q = 0` and the division after the **first** cluster fails.
   The error is swallowed (§10.3): only that first cluster's trees are placed, the run stops there
   and the header defaults of §11 are not written. With `n₀ = 0` no cluster runs and nothing fails.
   A reimplementation that wants the same map must stop at the same point.
6. A cell for which no choice can ever give a loaded sprite never leaves the pool, so the loop
   then runs until the break button is used.

Trees can thus stand on hill-covered plain cells (their ground is 6).

---

## 10. Quirks and special paths

### 10.1 Reusing the relief ("keep" on, "rebuild relief" off): code

The relief steps are skipped and the heights left in the work grid are used. Those are the
**forest field** of the previous run (§9.4 step 1 overwrote the relief), so the reused terrain is
cut from the last forest noise. Only the dialog's own grid is reused (it lives as long as the
dialog). The progress divisor `(W div 50)² × 30` of §9.4 step 4 is set only in the relief path, so
in this path it holds whatever was on the stack; if it is 0, or makes `q` 0, the forest step stops
silently (§10.3; the value at run time is unknown).

### 10.2 Dead or inert code: code

- The type switch for water stamps has a branch for type 2 that the guard never admits.
- Skerries river segments are drawn but not carved.
- The z interpolation of the line stamp is wrong in one branch but its value is never used.
- A count of tree-covered hills at the end is never used.

### 10.3 Breaks and errors: code

The generator body sits in a block that swallows **every** error silently, including the break
request, division by zero and range errors (0x5282a0). After a break or an error the map stays
half made and the header step of §11 is skipped. The buttons and cursors are always restored. A
division by zero is reached in normal use by a small forest pool (§9.4 step 5); a flat field (§4
step 1) gives an invalid 0 ÷ 0, which the default x87 control word does not mask.

### 10.4 Preview mode: code

With the hidden full-map box off, the terrain steps are skipped and only a preview is drawn
(blue below and green above the middle height, or grey by height). The box is always on.

### 10.5 Others

The preview picture can be saved as a bitmap by double-clicking it (0x5288c8); a handler that
zeroes the seed is attached to no control (0x528860). code.

---

## 11. Defaults written for the new map

**At the end of a successful run** (0x5281e2–0x52827e): code. The 303-byte header is zeroed, then
gets the format signature (dtm-format.md §3), the width at 0x0C, the height at 0x10, the seed at
0x14 and 0 at 0x124; the file name becomes a fixed placeholder. Everything else in the header
(start time, hero presets, victory and defeat events, scenario kind) is therefore 0.

**On exit after a generation** (0x528438): code. A 5000-entry table of allocated blocks is freed
and five record counters are zeroed (most likely the building, army, point and event lists:
unknown which is which), the three scenario texts are cleared, the title becomes a fixed placeholder and the relation matrix (rows and columns player,
ally, neighbour, enemy) is set to:

| | player | ally | neighbour | enemy |
|---|---|---|---|---|
| player | 3 | 2 | 1 | −2 |
| ally | 2 | 3 | 1 | −2 |
| neighbour | 1 | 1 | 3 | 1 |
| enemy | −2 | −2 | 1 | 3 |

If the dialog is left without a generation, the old map size is restored.

---

## 12. Razdor editor now → original

| Topic | Razdor now (`src/editor/`) | Original | Status |
|---|---|---|---|
| New map | an empty map of one terrain code (`defaults.rs`, `NewMap`) | the random generator of this file | missing |
| Sizes | 50, 100, 200 offered; up to 800 accepted (`validate.rs`) | 50, 100, 200, 400, 800 | partly |
| Map types, orientation, ratios, blur, seed | none | §1 | missing |
| Relation matrix | same values (`DEFAULT_RELATIONS`) | §11 | matches |
| Header defaults | a start date, hero presets with gold, a translated title | header zeroed except signature, size and seed; placeholder title and file name | differs |
| Seed in the header | not written | the generator seed at 0x14 | missing |
| Settings file `[MakeMap]` | not read | twelve panel widths | missing |

---

## 13. Unknowns

- What the main window does with the dialog's result and its refresh call (0x5ab588), and whether
  closing the dialog by its title bar after a generation keeps the half-applied document (the map
  cells are written in place, but the defaults of §11 are not applied).
- The stale progress-bar divisor in the reuse path (§10.1): its value at run time.
- The variant counts for the shipped graphics (they follow from the object graphics index).
- Rounding: the products and quotients are evaluated in 80-bit precision; a 64-bit reimplementation
  can differ only when a value falls within rounding error of a .5 boundary. Not measured.
