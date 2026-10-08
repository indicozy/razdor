# Discord Times: the scenario event engine (from the Community exe)

This file specifies the scripting engine of the Community Update (Unstable) `DiscordTimes.exe`
(Delphi, image base 0x400000): when events are checked, their time windows and conditions,
every result and the order it is applied in, the question dialog, flags, quests and rumours,
text placeholders, victory and defeat, the tutorial hooks and the campaign hand-over. The
rules are in our own words; addresses are virtual addresses in that build, given as evidence.
The record layout itself is in [../dtm-format.md](../dtm-format.md) §9; offsets below are
file offsets of the 171-byte event record (the exe keeps the record in memory with the same
layout, so file offset = memory offset).

Confidence tags:
- **code**: confirmed by reading the code.
- **data**: consistent with the shipped maps, help or footage, but not traced in code.
- **unknown**: not determined; Razdor keeps a documented guess.

The event engine used to be summarised in [economy.md](economy.md) §6; that section now only
points here, and this file is the reference for every rule below.

## 0. Conventions

- **Time.** "now" is the game clock in whole minutes: the centi-minute counter divided by 100
  plus the scenario's start offset (0x4a7b9b). A day is 1440 minutes. **code**
- **Ids.** Events, armies, buildings and points are 1-based in the file and in every
  cross-reference; the exe stores event *n* at index *n*−1. **code**
- **Runtime state lives in the record.** The record is loaded byte for byte (0x4b2504), and
  three of its "unknown" file bytes are the engine's state: 156–159 *last fired* (minutes),
  160–161 *times fired*, 162 *answer* (1 = the last response was No). Shipped maps have them
  at 0, except one map whose events carry *last fired* = 1 (harmless). Two editor bytes are
  also changed at run time: 76 *ask* and 141 *once* (§6). **code**
- **Strings.** Every string of the map has runs of two spaces collapsed to one at load
  (0x4b2aa1). Building and army descriptions get `#HERONAME` filled at load; event texts get it
  filled each time they are shown (§6.4). **code**
- **No randomness.** The vanilla event engine draws no random numbers of its own. The only roll
  in normal play is the Community opcode 18 (§15), which uses the Community generator
  (0xc28dae). The gold routine that event gold results go through (0x4ab150) can also roll,
  but only in its anti-cheat branch when the gold seal is broken ([engine.md](engine.md)).
  **code**

## 1. When the engine runs

The scan (0x4abfbc, §2) is started from these places. **code**
- **Map start**: Map_Load starts a 250 ms timer (0x4af658 with no event), which runs the scan.
- **Each hero step**, once the step is complete and no battle is running (0x4aed3a). Before
  the scan, the event point under the hero is noted (0x4aed35).
- **Each tick of a wait or of an event delay** (30 game minutes per tick, 0x4ae4e9).
- **Contact with an army** (0x4adcde, 0x4adff5, 0x4ae1dc): the scan runs with that army as the
  *met army* (§9). If an event fires, the battle the contact would start does not happen.
- **After a dialog is finished** (OK, 0x4abea0), **after a No** (0x4c2452, 0x4c2480), after a
  chained event's timer with nothing to chain (0x4af6c9), after the village chooser (0x4bbd2f).
- **When a window over the map closes** (0x4b8d28(0), not when another window replaces it)
  after the player healed, raised or traded in the building window (flag 0x4ed440, set by
  heal and raise at 0x4b13cb and 0x4b14dc and by a purchase or a sale at 0x4b9fd9; checked
  at 0x4b8f63; a hire does not set it). The flag is cleared when a scan goes idle (0x4ac3a6).
  Razdor: the same (`Game::window_closed`).

## 2. The scan

**Order of candidates** (0x4abfbc). The scan is a loop; one pass is: **code**
1. **Global events** (type 1) in file order; the first that passes is taken.
2. If none: the **event point** under the hero, its listed events in list order, **whatever
   their type** (global, local, quest or rumour).
3. If neither: the **building** the hero stands in, its listed events in list order, but only
   when that building is not the one the hero was already in when his current move began
   (0x4ed434, written at the start of a move 0x4b84f4, at a map click 0x4cc42b and by the
   village chooser 0x4bbdfe). Only **local events (type 2)** count, except in **villages
   (building type 2) and shipyards (type 9)**, where every listed event counts. Quests and
   rumours of other buildings are not fired by the scan; they are offered in the main hall
   (§10).

"Passes" means the full check of §3 and §4 with *apply* on. An event that passes is opened
(0x4a8ae8). Then:
- If it is **silent** (no question text, no message text, no spell, no battle, no lanterns, no
  shown army, no delay: 0x4a7a40), it is finished at once (§7) and the loop starts again from
  step 1. A run of silent events is therefore applied in one go.
- Otherwise the loop stops; the dialog (or, for an event with no texts but with a spell,
  lanterns, a shown army, a battle or a delay, a direct finish) takes over, and the scan is
  started again when it ends.

So the events after a shown window, a silent one too, are not even checked until that window
is closed: its OK finishes it (0x4c206c → 0x4ab1ec), then its chain runs or the scan starts
again, finding the next event, whose own window waits for the next OK. Checked live (Frida on
0x4abfbc, 0x4a8ae8, 0x4ab1ec; РК1, the hero entering the church at (47,27)): the scan opens
event 9 (shown) and returns; OK finishes 9 and the new scan opens 10 (its chord); OK finishes
10 and the new scan opens and finishes 19 at once (silent, it activates army 2: its wander
points' draws come only now, and the stop's snap before them did not count it; FINDINGS.md
§22). A building's local events stay candidates through these rescans (the entry mark is
cleared only when the scan goes idle).

**Noon.** If no event was taken in the pass, now ≥ the next noon and no world spell is being
cast, the scan does the noon processing (Ranger healing, wages, the noon report; see
[economy.md](economy.md)). **code**

**When the scan goes idle** (nothing taken, no noon report): every event whose *last fired*
lies in the future is set back to now (0x4ac369), the met army is cleared (0x4ac3a0), and the
rescan-on-close flag is cleared. After the dialog machinery is idle, the "meeting event
waiting" marks are recomputed (§9). **code**

## 3. Eligibility: once, guard, window, class

Checked first, in this order (0x4a7b80). **code**
1. **Done** (byte 140, the editor's "subordinate event" box): never eligible. The engine never
   clears it, so such an event runs only as a chain (§8).
2. **Firing guard** on *last fired* L:
   - duration ≠ 0: eligible when L ≤ now;
   - duration = 0: eligible when L < now − 60.
   Firing sets L = now + 1 (0xc28710), so an event cannot fire twice in one scan; the idle reset
   of §2 sets L back to now, so a repeatable event with a duration can fire again on the next
   scan, even in the same minute.
3. A negative start is set to 0. If **start > now**: not eligible (this is how events waiting
   for a relative start, §8, stay closed; the editor's "relative only" start is year 2000).
4. **Window.** Let D = max(duration, 1) in **hours** (the stored number is taken as hours; the
   editor stores hours × 60, so an editor window of 24 hours lasts 1440 hours), k = (now −
   start) div 1440 and day = start + 1440·k. The event is open when any of these holds:
   - repeat R ≠ 0, k mod (R div 1440) = 0 and day ≤ now ≤ day + 60·D;
   - R = 0 and now ≤ start + 60·D;
   - R = 0, duration = 0 and times fired = 0 (open with no end until it first fires).
   A repeat of 1–1439 minutes divides by zero (R div 1440 = 0); the editor cannot produce one
   and no shipped map has one. Repeats are whole days (R is truncated to days).
5. **Once** (byte 141 ≠ 0) and times fired ≠ 0: closed.
6. **Class** (byte 10): 0 any; otherwise it must equal the hero's class + 1 (1 knight,
   2 archmage, 3 ranger).

Consequence: a duration-0 event without repeat, after its first firing, is open only until
start + 60 minutes while the guard needs 61 minutes since that firing, so in practice it fires
once. **code**

## 4. Conditions

All conditions must hold. They are evaluated in this order (0x4a7b80 and its continuations
0x4a80a7 … 0x4a8237 and the Community hooks between them). **code** unless noted.

| # | Condition (file byte) | Rule |
|---|---|---|
| 1 | Defeated by the player (53; armies 54, 55) | Each listed army's "beaten by" mark is the player (1). |
| 2 | Happened with Yes (56; events 57, 59) | Each listed event has times fired > 0 and answer ≠ 1. |
| 3 | Happened with No (69; events 70, 72) | Each listed event has answer ≠ 0 (only a Community poke can store a value other than 0 and 1; such a value passes both this test and condition 2). |
| 4 | Not happened (61; events 62, 64) | Each listed event has times fired = 0. |
| 5 | Army active (75) | The army is on the map and not destroyed. |
| 6 | Army inactive (15) | The army is off the map **and not destroyed**. A destroyed army is neither active nor inactive. |
| 7 | Beaten by anyone (66; armies 67, 68) | Each listed army is destroyed, or has a "beaten by" mark. |
| 8 | Meet army (74) | The army is the one being met right now (§9). |
| 9 | Hero at 1 HP (145) | **The hero's HP is exactly 1.** This is a condition, not a result: the exe never sets HP from this byte. A hero is left at 1 HP when he falls while some of his army survives ([battle.md](battle.md), "Hero 1 HP"). |
| 10 | Army in its home (146) | If the army has a home building, it must stand in it (its current building = its home); an army without a home passes. |
| 11 | Numeric conditions, **only when "current stats" (18) is set** and, in the Community build, either byte 17 (patrol delta) is 0 or the squad value (11) is ≤ 0 (0xc2698c); otherwise all five are skipped | Level (19) vs the hero's 0-based level; gold (21); mana (25); squads (11) vs the number of unit records in the hero's army (hero and dead units included); strength (13) vs the army's strength total (army record +0x1648). Each value is signed: v > 0 needs ≥ v, v < 0 needs ≤ −v, 0 is no check (0x4a7b40). The extra Community gate also hits plain events that change a patrol and ask for a squad count; no shipped event combines byte 18, a patrol delta and a positive squad value. |
| 12 | Building owners (29; ids 30–32, codes 33–35) | Per slot with both id and code ≠ 0: code 1 the building's owner is the player; 6 it is not; 2–5 the building's **faction** is code − 1 (whoever holds it). The faction test is made for every code, so code 1 also passes on faction 0 and code 6 on faction 5; shipped buildings have factions 1–4 only, so this never shows. A slot with code 0 is skipped. |
| 13 | Named squads (36; types 37–39, names 40–42, codes 43–45) | Per slot with type ≠ 0, see below. |
| 14 | Artifacts (46; ids 47–49, codes 50–52) | Per slot with id ≠ 0, see below. |
| 15 | Flag test (title script) | See §5. |

Lists of armies or events skip zero entries. In conditions 1–4 the ids are event ids.

**Named squads** (0x4a8237). Each slot looks for its own unit, so three slots alike need three
units. **code**
- Codes 1 and 6 search the hero's army, slots 1–12, for a **living** unit that no earlier slot
  took. A unit matches when: its type is the slot's type and the slot has no character name
  (the unit's own name is **not** looked at, so a named character of that type also counts);
  or it carries the slot's character name (whatever its type); or the slot type is 255, the
  unit was added by an event (wage kind 3) and has no name. Code 1 needs a match, code 6
  needs none. (Unit removal, §7.1, does require the unit to be unnamed for a type match.)
- The units taken are noted in a list of three places; once it is full, each new one
  overwrites the third, which frees the unit noted there.
- Codes 2–5 search the AI armies whose faction is code − 1 with the same type-or-name rule but
  without the alive test and without the 255 rule; a match in any such army passes.
- **Code 0 fails the condition** (no branch accepts it).
- Community opcode 21 replaces this condition (§15).

**Artifacts** (0x4a815a tail). **code**
- For each slot: look in the pack, then in the worn slots of the hero's units. A found item is
  **taken aside for the rest of the check**, so two slots with the same artifact need two
  copies (9 shipped events list an artifact twice).
- Code 1 passes when found; code 6 when not found. Whenever the slot has not passed by then
  (codes 2–5, code 1 not found, code 6 found), the worn items of the AI armies of faction
  code − 1 are searched (their packs are not); for codes 1 and 6 that is faction 0 or 5, which
  no army has.
- **Code 0 fails.** Everything taken aside is put back at the end.

**Side effects of the check.** The meet-army test marks the army "meeting event waiting" when
everything before it held (§9). With *apply* on, a passing event also sets the silent flags of
§2 (0x4a7a40); for an event with no question and no message this clears its *ask* byte.
**code**

## 5. Flags

The scenario's flags live in one string (0x68ed00). Each entry is a name followed by a
non-breaking space (0xA0); a counter entry has one digit between the name and the space.
**code**

**Script syntax.** An event title may hold `%` followed by an action and a test:
`%` *action* [`=` *test*]. The action is the text between `%` and `=` (or to the end); the
test is the text after `=`. (dtm-format.md §9 gives the editor's side.)

**Test** (condition 15, 0x4a8a22 and the Community copies). **code**
1. Remove the first `^` if any.
2. Empty test, or a test containing `end_tutorial`: passes (§13).
3. If it contains `/`: remove that `/`; the test passes when the rest does **not** occur in
   the flag string.
4. Otherwise it passes when the test **occurs anywhere in the flag string** as a substring.

So the test is a substring search, not a name lookup: a test for `AB` holds while `AB1` or
`XAB` is set, and a test like `AB2` reads a counter's digit. Three shipped maps rely on this,
one on purpose (a short prefix test meant as "any of these numbered flags").

**Action** (0x4ab2a3, on firing). An action of 2 characters or fewer does nothing. **code**
- `+X`: if X does not occur in the flag string, append X and a space. If it already occurs
  (even inside a longer name), nothing.
- `-X` (only when the string is not empty): remove the first occurrence of X and the one
  character after it (the space for a plain entry; for a counter entry that is its digit, and
  the space stays).
- **Counters** are the `^` form: `+X^` appends X, the digit 1 and a space when X does not
  occur, else raises the character after X by one; `-X^` lowers the character after the first
  occurrence of X by one and removes X, the digit and the space when it reaches 0. The lower
  form does not check that X exists (it then changes an unrelated character). No shipped map
  uses `^`.
- Community opcode 18 makes the digit written by this event's `+X^` a random byte (§15).
- After the action: a test part containing `end_tutorial` marks the tutorial done (§13).

**Engine flags.** The exe itself adds `Sea` while the hero is aboard a ship and removes it when
he lands (0x496d28), and adds or removes `EnterShipyard` in the ship-return logic (0x497c68);
going to sea also removes `EnterShipyard` (0x496d28).
Scenario tests see them like any flag. **code**

**Lifetime.** A new game empties the flag string (0x4c10e3). Loading the next campaign map does
not touch it, so **all flags carry over** (§12). A restart restores the flags saved with the
restart snapshot (0x4b5ff8). **code**

**No implicit counter.** Only the `^` form keeps a digit. The plain `+X` sets a flag with no
digit (an older summary had it adding `X1`). **code**

## 6. The dialog: question, answers and when results apply

### 6.1 Opening (0x4a8ae8)

Opening sets the current event and builds the window.
- **With "ask" (76) set**: only the question text, the picture and Yes/No are built. **No
  result is applied.**
- **Without "ask"**: the answer byte of the event is cleared, and the dialog-time results are
  applied while the window is built, in this order: artifacts gained, artifacts lost, units
  added, units removed, spells learned (§7.1). The quest labels ("quest added" for a quest,
  "quest done" when it completes one) are shown. The message text is shown.

The other results are applied when the dialog is finished (§7.2). A silent event goes through
the same two steps without a window. **code**

### 6.2 The buttons

- **OK** (0x4c206c): finishes the event (§7.2). If the player has been defeated (§11), it
  returns to the main menu instead. **code**
- **Yes** (0x4c2100): **code**
  1. *ask* is cleared, and **once := "repeat after yes" (149) xor 1** (a bit flip: 2 gives
     3). Without byte 149 (no shipped event sets it) every answered event becomes a
     once-event the moment the player says Yes, whatever its once box said; with 149 = 1 it
     becomes a many-times event even if its once box was set.
  2. If the event has **no message text**, it is finished at once **without the dialog-time
     results**: its artifacts, units and spells are never applied (10 shipped events ask with
     an empty message; one of them would remove four units).
  3. Otherwise the dialog is opened again without the question, which applies the dialog-time
     results and shows the message; *ask* is set back to 1 for later firings. OK then finishes.
  4. With 149 set, *ask* is set back to 1 afterwards. In case 3 that reaches the event. In
     case 2 the finish has already cleared the current event, so the write lands outside the
     event table (the slot before event 1) and the event keeps *ask* = 0: its later firings
     skip the question and apply all results. No shipped event sets 149.
- **No** (0x4c2320): answer := 1, *last fired* := now + 1, times fired += 1, and nothing else:
  no flag action, no result, no chain, no quest. A No therefore counts as "happened" for the
  "not happened" test, and **uses up a once-event**. The scan then runs again. **code**

Opening a later firing of the event clears the answer again (6.1), and finishing an event
clears it too (0x4ab51b tail), so "happened with No" means "the last response was No". **code**

So a many-times question declined in a building is not asked again during that visit (the
No's firing guard, then the scan skips the building the move began in, §2) and is asked again
when the hero enters the building on a later move; a Yes makes it a once-event. Example, РК2's
village offers of peasants for the two mines (events 8, 9 and the replacement offer 10, which
a Yes to 9 opens a day later and which asks only while no peasant is left and the quest's end,
27, has not fired): two groups of three staff the two mines (19, 24), and 27 completes the
campaign quest only after both. Razdor: the same (`rk2_the_peasant_offers_and_the_mines`).

### 6.3 Look of the window

- The window is drawn in the **defeat** style when the title contains `#DEFEAT`, when the event
  is the map's defeat event, or when the player was defeated; in the **victory** style when the
  title contains `#VICTORY`, or when it is the victory event of a map with no next map. The
  style changes the frame and the music only; it does not end the game. **code**
- **Picture** (82): 0 the event's own picture; 200 the defeat
  picture; 201 the victory picture; 202 the portrait of the met army's leader; any other
  value the portrait of that unit type. **code**
- **Title**: the title text before the first `%`, then before the first `#`, with trailing
  spaces removed (0x4833f0); empty gives the language file's default title key. So `#` starts
  an editor note that is never shown (378 shipped titles have one). **code**
- **Resource row**: the event's gold and mana. Events with "no meeting" (148) set show no
  resource row (Community 0xc2831f); the values are still applied. **code**
- Layout: 634 px wide, centred on 1024 × 682 (on the tutorial's own position in the
  tutorial); the height grows with the reward rows, with a scroll bar past a limit. **code**

### 6.4 Placeholders

- Event texts: every `#HERONAME` in the question or message is replaced by the hero's name
  (the name typed at the new-game screen) each time it is shown. Case-sensitive, all
  occurrences (0x4aa3de). **code**
- Building and army descriptions: `#HERONAME` replaced once, at map load (0x4b2930,
  0x4b2a46), with the name known then. **code**
- Engine messages use their own keys: `#NAME1` (army name in the victory report), `#NAME2`
  (building name), `#UNITNAME` and `#GOLD` (garrison purchase), `#ARMYNAME` (battle start).
  There is no other escape: no class placeholder. **code**

## 7. Results

### 7.1 Dialog-time results (applied when the window is built, §6.1)

**Artifacts gained** (113–116): each non-zero id goes into the first empty pack slot of 256;
a full pack loses it silently (0x49a85c). **code**

**Artifacts lost** (117–120): if at least one of them is held, each held one is removed:
first from the pack, else from the first unit wearing it (0x49a8a8). **code**

**Units added** (97–100, names 101–104), per non-zero slot (0x4a8fe5): **code**
- If the army already has 12 units, the **weakest** of units 2–12 is dismissed and its worn
  items go to the pack. "Weakest" is the lowest **level value** of the tactical cost
  ([experience.md](experience.md) §1, mode 0, 0x4a02a0): the strength of the unit's level
  stats, without its items and without a building's defence, times its type's cost
  multiplier, divided by 100, with no Community "at least 1" hook. The slots are compared from
  2 to 12 and only a strictly lower value replaces the candidate, so on a tie the first one
  goes (0x4a942c). The hero (slot 1) is never compared. **code**
- The unit joins at **level 1** (exe level 0), unhurt, as an event unit (wage kind 3: free),
  paid as of now, with the slot's character name.
- **Taken from an army** (142): if that army has a matching unit (same search as removal,
  below), its whole record replaces the new one (level, XP, HP, items) and it leaves that
  army; an army left empty is taken off the map.

**Units removed** (105–108, names 109–112). Nothing happens unless at least one slot finds a
unit. Then per slot, the unit is searched **from the last unit backwards** (0x4961d0): **code**
- 255: the last unit after the hero with no character name;
- 254: the same, and it must be an event unit;
- any other type: the last unit of that type with no name when the slot has no name, or the
  unit carrying the slot's name. This search can reach unit 1, **the hero**, when no other
  unit of his type is left.
- **Given to an army** (136): when the slot has no character name the unit is appended (its
  count is capped at 12, so in a full army it overwrites the last unit); when the slot names
  one, the unit is put first, as the army's leader, with the others shifted down. The test
  is on the slot's name, so an unnamed unit picked by 255 or 254 under a named slot leads.
- Then the unit is removed and the army closes the gap (0x496310). The removal only deletes
  the record: a unit given to no army loses its worn items with it.

**Spells learned** (93–96): each one not yet in the book is appended (0x49c144); the code has
no book limit. Events with "no meeting" (148) set teach nothing (the Community hook 0xc28604
skips the call whenever byte 148 = 1); no shipped event combines the two. **code**

### 7.2 Finish-time results (0x4ab1ec → 0x4ab2a3 → 0x4ab51b)

In this order: **code**
1. "No meeting" (148): the met army is cleared, so later meet-army events of this scan fail.
2. *last fired* := now + 1; times fired += 1.
3. The flag action (§5); the `end_tutorial` mark (§13).
4. **Experience** (83, signed) to the hero alone ([experience.md](experience.md) §5).
5. **Gold** (85, signed): added, and the total **cannot go below 0** (0x4ab1be). A rumour that
   costs more than the player has leaves him at 0.
6. **Mana** (89, signed): added, clamped at 0.
7. **Activate** armies (121, 122), then **deactivate** army (123). Activation of an army that is
   off the map clears its destroyed and "beaten by" marks, revives and pays all its units,
   puts it on its start cell and, if that is its home building, gives it the building
   ([world.md](world.md); 0x4969b8).
8. **Patrol** (16, 17): radius := max(0, radius + delta); the patrol box is recomputed as home
   ± radius, clamped to the map.
9. **Show army** (144): the view scrolls to the army and reveals 6 half-cells (3 cells) around it.
10. **Lanterns** (128–135): for each point whose lantern radius byte is not 0, the view scrolls
    there and reveals radius × 2 half-cells. A point with radius 0 is not lit. Nothing else
    about the point changes.
11. **Quest**: an event of type 3 adds itself to the journal (§10).
12. **Completes quest** (124): removes that quest from the journal.
13. **Spell** (81) is queued; its id is capped at the number of spells.
14. **Move army to hero** (143): the army named in byte 136 (not 142) is placed on the cheapest
    free cell around the hero (0x4980d8: of the 8 neighbours, clockwise from north-west, by
    terrain cost, +50 000 for a building cell, +100 000 for an occupied cell; no move when
    every cell costs 100 000 or more). It is not activated by this.
15. **Battle** (147) is queued.
16. **New hero class** (137): the hero's unit type becomes that unit (the byte is a 1-based
    unit id).
17. **Delay** (126) is queued.
18. The **chained event** (138) is noted.
19. **Relative event** (77): its start := now + 60 × delay hours (79). The start is overwritten,
    so a repeating event is re-based.
20. **Victory or defeat** check (§11). If the event is either, processing stops here: no
    follow-up and **no chain**.
21. Otherwise the army is recomputed and the event is closed; then the follow-ups (§8).

With the Community opcode switch (§15) the steps 4–6 are replaced by the opcode.

## 8. Follow-ups, chains and delays

After an ordinary event is closed (0x4ab51b tail): **code**
- **Army and resource line.** First the hero's army is recomputed (0x497240), then the bottom
  resource line (mana, gold, daily income, daily upkeep; [interface.md](interface.md) §6) is
  redrawn (0x4aba83 → 0x49d224), so gold, mana and wage changes show at once. The redraw
  changes no value. A victory or defeat event leaves through §11 before this point and skips
  both.
- **Battle**: the army is activated and the battle opens.
- **Lanterns or a shown army**: the scroll and reveal animations run; a timer then runs the
  chain 250 ms after they end (0x4af658).
- **Spell**: its animation runs and the spell is cast on the hero's army (free, long duration:
  [economy.md](economy.md) §4); the chain timer runs after it.
- **Delay**: the hero waits *delay* hours (2 ticks of 30 minutes per hour, the world going on,
  events checked every tick). **A chain behind a delay is dropped**: the delay path starts no
  chain timer. No shipped event has a delay.
- **None of lanterns, shown army, spell or delay**: the chain runs at once; with no chain, the
  scan runs again. A battle does not count here: an event with a battle and none of the four
  opens the battle and then, in the same step, its chain (or a new scan). No shipped event has
  a battle.

**Running a chain** (0x4ab1ec end, 0x4af658): the chained event is opened **as it is**: its
done flag, window, guard, once flag, class, place and conditions are not checked. If it is
silent it is applied at once (and its own chain follows); otherwise its dialog opens, with
its question if it asks one. The exe has no depth limit or cycle guard for chains. **code**

## 9. Meetings

- When the hero and an AI army come into contact, the exe records the army as the *met army*
  and runs the scan (§1). If an event fires, the contact's battle is cancelled and the dialog
  shown; otherwise the battle starts (0x4adccf–0x4add27). **code**
- The meet-army condition holds only for the met army. The met army is cleared when the scan
  goes idle, when an event with "no meeting" (148) finishes, and when that army is
  deactivated (0x496900: the deactivate result, or a "take from army" that empties it), so a
  question that sends the met army away ends the meeting for the rest of the run (Обучающий1:
  event 6's Yes deactivates the ghost, and event 7, "meet the ghost and Yes to 6", waits for
  a new meeting). **code**
- Whenever every condition before the meet-army test holds for a meet-army event, that army
  is marked "meeting event waiting" (0x4a801a). The marks are cleared and recomputed for all
  meet-army events after each dialog (0x4abf44). The contact code uses the mark to treat the
  army as one to talk to ([world.md](world.md) §4). **code**

## 10. Quests, the journal and rumours

**Journal** (0x49c170, 0x49c2b8). **code**
- A quest (type 3) that finishes appends itself to the journal, **every time** it finishes (no
  duplicate check; a repeatable quest appears twice). Up to 128 entries are reserved; the add
  does not check the limit.
- "Completes quest" removes the **last** journal entry for that event. A completed quest can
  be received again if it fires again.
- The journal list shows the display title (§6.3); the detail below it shows the question and
  the message (see *Journal detail*).
- The journal is emptied when a map is freed (0x4b2204): it does **not** carry over to the next
  campaign map.

**Journal detail** (0x49c388, the selection handler of the hero window's quest list, set at
0x4d8db3). Selecting an entry clears the description box under the list and fills it, top to
bottom: **code**
1. The entry's display title (the same cut as §6.3: before the first `%` and `#`, trailing
   spaces removed, 0x4833f0), word-wrapped, in its own title font.
2. An empty line, then the label of the language file's `QuestFullDescript` key (section
   `Hero`).
3. The quest text as one justified, wrapped block (0x47ea80): the **question** first, if the
   event has one, then a line break, then the **message**. Each part starts with the same
   paragraph-indent marker the other text windows use. Line breaks and the `#\` marker in the
   texts split lines. Two differences from the event window: `#HERONAME` is **not** filled
   (the dialog fills a copy each time it shows a text, 0x4aa3d9, and the stored text keeps the
   placeholder), and the window's colour and centring markers are not interpreted (this path
   only splits lines; the dialog goes through its markup reader 0x48e438), so both appear as
   typed.
4. An empty line, the label of the `QuestGetTime` key, then a duration written by the long
   calendar (0x49cbf0: years, months, days, hours, leading zero parts left out; under 60
   minutes a fixed word).
5. The box is redrawn from its top (0x47cf4c) and a scroll control is set back to 0 (0x47c650).

The duration is **the time since the event last fired**: now − *last fired* (byte 156,
0x49c517). It is not a time left: quests have no deadline. Since finishing sets *last fired*
to now + 1 and the idle reset (§2) brings it back to now, a quest just received shows the
under-an-hour word; it then grows with the clock. A quest that fired more than once shows the
time since its **latest** firing in every one of its entries. A No answer (§6.2) also sets
*last fired*, but a No adds no entry. With no entry selected (or the selection past the end)
the box is left empty. **code**

**Main hall list** (0x4beaac, take 0x4bb798). **code**
- In a building's main hall, the building's listed events of type 3 (quests) and 4 (rumours)
  that pass the full check of §3–§4 are listed, quests in one colour and rumours in another.
- Taking an entry opens its dialog at once, without a new check: a question first if it asks,
  else its results.
- The places the taken event shows (lanterns, a shown army) are flown to at its window's OK,
  over the world map, before the building window comes back (interface.md §9.8; checked
  under Wine on РК1), not when the building window is closed.
- Rumours have no price of their own: a rumour that costs money has a negative gold result
  (and usually a gold condition and a question). In shipped maps prices run from 5 to 700.
  Combined with the Yes rule of §6.2, a rumour that asks is heard **once**.
- In villages and shipyards, and at event points, quests and rumours are not listed: the scan
  fires them like local events (§2).

Most shipped quests are started by chains (78 of 103); 13 sit in ordinary buildings and are
taken in the hall; 4 sit at event points or in villages and fire by themselves. **data**

## 11. Victory and defeat

**Victory and defeat events** (header 0xD2, 0xD8). When either finishes (after its results,
§7.2 step 20): **code**
- If it is the **victory** event, the map is part of a campaign (header 0x10F ≠ 0) and the
  file `Maps_Rus\<next map name>` exists: the campaign hand-over (§12).
- Otherwise (the defeat event, or no next map, or a missing file): the game returns to the
  main menu (0x4bfb34). The event's own window was the last thing shown.
- Either way the event's chain and follow-ups do not run.

**Defeat by losing the army.** When the player's army has no living unit after a battle
(0x4c527d) or after other damage (0x4906eb), the defeat mark is set and, after 250 ms, the
battle-report window is shown with the defeat texts; its OK returns to the main menu. **This
does not fire the map's defeat event**; that event is an ordinary scripted end, with its own
conditions (one shipped map chains its defeat event from an event that requires the hero at
1 HP, §4 condition 9). **code**

## 12. Campaign hand-over and carry-over

The hand-over (0x4b5b64) runs only on the victory event (§11). **code**
1. Every unit of the player's army loses its spell effects (the 4 lasting-spell slots).
2. The army record, gold, mana, spell book and pack are saved; the next map is loaded (which
   builds a fresh hero, army, gold, mana, pack and book from that map's preset for the same
   class).
3. **The old hero's whole unit record always replaces the new hero**: type (an event-changed
   class stays), level, XP, HP (wounds stay), items, name.
4. Then, by the next map's header bytes 0x110–0x116:
   - [0] gold: gold is **set** to the old amount (the new preset's gold is lost)
     (0x4b5d4f). It is not added to it.
   - [1] mana: set to the old amount.
   - [2] fame: no code.
   - [3] off: the hero's level and XP are set to 0 (level 1, no XP) and the new map's book is
     kept; on: the old book replaces the new one.
   - [4] off: the hero's four worn slots are emptied; on: he keeps them.
   - [5] on: the old pack **replaces** the new pack.
   - [6] on: the old army **replaces** the new map's army (the new preset troops are lost);
     every unit is marked paid as of now; dead units are then removed; the army is
     recomputed. Off: the new map's preset army with the old hero in front.
5. Flags carry over (§5); the journal does not (§10). A restart snapshot of the new map's
   start is taken (0x4b5ef8).

Community opcode 15 can change the digits of the next map's name (§15).

## 13. Tutorial hooks

- **The offer.** At "new game", while the tutorial is not marked done (the language ini's
  `[Tutorial] Completed` key, or any save present: 0x4e2970), the exe replaces the event table
  with one synthetic asking event built from the ini's `[Tutorial]` keys and shows it
  (0x4ac748). Yes starts the tutorial map (the ini's tutorial map name); No returns to the menu.
  Either way the synthetic table is freed (0x4ac950). **code**
- **The end mark.** An event whose title test (§5) contains `end_tutorial` passes its flag
  test, and when it finishes the exe writes `[Tutorial] Completed=1` into the language ini
  (0x4ac9fc). One shipped tutorial event carries it. **code**
- In tutorial mode the event window keeps the tutorial's own position and a fixed height.
  **code**

## 14. Spare event slots

After the N map events the exe keeps four built-in slots that go through the same dialog and
finish code (0x4ab1ec): N the **battle report** (loot; more than 8 loot items show again in a
further report), N+1 the **noon report**, N+2 the **village offer**, N+3 the **garrison
purchase**. Their rules are in [economy.md](economy.md). **code**

## 15. Community opcodes (summary)

An event is in **opcode mode** when byte 148 ("no meeting") is 1 and byte 17 (patrol delta)
is not 0; byte 17 is the opcode, read as a signed byte (0xc2669e, 0xc27862). Opcodes 6–22 go
to the effect table; every other non-zero value (1–5, 23 and up, and negative values) goes
through the byte-poke step, where 2 sets and everything else at or below 2 (1 and the negative
values) adds. No shipped map uses it. In opcode mode
the XP, gold and mana results are not applied (their fields are the opcode's arguments: A =
XP field, B = gold field, C = mana field), the patrol change still happens with the opcode as
delta, and the rest of §7 runs normally. The Community map notes hold the full detail.
**code**

Two Community hooks look only at byte 148 and so also touch plain "no meeting" events: the
spell and resource-row hooks of §6.3 and §7.1, and the cell test of opcode 19 (0xc2875e),
which replaces the gold, mana, squad and strength tests whenever the numeric gate of §4
condition 11 lets them run, byte 148 = 1, byte 17 is at most 19 as a signed byte (0 and
negative values included) and byte 13 (the low byte of the strength condition) is not 0. No
shipped event meets that. **code**

| Op | Effect |
|---|---|
| 1, 2 | Change one byte of another event record: event (this + A), byte offset B, value C; 1 adds, 2 sets (negative opcodes add like 1). A second change is driven by byte 11 (signed; 2 sets, 1 or negative adds) with event (this + gold condition), offset level condition, value mana condition; it runs for every opcode outside 6–22. |
| 3, 4, 5 | Condition on such a byte: 3 needs byte < C, 4 byte = C, 5 byte > C (signed bytes). A second test is driven by byte 11 (3–5, same meanings, with the second change's fields); it applies to any opcode-mode event, whatever byte 17 holds. |
| 6 | Unit B of holder A gets the four artifacts 113–116 as its worn items (the normal gain is skipped). Holder A ≥ 0 is an army (0 the hero's), A < 0 is the garrison of building −A. |
| 7 | Unit B of holder A becomes type C. |
| 8 | Byte at army record +0x1694 of army A := B. |
| 9 | Faction of army A (or of building −A) := B. |
| 10 | Attitude B of army A (or building −A) := C. |
| 11 | Unit B of holder A (B = −1: all 12) gets spells 93–96 as permanent effects; empty entries clear slots. Spells are not learned. |
| 12 | Unit record byte +0x14 (named character) of unit B of holder A := C. |
| 13 | Unit B of holder A (−1: all) gains C XP. |
| 14 | Condition: unit B of army A (−1: all) has each of spells 93–96 among its effects (inverted when B = 1). Spells are not learned. |
| 15 | Next map name: the 5th-from-last character := digit B, the 7th-from-last := digit A (when non-zero). |
| 16 | Removes spells 93–96 from the hero's book (the book count is not lowered). |
| 17 | Byte at army record +0x169D of army A := B. |
| 18 | The digit this event's `+X^` writes is a random value in [A, B] (raw byte; A and B are the low bytes of their fields, read as signed). |
| 19 | Army A's 1-cell rectangle := (B, C). As a condition (with byte 13 = army): that army stands at (gold condition, mana condition). |
| 20 | Teleports the hero to (A, B). |
| 21 | Replaces the named-squad condition: for each slot whose name is carried by a unit of the hero's army, that unit must be of the slot's type (a missing name passes). The type is compared with the slot byte as stored, without the −1 shift of the other unit tests, and unit records 1–12 are scanned whatever the army size. |
| 22 | No effect. |

---

## Razdor now → original

Razdor: `src/rules/events.rs` (engine), `src/rules/script.rs` (the game as the engine's
world, carry-over), `src/rules/journal.rs`, `src/dt/dtm.rs` (record and flag script parsing),
`src/ui/story.rs`, `src/ui/mod.rs`.

| Topic | Razdor now | Original | Status |
|---|---|---|---|
| Byte 145 | A condition: the hero's HP is exactly 1; it never sets HP | A **condition**: the hero's HP is exactly 1 (§4) | Matches |
| Yes answer | Yes clears ask and sets once := byte 149 xor 1; with a message, ask is set back for later firings; without one, ask stays 0 (the write that misses) | Yes sets once := byte 149 xor 1; the question returns for later firings (§6.2) | Matches |
| Ask with empty message | Yes finishes it at once: artifacts, units and spells never applied | Artifacts, units and spells are never applied (§6.2) | Matches |
| When results apply | The window's results (gains, losses, units added, removed, spells) when it opens, then the finish's in the exe's order, then battle, spell, delay (`EventEngine::show`, `finish`) | Artifacts, units, spells when the window opens; the rest at OK (§6.1, §7.2) | Matches (the finish follows the opening at once: nothing happens while the window is up) |
| Events behind a window | A message window holds the scan: the next events, and the shown event's chain, run when it is closed (`EventEngine::window_closed`, `Game::event_window_closed`; the interface and the replay call it at the window's OK) | The scan stops at a shown window and runs again after its OK (§2) | Matches; an event without texts that only shows lanterns, an army or a spell does not hold the scan in Razdor (the original waits for the animation) |
| Flag test | Substring search of the one flag string; first `^` dropped; `/` negates; empty or `end_tutorial` passes | Substring of the flag string (§5) | Matches |
| Flag action | `+X` appends X with a non-breaking space if not a substring yet; `-X` removes the first occurrence and the next character; counters only with `^`, their slips included; actions of ≤ 2 characters ignored | Same (§5) | Matches |
| Engine flags | `Sea` added when the hero goes to sea and removed when he lands, `EnterShipyard` removed when he goes to sea (`Game::sea_changed`) | `Sea` aboard a ship, `EnterShipyard` in the ship logic | `Sea` matches; `EnterShipyard` is only removed at sea: its setting is left out (Razdor does not track the ship's shipyard; no shipped event tests it) |
| Quests in buildings | Listed in the main hall with the rumours and fired when taken; villages and shipyards fire them on entering | Listed in the main hall and taken by the player, except in villages and shipyards (§2, §10) | Matches |
| Event points | Every listed event, whatever its type | Every listed event, whatever its type | Matches |
| Rumour or quest list | The building's quests and rumours that pass the full check (`EventEngine::hall`); taking one opens it without a new check | Quests and rumours of the building that pass the full check | Matches |
| Places shown by a taken quest | Flown to at its window's OK over the map, then the building window comes back (`App::fly_from_building`) | At the OK, over the world screen, then the building window again (§10, interface.md §9.8; live on РК1) | Matches |
| Journal | The engine's journal: re-added on every finish, completion removes the last entry, a new engine on the next map; Razdor's history is a separate extra | Re-added on every finish; completion removes the last entry; emptied on the next map | Matches |
| Journal detail | Active quests: question then message, `#HERONAME` not filled, markers raw, and the time since the event last fired (`Game::quest_row`) | Title, a label, question then message as one justified block (no `#HERONAME` fill, markers raw), a label and the time since the event last fired, long calendar (§10) | Matches in content; the justified layout and the long calendar's wording are presentation, left |
| Dismissed unit on a full army | Lowest level value (`experience::level_value`: level stats, no items, raw multiplier) of units 2–12, first on a tie; its worn items to the pack | Lowest mode-0 tactical cost of slots 2–12, first on a tie (§7.1) | Matches |
| Resource line after an event | The bar reads the live values every frame; the army is marked for rescoring after each event | Army recomputed and the resource line redrawn on every non-final finish (§8) | Matches in effect |
| Gold result | Clamped at 0 | Clamped at 0 (§7.2 step 5) | Matches |
| Numeric conditions | All five only with byte 18, skipped when byte 17 ≠ 0 with a positive squad value; the opcode-19 cell test replaces gold, mana, squads and strength under its own test | All five only with byte 18, and skipped when byte 17 ≠ 0 with a positive squad value (Community) | Matches |
| Named squad, unnamed slot | A type match ignores the unit's own name; codes 2–5 search the faction's armies without the alive test | A type match ignores the unit's own name: a named character of that type satisfies it | Matches |
| Squad count | Unit records, the dead included | Unit records, the dead included | Matches |
| Army inactive | Waiting off the map; a destroyed army is neither | Off the map and not destroyed | Matches |
| Army in its home | Its current building is its home; no home passes | Its current building is its home; no home passes | Matches |
| Owner code 0 | Units and artifacts: the slot fails; buildings: skipped | Units and artifacts: the slot fails; buildings: skipped | Matches |
| Artifact slots | One item per slot, taken aside while checking | One item per slot | Matches |
| Artifact holder 2–5 | Worn items of the faction's armies only | Worn items only | Matches |
| Units added to a full army | The weakest of units 2–12 is dismissed (items to the pack), then the unit joins | The weakest of units 2–12 is dismissed (items to the pack), then the unit joins | Matches |
| Unit taken from an army | The whole record (level, XP, wounds, items, name, wage kind); an emptied army leaves the map | Takes the whole record; an emptied army leaves the map | Matches |
| Unit removed, by type | From the last unit back; unnamed slot: unnamed unit of the type; named slot: that name, any type; nothing unless a slot finds one; never the hero; given to no army, its worn items go with it | Unnamed slot: unnamed unit of the type; named slot: that name, any type; can hit the hero | Matches, except the hero (unknown whether reachable: Razdor's guard kept) |
| Removed unit given to an army | Slot without a name: appended, over the last of a full army; slot with a name: the leader, the others down | Appended (overwriting the last of a full army); under a named slot it becomes the leader | Matches |
| "No meeting" and spells | Not learned; no resource row | Not learned; no resource row | Matches |
| Lantern with radius 0 | Not lit | Not lit | Matches |
| Move army to hero | Army 136 only, not activated, cheapest neighbour | Army 136 only, not activated, cheapest neighbour by terrain cost | Matches |
| Patrol change | Radius, and the box back around the home cell; in opcode mode the opcode is the delta | Radius and the patrol box around home | Matches |
| Chain timing | Dropped behind a delay unless lanterns, a shown army or a spell start the chain timer; a battle does not stop it; nothing after a victory or defeat (no spell, battle or delay either) | After animations; dropped behind a delay; not after a victory or defeat | Matches |
| Guard | *Last fired* := now + 1; with a duration L ≤ now, without L < now − 60; set back to now when the scan goes idle | Same scan blocked (idle reset to now); duration 0: more than 60 min after `now + 1` | Matches |
| Opcode mode test | Byte 148 = 1 and byte 17 ≠ 0 (signed); pokes of one byte across records, the run-time bytes 156–162 included; strict < and >; op 18 the digit of the event's `^` flag; op 21 only with byte 17 = 21 | Byte 148 = 1 and byte 17 ≠ 0; op 21 needs byte 17 = 21; no spell-removal extension | Matches |
| Placeholders | `#HERONAME` only; titles cut at `%` and `#` | `#HERONAME` only; titles cut at `#` | Matches |
| Double spaces | Collapsed when the game loads the map's events, names and descriptions | Collapsed at load | Matches |
| Defeat by army loss | Game over screen | Defeat report, then the main menu; the defeat event does not fire | Matches in effect |
| Victory without next map | Victory screen | The event's window, then the main menu | UI choice; fine |
| Carried gold | Set to the old gold | Set to the old gold | Matches |
| Carried pack, army | Replace the new map's (the army's dead dropped, paid as of now) | Replace the new map's | Matches |
| Carried hero | His whole record (HP and class included, lasting spells cleared) replaces the new hero, then the bytes | His whole record (HP included) replaces the new hero, then the bytes | Matches (HP above a reset hero's maximum is cut: a guess) |
| Carried flags | Carried | Carried | Matches |
| Tutorial offer | At every new game until done: the install's mark, Razdor's settings (set by the `end_tutorial` event), or any save | Only until the tutorial is marked done (ini, or any save) | Matches (Razdor writes its own settings, not the install's ini) |
| Window, once, class, relative start, chain ignoring checks, No counting as happened, victory stopping the chain | — | — | Match. |

## Unknowns

- What the exe does with a repeat of 1–1439 minutes (division by zero; no shipped map has one).
- The exact moment the "building of the current move" mark (0x4ed434) is refreshed relative to
  the scan at the building's entry; read: a building's events are checked once per entry.
- Whether the empty-next-map check (`Maps_Rus\` with no file name) can count as existing.
- Byte 150 ("generate the battle army"): no reader was found in the event code.
- Whether removing the hero by a type match (§7.1) was ever reachable in practice.
- The restart snapshot's role at the first map (taken at new game, 0x4c1124) versus loads.
