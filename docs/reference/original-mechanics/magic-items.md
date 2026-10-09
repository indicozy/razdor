# Discord Times: spells, items and potions (from the Community exe)

This file covers world spells (the spell book, learning, casting, every effect key of
`Rus_Spells.ini`, durations and stacking) and items (every `Rus_Artefacts.ini` key, the wear
rules, the stat rebuild, the pack, potions, and how the AI hands out items). The source is the
Community Update (Unstable) `DiscordTimes.exe` of the player's install (Delphi, image base
0x400000), read statically. The rules are in our own words. Addresses are virtual addresses in
that build, given as evidence only. Battle-time magic of units (mage blessings, curses and
strikes) is in [battle.md](battle.md); prices and village offers are in [economy.md](economy.md).
This file supersedes the spell and item parts of economy.md §4–§5 where they differ.

Confidence tags:
- **code**: read in the code.
- **data**: consistent with the data files or help, not traced in code.
- **unknown**: not determined.

Conventions used below:
- "now" is the game clock in hundredths of a minute, counted from the map start (0 when a map
  loads, 0x4b2504). One hour is 6000 of these.
- Unit HP −1 means unhurt, 0 means dead, a positive value is a wounded unit.
- Integer division truncates toward zero. "Round" is Delphi rounding (halves to even).
- Spells are numbered by their order in `Rus_Spells.ini` (1 = first section). Items and unit
  types are numbered by `GlobalIndex`. Internally a unit stores its type as GlobalIndex − 1
  (the record table is indexed that way at 0x4e05a3); this matters in §5.3.
- No random numbers are drawn anywhere in this subsystem (casting, effects, potions, wear tests,
  AI equipping). The only rolls nearby are the village offers (economy.md §3) and AI promotion
  (experience.md). **code**

---

## 1. Data records

### 1.1 Spells (`Rus_Spells.ini`, loader 0x49ac64, list 0x4e0c60) — code

- A section is loaded only if it has `Name`, `Type` and `CostGold`; spells are numbered in
  section order. Empty integer keys read as 0.
- Keys and their meaning:

| Key | Values | Use |
|---|---|---|
| `CostGold` | int | price in a spell shop |
| `CostMana` | int | mana per cast (§3.3) |
| `Type` | Life / Death / anything else = Elemental | **not used by any rule**: the value is stored (0x49ae98) and never read again |
| `TimeWork` | hours | duration (§4.2) |
| `TimeCast` | hours | casting time (§3.3) |
| `Target` | `Enemy`, `OneEnemy`, anything else = Hero | own army or an enemy army (§3) |
| `DeltaFixedHits` | int | instant HP change (§4.1) |
| `DeltaPercentHits` | int | instant HP change in percent (§4.1) |
| `p-LifeLose` | int | permanent max-HP drain or its cure (§4.1) |
| `d-Hits`, `d-AttackBlow`, `d-DefenceBlow`, `d-AttackShot`, `d-DefenceShot`, `d-MagicPower`, `d-Initiative`, `d-Manevres` | int | flat modifiers while the spell lasts (§6) |
| `p-Hits`, `p-AttackBlow`, `p-DefenceBlow`, `p-AttackShot`, `p-DefenceShot`, `p-MagicPower`, `p-Initiative`, `p-Manevres` | int | percent modifiers while it lasts (§6) |
| `p-ProtectLife`, `p-ProtectDeath`, `p-ProtectElemental`, `p-Regen`, `p-Vampirizm` | int | **points** added while it lasts (§6) |

- Keys the loader never reads: any `f-` key, `d-ProtectLife`, `d-ProtectDeath`,
  `d-ProtectElemental`, `d-Regen`, `d-Vampirizm`. `Icon1–3`, `ColorC1–3` and `Effect1–3` are
  visuals only.
- The visual effects give each spell a real-time length: the latest end (start + duration) of
  its three effects, in milliseconds (stored at record +0x26b by 0x4e0c60). §3.4 uses it.

### 1.2 Items (`Rus_Artefacts.ini`, loader 0x4e0ab8 → 0x498eb4, 0x4990b6, 0x4993a8, 0x4995c8) — code

- The record slot is `GlobalIndex`. A section is parsed only if `Icon`, `Name`, `Type` and
  `Cost` are present. The item count goes up by one per parsed section; worn item numbers above
  count + 1 are removed from units at every stat rebuild (0x4908a8).
- `Type`: BlowWeapon 0, ShotWeapon 1, Armor 2, Helm 3, Shield 4, Staff 5, Amulet 6, Ring 7,
  Potion 8, Item 9. **An unknown or empty string means Potion.**
- `Bonus`: the unit bonus token (same enum as the units file, Community tokens included). It
  overrides the wearer's bonus byte (§6, battle.md §7).
- `Magic`: LifeMagic 1, ElementalMagic 2, DeathMagic 3, else none. It **restricts who may wear
  the item** (§5.3); it does not change the wearer's school.
- `f-` keys read: Hits, AttackBlow, DefenceBlow, AttackShot, DefenceShot, MagicPower, Initiative,
  Manevres, ProtectLife, ProtectDeath, ProtectElemental, Regen, Vampirizm.
- `d-` keys read: Hits, AttackBlow, DefenceBlow, AttackShot, DefenceShot, MagicPower, Initiative,
  Manevres. (No `d-` protections, regeneration or vampirism.)
- `p-` keys read: the same 13 as `f-`.
- Community tripwires: a `Cost` of exactly 32000 (0xc26c18) or a `d-Manevres` of exactly 4
  (0xc26c08) makes the loader loop forever. **code**

## 2. The spell book and learning — code

- The book is a list of up to 256 spell numbers with a count (0x68e0e8, 0x68e4e8). The magic
  window shows and casts only the **first 15** entries (0x4d261c, 0x4dad08: 3 columns × 5 rows).
- Adding a spell (0x49c144) does nothing if it is already known; otherwise it is appended.
  **There is no limit in the add itself.**
- Spell shop (0x4ba078 select, 0x4ba3b0 buy): the buy button is offered only when the spell is
  not known, gold ≥ `CostGold`, and the count is not exactly 15. Buying adds the spell and takes
  exactly `CostGold`. No relation factor, no Merchant discount.
- Other sources, which skip the 15 test: the hero class preset at map start (up to 6 spells,
  0x4b2504), and event results "learn spell" (up to 4, 0x4a9b75). So an event can push the book
  past 15; the extra spells are known (the shop says "already known") but can never be cast.
  The Community build routes the event add through a hook that skips it entirely when the
  event's byte +0x94 (the "no meeting" / Community switch, events.md) is 1 (0xc28604).
- Community opcode 16 removes spells from the book and closes the gaps but does not lower the
  count (map row 0xc28590), so the "15 = full" test of the shop keeps counting the removed ones.

## 3. Casting on the world map — code

There is no hero casting in battle: the only route to a spell's effect is the world-map cast
timer (0x4af2f8, the only caller of the effect routine 0x4900fc). The AI never casts world
spells.

### 3.1 Starting a cast (book click, 0x4c2e34)

1. The clicked book entry gives the spell. If the hero's mana is below the **mana cost**
   (§3.3) nothing happens (hovering shows a "not enough mana" hint, 0x4c2cf4).
2. Otherwise the book closes and casting mode starts.
3. A `Hero`-target spell runs at once, as three queued steps: the camera centres on the hero
   (only if it is more than 300 px away, 0x4afa98), the **casting wait** (§3.2), then the
   **effect animation** at the hero's cell (§3.4).
4. An `Enemy` or `OneEnemy` spell first asks for a target (§3.5). The click on a valid target
   queues: the casting wait, the camera move to the target, the effect animation on the target.

### 3.2 The casting wait (0x4ae280, 0x4ae42f, Community 0xc27802)

- It lasts `n` steps of 30 game minutes, where `n` is the **time cost** of §3.3. Each step takes
  the walk delay in real time (0x68dc84), and the clock runs smoothly between steps.
- `n = 0` ends at once.
- The world keeps running: AI armies move, and the event scan runs after every step.
- The noon pay and report are held back while a cast is in progress (0x4abfbc tests the cast
  flag 0x66c0b9).
- **An event that fires during the wait ends the wait early**, and the spell then lands at once
  (the wait is simply popped from the queue, 0x4ae4f2–0x4ae50f).
- If that event casts a spell (§3.6), its cast sets the target code (0x68eca4) to 1, "an event's
  cast", and the hero's spell reads that code when it lands (§3.4): it hits **the player's own
  army** whatever its `Target`, no mana is paid, and an own-army spell gets the event's
  10× / 5× duration. The event's spell, queued behind it, lands next.
- **An enemy that reaches the hero** starts a battle; that path (in World_AdvanceAI 0x4ade3c)
  clears the cast flag and every queue, so the spell is lost and no mana is paid.
- **The enemy target leaving the map or being destroyed** during the wait cancels the cast, again
  with no mana paid (0x4ae536). There is no other check: the target may walk anywhere meanwhile;
  the spell still lands on it.
- While a spell is in progress the world map ignores input (all input is gated on 0x68dc62).

### 3.3 Mana and time cost (Community 0xc27448)

Let `d` = 2 for the Archmage hero class, 1 otherwise (0x68e4f0, set at map load and on loading a
save). For a value `x`:
- If `d = 1` and the Caster flag is set: `floor(4·x / 5)` (an exact integer multiply by
  0xcccccccd).
- Otherwise: `x div d`.

Mana cost = that function of `CostMana`. Time cost (half-hour steps) = that function of
`2 × TimeCast`. The spell card shows the function of `TimeCast` as hours (0x49bbe0, no
doubling), which can be half an hour shorter than the real wait: with the Caster discount, and
also for the Archmage whenever `TimeCast` is odd (the card shows `TimeCast div 2` hours, the
wait is `TimeCast` half-hours).

**Archmage and Caster never stack**: the Archmage halves and ignores Caster.

**The Caster flag is unreliable in the original.** It is a single global byte
(0xc25cf1). It is cleared each time the totals of an army with at least one unit are recomputed
(0x4a16d4 via 0xc2743c, once before the unit loop) and
set whenever the strength of a unit whose bonus is Caster is computed (0xc25cc9). The recompute
covers every unit, dead ones included, using both the type bonus and the item bonus. The AI's
item evaluation also computes strengths and can set it. So the flag says "a unit with Caster was
in the last army recomputed (or evaluated since)", which is often an AI army. The intent is
clearly "the hero's army has a Caster". **code**

### 3.4 The effect and the mana payment (0x4af2f8)

- The effect animation plays in real time. When the time since its start passes the spell's
  visual length (§1.1), the effect routine (§4) runs on the target:
  - `Hero` spells: the player's army.
  - Other spells: the army chosen in §3.5; for an event cast, see §3.6.
- **Mana is paid only now**, and it is the mana cost recomputed at this moment. It is
  subtracted with **no check and no floor**: if mana dropped meanwhile it goes negative.
- Event and village casts pay nothing.

### 3.4a How the effect is drawn (0x4e0c60, 0x4b911c, 0x4af2f8, renderer 0x4c8864 / 0x4c9b5b)

- `EffectN=file,R,G,B,length,Y,scale×1000,start` (lengths and start in ms). The loader 0x4e0c60
  keeps `length` at effect +0xc1 and `start` at +0xc5, and builds a 128×128 quad (0x48c4c0,
  the whole texture) whatever the frames' size. 0x48c6b0 moves it by (−64, Y − 128), the
  scale (field 7 / 1000) multiplies it and (16, 11) is added. The renderer adds the target
  cell's top-left. So the frame is **centred** on the cell's centre and its **bottom row is
  Y·scale px below that centre**. A larger Y draws lower. **code**
- Colour: every vertex's diffuse is `R·65536 + G·256 + B` (0x48c5b4; alpha 0, specular 0).
  The device (D3D7, init 0x481cd0) never sets stage 0's COLOROP, so it keeps D3D's default
  MODULATE: texel × diffuse. Each layer is drawn with **SRCBLEND = DESTBLEND = ONE** (pure
  additive; layer 1 0x4c92ec/0x4c93e0, layer 2 0x4c9e63/0x4c9e72, layer 3 0x4ca1a1/0x4ca1b0).
  It is then put back to SRCALPHA / INVSRCALPHA. The min filter is LINEAR during the draw.
  So each pixel is `below + texel × (R, G, B) / 255`, saturated. Black adds nothing, and the
  layer can only brighten the map, never darken it. **code**
- Frames: 0x4b911c reads 50 frames of 128×128 16-bit pixels per layer (+4-byte header each)
  into the textures at 0xae28f8 + layer·200. Each pixel goes through the format LUT 0xae6f60,
  then through 0xae2b50 (RGB444 with each nibble ×17). The art's alpha nibble is 0. **code**
- Timing: the timer copies the three quads to 0x68ea90 / 0x68eb30 / 0x68ebd0 on its first
  call and marks the cell (+0x10 = spell + 1). On every tick with t = ms since the cast,
  layer e is shown iff `start ≤ t ≤ start + length` (flags 0x68eca8..aa), and frame
  `(t − start)·49 div length` is uploaded (0x482290). Frame 49 shows only at the very end.
  **code**
- Order: layer 1 is drawn in the cell pass after the hills, before that cell's mark and route
  arrow (0x4c935b), under every standing sprite. Layers 2 and 3 are drawn in the sprite pass
  of the target cell, layer 2 before the cell's armies (0x4c9dca) and layer 3 after them
  (0x4ca108). So `Effect2` is behind the figure and `Effect3` in front of it. **code**

### 3.5 Choosing an enemy target (world frame 0x4cc148, hit test 0x4cbf20)

- In target mode, hovering any cell that holds an army (not the hero, not the ship) on a
  **valid cell** makes it the target and shows its name. A valid cell is on the map area (not
  over the minimap, above the bottom bar), **explored**, and passable for the hero's current
  movement map.
- **There is no distance limit and no line-of-sight test.** The army's code is stored in its
  cell whether or not the army is currently visible, so an army under fog on explored ground can
  be targeted. Armies inside a building are not in the cell layer and cannot be targeted.
- An army is refused when it both has a meeting event (army byte +0x3826) and a positive
  attitude to the player (+0x16af ≥ 1): friendly talking armies. Any other army, friendly or
  not, can be cursed.
- A click on a valid target starts the cast. Right-click or Esc leaves target mode (0x4cd016).

### 3.6 Spells cast by events and villages (0x4ab1ec tail)

- The event's spell number is clamped to the spell count. No wait and no camera move: only the
  effect animation at the hero's cell, then the effect.
- **The target is always the player's army**, whatever the spell's `Target`: an event that casts
  an `Enemy` spell curses the player's own army (with its normal duration).
- Free of mana.
- A `Hero` spell cast this way lasts **10 × TimeWork** hours, or **5 × TimeWork** when
  `TimeWork` ≥ 8 (§4.2).
- Village offers use the same path (economy.md §3: the priest's spell 1 and the blessing
  3 + 2·Random(5)).

## 4. What a spell does to an army (0x4900fc) — code

### 4.1 Per unit

Which units:
- `OneEnemy`: only the first unit of the army (the leader; the hero for the player's army).
- A spell with a non-zero `p-LifeLose`: only the first unit, whatever its target.
- Otherwise every unit of the army, dead ones included in the loop (but see below).

For each chosen unit, in order:
1. Expired spell slots are emptied (end time ≤ now).
2. The unit has **4 spell slots**. If the same spell is already in a slot, that slot is used
   (recast). Otherwise the first free slot. With neither, **nothing at all happens to this
   unit**, not even the instant HP change.
3. Recast: the end time grows by `TimeWork` hours (time left is kept). New: the slot gets
   the spell and an end time of now + `TimeWork` hours.
4. The instant part is applied, on a new slot only if the unit is alive. The recast branch has
   **no HP test**: a dead unit that still holds the spell gets the instant part too, so a
   positive `DeltaFixedHits` or `DeltaPercentHits` raises it from 0 HP. Spells and battles of
   the player strip dead units' slots (§4.3, §9), so this needs a unit that died some other
   way while holding the spell; whether that happens in play is **unknown**.
   - HP −1 (unhurt) becomes the current maximum first.
   - `p-LifeLose` < 0: the unit's drain percentage D (a permanent unit field, 0 on a fresh unit)
     becomes `100 − (100 − D)·(100 + L)/100` (integer), so repeated drains compound
     (−20 twice gives 20, then 36). HP goes down by `HP·|L|/100` (truncated) now.
   - `p-LifeLose` > 0: D goes down by L, not below 0. HP is not changed now.
   - HP += `DeltaFixedHits`.
   - `DeltaPercentHits` > 0: HP += `DeltaPercentHits × current maximum / 100`.
     `DeltaPercentHits` < 0: HP += `DeltaPercentHits × HP / 100` (a share of what it has).
   - HP below 0 becomes 0 (dead); HP at or above the current maximum becomes −1 (unhurt).
     Exactly 0 is also dead.
5. Event and village casts of a `Hero` spell on the player's army then **reset** the end time
   to now + 10 × `TimeWork` hours (× 5 when `TimeWork` ≥ 8). On a recast this replaces the
   accumulated time instead of adding to it.

### 4.2 Durations

- `TimeWork` hours exactly, with no scaling by any level (the ini comment about caster level
  has no counterpart in code).
- `TimeWork` 0 or empty: the slot ends at once, so the spell has only its instant part. (It
  still takes a free slot during the cast; with 4 active spells a 0-hour heal does nothing.)
- 9999 is not special: it is 9999 hours.
- A spell's modifiers count while its end time is in the future. They show and act from the
  next stat rebuild (§6). Rebuilds happen whenever the army changes, at every battle start, at
  the noon wage recount and whenever a unit card is drawn; an expired spell is removed at the
  next one.
- The drain percentage D is a unit property, not part of the slot: it stays after the spell's
  slot runs out, until a positive `p-LifeLose` lowers it.
- Community opcode 11 sets slots with the end time 15,658,734 (about 108.7 game days after the
  map start): an absolute time, so these "permanent" spells do run out then (map row 0xc27862).
  **code** (read in the map, not re-read here)

### 4.3 After the units

- Every dead unit of the army loses all 4 spell slots.
- If some unit of the army is alive and **the player's hero** has 0 HP, the hero is set to 1 HP.
  (The test is on the player's hero whatever army was hit; in practice it matters only for the
  player's own army.)
- Some unit alive: the army is recomputed (§6), which applies the new modifiers and D.
- An enemy army with nobody alive is destroyed (0x496834) as beaten by the player: **no loot and
  no battle XP** are given.
- The player's army with nobody alive: **game over** (the defeat sequence, 0x4af658).
- Note the **double LifeLose cut**: the HP was cut by L% at step 4, and the rebuild then cuts
  the maximum by the new D and rescales HP proportionally (§6 step 12). An unhurt unit of 100 HP
  hit by L = −20 ends at 64 of 80, not 80 of 80.

## 5. Items: where they live and who may wear them — code

### 5.1 The pack

- 256 slots (0x68dce0), each 0 or an item number. Slots keep holes when an item is taken out;
  the grid shows 5 × 5 cells and scrolls by rows.
- Adding an item (events, loot, purchases; 0x49a85c) uses the first empty slot. **If the pack is
  full the item is lost without a message.**
- Removing an item by number (events "lose item", 0x49a8a8) takes the first match in the pack,
  else the first match among the worn items of the player's units, in army order.
- The AI armies have a separate 12-slot pack (army +0x37b0).

### 5.2 Equipping in the original's screens

- **Hero window** (0x4c24f4): the pack plus the 4 worn slots of the unit selected in the army
  window (the hero when none). Clicking picks up, puts down or swaps:
  - picking from a worn slot is always allowed (a "locked slots" count at unit +0x18 exists in
    the code but nothing ever sets it above 0); items can also be taken from dead units;
  - putting an item on an **empty** worn slot runs the wear test (§5.3) and puts the item in
    **the clicked slot**;
  - an occupied worn slot does not swap;
  - pack cells swap freely.
  Potions cannot be put on a worn slot.
- **Army window** (0x4c346c): dropping the held item on a unit's card "gives" it
  (0x4979c4):
  - a potion is drunk (§7);
  - for the **hero**, any other item goes back to the pack (the hero equips only through the
    hero window);
  - for another unit, the wear test runs and the item goes to the first free slot;
  - a refusal leaves the item on the cursor.

### 5.3 The wear test (0x49765c)

Checks in this order; the first that fails decides:
1. No item → refused.
2. **The crown (item 154)** on a unit with wage kind ≠ 0 (not the hero, not an army leader):
   allowed only if the unit's internal type index is one of 1, 2, 3, 11, 13, 15, 36, 42, 45, 46,
   48, 49, 53, 56, 58, 69, 70, 72, 73, 77, 89, 97, 99 (bit set at 0x4979a4, covering indexes
   0–103; any index ≥ 104 is refused without a lookup). Because the index is GlobalIndex − 1, the unit types that can actually wear it are
   **GlobalIndex 2, 3, 4, 12, 14, 16, 37, 43, 46, 47, 49, 50, 54, 57, 59, 70, 71, 73, 74, 78, 90,
   98, 100**. Read as GlobalIndex the listed numbers name the knights, royals and undead lords
   of the data, so the original very likely meant those, with an off-by-one in the test. Parity
   means following the code.
3. A **dead** unit refuses everything except a potion with `f-Hits` ≥ 1000.
4. **Potion**: not worn (the army window drinks it instead). **Item** type: never worn.
5. **Shield** on a unit whose level melee attack is 0 → refused.
6. **Holy items** (12, 46, 59, 72, 73, 74, 75, 76, 77, 85, 94, 120, 131; bit set 0x4979b4 by item
   number) on a unit whose **Nature is Undead** → refused. This is the type's `Nature` key, not
   the Dead/FastDead bonus: in the shipped data 23 unit types are Undead.
7. **Class**: a BlowWeapon needs level melee attack > 0; a ShotWeapon needs level ranged attack
   > 0 and the type's base `AttackShot` not above `ShotWeaponRange`; a Staff needs level magic
   power > 0 (which also requires a magic school).
8. **School**: an item with a `Magic` school is worn only by a unit of the same school.
9. **One weapon**: BlowWeapon, ShotWeapon and Staff are all weapons; a second one is refused.
10. **One of each type**: an item of a type already worn is refused.
11. **Free slot**: the lowest empty of the 4 slots, else refused.

"Level" values are the unit's stats from its type and level, without items (§6 step 1).

## 6. The stat rebuild (0x4908a8) — code

Run for one unit; it rebuilds the current stats from scratch. Order:

1. **Level stats**: each base stat plus its per-level gain × level for Hits, attacks, defences,
   magic power (only if the type has a school), Initiative, Manevres; the protections,
   regeneration and vampirism use the percentage-per-level rule (experience.md §2).
2. Remember the previous maximum HP (the old current value, or the level value if 0).
3. Drop worn items whose number is above item count + 1; empty expired spell slots.
4. **Worn items' `f-`**, slot by slot (0 to 3): each non-zero, positive value **replaces** the
   stat (Hits, attacks, defences, magic power if the unit has a school, Initiative, Manevres,
   the three protections, regeneration, vampirism). A later slot wins.
5. **Potion `d-`** (if a potion is active): added.
6. **Items' `d-`**, slot by slot: added.
7. **Spells' `d-`**, slot by slot, for spells still running: added.
   (In steps 4–7 and 9–11, magic power is touched only if the unit has a school.)
8. Initiative and Manevres are multiplied by 100 (kept in hundredths from here on).
9. **Potion `p-`**: Hits, attacks, defences, magic power, Initiative, Manevres become
   `x + x·p/100` (truncated); the three protections, regeneration and vampirism get **p points
   added** (positive or negative).
10. **Items' `p-`**, one item at a time, the same way: percents compound item by item, points
    add.
11. **Spells' `p-`**, one spell at a time, the same way.
12. **Drain**: if D > 0, maximum HP loses `max·D/100` (truncated).
13. Initiative = hundredths / 100, truncated. Manevres = hundredths / 100 truncated when above
    the level value × 100, otherwise `(hundredths + 50) / 100` (rounded half up). Both not below 0.
14. **HP follows the maximum proportionally**: for a wounded unit,
    `v = newMax × (HP + carry) / oldMax` in single-precision floating point; HP = the integer
    part, carry = the fraction (kept on the unit for next time); HP 0 becomes 1. An unhurt unit
    stays unhurt, a dead one dead. If HP ends above the new maximum it becomes unhurt.
15. A unit whose level melee attack, ranged attack or magic power is 0 has that current value
    forced to 0 (items cannot give a non-shooter a shot).
16. Protections are clamped to 0..99; regeneration and vampirism only to at most 99 (regeneration
    may stay negative).
17. A hero unit (types 1–3 by GlobalIndex) at exactly 1 HP has Initiative 1.
18. **Bonus**: the unit's bonus byte is the type's; each worn item with a bonus overwrites it,
    a later slot wins (battle.md §7).

Not clamped: Hits, attacks, defences and magic power can go below 0 from penalties (the battle
formula clamps defence at use, battle.md §0).

## 7. Potions — code

### 7.1 Drinking (army window drop → 0x4b11cc animation → 0x48fdd0)

- Only outside battle, from the army window, by dropping the potion on a unit card. The potion
  is used up. The AI never drinks potions.
- **HP**: a living unit (unhurt counts as full) gains `f-Hits`; below 1 it dies; at or above
  the maximum it becomes unhurt. A **dead** unit drinking a potion with `f-Hits` ≥ 1000 comes
  back with `max × f-Hits / 10000` HP (truncated; 10% for 1000; it stays dead when
  `max × f-Hits` < 10000). A dead unit cannot drink any other potion (the wear test refuses it,
  §5.3). A living unit with such a potion is healed by `f-Hits` like any other (full unless its
  missing HP exceeds `f-Hits`).
- **Lasting part**, added into the unit's potion block (several potions accumulate):
  - `d-` Hits, attacks, defences, Initiative, Manevres: added.
  - `p-` Hits, attacks, defences, Initiative, Manevres: added (they act as percents in §6 step 9).
  - **`d-MagicPower` and `p-MagicPower` of a potion never take effect**: the drink only stores
    them when a school byte of the potion block is set, and nothing ever sets it.
  - Protections, regeneration, vampirism: an `f-` value **replaces** the stored value, then
    the `p-` value is added; these are points (§6 step 9).
  - The potion counts as active only if one of these lasting values is non-zero; a pure heal
    leaves nothing behind.
- Then the unit is rebuilt (§6), so a `p-Hits` potion's bonus to the maximum scales the HP
  proportionally.

### 7.2 Duration

- The potion block is cleared after **every battle the player fights**, won or lost, for the
  player's army only (0x4c50ec → 0x490720), followed by a rebuild. Nothing else clears it: it
  is not time-based.

## 8. Items and the AI — code

- **Starting items** of map armies and garrisons (0x4a273c): the item is tried on each unit in
  turn; its value is the unit's tactical cost with all its items minus its cost with none
  (experience.md §1, mode 2). It goes to the unit with the highest positive value (the first one
  on a tie), else into the army's 12-slot pack (lost if that is full).
- **Redistribution** after AI battles and promotions (0x4a473c): all loose items (the army pack
  and a shared pool) are matched repeatedly: every unit × item pair is valued as above, the best
  pair with value above 5 is equipped, until no pair qualifies. Leftovers fill the pack by
  highest absolute price (up to 12; an item with `Cost` 0 is never picked, since the pick needs
  a strictly positive absolute price); the rest stay in the shared pool.
- **Shopping** (AI arrival, around 0x4a6000): the AI first sells its pack (economy.md). For each
  market item and unit that can wear it and gains tactical cost, the value is as above and the
  price is the relation price of `Cost`. It buys the best pair worth more than 5 that it can
  afford, equips it, and repeats. Potions and trade goods are never bought this way.

## 9. Other spell and item details — code

- After a battle the player's dead units lose their spell slots (0x4c50ec).
- At a campaign map change every unit's 4 spell slots are wiped (0x4b5b64). The drain
  percentage D is part of the unit and is carried. (Whether a still-active potion is carried is
  not checked.)
- Unit cards show an icon for each running spell whose `CostMana` > 0, with the time left in
  the hover hint (0x493a64).
- An item's `Cost` below 0 makes it unsellable; selling and buying are in economy.md.

---

## Razdor now → original

Razdor's code read: `src/rules/items.rs`, `src/rules/magic.rs`, `src/rules/units.rs`,
`src/rules/town.rs` (`learn_spell`), `src/rules/script.rs` (event spells and items),
`src/rules/game.rs` (equip, drink, after battle), `src/rules/battle.rs`,
`src/ui/items_view.rs`, `src/ui/spellbook.rs`.

| Topic | Razdor now | Original | Section | Status |
|---|---|---|---|---|
| Spell slots | 4 slots on every unit and troop (`Unit::spells`); a unit with no slot for the spell is skipped, instant part included (`magic::spell_on_unit`) | 4 slots on every unit; full slots skip that unit entirely, instant part included | §4.1 | Matches |
| Enemy target range | Any army on an explored cell a click can target, outside buildings, at any distance; friends too unless they have a meeting event waiting and attitude ≥ 1 (`Game::can_curse`, `EventEngine::meeting_waiting`) | Any army on an explored, passable cell, no distance, visible or not; friendly armies may be cursed unless they are "talking" friends | §3.5 | Matches |
| Target moves during the cast | Lost only if it leaves the map (removed from it, destroyed or deactivated) | Lost only if it leaves the map or is destroyed | §3.2 | Matches |
| Mana short at the end | The cost, worked out again when the spell lands, is subtracted with no check; mana may go negative | Mana is subtracted anyway and can go negative | §3.4 | Matches |
| Event during the cast | An event that fires in a casting step ends the wait; the spell lands at once | The wait ends early and the spell lands at once | §3.2 | Matches |
| Event spell during the cast | The hero's spell lands on its target, paid, then the event's spell (`Game::end_reading`) | The event's cast leaves the target code at 1, and the hero's spell lands on that code: his own army, unpaid, with the event's duration (bug); the event's spell lands after it | §3.2, §3.4 | Razdor fixes the original's bug |
| Noon during the cast | Held back while a spell is read (`Game::cast` reads too) | Held back until the cast ends | §3.2 | Matches |
| Caster discount | Any unit of the hero's army, dead or alive, with Caster as its type's or its current (item) bonus; `floor(4x/5)`, a negative value keeping its sign (the original's unsigned multiply made it huge: Razdor fixes the original's bug) | A global flag from the last army recomputed (often an AI army); dead units count | §3.3 | Differs: the global flag's call points are not mirrored |
| Displayed cast time | The card shows the cost function of `TimeCast` in hours (`card_cast_hours`) | The card shows the function of `TimeCast`, which can be 30 min short with Caster, or for the Archmage with an odd `TimeCast` | §3.3 | Matches |
| Event `Enemy` spell | Hits the hero's army with its normal duration | Same | §3.6 | Matches |
| Event spell number | Held to the number of spells | Clamped to the spell count | §3.6 | Matches |
| Event recast duration | Resets to now + 10× (5×) | Resets to now + 10× (5×) | §4.1 | Matches |
| When an event's spell lands | After every other result of the event (units it added included), before its delay passes | Queued by the results, cast in the event's finish before the delay's wait | §3.6 | Matches |
| Spell killing the whole player army | The hero gets 1 HP only if someone else survives; otherwise the game is lost (`Game::army_fallen`) | The hero gets 1 HP only if someone else survives; otherwise game over | §4.3 | Matches |
| Hero at 0 HP after a spell | The first unit of the army hit (the hero, a leader) set to 1 before the rebuild (so it follows its maximum) | The same test on the player's hero for any army hit, before the recompute (bug: a curse on an enemy raises him) | §4.3 | Razdor fixes the original's bug |
| Spell destroying an enemy army | Beaten by the player, no loot, no XP; its dead stay in the record until then | Destroyed as beaten by the player, no loot, no XP (code) | §4.3 | Matches |
| Spell kills | The dead keep their items; a recast on a dead holder leaves it dead | The effect routine moves no items; the recast branch has no HP test (bug: it raises the dead holder) | §4.1 | Razdor fixes the original's bug (the recast) |
| Battle deaths | The fallen keep their worn items on the corpse (a player's request, 2026-10-08); burying or dismissing a unit that wears items warns first that they will be lost | Not traced here (Razdor moved them to the pack until 0.3.13, losing what did not fit) | — | Razdor's choice |
| `p-LifeLose` | A permanent unit percentage D (`Unit::drain`) that compounds, cuts HP at cast time and again via the rebuild; a positive value lowers D linearly; carried to the next campaign map | A permanent unit percentage D that compounds, cuts HP at cast time and again via the rebuild; a positive value lowers D linearly | §4.1, §4.3 | Matches |
| Spell school (`Type`) | Shown in the book; no rule reads it | Ignored by every rule | §1.1 | Matches |
| Event learning past 15 | No cap; only the first 15 can be cast; the shop refuses a book of 15 or more | No cap; spells past 15 are known but not castable; the shop refuses only at exactly 15 (bug) | §2 | Razdor fixes the original's bug (the shop) |
| Event learning, "no meeting" 1 | Skipped for any event whose "no meeting" byte is 1 | The Community hook skips it whenever that byte is 1, opcode or not | §2 | Matches |
| Spell shop price | Exactly `CostGold`, a negative one paying the hero | Exactly `CostGold`, no clamp | §2 | Matches |
| Percent stats (protections, regen, vampirism) | Points both ways; clamp 0..99 (protections), ≤ 99 (regen, vampirism) | Points both ways; clamp 0..99 (protections), ≤ 99 (regen, vampirism, regen may be negative) | §6 | Matches |
| Order of spells' `d-` | Before all `p-` (`items::rebuild_stats`) | Before all `p-` (potion, items, spells) | §6 | Matches |
| Initiative and Manevres | Kept in hundredths, divided at the end; Manevres rounded half up when not above the level value | Kept in hundredths, divided at the end; Manevres rounded half up when not above the level value | §6 | Matches |
| Max HP change (items, spells, potions) | HP follows the maximum in single floats with a carried fraction (`units::follow_max`), at each change and when a spell runs out | HP scales proportionally with a carried fraction; unhurt stays unhurt | §6 | Matches |
| Level AB/AS/MP 0 | Forced back to 0 | Forced back to 0 | §6 | Matches |
| Stat floors | Only Initiative and Manevres ≥ 0 and protections ≥ 0 | Only Initiative and Manevres ≥ 0 and protections ≥ 0 | §6 | Matches |
| Hero at 1 HP | Initiative 1 (GlobalIndex 1–3) | Initiative 1 | §6 | Matches |
| Bonus byte | The type's, overwritten by each worn item's (the last slot wins) | Same | §6 | Matches |
| Worn numbers above item count + 1 | Kept (Razdor's content is keyed by `GlobalIndex`, not a record array) | Removed at every rebuild | §6 | Differs (data robustness) |
| Item `Magic` school | No effect on the school; worn only by a unit of that school | No effect on the school; the item may be worn only by a unit of that school | §1.2, §5.3 | Matches |
| Item `Type` | An unknown string reads as a potion (compared case-sensitively); an empty one skips the item | An unknown or empty string means Potion; a section without Type is skipped | §1.2 | Matches |
| Spell `Target` | Compared case-sensitively; anything but `Enemy` and `OneEnemy` is the hero's army | The same exact string compare | §1.1 | Matches |
| Holy items | Refused for every unit of Nature Undead | Refused for every unit of Nature Undead (23 types) | §5.3 | Matches |
| Crown wearers | GlobalIndex = listed (1, 2, 3, 11, …: the knights, royals and undead lords) | GlobalIndex = listed + 1 (2, 3, 4, 12, …), the code's off-by-one (bug) | §5.3 | Razdor fixes the original's bug |
| Wear test order | Crown, dead, potion/goods, shield, holy, class, school, weapon, type, slot | Same order | §5.3 | Matches |
| Dead units and potions | A potion with `f-Hits` ≥ 1000 revives to max·f/10000 HP; any other is refused | A potion with `f-Hits` ≥ 1000 revives to max·f/10000 HP | §7.1 | Matches |
| Potion `f-` protections/regen/vampirism | Replace the stored value, then `p-` adds (`PotionBlock`) | Replace the stored value, then `p-` adds | §7.1 | Matches |
| Potion percents | Summed into one block, applied once | Summed into one block, applied once | §7.1 | Matches |
| Potion magic power | `d-` and `p-MagicPower` add like any stat (for a unit with a school) | Never takes effect (bug: stored only under a school byte nothing sets) | §7.1 | Razdor fixes the original's bug |
| Potion clearing | After every battle of the player, then a rebuild (HP follows the maximum) | After every battle of the player, for the player's army | §7.2 | Matches |
| Pack full on gain | Events drop the item silently; battle loot still counts what did not fit (battle.md) | Item silently lost | §5.1 | Matches for events |
| Drop on a card (army window) | `Game::give_item`: a potion is drunk; on the hero's card any other item, from the pack or from any unit's slot, goes to the pack (a full pack refuses); on another unit's the wear test and its lowest free slot; a refusal keeps the item where it was. The unit panel's drop still equips the selected unit (the hero window's part) | Potion drunk; the hero's card sends any other item to the pack; another unit: wear test, first free slot; a refusal leaves the item on the cursor | §5.2 | Matches |
| Equip slot choice (hero window) | `Game::equip_at` puts the item in the given empty slot; the army-card drop takes the first free one. The items screen has no drop on a worn slot yet | The clicked empty slot; the army-card drop uses the first free one | §5.2 | Rule matches; the screen is left for later |
| Spells after a campaign change | All slots wiped; D kept (the hero's too) | All slots wiped; D kept | §9 | Matches |
| Spells after a battle | The player's units the battle left dead lose their slots | The player's dead units lose their slots | §9 | Matches |
| Community opcode 11 "permanent" spells | End at the map start + 156,588 minutes (0xEEEEEE hundredths, rounded up); slot k takes entry k, a 0 empties it; garrisons too | End at an absolute time ≈ 108.7 game days after the map start | §4.2 | Matches |
| World spell effect drawing | The layer's frame `(t − start)·49 div length` added to the map, times its R, G, B (`chrome::additive` over glow frames), on a 128·scale square centred on the target, its bottom Y·scale px below the centre; layer 1 under the marks and sprites, 2 just behind the figure, 3 just in front of it (`world_view::draw_spell_layer`) | The same, with SRCBLEND = DESTBLEND = ONE over texel × diffuse; 2 and 3 in the target cell's sprite pass, so sprites of the cells below cover layer 3 | §3.4a | Matches (Razdor draws figures after all scenery, so no tree covers layer 3) |
| AI item handling | Value = tactical cost gain over the bare unit, threshold 5 (`ai::give_item`, `ai::redistribute`, shopping) | Value = tactical cost gain over the bare unit, threshold 5, as in §8 | §8 | Matches |

## Unknowns

- What the player sees when an event fires during a cast (the spell lands while the event
  dialog is open; the order of the dialog and the effect animation is not traced).
- Whether the crown off-by-one is intended (the code is unambiguous; the data suggest a slip,
  which Razdor fixes).
- Whether a potion still active at a campaign map change is carried into the next map.
- The meaning of the unit fields cleared together with the potions (+0xc5, +0xc9) and of the
  unused "locked slots" count (+0x18).
- The contact branch of 0x4ade3c that cancels a cast was read only for its effect (flag off,
  queues cleared), not end to end.
