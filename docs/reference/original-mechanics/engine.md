# Discord Times: the engine (clock, frame loop, random numbers, input, text, sound, animation)

Source: the user's `DiscordTimes.exe` (Community Update, Unstable), read statically only (the
game was not run). It covers the low-level units: the engine's window and DirectDraw code
(0x4741b4–0x47e944), the timer unit (0x4718ec–0x471b44), the sound unit
(0x48034c–0x481ba0), the game's random generator (0x4832fc), and the game-side code that sets
animation and music timing. The rules are in my own words, and addresses are given so they can be
checked.

Confidence tags: **code** = read in the code (decompiler and disassembly agree), **data** =
consistent with the data files or the maps but not traced to the end, **unknown** = not settled.

Conventions used below:
- *Now* is the game's millisecond clock (§2). *Raw now* is the system millisecond counter.
- `a div b` is integer division toward zero, `a mod b` its remainder (sign of `a`).
- Game time is in 1/100 minute (see economy.md); it is not the clock of this file.

---

## 1. Display and presentation

**Mode** (0x47609c, 0x475ad0, entry 0x4e7424): code.
- The game always opens its own borderless window and switches the screen to **1024×768 in
  16-bit colour** with the default refresh rate, in exclusive fullscreen.
- A windowed path exists (only when a host window is passed in), but the game never uses it.
- Colour channels follow the masks the display mode reports (normally 5-6-5).
- One primary surface plus **one back buffer** form a flip chain. Every flip waits for the
  previous flip and for vertical blank (flip flag "wait", 0x4ecd4c = 1, 0x474228). So the frame
  rate is the monitor refresh rate. There is no other cap and no fixed tick.

**What a frame does** (0x474648), in this order: code.
1. The mouse delta since the last drawn cursor position is computed. The "frames held" counter of
   the pressed mouse button goes up by one.
2. Every rectangle that changed in the previous frame is copied from the visible surface back
   into the back buffer, so the back buffer again shows the current picture there.
3. The area under the software cursor is restored.
4. The current screen's per-frame handler runs. The world map draws itself straight into the back
   buffer through Direct3D here.
5. If the screen takes input: the keyboard hooks run, hover and click dispatch run (§4).
6. Every rectangle marked dirty this frame is copied from the full-screen 2D canvas (where
   windows, text and widgets are drawn) into the back buffer.
7. The screen's after-draw handler runs.
8. The cursor is drawn: the old and new cursor rectangles are marked, the area under the new one
   is saved, and the cursor is drawn colour-keyed or with its alpha mask.
9. The typed character is cleared, the frame flips, and the wheel sum is reset.

Outside the loop (loading, saving) the same steps run from a "present now" routine (0x474b08). It
is called by the loading bar and the save bar.

**Message pump** (0x476298): code. Each pass takes one Windows message if there is one and
dispatches it. Only when no message is waiting does it run one frame. While the window is
inactive it sleeps 200 ms after each message. An idle pass draws a frame only while the "running"
flag is set, and sleeps 200 ms otherwise. Losing focus does not clear that flag: the first paint
message that arrives while inactive does. So a few frames may still be drawn after focus loss
(with the clock already paused), and frames resume at the paint that follows refocusing.

**Focus loss** (0x475748): code. Losing focus saves the cursor, **pauses the clock** (§2) and
marks the program inactive. Regaining focus only marks a restore as pending; the next paint
restores every lost surface (screen, back buffer, cursor save areas, then the game's own
textures through a hook), redraws the whole screen, resumes drawing and **resumes the clock**.

**Floating point**: unknown. The DirectDraw cooperative level asks for "FPU setup" and not for
"FPU preserve". Under DirectX 7 the Direct3D device creation (0x481cd0, at start) then leaves the
x87 unit in single precision for the rest of the run. The game never sets the FPU control word
itself. If that holds, every float formula in the game is evaluated with 24-bit mantissas. The
last bit of a rounded result can then differ from a double-precision reimplementation (§3.3).
In the running game (the diff test, Wine 11) the products are rounded with 64-bit mantissas: a
relation price of exactly x.5 in decimal goes the way the 80-bit constant's error leads it
(economy.md §2, FINDINGS.md §24), which single precision would not give. The game's own control
word, Delphi's 0x1332, is also loaded again by the RTL's FPU init (0x403984). Whether a Windows
run with DirectX 7 differs was not checked.

## 2. Clock and timing

**Timer resolution** (unit init 0x473e24): code. At start the program asks Windows for the
finest timer period it accepts, trying 1 ms, then 2 ms and so on. All clocks are built on the
multimedia millisecond counter. QueryPerformanceCounter and GetTickCount are used only by
library code.

**Three clock functions** (0x471994, 0x4719b0, 0x4719e4): code.
- *Raw now*: the system millisecond counter.
- *Now*: the counter minus a base. The base starts at 0, so Now begins as the system uptime;
  only differences matter. While the program is inactive Now is frozen. On reactivation the base
  moves forward by the inactive time, so **Now does not advance while the game is in the
  background**. Every game timer uses Now: music changes, fades, animations, the walk and wait
  pacing, and edit-box and list repeat.
- A microsecond variant is only Now × 1000. There is no finer resolution.
- Both Raw now and Now store their result in one shared "last time read" variable (0x4f1c34).
  The world renderer and the sprite code read that variable instead of calling a clock. Before
  the first focus loss the two clocks agree. After it, the variable holds whichever clock was read
  last. Only animation phases are affected (a visual glitch).

**Sleeps and waits**: code.
- Flip or lock busy: Sleep 10 ms and retry, at most 20 tries (0x474228, 0x476c94).
- Inactive window: Sleep 200 ms (above).
- Splash: 2000 ms when the install has logo pictures (0x4e5f8f).
- **Minimum loading time** (0x48c844): the loading bar is never drawn ahead of time. Each time it
  grows to `p` of its `W` pixels (p < W), the loader busy-waits until `(p × 4000) div W` ms have
  passed since the bar started (the clock is taken when the logo slideshow phase ends). Nothing
  waits once the bar is full, so loading takes nearly 4 s at least: the floor is set by the last
  bar length below full. A shimmer runs along the bar at 250 px/s, wrapping at the bar width.
- Save bar: shown for at least 1500 ms (0x4b648c).
- Several screens wait at least 500 ms after a map load (map 004b5b08). **data**.

**FPS counter** (0x4c7488, shown when the option `OptValue5` is 1, read at 0x4b89a4): frames ×
1000 div elapsed ms (0 when no time has passed), at most 999. The count restarts once more than
5 s have passed. The code sits in the original part of the exe, not in the Community section.
code.

## 3. Random numbers

### 3.1 The game's generator (0x4832fc): code

A single global 32-bit state `S` (at 0x659154). A call `Random(n)` does:
1. `S ← S × 214013 + 2531011` (mod 2³², the multiplier is 0x343FD and the increment 0x269EC3).
   This step **always** happens, even for n = 0.
2. If n = 0, the result is 0.
3. Otherwise `v = (S >> 16) and 32767` (15 bits), and the result is `v mod n`, a signed 32-bit
   remainder.

This is exactly the C runtime `rand()` of Microsoft compilers. Consequences:
- The result is in `0 .. n−1` for 0 < n ≤ 32768.
- **For n > 32768 the result never exceeds 32767.** The game asks for 50 000, 60 000 and
  90 000 in the music code (§3.4), so those ranges are really 0 … 32767.
- For n < 0 the result is `v mod |n|` and never negative (no caller does this).
- The small modulo bias of `v mod n` is part of the behaviour.

Test vectors (the raw 15-bit values `v`): from S = 1: 41, 18467, 6334, 26500, 19169, 15724, 11478,
29358. From S = 0: 38, 7719, 21238, 2437, 8855.

### 3.2 Seeding and lifetime: code

- **At program start S = 0** (uninitialised global). The game never seeds from the time.
- **Map load (new game, campaign mission, restart) sets S = 1** (0x4b51ce). This happens after
  the plant jitter and the AI idle offsets have used the generator, and **before the markets are
  stocked**. So the market goods of a fresh map are the same every time.
- **S is not saved.** Loading a save does not restore or reset it. After a load, S is whatever the
  load sequence leaves (§3.3): a fixed function of the map's plants and the number of AI armies.
  Reloading the same save therefore replays the same random stream, as long as nothing else
  draws in between.
- **Wall-clock draws** do come in between: the music rotation (§3.4) draws when real time runs
  out, not at fixed game moments. That is the only source of run-to-run variation in the vanilla
  game, apart from the Community event generator (§3.5).

**Exact draw order at a map load** (0x4b2504, every call that can reach the generator): code.
1. Plant jitter for every plant cell (§3.3). This re-seeds S per cell.
2. For every AI army in file order, `Random(3000)` is drawn: the idle-animation offset in ms
   (0x4b4b43).
3. S is set to 1.
4. A market restock, in building order, for every building of type 1, 6 or 7 whose market flag
   (+0x127) is set: only those get a restock time, and the restock draws nothing for a building
   without one (economy.md §2).
5. AI initialisation and the starting gold, with no draws (the simulated battles inside reach
   only a disabled branch, §3.4).
6. The screen then starts the world music, which draws `Random(90000)` (§3.4).

**Draw order at a save load** (0x4b771c): code. Plant jitter, then AI initialisation, then
`Random(3000)` per AI army, then the world music. There is no reset to 1.

### 3.3 Plant jitter, the only re-seeding (0x4cfb24, 0x483344): code

Done once per map load and save load, over all cells in row-major order (x fastest). For each cell
whose object class (the high byte of the cell's object word) is 9, 10 or 11:
1. `S ← Trunc( sin(800·y + x) × 10⁶ + cos(600·x + y) × 10⁴ + w )`. Here x and y are the 0-based
   cell column and row, w is the full 16-bit object word, sin and cos take radians, and Trunc
   cuts toward zero (to a 64-bit integer, of which S keeps the low 32 bits). The sine and cosine
   are the x87 `fsin`/`fcos` instructions, whose argument reduction is not the correctly rounded
   one of a modern maths library.
2. The x offset is `4 + Random(16)` on odd rows and `28 − Random(16)` on even rows (pixels).
3. The y offset is `Random(11)`.
4. The sway phase is `Random(1000)`, with the state continuing from step 1.

The offsets place the plant inside its cell. The phase drives the tree sway (§7). Step 1 runs in
x87 floating point, and the arguments reach about 10⁵ radians. A reimplementation must evaluate
sin/cos the same way to get the same seeds (unknown: see the FPU note in §1). Exactness matters
for the visual offsets, and for the random stream after a save load, which starts from the last
plant cell's state.

### 3.4 Every consumer of the generator: code

All 43 call sites in the exe (none in the Community code). "When" says what triggers the draws.

| Where | Draw | Meaning | When |
|---|---|---|---|
| 0x483344 | 16 (one of two sites, by row parity), then 11 | plant x and y offsets (§3.3) | map load, save load |
| 0x4cfb24 | 1000 | plant sway phase | same |
| 0x4b4b43, 0x4b8691 | 3000 | idle-animation offset of each AI army (ms) | map load, save load |
| 0x4ad8a0 | 3000 | the same for every AI army on the map facing a next step (direction < 8), with a patrol radius above 0, in no building | each time the hero stops: a walk's or a wait's end, an event or an AI army stopping him; not a run into an army or a garrison (world.md §2.2.1) |
| 0x4be178 | 5, 5, 3, k+1, k+1 | market goods: max price +1 when 0; the town's extra potion (4 of 5 cases); the healing potion 98 + r; biased item pick; candidate pick (economy.md §2) | every midnight per building whose restock time is set and due; map load (market buildings only) |
| 0x4a1998 | D div max | barracks slot +1 when the result is 0 (D = MaxDayCountForNewUnit) | every midnight |
| 0x4bba40 | 2, 3, 6, 6, 6 | village offer rolls: innkeeper, priest, blessing, furs, witch (economy.md §3); the draws stop at the first roll that picks an offer, and each `Random(6)` after the first is drawn only when the previous one did not pick | entering a village with stock |
| 0x4aca80 | 5, 5 | village spell `3 + 2r`; witch mana `300 + 50r` | building the village offer |
| 0x4a2550 | W, H (free) or the box widths (patrol) | 4 wander points of an AI army (below) | AI goal refresh |
| 0x4a4a7c | 3 (repeated) | AI promotion option (below) | AI XP gain |
| 0x4a548c | X | the XP of a hired AI unit: `Random(X) + X div 2` (the calls return to 0x4a6b74 for a unit the army hires, 0x4a7017 for one it buys for its garrison) | AI hiring |
| 0x4ab150 | 20 | anti-cheat: gold becomes `5 + Random(20)` | only when the gold seal is broken |
| 0x4d1282, 0x4d155f, 0x4d165e | 3 | which of the three event sounds plays | opening the village, shipyard or event window; the event window also shows the victory box of the player's battle and the noon report, which draw it too (FINDINGS.md §14, §15) |
| 0x49d774, 0x49d7f8 | 8; 50 000 / 60 000 / 90 000 | music rotation (§9) | real time |
| 0x486237 | spread % | battle AI noise | **never**: the spread is 0 at every caller |

Details the other files rely on:
- **Draw counts matter.** `Random(0)` and `Random(1)` still advance S. The barracks roll always
  draws for a qualifying slot, even when its divisor is 0 or 1, and then always grows.
- **Midnight order** (0x4a1998): for each building in turn, its market restock draws first, then
  its six barracks slots (slots 1–6, only those with a unit, a non-zero max and count < max).
- **Wander points** (0x4a2550): four points, drawing x then y for each. For a non-patrolling
  army, x = `Random(map width)` and y = `Random(map height)`, anywhere on the map. For a
  patrolling army, x = `xmin + Random(xmax + 1 − xmin)`, and the same for y inside its patrol box.
  A point equal to the army's own cell is dropped (its x is set to 0). There is no passability
  test here. The user of these points (0x4a2d88) skips every point whose x is not above 0, so a
  free point drawn in column 0 (or a patrol point in a box starting at column 0) is lost as well,
  and it passes the others through a map test (0x4764c0, see ai.md) before using them.
- **AI promotion** (0x4a4a7c): only when the unit type has at least one upgrade. 0-based type 3:
  `Random(3) = 0` gives option 1, otherwise option 3. Type 7: `Random(3) = 0` gives option 3,
  otherwise option 1. Any other type draws `Random(3) + 1` again until it hits an existing option,
  so it may draw several times.
- **Battle AI**: the only random code in battle is the disabled noise above. When enabled, it
  would re-seed S from the turn and initiative counters (0x4861d8). Battles are deterministic.

### 3.5 The other generators: code

- **Community event generator** (0xc28dae): a separate 32-bit LCG, `s ← s × 23 479 589 +
  328 221 219`. The constants are the textbook 1664525 / 1013904223 typed as hexadecimal by
  mistake. It is seeded from the CPU time-stamp counter whenever the events array is allocated
  (map load and save load both allocate it), so it is not reproducible. `Range(lo, hi)` draws until `s` falls below the largest
  multiple of `n = hi − lo + 1`, then returns `lo + s mod n` (unsigned). Its retry loop jumps back
  one instruction too far, so after a rejection the limit becomes the rejected value × n. Only
  user: event opcode 18 (random flag digit, events file).
- **Delphi RTL generator** (0x403278): `seed ← seed × 134775813 + 1`, result `(range × seed) >> 32`.
  It is used only by a byte scrambler of the engine's file container (0x471be0): the seed is set to
  the buffer length, then each byte is XOR-ed with `Random(256)` (mode 2) or with a counter
  1, 2, 3, … (mode 1). The container has a 12-byte header starting with "A?pf" and optional
  zlib chunks. No file in the install uses it; probably the save files (saves-data). **data**.

## 4. Input

**Mouse** (0x475748, 0x4756f8): code.
- The position comes from mouse messages in client pixels. The cursor's drawn position is the
  mouse plus the cursor's hotspot offset.
- Button down sets "button down" to left or right and clears "released". Button up sets
  "released" to that button, clears "down" and resets its held-frame counter. "Released" is a
  latch: it stays until the next press.
- Double clicks set a flag nothing reads (unused).
- The wheel adds its delta to a per-frame sum, which is reset every frame. The wheel message also
  overwrites the mouse position with its screen coordinates. That is harmless in fullscreen.

**Hover and click dispatch** (0x474648): code.
- Each frame the 128 control slots of the active window are tested in order. The first control
  whose rectangle contains the mouse wins. A "shaped" control also needs the pixel under the
  mouse to differ from its colour key. Hidden and disabled controls never win.
- When the hovered control changes, the old one gets "leave" and the new one "enter".
- A press counts only if it started over a control while no button was down. So dragging onto a
  button and releasing does not click it.
- The press goes to the hovered control and remembers it. The release goes to **the control
  that got the press**, even if the mouse has left it (the control decides what to do).
- The hovered control gets a hover call every frame.

**Keyboard** (0x475748): code.
- Key down stores the key code as "held" and clears "pressed". Auto-repeat keeps storing the same
  key.
- Key up turns the **current held key** into "pressed" and clears "held". If two keys overlap,
  the reported key is the last one pressed, not the one released.
- Typed characters are stored for one frame.
- System keys (Alt combinations) are not handled.
- Opening a window clears both, and if a key is still down its key-up is swallowed (0x47f358).
- Screens act either on "held" (Esc, with their own one-shot latch) or on "pressed", which they
  clear themselves.

**Lists** (0x47eb84, 0x47ede0): code.
- Holding Up or Down scrolls or moves the selection by one step each time more than 50 ms have
  passed since the last step. At 60 Hz that is one step every fourth frame.
- The wheel moves one line per frame in which the wheel sum is non-zero, whatever its size.

**Edit box** (0x47a624, 0x47ab2c, 0x47ad20): code.
- Only characters that exist in the 150-glyph font order (§5) are accepted, up to 255. That order
  includes code 1, the caret glyph, so the control character Ctrl+A types is accepted too.
- If the new text is wider than the box minus twice its margin, the character is removed and the
  box acts as if **Enter** were pressed (it commits).
- Backspace deletes the last character. Esc restores the committed text and leaves the box.
  Enter commits and leaves.
- Clicking elsewhere reverts by default (a per-box flag can make it commit).
- The caret is a glyph appended to the text. It blinks 500 ms on and 500 ms off, by the phase of
  Now.

## 5. Text and fonts

**Font sheets** (0x47866c): code.
- A font is a picture (BMP or the engine's LIT format) with glyphs separated by lines of a
  separator colour. The separator colour is the colour of the top-left pixel.
- The row step is the y coordinate of the first separator pixel in column 1 at y ≥ 1 (that is,
  the distance from the top line). Rows start at y = 1, 1 + step, 1 + 2·step, … Every glyph is
  one pixel less tall than the step.
- Glyphs are cut along each row from x = 1. A glyph runs up to the next separator pixel on the
  row's first line, and the next glyph starts just after it. A row ends when no separator pixel
  is left to its right. Rows repeat while column 1 still has separator pixels below.
- At most **150** glyphs are cut. They are assigned in a fixed order: Latin capitals A–Z,
  lower case a–z, the digits 1 to 9 then 0, eighteen punctuation marks (backslash, equals, minus,
  plus, question mark, exclamation mark, comma, full stop, apostrophe, double quote, colon,
  semicolon, both parentheses, less-than, greater-than, slash, percent), space, the caret
  (character code 1), the 33 Cyrillic capitals, then the 33 Cyrillic lower-case letters, then the
  at sign and the underscore. The Cyrillic letters are in alphabetical order with Ё after Е,
  except that the three signs come as soft sign, yeru, hard sign (not hard sign, yeru, soft sign).
- A sheet without an underscore glyph uses the space glyph for it.

**Measuring and drawing** (0x4788ec, 0x478978): code.
- Width = the sum of the widths of the glyphs present. Characters without a glyph have width 0
  and are skipped.
- Height = the height of the capital A glyph.
- Each glyph advances by its own width: no kerning and no extra spacing.
- Draw modes: colour key black, colour key white, additive, subtractive.

**Justified lines** (0x478a9c): code. The free width (box width minus the non-space glyphs) is
split evenly over the spaces, as a float. Each word starts at the rounded running position. The
rounding is the engine's own **round-half-up on the first decimal** (0x471d9c): the integer part,
plus one if the first decimal digit is 5 or more (negative values just truncate). Its argument is
a single-precision float, so the value is rounded to 24 bits before the digit is taken. This rounding
is used only by the text widgets (list line positions, scrolling, highlight rectangles).

**Word wrap and lists** (0x47e46c, 0x47e1fc): code.
- Lines break at the last space before the overflow, and trailing spaces are trimmed.
- Usable width = box width minus twice the margin.
- Line advance = glyph height × the line's spacing factor, capped at 128 px.
- Alignment: left, centre, right, or justified, where the last line of a paragraph is left-aligned.
- Lines that do not fit are cut and finished with an ellipsis (0x47e944).

## 6. Colour and blending: code

These are summarised here; the full tables are in `docs/notes/map/004741b4-0047e944.md`.
- Packing 8-bit channels into 16 bits adds a rounding half step first: +4 for a 5-bit channel, +2
  for a 6-bit channel, saturating at 255. Unpacking copies the top bit of a 5-bit value into the
  freed low bit.
- Additive and subtractive blits saturate per channel. Pixel value 0 is transparent in them.
- Alpha masks have 5 bits and are premultiplied: `out = dst × (31 − a)/31 + src`, saturating.
- Greyed pictures use `(299 R + 587 G + 114 B) / 1243`, a darkened luminance (0x48dc60).

## 7. Animation rates

Animation phases come from a clock, not from frame counts, so they do not depend on the frame rate.
"t" is the shared last-read time (§2) unless stated otherwise. The animation flag that gates the
water and the tree sway (0x65d4d7) is forced on by the options reader (0x4b8a7c), so both always
animate.

| What | Rule | Where | Tag |
|---|---|---|---|
| Water terrain (codes 0–2) | texture frame `(t div 100) mod 32`: 32 frames at 10 per second | 0x4c8db2 | code |
| Tree sway (class 9) | `s = (t div 70 + phase) mod 51`; offset `0.2·(s − 12.5)` for s < 25, else `0.2·(37.5 − s)`; added to the x coordinate of the sprite's first two vertices, so the top edge leans left and right (−2.5 … +2.5 px, period 3.57 s) | 0x4c99d0 | code |
| Hero walking | frame `((t − step start) div (WalkDelay div 2)) and 3`, plus 3: figure frames 3–6, two frames per step at the default 150 ms; standing = frame 0 | 0x4ae8f7 | code |
| AI army walking | frame `((game time div 1000) and 3) + 3`: it changes every 10 **game** minutes | 0x4ad779 | code |
| AI army idle on land | a 20-step table at 250 ms per step, from `t − stamp − offset` | 0x4c868c | code |
| Ship | 8 frames: AI ships at 100 ms (0x4ad314) or 200 ms with the army's offset (0x4c868c); the hero's ship while moving at `((t − step start) div (WalkDelay div 2)) and 7` (0x4ad4cc) | | code |
| Selection ring | 8 frames of 100 ms, 5.625° per frame | map 004c612c | code |
| Route arrows | brightness 150–240 by a triangle wave over `(t/10 + 18k) mod 181` | map 004c612c | code |
| Animated cursors | 50 frames: clock and swords every 30 ms, house every 50 ms, question every 25 ms, from **raw** time | 0x48eb98 | code |
| Main menu background | 9 frames × 150 ms cross-faded, plus a 16-frame layer × 100 ms | 0x4c7774 | code |
| Credits | scroll at 35 px/s | 0x4c82c4 | code |
| Tooltip | fades in over 500 ms | 0x4c9b5b | code |
| Selected army slot | brightness `8 + triangle((t div 100) mod 15)` | 0x4cd704 | code |
| Unit card slide | at most 200 ms | 0x4b0284 | code |

- **WalkDelay** (0x4b8c54) = `(100 − WalkSpeed) × 250 div 100 + 150` ms. That is 150 ms at the
  shipped WalkSpeed 100. Half of it (integer) paces the walk frames.
- **Scroll factor** = `(1 − ScrollSpeed/100) × 1.5 + 0.5`.
- **AnimationSpeed** (Community, 0..100) caps some world animation delays at `(100 − S) × 5` ms.
  data.
- **Animation queues** (0x48c2d4–0x48c45c): 64 queues. Each frame the first entry of every
  non-empty queue runs, and an entry removes itself when finished. So animations in one queue play
  one after another, and different queues play at the same time. code.
- The AI army offsets (`Random(3000)`, §3.4) only desynchronise the idle animations.

## 8. Sound

**Format** (0x48034c, 0x480618, 0x480888): code.
- DirectSound with a mono, 22 050 Hz, 16-bit primary buffer. Mixing is done by DirectSound.
- Effects are 8-bit mono 22 050 Hz static buffers. The loader ignores the file's header fields.
  It skips the first **64 bytes** and takes `file size − 80` bytes, so on the usual 44-byte WAV
  it drops the first 20 samples and the last 16.
- "raw" files are headerless 16-bit mono 22 050 Hz (music, and some effects).
- Community converts any `.ogg` once into a `.raw` next to it, ignoring its channel count
  (0x4e5a30). The current install has none.

**Slots and playback** (0x481420, 0x4813ac, 0x4812a0): code.
- Up to 1024 sounds, each with its own volume, a maximum volume (0..1000) and a group (1 =
  music, 2 = effects).
- Each sound has exactly one buffer. Playing a sound that is still playing **restarts it**
  (rewind) when asked to, otherwise it simply continues. A sound never overlaps itself.
- Music streams through a 4-second ring buffer, refilled when less than 1 s is queued, checked at
  most every 200 ms. A stopped stream remembers its position.

**Volume curve** (0x481554): code. For a volume v in 0..1000, the attenuation in hundredths of
a decibel is
`Round(10000 − 10 × √v / (√v/1309 + 1/132))`,
and the buffer gets minus that value. Here and in the options below, Round is the x87 default:
to nearest, ties to even. The intermediate quotient and the attenuation are stored as
single-precision floats. Sample values: 1000 → −0.35 dB, 800 → −3.1 dB,
500 → −9.3 dB, 200 → −23.1 dB, 100 → −34.3 dB, 20 → −59.3 dB, 1 → −88 dB, 0 → silence
(−100 dB). The slider is therefore strongly non-linear: the lower half of the range is nearly
silent.

**Options** (0x4b8bb4, 0x4b8b3c): code.
- `MusicVolume` and `SoundVolume` (0..100, in the options section of the language ini) become
  `Round(value / 100 × 1000)`.
- That number becomes both the maximum and the current volume of every sound in the group.
  Lowering a group's maximum also rewrites the ends of running fades that aimed at the old
  maximum.
- Moving the options slider plays a test sample while it moved in the last 250 ms (0x4bf308).

**Fades** (0x4819cc, 0x481790): code.
- A fade is (start, duration, from, to, sound). Fades are kept sorted by start and processed
  every frame by the main loops.
- While running, `volume = from + (now − start) × (to − from) div duration`.
- A fade that starts from 0 first starts the sound (looping, from the beginning) at volume 1.
- Any volume below 1 stops the sound. At the end the volume is set to `to`, or the sound is
  stopped if `to` is below 1.
- A newer fade on the same sound cancels the older one and starts from the sound's current
  volume.

## 9. Music rotation (0x49d774, 0x49d7f8): code

- **Playing a track** cross-fades: the current track fades to 0 and the new one from 0 to the
  music volume, both over **2 s**.
- When the track is the world theme (the second map theme, `BkgMap2`), the rotation index becomes
  1 and the next change is due at `now + 90 000 + Random(90 000)` ms, that is **90.0–122.8 s**
  (§3.1).
- **When the change is due** (checked by the world, window and menu frames while a game is
  running; not in battle; the world frame skips it while the triumph or defeat piece plays):
  1. Draw `Random(8)` again until it differs from the current index.
  2. Play that index's track, cross-fading over **4 s**.
  3. Schedule the next change:

| Index | Track (ini key) | Next change after |
|---|---|---|
| 0 | BkgMap1 | 40 000 + Random(50 000) ms → 40.0–72.8 s |
| 1 | BkgMap2 | 90 000 + Random(90 000) → 90.0–122.8 s |
| 2 | BkgMap3 | 40.0–72.8 s |
| 3 | **BkgAuthors** (the credits theme) | 90.0–122.8 s |
| 4 | BkgMap4 | 90.0–122.8 s |
| 5 | BkgMap5 | exactly 80 s (no draw) |
| 6 | BkgMap6 | 60 000 + Random(60 000) → 60.0–92.8 s |
| 7 | BkgMap7 | 40.0–72.8 s |

- Tracks loop until replaced. The change times ignore the track lengths.
- The credits theme is part of the world rotation, and BkgMap2 is both the entry theme and a
  rotation member.
- After the triumph piece, closing the event window switches to a random track at once (0x4c20b3).
- These draws happen in real time, interleaved with the game's own draws (§3.2).

## 10. Razdor now → original

| Topic | Razdor now | Original | Status |
|---|---|---|---|
| Generator | the LCG `S×214013+2531011`, `random(n)` = 15 bits `mod n`, drawn even for n = 0, one state per game, not saved (`rules/rng.rs`); Razdor's own rolls use `range(lo, hi)` = one `Random(hi − lo + 1)`; the Community event generator (`EventRng`) with its retry slip, seeded from the clock at every load | MSVC-style LCG `S×214013+2531011`, 15-bit output `mod n`, cap 32767; one global state | Matches |
| Seeding | 1 at every new game and campaign map (`Game::with_world`), not saved (save format 2; format 1 saves load, their generator ignored); a save load runs the load sequence: plant hash of the map's last plant, `Random(3000)` per army of the map file, `Random(90000)` for the world music (`Rng::save_load`) | 0 at start, **1 at every map load** (before the markets), not saved; after a load it comes from the plant hash | Matches (a map without plants starts from 0, not from the last session's state; the plant hash uses f64 sin/cos, see §11) |
| Draw order | map load: 1, the markets, the music's draw; save load as above; midnight building by building, its market restock then its barracks slots (`economy_midnight`) | fixed order at load (§3.2) and at midnight (restock, then barracks, per building) | Matches; the music rotation draws in real time and the event, village and shipyard windows and the victory box and the noon report (the event window) draw their chord's `Random(3)` as they open (`rules::music`, `Game::event_chord`); the idle patrollers' `Random(3000)` when the hero stops is not drawn |
| Barracks roll | one `Random(D div max)` per slot with a unit, a maximum and room, also when the divisor is 0 or 1 (`economy::regrow`) | always one draw per qualifying slot | Matches |
| Wander points | 4 points, x then y: `xmin + Random(xmax + 1 − xmin)` in the patrol box (post ± radius, clamped), else `Random(W)`, `Random(H)` over the map; no passability test; the own cell and column 0 dropped; guards draw nothing (`ai::wander_points`) | 4 points uniform over the whole map (non-patrol) or the patrol box, no passability test, own cell dropped | Matches (drawing; Razdor's goal scoring still seeds only patrollers' points and has no obstacle map, ai.md) |
| AI promotion | `Random(3)` for Militia and Infantry, `Random(3) + 1` rejection loop for others (`ai_promote`) | same | Matches |
| Hire XP | `Random(X) + X div 2` | `Random(X) + X div 2` | Matches |
| Plant jitter and sway | the draws are made at a save load for the stream (`Rng::jitter_plants`), over the original's `(W + 8) × (H + 2)` cell array with its columns for the hash (`rng::plant_layer`); the map draws the offsets (`rng::plant_offset`), not the sway | per-plant offsets from the trig hash; triangle sway 3.57 s | partly (the sway is left for later) |
| Frame pacing | macroquad `next_frame`, variable window size, game time from frame time | vsync flip, no fixed tick, all timers on a millisecond clock | equivalent in spirit |
| Clock pause on focus loss | not modelled | Now freezes while inactive (walks, waits, music, fades stop) | missing |
| Hero walk frames | 8 frames at 10 per second while moving (`world_view::draw_figure`) | frames 3–6 at half the WalkDelay (75 ms default); AI walk frames by game time; land idle 20 × 250 ms | differs |
| AI walk frames | Frames 3–6, the next every 10 game minutes (`(time_cs div 1000) and 3` + 3) while the army has a step to take, by the game time interpolated inside the stretch, so they stand still with it; its standing frame without a step (`Game::army_walk_frame`) | 0x4ad660 → 0x4ad314 with `[0x68dcb8] div 1000`, from the AI's per-frame advance 0x4ade3c (only while the hero walks or waits) | Matches |
| Water | static textures | 32 frames at 100 ms on terrain codes 0–2 | missing |
| Cursor animation | system cursor (no animated cursors found in `src/ui`) | 50-frame cursors at 25/30/50 ms from raw time, hotspots per cursor | missing |
| Main menu background | 9 frames at 100 ms, not cross-faded (`main_menu.rs`) | 9 frames at 150 ms cross-faded + 16 frames at 100 ms | differs |
| Credits scroll | 30 px/s × UI scale | 35 px/s | differs |
| Font sheets | glyph boxes found by colour heuristics; order string lacks the 150th glyph (underscore); text scaled to a TrueType cap height (`ui/dt_font.rs`) | separator colour from pixel (0,0), fixed slicing rule, 150 glyphs ending with underscore, pixel-exact | differs (layout scale is a Razdor choice) |
| Text rounding | float layout | round-half-up on the first decimal for justified spaces and list lines | differs (cosmetic) |
| Volume | linear gain 0..1 in `audio.json` | 0..1000 with the √-shaped dB curve; options 0..100 ×10 from the ini | differs |
| Music | `BkgMap2` at a map start or load, then the timed rotation of the 8 tracks drawn from the game's generator (`rules::music`, `ui/jukebox.rs`), tracks looped; no fades | timed random rotation of 8 tracks (BkgAuthors included), cross-fades of 2 s and 4 s, change times independent of track length | Matches but the cross-fades (presentation, left out) |
| Effect playback | one sound per effect, a replay stops it and starts it again (`ui/audio.rs` `play_effect`) | one buffer per sound: a replay restarts it, never overlaps | matches |
| WAV start offset | whole sample | first 20 bytes and last 16 bytes dropped | negligible |
| Edit box | save name: any non-control character up to 60, caret 500 ms (`ui/saves.rs`); `widgets::text_field`: any script, Esc or click elsewhere ends typing and keeps the edit | only glyph-order characters, length limited by the box width (overflow acts as Enter), Esc reverts, click elsewhere reverts, 500 ms caret | differs |
| Key semantics | macroquad key events | "pressed" is reported on key-up (last held key) | differs (input feel) |
| Loading time | as fast as possible | at least 4 s for the initial load bar | differs (intentional speed-up is fine) |

## 11. Unknowns

- **FPU precision** on Windows with DirectX 7: whether the game runs with x87 single precision
  after the Direct3D device creation there. Under Wine 11 it runs with 64-bit mantissas (§1).
  That decides the exact plant-hash seeds, and the last bit of every float formula elsewhere.
- How DDrawCompat (shipped `ddraw.dll`) changes flip timing, vsync and the FPU state.
- The exact source rectangles of the per-frame "visible → back buffer" restore (the stack arguments
  are lost in the decompiler); assumed one copy per dirty rectangle.
- The Direct3D render and texture-stage states set at start (texture filtering of the world
  quads).
- The writer of the "cursor hidden" flag (0x4ecd88).
- Which files use the scrambled "A?pf" container (probably saves).
- The two ship-frame formulas (100 ms in 0x4ad314, 200 ms with an offset in 0x4c868c): which
  one wins for AI ships each frame.
- The Community AnimationSpeed rules were only read from the map, not traced.
