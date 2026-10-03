# Map editor testers: the battle tester and the AI viewer

The Community Update ships a map editor, `DTMapEdit.exe` (32-bit Delphi, image base 0x400000),
next to the game. Besides the map tools it has two testers. The **battle tester** fights two
armies on the battle grid. The **AI viewer** runs the world AI on the map being edited, with no
game around it. Neither calls into the game. The editor carries its own copies of the battle
engine and the world AI, and those copies are the **pre-Community (vanilla) code**: they have
none of the patch hooks the shipped game runs. This file describes both testers and lists every
rule where the editor's copies differ from the game described in
[battle.md](../original-mechanics/battle.md), [ai.md](../original-mechanics/ai.md) and
[community-patches.md](../original-mechanics/community-patches.md). It also covers the strength
figure that the editor's army window shows.

The rules are in our own words. Addresses are virtual addresses in `DTMapEdit.exe` unless marked
"game" (the Community `DiscordTimes.exe`). They are given as evidence only.

Confidence tags:
- **code**: read in the code (decompile, and disassembly where noted).
- **data**: matches the shipped files, not traced in code.
- **unknown**: not determined.

Record sizes differ from the game's: a battle unit is 0xB4 bytes (game 0xA5), a battle side 0x908
(game 0x851), a world army 0x3829 (game 0x3827). The layouts are the same fields, realigned.
**code**

## 1. Settings: what is read from the ini and what is built in

- At start-up the editor reads `_Global.ini`, section GlobalOptions, from its own folder
  (0x59cbd4). It reads the same keys as the game into the globals that its battle engine and
  world AI use: the spell divisors, the per-turn mana drain, Row2Def, the turn limit, the two XP
  corrections, the AI's distances, upkeep, healing, resurrection, loot and hiring values, the
  five rows of AI target thresholds, AIGetPathDistance, AIExpiriencePercent and
  ShotWeaponRange. The Community install keeps that file next to the editor, so in practice both
  programs see the same values. **code**
- The values compiled into the image are only fallbacks. Some differ from the shipped file
  (resurrection time 1440 against 10080, both XP corrections 100 against 30 and 50). **code**
  for the image values, **data** for the file.
- These rules are never read from any file, so they are built in (§4 and §5 give the details):
  - the knight damage share (90 %);
  - the five rule switches of the battle tester (§2.4);
  - the nature sets;
  - the poison regeneration (−15);
  - the unpaid-unit penalty;
  - the step-bank cap;
  - the hero's castle-income share;
  - who receives castle and fort income;
  - the Community keys (MinSpell…, ManaDrain, Evasion). The editor has no globals for them.
  **code** (no write to these globals outside the battle tester's option panel, 0x57e304)

## 2. Battle tester

### 2.1 Window and how it opens

- The window shows two armies of up to 12 units on the battle grid. The **top army is side 2**
  (the AI's) and the **bottom army is side 1** (the player's). The front is 6 wide, so the grid
  is the wide 6/4/2 pyramid: row 2 keeps columns 2–5 and row 3 keeps columns 3–4. Each army has a
  status bar with four numbers (§2.6). The right panel holds the unit-type list with a stat card
  for the selected type, the buttons, and three check boxes:
  - **Super AI**;
  - **all AI** (both sides played by the AI);
  - **delay** (on by default).
  (form layout, 0x5778a4) **code**
- **From the main window** (0x5770d8): the window is created once. Its type catalogue is built
  (0x5771c0) and the last setup is shown. The start button stays disabled until the armies are
  cleared, loaded or generated. **code**
- **From the AI viewer** (0x5770f8): the two armies are copied in as built for battle, their
  strengths are computed, the start button is enabled and the window opens. **code**

### 2.2 The type catalogue

- One entry per unit type: name, picture and the type's base stats. This is the level-0 block,
  with no level gains, no items and no building defence. **code** (0x5771c0)
- The entry's strength field is set to the type's **gold Cost**, not to a tactical cost. That
  field is what the side-strength formula weighs, so units added from the catalogue count by
  price. Armies that come from the AI viewer carry their real tactical costs instead (§5).
  **code**
- The type's Surrender value is copied, so whole-side surrender works. **code**

### 2.3 Building an army

The cell under the mouse is tracked by a 10 ms timer (0x57b12c). A click on a slot (0x57d3f8)
does one of these things:
- **During a battle:** if the cell is legal for the acting unit, it becomes the player's chosen
  target.
- **Outside a battle,** and only while the start button is enabled (**code**; the enabled test
  is read from the virtual slot):
  - a click on an occupied cell removes that unit. The later units move down one place in the
    list, and the army's cost total drops by the unit's value;
  - a click on an empty cell with a type selected adds that type there and adds its value to the
    cost total.
  Each change refreshes the cost label and briefly highlights the slot. **code**

The other buttons:
- **Clear** (0x57de24) empties both armies and enables the start button. **code**
- **Swap** (0x57dea8) exchanges the two armies whole. **code**
- **Redraw** (0x57df28) repaints the field. **code**
- **Esc** works as the stop button. **code**

**Random armies** (0x5795ec). Two buttons generate both armies at once. **code**
- Budget: 1100 for one button and 2100 for the other (Tag × 1000 + 100). Each side gets the full
  budget and makes its own choices.
- Each side first draws a **nature set** with one roll out of nine:
  - six in nine: Normal, Rogue or Hero;
  - two in nine: Rogue only;
  - one in nine: Undead only.
- Then it draws types at random. A type is taken when all three hold:
  - its nature is in the set;
  - its Cost is at least 10;
  - its Cost is **below** the budget still left.
  The cost is then subtracted from the budget.
- It stops when 100 or less is left, or at 12 units.
- Each army is then auto-arranged and its strength computed.
- The generator is seeded from the CPU clock, so the armies cannot be reproduced.
- **Quirk:** a rejected draw is simply redrawn. If no type of the chosen set costs between 10 and
  the remaining budget, the loop never ends and the editor hangs.
- Nothing excludes the hero-class types: any type of a drawn nature can be picked. A Knight
  placed first makes its side a knight army (§4, item 6).

**Placement file** (0x57e3c4 save, 0x57e77c load). **code**
- The file is `Battle.Sav` in the editor's folder, in ini format.
- It has one section per side. Each grid cell (row 1–3, column 1–6) gets one key holding the
  unit's type number, or 0 for an empty cell.
- Loading rebuilds the units from the catalogue in row-then-column order and recomputes the cost
  totals. Without the file, a message box says so.
- Only type numbers are stored. Levels, items, wounds and the building defence of an army that
  came from the AI viewer are lost when it is saved and loaded.

### 2.4 Rule switches

An options panel edits five switches of the battle engine (0x57e264 shows them, 0x57e304 stores
them; Cancel keeps the old values). They are program-wide: they last for the session and also
change the AI viewer's simulated battles. **code**

| Switch | Default | Effect when on |
|---|---|---|
| Counterblow (0x5bb114) | on | Counterblow units hit back after melee |
| Short-range shots (0x5bb118) | off | Shooters and casters reach only enemy cells in columns c−1..c+1 of the enemy rows 1–2, not the whole rows. This holds in both rows: for back-row units and for front-row units that are free to shoot or cast (0x4f84f2, 0x4f8689, 0x4f886d, 0x4f8a48) |
| Collapse (0x5bb11c) | on | Empty front rows collapse after deaths (0x500839, 0x502fbe). Off: no collapse, and the dead "pull" action of the game (map code 3) comes back (0x4f7f83, 0x4f9fe2) |
| Long strike (0x5bb120) | on | A front-row melee unit with nothing adjacent may strike the nearest enemy front unit (0x4f81c4); also enables the AI's edge rule (0x4fa523) |
| Initiative cost (0x5bb124) | off | Every action also costs 1 current initiative (0x501145) |

The defaults are the game's fixed values (game 4ed384–4ed394). In the game these switches cannot
be changed. **code**

### 2.5 Running a battle

The start button runs the battle (0x57bb60, then 0x57bc7c). **code**
1. Both sides' strengths are computed and shown.
2. The battle is set up (0x50321c) with the subtractive damage mode and no random spread. Side 1
   gets +1 base initiative, as in the game. The AI mode is:
   - **1** with Super AI;
   - **0** otherwise.
   The full AI-vs-AI pre-simulation runs, as in the game.
3. **Super AI** (0x57ebac) also ticks all-AI when it is checked. Unticking Super AI leaves all-AI
   on.
4. The loop runs until the battle is over:
   - The engine picks the actor, and its slot is shaded.
   - **Bottom army without all-AI:** if the actor is on the bottom army (side 1) and all-AI is
     off, the tester waits for a click on a legal cell.
   - With the delay box on, the mouse pointer first jumps onto every actor, the player's
     included (0x57bb94).
   - **All other actors:** the engine's AI choice is used. With the delay box on, the pointer
     also jumps onto the target, and the tester pauses **500 ms** (0x504280).
   - The action runs. With the delay on, only the changed slots are redrawn, unless the engine
     has raised one of the four per-side change flags (side bytes +0x904 and +0x905), in which
     case the whole field is redrawn (0x57c1a8). With the delay off the whole field is always
     redrawn.
5. **Stop** (button, Esc or closing the window) ends the loop at once, with no result. **code**
6. The finished state is never written back. Pressing start again replays from the same setup.
   **code**

What the AI modes mean (killable test, battle.md §4):
- Mode 1 gives the full "kill with my remaining actions" test to **side 1 only**, which is the
  bottom army, not the AI's army.
- Mode 0 gives it to nobody.
- The game's improved mode 2 (both sides) is never used.
- In the normal mode the editor's code has the same own-side HP slip as the game (0x4fad9d).
**code**

### 2.6 The numbers and the closing message

**The status bars.** Each army's bar shows:
- the sum of its units' values (gold Cost for catalogue units);
- its side strength (the formula of experience.md, side strength, with the editor's knight
  flag);
- after the battle, its remaining value: the sum over surviving units of value × HP ÷ max HP,
  each term rounded down, the total kept in 16 bits (it wraps past 65535);
- a diagnostic line: the turn, three figures from the end-of-battle step (the third is shown as
  a percentage), and the damage taken against the damage the pre-simulation predicted.
**code** (0x57af28, 0x57c6f0–0x57cb45). The exact meaning of the three end-of-battle figures is
**unknown**.

**The closing message** (0x57c318–0x57c6d3). It appears **only with the delay box on**. **code**

1. **The bottom army lost** (no units left, or it surrendered):
   - Compute `r = round(2 × strength(bottom) / strength(top))`, using the strengths from battle
     setup. The rounding is the runtime's default, half to even (2.5 gives 2). A negative `r`
     gives no message.
   - `r` = 0 or 1 gives one of two fixed boasts.
   - `r` = 2, 3–4 or 5–99 gives one of three boasts that mention the turn count.
   - `r` of 100 or more gives no message.
   - Every message is the "AI" mocking the player. The tone depends on how much stronger the
     player's army was.
2. **The top army lost:** the same with the ratio reversed, `round(2 × top / bottom)`. Five
   messages of reluctant praise are graded the same way. Three of them mention the turn count.
3. **Both armies still stand** and neither surrendered (the turn limit was reached): the AI says
   it refuses to fight with such an army.
4. Cases 1 and 2 are tested separately. If both sides end empty, both messages could appear.

## 3. AI viewer

### 3.1 Opening

When the viewer opens (0x58cc84, from the main window), these steps run. **code**
1. The window is created once. The random generator is set to a **fixed seed**, so a run can be
   repeated. A check box reseeds it from the CPU clock at the AI start (0x58e9f8).
2. The clock starts at the map's start time.
3. The armies (0x58b9d0) and buildings with garrisons (0x58c8b4) are built from the editor's
   records:
   - artefacts go to the leader;
   - the army spell goes on every unit, never expiring;
   - each market is re-rolled once.
4. The map's events and flags are copied.
5. A one-pixel-per-cell overview is drawn, with the towns, castles and forts in their faction's
   colour.
6. Two lists pick two subjects to inspect. The first holds the hero slot and the armies. The
   second holds the armies, then towns, castles, forts and the type-12 buildings.
7. The panel shows the daily income totals of towns, of castles and forts, and of villages.
8. The cost maps are built (0x57ec1c), and the world AI is initialised as at map start
   (0x582bcc).

**Hero.** A radio group picks no hero, or one of the three classes. A class can be picked only if
its start preset has a position, and only before time has run. **code**
- With a class (0x592aac), army 0 is built from the preset:
  - the class leader and the preset's troops;
  - the preset's position and gold;
  - faction 1, with the header's attitudes;
  - aggression −20;
  - speed 4 for every class (game: 5, 5, 4);
  - the three preset items **worn** by the hero (the game puts them in the pack).
- The start building and every building flagged for that class become the hero's.
- Without a hero, army 0 does not act.

### 3.2 Time

- **Step:** each step is **6 game minutes** (0x58eaf0). **code**
- **Buttons:** one step, 10 steps (1 hour) or 60 steps (6 hours) (0x58f220); also run and stop
  (0x58f2e4, 0x58f374). While running, the panels refresh every 3 game hours. **code**
- **24-minute tick:** when the clock is a multiple of 24 minutes, a new AI tick of 24 minutes
  opens. Every moving army's step bank then gets 24 minutes, capped at 200. Between those ticks
  the 6-minute steps only spend what is already banked. Event scans happen on these ticks too
  (§3.4). The game opens an AI tick per hero step or per 30-minute wait instead, and keeps its
  bank in hundredths of a minute; the editor banks whole minutes. **code** (0x584b60 for the
  bank)
- **Order of one step:**
  1. the midnight work (0x582398);
  2. then, for each AI army in index order: alive or respawn (0x58367c), the step clock
     (0x584b60) and, only if it arrived, the arrival rules (0x586954). An arrival that meets the
     hero is recorded for the events. **code**
- **Re-planning:** the AI's own countdown, AIGetPathDistance, as in the game (ai.md §2). **code**

### 3.3 What it shows

Each army is a dot on the overview. **code**
- The army picked in the first list is black.
- The army picked in the second list is white.
- Every other army is in its faction colour.
- Towns, castles and forts are recoloured to their occupier.

Per picked subject (0x58fb2c):
- its 3-row grid of units, with level, XP and four item slots (the item row shows the selected
  unit's);
- active or not;
- HP lost;
- time it is busy for;
- gold and the AI's money sums;
- for a building: its gold stock, income, defence and owner.

Between the two picked subjects:
- their cached AI scores: army against army, the talk counter, army against building;
- if both are on the map, **two predicted battles**, each way round, shown as a pair of
  outcomes. In each, the first army fights as attacker and its unpaid units are weakened (§5).

Overlays for a picked army:
- its remaining route (0x59149c);
- its passability density map (0x591658);
- its flood distances from the targets, in a hue scale (0x591b10). Opening the density map or
  the flood map runs the army's planning.

The mouse readout gives the cell and its value. A click fills a 9×9 table of the values around
the cell. The table has a bug: near the top edge the column start is clamped instead of the row
start (0x592144).

Buttons:
- two buttons open the **battle tester** with the two picked armies, either one as side 1
  (0x591278);
- a debug button computes an army-against-army score and throws it away (0x592f00).

### 3.4 Its reduced event engine

- **When it runs:** only with the events box on, and only on the 24-minute ticks. **code**
- **Which events:** only **global** events (type 1) are checked. The first one that passes
  fires. **code** (0x58e988)
- **Conditions** (0x58deac). An event must not be marked done. It must not have fired in the last
  60 minutes, unless it has a meet army. The time window, the repeat and the once flag apply.
  The checks are:
  - the hero class;
  - the hero's level, gold, squad count and army strength;
  - building owners;
  - units present or absent in the hero's army, or in armies of a faction;
  - artefacts worn by a faction's army;
  - happened or not happened (with the answer);
  - armies defeated by the player or by anyone;
  - armies active or inactive;
  - the meet army;
  - the flag string, without the game's counter stripping and without its tutorial keyword.
  **code**
- **Meet quirk:** a meet event passes only on the step in which that army met the hero. The
  army's "met" flag, however, is set as soon as the other conditions pass, even without a
  meeting. **code** (disassembly 0x58e83a–0x58e869)
- **Repeat quirk:** a repeating event with a period under one day divides by zero.
  **code** (decompile)
- **Results** (0x58d9d8):
  - its message;
  - the flag script;
  - armies activated or deactivated;
  - artefacts lost;
  - starting a relative event;
  - chaining to the next event.
  A victory or defeat event stops the clock. **code**
- **Results not run:** XP, gold, mana, units added or removed, spells, battles, lanterns, show
  army, patrol changes, moves, hero class and delays. **code**

## 4. The editor's battle engine against the game's

The editor's engine runs the game's turn order and pre-simulation, and the game's legal-cell,
damage and AI framework. The pieces sit at 0x4f5544–0x504080, matching game 483680–48bb10.
It differs in these points. **code** unless marked.

1. **No Community rules.** It has none of the Community bonuses: Hunger, Berserk, …,
   PoisonArmorIgnore, FateGift and the rest. It has no Splash, EternalGift, FirstShot or
   Assault. The piercing lists are fixed: ArmorIgnore and both vampire gifts in melee,
   ArmorIgnore and Artillery for shots.
2. **Four extra bonuses, ids 22–25** (names at 0x5baf1c). The game uses these ids for Hunger,
   Berserk, Exhaustion and Drying.
   - **22 OldVampiressGist:**
     - it takes ×2/3 physical damage, like Evasive (0x4f9428);
     - on turn 1, while it still has actions, it cannot be attacked, shot, cursed, healed or
       blessed (0x4f7c98);
     - after a melee hit or a Death strike, its vampiric heal is **not capped** at its max HP,
       and a kill gives it one more action (0x502c58, 0x501ff0).
   - **23 Chatty:** no battle effect found. **unknown**
   - **24 Terrible:** at the start of turn 1, every enemy front-row unit in the column of a
     front-row Terrible unit loses all its actions (end of 0x4f6510).
   - **25 Parrying:** a pass sets a guard flag (0x5012e7). While the flag is up, melee damage to
     the unit is 1. The flag drops at the unit's next action and when it is hit in melee. The
     game still loads a "parry" sound that it never plays (community-patches.md), a trace of
     this rule.
3. **Poison.** A Poison hit with damage above 1 sets the target's regeneration to **−15**
   (0x50293f, 0x502b57). The game uses −20. The editor also skips Undead and Elemental targets.
4. **Nature sets.** Natures: 0 Normal, 1 Undead, 2 Elemental, 3 Rogue, 4 Animal, 5 Hero, 6 People
   (0x5baf84).

   | Rule | Editor | Game |
   |---|---|---|
   | Melee vampirism, Death-strike vampirism, effect of a Life blessing | Normal and Hero targets only | All but Undead and Elemental |
   | Life heal and Life blessing power | 0 on Undead | 0 on Undead and Elemental |
   | Death heal | 0 on Normal and Hero | 0 unless the target is Undead |
   | Life strike | ×2 on Undead | ×2 on Undead, ×3/4 on Elemental |
   | Death strike | ÷2 on Undead | ÷2 on Undead, ×3/4 on Elemental |
   | Elemental heal and strike | heal ÷2, strike ×3/4 | same |

   (0x4f9688; the sets are one-byte constants at 0x5bafa0, 0x5bafac and 0x5bafb0)
5. **AI melee score.** The target's Manevres gets half its actions left added **when it has
   actions left** (0x4fab03). The game's code adds it only for a negative count, which never
   happens (game 486bb9). So the editor's AI values targets that can still act more highly.
6. **Knight.** A knight side takes **90 %** physical damage (0x5bb160, never loaded); the game
   forces 80 %. A side is a knight side when its first unit is the Knight type, set at each
   strength computation (0x4f61f4).
7. **Mana drain.** The amounts come from the ini (§1). The floors are the vanilla ones:
   - Life 15;
   - Elemental 15, except that an Undead or Rogue caster whose power went below zero drops to 0;
   - Death 25 for Undead casters.
   The game's Community block uses its MinSpell values instead (battle.md §1).
8. **Same as the game:** the Life curse divisor (two thirds of CurseMainSpell, recomputed each
   battle), the turn limit, Row2Def, Counterblow, Ghost, DeathCurse, collapse, surrender, the
   threshold turn order with +1 initiative for side 1, and the pre-simulation.

## 5. The editor's world AI against the game's

The AI viewer and the world generator use a copy of the game's world AI (0x57ec1c–0x58925f, game
49fc50–4a548c with the market re-roll 4be178). It is the vanilla code. **code** unless marked.

1. **Unpaid units fight at three quarters.** When an army is built for battle as the attacking
   side, every living unit takes part. An unpaid unit has its melee, ranged and magic attack
   ×3/4 and its base initiative halved (0x58af74). In the game the attacker's unpaid units stay
   out (game 49855c). This applies in the AI's simulated battles (0x580410), the viewer's
   predictions and the viewer's battle buttons. The army window's figure (§6) uses full strength.
2. **No Community economy.** Elementals are not paid in mana, and the hero's gold has no checksum
   at noon (0x5855b4).
3. **Speed.** An army's speed is its map record's speed byte **+ 4** (0x58b9d0). The game uses 5
   minus that byte, and 1 less again for a leader of type 2 (dtm-format.md, army byte 13). The
   viewer's hero has speed 4 whatever the class.
4. **Cost maps** are built from the editor's own cell layers (0x57ec1c), with the game's terrain
   and object cost tables. The build order is the editor's, not the game loader's.
5. **The step bank** is in whole minutes, capped at 200, and fed in 24-minute ticks (§3.2).
6. **Built-in values:** castle and fort income always goes to the owner, and a hero gets 100 % of
   castle income (§1).
7. **Stats.** The editor's stat recomputation (0x58a064) lacks the game's guard against item ids
   past the item count, and lacks the game's HP rescaling. **code**
8. **Promotions** (0x585f08). For the two fixed-pick types the editor's second choice is the
   upgrade option 2 where the game uses option 3 (game 4a4a7c):
   - 0-based type 3: a roll of 0 out of 3 gives option 1, otherwise option 2 (game: option 3);
   - 0-based type 7: a roll of 0 out of 3 gives option 2 (game: option 3), otherwise option 1;
   - every other type redraws option 1–3 until it hits a filled one, as in the game.
   The rolls use the editor's own generator (fixed seed in the AI viewer, §3.1). **code**

## 6. The army window's strength figures

The army record editor recalculates two pairs of numbers whenever a unit, level, artefact or
spell changes (0x547070). **code**
1. It builds a scratch army from the leader and every troop unit, one per head. More than 12
   units fails, and the last count change is undone.
2. It recomputes the stats, gives the three artefacts to the leader, and puts the army spell on
   every unit.
3. It shows **cost / upkeep**:
   - cost = the sum of the types' gold Cost over all units;
   - upkeep = the sum of Cost ÷ CostRecrutDiv over every unit except the leader, each term
     rounded down (CostRecrutDiv is 2 in the shipped ini).
4. It shows **tactical / side strength**:
   - tactical = the sum of each unit's tactical cost on its current stats, artefacts and spell
     included, with no building (the battle value, experience.md §1);
   - side strength = the battle side built at full strength, auto-arranged, then rated with the
     side-strength formula (row weights, the one-unit rule).
5. Saving the army writes the tactical sum and the side strength into the army record (bytes 6
   and 74, each capped at 65000; 0x544dac). The game never reads them (dtm-format.md).

The tactical cost formula matches the game's (experience.md §1), but there are two differences:
- the editor's copy has no Community hook, so a stored zero or negative value is not adjusted;
- whether the bonus switch covers ids 22–25 is **unknown**, because that part sits in
  floating-point code that was not read.

## 7. Razdor editor now → original

| Topic | Razdor now | Original | Status |
|---|---|---|---|
| Battle tester window | toolbar: Battle test (`src/editor/tester.rs`, `src/ui/editor/tester.rs`), made once per session; the AI's army on top, the player's below, the right panel | §2.1 | matches (Razdor's layout) |
| Type catalogue | the session's unit types by number, level-0 stats, strength field = gold `Cost`, surrender kept | §2.2 | matches |
| Building an army | click removes or adds only while the start button is enabled; later units move down; cost total; Clear, Swap; Redraw not needed (Razdor redraws every frame) | §2.3 | matches |
| Random armies | budget Tag·1000+100 per side, the one-in-nine nature roll and its three sets (People is the editor's own nature, outside the first set), Cost ≥ 10 and < budget left, stop at ≤ 100 or 12 units, auto-arrange by cost; seeded from the clock | §2.3 | matches; where no type can be drawn the original hangs, Razdor stops that army and says so |
| `Battle.Sav` | sections `Army1`/`Army2`, keys `U<r><c>` with the type number or 0, in Razdor's editor folder; load rebuilds catalogue units row then column; a missing file is reported | §2.3 | matches; a number past the catalogue leaves its cell empty (the original copies an empty template) |
| Rule switches | the five switches, defaults 1, 0, 1, 1, 0, OK/Cancel, session-wide (the AI viewer's battles use them too) | §2.4 | matches |
| Running a battle | Super AI = mode 1 (side 1 gets the full killable test), else 0; Super AI ticks all-AI; side 1 waits for a legal click unless all-AI; 500 ms delay with the delay box; Stop/Esc ends at once; the setup is replayed | §2.5 | matches; the pointer is not moved, the actor's and the AI target's cells are marked with an arrow |
| Figures and closing message | cost, side strength, remaining value (16-bit), the bands of `round(2·S/S')` half to even, the turn-limit refusal, only with the delay box; diagnostic line with the turn, the XP pool and HP lost against predicted | §2.6 | partly: the diagnostic line leaves out the two figures whose meaning is unknown (§8); a side of strength 0 gives no message where the original's division fails |
| Battle engine | `Rules::Editor(switches)` of Razdor's battle engine (`src/rules/battle/editor.rs`): every point of §4, the pull and the long-strike-off retreat scores included; the game's battles keep `Rules::Game` | §4 | matches; a pull onto an occupied cell is not offered (it would put two units on one cell); Chatty does nothing |
| Army window figures | cost / upkeep and tactical / side strength with artefacts and spell, bytes 6 and 74 written on save (`src/editor/records.rs`) | §6 | matches |
| AI viewer window | toolbar: AI view (`src/editor/viewer.rs`, `src/ui/editor/viewer.rs`), new at every opening, list picks kept | §3 | matches (Razdor's layout) |
| Opening and seed | a `Game` of the map under `EditorAi` (`src/rules/ai.rs`), the generator set to 1 after the load, or from the clock with the seed box (at the next opening); clock at the header's start time; income totals | §3.1 | matches; the market roll at the load is the game loader's |
| Hero | none, or a class whose preset has a position, before time runs: an army the AI steers (preset troops above type 3, items worn, faction 1, aggression −20, speed 4, start buildings the hero's) | §3.1 | matches |
| Time | 6-minute steps, the 1/10/60-step buttons, run and stop; a 24-minute AI tick banks 24 minutes up to 200, whole minutes | §3.2 | matches |
| Event engine | global events on the ticks with the events box; the conditions and results listed; meet only on the step of the meeting, the army marked met anyway; the 60-minute rule; chains; victory and defeat stop the clock | §3.4 | matches; a repeat under a day (the original divides by zero) and an event firing again within one scan (endless in the original) stop the clock with a message; the hero's strength condition uses the summed tactical costs |
| Overlays, grid, predictions | route, density (the plan's multiplier map) and flood distances, planning the army; mouse readout; 9 × 9 grid with the top-edge clamp bug; two predicted battles; battle buttons; debug score | §3.3 | matches |
| World AI copy | `EditorAi`: editor battle rules with the switches, unpaid units ×3/4 attack and ½ initiative on the attacking side, speed byte 13 + 4, no mana wages, the editor's promotion picks | §5 | partly: the editor's stat recomputation (no item-id guard, no HP rescale) and its own cost-map build order are the game's |
| Game unchanged | the game's battles and world AI run as before; their tests pass unchanged | — | — |

## 8. Unknowns

- What bonus 23 (Chatty) does, if anything.
- Whether the tactical cost formula's bonus switch has cases for ids 22–25.
- The meaning of the three end-of-battle figures in the tester's diagnostic line.
- The missing-key default the editor uses when it reads `_Global.ini`.
- Whether the enabled test in the tester's click handler is exactly "start button enabled" (read
  through a virtual slot).
