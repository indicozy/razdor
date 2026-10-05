# Discord Times interface: screens, input, camera, hints, sound and timing

This file covers the Community Update (Unstable) `DiscordTimes.exe` (Delphi, image base
0x400000) of the player's install. It describes how the original runs its screens: start-up,
the main menu, a new game, the world map, the windows over it, the battle screen, hints,
message boxes, the camera, the music and which sound plays when, and the frame timing. Rules
and numbers are in our own words. Addresses are virtual addresses in that build and are given
as evidence only. Rendering is described only where it decides behaviour.

Confidence tags:
- **code**: confirmed by reading the code.
- **data**: consistent with the data files or footage, or partly traced.
- **unknown**: not determined.

Other files own the rules behind the screens: walking and the clock (world.md), battle rules
(battle.md), prices and buildings (economy.md), levels (experience.md).

---

## 1. Display, frame loop and timing

**Display mode.** The game always asks for exclusive full screen at 1024×768 with 16-bit
colour (one back buffer, page flipping). A windowed mode exists in the engine but only runs
when a window handle is passed in, and the game never passes one (47609c called from the entry
point with no handle; 475ccc full-screen branch, display mode set from the stored screen size
and 16 bits). The player's install gets a window only through an external DirectDraw wrapper.
**code**

**Two layers.** The world map, the minimap, the menu background, map tooltips and hint boxes
are drawn with Direct3D on the back buffer. Windows, buttons and text are drawn into a
1024×768 software canvas, and only the rectangles that changed are copied to the back buffer
each frame. The mouse pointer is drawn by the game itself. **code** (474648, 47453c)

**Frame loop** (476298, 474648). There is no frame cap and no fixed tick. Whenever the message
queue is empty the engine runs one frame and ends it with a page flip, so the frame rate is
whatever the flip allows. When the program loses the focus it stops running frames and
sleeps 200 ms per loop pass, so the whole game pauses. **code**

One frame, in order: count how many frames each mouse button has been held; restore the
previous frame's changed areas; run the current screen's per-frame handler (all game logic
lives there); if the screen takes input, run the keyboard hooks, find the widget under the
mouse, send it enter/leave, press/release and hover events; copy the changed areas; run the
screen's after-draw handler (hint boxes); draw the pointer; flip. **code** (474648)

**Clocks.** Two millisecond clocks are used: the system time read on demand, and a per-frame
copy of it. Animations compare one of them with a start time; no animation counts frames.
**code**

**The timeline.** Timed sequences (the hero's walk, waiting, camera glides, battle strikes,
pauses, chained events) are callbacks placed in one of 64 first-in-first-out queues
(48c2e4). Each frame the head entry of each non-empty queue is called once with three numbers
it keeps between calls (usually a start time that is 0 until the first call, and two
parameters); the callback removes itself when done, and the next entry starts on a later
frame. Almost everything uses queue 0, so timed things never overlap: a camera glide finishes
before the walk it precedes, a strike finishes before the next battle action is chosen. The
heads run only from the per-frame handlers of the world map, the battle screen, the army
and building windows and the hero-choice screen (the five callers of 48c45c; the village and
shipyard windows do not run them; the building window only while its hire or garrison tab is
open, 4cd94c). The queues have no overflow check (64 entries each). One
callback (look at an army, 4afa98) inserts a glide at the front of the queue instead of the
back. **code**

## 2. Input model

**Mouse buttons** (475748). A button press records which button is down and clears the
"released" code; a release records which button was released and resets that button's
held-frame counter. Each frame the held counter of a pressed button goes up by one, so "1"
means "pressed this frame". Double clicks are recorded but nothing reads them. The wheel is
summed per frame and only text lists use it. **code**

**Widgets** (474648, 4743c8). The widget under the mouse is the first one, in slot order, whose
rectangle contains the mouse (and, for shaped widgets, whose image pixel there is not the
transparent colour); disabled and hidden widgets are skipped. Controls are stored from the
last slot downwards, so the control added last is found first and wins overlaps. **code**

- Enter and leave events fire when the widget under the mouse changes; the entered widget's
  tag becomes the "current control id" that most handlers read. **code**
- A press is delivered only if the mouse came onto the widget while no button was held, so
  dragging onto a button with the button down does nothing. **code**
- The release goes to the widget that received the press, wherever the mouse is now. A
  standard button calls its click action on that release unconditionally, so pressing a
  button and releasing elsewhere still clicks it. Some handlers add their own check (the
  bottom panel's wait and centre buttons only act when released over the same button). **code**
  (47a0dc, 4b9448)
- Standard buttons show their pressed image and play the button sound on the press, and act
  on the release. **code** (479eb8, 4b958c)

**Keyboard** (475748). A key-down stores that key as the "held key" (only the last one; there
is no key state table) and clears the "released key". A key-up copies the held key into the
"released key" and clears the held key. The released key is not cleared each frame; it stays
until the next key-down or until a handler clears it. Typed characters are kept for one frame.
Keys sent as system keys (Alt, F10) never reach the held key. Showing a window clears both the
held and the released key, and if a key was held at that moment its next key-up is swallowed,
so the key that opened a window does not act in it. **code** (475748, 47f358)

**One action per Esc.** Every screen that reacts to Esc uses one shared latch: when no key is
held the latch opens; Esc with an open latch acts and closes it. A second Esc therefore needs
all keys released in between. **code** (4cd021, 4c8040 and the other frame handlers)

## 3. Start-up and loading

The entry point (4e7424) opens the six ini files, sets up the display, Direct3D and sound
(one channel, 22050 Hz, 16 bit), reads the option flags, then runs the splash and loader
(4e5c9c) and enters the main menu. **code**

1. **Splash and logos.** The splash picture is drawn. If `Logo\Logo.ini` exists, up to eight
   logo slides are read (`Logo_Count`, and per slide its picture, show time and fade time);
   after a 2000 ms pause they play in turn, each fading in and out over its fade time
   (alpha = elapsed × 255 / fade). **code** (4e5c9c, 48c844)
2. **Loading bar.** Fonts, the bar and the loading, copyright and version texts are drawn.
   The bar fills in proportion to the bytes loaded against a fixed budget, with three glints
   moving at 250 px/s and its last 16 px dimmed. It never fills faster than 4 seconds: each
   redraw waits until `filled/width × 4000 ms` have passed. **code** (48c844, 48cd24)
3. **First-run sound conversion.** Every `Sounds\*.ogg` is decoded to headerless 16-bit PCM
   `.raw` and the `.ogg` is deleted. **code** (4e5a30)
4. **Resources and options.** Textures, sprites, window graphics, fonts, unit pictures,
   backgrounds, the map and save lists (with their caches), volumes and speeds. **code**
5. **Menu music** starts at full volume without a fade, looped. **code** (4e6484)

At exit the map-list cache is written if it changed and the save-list cache always. **code**

## 4. Main menu

**Layout** (4d2764). Five invisible hot zones over the 3D buttons of the animated background:
four of 228×96 px at x = 398, y = 200, 298, 396, 494, and a fifth of 196×131 at x = 414,
y = 592. Top to bottom they are New game, Load, Options, Authors and Exit. **code**

**Hover.** Entering a zone lights that item and plays a bell. The hover code picks one of
three bell slots per item (item 1 → slot 3, 2 → slot 2, 3 → slot 1, 4 → slot 2, 5 → slot 3),
but the sound loader reads only `MainMenuSelect-1` and copies that one handle into all three
slots; the keys `MainMenuSelect-2` and `-3` are never read. So every item plays the same
`MainMenuSelect-1` sound. Leaving clears the light. **code** (4b9b3c, table 4b9b62; loader
4e3018–4e303c)

**Press and release.** Pressing a zone pushes its 3D button in (its corner vertices move by 1
to 3 px) and plays `MainMenuPress`. Releasing pops it out and the item acts on the next frame.
**code** (4b987c, 4b95d0, 4c7f0c)

**Actions** (jump table 4c7f30):
- **New game.** If the tutorial is not completed and no save exists (the flag is set by
  `[Tutorial] Completed=1` or by finding any save), the tutorial starts at once as a one-event
  dialog built from the `[Tutorial]` section. Otherwise the scenario list opens. **code**
  (4c7f44, 4ac748)
- **Load** opens the load window, **Options** the options window, **Authors** the credits.
  **code**
- **Exit** closes the game at once: black screen, display mode restored, window closed. Esc
  in the main menu does the same. There is no confirmation. **code** (4c7ffe, 4c8059, 47f744)

**Background** (4c7774): a 9-frame layer at 150 ms per frame and a 16-frame layer at 100 ms
per frame, each cross-faded between neighbouring frames, the copyright and version lines, and
the Community FPS counter. **code**

**Credits** (4c82c4, 4cfc6c): the credits picture scrolls up at 35 px/s with faded top and
bottom edges; the credits theme (`BkgAuthors`) plays; Esc or OK closes it and the menu theme
comes back with a crossfade. **code**

## 5. New game, hero choice, starting and loading

**Scenario list** (4d2a30). The list order (tutorial first, then by map size, campaigns
followed by their chain) is in the phase-1 notes and dtm-format.md. Each row gets a small
icon in its fourth column by the map header byte 0x120: 1 castle, 2 helm, 3 swords, 4 skull,
5 tutorial (pictures `SI_Castle`, `SI_Helm`, `SI_Swords`, `SI_Skull`, `SI_Tutorial`); 0 gives
no icon. The icons are set on all rows the first time, then again on the old and the new row
whenever the selection changes. **code** (4c1968, pictures loaded at 4dc118) Start
copies the chosen map's header and enables each hero class whose preset has a start cell
(start x or start y non-zero; gold, troops or a start building alone do not count). It walks
the classes from the ranger down to the knight and opens the hero window on the last one it
enables, that is the **first** offered class in knight, archmage, ranger order; with none
offered it returns to the main menu instead (4c1804). Back or Esc returns to the main menu.
**code**

**Hero choice** (4d7d30, 4c1458). Three class portraits, a name field (default from the
interface ini), and three text boxes. Choosing a class redraws the portraits, shows the class
texts and plays a 200 ms highlight animation that starts with `MainMenuPress` (4b2044).
Cancel or Esc returns to the **main menu**, not to the scenario list. **code** (4c0fd4, 4c8584)

**A class the map does not offer cannot be picked.** Its portrait is drawn with the second
greyed copy (`65bce4[i]`) and its enabled byte (portrait widget +0xd, `65bc01 + i·0x4b`) is
0; the frame's hit test (4743c8) never reports a widget whose enabled byte is 0, so the
press handlers (4c17c0 / 4c17d4 / 4c17ec, the widgets' +0x2f, which call 4c1458 with no check
of their own) cannot run for it, and hovering it does nothing. The window's frame (4c8584)
reads no key but Esc; the name field's keys only edit the name. Start (4c1000) does not check
the class, but the class (`68dccc`) can only have been set by the opening or by a click on an
enabled portrait, and the next opening resets it. **code**, checked in the running game
(Устье Трейна, whose archmage preset has no start cell: the window opens on the knight, a
click on the archmage and the arrow, Tab, Space, digit, Home and End keys leave the pick
unchanged, the ranger can be picked and starts on his preset's cell). The only way the
original plays a class a map does not offer is a campaign: the next map keeps the class the
campaign was started with and never looks at whether that map offers it (saves-data.md
§10.4, §15).

**Name field** (47a624–47ae5c). A typed character is accepted only if the font has a glyph
for it (the 150-glyph sheet order) and the text still fits inside the field less its margins;
Backspace removes the last character; Esc restores the text the field had when it got the
focus; Enter keeps it. The caret blinks with a 500 ms half-period. **code**

**Start** (4c1000). Busy pointer, screen cleared, start time noted; item, spell and battle
effect data are loaded on first use; the hero's name is taken from the field; the map is
loaded and the restart snapshot taken; the map view becomes 1024×682; the world theme starts
(always `BkgMap2`, see §13); the game waits until at least 500 ms have passed since the start,
then shows the world screen. **code**

**Loading a save** (4c01fc) does the same: windows closed, save read, world theme, at least
500 ms, minimap rebuilt at 200 px for maps under 100 cells wide or 400 px otherwise. **code**

## 6. World screen layout

- **Map view**: the top 1024×682 px. Cells are 32×22 px. **code**
- **Bottom panel** (4d46e0): y = 682, 86 px high. **code**
  - A message box at (372, 684), 280×60, showing the time line and messages.
  - Hovering the message box (while the map is idle) shows three small buttons on it:
    left at (378, 693) **wait 1 hour**, right at (566, 693) **wait 4 hours**, centre at
    (461, 687) **centre the view on the hero**. Leaving the area hides them. Hovering one of
    the three (the same enter handler) shows its name (`cp_Wait1Hour`, `cp_Wait4Hour`,
    `cp_ShowHero`) in a 510 px hint box when hints are on; the area itself has no hint.
    **code** (4b930c, 4b93a8, 4b9448, 49d5ac)
  - Eight icon buttons, four on each side of the message box. Left side, from the box
    outwards: Save, Load, Options, Exit menu (the save icon's right edge at x = 370, each next
    one 1 px overlapping to its left). Right side, from x = 654 outwards: Hero (inventory and
    quests), Army, Spell book, Minimap. They are vertically centred in the 64 px below y = 682.
    Hovering one shows its name (keys `cExit`, `cOptions`, `cLoad`, `cSave`, `cHero`, `cArmy`,
    `cMagic` and the minimap key) in a 510 px hint box, only when hints are on (§10);
    pressing plays `InterfacePanelDown`; releasing opens or closes its window (§9). The
    minimap button toggles the minimap. **code** (4d46e0, 4b9574 → 49d5ac, 4b94b8, 4b94d8,
    4b94e4)
  - The resource line at y = 744: four fields centred at x = 128, 384, 640, 896 with mana,
    gold, daily income (with a plus sign) and daily upkeep (with a minus sign). **code** (49d224)
  - The time line: the label and the game date (a year of 360 days, months of 30, days of
    24 h; leading zero parts left out; under an hour a fixed word). While the hero walks a
    route it also shows the time left, `(route cost × speed × 100 + route start − now) / 100`
    minutes (integer division); a value under 60 is shown as 0, which the calendar writes as
    its fixed under-an-hour word. **code** (49d5dc, 49cbf0)
- **Minimap** (49df9c): in the top right corner, its right edge at x = 1010 and its top at
  y = 14, 200 px square for maps under 100 cells and 400 px otherwise. Its on/off state is
  saved in the save file. Colours of buildings and armies come from the Community colour keys
  of the interface ini. **code**

## 7. World map: mouse and keys

### 7.1 What is under the mouse (4cbf20)

Cursor cell: `x = floor((camera x + mouse x) / 32) − 1`, `y = floor((camera y + mouse y) / 22)
− 1` (the map has a one-cell border). The game then decides, for mouse y below 682 and not
over the minimap:
- **unexplored**: the explored-map pixel there is 0;
- **army** under it (the cell's army byte) and **building** (through the footprint anchor),
  only when explored;
- **valid target**: explored, and passable on the hero's current cost map (land, or the
  land-and-water map while sailing), or the cell holds the hero's parked ship;
- while the hero is aboard a ship, bridges are never picked. **code**

### 7.2 Pointer shapes (4cce5b, 48ed14, 48edc8, 48eb98)

| Situation | Pointer | Animation |
|---|---|---|
| Normal, or over the bottom panel | arrow | static |
| Over an unexplored cell | question mark (static "Ask") | static |
| Over an explored cell that cannot be entered | "Denied" | static |
| Over an army | crossed swords | frame = (ms / 30) mod 50 |
| Over an army whose meeting event is waiting and that is friendly (attitude ≥ 1) | animated question mark | (ms / 25) mod 50 |
| Over a building (not a bridge) | house | (ms / 50) mod 50 |
| Over ruins whose owner is not a map army, or building type 15 | animated question mark | (ms / 25) mod 50 |
| Busy (loading, glides, battle pauses) | clock | (ms / 30) mod 50 |

Over a village, castle, fort, ruins or bridge the game first looks for a guard: an army on the
map whose home is that building and that is hostile (attitude ≤ 0), or any such army on a
bridge. A castle, fort or ruins with a garrison counts as hostile when its attitude to the
player is ≤ 0 (ruins always). A guard or a hostile garrison gives the swords; ruins with an
empty garrison do not. **code** (4ccc56–4ccdeb)

The hovered cell also gets a ring marker (normal, hostile, spell target or "no target"
colours). **code** (48eeb0 calls)

### 7.3 Left button (first frame of the press)

Only on a valid target, and only while the map is idle (no walk, wait, glide or event):
1. **The building the hero stands in**: it opens again (4bbc84). **code**
2. **The hero's own cell**: nothing. **code**
3. **The cell the route was last planned to**: the hero sets off (the walk callback is
   queued). **code** (4cc99f)
4. **Any other cell**: the route is planned and drawn as animated arrows; nothing moves yet.
   If the cell holds an army, that army becomes the hero's target (to fight or meet), unless
   the clicked cell belongs to a building other than a bridge (then the target is cleared). A
   second click on the same cell sets off. The cell only becomes "the planned cell" when the
   planner reached it (path cost below 0xffff); an unreachable cell is re-planned on every
   click and never sets off. Setting off also needs a non-empty route. **code** (4cc426–4cc99a,
   4cc8c3, 4cc905–4cc95d, 4cc99f–4cc9bc)

So walking always takes two clicks on the same cell; the first shows the route and the time
it takes in the time line. **code**

**While the hero walks** (map not idle) a left click or **any key held** cuts the route so
that it ends at the cell of the step in progress: the hero finishes that step and stops.
**code** (4cd132). **A wait is not cut**: checked under Wine on РК1 (2026-10-04), a 4-hour
wait ran its full 240 minutes after a left click on the map, a left click on the bottom
panel, a right click, the A key or Space; the Community endless wait (F4) went on after a
click and a key, and only F5 ended it.

While the map is not idle (walking, waiting, a glide or an event) the world frame skips
everything else: no hover, no tooltips, no scrolling of any kind, no Esc and no Community
F-keys; it only checks the stop rule above and runs the timeline. **code** (4cc1ff jumps
straight to 4cd132)

With the auto-pursue option (`OptValue7`, §16) on, a target army that moves makes
the walk re-plan towards it. **code** for the trigger (4adfb0); the re-plan itself belongs to
the walk loop (world.md).

### 7.4 Right button: tooltips, not commands

Holding the right button (left button up) over an army or a building shows its tooltip: the
army's panel, or for a village, castle, fort, ruins or bridge with a guard army the guard's
panel, otherwise the building's. For the tooltip the "guard" is the last army (in army order)
that is on the map and whose home is that building, whatever its attitude (the hover pointer
of §7.2 also requires hostility; the tooltip does not). While it is up the pointer is hidden and frozen (mouse moves
are ignored); releasing the right button or pressing the left one closes it, and the pointer
jumps to where the mouse really is. The tooltip fades in over 500 ms. The right button never
stops the hero or moves the map. **code** (4cc217–4cc35a, 4ccf10; fade in the renderer at
4ca728: alpha = elapsed × 255 / 500)

Tooltip box: centred on the mouse, kept inside the 1024×682 view; styles normal, hostile
(attitude < 1) and neutral (bridges, empty ruins). The army panel lists up to 2×6 unit
portraits with a darkened band of height `46 × (1 − HP/max HP)` px on wounded units; ruins
never show their defenders. **code** (4ca8ac, 4ca9f0, 4cb18c)

### 7.5 World spell targeting

After a world spell is chosen in the spell book, the map is in targeting mode. Hovering an
army that is a valid target selects it (ring marker, hint with its name); a friendly army with
a waiting meeting cannot be targeted. A left click with a target chosen queues: a wait of
`2 × TimeCast` ticks (through the spell cost divisor), a look at the target army, and the cast.
A right click or Esc leaves targeting mode. **code** (4cc36d, 4cca3b, 4ccb8f, 4cd043)

### 7.6 Scrolling the map

Per frame the scroll step is `round(dt / F)` px horizontally and `round(dt × 0.6875 / F)` px
vertically, where dt is the milliseconds since the last world frame and
`F = (1 − ScrollSpeed/100) × 1.5 + 0.5`. So at ScrollSpeed 100 the view moves 2 px per ms
(62.5 cells per second both ways, since 0.6875 = 22/32), and at 0 it moves 0.5 px per ms.
**code** (4cc18f–4cc1d0, 4b8c4f)

- **Arrow keys**: the held arrow key moves the view one step per frame (only one arrow at a
  time, since only the last key down is kept). **code** (4ccfd9–4cd01b)
- **Edge scrolling**: mouse x below 5 or above 1019, y below 5 or above 763 (the bottom
  panel's lower edge included) moves the view that way; corners move both ways. **code**
  (4cd0af)
- **Minimap**: while the left button is held over the minimap, the view centres on the point
  under the mouse: `camera x = round(((mouse x − minimap left) × map size / minimap size − 15)
  × 32)`, `camera y = round(((mouse y − minimap top) × map size / minimap size − 15) × 22)`,
  where the reference points are x = 1009 − minimap size and y = 15 (one pixel outside the
  drawn image on the left). Round is the FPU's round-half-to-even. Dragging keeps following.
  **code** (4ccf5a, 49df9c)

The view stays where it was scrolled until something moves it (§8).

### 7.7 Keys on the world map

| Key | Effect | Evidence |
|---|---|---|
| Arrow keys (held) | scroll | 4ccfd9 |
| Esc | leave spell targeting, else open the exit menu | 4cd021 |
| Any key while walking | stop after the current step | 4cd144 |
| F1 (Community) | load the newest autosave | c26ae3 |
| F2 (Community) | load the newest own save | c26b38 |
| F3 (Community) | save through the save window's OK on its first slot row | c26bbd |
| F4 (Community) | wait without end (ticks of 30 minutes until stopped) | c277d2 |
| F5 (Community) | end the endless wait | c27802 |

There are no other world-map keys in the original (no letter shortcuts, no zoom). **code**
(every key compare in the world frame and its Community hooks was checked)

All rows except "any key while walking" act only while the map is idle (§7.3). The F1–F4
checks test the held key, not a latched press. **code** (c26ae3–c26bfc, c277d2)

## 8. Camera

- **Limits**: the view's top-left corner stays between (32, 22) and
  (map width × 32 − 1056, map height × 22 − 704) px, applied when the map is drawn. **code**
  (4c8864, 4b771c)
- **While the hero walks** the view is locked to him: at each step it is set to
  `camera = ((x − 14) × 32, (y − 16) × 22)`. Because of the one-cell map border (§7.1) this
  draws his cell at screen pixels 480–511 across and 374–395 down, just right of and below the
  centre of the 1024×682 view (the formula's "14, 16" are not the on-screen column and row).
  Between
  steps it moves with his sprite. Any manual scrolling is overridden while he walks. **code**
  (4ae8a8, 4aea50)
- **A click on the map moves no camera.** The planning click (§7.3) leaves the view where the
  player scrolled it, the hero off screen included, so the second click can be made on the
  same place; the view goes back to the hero only when he sets off, by the walk lock above.
  The click handler writes the camera only for a minimap drag and the arrow keys
  (4ccf5a-4cd00f). **code**; checked under Wine on РК1 (view scrolled 400 px away from the
  hero: the first click left the camera at (210, 440) with the route drawn, the second set
  him off and the camera jumped to (608, 440)). Razdor: the same.
- **Glides** (4af96c): the view moves to the same placement as the walk lock (target
  `(x × 32 − 448, y × 22 − 352)`, the same numbers as above) in 900 ms on a cosine ease: progress
  `e = round(900 × (1 − cos(π t / 900)) / 2)`, position `start + (target − start) × e / 900`
  with integer truncation. Input and the panel are off and the pointer is busy during the
  glide. Used by the centre-on-hero button, events that show armies and lanterns, and world
  spells. **code**
- **Look at an army** (4afa98): glides to the army only if it is more than 300 px away from
  the view centre; otherwise no glide. The distance is measured between (camera x + 512,
  camera y + 352) and (army x × 32, army y × 22) as `(2 × max(|dx|, |dy|) + min(|dx|, |dy|))
  / 2` with integer truncation, that is the larger difference plus half the smaller one (not
  a true octile distance). **code** (4afa98, 4826f8)

## 9. Windows over the map

**One side window at a time** (4b8d28). Ids: 1 exit menu, 2 options, 3 load, 4 save, 5 hero
(inventory, quests), 6 army, 7 spell book; the building window counts as −1. Opening a window
closes the open one first; asking for the one already open closes it (toggle). The matching
panel icon stays pressed while its window is open. Closing any window returns an item held on
the pointer to the pack, and, when a purchase asked for it, re-runs the event check. Opening a
window also leaves spell targeting. **code**

Most windows close with Esc or their close button (top right, `x + w − button − 4, y + 4`),
which shows a close hint when hints are on. **code** (4d2684, 4b95ac)

The window contents follow the rules in economy.md (prices, hiring, market, garrison
exchange), experience.md (promotion) and magic-items.md (wearing items, spells); what the
windows show and how they are operated is in §9.1–§9.9 below. Interaction rules shared by
several windows:
- **Unit grids** (building, army, battle): two rows of six 94×133 cards (rows mirrored for the
  opponent). Hover shows the unit card and a hint line; the row labels say front (slots 1–4),
  back (7–10) or reserve (0, 5, 6, 11). Highlighted cards pulse: brightness level
  `8 + tri((ms / 100) mod 15)` with `tri(v) = v` for v ≤ 7 else `15 − v`. **code** (4c612c,
  4cd704)
- **Load window** (4d67ac): two tabs, own saves and autosaves (the autosave tab only when the
  option is on and an autosave exists); switching tabs plays `InterfaceCastSpell`. **code**
- **Save window** (4d75d4): 12 rows of 31 px; Enter (held) confirms, Esc cancels; the
  progress bar stays at least 1.5 s. **code** (4c820c, 4b648c)
- **Options** (4d2f48, 4d339c): five sliders (music, sound, scroll speed, walk speed,
  Community animation speed) and eight checkboxes (§16). Moving a slider applies the volumes
  and plays the scroll sound as a test; when no slider has moved for 250 ms the test sound
  stops. **code** (4bf308)
- **Exit menu** (4d5214): cancel, quit, back to the main menu, restart (with a confirmation).
  **code**

**Back to the main menu** (4bfb34): all windows closed, the in-game flag cleared, the menu
theme crossfaded in, at least 500 ms waited. **code**

### 9.1 Unit info card (492f24, 491fa4, 4924c9)

The panel on the left of the army, hero, building and battle windows describes one unit; the
new-hero window uses the same stat list for its class preview (4c1334). The header (492f24)
shows the portrait, the title (hero class or type name), the bonus name, status lines (dead,
unpaid outside battle, standing in the back row, garrison or at home), the name, the level
(shown as L + 1) with "XP / XP needed", and the four worn-item slots as buttons. **code**

**Three values per stat.** The list (491fa4) first rebuilds the unit's stats (4908a8). Outside
an interactive battle it copies the current block into the battle block, so the shown values
are the current ones (items, potions, spells included); in battle they are the battle values.
Each line compares three numbers: the **level value** (the type at the unit's level, no items),
the **shown value** (the battle block) and the **current value**. A line is left out when the
shown and the current value are both 0. **code** (491a58)

**Colours** (491a58). The value and its label share one colour: plain when the shown value
equals the level value; a blue tint when it is above; a red tint when it is below the level
value **or** below the current value (red wins over blue). So a wounded unit's Hits are red,
Hits raised by an item blue, a cursed stat in battle red. In a game the lines have a drop
shadow; in the class preview they do not. The tints are font recolourings made at start-up
(4dbc26: blue = red −180, green −70; red = green −152, blue −159; the base font colour is the
font picture's own). **code**

**Lines, in order** (label keys of the interface ini in brackets):

| # | Line | Value | Notes |
|---|---|---|---|
| 1 | Hits (`SHit`) | current HP, or max HP when unhurt | compared with max HP, so wounded = red |
| 2 | Melee attack (`SAttackBlow`) | | |
| 3 | Melee defence (`SDefenceBlow`) | never below 0 | in a game, if the unit's army has a building defence n > 0, written "v + n" |
| 4 | Ranged attack (`SAttackShot`) | | |
| 5 | Ranged defence (`SDefenceShot`) | never below 0 | in a game "v + n" with n = building defence, plus `Row2Def` when the unit stands in a back-row slot (7–10) and is not a hire offer |
| 6 | Magic | see below | only for a unit with a school |
| 7 | Protections | with "%" | one line `SProtectAllMagic` when the three are equal and above 0, else Life, Elemental, Death in that order |
| 8 | Regeneration | with "%" | label `SRegen`, or `SPoison` when the shown value is negative |
| 9 | Community: bleed, evasion | | see below |
| 10 | Vampirism (`SVampirizm`) | with "%" | |
| 11 | Initiative (`SInitiative`) | never below 0 | |
| 12 | Manevres (`SManevres`) |  0.8 s pan, 1.2 s fade, 0.4 s rest, smoothstep; Matches: an event's places right after its window closes, then back to the hero, the next window waiting; the fog opens around the hero at a map start  | |
| 13 | Wage (`DailyPayment`) | the recruit wage of the type's Cost (4a163c) | in a game only; not for a hero class, a named character, a unit of wage kind 3 (event units) or a unit whose level nature is Undead; bold orange |

In "v + n" the colour still compares only v (the defence without the building) with the level
and current values. The "+ n" part is only added in a game; the line is kept when v + n is
above 0 even if v is 0. **code** (491b84, 491fa4; 492f24 sets the row and building flags
68dc66–68dc68)

**Magic lines.** Outside a game (the class preview) a unit with a school gets a single power
line labelled by its school (`SMagicLife`, `SMagicElemental`, `SMagicDeath`). In a game it gets
up to ten effect lines computed from P, the shown magic power, the school of its **type** and
the nature of its **type** (49f8a0). Divisions are integer divisions; W, BM, BN, CM and CN are
`WizardMainSpell`, `BlessMainSpell`, `BlessNextSpell`, `CurseMainSpell` and `CurseNextSpell`
of `_Global.ini`; step(P) = 0 below 20, 1 below 45, 2 below 100, else 3. **code**

| Line (key) | Life | Elemental | Death | Written |
|---|---|---|---|---|
| `CureHit` | P | P / 2 | P, but 0 for an ordinary, Rogue or Animal nature | "+v" |
| `BlessHit` | never set | never set | never set | (never shown) |
| `Atk_Def` | P / BN, then P / BM + 1 | – | P / BM + 1, then P / BN | "+a/+b" |
| `BlessIni` | – | P / W + 1 | – | "+v" |
| `BlessMov` | – | step(P) | – | "+v" |
| `StrikeHit` | P | P | P | "−v" |
| `CurseHit` | – | (P / CM) / 2 + 1, Undead nature only | the same | "−v" |
| `Curse_Atk_Def` | P / (2·CN/3), then P / (2·CM/3) + 1 | – | P / CM + 1, then P / CN | "−a/−b" |
| `CurseIni` | – | P / W + 1 | – | "−v" |
| `CurseMov` | – | step(P) | – | "−v" |

Life and Death write their two `Atk_Def` (and `Curse_Atk_Def`) numbers in opposite orders.
Then the unit's current magic direction removes groups: ToEnemy (1) removes `CureHit`,
`Atk_Def`, `BlessIni` and `BlessMov`; ToAlly (2) removes `StrikeHit`, `CurseHit`,
`Curse_Atk_Def`, `CurseIni` and `CurseMov`; direction 0 (all) keeps everything. Values 3–6
have their own masks but no ini name produces them. A zero value is not shown. The magic lines
use a yellow tint, blue when the battle power is above the level power and red when below.
**code** (49f8a0, 491d04, 491e04)

**Community lines** (after regeneration, hook c2a4be at 4924c1): a bleed line (`SBleed`, "%")
and an evasion line (`SEvasion`), both left out when 0. The bleed value is read from the battle
bleed table: for a unit of the hero's army the player-side entry of its slot, but for any other
army the player-side entry of the **next** slot instead of the enemy side, so an enemy's card
shows a player unit's bleed. The table is not cleared after a battle, so on the map the hero's
units can show the last battle's bleed until the next battle's first turn. The evasion line
reads the per-type table one entry off (the next type's value) and is always plain; with the
shipped data it is never shown. **code** (c2a4be, c2a7d8, c2a7eb, c2a837; see
community-patches.md for the bleed and evasion rules themselves)

### 9.2 Item text (499eb0)

The same builder fills the inventory's description box (4c280c) and the event dialog's item
tooltip (4cbab0). Parts, top to bottom: **code**
1. The item name, in the title font.
2. Only when the item has a description: a usage line in grey, chosen in this order (each
   choice replaces the one before): `OnlyForWarriors` for a melee weapon or a shield,
   `OnlyForBowmans` for a ranged weapon, `OnlyForMages` for a staff; `OnlyForMages` for any item
   with a `Magic` school; `NotForUse` for a Death school item. Then an empty line and the
   description.
3. The effect text, in one colour, word-wrapped as one paragraph: each entry is "label value "
   and the entries follow each other. A value of 0 adds nothing; a positive value gets "+"
   unless the entry has its own prefix.
   - **f- entries** (fixed values) have the prefix "=": Hits (label `SHit`), melee and ranged
     attack and defence, magic power (`SMagicPower`), the protections (merged into one
     `SProtectAllMagic` entry when all three are equal and not 0, else Life, Elemental, Death,
     with "%"), regeneration (`SPoison` label when negative, "%"), vampirism ("%"), initiative,
     manevres. **For a potion** the f-Hits entry is a heal instead: below 1000 it is written
     with the `SCurrentHit` label and "+v"; at 1000 or more it is the `Resurrect` label alone,
     without a number.
   - **d- entries** (added values): Hits, melee attack, ranged attack, then the defences: when
     d-melee defence equals d-ranged defence, one `SPhysicalDefence` entry (and, if the
     p-defences are also equal and not 0, their percentage right after it as "+p%");
     otherwise melee defence and ranged defence each followed by its own percentage when both
     the d- and p- value are not 0. Then magic power, initiative, manevres.
   - **p- entries** (percentages, with "%"): Hits, melee attack, ranged attack, the defences
     (one `SPhysicalDefence` entry when the two p-defences are equal **and** both d-defences
     are 0; otherwise a p-defence entry only when its d-value is 0), magic power, the
     protections (merged as above), regeneration, vampirism, initiative, manevres.
4. The item's bonus name, plain, when it has one (the Community bonus-name table; the part
   before " - ", trimmed).

The box's top margin is set so the text is centred vertically, at least 4 px. Attack values
are never merged (there is no AB = AS rule for items). **code**

Quirks of the defence merging (**code**, read from the conditions; not seen on screen):
- Equal d-defences that are not 0 with **unequal** p-defences: the p-defences are written
  nowhere.
- Unequal d-defences, one of them 0, with equal p-defences: the p-value of the defence whose
  d-value is 0 is lost.

### 9.3 Spell text and spell cards (49bde4, 49ba84, 49b63c)

**Spell card** (49bde4), used in the spell book (15 cards, 4d261c), the spell shop and the
event dialog's spell tooltip (4cbd58). An empty book slot (no spell) shows an empty frame with
a caption. Otherwise the background is the normal or the selected one, and outside battle a
spell whose mana cost (after the Community Caster rule, c27448) is above the hero's mana gets
the **cannot-afford** background instead. The 50×50 picture sits at (+6, +6); it is drawn
through a colour filter when the card is selected and affordable. The text starts at x + 108
and is centred vertically in 112 px. **code**

**Card text** (49ba84): the spell name; then, only if the spell has an effect text: an empty
line, the effect text, a cost line (`Mana` and the cost, `Reading` and the casting time with
`Hour`, both through c27448) and a duration line (`MomentaryEffect`, or `TimeOfEffect`, the
hours and `Hours`). The effect text is blue for a spell on the own army (Target 1); when the
hero cannot afford it the name, cost, duration and effect text are all in the red tint and the
effect is not turned blue. Keys are read from the `Magic` section. **code**

**Effect text** (49b63c), the same "label value " entries as items:
- DeltaFixedHits: label `CureHit` when it is 0 or more, `CurseHit` when negative.
- DeltaPercentHits, with "%": its label is chosen by the sign of **DeltaFixedHits**, not its
  own (a spell with only a negative percentage is labelled as a heal). **code**
- d- values: Hits, then attack (one `SPhysicalAttack` entry when melee equals ranged attack,
  else the two), defence (one `SPhysicalDefence` entry when equal, else the two), magic power,
  initiative, manevres.
- p- values with "%": the same, then the protections (merged into `SProtectAllMagic` when all
  three equal and not 0), regeneration (`SPoison` when negative), vampirism, initiative,
  manevres.

**Event dialog tooltips** (4cbab0, 4cbd58): the item box is 272 px wide with a 16 px margin
and as tall as its text plus 32 px; the spell tooltip is a spell card. Both are centred on the
mouse, kept 10 px inside the event window, and drop an 8 px shadow. **code**

### 9.4 Spell badges and their hint (493a64, 49ece8, 49d044)

A unit card shows up to four badges, 23 px apart, for its active spells that have a mana cost
(spells that are still running). Hovering one pushes a 420 px hint box with the spell's name,
its effect text (blue for a spell on the own army, else red), a `LifeLost` line with the unit's
life-loss percentage when the unit has one and the spell has a p-LifeLose value, and the time
left: `RemainedTimeOfEffect` and a duration, or `RemainedTimeOfEffectAll` when 40 000 minutes or
more remain. The duration (49d044) is months, days and hours joined by ", " (no year; under an
hour a fixed word). It writes the month count plus one, but the 40 000-minute cut-off means a
month (43 200 minutes) is never reached. **code**

Details (**code**, and seen under Wine on Проклятое озеро, «Укрепление Брони» on the army):
- **Which and where.** The four slots are walked in order; a slot shows a badge when its end is
  after the game time and its spell's `CostMana` is above 0, and only shown badges advance the
  position: badge n at (card + 1 + 23·n, card + 0x47), 22×22, so along the portrait's bottom
  edge. In battle a unit with 0 HP (and a hidden unpaid one) shows none; outside battle a dead
  unit's card keeps them.
- **The picture** (49ac64): the spell's 100×100 picture (its `Icon1..3` layers added, less their
  `ColorC`; `Spell-IconMask` subtracted; `Spell-Frame` laid in through `Spell-FrameAlpha`) is
  shrunk to 20×20 at (1, 1) of a 22×22 surface; `si-mask` is drawn over it with white as the
  colour key (black corners), then the `si-border` ring through its alpha. On the card it is laid
  in through `si-alpha` (the round mask); under the pointer it is **added** onto the card
  instead (brighter). The same composed surface, with the alpha of `si-alpha`, is what the
  editor writes as `Graphics/Editor/*.spi`.
- **The hint** (kind 4, 420 px): the spell's 50×50 picture on the left; the name in the title
  font; the effect text of §9.3 (49b63c) blue for `Target=Hero`, else red; then, when the unit's
  life loss (unit +0x1bf) is above 0 and the spell's `p-LifeLose` is not 0, "`LifeLost`: n %" in
  red; then "`RemainedTimeOfEffect` " and the duration (time left in hundredths of a minute,
  capped at 4 320 000, divided by 100), or `RemainedTimeOfEffectAll` from 4 000 000 on, in the
  pale yellow font. Words: `[Skills]` and `[Time]` (`cMounth`, `cDay`, `cHour`, `cLessAtHour`)
  of the interface ini; each part of the duration is "n word".

### 9.5 Card stat strip and the building panel (49462c)

Under each card of the army, building and battle grids three short lines are drawn, starting
2 px below the card. **code**

| Line | Left | Right |
|---|---|---|
| 1 | attack: "Pwr: P" for a caster (attack kind 0x11) whose melee attack is 0 or that stands outside the front slots 1–4; else "A: v" with the ranged attack when it is above the melee attack, else the melee attack | "D: m/r", melee and ranged defence plus the army's building defence, and `Row2Def` on r in a back-row slot (except on the building's side of the hire view); left out when both are 0 |
| 2 | "Mnvr: v" | "Ini: v" |
| 3 | "Hits: max" when unhurt, "Hits: hp/max" when wounded, centred | |

Colours compare the battle value with the current value (not the level value): the attack line
is blue when any of melee attack, ranged attack or magic power is above its current value, red
when any is below; manevres and initiative are red when below. The D line compares only one
defence: the melee one, or the ranged one when the row bonus applies; blue above the current
value, red below the current **or** the level value. Unhurt Hits use an orange tint, wounded
Hits red. **code**

**Who gets a strip.** In battle a dead unit, or an unpaid unit of a side that leaves unpaid
units out, shows an empty slot. In the player's army, and in battle, the strip of the hero and
of a named character is drawn on its own background panel. **code** (49462c, panels ae269c /
ae26a0)

**The building's hire view** (the hire tab passes a flag): for each of the player's units
- a wounded unit (0 < HP < max) gets the `CureArmy` button instead of the strip, in any building
  whose hire tab is shown; a dead unit in a town or church gets `ResurrectArmy`. Under the card
  the price is written "`Cost` = N" in green, centred where the Hits line would be. The button
  is enabled when the price is not above the gold (Community: an Elemental's resurrection
  compares with mana). Prices are in economy.md §2. **code** (494d63, 494e3e, 494eab, hooks
  c260c5 / c260de)
- the building's side shows each barracks slot's unit as a temporary unit of that type, with
  the same "`Cost` = N" line (Community: `MCst` and mana for an Elemental) and the `HireArmy`
  button, enabled only when the price is affordable, the hero's army has **fewer than 12 units**
  and the slot still has units. **code** (4957e9, 49571d, hooks c25e18 / c25e38 / c25e61)

### 9.6 Editing the formation outside battle (4c346c, 4c653c, 4c6f50)

Outside battle the formation is edited by clicking cards, never by dragging; actions run on the
**press**. A press is ignored while a timed action runs (busy flag 68dc63). **code**

**Army window** (press handler 4c346c, hero's army only):
- Holding an item from the pack over a unit: the item is offered to it (§9.7).
- Pressing the selected unit deselects it.
- Pressing a unit with nothing selected selects it (its card pulses).
- Pressing another unit while one is selected **swaps the two cells at once** (`Card-Move`
  plays; no slide) and clears the selection.
- Pressing an empty cell while a unit is selected queues a card slide (4b0c04, `Card-Move`,
  `round(0.7 × distance)` ms, at most 200); when it ends the unit is placed in that cell and the
  selection is cleared. Presses wait while the card slides.
- A press then switches the right side to the pack or the promotion view of the selected unit
  (498d0c): after a select, the unit's tree; after a swap or a press on an empty cell with
  nothing selected, the pack. A **deselection skips it**, and so does the end of a slide: the
  deselected (or slid) unit's promotion tree and Dismiss button stay up with nothing selected,
  until a later press switches them. **code**
- "Nothing selected" and "the hero selected" both show the pack (498d0c: unit < 2), but with
  the hero selected a press on another unit swaps it with the hero.

There is no restriction on cells: any unit, **the hero included**, can be put in any of the 12
cells, the reserve included, and any two units can swap. **code** (no unit test in 4c346c or
in the same-army end of 4b0c04)

**Building window** (4c653c hero grid, 4c6f50 garrison grid): the same select, swap and slide
rules inside one army, the hero grid in every tab that shows it (the hire tab too), each swap
and slide with `Card-Move`. Across the hero's army and the garrison a swap exchanges the two unit
records in place, and a move into an empty cell slides; the hero and named characters can
never be swapped or moved into a garrison, and an unpaid garrison unit moved to the hero first
asks for its price. Details are in economy.md §2 (garrison moves). **code**

**Grid mapping** (4927ac, 492c64). The 12 cards map to cells of the 3×W formation grid:
- W = 4: card 1–4 front columns 1–4, cards 7–10 back columns 1–4, cards 0, 5, 6, 11 the reserve
  columns 4, 1, 2, 3.
- W = 6 (wide row): cards 0–5 the six front columns, cards 7–10 back columns 2–5, card 6 reserve
  column 3, card 11 reserve column 4.
In the wide layout the six cells blocked by −1 (back columns 1 and 6, reserve columns 1, 2, 5,
6) have no card, so the screen never shows a blocked cell and the player cannot use one. A cell
holding a number above the unit count reads as empty. **code**

When a battle ends, units missing from the formation are put back (495fac) without restoring
the blocks; in the wide layout the scan tries reserve columns 4, 3, 5, 2, 6, 1, then the back
row, then the front, so with both visible reserve cells taken the unit lands in reserve column
5, which has no card: it is in the army but not shown on the army screen. **code** (the mapping
and the scan order; the case is rare and not seen)

**Hover hints of the hero's grid** (4c2f54 army window, 4c612c building window): with a unit
selected, a line naming the selected unit; over the selected unit, a line naming it; over a
unit with nothing selected, a line naming that unit (lines 0–2 of the window's ini section,
the quoted name replacing `#NAME1`). Over an empty cell the row text: `Line1` for cards 1–4,
`Line2` for 7–10, `Line3` for 0, 5, 6, 11. These card numbers are fixed, so in the wide layout
the front cards 0 and 5 are still described as reserve. With hints on the text goes into a
420 px hint box, else into the window's hint line. In the building window a hovered unit that
cannot be exchanged with the selected garrison unit (the hero or a named character) is marked
as forbidden. **code**

### 9.7 Army and hero windows: pack, promotion, item hints, resources

**Right side of the army window** (498d0c). With no unit or the hero selected it shows the
pack: 25 buttons (5×5) with the scroll bar, and the item description box at 308×252. With any
other unit selected it shows the promotion choice instead: the four tree portraits and a
272×191 description box. **code**

**Promotion portraits** (494340): portrait 0 is the unit, 1–3 its next types. When the unit's
level is 0 or its type has no next type, every portrait (the unit's own included) is locked:
turned grey (weights 100/200/100 for red, green, blue), each channel scaled by 1600/1024 and
lowered by 48 (red), 176 (green) and 256 (blue), which gives a dark brown, and darkened at
the edges by subtracting a 92×92 vignette built at start-up (squares inset by d = 0..18 px of
grey 96 − d·5334/1000: the border loses 96, the middle nothing). The "lock" is this vignette,
no picture of the install. Otherwise each portrait is plain under a glow frame. An empty
option is a plain background. The original writes no text on this view. **code**

**Pack scrolling** (49a718). The pack holds 256 slots; its length is the last used slot + 1.
The first shown slot is `5 × Round(max(0, length div 5 − 4) × s)`, where s is the scroll bar
position from 0 to 1 and Round is the FPU's round-half-to-even. So 20–24 items never scroll and
exactly 25 items already allow one row of scrolling (an empty last row). **code**

**Item hover hints** (4c280c):
- Over a pack or worn slot with nothing held: the item text (§9.2) in the description box.
- Over one of the first k worn slots of a living unit, where k is the unit's count of personal
  items (+0x18): the `ItemI` warning (the item cannot be taken off), and the unit's card is
  marked forbidden.
- Holding an item over a worn slot in the army window: the wear test (49765c); a refusal shows
  `Item<n>` for refusal code −n, or `ItemA5` for a potion (potions go onto the unit's card).
- Holding an item over a unit card (4c2f54): a refusal shows `Item<n>` (for a potion, code −5,
  the potion note `Item5` in the normal colour, the card not marked); acceptance shows line 3
  of the section naming the unit. **code**

**Hero window resource lines** (4d1814): four rows 78 px apart from y + 342, each with a frame,
an icon, the label (lines 0–3 of the `[Hero]` section) and, 20 px right of and below it, the
value: gold, mana, daily income written "+ n", daily upkeep written "- n" (a zero is written
"0"). Income is the gold of the owned towns, castles and forts (497308); upkeep is the army's
wage total. **code**

### 9.8 Building window details (4bedec, 4bb6c8, 4bb734, 4bb798, 4cd94c)

- **Info tab with quests** (4bedec): without quests the description takes the full 272 px
  height. With quests a framed list is drawn (dark 2 px and 4 px borders, decorative side
  strips tiled down, a header picture with the `QuestList` title and a drop shadow), its scroll
  bar, and the Take Quest button, shown but disabled. Selecting a row enables it (4ba754). Take
  Quest (4bb798) prepares the selected quest's event and opens its event dialog (4a8ae8,
  4ac3b4). **code**
- **An event read in the building window shows its places at once.** The event window's OK
  (Event_Finish 0x4ab1ec) queues the glide to each shown place (4af96c), its reveal (4af83c),
  the glide back and the event chain (4af658); they play over the **world screen** (the
  building window steps aside: the screen pointer is the world's, 0x674a20, while they run),
  then the scan runs and the building window comes back on the tab it was on, without a
  sound; closing it later moves nothing. Checked under Wine on РК1 (the castle's quest
  «Сообщение посыльного», lantern 2, taken in the main hall: screen event → world at the OK,
  camera y 440 → 264 in about 0.9 s, the reveal, back to 440, then the building window again
  at about 2.7 s; Frida hooks on 4af96c/4af83c fire inside that step, none after the Exit).
  Razdor: the same (the building window steps aside for the flights, input off meanwhile).
  **live**
- **Tab buttons** (4bb6c8 enter, 4bb734 leave): hovering a tab lights it (normal → lit picture)
  and repaints the left panel the first time; leaving restores every lit tab. Pressing a tab
  (4ba770) plays the click sound and shows it pressed; the release switches the tab (4ba854).
  **code**
- **Per frame** (4cd94c): sound and music upkeep, the Community FPS counter, the hint box,
  the map-music rotation; only in the hire and garrison tabs: the pulsing of highlighted cards
  of both grids and the **timeline**. So queued card slides, hires and heals only progress
  while one of those two tabs is open. Esc (with the shared latch) closes the window through
  its close button (4cd930). **code**

- **The shipyard's ship window** (built by 4d3ec0, opened by 4bbc84, drawn by 4d1314): a
  shipyard (type 9) never opens the building window. With the hero on land it opens this
  small window; at sea it opens nothing. The window is the generated 634×516 frame (the one
  of the exit dialogs) centred over the map area (195, 83 at 1024×768), titled with the
  building's name. Inside: the `S_ShipYard` picture at (5, 32); a text box at (18, 45), 598
  wide and as tall as its text plus 14 above and below, at most 193 (a longer text is
  centred in it); `[Building] AboutShipyard` with `#NAME1` replaced by the building's owner
  name and, when the gold is below `[Costs] ShipCost`, an empty line and `NoMoneyForShip` in
  red; under the box (17 px) the line "`CostShip` = ShipCost", centred with a shadow. At the
  bottom, 10 px from the edges, two `Btn3` buttons: `BuyShip` on the left, enabled only when
  ShipCost ≤ gold (fixed as it opens), and `[Buttons] Cancel` on the right; the close box at
  the top right. No attitude or owner test. Every button plays the button sound as it is
  pressed. Buying (4c60ac) closes the window, removes any old ship, switches the planner to
  the MIXED map, takes ShipCost (read again) and plays `Item-Gold`. Cancel, the close box and
  Esc (4c6118, 4cd8d0) only close it: the hero still stands in the shipyard. Its frame only
  keeps the sound and the map music going (no timeline). It opens with an event chord (§14).
  The village window (4d3a38) is a separate window of the same frame. **code**

### 9.9 After a won battle: the experience cards (4b09e8, 4b0684)

During the 2.5 s hold after a win (§12) the battle screen rebuilds every unit's stats, clears
the highlights and redraws all 24 cards; then, each frame, for each of the player's 12 cards
whose unit has a last gain above 0: the card darkened, an animated overlay strip (row
`(elapsed / 3) mod 512`), the `Expirience` label at y + 31 and "+ N" (the last gain) at y + 47,
centred, and the promotion marker at (+3, +3) when the unit has a level to spend and a next
type. When the hold ends, a dead hero is set to 1 HP before the screen closes. **code**

## 10. Hints and tooltips

**Hint boxes** (49e710, 49e9b8, 49ece8). Controls set a hint text when hovered. The box is
drawn after the window, shown only when hints are on (option 6 unticked) except for two kinds
the game always shows (the spell-target hints and pushed hints). **code**
- **Placement**: 21 px right and 37 px below the mouse (centred on the pointer for map-wide
  hints; around the held item when carrying one); flipped to the other side of the mouse when
  it would leave the owner window (or the 1024×682 view), then clamped inside it. **code**
- **Fade-in**: the spell-target hint (kind 7) appears at full opacity at once; the pushed
  hint (kind 4) fades to full opacity over 300 ms (`elapsed × 255 / 300`); all others fade to
  160/255 over 300 ms (`elapsed × 160 / 300`, with a slightly darker bottom). **code**
  (49ee4b–49ef0d)
- **Closing**: depending on the kind, when the control under the mouse changes, when a mouse
  button is pressed, or when no control is under the mouse. The same text for the same
  control is not restarted. **code**

**Hint line.** Unit grids and the battle screen also write a one-line description into a hint
label, or into the window's hint panel when hints are on. **code** (4c612c, 4c3e58)

**Panel hints** (49d5ac): the eight panel icons and the wait and centre buttons push a hint box
510 px wide owned by the bottom panel, only when hints are on; with hints off they show
nothing (there is no status-line fallback). **code** (callers 4b933e, 4b9586)

**Window hints** listed with their windows: unit grids and items §9.6–§9.7, spell badges §9.4.

**Map tooltips** are the right-button panels of §7.4.

## 11. Message boxes and text

- **Yes/No question** (4d64c8, 4c811c): 400×236, Yes on the left, No on the right. Esc
  answers No; **any other key** (except Tab, Alt and the Up and Down arrows) answers Yes: the
  key-down presses the button and the key-up releases (clicks) it. **code**
- **Event dialogs** (events subsystem): 634 px wide, height from the content, centred on the
  map view. Their text uses line breaks, colour markers, a centring marker and justified
  paragraphs with an indent. **code** (48e438, 4a9b75)
- **Text lists**: the wheel scrolls one line per wheel step; a click gives the list the
  keyboard, and the Up and Down keys then repeat every 50 ms. Scroll bars move by 0.1 of the
  range per arrow and 0.5 per page. **code** (47ede0, 47eb84, 47b8d4)

## 12. Battle screen

**Window** (4daa80): an 896×640 frame centred on the 1024×682 view, the opponent's 12 cards
on top (rows mirrored), the player's 12 below, and a red close button that asks to leave the
battle. There is no deployment step: the battle starts when the window opens, with the
formation set beforehand in the army window (§9.6). **code**

**Opening** (4d1fb0): the horn (`Global-Battle`) plays, then the battle music crossfades in
(§13). **code**

**Turns** (4c57bc). Before each action the game picks the next actor. On the player's turn
his units that may act are lit as "can act", enemies as "enemy", the actor as "current", all
24 cards accept input, and the actor's card is shown. On the AI's turn input is off, the
actor and its target are lit, and the AI's action plays at once. **code**

**Player actions are on the press** of a card (not on the release), only while input is on:
- **Enemy card** (4c43e8): the action the rules allow on that cell (melee, long strike, shot,
  hostile magic) happens immediately; then the animation plays. Nothing for a cell with no
  action. **code**
- **Own card** (4c4d4c): a move to that cell (the card slides there), the actor's own card
  (passes one action, with a 100 ms pause), or a heal or blessing on that unit. **code**
- **Space** (released) does exactly what clicking the actor's own card does: a pass, or a
  heal or blessing on itself when that is the action its own cell holds. It is not "end the
  turn". **code** (4cd2ff, 4c4f8c)
- **Esc** or the red button opens the leave-battle box (quit, main menu, restart, cancel);
  the battle is frozen under it. **code** (4cd1d0, 4d3620)
- **Hovering an enemy card** shows a hint with the unit's name and the predicted effect of the
  action (damage in hits, attack, defence, initiative and action changes, from the preview
  function 485d58). **code** (4c3e58)
- The Community F1/F2 quick loads also work in battle. **code** (c269ac)

**Animation of an action.** The rules are applied first; then, on the timeline:
1. **Slide** (4afbd8): a highlight sprite moves in a straight line from the actor's card to the
   target's, taking `round(1.8 × distance in px)` ms, capped by the Community animation speed
   at `(100 − speed%) × 5` ms. **code** (the vanilla cap is unknown, see Unknowns)
2. **Effect** (4afe7c): the sound plays, and a 25-frame effect (220×110, drawn at double
   height) plays over the target with frame `elapsed × 24 / 350`, for 350 ms in the base game
   (inferred from the frame formula) or `(100 − speed%) × 3` ms with the Community patch.
   **code**
3. A **counter** or first strike adds a slide back and an effect on the attacker (same
   picture and sound as the action); a **DeathCurse or Ghost** death of the killer adds a
   sorcery effect on the attacker. **code**
4. A **pass** is a 100 ms pause with the busy pointer. A **move** is a card slide of
   `round(0.7 × distance in px)` ms, at most 200 ms, with `Card-Move`. **code** (4afb54,
   4b0284, factor at 4b059c)
5. When an effect that ends an action finishes, the sides are written back and the next actor
   is chosen. In the plain case and the DeathCurse/Ghost case only the last effect does this
   (the first one is flagged to just refresh the cards). In the **counter** case both the
   effect on the target and the counter effect on the attacker are queued without that flag,
   so the next-actor step runs once when the target's effect ends (before the slide back
   plays) and again when the counter effect ends. **code** (4c4611–4c4683, 4b00f8–4b0135;
   what the second run does to the turn order is not traced, see Unknowns)

Effects and sounds by action:

| Action | Effect picture | Sound |
|---|---|---|
| Melee, long strike | melee effect | `Battle-Fight` |
| Shot | shot effect | `Battle-Shoot` |
| Shot by a unit whose range ≥ `ShotWeaponRange` | melee effect | `Battle-Strike` |
| Curse or magic strike | magic effect | `Battle-Sorcery` |
| Heal (target wounded) | heal effect | `Battle-Cure` |
| Blessing | bless effect | `Battle-Bless` |

After a hostile magic effect the target's card shows a curse badge; after a blessing a
blessing badge. **code** (4b00c1)

**End.** After a win and the experience share, the triumph music starts its crossfade at
once, the battle screen stays up for 2.5 s with the busy pointer, then closes, and 250 ms
later the chained event step (the result report) runs. After a defeat the battle screen
closes at once, the defeat music starts and the defeat report follows after 250 ms. **code**
(4c56a8: the music call comes right after queueing the hold and the 250 ms chain; 4c575b,
4b09e8, 4af658)

## 13. Music

**Crossfades** (49d774). Changing the music fades the old track out and the new one in over
2000 ms, both linear in the volume scale. A track fading in from silence starts from its
beginning and loops until it is changed; a track faded to silence stops. **code** (4819cc,
481790)

**Which track** (callers of 49d774):

| Moment | Track |
|---|---|
| Program start (no fade), back to the menu, credits closed | `BkgMenuMain` |
| Credits | `BkgAuthors` |
| New map, loaded save, restart, next campaign map | `BkgMap2` (always), next change 90–122.8 s later (see the note on `rand` below) |
| Battle against a building's garrison | `BkgBattle1` |
| Battle against an army | `BkgBattle2` |
| Battle won | `BkgTriumph`, started at the win, looping until the report is closed, then a random map track |
| Battle lost, or the army wiped out by a spell | `BkgDefeat` |

**Rotation on the map** (49d7f8). When the change time has passed, a new map track is picked
by the per-frame handler of the world map, the army, hero, spell book, building, village and
shipyard windows, the exit menu and the save window, and, only in a game, the options and
load windows and the question box (the question box also not in battle). Only the world map
skips the change while the triumph or the defeat theme plays; the window handlers do not
check it. The battle screen never changes the music. (13 call sites of 49d7f8; the
Triumph/Defeat test exists only at 4cc1e0.) The pick is uniform among eight tracks, redrawn
until it differs from the previous pick (a stored index, set to 1 when `BkgMap2` is started
by a map start; battle and menu themes do not change it), using the game's own random
generator (4832fc). The eight are the seven `BkgMap` tracks **and** `BkgAuthors`. The
crossfade then takes 4000 ms. Time until the next change, by pick: **code**

| Pick | Track | As written in the code | Actual next change |
|---|---|---|---|
| 0 | BkgMap1 | 40 s + rand(50 s) | 40–72.8 s |
| 1 | BkgMap2 | 90 s + rand(90 s) | 90–122.8 s |
| 2 | BkgMap3 | 40 s + rand(50 s) | 40–72.8 s |
| 3 | BkgAuthors | 90 s + rand(90 s) | 90–122.8 s |
| 4 | BkgMap4 | 90 s + rand(90 s) | 90–122.8 s |
| 5 | BkgMap5 | 80 s | 80 s |
| 6 | BkgMap6 | 60 s + rand(60 s) | 60–92.8 s |
| 7 | BkgMap7 | 40 s + rand(50 s) | 40–72.8 s |

The code draws `rand(50000)`, `rand(90000)` and `rand(60000)` milliseconds. The game's
generator returns a 15-bit value (0..32767) taken modulo n, so for any n above 32768 the draw
is simply 0..32767 ms, uniform, which gives the last column. Because the music draws from the
game's generator, the music changes shift every later random roll of the game. **code**
(4832fc: seed × 0x343FD + 0x269EC3, result (seed >> 16) & 0x7FFF mod n)

## 14. Sound effects

All effects restart if already playing and do not loop, unless noted. **code** (call sites
listed in the notes)

| Sound key | When |
|---|---|
| `InterfaceButtonDown` | press of almost every standard button; press of the wait and centre buttons |
| `InterfacePanelDown` | press of one of the eight bottom panel icons (only there) |
| `InterfaceCastSpell` | casting from the spell book (when the mana suffices); switching a building window tab; switching a load window tab |
| `InterfaceBarScroll` | test sound while an options slider moves (stopped 250 ms after the last move) |
| `MainMenuSelect-1` | hovering any main-menu item (§4; keys `-2` and `-3` are never read) |
| `MainMenuPress` | pressing a main-menu item; choosing a hero class |
| `Global-Event-1..3` | opening an event dialog, the village window or the shipyard window: one of the three at random (game generator) |
| `Global-Battle` | the battle screen opens |
| `Card-Move` | a card slides (battle move, army exchange), moves in the building and army grids |
| `Unit-Upgrade` | the promotion screen |
| `Spell-Good` / `Spell-Evil` | a world spell lands on the player's army / on another army |
| `Battle-*` | battle effects (§12); `Battle-Cure` also for healing or resurrecting in a building |
| `Item-<type>` | an item is picked up, dropped or worn (by its type; type Item uses `Item-Item`) |
| `Item-Gold` | press of the market trade button, the hire buttons, an event dialog button, taking a village's tribute, buying a ship; a hire button plays it again in its click action, on the release (4c7370 press, 4c7380 click, both restarting the one buffer) |

`Battle-Parry` is loaded by the Community patch but the shipped sound ini has no entry for
it. **code**/**data**

## 15. Animation and timing reference

| Thing | Timing | Evidence |
|---|---|---|
| Army idle on land | 20-step sequence, 250 ms per step | 4c868c |
| Army on a ship | 8 frames, 200 ms each | 4c868c |
| Hero walking frame | 4 frames by step tick; ship (ms/100) & 7 | 4ad314 |
| Selection ring | 8 frames of 100 ms, turning 5.625° each | renderer |
| Water | frame (ms/100) mod 32 | renderer |
| Tree sway | triangle wave ±2.5 px, 51 steps of 70 ms | renderer |
| Route arrows | brightness 150–240 running along the route, (ms/10 + 18k) mod 181 | renderer |
| Pointer animations | §7.2 | 48eb98 |
| Card pulse | (ms/100) mod 15 triangle, levels 8–15 | 4cd1fc |
| Map tooltip fade-in | 500 ms | 4ca728 |
| Hint fade-in | 300 ms (none for the spell-target hint) | 49ece8 |
| Camera glide | 900 ms, cosine ease | 4af96c |
| Menu background | 150 ms and 100 ms frames | 4c7774 |
| Credits scroll | 35 px/s | 4c82c4 |
| Loading bar | at least 4 s | 48c844 |
| Save progress bar | at least 1.5 s | 4b648c |
| Map start / load | at least 500 ms | 4c1000, 4c01fc |
| Battle slide | 1.8 ms per px (capped) | 4afbd8 |
| Battle effect | 350 ms base, 25 frames | 4afe7c |
| Battle pass / card slide | 100 ms / 0.7 ms per px, at most 200 ms | 4afb54, 4b0284 |
| Battle won, hold | 2500 ms, then 250 ms to the report | 4c56a8 |
| Promotion screen ease | piecewise slopes 1, 2, 3, 2, 1 | 4b1918 |
| Walk and wait pacing | WalkDelay per step or 30-minute tick (world.md) | 4b8c4f |

## 16. Options (`[Options]` of the interface ini)

A checkbox is on when its value is 1. **code** (4b8974, 4bf4d8)

| Key | Meaning | Note |
|---|---|---|
| `OptValue4` | no music and sounds | needs a restart |
| `OptValue5` | show the FPS counter | Community counter: frames × 1000 / ms, max 999, reset every 5 s |
| `OptValue6` | hints **off** | hints are on when the value is not 1 |
| `OptValue7` | pursue the targeted army automatically | §7.3 |
| `OptValue8` | autosave | |
| `OptValue9` | improved enemy battle AI | battle.md |
| `OptValue10` | impossible difficulty | income factor 100 instead of 120; needs a restart |
| `OptValue11` | wide front row | 6 columns instead of 4; needs a restart |
| `MusicVolume`, `SoundVolume` | 0–100 | |
| `ScrollSpeed` | 0–100 | §7.6 |
| `WalkSpeed` | 0–100 | WalkDelay = (100 − w) × 250 / 100 + 150 ms, integer division (w = 1 gives 397, not 397.5) |
| `AnimationSpeed` | 0–100 (Community) | battle animation caps, §12 |

Water animation and high-resolution textures are always on. **code** (4b8a7c)

## 17. Map editor

The game has no way to start the map editor; it is a separate program. **code** (no launch
call in the game code; the process functions found are library code)

---

## Razdor now → original

Razdor files: `src/ui/world_view.rs`, `hotkeys.rs`, `main_menu.rs`, `game_bar.rs`,
`minimap.rs`, `battle_view.rs`, `audio.rs`, `jukebox.rs`, `dialog.rs`, `saves.rs`,
`screens.rs`. Rows marked "extra" are Razdor features the original does not have; per the
parity rule they are candidates to hide or remove, not bugs to copy.

| # | Topic | Razdor now | Original | § |
|---|---|---|---|---|
| 1 | Display | Resizable window, interface scaled from the 960×720 footage | Full screen 1024×768, 16-bit, fixed layout | 1 |
| 2 | Click to walk | Matches: first click shows the route, a second click on the same spot walks; a planned cell whose route was dropped (a click out of reach, a wait) stays planned and its clicks do nothing (`world_view::plan_click`) | The original does the same two-click walk (no double click); the comment is wrong | 7.3 |
| 3 | Stop walking | Matches: left click or any key held; the step under way is finished (`Game::cut_walk`); no other input while he walks or waits | Left click or any key; the hero stops at the end of the current step | 7.3 |
| 4 | Right button | Matches: held, the tooltip of the army or building under it; never a command (the pointer is not frozen: presentation, left out) | Held: tooltip of the army or building under it, pointer frozen; never a command | 7.4 |
| 5 | Map tooltips | Only while the right button is held (left up), on the idle map; no fade-in (presentation, left out) | Only while the right button is held, 500 ms fade-in | 7.4 |
| 6 | Pointer | System pointer | Own pointers: arrow, ask, denied, swords, house, clock, with animated frames | 7.2 |
| 7 | Edge scroll | Matches: 5 px margin (scaled with the screen), `round(dt / F)` and `round(dt × 0.6875 / F)` original px per frame, dt the whole ms since the map's last frame, `ScrollSpeed` from the install | 5 px margin, `dt / F` px per ms with F from ScrollSpeed (62.5 cells/s at 100) | 7.6 |
| 8 | Arrow keys | Matches: the held arrow scrolls (only the last key down counts) | Held arrow scrolls | 7.6 |
| 9 | Zoom | Wheel and +/− (extra) | None | 7.7 |
| 10 | Camera while walking | Locked on the hero while he walks (centred on him; the original's off-centre placement is presentation, left out) | Locked on the hero (his cell at column 14, row 16) while he walks | 8 |
| 11 | Centre on hero | Matches: the centre button over the message box (`game_bar::TimeButton::ShowHero`, the install's art and `cp_ShowHero` hint), a 900 ms glide on the original's rounded cosine (`world_view::glide_ease`) during which the map takes no input; Tab does the same when the view is off the hero (extra key) | Centre button on the message box, 900 ms cosine glide | 6, 8 |
| 12 | Shown places | 0.8 s pan, 1.2 s fade, 0.4 s rest, smoothstep; Matches: an event read in a building window flies at its OK over the map, then the window comes back | 900 ms cosine glide, only when farther than 300 px; in a building window at the OK, over the world screen, then the window again | 8, 9.8 |
| 13 | Waiting | Matches: the 1 h and 4 h buttons appear over the message box when it is hovered on the idle map, its text hidden, with their art, `cp_Wait1Hour` / `cp_Wait4Hour` hints and the button sound (`game_bar::time_button_at`); F4 endless wait, F5 ends it (`Game::begin_endless_wait`; the minutes of the tick under way are dropped). Extras: keys 1 and 4, time panel left / right click off the buttons; a left click anywhere or a key press during any wait ends it after the half hour under way and does nothing else (`Game::cut_wait`, the user's wish) | Two buttons that appear over the message box (1 h, 4 h); Community F4 endless wait, F5 ends it; nothing else stops a wait | 6, 7.7 |
| 14 | Minimap | M or the panel button opens a minimap window | Overlay in the top right corner, 200 or 400 px, toggled by the panel button and saved; left-drag on it moves the view | 6, 7.6 |
| 15 | Hotkeys | F1 key list, F2 language, F5 quick save (not during the endless wait), F9 quick load, N music, letters for windows (extras); F4 / F5 endless wait as the original | Community F1 newest autosave, F2 newest own save, F3 save, F4/F5 endless wait; no letters | 7.7 |
| 16 | Panel icon order | Left Menu, Settings, Save, Load; right Journal, Squad, Spells, Map | Left from the centre: Save, Load, Options, Exit menu; right from the centre: Hero, Army, Spell book, Minimap | 6 |
| 17 | Esc on the map | Matches: opens the game menu (Razdor has no spell targeting on the map) | Leaves spell targeting, else opens the exit menu | 7.7 |
| 18 | Esc in the main menu | Matches: quits at once | Quits the game at once, without a question | 4 |
| 19 | Hero choice Back / Esc | Matches: back to the main menu | Back to the main menu | 5 |
| 20 | Yes/No boxes | Matches: Esc = No, any other key except Tab, Alt, Up, Down = Yes (on the key's press, not its release) | Esc = No, any other key except Tab, Alt, Up, Down = Yes | 11 |
| 21 | Battle deployment | Matches: none, the battle starts when the window opens (`BattleView::new`) | None: the battle starts when the window opens | 12 |
| 22 | Space in battle | Matches: as a click on the actor's own card (a pass, or a spell on itself) | Same as clicking the actor's own card: one pass (100 ms) or a self heal/bless | 12 |
| 23 | Quick battle (Q) | Plays the battle under way out at once (extra the user asked for; kept) | None | 12 |
| 24 | Battle input timing | Matches: acts on the press of a card (the hover hint is presentation) | Acts on the press of a card; hover hint predicts the effect | 12 |
| 25 | AI pacing | 0.45 s delay before each AI action | No delay: the AI acts as soon as the previous animation ends | 12 |
| 26 | Strike animation |  0.7 s lunge and effect; Matches: a counterblow or preventive strike adds the target's lunge back and the effect and sound on the actor, a DeathCurse death of the killer the sorcery on it; pass 0.1 s; moves 0.25 s  | Slide 1.8 ms per px (capped) + 350 ms effect; counters add a second slide and effect; pass 100 ms; card slide ≤ 200 ms | 12 |
| 27 | Battle end |  Matches: a win holds the screen 2.5 s with the experience on the cards, then the screen closes and the report follows 250 ms later (`Dialog::not_before`); a defeat or a battle nobody won shows Razdor's result box  | 2.5 s hold with the busy pointer, then the screen closes and the report follows 250 ms later | 12 |
| 28 | Music crossfade | No crossfades (presentation, left out); tracks loop until changed | 2000 ms crossfade on screen changes, 4000 ms in the rotation; tracks loop | 13 |
| 29 | Map music order | Matches: `rules::music::rotate` with the game's generator, timed per pick; checked by the map and its windows, the map waiting while the triumph plays | Uniform pick among eight (the seven map tracks and `BkgAuthors`), never the previous pick, changed on a timer per track (40–122.8 s, see §13) | 13 |
| 30 | First map track | Matches: `BkgMap2`, its first change from the load's draw (`Game::take_music_wait`) | Always `BkgMap2` | 13 |
| 31 | Battle music | Matches: `BkgBattle1` against a garrison, `BkgBattle2` against an army | `BkgBattle1` against a garrison, `BkgBattle2` against an army | 13 |
| 32 | Triumph | Matches: starts with the result, loops, the world map waits for it; closing a dialog draws the next map track | Starts at the win (during the 2.5 s hold), loops until the report is closed, then a random map track; the world map does not change it meanwhile | 13 |
| 33 | `BkgAuthors` | Matches: the credits theme and pick 3 of the rotation | The credits theme, and part of the map rotation | 4, 13 |
| 34 | Event chord | Matches: `Random(3)` of the game's generator (`Game::event_chord`) for event dialogs and the village and shipyard windows, drawn when the dialog comes up (several dialogs of one moment draw after that moment's rolls) | One of three at random (game generator), also for the village and shipyard windows | 14 |
| 35 | `InterfacePanelDown` |  Matches: the press of a bar icon; the side windows open silent (Razdor's own non-event dialogs still play it)  | Only the press of a bottom panel icon | 14 |
| 36 | `MainMenuPress` |  Matches: pressing a main-menu item; a class portrait that changes the class (Next and Start: the button sound)  | Pressing a main-menu item; choosing a class | 14 |
| 37 | Hover bells |  Matches: `MainMenuSelect-1` as the pointer comes onto an item  | Main-menu hover: the same `MainMenuSelect-1` sound for every item | 4, 14 |
| 38 | `InterfaceCastSpell` |  Matches: world spell cast, the building window's opening and tab switches (Razdor's load window has no tabs)  | Spell book cast, building tab switch, load window tab switch | 14 |
| 39 | `InterfaceBarScroll` | Unused | Options slider test sound | 14 |
| 40 | `Item-Gold` |  Matches: the money buttons (trade, hire, heal, learn), the ship window's Buy after its button sound, the village tribute as its window closes; a hire plays it on the press and restarts it on the release, as the original's two calls  | Presses of money buttons (trade, hire), event dialog button, village tribute, ship purchase | 14 |
| 41 | Random generator | Matches: the music picks and the event chord draw from the game's generator | Music picks and the event chord use the game's generator | 13, 14 |
| 42 | Hints | Razdor tooltips at once | Hint boxes with a 300 ms fade, flip-and-clamp placement, off when option 6 is ticked | 10 |
| 43 | Options window | Music and sound volume, battle AI, the front row's width for new games (6 or 4: a switch for `OptValue11`, which stays the default until chosen; a save keeps its own width) | Five sliders and eight checkboxes; slider test sound | 16 |
| 44 | Loading | Razdor's own start-up | Logo slides, a loading bar that takes at least 4 s, first-run sound conversion | 3 |
| 45 | Info card colours | Compares the value with its start-of-battle value (blue above, red below) | Compares the shown value with the level value (no items): blue above, red below the level **or** the current value | 9.1 |
| 46 | Info card values | "base + bonus" split for what items add; "v + n" for building defence on both defences and `Row2Def` on ranged defence in the back row | Only the shown value; "v + n" for the building defence on both defences and `Row2Def` on ranged defence in the back row | 9.1 |
| 47 | Info card magic | Own formula set (strike and curses for ToEnemy, heal and blessings for ToAlly; Death heals 0) | Ten lines from 49f8a0 by school, nature and direction (Death heals for Undead, Elemental and Hero natures; Life also strikes and curses; Life and Death write the two Atk/Def numbers in opposite orders) | 9.1 |
| 48 | Info card wage | Shown when the wage is above 0 | Recruit wage of the type's Cost; hidden for hero classes, named characters, wage kind 3 and Undead | 9.1 |
| 49 | Card stat strip | Labels match: "Pwr:" for a caster (attack kind 0x11) with no melee attack or outside the places 1–4, else "A:" with the larger attack (`unit_sheet::attack_piece`); the attack line blue when any of the three attacks is above, red when any is below; `Row2Def` on r in the back row as the original; colours against the start-of-battle value (none outside battle); D colour by the sum of both defences | "Pwr:" for a caster outside the front or with no melee, else "A:" with the larger attack; colours against the current value; D colour from one defence only | 9.5 |
| 50 | Formation editing | Matches: presses as the original's, in the army window (`items_view::ArmySel`, `unit_drag::grid_press`; the deselection and the slide's end leave the right side as it was), the hire tab's hero grid and the garrison grids (a refused hero or named unit keeps the selection), `Card-Move` on every swap and slide, the army window's and hire tab's slides timed as 0x4b0c04. Extra: a card can also be dragged onto another cell (a press that swaps starts no drag) | Click to select, click another unit to swap at once, click an empty cell to slide there; acts on the press; the hero may go anywhere | 9.6 |
| 51 | Item text | Kind name, then f-, d-, p- values and the bonus, comma-joined | Name, usage line by type and school, description, then "label value" entries with "=" (f-), "+" (d-), "%" (p-), potion heal/revive rules, defence merging with its lost-value quirks, bonus name | 9.2 |
| 52 | Spell text | Effect summary, school, mana and casting time, duration and target | Name, effect entries (heal/curse label by the sign of DeltaFixedHits, AB = AS and DB = DS merged), `Mana`/`Reading`, duration; red when unaffordable, blue for own-army spells | 9.3 |
| 53 | Pack scrolling | Wheel by one row over the whole 256-slot pack | Scroll bar over `max(0, length div 5 − 4)` rows of the used part, rounded | 9.7 |
| 54 | Army window right side | Matches: the pack with nothing or the hero selected, the promotion tree for any other unit, every portrait locked as the original's (grey, dark brown, vignette: `chrome::lock_portrait`) when the unit is at its first level or of a final class, no note under it; the "Lv" labels and the arrows' colours are Razdor's | Pack when nothing or the hero is selected, the promotion tree for any other unit (every portrait greyed, browned and vignetted when it cannot be promoted) | 9.7 |
| 55 | Panel icon hover | (see row 42) | A hint box with the icon's name only when hints are on; nothing otherwise | 6, 10 |
| 56 | Spell badges | Matches: up to four badges along the portrait's bottom on the army, building and battle cards, from the units' slots (running spells with a mana cost, slot order), composed from the install's art (a coloured disc without one), added on hover; the hint with the picture, name, effect text (49b63c), `LifeLost` line and time left with the original's words and quirks (`spell_hint`, `ui::spell_badges`); the box is Razdor's parchment, flipped and clamped to the screen | Four 22 px badges 23 px apart at card + 0x47; 420 px hint box | 9.4 |
| 57 | Battle card signs | Matches: potion and blessing from the top left, poison and curse from the top right, 23 px apart; the curse and blessing signs set as a magic or a blessing effect ends on the card and kept to the battle's end (the turn order number, Razdor's, moved to the bottom right). The army and building cards show the promotion (the hero's army) and then the potion sign from the top left in the same places (`chrome::card_signs`; the hero's helm, Razdor's, takes the first place); their payment sign is still Razdor's and they show no poison sign yet | `Sign-*` badges by the unit's potion, +0xc9, regeneration < 0, +0xc5; outside battle (493a64) also `Sign-Upgrade` first from the left (the hero's army, a promotion to take) and `Sign-Payment` first from the right (unpaid), poison on any card | 12 |
| 58 | Shipyard window | Matches: a shipyard opens the original's small ship window on land and nothing at sea (no main hall, no other tab): its picture, the install's `AboutShipyard` text with the owner's name, `NoMoneyForShip` when the gold is short, the price line, "Нанять корабль" (enabled iff ShipCost ≤ gold) and "Отмена"; Buy closes it (`building_view::ship_window`, `Game::window_at`). The text box's font is Razdor's, so its lines wrap a little differently | The ship window on land, nothing at sea; Buy closes it (4bbc84, 4d3ec0, 4c60ac) | 9.8 |

## Unknowns

- **Vanilla battle animation caps.** The Community animation-speed hooks overwrote the base
  game's compare values for the slide length and the effect length. The effect length of
  350 ms is inferred from the frame formula; the slide cap is not known (500 ms would match
  the Community formula at speed 0).
- **Walk camera offset.** The exact sub-cell offset used while the hero moves between cells
  (floats near 4af2ec) is not decoded; the view follows the sprite.
- **Community F3.** What name the quick save uses when the save window was never opened is
  not traced (an empty name reopens the save list).
- **Exit menu frame** (4c8270) was read only up to its music rotation check (it does rotate
  the map music); its Esc is assumed to close it like the other windows.
- **Counter double step.** In the counter case the next-actor step (4c57bc) runs twice per
  action (§12). It has no guard, so on an AI turn the second run may let a further actor act
  before the queued animations have played; this was not traced through 489ca0.
- **Volume curve.** How the 0–100 sliders map to the sound device's volume (hidden in FPU
  code at 481554).
- **Hint box layout.** The font and inner layout of hint boxes, and which controls use which
  hint kind, were not mapped one by one.
- **Pointer hotspots.** The six pointer records have offsets (0,0), (−27,−27), (−21,−21),
  (−21,−21), (−24,−24) and (−20,−20) in build order; which offset belongs to which animated
  pointer is only partly checked.
- **Event-window chord.** The three window open handlers that play the chord (4d128a,
  4d1567, 4d1666) were assigned to the village, shipyard and event windows by their address
  ranges, not by tracing.
