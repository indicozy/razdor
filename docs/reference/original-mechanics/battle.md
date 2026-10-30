# Discord Times battle mechanics: reverse-engineering notes

This file covers the Community Update (Unstable) `DiscordTimes.exe` (Delphi, image base 0x400000).
The rules below are in our own words. Addresses are virtual addresses in that build, given as
evidence only. The raw working notes (record layouts, call chains, disassembly) stay local and
are not part of the repo.

Confidence tags:
- **code**: confirmed by reading the code.
- **data**: consistent with the data files or help text, but not traced in code.
- **unknown**: not determined.

Community Update hooks are named where the battle code jumps into them, but their full
behaviour is documented in [community-patches.md](community-patches.md). Section 7 keeps the
battle-relevant summary of each Community bonus.

## 0. Battle model

**Sides.** Side 1 is always the player's army and side 2 the enemy. Both battle starts
(4d2338 and 4d2358) pass the player's army as side 1, and the end-of-battle UI (4c50ec) reads
side 1 as the player. In the off-screen battle between two AI armies (section 10) side 1 is
the attacker. **code**

**Records.** Each side has a 3×6 grid of unit indices: row 1 is the front, row 2 the back,
row 3 the reserve. A grid cell holds 0 (empty), −1 (blocked, wide row only) or a unit index.
A side also has up to 12 unit records of 0xA5 bytes, kept in list order with no gaps: when a
unit dies, the later records shift down by one (489f69). List order is the order of the
army's units, and it decides ties in the turn order. **code**

**Main functions:**

| Address | Role |
|---|---|
| 4d1fb0 | Battle window opens: builds both sides and starts the battle (section 9) |
| 49855c | Builds one battle side from an army |
| 483b3c | Auto-arrange of a side's grid |
| 48b75c | Battle setup and pre-simulation |
| 4840ec | Start of a turn |
| 489ca0 | Picks the next actor |
| 484c4c | Legal-cell map |
| 4864e0 | AI choice (continued in 486bb9, 487e99, 488928 Death, 489549 moves) |
| 4863e8 | AI hits-to-kill helper |
| 48a5c4 | Performs one action |
| 485908 | Physical damage |
| 485b3c | Magic power by action |
| 485d58 | Hover preview of an action (called at 4c3ff7 and 4c4848) |
| 48a354 | Applies damage |
| 48a3f0 | On-kill effects |
| 489f50 | Removes a dead unit |
| 48a170 | Row collapse |
| 48b5ac | End of an action: collapse, surrender and end checks |
| 48bb10 | End of battle (XP and surrender) |
| 4c50ec | Victory or defeat screen and loot |
| 4a0710 | Off-screen (simulated) battle |
| 495ce0 / 495fac | Add a unit to an army / make sure a unit has a formation cell |

**Action codes.** The legal-cell map gives every cell one code: 0 nothing, 1 pass (own cell),
2 move, 3 pull (dead, section 2), 4 melee, 5 long (flank) strike, 7 shot, 8 hostile magic,
9 friendly magic. Code 8 becomes 0xD curse or 0xE strike, and code 9 becomes 0xB bless or 0xC
heal, when the action runs. The damage kinds passed to 485908 are 4, 5 and 7. **code**

### Physical damage formula (485908; the base code, Community hooks in section 7)

- `atk = Attack(kind) + atkModifier`. Attack is AttackBlow for kinds 4 and 5, AttackShot for kind 7.
  The modifier is added even when the attack is 0 (that matters only for a counter blow).
- `def = Defence(kind) + defModifier`, clamped to at least 0.
- **Melee only:**
  - SpearDefense on battle turn 1: `def ×= 3`.
  - Piercing: ArmorIgnore, PoisonArmorIgnore, VampirsGist or OldVampirsGist sets `def = 0`.
  - Long strike: `def /= 2` (rounded down), and FlankStrike gives `atk ×= 2`.
  - Then `def += buildingDefence`.
- **Shot only:**
  - Piercing: ArmorIgnore, PoisonArmorIgnore or **Artillery** sets `def = 0`. The vampire gifts
    do **not** pierce against shots.
  - *Re-checked* in 485908 and its two hooks. For melee (kinds 4 and 5) the Community hook
    c2a27c accepts bonus values 45 and 3, and the original code after it accepts 10 and 11:
    PoisonArmorIgnore, ArmorIgnore, VampirsGist and OldVampirsGist. For shots (kind 7) the hook
    c2a3bf accepts 45 and 3, and the original code after it accepts 14: PoisonArmorIgnore,
    ArmorIgnore and Artillery. The enum is 1-based (the same function tests 21 = FlankStrike and
    target 15 = Garrison), so these are the bonuses named. **code**
  - Then `def += Row2Def` if the target is in row 2, and `def += buildingDefence`.
- `dmg = atk − def` if `atk > def`, else 1. This is the subtractive mode, B+1 = 1, which
  48b75c always sets. The percentage mode `round(atk·(1−def/100))` is unused.
- The target modifiers follow in this order; every ×2/3 and ×3/10 is an integer multiply then
  divide, rounded down:
  1. Evasive, VampirsGist or OldVampirsGist: ×2/3. Community Assault can also apply
     ×2/3 here (c2a403, section 7).
  2. Garrison with building defence ≥ 10: ×2/3.
  3. Dead or FastDead against a shot: ×3/10.
  4. Knight army: `× Knight% / 100`.
  5. Unvulnerabe or Ghost: `dmg = 1`.
  6. The attacker's GodAnger adds +10 and GodStrike +20 (also on top of the 1 of step 5).
     If the result is 0 it becomes 1.
  7. Community hook c2a802 (Evasion; all values 0 in the shipped data).
- **Knight%.** The value is loaded at 4e4501. This build **sets it to 80 unconditionally** when
  `[GlobalOptions]` is read: the army takes −20% physical damage. The data default is 90 and the
  help text says 10%. **code**
- **Which army is a knight army.** Each side carries a flag that is set when the army's first
  unit is of unit type 0 (the Knight hero class) (49855c). 48b75c copies it into every unit
  record of that side, and step 4 reads the **target's** copy. So it protects any army led by a
  Knight-class hero, an AI lord's as well as the player's. Counter blows and Community
  preventive strikes go through the same function and are reduced too. **code**

**Magic power by action (485b3c).** **code**
- Start from the caster's current MagicPower (after the per-turn drain).
- Curse and strike only: `P = round(MP × (1 − Protect_school/100))`, with the target's
  ProtectLife, ProtectElemental or ProtectDeath for a Life, Elemental or Death caster. The
  rounding is Delphi's, half to even, on the floating-point product. A caster without a
  school is not reduced. Community Potent skips this (section 7).
- Then by action and the **target's nature** (no table for curses):

| Action | Life caster | Elemental caster | Death caster |
|---|---|---|---|
| Bless | 0 on Undead and Elemental targets | P | P |
| Heal | 0 on Undead and Elemental targets | P/2 | 0 unless the target is Undead |
| Strike | ×2 on Undead, ×3/4 on Elemental | ×3/4 on everyone | ×1/2 on Undead, ×3/4 on Elemental |

- Halves and three-quarters round down. On a strike, a caster with MagicPower > 0 then adds
  +10 for GodAnger and +20 for GodStrike, even when P has dropped to 0.
- There is no minimum: a strike of power 0 does nothing, and a heal of power 0 is not a
  heal (section 3).

## 1. Buff and curse duration, stacking, magic drain

**Duration.** **code**
- Every blessing and curse is stored in three per-unit modifier fields: attack, defence and
  initiative.
- The start of **every** battle turn (4840ec, 484238–484288) sets all three to 0 and clears the
  blessed and cursed flags. So a blessing or curse lasts **for the rest of the turn in which it
  is cast**, and it is gone when the next turn starts.
- There is no duration counter. In practice a caster with high initiative gets the most out of
  its spells.
- Elemental bless and curse change the target's **actions left** directly. That only affects the
  current turn. Curses cannot push it below 0.
- The attack modifier adds to AttackBlow and to AttackShot; the defence modifier adds to
  DefenceBlow and to DefenceShot.

**Sizes** (48a9f8–48aaf1 for blessings, 48aef8–48b1e6 for curses). P is the bless or curse power
from 485b3c; every division rounds down. The divisors are the `[GlobalOptions]` values
WizardMainSpell (W, shipped 7), BlessMainSpell (BM, 6), BlessNextSpell (BN, 12),
CurseMainSpell (CM, 5) and CurseNextSpell (CN, 10), plus a fixed 10. **code**

| School | Blessing | Curse |
|---|---|---|
| Life | defence +(3P/(2·BM) + 1), attack +3P/(2·BN) | defence −(P/⌊2·CM/3⌋ + 1), attack −P/10 |
| Elemental | actions +tier(P), initiative +(P/W + 1) | actions −tier(P) (not below 0), initiative −(P/W + 1) |
| Death | attack +(P/BM + 1), defence +P/BN | attack −(P/CM + 1), defence −P/CN |

- With the shipped values: Life bless def +(P/4 + 1), atk +P/8; Life curse def −(P/3 + 1),
  atk −P/10; Death bless atk +(P/6 + 1), def +P/12; Death curse atk −(P/5 + 1), def −P/10;
  Elemental ±(P/7 + 1) initiative.
- `tier(P)` (483680) is 0 below 20, 1 below 45, 2 below 100, else 3.
- The Elemental actions change is applied before the initiative change.
- `⌊2·CM/3⌋` is computed at every battle setup (48b7b8) and stored at 4ed3a8.
- A Life blessing on an Undead or Elemental target changes nothing but still marks it blessed.
- A caster with MagicPower but no school blesses or curses for nothing: only the flag is set.

**Stacking.** **code**
- Values are added (+=) to the modifier fields, not replaced.
- A friendly mage cannot pick an already-blessed unit in the same turn, unless the unit is
  wounded, and then it heals instead (except a reserve caster, section 3). **Bug:** "heals"
  only when the heal power is above 0 (48a8d3, tested at 48a911/48a917); otherwise the action
  blesses again (48a93c) and the blessings add up. So a Death caster re-blesses a wounded living
  ally on every action (its heal is 0 on the living), a Life caster a wounded Undead or
  Elemental one (for nothing), and a reserve caster any reserve unit (no test there, 4857fb).
  Razdor on main fixes this (the owner's decision, row 2 of the table).
- A hostile mage whose target already has a negative modifier strikes instead of cursing
  (section 3).
- So in practice there is at most one curse per unit per turn, and one blessing except in the
  bug's cases above (and the Community Splash follow-ups, which re-run the case on the
  neighbours whatever their flags, c26fd2). The flags reset
  every turn.

**EternalGift** (hooks c29e59, c29ea3, c29f2f for curses; c2a019, c2a0ae, c2a0f8 for blessings).
**code**
- The caster's blessings and curses write to the target's **base** stats instead of the
  modifiers. So they last the whole battle **and stack** with every cast.
- Elemental: base initiative changes by the amount. The change to actions left still applies to
  the current turn only.
- Life and Death: attack and both defences change.
- Bug: an EternalGift **Life blessing** *lowers* both defences by the defence amount, while it
  raises attack. The attack goes to AttackBlow, or to AttackShot when AttackBlow is 0.

**Magic power drain.** Community replaces the whole vanilla block (c2851a; the vanilla code at
48446d is never reached). **code**
- From turn 2, for each unit with MP > 0, at the start of the turn:
  1. `MP −= ManaDrain`;
  2. `MP = max(MP, MinMagicPower)`;
  3. `MP = max(MP, 0)`.
- **Defaults** by school (an ini value of 0 falls back to the default):
  - `ManaDrain`: DecSpellLife 2, DecSpellDeath 2, DecSpellElemental 5.
  - `MinMagicPower`: MinSpellLife 15, MinSpellDeath 0, MinSpellElemental 15. **Undead Death
    casters get +25** on the floor.
- The floor also **raises** a weaker caster. A Life mage with MP 10 has 15 on turn 2.
- **Concentration** *adds* the drain instead of subtracting it, with no cap.
- **The original's own block** (0x484466–0x484683) is what an exe *without* the Community patch
  runs (seen in the Evolution Mod's exe; its `_Global.ini` has no `MinSpell…` keys). The Community
  exe has a `JMP` (E9) at 0x484466 to c2851a; the original's block starts with an IMUL
  (`69 45 F8 51 08 00 00`). Per unit with MP > 0, from turn 2, by school:
  - **Life:** MP −= DecSpellLife (+= with Concentration); then MP = max(MP, 15).
  - **Elemental:** MP −= DecSpellelemental (+= with Concentration); then if MP < 0 and the nature
    is Undead or Rogue, MP = 0; otherwise MP = max(MP, 15).
  - **Death:** MP −= DecSpellDeath (+= with Concentration); an Undead caster gets MP = max(MP, 25),
    other casters no floor.
  - No school: no change.
  - Finally (0x484683), every unit with MP < 0 gets MP = 0.
  - The floors are fixed (15, 15, 25). This block reads neither `MinSpell…` nor the per-type
    `MinMagicPower` / `ManaDrain`; the amounts still come from `DecSpell…`.
- **Which rule an install uses** (read at load, `install::drain_rule`): the install's
  `DiscordTimes.exe`, when there is one: the byte at the file offset of 0x484466 (through the PE
  section table) is E9 → Community, anything else → the original's block. Without an exe: Community
  when `_Global.ini` has a `MinSpellLife` key, else the original's block.

## 2. Reserve row, movement and collapse

**Movement options (484c4c, helpers 4848a4 and 484b64).** **code**

- **Every action costs one action,** moves and "pass" included. 48a5c4 decrements actions-left
  before it does anything else. (A switch at 4ed394 that would also cost current initiative
  is off.)
- **From row 1 or row 2:** a unit may move to an empty cell of row 1 or row 2 in columns
  c−1, c or c+1. It may also move to **any** empty reserve cell, but only while its per-turn
  reserve flag is set. This works from the **front row as well as the back row.**
- **From the reserve:** a unit may move to any empty cell of row 1 or row 2, in any column, but
  only while the flag is set. Without the flag it cannot move at all.
- **The reserve flag.**
  - It is set to 1 for every unit at the start of each turn (4840ec).
  - It is cleared when a unit moves into or out of the reserve (48a7b7–48a813).
  - Result: one reserve transition per unit per turn. A unit that entered the reserve cannot
    leave it that turn, and a unit that left it cannot go back that turn.
- **No swapping.** A unit can never swap places with an own unit. Only empty cells are offered.
- **Clicking one's own cell** (code 1) passes one action. It increments a per-unit counter
  (+0x99, reset to 1 every turn). For a friendly caster the own cell is often code 9 instead,
  a self-cast (section 3).
- **The space key** (4c4f8c) does exactly what a click on the active unit's own cell does:
  **one** action, a pass or a self-cast. It does not skip the unit's remaining actions.
- **Map code 3** would pull an enemy back-row unit into its front row. It exists only when the
  constant at 4ed38c is 0, and in this build it is 1, so the code is dead.

**Collapse (489f50 → 48a0b5 → 48a170).** **code**

The check runs right after any unit of a side is removed as dead (any cause: a blow, a spell,
poison, a curse on the killer). It also runs for the actor's own side when the actor has used
its last action (48b5ac). It never runs at a turn start.
- **Rows 1 and 2 both empty:** every reserve unit moves to **row 1**, in the same column, and its
  remaining actions for this turn are set to 0. The reserve never steps into row 2.
- **Only row 1 empty:** every row-2 unit moves to row 1, in the same column, and keeps its actions.
- **A voluntary move** that empties the front row therefore makes the back row step forward only
  once that unit has finished its actions.
- **Removal of the dead:** a dead unit is taken out of the side's list. Later records shift down
  and the side's unit count drops (hook c252bd). The action's result code becomes "a unit died".
- **Wide-row quirk.** The collapse copies the whole grid row, the blocked −1 cells included. With
  6 columns, after a back-row collapse the front row's columns 1 and 6 become blocked and the
  back row's columns 1 and 6 open; after a reserve collapse the front row keeps only columns 3
  and 4 usable. What the screen does with the opened cells is **unknown**.
- **At the start of the battle** there is no collapse in the battle code. The battle window
  (4d2141) instead fixes the **player's army formation** before the sides are built: if its
  front row is empty, its back row moves into the front row (same columns). The reserve is not
  moved, and the change stays in the army's formation after the battle. The test reads the
  army's formation, where the dead and the units that sit out keep cells: one of them in the
  front row stops the fix. In wide mode the copy
  also carries the back row's blocked end cells (columns 1 and 6) into the formation's front row
  and opens them in the back row; the battle grid rebuilt from the formation (section 6) turns
  any blocked cell into an open one, and the write-back after the battle restores the normal
  blocks. The enemy needs no fix: its auto-arrange always puts a unit in row 1 (section 9).
  **code**

**Row 2 defence.** Row2Def (+5) is added only against shots, after any piercing (485a04). So a
piercing shooter still faces Row2Def and building defence. **code**

## 3. Mage action choice and targets

**Hostile mages.** There is **no player choice** between strike and curse. The player's click
(4c43e8) and the AI both execute the cell's code 8, and 48a5c4 (48ac99–48acf1) decides:
- if the target already has any negative attack, defence or initiative modifier this turn, the
  mage **strikes**;
- otherwise it **curses**.

Since modifiers are wiped at every turn start (section 1), each fresh target is cursed first and
struck afterwards. This holds for every school, Life included. **code**

The hover preview (485d58, called from 4c3ff7 and 4c4848) only shows numbers; it does not decide
the action. It caps damage at the target's HP and heals at the missing HP, shows no attack
gain for a target without AttackBlow and AttackShot, and shows no positive change on an
already-blessed target or negative change on an already-cursed one. **code**

**Friendly mages.** The action on an ally (code 9) is a **heal** when the target is wounded and the
heal power (after the nature table) is above 0; otherwise it is a **bless** (48a87c–48a937). So
a Death mage clicking a wounded living ally blesses it, and a Life mage clicking a wounded
Undead blesses it for nothing. **code**

**Legal targets.** **code**
- **Order of the map.** Melee cells are written first, then shots, then hostile magic, and a
  later code overwrites an earlier one on the same cell. A unit with several attack types uses
  hostile magic where it can, else a shot, else melee. (No shipped unit has both AttackBlow and
  AttackShot; items could create one.) **code** for the order, **data** for the units.
  Two later writes win on their cells: the Community Flying hook (c29150)
  writes melee on the enemy front cells c−1 to c+1, replacing a shot or spell option there,
  and the Ghost cast (below) writes hostile magic on the same three cells. **code**
- **Melee (AttackBlow > 0, from row 1 only):** the occupied enemy front cells c−1, c, c+1. If
  none, a long strike (code 5) on the nearest occupied enemy front cell to the right at distance
  2 or more, and on the nearest to the left at distance 2 or more.
- **Shooter (AttackShot > 0):**
  - From row 2: every enemy in rows 1–2.
  - From row 1: the same if the enemy front row is "clear" opposite it (below); otherwise only
    the occupied enemy front cells c±1.
  - From the reserve: nothing.
- **Hostile mage (MagicPower > 0, ToAll or ToEnemy):**
  - From row 2: every enemy in rows 1–2.
  - From row 1: the same, but only if the enemy front row is "clear"; otherwise it cannot cast
    at all.
  - Ghost-bonus casters with MagicPower may also cast at the three opposite front cells from
    any row, the reserve included, whatever their direction (48555b). The power test there
    looks only at the lowest byte of MagicPower, read as signed, so a power of 128 to 255 (or a
    multiple of 256) fails it. No shipped Ghost has more than 45. **code**
- **"Clear" (484ac8).** The enemy front cells c−1, c and c+1 must all exist and be empty. So a
  unit in the first or last column is **never** clear: a front-row shooter there only hits the
  occupied enemy front cells from c−1 to c+1 (the opposite cell and its existing neighbour),
  and a front-row mage there can never cast, even when the enemy front opposite is empty. **code**
- **Friendly mage (MagicPower > 0, ToAll or ToAlly), from rows 1–2:**
  - It may target any own unit in rows 1–2, in any column, itself included, that is wounded
    **or** not yet blessed this turn (flag +0x9d).
  - An Elemental caster cannot target an Elemental-school ally at full HP.
  - Units crippled by NoHeal are never offered here (Community hook c2967a).
- **Caster in the reserve:** it may target every own reserve unit, itself included, and nothing
  else (4857fb). Here there is **no** wounded-or-unblessed test, so a reserve unit can be blessed
  again, and the NoHeal hook is not on this path either: a NoHeal-marked reserve unit can still
  be healed or blessed by a reserve caster (the mark table is read only at c2967a). **code**
- **Own cell (48555b end).** The active unit's own cell is "pass" unless it is a friendly target
  that is wounded or unblessed; then a click on it (or the space key) casts on itself.

**Stacking.**
- **Blessings:** their values are added to the modifiers. A blessed unit in rows 1–2 is no
  longer a legal target unless wounded; a wounded, blessed unit can still be healed. But when
  the heal power on it is 0 (a Death caster on a living ally, a Life caster on an Undead or
  Elemental one) the action is a blessing again (48a93c), and a reserve caster may bless any
  reserve unit again (4857fb), so in those cases blessings stack without limit (section 1,
  the bug; fixed on Razdor's main only).
- **Curses:** in practice one per target per turn. After the first curse the auto-choice
  switches to strikes.

**Extra: Undead casters drain.** If the **caster** is Undead and its school is Elemental or Death,
its curse also drains `(P / CurseMainSpell) / 2 + 1` HP (48afce and 48b100). The target loses
this amount (capped at its HP) and the caster gains the **full** amount, capped at its own max
HP. Life-school curses do not drain. **code**

**Extra: vampirism on magic.** A Death-school **strike** heals the caster by
`Vampirizm% × strike power / 100` (the strike value before it is capped at the target's HP),
but not against Undead or Elemental targets (48ad61). **code**

## 4. AI in battle

Everything here is **code** unless marked.

**Framework.**
- One routine, 4864e0, serves both sides. It runs for every actor, the player's included (the
  result is ignored for a side under manual control).
- The picker 4860cc scores cells with a strict ">", scanning rows front to back and columns in
  the preferred order: 3,2,4,1, or 4,3,5,2,6,1 with six columns. The first cell in that order
  wins ties, and a score of 0 is never chosen.
- **Unit strength** (4836cc):
  - `power = max(AB, AS, MP) + (sum of the other two)/3`.
  - Bonuses add to it: GodAnger +10, ArmorIgnore +15, GodStrike +20, Counterblow +AB,
    FlankStrike +10.
  - Role: shooter if `AS ≥ power/1.5`, mage if `MP ≥ power/1.5` (mage wins a tie), else warrior.

**Priority order.** The first step that yields a positive score wins.
1. **Front-row retreat.** A unit in row 1 retreats when all hold: it is not a Ghost, it has more
   than 1 action left, its role is not warrior, and its AttackBlow is not above both its
   MagicPower and its AttackShot. It also needs another own unit in the front row, or to be the
   side's only unit and a mage. Each legal back-row cell scores 1000, plus the HP of the own
   front unit in that column (other columns only). So it hides behind the toughest front unit.
   With the switch at 4ed390 on (it is), there is also an edge rule, which needs another own
   front unit: a unit in the second column scores 1 on its side's first front cell when an
   enemy stands in the last front column, and a unit in the last but one column scores 1 on
   the last front cell when an enemy stands in the first. A free back-row cell always wins
   over it; without one, the AI takes that cell's own code: a step when it is free and next
   to the unit, a heal or blessing on an ally there, else an action spent for nothing.
   **code** (4864e0, the 4ed390 branch)
2. **Melee** (row-1 targets, codes 4 and 5):
   - R is the target's return damage on the actor (its melee damage if it is a warrior, its
     shot damage if a shooter, its power if a mage); M is the target's Manevres.
   - `score = dmg × round((R + 1) × M)`.
   - An attacker with Poison against a target with regen ≥ 0, when dmg > 1: ×2.
   - Killable target: the score is **replaced** by `100 × round((R + 1) × M)`.
3. **Shots** (the actor is not in the reserve):
   - `score = round((targetPower + 1) × dmg × (M + √targetActionsLeft))`; the square root only
     when the target has an action left.
   - Poison as above ×2; killable ×4.
   - Back-row targets: a warrior ÷3, a shooter ×1.5, a mage ×7/4. A back-row **mage** is halved
     again if it can cast at enemies and has the actor's nature, and halved once more if it has
     1 Manevres. The "Manevres 1" rule applies **only** to back-row mages (487193, inside the
     mage branch).
4. **Magic by school.**
   - Life heals the biggest wound, but only one of at least MP/4, and not on Undead. Otherwise
     it blesses low-defence, strong front-liners. Its strikes prefer Undead ×3.
   - Life's "curse value" has an original slip: of its two defence terms, the second compares
     DefenceShot but adds DefenceBlow (4879c0–4879dd).
   - Elemental compares haste or slow against heal or strike, per side (details below).
   - Death weighs urgency against threat by role; a nearly dead Death caster may target itself
     (details below).
   - The Life scoring above is **medium** confidence. The Elemental and Death scoring below was
     read in full (**code**).
5. **Moves, only when nothing else scored:**
   - Back-row pure warriors (no AS, no MP) step forward, preferring a column facing an enemy and
     one with a strong own back-row unit behind.
   - Front-row units shift sideways toward the most enemies: per front column, `2·(4 − |d|)`
     for each enemy front unit at d columns and `4 − |d|` for each enemy back unit. Only the
     cells with a code of their own keep their score: the free cells next to it, its own cell
     (a pass or a self-cast) and, for a friendly caster, an ally's front cell it may tend, in
     any column; picking an ally's cell heals or blesses it. **code** (489549)
   - Reserve units come out: warriors to row 1 or 2, others to the nearest back-row cell. A mage
     in the reserve heals wounded reserve units instead.
   - A back-row non-warrior never moves (its branch looks for a code that is never written).
   - The AI **never** moves a unit into the reserve.
   - With nothing to do, the unit passes on its own cell.

**The AI picks a cell, not a spell.** The magic scoring only chooses a target cell. The action
is then fixed by the cell's code, as for the player (section 3, 489ca0 and 48a67e): on an ally,
a heal if it is wounded and the heal power is above 0, else a blessing; on an enemy, a curse if it has no negative modifier,
else a strike. So an Elemental "haste" pick on a wounded ally becomes a heal, and a "slow" pick
on an enemy whose attack or defence modifier is already negative becomes a strike. **code**

**Hits to kill (4863e8).** Used by the Elemental haste value. For a friendly unit U and a
column c:
- Take the enemy unit in the front row of column c. If there is none, the result is 0.
- Otherwise compute U's physical damage on it (485908): a melee blow if U has no AttackShot,
  a shot otherwise. Whether U could reach that cell is not checked.
- Result: `⌊enemy HP / damage⌋ + 1`. The +1 is added even when the division is exact, so the
  count is one too high for an HP that is a multiple of the damage. The damage is at least 1,
  so there is no division by zero. **code**

**Elemental scoring** (the Elemental branch of 486bb9, continued in 487e99 up to 488928).
**code**
- Two flags are set first:
  - *all-Ghost*: every unit of the caster's side, the caster included, has the Ghost bonus;
  - *shielded enemies*: the number of enemy units with Unvulnerabe or Ghost.
- *Shield rule* (own-side values only): the value is divided by 10 (rounded down) when there
  is at least one shielded enemy and their number is at least half the enemy count (rounded
  down), the target has neither GodAnger nor GodStrike, and either the target has no Life or
  Elemental school or the **caster's** MagicDirection cannot reach enemies. The last test reads
  the caster's direction, not the target's; the intent is **unknown**.
- Each side keeps two best values: a *main* one and an *alternative* one.
- **Haste** (main, own side). Candidates: own cells with code 9, not the
  caster's own cell, initiative modifier at most 0, at least one action left (the Community
  hook c25da0 lifts the action test for Manevres-0 units).
  - `h = round(tier(caster MP) × power_t × Initiative_t / enemy mean initiative)`, with the
    target's base Initiative and the enemy side's mean initiative of this turn (1 if 0).
    `tier` is that of section 1, applied to the caster's MP, not to the blessing power.
  - Target in the front row: add up hits-to-kill(target, column) over columns c−1, c, c+1
    (those inside the grid) into S, and let L = tier(caster MP) + the target's actions left.
    If S < L, then `h = round(h × S / L)`. So a front-row unit with no enemy in front of it
    (S = 0) is worth 0, and one that could already finish the enemies facing it is scaled down.
  - Then the shield rule.
- **Heal** (alternative, own side). Candidates: own cells with code 9 and HP
  below maximum, the caster included. Value `min(⌊2·MP/3⌋, wound)`, then the shield rule. This is
  an estimate: the real Elemental heal is P/2 (section 0).
- **Enemy cells** with code 8, rows 1 and 2. Let
  `s = tier(curse power on the target) × power_t`.
  - If s < 1, or the target is already slowed (initiative modifier < 0), or it has no action
    left: alternative = the strike power on it (485b3c), ×10 when *all-Ghost* holds and the
    target is already cursed.
  - Otherwise: main = `s × the target's actions left` (the slow value). If the caster has more
    than one action left, alternative = slow value + strike power, with the same ×10.
- **Choice.** Per side, the alternative replaces the main when it is strictly larger. The own
  side is chosen only when its value is strictly larger than the enemy side's; ties go to the
  enemy side. If the chosen value is positive, the AI targets that unit's cell; otherwise it
  goes on to the moves.
- The "all-Ghost ×10" operand was re-checked: the flag loops over the caster's own side and
  tests the Ghost bonus; the ×10 tests the enemy target's cursed flag (487d2d, 4886a6, 4887ab).

**Death scoring** (488928–489549). The score grid is cleared first; picks use the picker
4860cc over rows 1–2. **code**
- **Hostile part**, only when the caster's MagicDirection can reach enemies (ToAll, ToEnemy,
  Community values 3 and 4):
  1. *Self-target.* Score every enemy cell with code 8 by the strike power on
     it, and pick the best. If there is one, its strike power is at most `⌊MP / CM⌋`
     (CurseMainSpell, 5), the caster's HP is at most `⌊maxHP / 4⌋` and at most `maxHP − MP`,
     the AI chooses the caster's own cell (section 3: a self-cast if one is offered, else a
     pass).
  2. Otherwise, for every enemy cell with code 8 and strike power P > 0:
     - Spell value V. If the target has a negative attack, defence or initiative modifier (it
       would be struck): `V = caster's actions left × P`. Else (it would be cursed):
       `V = ⌊⌊P / CM⌋ / 2⌋ + 1 + (caster's actions left − 1) × P`.
     - Kill bonus: `20 − 2·⌊HP_t / P⌋` when `HP_t ≤ V`, else 0.
     - Threat T. Let d = the target's damage on the caster × the target's Manevres, where the
       damage is a melee blow for a warrior, a shot for a shooter and the target's strike power
       for a mage. Warrior: `8 − ⌊casterHP / d⌋`, at least 1. Shooter: `16 − ⌊casterHP / d⌋`,
       at least 3. Mage: `12 − ⌊casterHP / d⌋`, at least 2. When d ≤ 0, T is that minimum.
     - Score `power_t × (kill bonus + T) × V`.
     - If the best score is positive, the AI targets it.
- **Friendly part**, when nothing hostile was chosen and the direction can reach allies (ToAll,
  ToAlly, Community values 5 and 6):
  1. Heal: own cells with code 9, score = wound, kept only for Undead or
     Elemental targets (a Death heal is 0 on anyone else, section 0).
  2. Bless: own cells with code 9, not yet blessed this turn, at least one
     action left, not a mage: `actions left × power × HP`.
- Otherwise the moves (489549).
- A target whose role is none of the three would keep a stale T. Every unit has one of the
  three roles, so this cannot happen.

**"Killable" and the difficulty option.**
- The battle object's AI mode is 2 with OptValue9 (the improved battle AI option) on (the
  global at 65d4da), else 1 in an interactive battle, and 0 in an off-screen battle.
- Mode 2, or mode 1 for side 1 (the player's auto actions): a target is killable when
  `HP ≤ actionsLeft × dmg`.
- Otherwise the test is meant to be `HP ≤ dmg`, but the code (486d03 melee, 486feb shots) reads
  the HP of the unit **with the target's list index on the actor's own side**. So the normal
  enemy AI, and every AI army off-screen, judges "killable" from an unrelated own unit's HP.
  When that index is past the end of the actor's own list, the record read is one the side
  holds beyond its units: a record a death emptied this battle (a removal shifts the records
  down and zeroes the last, 489f69) reads 0; one beyond the side's units at the start holds
  whatever was there before the battle. The off-screen battles (0x4a0710) are played from
  two static sides, 0xc081ac (attacker) and 0xc08a00 (defender), copied whole into the battle
  (48b75c) and back out at its end (48bb10); 49855c writes an army's units into records
  1..n and leaves the others, and nothing clears them. So a record beyond the units holds
  the HP the last battle, or army passed through that side (an arrival's garrison
  reshuffle, a respawn: 49855c + 4988c0 on the first side), left there: an off-screen
  battle's targets depend on the battles before it. Checked in the running game (Frida on
  48b75c and 0x4a0710, РК4's opening: the records of the first 269 off-screen battles, the
  map load's included, are the ones this model gives; FINDINGS.md §27).

**No randomness.**
- The picker would add `rand((max − min) × B+9 / 100)` from the game's own LCG (4832fc), but
  B+9 is 0 at all three battle creations (4d2338, 4d2358, 4a07b7).
- CrazyAI (4ed3d4) is written by the ini loader and **never read**.
- Delphi `Random` has no caller in battle code.
- Even with a spread, the result would not vary between runs: the picker reseeds that LCG
  from the turn, the cursor side and index and the threshold before rolling.
- So battles are fully deterministic.

**Pre-simulation.** Battle setup (48b75c) first plays a complete AI-vs-AI copy of the battle,
then restores the state. It keeps only each side's damage taken in that copy (side field +8),
which the XP code reads (see experience.md). Players never see it. It runs for the interactive
battle too, before the Splash flag is set. **code**

## 5. Turn limit and surrender

**End check** (end of 48a5c4, 48b67b–48b751). It runs after **every action**. The battle ends if:
- a side has no units, or
- `turn ≥ BattleEndTurn`, or
- one side's every remaining unit has `Surrender > 0`.

The turn counter goes up when a turn starts (4840ec). So with BattleEndTurn = 25 the battle
stops after the **first action of turn 25**: 24 full turns plus one action. **code**

**Turn-limit outcome** (4c50ec, 4c5274). There is **no draw branch**.
- If the player's side still has any unit, the **victory** path runs: gold loot at
  `enemy gold / VictoryGoldDiv`, trophies, and the lord and army handling for the enemy.
- Only a player side with 0 units is a defeat.
- So reaching the turn limit alive counts as a win. A beaten field army is destroyed
  (496834, "beaten by the player") whether units of it survived or not. **code**

**Surrender.**
- **Values:** each battle unit carries its type's `Surrender` value (copied at 4983c6).
- **Trigger:** when every remaining unit of a side has Surrender > 0, that side is flagged at
  the end-of-action check (48b6ba–48b70a). Examples are priests, nuns, witches, mages and
  townsfolk. The check only runs after an action, so a side that starts out like that gives up
  after the first action of the battle, whoever makes it.
- **Result** (48bfb4–48c091):
  - The flagged side's units are all removed (HP 0, count 0).
  - The **sum of their Surrender values** is stored on that side; the victory path gives the
    enemy side's sum to the player as mana (4c50ec). This is the mana the VictoryMana message
    reports after such a battle.
  - Units killed before the surrender give **no** mana.
- **Even after a win:** the test runs for each side that still has units, whether the
  other side is gone or not (48b6ba). So a player whose last enemy falls while only
  surrender-capable units are left on his side surrenders all the same, and since his side
  then has no unit, the battle is a defeat. **code**
- **Either side:** the rule applies to the player's side too. With only surrender-capable units
  left (a hero has Surrender 0, so only once the hero is down), the player's side is removed and
  it is a defeat. Nothing gives the enemy mana. **code**
- **Why a lone unit gives mana:** a garrison of one Surrender = 20 unit surrenders after the
  first action and gives +20 mana. This matches the footage.

## 6. Columns and wide row

- **Column count.** The global at 4ed044 is 6 when `[Options] OptValue11 = 1`, else 4 (read at
  4b8a46). The width is stored in the save header at new game (4b25a2) and restored on load
  (4b77f3). **code**
- **Vanilla (4 columns):** 3 rows × 4 columns, 12 cells. On screen they take the same 2 × 6
  places (492940): the front row the middle four of the first line, the back row the middle
  four of the second, and the reserve the four ends (columns 1, 2, 3, 4 at the first line's
  right end, the second line's left end, its right end and the first line's left end), so the
  front row's two edge places are reserve cells as the back row's are. **code**
- **Wide (6 columns)** (48395c). The grid blocks cells by filling them with −1:
  - front row: 6 cells;
  - back row: 4 cells, columns 2–5;
  - reserve: 2 cells, columns 3–4.
  - That is still 12 cells. Blocked cells can be neither entered nor targeted. **code**
  - On screen it is a 2 × 6 grid (492940 maps a cell to one of 12 places): the front row on
    the first line; on the second, reserve column 3, back columns 2–5, reserve column 4. So
    the two reserve cells are the ends of the back row, where the footage shows tent icons.
    **code**
  - **Exception, player's side.** The battle window rebuilds side 1's grid from the army
    formation (4d2233), and the army's −1 cells match no unit, so they become empty: in battle
    the player's side has no blocked cells. Only AI-chosen moves for the player's units (the
    pre-simulation) could use them; the screen has no place for them. **code** for the grid,
    **unknown** for any visible effect. The enemy side keeps its blocks.
  - The collapse quirk in section 2 moves blocks between rows.
- **Preferred column order** (AI picking and placement):
  - 4 columns: 3,2,4,1 (tables 4ed030 and 4ed034);
  - 6 columns: 4,3,5,2,6,1 (4ed018 and 4ed01c). **code**

## 7. Community bonuses: what the code does

Everything here is **code** unless marked. Where the code, the in-game text and the changelog
disagree, the numbers below are the code's. The hook mechanics (where each hook is patched
in, the `.mod` variables) are in [community-patches.md](community-patches.md).

**Enum.** The token table gives these values: 22 Hunger, 23 Berserk, 24 Exhaustion, 25 Drying,
26 CtrPoison, 27 Suicide, 28 Caster, 29 Splash, 30 Fortify, 31 Dominate, 32 PoisonS,
33 Concentration, 34 Potent, 35 Stun, 36 FirstShot, 37 Bastion, 38 Flying, 39 Bleed,
40 PreventiveStrike, 41 Flock, 42 ArmorBreaker, 43 NoHeal, 44 FasterAttack,
45 PoisonArmorIgnore, 46 HoldLine, 47 Neutralize, 48 KillingStrike, 49 BloodThrist,
50 Assault, 51 EternalGift, 52 FateGift. The parser is at c255a1, c25af7, c27d94 and c28e85.
This matches src/dt/data.rs. The vanilla values are 1 SpearDefense, 2 HorseAtack, 3 ArmorIgnore,
4 ArmyMedic, 5 Merchant, 6 DeathCurse, 7 GodAnger, 8 GodStrike, 9 Unvulnerabe, 10 VampirsGist,
11 OldVampirsGist, 12 Evasive, 13 Ghost, 14 Artillery, 15 Garrison, 16 AddPayment, 17 Poison,
18 Dead, 19 FastDead, 20 Counterblow, 21 FlankStrike.

**One bonus per unit.** The unit has a single bonus byte. Each worn item that has a bonus
**overwrites** it; the last slot with one wins (4919f0–491a4f).

**Where the hooks run.**
- Three hit paths each have their own chain of after-hit hooks: melee (48b2f6), shot (48b214)
  and hostile magic (48ac7e).
- In the magic path the "damage" tested is the spell's power, so the "damage > 1" effects also
  fire on curses.
- The turn-start hooks sit in 4840ec, after the per-turn reset of each unit and **before** that
  unit's drain and regeneration (section 8).
- Other battle hook points: 484c4c end (Flying, c29150), 48555b (NoHeal, c2967a), 485908
  (Splash scaling c270ae/c27337, piercing c2a27c/c2a3bf, target modifiers c2a403, Evasion
  c2a802), 485b3c (c27374, Potent c26e78), action start (Bleed, c2a53c/c2a5a1), the
  bless/curse appliers (EternalGift), unit removal (c2a95a, c252bd), the action end (Splash
  loop c26fd2).

| Bonus | Original rule | Where |
|---|---|---|
| Hunger | **Melee** kill: heals to full. Also, at a turn start from turn 2, if the total number of living units changed since the last check, a Hunger unit heals to full. The "last seen" counter is shared, so only the first Hunger unit benefits. Shot and spell kills do not heal. | c252e9, c25370 |
| Berserk | Attack modifier **set** to `AB × 75 × (maxHP − HP) / maxHP / 100`, up to +75% of AB. Recomputed at turn start (from the HP before that turn's regeneration and poison) and whenever it is hit. It overwrites a blessing's attack. | c256c8, c25777, c2581b, c258bf |
| Exhaustion | Each of its hostile spells lowers all 3 protections of the target by **10 points**, cumulative, not below 0. The text says 10%, the changelog 15. | c26222, c27ec7 |
| Drying | After its hostile spell: `max(1, 8% maxHP)` extra, ignoring protection. | c2596b |
| CtrPoison | A **melee** or long striker gets **regen −20**, stacking per hit. Shots and spells do not trigger it. | c25c43 |
| Suicide | Dies after any hostile action of its own. | c2633c, c262a1, c261c1 |
| Caster | World spells only. | c25ccc |
| Splash | The first hit uses 80% of attack or power. The same action is then repeated at 40% on the target's same-row neighbours at c±1. For melee the neighbour must also be within 1 column of the attacker. Works for shots, hostile magic, **and heals and blessings**. Only in the interactive battle (flag 4ed424), not in simulations or the pre-simulation. The 40% uses a slightly low fixed-point constant: a multiple of 5 comes out one lower (an attack of 10 gives 3). | c26f2d, c270e5, c2718d, c27274, loop c26fd2 |
| Fortify | From turn 2, at turn start, the defence modifier gets `max(1, DefenceBlow×25%) × min(turn − 1, 5)`: +25% per turn after the first, up to +125% from turn 6. It is based on DefenceBlow and counts for both defences. The text says 20%/100%. | c25a1d |
| Dominate | **No effect.** Its routine (c263d2) is unreachable: the entry is jumped over and nothing branches to it. | c263cb |
| Poison (vanilla #17) | Melee or shot damage > 1 sets regen to **−20**, replacing the unit's own regen. Community: mages poison too, when power after protection > 15. The text says 15%. | 48b2bd, 48b3bc, c259da |
| PoisonS | Regen set to **−25**. Same triggers as Poison. | c2639c, c26301, c26d5c |
| Concentration | The drain is added instead of subtracted, with no cap. The floor still applies. | c2851a |
| Potent | Its strikes and curses skip protection **and** the nature multipliers (no ×2 on Undead, no ×¾ on Elementals). GodAnger and GodStrike still add. | c26e78 |
| Stun | Every hostile hit or spell, with no damage test, lowers the target's initiative modifier by **30% of its current initiative**, cumulative within the turn. It is reset at the next turn start. The text says 35%, the changelog 25%. | c26e1f, c27e6e, c289ae |
| FirstShot | Turn 1: +30 initiative, and +30 more with building defence ≥ 10. The same as vanilla Artillery. | c28935 |
| Bastion | At **every** turn start it doubles its own AB, AS, DB and DS, compounding (×2, ×4, ×8…). There is no building check, no damage halving and no army +10. Almost certainly a bug. | c26ebb |
| Flying | From row 1 **or row 2**, it can melee the three enemy **front** cells c±1. It cannot reach the enemy back row and cannot act from the reserve. The hook means to pick melee, shot or magic by attack type, but its test (AttackBlow not negative) is always true, so it always writes melee, and it runs after the shot and magic cells are written: on those three cells a Flying shooter or mage **loses** its shot or spell and strikes in melee instead (with AttackBlow 0 that hits for 1 plus the attack modifier). Its other targets are unchanged. | c29150 |
| Bleed | A hit with damage (or power) > 1 sets the target's bleed to 75, a maximum that does not stack. **Each time the bleeding unit starts an action** it loses `(AB + AS + MP) × 75%` HP; if that kills it, the action is cancelled. It lasts all battle. | c29235, c2a53c (hooked at 48a677) |
| PreventiveStrike | Before an enemy's **melee** on it, it strikes first: melee if it has AB, else a shot. Before a **shot or spell** on it, it shoots first, but only if it has AS. Every attack, no limit. If the attacker dies, the attack is cancelled. | c2a181, c28aea |
| Flock | Turn start: the attack modifier ±25% of AB (or of AS when AB is 0), depending on which army record has more units. That these are the **start-of-battle** sizes is medium confidence. Equal sizes: nothing. | c29d0f |
| ArmorBreaker | A hit with damage > 1: the target's DB and DS ×**0.75**, cumulative, for the battle. | c29282 |
| NoHeal | Any hit marks the target for the battle. A marked unit in rows 1–2 can be neither **healed nor blessed** (no code-9 cell); the reserve caster's targets skip this test, so a marked reserve unit can still be tended there. Positive regen is set to 0 when it is marked. Vampirism and Hunger are **not** blocked. | c2967a, c2962c |
| FasterAttack | +1 action on turns 1 and 2. | c29c6b |
| PoisonArmorIgnore | Pierces like ArmorIgnore (building defence and, for shots, Row2Def still count). A hit with damage > 1 sets regen to `min(regen, −10)`. | c2a27c, c2a3bf |
| HoldLine | No code. | — |
| Neutralize | Every hit clears the target's bonus byte for the rest of the battle, with no damage test. | c29411 |
| KillingStrike | After damage > 1, the target dies if its HP ≤ 25% of max. It is checked before FateGift, so FateGift can still save the target. | c2930a |
| BloodThrist | A kill in any path gives +1 action. | c2a296, c2a2f9, c2a35c |
| Assault | Turn 1 only: AB, AS, DB and DS ×2 when the enemy's first unit has building defence ≥ 10. Damage taken ×**2/3**. The damage test reads a misaligned field (medium confidence). | c29c97, c2a3d9, c2a403 |
| EternalGift | See section 1: base-stat changes that last the battle and stack. The Life blessing lowers defence (bug). | c29e59 … c2a0f8 |
| FateGift | Once per battle, a hit that leaves it at HP ≤ 0 instead:<br>• refills its actions;<br>• gives all protections +20 and regen +20;<br>• raises max HP by 20% with a full heal;<br>• gives the initiative modifier +5 (this turn).<br>The bonus is then erased. Poison and bleeding deaths are not saved. | c2937f |
| Evasion (unit field) | The last step of physical damage, counter and preventive strikes included: `max(1, dmg × (100 − E) / 100)`, from a table per unit type. No shipped unit sets it. | c2a802 |
| Garrison (Community fix) | At each turn start, if building defence is **exactly 10**, the attack modifier gains AttackShot. That also counts for melee. | c2651a |

## 8. Other battle rules

**Turn order** (489ca0, 4840ec). **code**
- **Threshold scan.** The battle keeps a descending initiative threshold T.
  - On turn 1, T starts at **75**. On later turns it starts at the threshold at which the first
    unit acted the turn before.
  - At each T it scans side 1 (the player) units in list order, then side 2.
  - A unit acts if `initiative + initModifier ≥ T` and it has actions left. It uses all its
    actions in a row before the scan moves on (the scan position does not advance while the
    unit still qualifies).
  - After a full scan T drops by 1. At T = 0 a new turn starts.
- **The cap.** T never starts above 75 on turn 1, nor above the previous turn's first acting
  threshold later. Initiative above the starting T makes no difference: such units tie at T.
- **Ties** go to the **player's side**, then to list order (army order), whoever attacked.
  Units with effective initiative ≤ 0 never act.
- **Initiative changes** made mid-turn (bless, curse, Stun) take effect at once. A unit already
  passed by the scan at the current T waits until the next T.
- **List shifts.** A unit that dies shifts the later records down; the scan position is an
  index, so the unit after a dead one earlier in the list can be skipped at that T.
- **Stall guard.** If 151 side switches pass without an actor, 489ca0 returns a "no actor" code.
  The pre-simulation loop (48b75c) and the off-screen loop (4a0710) then skip the action and
  ask again. **code** (the battle screen's handling is **unknown**)
- **"Attacker +1".** Battle setup adds +1 base initiative to **side 1 = the player**, always
  (48b917). There is no code that gives it to the enemy. Off-screen, side 1 is the attacking AI
  army, so there the attacker does get it.
- **Artillery** is not "always first". It gets **+30 initiative on turn 1 only**, and +30 more
  when its building defence is ≥ 10 (484365–4843d8). The bonus goes to the unit's current
  initiative, not to its initiative modifier, so the tests that read the modifier (the
  Elemental AI's haste candidates) do not see it. Community FirstShot does the same (c28935).
- **+1 action on turn 1:** HorseAtack, OldVampirsGist and FastDead (48431a).

**Start of each turn** (4840ec, 4843f3, 484683), unit by unit in list order, side 1 first:
1. attack, defence and initiative modifiers are reset to 0;
2. actions are set to Manevres, and initiative to its base; the reserve flag is set to 1;
3. turn 1 only: the +1 action and Artillery bonuses above;
4. the Community turn-start hooks (Hunger, Berserk, Fortify, Garrison fix, Flock, Bastion, …);
5. the blessed and cursed flags are cleared;
6. from turn 2: magic power drains (section 1); then, if the unit's regen is not 0,
   **regeneration and poison**: `HP += round(maxHP × regen / 100)`, capped at max HP. The
   rounding is Delphi's (half to even). There is no minimum of 1. A unit that reaches 0 or less
   dies and is removed (4846c1–4847e9), and its side may collapse.
- Poison and regeneration losses do not count in the side's "damage taken" (they do not go
  through 48a354), and a unit killed by poison triggers no on-kill effect.

**Poison (vanilla)** (48b299, 48b398). **code**
- A Poison attacker's physical hit of more than 1 damage sets the target's regen field to
  **−20**. The target then loses **20%** of max HP per turn, and its own regeneration is replaced.
- The in-game text says 15%. The Community changes are in section 7.

**Vampirism** (48b3d4). **code**
- After a **melee or long strike** the attacker heals `Vampirizm × dmg / 100`, where dmg is the
  **uncapped** computed damage (overkill counts), capped at its max HP. It does not apply against
  Undead or Elemental targets.
- **Signed.** The multiply and divide are signed (IMUL, IDIV: the quotient truncates toward zero)
  and there is no sign test (48b3d4–48b42a). A **negative** Vampirizm (an item's −10%) takes HP
  from the attacker. Only the max caps the result (48b460); there is no lower bound. An attacker
  drained to 0 or less is **not removed** by its own strike: the removal test after a blow reads
  only the target's HP (48b580). An attacker removed by a counter blow (48b566) is the exception
  (see Counterblow). The unit's stat keeps its negative value (the stat is not floored).
- **Shots never heal**: the shot path does not reach this code. (No shipped shooter has
  Vampirizm; an item could give it.)
- Magic: Death-school strikes only (section 3).

**Counterblow** (48b49f–48b579). **code**
- The constant at 4ed384 is 1. After a **melee or long strike** (not a shot or spell), a
  surviving Counterblow target strikes back with melee damage (kind 4), even against a long
  strike from afar.
- There is no warrior check: a unit with AttackBlow 0 counters for 1 (plus its attack modifier).
- The counter does not trigger vampirism or poison. A counter that kills the attacker removes it,
  with no on-kill effects. The counter's damage is not counted in the side's damage taken.

**On a kill** (48a3f0). **code**
- It runs for kills by melee, long strike, shot and hostile magic, not for deaths by poison,
  counter blows or Community side effects.
- **DeathCurse target:** the killer dies.
- **Ghost target:** the killer dies only if its **ProtectDeath** (+0x44) is below
  `30 × the Ghost's Manevres` (+0x50). With 1 action, a killer with Death protection ≥ 30%
  survives. So the vanilla Ghost curse is conditional.
- A killer that dies this way adds its remaining HP to its side's damage taken; it is removed
  before the target.

**Garrison** (side build 49861d). **code**
- In a building with defence ≥ 10, a Garrison unit's AttackBlow, DefenceBlow and DefenceShot are
  doubled at battle start. **AttackShot is not doubled.**
- The Community "garrison works for shooters" patch is in section 7.
- The ×2/3 damage taken also needs building defence ≥ 10.

**Building defence.** Each side carries its army's building defence (49855c), and setup copies
it into every unit of that side (48b75c). Physical damage adds the **target's** value. **code**

**Hero 1 HP** (4906a0). **code**
- After the battle, if any unit of the player's army has HP > 0 and the hero (unit 1) has 0, the
  hero's HP is set to 1. The same fix appears in an event path (4b0baf).
- AI armies: the off-screen battle sets the leader's HP to at least 1 for a surviving army
  (4a4c68, world/AI side). For an AI army beaten by the player it does not matter: the army is
  destroyed.

**Knight:** −20% physical damage in this build (section 0).

**First-turn rules.**
- SpearDefense ×3 melee defence on turn 1 only.
- FasterAttack is Community, see section 7.

## 9. Setup and deployment

**Battle window** (4d1fb0–4d2364). **code**
1. **Who fights.** The army that **attacks** brings only its paid units; the **defending**
   army fights with all its living units. When the hero walks into an enemy (4ad94c) the
   player's unpaid units stay out; when an AI army attacks the hero (4ade3c) or an event starts
   the battle, they fight and the enemy's unpaid units stay out. (49855c takes a unit when its
   HP is not 0 and, unless all units are wanted, its paid flag is set.)
2. **Player's empty front row:** fixed in the army formation, as in section 2.
3. Both armies' stats are recomputed (4a16d4) and the two sides are built (49855c): unit
   records, building defence, the knight flag, the Garrison doubling, then auto-arrange (483b3c)
   and the side strength (483ecc).
4. The player's side is marked as under manual control, and its grid is replaced by the
   **army's own formation** (4d2233).
5. The enemy keeps the **auto-arranged** grid, and that arrangement is written back into the
   enemy army (4988c0), replacing whatever formation it had. This write-back runs in its
   "all units" mode: it first sets the HP of **every** unit of the enemy army to 0 and then
   copies back only the units that are in the battle. When the enemy brought only its paid
   units (it attacked, or an event started the battle), its unpaid units are therefore left
   with HP 0 in the army record, the value that marks a dead unit. **code**; whether anything
   later restores them, and whether AI armies ever have unpaid units, is **unknown**.
6. Battle setup (48b75c) with the AI mode of section 4, then the interactive flag is set and
   the battle runs (4c57bc). No deployment step was seen between the window opening and the
   first turn (**unknown** whether the screen offers one).

**Auto-arrange** (483b3c, 48395c). **code**
- With 6 columns the blocked cells are set to −1 first.
- Each unit is placed in the first empty cell of a row, scanning columns in the preferred order
  (section 6).
- Step 1: the unit with the highest "front value" goes to row 1. The value is
  `HP × v + 1` with `v = Manevres × AB + DB`, plus DS for a warrior, ÷3 for a shooter, ÷5 for a
  mage (roles as in section 4). A Ghost uses `6 − Manevres` instead.
- Step 2: up to 5 times, the unplaced non-warrior with the highest strength (the army unit's
  tactical value, experience.md) goes to row 2.
- Step 3: the rest, strongest first: a warrior tries row 1, then the reserve, then row 2;
  anyone else row 2, then the reserve, then row 1.
- Ties go to list order (strict "greater"). A unit with strength 0 is never picked in steps 2–3
  and stays off the grid. **code** (no such unit is known in the data).

**Where a new unit lands in the army formation** (495ce0, 495fac). This is the army's own
3 × (4 or 6) grid, the one the player's side uses in battle (step 4 above). **code**
- **Adding a unit** (495ce0). Hiring in a building (4c7380), AI hiring (4a548c), the starting
  units at map load (4b2504) and units given by events (4a8e66, 4a8fe5) all go through it. Garrisons
  use it too. At map load the formations it builds do not last: the load then auto-arranges
  every army, the hero's included, through a battle side and back (0x49855c, 483b3c, 0x4988c0;
  saves-data.md §10.1 step 9).
  - It refuses when the army already has 12 units. The new unit gets the next number n.
  - With 6 columns it first writes the −1 blocks into the six unused cells (back row columns 1
    and 6, reserve columns 1, 2, 5, 6). This is unconditional: a unit standing in one of those
    cells loses its place.
  - It then scans the **reserve first, then the back row, then the front row**, columns in the
    preferred order (section 6), and puts n into the first empty cell. The unit's role plays
    no part. With 4 columns a new unit goes to reserve column 3, then 2, 4, 1; with 6 columns
    to reserve column 4, then 3, then the back row 4, 3, 5, 2.
  - If no cell is empty the unit is still added, without a place on the grid.
- **Making sure a unit is placed** (495fac, for unit i).
  - It sweeps all cells: a negative cell, or one that holds a number above the unit count,
    becomes empty, and any second cell holding i is emptied.
  - If i is then nowhere on the grid, it places unit **n** (the last unit, not i) with the same
    reserve → back → front scan, but **without** writing the −1 blocks again.
  - With 6 columns the sweep has just turned the blocks into empty cells. Columns 4 and 3 come
    first in the scan, so a unit lands in a formerly blocked cell (first reserve column 5) only
    when those are taken.
  - Callers: the end of a battle (4c50ec) runs it for every unit of the player's army and of
    the other army after the formation write-back; an event that moves a unit into another
    army (4a8e66, 4a8fe5) runs it for the moved unit.
- So in the 6-column mode, after any battle the armies' formations have no −1 blocks until the
  next unit is added. **code**; the visible effect on the army screen is **unknown**.

**Battle setup** (48b75c). **code**
- Copies both sides in, sets the subtractive damage mode, computes `⌊2·CurseMainSpell/3⌋`, both
  side strengths, the start count and the start HP sum of each side.
- Every unit of side 1 gets +1 base initiative; every unit gets its side's knight flag and
  building defence; the XP counters are zeroed.
- The turn counter and the threshold are 0, so the first actor request starts turn 1.
- Then the pre-simulation (section 4) and the restore.

**Side strength** (483ecc) and the unit strength used here are those of experience.md §3.

## 10. Off-screen battle (4a0710)

When two AI armies meet, or when the AI rates a target (4a08f8), the game plays a full battle
with the same engine and no screen. **code**
- Side 1 is the attacking army with its paid units only; side 2 is the defender (an army or a
  building's garrison) with all its units. Both are auto-arranged.
- Setup is called with AI mode 0, so the pre-simulation runs too, and both sides use the
  simple "killable" test with its wrong-side reading (section 4).
- The interactive flag is off: no Splash.
- Side 1, the attacker, gets the +1 initiative.
- The loop is the same: pick an actor, perform its action, until the end check fires. The
  result passed back is the number of turns played; a battle that reached BattleEndTurn counts
  as "no result" in the AI's target scoring (4a08f8).
- How the result is applied (losses, XP, loot, destroyed armies) is world/AI logic (4a4c68),
  described in world.md and economy.md.

## 11. End of battle

**During the battle** the screen syncs the sides back after every action (48bb10 without the
wrap-up) **and writes them into the army records** (4988c0 for both sides, as at the end, step
3 below): after the player's action (0x4c4f8c), after each of the enemy's (0x4c57bc) and
when a card's move ends (0x4b0284). So the armies' units carry their battle HP as the battle
goes on (a fallen unit 0), and their grids the battle's cells. **code**; checked in the
running game (rk1-day1: the knight's army record read 63 HP right after the blow of step 22;
FINDINGS.md §7).

**Wrap-up** (48bb10 with the flag, from 4c50ec). **code**
1. Both side strengths are recomputed and the XP shares are rolled (experience.md §3).
2. A side that surrendered: the Surrender sum of its remaining units is stored on the side, and
   all its units are removed.
3. The sides are copied back to the armies (4988c0): HP, the per-battle stat blocks, and the
   army grid rebuilt from the battle grid (with the wide-row blocks restored, and units that
   did not fight placed in free cells, reserve first). That placement (4988c0) takes the units
   that sat out, then every unit with HP 0, each in the first free cell of the reserve, then
   the back row, then the front row, scanning the columns in plain order from the first (not
   the preferred order of section 6). **code**

**Outcome** (4c50ec). **code**
- **Defeat** if the player's side has no unit (game-over path).
- **Victory** otherwise, the turn limit included: loot and trophies (economy.md), the beaten
  field army destroyed, a garrison's building captured, the enemy's surrender mana added, then
  the player's XP (Community hook c2518f).
- **A won garrison battle does not enter the building.** The hero stays on the cell he
  attacked from (the garrison is engaged before his step onto the building, world.md §4.2);
  the building is captured but not entered (0x68dc74 stays none), no building window opens
  and its local events are not scanned. A click on it afterwards plans a route and walks him
  in: the window opens on arrival (world.md §7.2). **live**: РК1's ruins 8 (2×2 at (36,23))
  won from (34,24) in the diff test (run q3-ruins): after the result box the screen is the
  world, the hero at (34,24), the ruins owner 0, entered −1; `click_map 36,23` walks him to
  (36,23) and the building window opens (entered 8).

## Razdor now → original

Razdor's code as of this check: `src/rules/battle.rs`, `formation.rs`, `game.rs`
(`start_battle`), `ai.rs` (AI battles and `simulate`), `src/ui/battle_view.rs`. "Matches" rows
were implemented and tested earlier (`src/rules/battle/tests.rs`, `rowN_…`); the rows marked
**differs** are open.

| # | Topic | Razdor now | Original (this exe) | § | Status |
|---|---|---|---|---|---|
| 1 | Buff/curse duration | Modifiers reset every turn | Until the start of the next turn | 1 | Matches |
| 2 | Stacking | Additive; curse then strike; **main:** a unit is blessed at most once a turn, a blessed ally (in rows 1–2 or the reserve) is a target only for a heal that does something, a Splash follow-up does not bless a blessed neighbour again (the owner's decision, 2026-10-17); `dt-original` keeps the original's rule | Blessed allies untargetable unless wounded, but a wounded one with a heal of 0 (a Death caster on a living ally, a Life caster on an Undead or Elemental one) is blessed again, as is any reserve unit by a reserve caster, and Splash re-blesses neighbours: blessings stack | 1, 3 | Razdor fixes the original's bug (main only, a deliberate deviation) |
| 3 | Bless and curse sizes | `bless_effect` / `curse_effect` with the ini divisors, Life curse ⌊2·CM/3⌋ and 10; the attack modifier given to units without an attack too (only the hover hides it); a caster with no school sets only the flag | Same | 1 | Matches |
| 4 | EternalGift | Base stats, stacks; the Life blessing raises the defences | Base stats, stacks, Life blessing lowers defences (bug) | 1 | Razdor fixes the original's bug |
| 5 | Magic drain | `max(MP − drain, floor)` if MP > 0, drain and floor per type (none for a type without MagicPower), Undead Death floor +25 on the default floor only, Concentration adds; an install whose exe lacks the patch (`DrainRule::Old`, from the exe at 0x484466, or `MinSpellLife` without an exe) runs the original's block: fixed floors 15 / 15 / 25 by school and nature | Same (community-patches.md §10); without the Community patch: the block at 0x484466–0x484683 (the rules above) | 1 | Matches |
| 6 | Hostile power rounding | `P·(100 − prot) / 100` rounded half to even (the product taken as exact; the FPU precision is unknown, engine.md) | Floating-point product, Delphi round half to even | 0 | Matches |
| 7 | Strike with power 0 | GodAnger/GodStrike added whenever the caster has MP | GodAnger/GodStrike still added when the caster has MP | 0 | Matches |
| 8 | Hostile mage action | Curse if no negative modifier, else strike; one action per cell | Same | 3 | Matches |
| 9 | Friendly mage | Heal if wounded and heal > 0, else bless; reserve casters tend the reserve, NoHeal-marked units too; **main:** never a second blessing in a turn (row 2) | Same, with the second blessings of row 2 | 3 | Matches but for row 2's deviation (main only) |
| 10 | Own-cell self-cast and the space key | Own card and space: one action, a self-cast if offered (a reserve caster's own cell only while wounded or unblessed), else a pass | Own cell and space are the same thing: **one** action, pass or self-cast | 2, 3 | Matches |
| 11 | Edge columns | Columns 1 and last are never "clear" (`front_clear`) | Columns 1 and last are never "clear": front-row shooters there hit only c±1, front-row mages there cannot cast | 3 | Matches |
| 12 | Map priority | Melee, then shot, then magic, later wins | Same | 3 | Matches |
| 13 | Ghost casters | Reach the opposite front cells from any row, with no direction test, with any magic power above 0; written after Flying's melee | Same, but the power test reads the power's low byte as signed (128..255 fails: bug) | 3 | Razdor fixes the original's bug |
| 14 | Undead caster drain | Caster gains the full drain; only the target's loss is capped | Caster gains the full drain; only the target's loss is capped | 3 | Matches |
| 15 | Vampirism | Melee and long strike only; any non-zero value, signed (negative takes HP from the attacker, no lower bound, capped at max only); not from Undead or Elemental targets; Death strikes | Melee and long strike only; signed, as the original (48b3d4–48b42a); Death strikes still ignore a negative value (see §3, open) | 8 | Matches on the melee path; the magic path open |
| 16 | Into the reserve | Front or back row, one reserve transition per turn | Same | 2 | Matches |
| 17 | Collapse timing | Only after a death and after the actor's last action | Only after a death and after the actor's last action; never at a turn start | 2 | Matches |
| 18 | Battle start | At `begin()` (after Razdor's deployment) only the player's back row moves into an empty front row; it persists. The front counts as taken while a unit that does not fight (a corpse, an unpaid unit of an attack) holds a cell there (`Battle::set_bench`) | Only the player's army back row moves into an empty front row, in the army formation (persists) | 2, 9 | Matches |
| 19 | Reserve collapse | Reserve to row 1, actions 0 | Same | 2 | Matches |
| 20 | Wide formation | Per-side battle grid: the enemy's has the blocks, the player's none (4d2233); the player's side, the hero's auto-battle and «Максимум» included, steps only to the 12 cells the screen shows (`Battle::shows_move`, the same set the player's input offers); the enemy's moves are unchanged; a collapse copies a row's blocks forward and opens the row left | Same, but the player's battle grid has no blocks (4d2233) and a collapse moves the blocks between rows; its screen shows 12 places (492940) | 6, 2 | **differs** (a Razdor fix, the owner's decision: the opened cells the screen does not show are not steps; what the original shows for them is unknown) |
| 21 | Enemy formation | Auto-arranged every battle (`Battle::auto_arrange`); off-screen both sides. Not written back to the enemy's troops: a beaten army or garrison is gone and a lost battle ends the game, so it shows nowhere | **Auto-arranged** every battle (483b3c) and written back to the army; off-screen both sides | 9, 10 | Matches (write-back: no effect) |
| 22 | Who fights | The attacker's unpaid units sit out, the defender's fight (AI troops have no unpaid units). Who attacks is Razdor's: walking into a garrison is the player's attack, an army contact the army's | The **attacker's** unpaid units sit out, the defender's fight | 9 | Matches (who attacks: world.md) |
| 23 | Deployment | A deployment phase before the first turn (`move_card`) | None seen; the battle starts from the army formation | 9 | Razdor extra (**unknown** in the original) |
| 24 | Turn order | Threshold scan from 75, ties to the player, cursor by index | Same | 8 | Matches |
| 25 | Attacker +1 initiative | Side 1 (the player; the attacker off-screen) | Same | 8 | Matches |
| 26 | Artillery | +30 (+60 with building defence ≥ 10) on turn 1, to the current initiative, not to the modifier (FirstShot too); pierces shots | Same | 0, 8 | Matches |
| 27 | Piercing set | Melee: ArmorIgnore, PoisonArmorIgnore, both vampire gifts; shots: ArmorIgnore, PoisonArmorIgnore, Artillery | Same | 0 | Matches |
| 28 | Defence order | SpearDefense, piercing, long strike, then building (+Row2Def for shots); the attack modifier added to an attack of 0; Unvulnerabe/Ghost 1 with GodAnger/GodStrike on top | Same | 0 | Matches |
| 29 | Knight | Any side whose first unit is of the Knight type, AI lords included | Any army whose first unit is of the Knight type, AI lords included | 0 | Matches |
| 30 | Counterblow | Any Counterblow unit, after melee or a long strike; its kill (and a preventive strike's) has no on-kill effects | Same | 8 | Matches |
| 31 | Ghost | Killer dies if ProtectDeath < 30 × Manevres | Same | 8 | Matches |
| 32 | Garrison | ×2 AB, DB, DS (≥ 10); Community +AS at exactly 10 | Same | 8, 7 | Matches |
| 33 | Regen and poison | `round(maxHP × regen/100)`, half to even, no minimum | Same | 8 | Matches |
| 34 | Damage-taken counters | Only the damage routine's wounds and a cursed killer's HP count in `lost`; counter blows, preventive strikes, poison, bleeding and the Community side effects do not | Not counted in the side's damage taken | 8 | Matches |
| 35 | Turn limit | Ends after the first action of turn 25; a win if the player has units | Same; the beaten army is destroyed | 5 | Matches |
| 36 | Surrender | Whole side gives up; Surrender sum as mana; the value read as a byte; tested only while both sides stand (a win with only surrender-capable units left is a win) | Same, but tested for each side with units even when the other side is gone, so such a win is a defeat (bug); only the player can receive the mana | 5 | Razdor fixes the original's bug |
| 37 | Bonus count | One bonus byte, the last item wins | Same | 7 | Matches |
| 38 | AI framework, melee/shot scores and moves | As section 4, in integers; the poison bonus for vanilla Poison only, a kill replacing the doubled score; reserve units go straight to the moves; the moves' weights as 489549 (3·\|MP\| support, unfloored front-row pull with the own cell a candidate, reserve mages tending any reserve target by its wound, the second-column start only for non-warriors; a front-row caster's fallback may pick an ally's front cell it can tend, in any column); the fallback is the own cell (pass or self-cast) | Same | 4 | Matches |
| 39 | AI shot "Manevres 1 ÷2" | Only to back-row mage targets | Only to back-row mage targets | 4 | Matches |
| 40 | AI front-row retreat | Stat test and a non-warrior role; a lone unit only as a mage by role; the edge rule (score 1 on the first or last front cell) when no back cell is free | Also needs a non-warrior role; the 4ed390 edge rule | 4 | Matches |
| 41 | AI "killable" (normal level, off-screen) | The target's own HP ≤ dmg; the off-screen battles still carry the static sides' old records (`SideRecords`), which nothing reads then | Reads the own unit with the target's index (bug; past the list a record a death emptied, or one an earlier off-screen battle left) | 4 | Razdor fixes the original's bug |
| 42 | AI Life scoring | As the code read (486bb9): heal, bless by rows with or without enemy shooters, the curse value on both defences (DB and DS) on the strike power, the cursed flag | Medium confidence; the curse value's second term compares DS but adds DB (slip) | 4 | Matches the reading; Razdor fixes the original's bug (DS/DB) |
| 42a | AI Elemental scoring | Main and alternative per side, cells scanned row by row; front-row haste scaled by hits-to-kill; the ÷10 rule with its exceptions; strike whenever a slow is impossible, slow + strike with a spare action; ×10 with an all-Ghost side; the turn's mean initiative | Front-row haste scaled by hits-to-kill (4863e8); ÷10 rule has GodAnger/GodStrike and school exceptions; strike whenever a slow is impossible, slow + strike summed with a spare action; ×10 strike with an all-Ghost side | 4 | Matches |
| 42b | AI Death scoring | As section 4 on the strike power; the self-target passes when no self-cast is offered; a threat of no damage is the role's minimum; blessings weigh the target's actions | Same, except the self-target: the original picks its own cell even with no self-cast on offer (a pass) | 4 | Matches |
| 43 | Pre-simulation | Played at `begin()` on a copy (AI on both sides, no Splash follow-ups but for heals and blessings); each side's loss is the pool's predicted loss; skipped only for the AI's target scoring, which pays no XP | A full AI-vs-AI copy before every battle; keeps only each side's simulated damage taken for the XP | 4, 9 | Matches |
| 44 | Off-screen battle | `set_simulation` (no Splash follow-ups but for heals and blessings, AI mode 0), both sides auto-arranged with the wide blocks, attacker paid only | Same flags; auto-arranged formations; attacker paid only | 10 | Matches |
| 45 | AI target-scoring battle (`ai::simulate`) | Mode 0, no Splash follow-ups but for heals and blessings, both sides auto-arranged, the defending player with all his living units | Same engine as the off-screen battle: mode 0, no Splash | 10 | Matches |
| 46 | Community bonuses (Hunger … FateGift) | As section 7; the turn start runs unit by unit (bonuses, then drain and regeneration) | Same | 7 | Matches |
| 47 | New unit's formation cell | Reserve, then back, then front, columns in the preferred order, for everyone (`Formation::new_unit_slot`: hiring, AI hiring, map start, event units); at map start the hero's army is then auto-arranged (`Game::arrange_at_load`), as the load's round trip does. The unused wide cells stay blocked: a formation has no cells outside the 12 | Reserve, then back, then front, for everyone; 6 columns re-block the unused cells, and the battle-end clean-up unblocks them | 9 | Matches (the unblocked cells after a battle are not modelled; their effect on the army screen is unknown) |
| 48 | Formation after a battle | The battle grid as it ended; units without a cell (on a cell outside the formation, sat out, then the dead) take free cells, reserve first, columns in plain order (`Formation::after_battle_slot`) | Rebuilt from the battle grid, blocks restored, units not in it placed reserve first (4988c0) | 11 | Matches |
| 49 | After a won garrison battle | He stays on the cell he attacked from; the building is the player's but not entered (`Game::resolve_battle`): no window, its events wait; a click on it walks him in and opens it | Stays on his cell; captured, not entered (0x68dc74 none), no window; a click walks him in (live, РК1 ruins) | 11 | Matches |
| 48a | Armies during the battle | every fighting unit's HP written into its army record after each action (`Game::battle_write_back`, called by the battle screen and the replay); the grid only at the end | sides written into both armies after every action (0x4c4f8c, 0x4c57bc → 48bb10, 4988c0): HP and grid | 11 | HP matches; the grid at the end only (no reader during the battle) |
| 50 | Poison on the Undead | **main:** no poison takes hold on an Undead unit (Poison, PoisonS, PoisonArmorIgnore, CtrPoison on an Undead striker, the mages' poison), and the AI counts no poison on one (the owner's decision, 2026-10-18; `Battle::poisonable`); `dt-original` poisons them | Every poison works on every nature (48b299–48b2bd, 48b398, c2639c, c2a27c, c25c43, c259da) | 8 | Deviation (main only) |
| 51 | Formation after a battle with the optional «Сохранять расстановку после боя» (default on, the owner's request, `Game::keep_formation`) | Every unit returns to the cell it had before the battle, the dead too; a cell now outside the formation or taken falls back to row 48's rule (`Game::return_to_cells`). Taken at `start_battle`, before the start fix | Rebuilt from the battle grid (row 48): the cells the battle left, after the start fix (4d2141) and the collapses (489f50) | 2, 9 | **differs** (a Razdor deviation; off, Matches row 48) |

## Unknowns and open points
- **Wide-row quirks on screen.** What the screen shows when the player's side uses a cell that is
  blocked in the formation, or after a collapse has moved the −1 blocks (sections 2 and 6). Razdor
  keeps every player's step to the 12 places the screen shows (row 20, `Battle::shows_move`); what
  the original shows for the other cells is still unknown (docs/OPEN_QUESTIONS.md).
- **Deployment.** Whether the original battle screen lets the player rearrange units before the
  first action (nothing seen in the window's open handler).
- **AI kill-test bug in play.** The wrong-side "killable" reading (486d03, 486feb) is read in the
  code and confirmed in the running game through the off-screen battles' old records
  (FINDINGS.md §27). Razdor fixes it (row 41).
- **AI magic scoring.** Life magic scoring is medium confidence; Elemental and Death were read
  in full (section 4). Why the Elemental ÷10 rule reads the caster's direction is unknown.
- **Low-confidence hooks.** Flock's army-size source and Assault's damage test are medium
  confidence. The Manevres-0 hook at c25b63 uses a garbage constant (low).
- **Per-turn pass counter.** Who reads unit +0x99 (reset to 1 every turn, +1 per pass).
- **XP side fields.** Settled: side +8 (the pre-simulation's predicted loss, 48baa3) and +0x14
  (the largest per-turn loss, folded in at 4840ec) are described in experience.md §3.
- **Unblocked wide cells.** After a battle the 6-column army formations have no −1 blocks
  (495fac, section 9); what the army screen shows for a unit in such a cell is not seen.
- **Enemy unpaid units at HP 0.** The pre-battle write-back of the enemy formation (4d2304)
  clears the HP of enemy units that stayed out (section 9); whether the world code later
  purges or restores them is not traced.
- **"No actor" code.** How the battle screen (4c57bc) handles the stall code; and a turn in which
  nobody acts would leave the threshold at 0 and stall the scan for good (unreachable with the
  shipped data, whose lowest initiative is 3).
