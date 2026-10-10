# Discord Times Community Update: what the patch code changes

This file covers the Community Update (Unstable) `DiscordTimes.exe` (Delphi, image base 0x400000;
the Evolution install ships a byte-identical exe). The Community Update is not a rebuilt game: it is
the original program with two extra sections, `.mod` (0xc25000–0xc35000, hand-written patch code)
and `.bonus` (0xc35000, patch variables and tables), plus about 126 five-byte jumps written into the
original code that lead into `.mod`. This file lists every such hook, says what each patch does,
and gives the rules exactly enough to reimplement them, quirks included.

The rules are in our own words. Addresses are virtual addresses in that build, given as evidence
only. [battle.md](battle.md) section 7 is the short overview; this file is the full detail and
supersedes it where they differ (the differences are listed in section 15).

Confidence tags:
- **code**: confirmed by reading the code.
- **data**: consistent with the data files, the changelog or the help, but not traced in code.
- **unknown**: not determined.

Terms used below:
- A **unit record** is a unit's battle record. Its fields are named in words (attack modifier,
  current initiative, …).
- **AB, AS, MP, DB, DS** are AttackBlow, AttackShot, MagicPower, DefenceBlow and DefenceShot as
  stored in the battle record. They are the battle copies: Bastion, Assault, ArmorBreaker and
  EternalGift change them for the rest of the battle.
- The **attack, defence and initiative modifiers** are the per-turn fields that every turn start
  resets to 0 (blessings and curses write them).
- **Current initiative** is a separate per-turn field. It is set to the base initiative at each turn
  start, and Artillery and FirstShot add to it.
- "Integer division" truncates toward zero unless said otherwise.

## 0. How the patch works

- **Jump hooks.** Each hook replaces one instruction of the original with a jump into `.mod`. The
  fragment then runs inside the original function's stack frame, re-does the overwritten
  instruction and jumps back, or jumps on to the next fragment. So the Community rules for one
  event form a **chain**: a fixed order of fragments, each testing one bonus. The order matters and
  is given below. **code** (126 sites found by scanning the whole image for relative jumps and
  calls into `.mod`; the list is in section 1.)
- **Operand patches.** Some original instructions were also rewritten to point into `.bonus`: the
  option sliders' values, a terrain-name table and two other tables. Their old values cannot be known
  without a vanilla exe. **code** for the sites (section 12), **unknown** for the purpose of most.
- **Global state.** All patch variables are single globals in `.mod`/`.bonus`: the splash state,
  the Hunger counter, the Caster and AddPayment flags, the bleed and cripple tables. They are not per
  battle object. The AI-vs-AI simulations use other battle objects but the same globals, so several
  rules check the **interactive-battle flag** (4ed424). Only some do, and the ones that don't check it
  are marked.
- **One bonus per unit.** A unit carries one bonus number. Each worn item with a bonus overwrites it,
  and the last slot wins (4919f0–491a4f; battle.md §7). **code**
- **Shipped data uses none of the new bonuses.** Neither install's units or artefacts files give any
  unit or item a bonus numbered 22 or higher. No unit has ManaDrain, MinMagicPower, Evasion or the
  Elemental nature. The new rules come into play only with modded ini files. The Community install
  does use FastDead (1 unit) and Counterblow (1); Evolution uses Counterblow (9), FastDead (4),
  FlankStrike (2) and AddPayment (2). **data**

## 1. Hook sites in the original code

| Original site | Host function | Patch | Section |
|---|---|---|---|
| 4843e0, 4843e5, 4843ed | turn start 4840ec | Hunger, Berserk, then the Fortify… chain | 3 |
| 484466 | turn start | Replaces the whole magic drain block | 10 |
| 4844aa, 484511, 4845db | vanilla drain | Concentration inside the replaced block: **dead**, since 484466 always jumps past it | 10 |
| 485554 | legal cells 484c4c | Flying | 7 |
| 485669 | legal cells | NoHeal friendly-target filter | 7 |
| 48595b, 4859d7 | physical damage 485908 | Splash attack scaling (melee, shot) | 6 |
| 485992, 4859f4 | physical damage | PoisonArmorIgnore pierces (melee, shot) | 6 |
| 485a78 | physical damage | Assault ×2/3 test | 6 |
| 485b26 | physical damage | Evasion, the last step | 6 |
| 485b73, 485b7f | magic power 485b3c | Splash power scaling; Potent | 6 |
| 486bb2, 487e93, 489b67 | battle AI | Manevres-0 targets; wide-row scan start | 11 |
| 489f62, 48a0ae | unit removal 489f50 | Bleed table shift; Hunger counter | 9 |
| 48a677 | action start 48a5c4 | Bleed loss | 4 |
| 48a9f1, 48aa6d, 48aa95 | blessing by school | EternalGift | 8 |
| 48aaf1 | heal/bless applied | Splash (heal/bless) | 6 |
| 48aef1, 48af64, 48b0ac | curse by school | EternalGift | 8 |
| 48b1e6 | after a hostile spell | Magic chain | 5 |
| 48b214, 48b2f6 | start of shot / melee case | PreventiveStrike | 7 |
| 48b2c8, 48b3c7 | after a shot / melee hit | Shot chain, melee chain | 5 |
| 48b4ac | after a melee hit | Hunger kill heal | 5 |
| 48b5a5 | end of a hit action | Splash neighbour loop | 6 |
| 48f33f, 48f371, 499392 | units / artefacts loader | New bonus names | 2 |
| 48f580, 48f5ce | units loader | ManaDrain, MinMagicPower, Evasion | 2, 10 |
| 4e45ba | `_Global.ini` loader | MinSpellLife / Elemental / Death | 10 |
| 48f3d8, 4990b0, 4995c2, 4e4673, 4e49d9 | loaders | Tripwires | 12 |
| 4a0025, 4a0069, 4a171c, 4a177f, 4a181d, 4a1850, 4a1871, 4a43e7, 4a44a4, 4a4544, 4a4656, 494e38, 494ea5, 495708, 495718, 4957e3, 4ab1b8, 4b0fd9, 4b1399, 4b13d2 | strength, wages, hiring, resurrection | Economy patches | 12 |
| 49bb8f, 49bbe0, 49be33, 4af572, 4c2d72, 4c2e6e, 4c2efb, 4cca6a (calls) | spell cost and time | Caster | 12 |
| 4c566f, 4c569f | victory screen 4c50ec | Experience product and cap | 12 |
| 4a809d, 4a80aa, 4a80db, 4a8150, 4a822d, 4a8e60, 4a9b6f, 4a9bfd, 4a9d83, 4ab29d, 4ab350, 4ab39d, 4ab50f, 4ab516, 471b61 | event engine | Event opcodes and their RNG | 12 |
| 4cd00f, 4cd2f6, 4ac422, 4ae428, 4ae511 | key handlers, wait loop | F1–F5 | 12 |
| 49e282 … 49e406 (11 sites), 4b8c48, 4bf4d3, 4d02c5, 4d3390, 4d3397, 4dc113, 4afd45, 4afd4e, 4b006a | map markers, options | Marker colours, animation speed | 12 |
| 4924c1, 4e44fc, 4e3027, 4dbc1f, 48d471, 48d54e | info card, texts, sounds, frames | Bleed and Evasion lines, extra images | 12 |
| 48ef24, 4c9b56 | map objects, map renderer | Owner byte, pointer guard | 12 |

## 2. Bonus numbers and data keys

**Numbers** (1-based, stored in the unit type and copied to the battle record). The vanilla game
already numbers 1–21: 1 SpearDefense … 18 Dead, 19 FastDead, 20 Counterblow, 21 FlankStrike. The
Community adds 22 Hunger, 23 Berserk, 24 Exhaustion, 25 Drying, 26 CtrPoison, 27 Suicide, 28 Caster,
29 Splash, 30 Fortify, 31 Dominate, 32 PoisonS, 33 Concentration, 34 Potent, 35 Stun, 36 FirstShot,
37 Bastion, 38 Flying, 39 Bleed, 40 PreventiveStrike, 41 Flock, 42 ArmorBreaker, 43 NoHeal,
44 FasterAttack, 45 PoisonArmorIgnore, 46 HoldLine, 47 Neutralize, 48 KillingStrike, 49 BloodThrist,
50 Assault, 51 EternalGift, 52 FateGift. **code** (c255a1, c25af7, c27d94, c28e85; artefacts c26569,
c27dfd, c29001)

**Counterblow in the units file.** The patch's jump at 48f33f overwrites the vanilla compare that
writes 19, and its fragment (c25126) compares the `Bonus` value with its own FastDead name (→ 19)
and with the vanilla Counterblow name (→ 20), then rejoins at the FlankStrike compare (→ 21). In the
patched exe no other code refers to the vanilla Counterblow name, and none to the vanilla FastDead
name either. Which name each of the two overwritten vanilla compares used (the one writing 19 and
the one writing 22, at 48f379) cannot be read any more. The vanilla name pool holds FastDead and
Counterblow side by side between Dead and FlankStrike, so vanilla may well have compared Counterblow
in one of those slots. **code** for the patch's two compares; **unknown** whether vanilla units
could have Counterblow (an earlier reading called this a fix). The vanilla Counterblow rule itself
(battle.md §8) is unchanged.

**Keys read only by the patch:**

| File | Key | Read at | Meaning |
|---|---|---|---|
| units file | `ManaDrain` | c283dc | Per-type drain per turn (section 10) |
| units file | `MinMagicPower` | c284e6 | Per-type floor (section 10) |
| units file | `Evasion` | c2a7b5 | Per-type % taken off physical damage (section 6) |
| `_Global.ini` | `MinSpellLife`, `MinSpellElemental`, `MinSpellDeath` | c28490 | Default floors (section 10) |
| options file (`Rus_DiscordTimes.ini`) | `AnimationSpeed`, 11 `Color…` keys | c2945c, c294b9, c294ed, c27604 | Section 12 |
| texts | `SBleed`, `SEvasion` | c2a45d, c2a67a | Info-card labels |
| sounds | `Battle-Parry` | c2a85c | Loaded but never played; `_Sounds.ini` has no entry |
| graphics | `Win-black`, `Win-yellow` window images | c281c7 | Loaded; the selector is dead (section 12) |

`CrazyAI` is a vanilla `_Global.ini` key that the hook only passes on to the vanilla loader; it is
never read in battle (battle.md §4).

## 3. Turn start

**Where it runs** (4840ec). **code** Side 1 is processed, then side 2. Within a side the units go
in list order. Each unit is processed **completely** before the next one:
1. Its modifiers are reset, actions are set to Manevres and current initiative to the base. The
   reserve-move flag is set here too (48430f), before the turn-1 extras.
2. Vanilla turn-1 extras: +1 action for HorseAtack, OldVampirsGist and FastDead; Artillery adds
   +30 current initiative, and +30 more with building defence ≥ 10.
3. The Community chain below.
4. The blessed and cursed flags are cleared (4843f4).
5. From turn 2: the magic drain (section 10), then regeneration or poison, after which a unit at
   HP ≤ 0 is removed at once.

So unit *k*'s Community bonuses are computed **after** the regeneration ticks (and poison deaths)
of the units before it, but **before** its own tick.

**Chain order** (each step tests the unit's bonus; only one can match): Hunger → Berserk →
Fortify → Garrison → FirstShot (turn 1 only) → Bastion → FasterAttack → Assault → Flock →
cripple-mark reset → bleed reset. **code** (c25370 → c256c8 → c25a1d → c2651a → c26e8c/c28935 →
c26ebb → c29c4e → c29c97/c2a3d9/c29cc5/c2a425/c29cd7 → c29d08 → c29fbb → c2a5ca)

| Bonus | Rule at turn start | Evidence |
|---|---|---|
| Hunger | If the shared "living units" counter differs from the shared "last seen" value, set last seen to the counter. Then, unless it is turn 1, heal to max HP. Section 9 has the counter. Last seen is one global, so after a change only the first Hunger unit in processing order heals. | c25370 |
| Berserk | **Set** the attack modifier to `((maxHP − HP) × 75 × AB / maxHP) / 100`, two integer divisions in that order. It uses HP before this unit's own regeneration tick. | c256c8 |
| Fortify | From turn 2: defence modifier += `max(1, DB × 25 / 100) × min(turn − 1, 5)`. The defence modifier counts against both melee and shots. | c25a1d |
| Garrison (fix) | If the building defence is exactly 10: attack modifier += AS. The attack modifier also raises melee. | c2651a |
| FirstShot | Turn 1: current initiative +30, and +30 more if building defence ≥ 10. This is the vanilla Artillery rule. A +60 variant at c26e9a is unreachable. | c28935 |
| Bastion | AB, AS, DB and DS each doubled, at **every** turn start, compounding (×2, ×4, ×8 …). There is no building test. AB 65 overflows a 32-bit value by turn 25. | c26ebb |
| FasterAttack | Turns 1 and 2: actions left +1. | c29c4e |
| Assault | Turn 1 only, if the **enemy side's first record** has building defence ≥ 10: AB, AS, DB, DS doubled (once, for the battle). | c29c97, c2a3d9, c29cc5, c29cd7 |
| Flock | Compare the first dwords of the two side blocks at 669df8 (side 1) and 66a64c (side 2). These are the side blocks the interactive battle is built from (49855c, 48b75c), and the battle copies its two sides back into them after every action and at the end (48bb10). The first dword is the side's living-unit count. So Flock compares the living counts as of the last action of the interactive battle; at turn 1, the starting counts. Equal: nothing. Otherwise, a Flock unit on the larger side gets attack modifier += `S × 25 / 100`, and one on the smaller side gets −=, where S = AB, or AS when AB is 0. The divisions are unsigned, so a negative S (only after EternalGift curses) gives a huge wrong step. | c29d08, 48bb10, 4c4f8c, 4c57bc |
| (reset) | On turn 1: all cripple marks and all bleed values are cleared (once per unit, harmless). | c29fbb, c2a5ca |

Notes:
- **Flock's counts lag behind.** The side blocks are only refreshed after an action, so deaths
  earlier in the same turn start (regen or poison ticks of units processed before) are not seen.
  They are fixed globals that only the interactive battle refreshes: a simulation or another battle
  object sees the interactive battle's last counts. **code** (48bb10 copies B+0x23 and B+0x874, the
  two living counts, as the first dwords). Whether the turn-1 counts include units left out of the
  battle (unpaid) depends on 49855c, which was not traced. **unknown**
- **Assault's building test** looks at record 1 of the other side, not at each unit. The building
  defence is the same for the whole defending side, so in practice this means "the enemy fights from
  a building with defence ≥ 10". **code** for the field; the equivalence is **data**.

## 4. Action start: Bleed

Every action spends one action first (vanilla, 48a64c). Then, for **every** acting unit (moves and
passes included):
- `loss = (AB + AS + MP) × bleed / 100`, using the unit's own current battle values. Bleed is the
  unit's bleed value (0, or 75 once bled; section 5).
- HP −= loss. If HP ≤ 0, its bleed value is cleared, the unit is removed, and the action is
  cancelled. **code** (c2a53c, c2a8a8, c2a5a1)
- The removal uses the plain removal routine: no on-kill effects (no DeathCurse or Ghost
  reaction, nothing credited to the bleeder). It counts for Hunger's counter (section 9). **code**
- A unit with several actions bleeds once per action. The loss does not depend on its HP.
- With a negative sum, the division is done unsigned: the loss comes out as a huge positive number
  (about 43 million) and the unit dies. That only happens if EternalGift curses push a base value
  below 0. **code**

## 5. The three hit chains

Each hit kind runs a fixed chain after the damage or spell has been applied. "d" is the value the
chain tests:
- melee and shots: the computed damage before capping at the target's HP;
- hostile magic: the spell power `P` from 485b3c, **for curses too** (a curse does no damage, yet
  the "d > 1" effects fire).

**Melee** (cell codes 4 and 5; host 48b2f6–48b5a5). **code**
1. PreventiveStrike of the target (section 7).
2. The damage is computed and applied. Vanilla Poison: a Poison attacker with d > 1 sets the
   target's regen to −20.
3. Berserk recompute for the **target** (the formula of section 3; it overwrites a blessing's
   attack modifier).
4. CtrPoison: if the **target** has it, the attacker's regen −= 20. This stacks per hit, with no
   damage or survival test, and can bring a positive regen down.
5. Suicide of the attacker (below). PoisonS: d > 1 → target regen = −25.
6. Splash bookkeeping (interactive battles only, section 6).
7. Stun (below).
8. The on-hit block (below).
9. NoHeal.
10. BloodThrist.
11. Vanilla vampirism: the attacker heals `Vampirizm × d / 100`, capped at max HP, unless the target
    is Undead or Elemental (natures above 6 are skipped too; 48b3db).
12. The Hunger kill heal (below).
13. Vanilla Counterblow: a surviving Counterblow target strikes the attacker with melee damage. A
    killed attacker is removed directly.
14. If the target is at HP ≤ 0, the vanilla on-kill routine runs.
15. Splash neighbour loop (section 6).

**Shot** (code 7; host 48b214–48b2f1). **code**
1. PreventiveStrike.
2. The damage and vanilla Poison.
3. Berserk (target).
4. Suicide. PoisonS when d ≠ 1, which, since damage is at least 1, is the same as d > 1.
5. Splash bookkeeping, Stun, on-hit block, NoHeal, BloodThrist.
6. The kill check. There is no vampirism and no counter.

**Hostile magic** (code 8; host 48ac7e–48b20f). **code** The vanilla code picks a strike if the target has any
negative modifier this turn, else a curse (battle.md §3). The strike also gets Death-school vampirism,
and curses get the Undead-caster drain. Then the chain:
1. Berserk (target).
2. Drying: the target loses `floor(8 × maxHP / 100)`, or 1 if that is 0, ignoring protection. This
   comes after Berserk, so Berserk does not see it.
3. Mage Poison: a Poison caster with **power after protection** > 15 sets the target's regen to −20.
   The power after protection is a separate formula (below).
4. Suicide.
5. Exhaustion: the target's three protections −10 each, then each clamped at ≥ 0.
6. PoisonS: the same > 15 test, regen −25.
7. Stun.
8. Splash bookkeeping, on-hit block, NoHeal, BloodThrist.
9. The kill check.

**Heal and bless** (code 9) have **no** Community on-hit effects; only the splash bookkeeping runs.
**code**

**Power after protection** (c26c9f), used only for the two mage-poison thresholds. **code**
- `MP × (99 − protection) / 100` for Life and Death casters (the target's ProtectLife or
  ProtectDeath), and `MP × (99 − ProtectElemental) / 114` for Elemental casters.
- MP is the caster's raw battle MP. Splash scaling and Potent are ignored: protection always counts.
- A protection above 99 (or a negative MP) makes the Life and Elemental results wrong: the division
  is unsigned, so the result is a huge positive number and the poison fires. Death is computed
  correctly (signed). School 0 counts as Life here.

**Suicide** (c2633c melee, c262a1 shot, c261c1 magic). After any of its hostile hits the attacker
gets:
- HP 0, regen −99;
- row 0 and column 0;
- Manevres 0 and actions left 0.

It is **not removed**. Its grid cell still holds it, and its side's living count is unchanged.
Then:
- In melee, vampirism right after can give it HP back.
- A Counterblow can hit it, which removes it, but clears the wrong grid cell.
- Otherwise the next turn start's regeneration tick (regen −99) takes it to HP ≤ 0 and removes it.
- Until then it never acts (no actions). The AI's melee scoring treats it specially (section 11).

**code** for the fields. How the screen shows such a unit is **unknown**.

**Stun** (c27e5a melee, c2899a shot, c26e1f magic). Every hit or spell, with no damage test: target
initiative modifier −= `current initiative × 30 / 100`. Current initiative is the turn's base value
(with the turn-1 Artillery/FirstShot bonus), **not** reduced by earlier Stuns. So each Stun hit
takes the same amount, and the turn order sees the change at once. It is gone at the next turn
start. **code**

**On-hit block** (c291d1 melee, c296bc/c2973c shot, c29984 magic). It runs in this order. The
"attacker" tests are on the hitting unit, the "target" ones on the unit hit. **code**
1. PoisonArmorIgnore (attacker), d > 1: target regen = min(regen, −10).
2. Bleed (attacker), d > 1: target bleed = max(bleed, 75). For shots only if the target is still
   alive (c2a8e6). Bleed never stacks above 75.
3. ArmorBreaker (attacker), d > 1: DS −= DS×25/100, then DB −= DB×25/100. That is
   `x − floor(x/4)`, the **upper** rounding of 75% (5 → 4, 3 → 3, 1 → 1). It is cumulative for the
   battle.
4. KillingStrike (attacker), d > 1: if `floor(maxHP × 25 / 100) ≥ HP`, HP = 0.
5. FateGift (target), if HP ≤ 0 now:
   - actions left = Manevres;
   - all three protections +20, regen +20;
   - max HP += floor(maxHP × 20 / 100), then HP = max HP;
   - the bonus is erased;
   - initiative modifier +5.
   This runs **after** KillingStrike, so it saves from a finishing strike. It runs **before**
   Neutralize, so a Neutralize attacker cannot stop it. Deaths outside these three chains are never
   saved: poison and regen ticks, bleeding, counters and preventive strikes.
6. Neutralize (attacker): target bonus = 0, every hit, no damage test.
7. NoHeal (attacker), every hit, no damage test: the target's slot is marked crippled
   (section 7), and a positive regen is set to 0.
8. BloodThrist (attacker): if the target is at HP ≤ 0 now, attacker actions left +1. This counts
   per hit, splash follow-ups included.

**Magic Bleed fault.** In the magic copy of step 2, the branch for "already bleeding (≥ 75)" jumps
into the middle of an instruction. The bytes there write to a tiny address (between 13 and 36,
built from the target's side and slot), which is an access violation. So a Bleed caster whose
spell has power > 1 faults when it hits a target that already bleeds; the shot copy is correct.
**code** for the jump; what the game then does (crash, crash log, or an exception handler) is
**unknown**.

**Hunger kill heal** (c252e9, after vampirism). It is meant as "melee kill → heal the attacker to
max HP". It only runs for melee: shot and spell kills never heal. The HP test has a slip: it builds
the target's record address from the wrong register. It drops the target's side and adds an unrelated
value: usually the attacker's slot × the record size, after the vanilla vampirism step. So the code
tests a dword at an essentially arbitrary address (below or inside the battle object), and heals if
that value is ≤ 0. **code** for the slip, **unknown** for the resulting behaviour.

## 6. Damage-formula hooks and Splash

**Physical damage (485908)**, with the Community steps in place (base formula in battle.md §0):
1. Attack = AB (melee, long strike) or AS (shot) + attack modifier. Then Splash scaling: melee
   ×40% if the splash state is "melee", else ×80% for a Splash attacker; shots ×40% if the splash
   state is any non-zero value, else ×80% for a Splash attacker. Both use multiply-high with a
   magic constant (below), unsigned. **code** (c270ae, c2731a, c27337)
2. Defence, SpearDefense on turn 1, then piercing. Melee: ArmorIgnore, **PoisonArmorIgnore**,
   VampirsGist, OldVampirsGist set the unit defence to 0. Shots: ArmorIgnore,
   **PoisonArmorIgnore**, Artillery. Row2Def and building defence are still added afterwards.
   **code** (c2a27c, c2a3bf)
3. Subtractive damage, min 1.
4. ×2/3 (integer) for Evasive, VampirsGist, OldVampirsGist **or Assault**. The Assault test is
   below. **code** (c2a403)
5. The vanilla steps: Garrison ×2/3, Dead/FastDead ×3/10 vs shots, Knight, Unvulnerabe/Ghost = 1,
   GodAnger +10, GodStrike +20, 0 → 1.
6. **Evasion** (last): if the target **type's** Evasion E ≠ 0, `dmg × (100 − E) / 100`, and a result
   of 0 becomes 1. E comes from the type, not from items. The per-type table holds one byte per
   type, so E is the ini value modulo 256. Values above 100 are not clamped and give garbage
   (unsigned division). **code** (c2a802, loader c2a7b5)

This routine serves every physical hit: melee, long strike, shot, Counterblow, PreventiveStrike, and
the AI's damage estimates. So all of the above, Splash scaling included, also applies to counters,
preventive strikes and AI scoring. **code**

**The Assault test** reads a misaligned dword from the **attacker's** record. Its top byte is the
attacker's building defence; its lower three bytes are the upper three bytes of the attacker's
initiative modifier. The test is "≥ 16". The ×2/3 therefore applies when either:
- the attacker's building defence is between 1 and 127, whatever its initiative; or
- the attacker has no building defence and its initiative modifier is negative (or ≥ 4096).

An attacker in the open with a modifier of 0 to 4095 deals full damage. **code**

**Magic power (485b3c).** **code**
- The caster's MP is first scaled for Splash: ×40% if the splash state is non-zero, else ×80% for a
  Splash caster. This applies to every kind: heal, bless, curse, strike (c27374).
- Then hostile kinds lose protection (`round(P × (1 − prot/100))`, Delphi rounding), and strikes get
  the nature multipliers. A **Potent** caster skips both (c26e78).
- Then a strike gets +10 (GodAnger) or +20 (GodStrike) if the caster's MP > 0. This also applies to
  Potent casters.

**Splash rounding.** Both factors use 32-bit multiply-high constants. **code**
- 80%: exactly `floor(0.8 × x)`.
- 40%: `floor(0.4 × x)`, **except** that a multiple of 5 gives one less: 5 → 1, 10 → 3, 25 → 9,
  100 → 39.
- The multiply is unsigned, so a negative attack (attack modifier below −AB) wraps around. At 80%
  it gives a huge negative value (damage 1). At 40% it gives about +1.7 billion: one hit kills.
  This needs a cursed Splash unit on a follow-up hit.

**The splash state machine** (variables in `.mod`: state, saved row, column and index, follow-up
count, left/right done flags, attacker column). **code**
- **Recording** (c26f2d melee, c270e5 shot, c2718d hostile magic, c27274 heal/bless). For melee,
  shots and hostile magic it is only done in the interactive battle (4ed424 = 1; the gates are
  c288ea, c28903, c2891c). The heal/bless recording has **no** gate: 48aaf1 goes through c273ab
  straight to c27274. The scaling hooks above have no gate either.
- The state values are 1 for melee, 2 for shot, 3 for hostile magic and 4 for heal or bless.
- **First hit.** When a Splash unit's hit runs and the state is not yet that kind's value, the hit
  is the primary one. The patch saves the target's row, column and index, and adds the kind's value
  to the state (which is 0 at that point, so the state becomes that value). The
  primary hit's damage was computed before that, so it used 80%.
- **Follow-up hits.** When the state already equals the kind's value, the hit is a follow-up. The
  count goes up, and at 2 the state, count and both flags are reset.
- **Neighbour loop** (c26fd2, at the end of each hit action). While the state is non-zero, the
  target side's records 1 to 12 are scanned from 1 each time. A candidate must stand in the saved
  row. For melee it must also be within 1 column of the attacker. For each record in index order the
  scan tests "left neighbour (saved column − 1), not taken yet", then "right neighbour (+ 1), not
  taken yet", and takes the first record that passes. So the two neighbours are hit in **slot index
  order**, not left before right; each side is taken once. (c27017)
- **Re-running the action.** The chosen neighbour becomes the target, and the code jumps back into
  the action at the start of that kind's case:
  - melee: the same cell code as the clicked cell, so a long strike stays a long strike;
  - shot;
  - hostile magic: curse-or-strike is decided again for this target;
  - heal-or-bless: also decided again.

  No action is spent.
- **Ending.** After the second follow-up, or when no neighbour is left, every splash variable is
  reset.
- **What a follow-up runs.** Each follow-up runs the **whole** hit path: the neighbour's
  PreventiveStrike, the full chain (Stun, Bleed, BloodThrist, Suicide, CtrPoison, vampirism,
  Counterblow …) and the kill check.
- **The primary target's counter.** Its Counterblow is computed after the state was set, so it
  strikes back at **40%**.
- **Neighbours' counters and preventive strikes** against the attacker are also at 40%, while the
  state is set. The second follow-up resets the state in its own recording step, which comes after
  its damage and before its Counterblow. So the second neighbour's preventive strike is at 40%, but
  its Counterblow is at full strength.
- **Cripple marks.** A follow-up heal skips the legal-cell map, so it **can** heal or bless a
  crippled (NoHeal) neighbour.
- **Simulations.** In AI-vs-AI simulations and battles that are not interactive, melee, shots and
  hostile spells get no follow-ups, but the 80% malus still applies. A Splash unit's heal or blessing
  **does** record there and gets its follow-ups (at 40%), since that recording is not gated.
- **Scan edges.** A removed unit's vacated record is zero-filled, so the scan never hits a stale
  record. If the attacker dies during a follow-up, the loop keeps using its old slot index, which now
  holds another unit; that is untested. **code** for the zero-fill, **unknown** for the outcome.

## 7. PreventiveStrike, Flying, NoHeal targeting

**PreventiveStrike** (target bonus 40). **code**
- **Before a melee** on it (c2a181): it strikes the attacker first, a melee hit if its AB ≠ 0, else a
  shot.
- **Before a shot** on it (c28aea): it shoots first, only if its AS ≠ 0.
- **Before a spell: never.** The code for that case (c28c00) exists but nothing jumps to it.
- **Damage** is the full physical formula with the striker as attacker, so Splash scaling, Assault
  and Evasion apply. There is no limit per turn, and it also answers every splash follow-up aimed
  at it.
- **If the attacker dies,** it is removed plainly (no on-kill effects) and its action ends (the
  neighbour loop still runs).
- **No side effects.** It triggers nothing of the hit chains: no poison, Bleed or vampirism.
- It increments the striker's attack counter.

**Flying** (c29150, inside the legal-cell map after the hostile-mage marking). **code**
- A Flying unit in row 1 or row 2 marks the occupied enemy **front** cells at its column c, c−1 and
  c+1 as **melee** (code 4). The code is 7 or 8 only if AB < 0, which only an EternalGift curse
  can cause.
- The mark **overwrites** whatever was there. A Flying shooter or mage therefore cannot shoot or cast
  at those three cells; it can only melee them. Its other targets are unchanged.
- It cannot reach the enemy back row, and does nothing from the reserve.
- In practice it adds "melee from row 2 on the three facing front cells".

**NoHeal targeting** (c2967a, friendly-target scan of rows 1–2). **code**
- A friendly cell whose unit slot is marked crippled gets no code at all: it cannot be healed or
  blessed by a click or by the AI.
- Not filtered: a caster in the reserve tending reserve units (4857fb), and splash follow-ups.
  Vampirism and Hunger heals still work.
- The marks are kept **per side and slot index** and are not moved when a death shifts the records.
  When a unit before a crippled one dies, the mark stays on the slot and so passes to the unit that
  moves into that slot. Marks are cleared on turn 1 of each battle.

## 8. EternalGift

The caster's blessings and curses change the target's **battle base** values instead of the per-turn
modifiers, so they last the battle and stack per cast. **code** (bless c2a019/c2a0ae/c2a0f8, curse
c29ea3/c29e59/c29f2f)
- **"Attack"** below means AB, or AS when AB is 0.
- **A** is the vanilla first amount, and the divisors are the vanilla spell constants (4ed398–4ed3b0
  from `_Global.ini`).

| School | Blessing | Curse |
|---|---|---|
| Life | **Both defences −A** (A = 3P/(2·4ed39c) + 1; a bug: the blessing lowers them); attack + 3P/(2·4ed3a0) | Both defences −A (A = P/4ed3a8 + 1); attack − P/4ed3b0 |
| Elemental | Actions left change as vanilla (this turn only); **base initiative** + (P/4ed398 + 1) | Actions as vanilla (not below 0); base initiative − (P/4ed398 + 1) |
| Death | Attack + (P/4ed39c + 1); both defences + P/4ed3a0 | Attack − (P/4ed3a4 + 1); both defences − P/4ed3ac |

The blessed and cursed flags are still set, so the vanilla once-per-turn limits hold. The
Undead-caster drain still follows Elemental and Death curses. **code**

## 9. Removal, kills and the counters that persist

- **Hunger's counter** (c252bd). Each time any unit is removed, after the vanilla count decrement,
  the patch stores the sum of the two living counts **of the interactive battle object**, whichever
  battle the removal happened in. Hunger compares with this at turn start (section 3). Neither the
  counter nor "last seen" is reset between battles. Both start at 3 in the file.
  - At turn 1 a Hunger unit only syncs.
  - From turn 2, any removal since the last sync heals the first Hunger unit processed.
  - A sum that happens to equal the stale value from the last battle is missed.

  **code**
- **Bleed table** (word per side and slot, value 0 or 75). It is cleared on turn 1, cleared for a
  unit when bleeding kills it, and shifted with the records when any unit is removed (c2a95a), so it
  stays attached to the right unit. **code**
  - Edge case: the shift loop always runs at least once. When the removed unit is slot 12 of side
    1, it copies the value of side 2's slot 1 into the empty slot 12 and then clears side 2's slot 1.
    So the enemy's first unit stops bleeding. For slot 12 of side 2 it reads and clears two padding
    bytes after the table, which is harmless. **code**
- **Cripple marks** are not shifted (section 7).
- **Removal without on-kill effects:** bleeding, PreventiveStrike, Counterblow, Suicide's later
  removal. **code**

## 10. Magic drain, ManaDrain and MinMagicPower

The patch replaces the vanilla drain (c2851a). **code**

**Per-type values**, computed once at load (c283dc, c284e6, c28579):
- **D (drain)** = `ManaDrain` if the type has a non-zero value. Otherwise it is the vanilla school
  drain: DecSpellLife, DecSpellElemental or DecSpellDeath (2 / 5 / 2 in the shipped `_Global.ini`).
  For a type with school 0 it is 0.
- **F (floor)** = `MinMagicPower` if non-zero. Otherwise it is MinSpellLife / MinSpellElemental /
  MinSpellDeath (15 / 15 / 0), plus **25 for a Death-school Undead type**. The +25 is added only on
  this default path, not to an explicit MinMagicPower.
- Both stay 0 for a type whose own MagicPower is 0, even if a unit of that type later gains MP from
  levels or items.
- A school-0 type with MagicPower would get the floor table's unused slot, which holds 16 777 215.
  This is theoretical; no such type exists.

**Each turn from turn 2, for a unit with MP > 0** (before its regeneration):
1. MP −= D, or MP += D for **Concentration**, with no cap.
2. If MP < F, MP = F. So the floor also raises a weak caster.
3. MP < 0 becomes 0 (vanilla).

The per-type table index assumes the loader's type counter is 0-based, as the battle reads it.
**code** for the battle side; the loader side is medium confidence.

## 11. Battle AI hooks

- **Manevres-0 targets** (c25b63, melee scoring). The vanilla score is
  `dmg × round((r + 1) × M)`: dmg is the estimated damage on the target, r a second estimate (the
  target's estimated reply damage, or a record field, depending on the case) and M the target's
  Manevres (plus half its actions left when those are negative). When M is 0, the patch substitutes
  1.5 × K. K is read from four bytes that are mostly the next instruction (1 164 546 049), so the
  factor is about 1.75 × 10⁹. The rounded value keeps only its low 32 bits and the product with dmg
  is 32-bit too, so the score wraps: with r = 0, damage 1 gives a huge positive score and damage 2 a
  negative one; with r ≥ 1 the factor itself already wraps. Suicide units waiting for removal have
  M = 0, so the AI can fixate on, or ignore, them erratically. **code** (486c26–486c44)
- **c25da0.** In another AI routine (486bb9), a unit with Manevres 0 skips the "has actions left"
  requirement. **code** for the test; what that routine does is in the AI notes.
- **c26c7e.** With 6 columns, an AI scan (489549) starts at column 2 instead of 1. **code**

## 12. Outside battle

These belong to other subsystems; the rules are summarised here so that the Community list is
complete. Economy, experience and event details live in [economy.md](economy.md),
[experience.md](experience.md) and the event notes.

**Economy.** **code**
- **Elementals are paid in mana.** Units whose nature is Elemental are hired, resurrected and paid
  in mana:
  - their barracks price label shows the mana cost, and the affordability tests use mana
    (c25e18, c25e38, c25e61, c260c5, c260de, c2609e);
  - the hire and resurrection amounts go to mana, clamped at 0 (c26085);
  - their daily wages are summed apart and taken from mana after the gold bill (c25e97, c25ec5,
    c25eef).
  - Mana ≤ 0 after paying sets mana to 0 and marks the Elementals unpaid. The marking loop covers
    army slots 1–11 only (c25feb).
  - Elementals are never dropped for lack of gold (c25f43).
- **AddPayment** (c2502b, c250e4, c25001). The flag is set when any of the player's 12 slot records
  has bonus 16, counting stale slots, as of the last strength computation. The flag is one global,
  so it discounts **every** paying side's gold bill, not only the player's: the bill is multiplied by
  178/256 when the player's income is non-zero, and by 78/256 when it is zero (integer division).
  The income tested is always player 0's (as last computed), whoever is paying (payment 4a41d8).
- **Tactical cost** (c25d86): a non-positive cost x becomes |x| + 1 (0 → 1), and positive costs are
  kept.
- **Caster** (c25cc9, c27435, c27448). The flag means "a Caster unit was seen in the army whose units
  were last recomputed". With no Archmage (divisor 1), spell mana cost and cast time become
  `floor(× 0.8)`. With an Archmage only the ÷2 applies; the two do not stack.

**Experience** (c2518f, c25264). The victory award per unit is computed in floating point:
`award × HeroExpirienceModificator × difficulty factor × the beaten army's correction / 10⁶`,
rounded to nearest. Its absolute value is then taken and capped at 5256. **code**

**Events** (c2669e, c26754, c26826, c268b4, c2698c, c27862 and the fragments listed in the map
notes). **code**
- **The marker.** An event whose "no meeting" byte is 1 becomes a script, and its patrol value
  becomes an opcode. Values 1–5 are byte add, set, <, = and > on any event record. Values 6–20
  set items, unit type, speed, faction, relations, lasting spells, named character or XP, branch the
  next map's name, remove book spells, change an army's figure, set a random flag digit, set an
  AI army's target cell, and teleport the hero. Values 14 and 19 also add conditions, and 21 makes
  the named-character check test the character's class.
- **The RNG.** The random flag digit uses an LCG (×1664525 + 1013904223) seeded from the CPU
  timestamp when the events array is allocated. It draws an inclusive range by rejection, and its
  rejection loop has a one-instruction slip (c28dae, c28deb).
- **Gold and mana conditions.** The vanilla gold condition and a new mana condition use a signed
  ≥/≤ comparison (c25200).

**Interface.** **code**
- **Keys.** F1 opens the load list on autosaves and F2 on private saves (F1 only works when
  autosaves exist). F3 opens the save window, on the world map and in the second key handler
  (c26ae3, c26b99, c269ac, c26bbd). F4 starts an endless wait and F5 stops it (c277d2, c27802,
  c2782b, c27842).
- **Marker colours.** 11 `Color…` option keys colour the map markers: player, ally and enemy armies
  and buildings, neutral, ruin, empty and full village, harbor (c27604, c276eb … c27795).
- **AnimationSpeed** S (percent) has an options slider. It caps two animation delays at
  (100 − S) × 5 and (100 − S) × 3 (c2945c … c295e8).
- **Info card.** The unit info card has two new lines: the bleed value (%) and the type's Evasion.
  - Bleed (c2a4be): for units outside the hero's army the card reads the **next** player slot's
    value.
  - Evasion (c2a837): the card indexes the type table one off from the battle code, so it shows the
    next type's value.

  Both lines are hidden when the value is 0.
- **Window frames.** Two extra frames, black and yellow, are loaded, but the only code that selects
  the black one has no caller (c281c7, c2828e, c282ad).

**Data loading tripwires** (c26c08, c26c18, c26c4c). They are deliberate traps:
- an artefact with `d-Manevres` = 4 or `Cost` = 32000 makes the loader spin forever;
- the 4th unit type with `LevelMultipler` 200 or 160 jumps to address 0.

Two more (c26c2a, c26c39) compare a pointer with a number and never fire. **code**

**World and engine.**
- The map-object owner write always takes the object's own branch (c25ab0).
- The map renderer skips a cell whose tile pointer is out of range on maps narrower than 200
  (c27248).

**code**

**Operand patches.** Original instructions read or write `.bonus` tables at c35000 (493365, 49a5d0,
4e3db3, 4e3e1d), c36000 (492ef1, 4c16dc, 4dc9d4–4dcb05), and a 17-entry terrain-name table plus a
dword table at c36500/c365a0 (4cdd1a, 4cdd7d, 4cde5a, 4cdf06, 4b3a60). The option values sit at
c39000–c3934b, and 4c756a shows the patch's version label. **code** for the sites, **unknown** for
the purpose (probably enlarged or relocated tables; the changelog lists crash fixes on map edges and
bad caches).

## 13. Wide row formation

The wide formation is **vanilla** code, switched by `[Options] OptValue11 = 1` (column count 6
instead of 4, 4ed044; battle.md §6). With 6 columns the battle grid blocks:
- back row columns 1 and 6;
- reserve columns 1, 2, 5 and 6.

That leaves front 6, back 4 (columns 2–5) and reserve 2 (columns 3–4), still 12 cells. Units are
placed in column order 4, 3, 5, 2, 6, 1 (48395c, 4ed018). The Community adds only the AI scan
start of section 11. **code**

## 14. Bonuses with no or partial effect

- **Dominate** (31): its routine (c263d2) is jumped over and has no other entry. No effect. **code**
- **HoldLine** (46): no code tests it. The bonus per neighbour that the changelog describes (25%)
  is not implemented. **code**
- **Caster** (28): world spells only (section 12). **code**
- **Battle-Parry** sound: loaded, never played. **code**
- **Unreachable code.** Several unreachable variants sit in `.mod`: a power-based HP drain after
  spells (c26ddb), a protection clamp that clamps ProtectLife three times (c26d72), older Suicide and
  bleed-shift copies (c25bc3, c2a909), and the PreventiveStrike-before-spell copy (c28c00). None of
  them ever runs. **code**

## 15. Corrections to battle.md section 7

- **PreventiveStrike:** no preventive shot before spells (battle.md says it shoots before a shot or
  spell).
- **Hunger:** the melee-kill heal has the address slip of section 5. Its "living units" counter is
  global and persists across battles.
- **Flying:** shooters and mages do not merely gain a melee option. Their shot or spell on the three
  facing front cells is replaced by melee.
- **Stun:** 30% of the unit's current initiative (not of the effective initiative), the same amount
  per hit.
- **ArmorBreaker:** each defence keeps `x − floor(x/4)`.
- **Assault:** the exact damage-taken test is in section 6.
- **Bleed:** the magic copy faults on an already-bleeding target.
- **Splash:** the 80% malus applies in every battle, and the 40% factor loses 1 on multiples of 5.
  Follow-ups run the full hit path, counters included. Splash heals ignore cripple marks.

## Razdor now → original (src/rules/battle.rs, magic.rs, economy.rs, events.rs, src/ui)

Most Community rules are implemented. The rows below are those where Razdor's code differed;
the Status column says where it stands now.

| # | Topic | Razdor now | Original (this exe) | § | Status |
|---|---|---|---|---|---|
| 1 | Splash outside interactive battles | The 80% malus in every battle and in the AI's estimates (`attack_factor`, `cast_power`); off screen no follow-ups for melee, shots and spells | 80% malus in every battle and in AI estimates; no follow-ups | 6 | Matches |
| 2 | Splash 40% rounding | The patch's multiply-high constants (`splash_scale`): 10 → 3, 100 → 39; negative attacks wrap | `floor(0.4x)`, but multiples of 5 give one less (10 → 3, 100 → 39); negative attacks wrap | 6 | Matches |
| 3 | Splash follow-ups | The splash state machine: each follow-up re-runs the whole case (preventive strike, chain, vampirism, counter blow, kill check) at 40% for any attacker while the state is set; the primary's counter at 40%, the second neighbour's at full; neighbours in record order, each side once | Each follow-up re-runs the whole action case: neighbour PreventiveStrike and Counterblow (at 40%), vampirism, BloodThrist per kill; the primary target's counter is also at 40%. The two neighbours are taken in slot-index order (each side once), not left before right | 6 | Matches |
| 4 | Splash heal on crippled units | Follow-ups bypass the cell filter and heal or bless them | Heals or blesses them (follow-ups bypass the cell filter) | 6, 7 | Matches |
| 5 | PreventiveStrike before spells | Never | Never: only before melee and shots | 7 | Matches |
| 6 | Flying shooters and mages | The three facing front cells are melee only | Those cells become melee only, for every Flying unit in rows 1–2 | 7 | Matches |
| 7 | Stun amount | 30% of the current initiative (`cur_initiative`: the base as of the turn start, with the turn-1 bonus), the same each hit | 30% of current initiative (base + turn-1 Artillery/FirstShot bonus), the same each hit | 5 | Matches |
| 8 | ArmorBreaker rounding | `x − x×25/100` (5 → 4, 1 → 1) | `x − floor(x/4)` (5 → 4, 1 → 1) | 5 | Matches |
| 9 | FateGift vs Neutralize | FateGift in the on-hit block, after KillingStrike and before Neutralize | FateGift is checked first and saves; Neutralize then clears an already erased bonus | 5 | Matches |
| 10 | Mage poison threshold | `poison_power`: raw MP × (99 − prot) / 100 (Elemental `/114`), unsigned for Life and Elemental, > 15 | `MP × (99 − prot) / 100` (Elemental: `/114`), raw MP, protection always applied, > 15 | 5 | Matches |
| 11 | Assault damage taken | The misaligned dword test: building byte and the initiative modifier's top bytes, ≥ 16 | ×2/3 if the attacker's building defence is 1–127, or it has none and its initiative modifier is < 0 (or ≥ 4096) | 6 | Matches |
| 12 | Turn start order | Per unit: reset, bonuses, drain, own regen tick (with removal) | Per unit: reset, bonuses, drain, own regen tick (with removal), then the next unit. Berserk uses HP before the unit's own tick; Hunger sees only earlier units' deaths | 3 | Matches |
| 13 | Berserk on spells vs Drying | Berserk recomputed before Drying's loss | Berserk recomputed before Drying's loss | 5 | Matches |
| 14 | Hunger kill heal | Heals on a melee kill, per hit (a follow-up would count, but one bonus per unit rules Splash out) | Tests a wrong address (unknown outcome); melee only, also per splash follow-up | 5 | Guess kept |
| 15 | BloodThrist | +1 per killing hit in all three paths, follow-ups included; not for a target FateGift saves | +1 per killing hit, splash follow-ups included, in all three paths | 5 | Matches |
| 16 | Suicide | HP 0, regen −99, no actions, stays in its side's list (counts, holds its cell) until a counter blow or the next turn start removes it; its vampirism can heal it. It cannot be targeted (guess) | HP 0 and off the grid but not removed until the next turn start; can still be countered (then removed), healed by its own vampirism, and scored oddly by the AI | 5, 11 | Matches (targeting a guess) |
| 17 | Undead Death floor | +25 only on the default floor | +25 only when the default floor is used | 10 | Matches |
| 18 | Drain for types without MagicPower | Per type (`drain_of`): no drain, no floor | No drain, no floor (both 0) | 10 | Matches |
| 19 | Magic Bleed on a bleeding target | Bleed stays at 75 | The patch faults (access violation). Razdor should **not** reproduce this; keep as a documented deviation | 5 | Deviation kept |
| 20 | Cripple marks after deaths | A table by side and record index, not shifted | Stay on the slot index, so they can pass to another unit | 7 | Matches |
| 21 | AI vs Manevres-0 targets | The factor `MANEVRES_0_FACTOR` (1.5 × 1 164 546 049), the rounded product's low 32 bits and a 32-bit score, as the patch | Factor ≈ 1.75×10⁹ with 32-bit wrap: erratic | 11 | Matches |
| 22 | Flock sizes | The side blocks of the battle on screen (`PatchGlobals`), refreshed after each of its actions; off-screen battles read them | Living counts of the interactive battle's two sides as of its last completed action (the starting counts on turn 1), so deaths during the current turn start are not seen yet; simulations see the interactive battle's counts | 3 | Matches |
| 23 | Hunger counter | Global (`PatchGlobals`, 3 at start, never reset); every removal stores the living count of the battle on screen; turn 1 only looks | Global, persists across battles (only matters on coincidences) | 9 | Matches |
| 24 | Keys | F1 help, F2 language, F5 quick save, F9 quick load (`ui/hotkeys.rs`) | F1/F2 load lists (autosave / private), F3 save window, F4/F5 endless wait on/off | 12 | Interface, left |
| 25 | AnimationSpeed option | Matches: the «Скорость анимаций в битве» slider in the settings (`OptionSld4`), 0–99 %, the install's value until moved; it caps the strike's slide at (100 − S)·5 ms and its effect at (100 − S)·3 ms | Slider in the options; caps two animation delays at (100 − S)·5 and ·3 | 12 | Matches |
| 26 | Info card Bleed and Evasion lines | Bleeding shown as a battle status | Card lines with the value (with two off-by-one slips) | 12 | Presentation, left |
| 27 | Tripwires | Not reproduced | Hangs or crashes on certain ini values. Deliberately not reproduced | 12 | Deviation kept |

Also brought in line with these: the bleed loss and Flock's step divide unsigned (a negative
sum kills, a negative attack gives a huge step), the bleed shift clears the enemy's first
bleeding when the player's 12th record goes, Evasion is a byte and divides unsigned, the hit
chains test a strike's damage (not its power) in the on-hit block, and Bastion's doubling wraps.
EternalGift's attack is AB unless AB is 0 (an AB cursed below 0 still takes it), and its
initiative change reaches the turn order and Stun only at the next turn start, since both read
the current initiative.

Already matching (checked against the code): Berserk, Fortify, the Garrison fix, FirstShot, Bastion,
FasterAttack, Assault doubling, Exhaustion, Drying amount, CtrPoison, PoisonS, PoisonArmorIgnore, the
pierce sets, Evasion, Potent, Bleed amount and death, KillingStrike, FateGift effects, NoHeal regen and
targeting, EternalGift, Concentration, ManaDrain/MinMagicPower defaults, the wide formation, the
Elemental mana economy, AddPayment, Caster, the 5256 XP cap, the event opcodes (`rules/events.rs`,
`rules/script.rs`) and the marker colours (`ui/minimap.rs`).

## Unknowns

- **Hunger kill heal:** which memory the slipped test reads in practice, and so how often it heals.
- **Magic Bleed fault:** what the game does after the access violation (crash, crash log, or an
  exception handler that lets the battle continue).
- **Flock:** whether the army copies count units that are left out of the battle, and what the
  copies hold in battles other than the player's.
- **Suicide display:** how the screen shows a Suicide unit waiting for removal, and whether its cell
  can be targeted.
- **Splash:** what happens when the attacker dies during a follow-up (the stale slot index).
- **Per-type tables:** whether the loader's type index is 0-based, as the battle assumes. If it were
  1-based, ManaDrain, MinMagicPower and Evasion would all be read one type off.
- **Operand patches:** the purpose of the relocated `.bonus` tables (c35000, c36000, c365a0).
- **The 22nd vanilla name:** which bonus name the vanilla loader compared for value 22 before the patch
  overwrote that comparison. The loader's string pool holds no spare name after FlankStrike (the next
  string is the `Surrender` key, which the loader reads right after the bonus), so that compare
  probably reused a name that appears earlier in the pool (**data**, layout only).
- **Two fields:** the per-unit field set to 1 at every turn start (record +0x99), and the vanilla
  use of the attack counter (+0x6D).
- **AI:** the purpose of the AI routine that c25da0 relaxes.
