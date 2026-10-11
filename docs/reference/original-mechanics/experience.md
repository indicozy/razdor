# Discord Times experience: levels, battle XP, promotion

This file covers the experience system of the Community Update (Unstable) `DiscordTimes.exe`
(Delphi, image base 0x400000): unit strength, the XP a battle pays, levels and what they
add, promotion through the upgrade tree, and every other source of XP. The rules are in our
own words; addresses are virtual addresses in that build, given as evidence only.

Confidence tags:
- **code**: confirmed by reading the code.
- **data**: consistent with the data files, the texts or the footage, but not traced in code.
- **unknown**: not determined; Razdor keeps a documented guess.

Revision 2026-10-01: §2 (hit points after a level), §3 (the pool has a live
"prediction" term, the turn limit pays XP, a surrendering side gets nothing, the cap is
an absolute value), §4 (worn items stay on promotion, the AI's random source, the slot
layout), §5 (opcode 13 details, hire XP for garrison purchases) were corrected against the
code. A second check the same day added the float-precision caveat (§0), the PreventiveStrike
and start-of-action counter cases (§3), the always-made AI promotion roll (§4), and
corrected the extended-event XP skip and the opcode 13 index bug (§5).

## 0. Conventions

**Level numbers.** The exe stores a unit's level from **0**: a hired unit, the hero at the
start and a unit an event adds all get level 0 (AddUnit 495ce0 called with 0 from the
barracks 4b0fab, the hero 4b4383 and events 4a94d3). The interface shows it plus one: the
footage has a fresh unit at level one with 0 of 580 XP. Troop levels in a map (preset,
army, garrison triples, army byte 27) are stored 0-based too and copied as they are
(4b44e9, 4b52fb, 4b46cf). Razdor counts from 1, so **Razdor level = exe level + 1**; the
formulas below use `L` for the exe's 0-based level. **code**

**Rounding.** `Round` (402dd0) is an x87 store with the default rounding mode: halves go to
the even neighbour. `Trunc` (402ddc) and `Frac` (402dac) switch to chopping. Integer `div`
truncates toward zero. **code**

**Random numbers.** Every roll in this subsystem uses the game's own linear congruential
generator `rand(n)` (4832fc): `seed ← seed·0x343FD + 0x269EC3` (32-bit wrap), result
`((seed >> 16) & 0x7FFF) mod n`, and 0 for `n = 0` (the seed still advances). The seed is
one global (0x659154) shared with the AI, the world and the battle picker. **code**

**Float precision.** Where this file says "80-bit", "64-bit" or "32-bit" it names the format
the code loads and stores. The precision the x87 unit computes with at run time is not
known: after the Direct3D set-up it may be single precision (engine.md §1). That would change
the last bits of every float result below, and a value close to a half could round the other
way. **unknown**

**Records.** A unit in an army is 0x1DB bytes: +0 type (0-based), +4 XP towards the next
level, +8 the last gain (shown as "+N" on the unit card after a battle), +0x10 level `L`,
+0x20 HP (−1 = unhurt, 0 = dead), +0xCD four worn item ids, +0xDD current stats with items
(+0xDE its maximum HP), +0x11D stats of the level without items, +0x1AA and +0x1AE the two
strength values of §1, +0x1B3 a fractional HP carry (§2). In battle a unit is 0xA5 bytes
(layout in the battle notes); +0x64 is its strength, +0x68 its battle role, +0x6D, +0x71,
+0x91 the action counters of §3, +0x75 its row and +0xA0 its XP award. A battle side
(0x851 bytes) keeps +4 the unit count at the start, +8 the predicted HP loss, +0xC the HP
lost, +0x14 the largest loss in one turn, +0x1C the HP at the start, +0x7E8 / +0x7EC the
strength now / before, +0x7F4 the pool. **code**

## 1. Unit strength (tactical cost)

The strength of a unit is computed from its **stats**, not from its price (49fc50, called
through 4a02a0). `Cost` plays no part. **code**

Inputs, from the unit's stat block: maximum HP `H`, melee attack/defence `AB`/`DB`, ranged
attack/defence `AS`/`DS`, magic power `MP`, school and direction, protections `PL`, `PD`,
`PE`, `Regen`, `Vamp`, initiative `I`, actions `M`, the bonus; and `bd`, the defence of the
building the unit's army stands in (army +0x378C, copied into every unit at 4a17c8).

1. If `H = 0` the strength is 0.
2. **Toughness.** A unit counts as melee unless `AS > AB and AS > MP` (a shooter) or
   `AB/2 < MP and AS/2 < MP` (a caster; real division).
   - Melee: `x = DB + bd`, `y = H·Regen/100 + DS + bd`.
   - Others: `x = max(0, DB + bd − 5)` (0 when `DB + bd ≤ 5`), `y = H·Regen/100 + DS + bd + 5`.
   - `D = round(H · (e^(x/21.5)/1.17 + e^(y/30.3)/1.07))`.
   - Bonus: SpearDefense ×1.15; Unvulnerabe or Ghost `D = 10·H`; VampirsGist,
     OldVampirsGist or Evasive ×1.5; Garrison ×2.
   - `T₁ = H + D`; Dead or FastDead ×1.7.
   - `T₂ = T₁ · (1 + (PL + PD + 1.5·PE)/560)`.
3. **Attack value** `A`:
   - `A = AB` if `AB ≥ AS and AB ≥ MP`; then `A = 1.4·AS` if `AS ≥ AB and AS ≥ MP` (a tie
     goes to the bow).
   - ArmorIgnore, VampirsGist, OldVampirsGist or Artillery: `A += 18`.
   - If `M > 0 and MP > AB and MP > AS`: `A = MP`, then by direction: ToEnemy
     `A = 0.8·A·(M−1)/M + 0.2·A` (+25 for the Elemental school); ToAll `A ×= 1.2`; ToAlly
     unchanged.
   - Garrison `A ×= 2`; GodAnger `+10` (replayed by the Community hook c2502b); GodStrike
     `+20`; Counterblow `+AB`; FlankStrike `+ AB div 3`.
   - `A = M·A + E`, where `E = 0.15·A` for HorseAtack, OldVampirsGist and FastDead, else 0.
   - `A = A·(1 + I/100)` if `I > 0`, else `A = 0`.
4. `T₃ = T₂ · (1 + Vamp/100 · A/H)`.
5. `S = round(3.2 · T₃ · (A + 1) / 200)`; DeathCurse or Ghost `+150`; Poison `×1.1`
   (applied before the rounding).
6. If `AS ≥ ShotWeaponRange` (60 in `_Global.ini`) and the unit is not Artillery: `S = S div 3`.
7. `S = 1` when it came out 0.

The intermediate values are 80-bit floats; constants 21.5, 1.17, 30.3, 1.07, 0.15, 3.2,
200, 150, 1.1 sit at 4a01e4–4a0294. **code**

The **tactical cost** is `S × CostMultipler div 100` (4a02a0). When it is stored as the
battle value, a Community hook (c25d86) keeps a positive value, turns 0 into 1 and a
negative `x` into `|x| + 1`. The army keeps two: +0x1AA ("mode 0": the level stats without
items, `bd = 0`, no hook; used by AI hiring, §5) and +0x1AE ("mode 1": the current stats
with items and the building's defence; used in battle). The sum of +0x1AE over **every**
unit record the army lists (records 1 to its unit count, the dead included: the HP test
comes after the addition) is its "army strength" (army +0x1648, 4a182b), which the
scenario conditions compare. **code**

Worked example (a synthetic warrior: `H` 50, `AB` 20, `DB` = `DS` = 5, `I` 10, `M` 1, no
bonus): `D = round(50·(1.0785 + 1.1023)) = 109`, `T = 159`, `A = 22`,
`S = round(3.2·159·23/200) = 59`.

Two consequences of the data: the Community archers reach `AS 60` after a few levels and
their strength then drops to a third; a warrior-priest whose magic outgrows its melee is
valued by its (weaker) spell term.

## 2. Levels

**XP needed** from level `L` to `L+1` (492680 with the series 492630):
`need(L) = round(S(L+1) − S(L))`, where `S(n)` is the 80-bit sum of the first `n` terms of
`StartExpirience · r^i` and `r = LevelMultipler/100`. That is
`round(StartExpirience · r^L)` up to float noise. A fresh unit (L = 0) needs
`StartExpirience`. In Razdor's numbering: level `n` needs `round(Start · r^(n−1))`. **code**

Footage check: the fresh unit shown with 0 of 580 XP is `StartExpirience=580` (unit 28);
the level-two unit with 33 of 560 is unit 14 (`400 × 1.4`); the archmage hero at level five
with 500 of 590 is `90 × 1.6⁴ = 589.8`. **data**

**Gaining XP** (49273c), in this order: **code**
1. A gain below 1 counts as 0.
2. The gain (possibly 0) is written as the "last gain" (+8).
3. If it is not 0: `t = XP + gain`; repeat { `need = need(L)`; if `need ≤ t`: `L += 1`,
   `t −= need` } while that same `need ≤ t`; then `XP = t`.

Several levels can come from one gain; what is left is kept. Two quirks follow from step 3:
with `LevelMultipler < 100` the loop can stop one level early (the comparison uses the
previous level's need), and `StartExpirience = 0` would never end. The shipped data has
neither (every multiplier is 140 to 170, every start above 0). The gain itself changes no
stat and no HP.

**Stats per level** (4908a8, rebuilt from the type every time the unit's stats are
recomputed): **code**
- Hits, AttackBlow, DefenceBlow, AttackShot, DefenceShot, Initiative, Manevres:
  `base + d-stat × L` (4907a0).
- MagicPower: the same, but only for a type with a school; without one it is 0.
- ProtectLife, ProtectDeath, ProtectElemental, Regen, Vampirizm (4907c4): `x = 100 − base`,
  then `L` times `x = x − d·x/100`, each step rounded to a 32-bit float; the result is
  `100 − round(x)`, at most 99 (the cap applies even at level 0; there is no lower clamp).
- Then the worn items, potions and spells, as in economy.md.

**Hit points after a level** (4908a8, 101254–101275 of the decompiled code): the stat
rebuild keeps a wounded unit's HP proportional to its maximum. Before rebuilding it takes
the old maximum (the stored current maximum; if that is 0, the new level's Hits). After:
- HP −1 (unhurt) stays −1, so the unit is at its new maximum; HP 0 stays dead.
- Otherwise `v = newMax × (HP + carry) / oldMax` as a 32-bit float; the fractional part
  becomes the new carry (+0x1B3), the integer part the new HP; 0 becomes 1 (carry 0).
- A result above the new maximum becomes −1 (unhurt).

Because the XP gain does not rebuild the stats, the stored maximum is still the old one
when the next rebuild runs, so the rescale happens then (for the player's army right after
the battle screen's refresh, 497240 → 4a16d4). Example: 30 of 50 HP, the level brings 55
max: `55 × 30 / 50 = 33`. The same rule applies to any change of the maximum (items,
spells, promotion). **code**

## 3. Battle XP

**Pre-simulation** (48b75c, both callers). After the battle object is set up (per side:
the start count copied, the loss counters zeroed, the start HP summed, every unit's action
counters zeroed), the engine saves both sides, plays the whole battle once with the AI
choosing for both sides, stores each side's HP loss from that run in the side's
**predicted loss** (+8, 48baa3), and restores the saved sides. The player's battle passes
the AI level 1 or 2 (option 0x65d4da), AI-vs-AI battles pass 0; the run advances the
shared random seed. Nothing else writes +8. **code**

**Side strength** (483ecc; called when the side is built 49855c, at the setup 48b75c, and
at the end 48bb10, so at the end the "before" field holds the start strength):
- A side flagged as surrendered (+0x84A) has strength 0.
- Each unit counts `v = round(strength × HP / maxHP)` (the product is a 32-bit integer),
  by row: front `R₁`, back `R₂`, reserve `R₃`.
- A back-row unit whose **battle role** is not melee counts twice. The role (4836cc), from
  the battle unit's base `AB`, `AS`, `MP`: score = the largest of the three plus the other
  two summed and divided by 3 (integer), +10 GodAnger, +15 ArmorIgnore, +20 GodStrike, +AB
  Counterblow, +10 FlankStrike; a shooter if `AS ≥ score/1.5`, a mage if
  `MP ≥ score/1.5` (divided and compared on the FPU; only the constant 1.5 is stored as a
  32-bit float; the mage test wins), otherwise melee.
- `f = min(1, 0.8·R₁/R₂ + 0.2)` when `R₂ ≠ 0`, else 1; strength = `round(R₂·f + R₃ + R₁)`.
- A side reduced to one unit whose role is not melee counts a fifth (`div 5`). **code**

**What counts as lost HP** (side +0xC): every wound applied through the damage routine,
capped at the unit's HP (48a354), plus the remaining HP of a killer that dies by
DeathCurse or Ghost (48a45c, 48a548). Healing never lowers it; Counterblow damage,
PreventiveStrike, regeneration loss and poison ticks do not go through it. The largest
per-turn loss (+0x14) is folded in at each turn start (4840ec), so the turn in progress
when the battle ends is not in it. **code**

**The action counters**: every action adds 1 to "taken" (+0x71, 48a67e); melee (48b30a),
shots (48b21b), hostile magic (48ac7e) and friendly magic (codes 9/0xB/0xC, counted
before the heal or bless is tried) add 1 to "useful" (+0x6D); passes and moves count only
as taken. "Left" is the unit's actions left (+0x91) when the battle stops. **code**

Two Community hooks change the counts. A unit with PreventiveStrike gets 1 "useful" each
time it strikes first against an enemy's melee (c2a1b7) or shot (c28a3d), without any
"taken"; so its useful count can exceed `taken + left` and its share can exceed `4t` below.
An attacker killed by that first strike gets no "useful" for the cancelled attack. A unit
that dies from the Community damage applied at the start of its own action (c2a53c, before
48a67e) gets no "taken" for it either. **code**

**Per side** (48bb10, for side 1 then side 2, only if its start count `N₀ > 0`): **code**
- `E₀` = the opponent's start strength, `pred` = the side's predicted loss, `lost` = HP
  lost, `HP₀` = HP at the start, `maxTurn` = the largest per-turn loss.
- A dead value (+0x7F0, written and never read): `ratio = E₀ / own start strength`,
  `k = 1 ± |ratio − 1|·ExpCorrection/100` clamped to [0.25, 4], value
  `round(MainExpCorrection × (E₀ − E_end) × k / 100)`, or 0 when the own start strength is
  not above 0. `MainExpCorrection` and `ExpCorrection` therefore do not change the XP.
- `base = E₀ div 20`.
- **Pool** (stored at +0x7F4 after rounding):
  - `lost > 0` and `pred = 0`: `pool = base × max(0, (HP₀ − lost)/HP₀)`.
  - `lost > 0` and `pred > 0`: `q = pred/lost` clamped to [0.8, 3];
    `pool = pred·q + base + maxTurn`.
  - `lost = 0`: `pool = 3·pred + base + maxTurn` (integers).
  - Then `pool = round(pool)`.
- `t = pool × 0.25 / N₀`; `N₀` counts the dead.
- For each unit still in the side (the dead were removed during the battle):
  `share = (4 − row)·t + row·t·useful/(taken + left)`, row 1 front, 2 back, 3 reserve;
  with `taken + left = 0` the share is `t`. A share below 0.5 becomes 1; the award
  (+0xA0) is `round(share)`. So an idle front unit gets `3t`, an idle reserve unit `t`, and
  any unit that spent all its actions attacking `4t`.
- Then, for a side flagged as surrendered: the `Surrender` values of its units are summed
  (+0x7F8, the victor's mana, read at 4c5652), every unit's HP is set to 0 and its count to
  0. Its awards are computed but nobody is left to receive them.

The hero is an ordinary unit here: he shares the pool by the same rule if he is alive at
the end. **code**

Worked example: `E₀ = 1000` (base 50), `pred = 120`, `lost = 60` → `q = 2`, `maxTurn = 40`
→ pool `120·2 + 50 + 40 = 330`; with `N₀ = 5`, `t = 16.5`; a front unit with 2 useful of
3 actions gets `round(3·16.5 + 16.5·2/3) = round(60.5) = 60`. The same side without a
predicted loss: pool `round(50 × (HP₀ − 60)/HP₀)`.

**The player's units** (4c50ec with the Community hook c2518f). The battle counts as a
defeat only when the player's side has no unit left; a turn-limit end with any unit
standing is a victory (battle.md §5), and it pays. Before paying, every unit of the army
gets last gain 0 and its stats rebuilt. If the beaten side was a building's garrison, its
army record is cleared and given correction 100 (4c55b9). Then each survivor of side 1:
- `x = round(award × HeroExpirienceModificator × F × C / 1 000 000)` in 80-bit floats
  (64-bit result, low 32 bits used), where `F` is the difficulty factor (120, or 100 when the
  hardest-difficulty option `OptValue10` is on) and `C` the beaten army's correction byte (army
  +0x3823, map byte 71, used as it is, so 0 pays 0);
- `x = |x|`, at most **5256** (c25264);
- added with the gain rule of §2.

The cap is per unit and battle; XP left over after a level is kept. The integer version of
the product that the original code had (4c5676) is jumped over. **code**

`HeroExpirienceModificator` applies to **every** unit of the player, not just the hero.
With the shipped values (50, F 100) an award of 48 gives 24. **code** / **data**

**The video's rate (2026-09-29).** The gameplay video's fort battle (third campaign map,
09:49) checks the formula: the garrison's start strength is 1516, so `base = 75`, the pool
75, and a unit that attacked with every action gets an award of 25; the cuirassier, the
sorceress and the hero show +25, +24, +26. A pool of exactly 75 means the predicted loss
was 0 and either nothing was lost or the loss rounded away. So that game paid awards at
×1.0 (`HeroExpirienceModificator × F × C / 10⁶ = 1`, e.g. modificator 100 with the
hardest-difficulty option), while the Community Update's `_Global.ini` has 50 and pays half.
Razdor first played with 100 whatever the install said (the player's choice of 2026-09-29);
since 2026-10-04 it **reads the install's value, as the original does** (the diff test showed
the original paying 16 where Razdor paid 33, tools/difftest FINDINGS §6). Test:
`real_fort_battle_pays_the_installs_xp_rate`.
**data**

**AI armies** gain XP only in battles between AI armies (4a4c68). After the simulated
battle (4a0710; side A takes only the paid units), each side whose **end strength** is
above 0 gets, for each survivor, `award × AIExpiriencePercent div 100` (4a4a7c), with no
difficulty factor, no army correction and no cap. Both sides can gain (turn limit). A
building's garrison that defends gains too. An AI army that beats the player gains
nothing. AI units keep XP and levels like the player's (49273c), and after each gain may
take the upgrade tree (§4). **code**

## 4. Promotion

**The upgrade tree as loaded** (4e0448). `NextUnit1..3` are matched by unit name and
stored as 1-based type numbers with their `NextUnitNLevel`; a missing level key reads as 0.
Then the slots are normalised (4e080b–4e0965): a single option (in slot 1 or 3) moves to
slot 2; two options (slots 1+2 or 2+3) end up in slots 1 and 3. So a lone option is in the
middle and a pair on the sides; three stay as they are. **code**

In the Community data 62 types have no option, 28 one, 8 two (Militia and Infantry among
them) and 4 three; required levels are 1, except types 59 and 98 (3) and one option of
type 83 without a level key (0). **data**

**The player** (the army screen, 4c3d38 → 4b1a04):
- Any unit **except the hero** (the first unit) can be promoted once its level `L ≥ 1`
  (Razdor level 2). `NextUnitNLevel` is **not** checked for the player (4c3d63), nor is the
  unit's HP. The hint shown for the unit follows the same test (4c39d8): at `L = 0` it says the
  unit lacks experience, otherwise it invites a choice, and a type without options gets the
  last-class hint. **code**
- Promotion is free. The unit becomes the chosen type with `L = 0`, XP 0 and last gain 0
  (4b1df0, again at 4b1f67 guarded by `L > 0`), not on the click but 100 ms before the end
  of the promotion animation, 1501 or 1746 ms long (interface.md §9.7). HP is not touched directly; the stat
  rebuild that follows (497240) rescales a wounded unit's HP to the new maximum as in §2.
  **code**
- **Worn items stay worn** and keep working: the promotion code does not touch them, and
  the stat rebuild applies any valid item id without checking the class. **code**

**The AI** (4a4a7c, after every gain it makes):
- If the type has no option, nothing happens.
- Militia (type 4, 0-based 3): `rand(3) = 0` picks slot 1, else slot 3. Infantry (type 8,
  0-based 7): `rand(3) = 0` picks slot 3, else slot 1. Every other class draws
  `rand(3) + 1` until it hits a filled slot. **code**
- The roll is made after every call, even when the gain was 0 and even when the level
  turns out too low, so it always advances the shared seed. The pick is taken when its
  `NextUnitNLevel ≤ L`: new type, `L = 0`, XP 0 (the last gain is kept), and its four worn
  items go into the battle's loot pool (0xC09254), which the side with the larger end
  strength takes back at the end of the AI battle (4a473c; on a tie the second army). Only
  one promotion per gain. **code**
- After the normalisation Militia's and Infantry's two options are in slots 1 and 3, so
  their fixed picks always find one. A type with a single option in slot 2 and one of
  these two type numbers would read an empty slot; the data has none. **code** / **data**

## 5. Other XP sources

- **Events** (4ab51b): the event's XP (signed 16-bit field +0x53) goes to the **hero
  only** (the first unit of the player's army, alive or not), as it is: no modifier, no
  cap; a negative value adds nothing but still sets the last gain to 0. A Community
  extended event pays this vanilla XP only when its opcode is 0: opcodes 6..22 branch away
  at c27862, every other non-zero opcode (1..5, above 22) at c2669e. **code**
- **Community opcode 13** (c27f12–c28043): `m` (word +0x59) XP to unit `g` (word +0x55,
  0-based: 0 is the first unit) of holder `h` (word +0x53: an army id when ≥ 0, a
  building's garrison when < 0), through the gain rule of §2; AI units bank it like the
  player's; no promotion check. With `g = −1` and an army, the loop runs 12 times, empty
  records included. With `g = −1` and a garrison, the holder is read as a signed byte and
  the loop counter is a byte that only stops when it wraps to 0, so it runs 256 times, far
  past the garrison's 12 records (a memory overrun). In both `g = −1` loops the record
  index is read as a 32-bit value whose low byte is the counter; the next two bytes are
  loop counters of the Community extended condition with opcode 14 (c28070–c281ac), which
  leaves them non-zero once it has been tested with a non-zero field +0x5d (the second byte
  at 4, the third at an army's unit count for its all-units form; c2859e later clears the
  second byte). After that, even the army form adds the
  XP to memory far outside the army and its own units get nothing. Do not reproduce either
  bug. **code**
- **The level condition** of events compares the hero's 0-based level (4a80b4, compare
  4a7b40): a positive value `c` means `L ≥ c`, a negative one `L ≤ −c`, 0 always passes. So
  "level 2" means Razdor's level 3. **code**
- **AI hiring** (4a548c, when an AI army hires in a building, or buys garrison units):
  with army byte +0x37F6 (map byte 14, the flag that gives hires XP in line with the
  player's army) the army first
  computes `P = Σ(player's unit records, the dead included: mode-0 strength +0x1AA + XP
  towards the next level) div (player's unit count + 2)`; otherwise `P = 0`. For every
  unit it hires: `X = P − (the new unit's mode-0 strength div 2)` when `P ≥ 1`, else 0;
  plus army +0x37F2 (map byte 19, bonus XP for hired units). If `X > 0` the unit gets
  `rand(X) + X div 2` (so `X div 2 … X div 2 + X − 1`), fed level by level (4a4c04): while
  the rest reaches the need, one level and a promotion try (4a4a7c with 100%); the
  remainder becomes its XP. The player's hires get no XP. **code**
- **Hero preset.** The preset has **no starting experience**: offset 8 is the starting
  **gold** (read as a signed 16-bit value into the gold, 4b5831 → 4ab150) and offset 12 the
  starting **mana** (4b5842 → 0x68E4F4). The shipped maps agree: offset 8 is the same for
  all three classes, offset 12 is largest for the archmage. The hero starts at level 0 with
  0 XP. **code** / **data**
- **Campaign carry-over** (4b5b64): the hero's unit record always carries over; with header
  byte [3] off his XP and level are set to 0 (4b5d6b; the last gain is not cleared), with it
  on the spell book is restored as well. With byte [6] the army carries over with its
  levels and XP. The rest is in economy.md. **code**
- **Surrender** gives mana, not XP (§3).

## 6. The hero

The hero levels exactly like a unit, with his class's `StartExpirience` and
`LevelMultipler` (100/150, 90/160, 80/170) and `d-*` gains; he shares the battle pool like
any survivor. He cannot be promoted. His level feeds nothing else that we found: the only
reader of the hero's level field is the event level condition (4a80cc). A Community load
guard (c26c4c) crashes the game at the fourth unit type whose `LevelMultipler` is 200 or
160; the shipped data has one (the archmage). **code**

## Razdor now → original

Razdor's code (`src/rules/experience.rs`, `battle.rs` `xp_awards`/`player_xp`/`ai_xp`,
`units.rs` `gain_xp`/`promote`, `ai.rs` `hire_xp`/`ai_promote`, `game.rs`
`start_battle`/`settle_battle`, `script.rs` `give_unit_xp`; the UI in `ui/battle_view.rs`
and `ui/items_view.rs`) as read on 2026-10-01, rows brought in line on 2026-10-02.

| Topic | Razdor now | Original (this exe) | Gap |
|---|---|---|---|
| Level numbering | 1 as hired; event level condition uses `level − 1` | 0 as hired, shown +1 | none |
| XP needed | partial sums in 64-bit floats of the raw `StartExpirience` and `LevelMultipler` (`experience::xp_to_next`) | 80-bit sums, raw values | Matches (a need of 0 or less stops the gain instead of hanging) |
| Gain loop | the original's loop, comparing the previous need; a gain below 1 adds nothing (`experience::add_xp`) | the same, comparing the previous need | Matches |
| Last gain | the battle screen shows the award | +8 set by every gain, zeroed for the army before battle XP | display only |
| Tactical cost | as §1, cached per type and level; `CostMultipler` as it is, then 0 → 1 and a negative `x` → its absolute value + 1 | 0 stays 0 (then 1) | Matches |
| Army strength (event condition) | tactical cost of every record, the dead included, with the defence of the building the hero stands in (`script.rs` `army_strength`) | sum of +0x1AE over every record, the dead included, with the army's building defence | Matches |
| Side strength | rows, back-row doubling, `f`, lone ÷5, HP-scaled; a surrendered side has no living units, so 0 | the same; a surrendered side is 0 | Matches |
| Pre-simulation | a full AI-vs-AI run on a copy at `begin()` (no Splash), its HP loss kept as the prediction | a full AI-vs-AI run, its HP loss kept as the prediction | none |
| Pool | the three branches with the prediction and the largest finished turn's loss (`battle_pool`) | three branches with prediction and largest turn loss (§3) | none |
| Share | `t·((4−row) + row·useful/(taken+left))`, below 0.5 → 1 | the same | none |
| Surrendering side | no award (`xp_awards` takes the living only) | shares computed, then the units are removed: nothing | Matches |
| Player modifier | × F × correction / 10⁶, low 32 bits, abs, cap 5256; correction 0 used as it is | the same, 0 used as it is | Matches |
| Turn limit | counts as victory, XP paid | the same | none |
| AI XP | AI-vs-AI only, ×AI% div 100, when end strength > 0, survivors only (the dead draw no promotion roll); the player's units left in a building gain and roll with its garrison; "beaten" and the loot pool by end strength (`ai_battle`) | the same (the player's units are in the garrison record) | Matches |
| Level-up HP | proportional rescale with the fractional carry after the gain (`Unit::gain_xp`, `troop_gain_xp`) | proportional rescale with a fractional carry at the next stat rebuild | Matches |
| Percent stats | gap to 100 closes by `d`% per level in 32-bit floats, max 99 | the same | none |
| Player promotion | any unit with a level, not the hero; level 1, XP 0; items stay worn; HP rescaled | the same rules; items stay worn; HP rescaled | Matches |
| AI promotion | Militia/Infantry fixed picks, others random filled slot; `NextUnitNLevel ≤ L`, from the game LCG (`rules/rng.rs`); worn items to the loot pool | the same with the game LCG; worn items to the loot pool | none |
| Upgrade slots | normalised at load (`dt::data::normalise_upgrade_slots`) | normalised (lone → 2, pair → 1+3) | Matches |
| Event XP | the hero, as it is; a new level shows on the map | the same | none |
| Opcode 13 | XP banked, per unit or all units | the same; both all-units forms can write outside the army (§5) | Razdor is safe (keep) |
| AI hires | bytes 14 and 19 as §5 with the mode-0 level value (no Community floor, `experience::level_value`), `Random(X) + X div 2` from the game LCG, garrison purchases too | the same formula with the LCG; also garrison purchases | Matches |
| Preset offset 8 / 12 | gold / mana; no starting XP | gold / mana | none |
| Carry-over [3] | level and XP kept, else level 1 | the same | none |

## Unknowns

- Whether the DirectX layer changes the FPU control word (engine.md §1). It decides the
  precision of every float formula here (see §0). And if the exception masks stay
  Delphi's defaults, a side whose start strength rounds to 0 (all its units nearly dead)
  makes the unused ratio a division by zero that raises an exception (48bb85).
- The prediction: Razdor runs the AI-vs-AI pre-run on a copy of the battle; its result
  matches the original's only as far as the battle AI and the shared seed do (battle.md).
- Whether any shipped map gives an army an XP correction of 0 (the original pays nothing
  for it; so does Razdor now).
- The pool when a side had no HP at the start (cannot happen with `lost > 0`; with
  `lost = 0` it is not used).
