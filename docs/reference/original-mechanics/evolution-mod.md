# The «Community Update Evolution Mod» build

A player install (backlog 1305) of the «Discord Times Community Update Evolution Mod». It ships its
own `DiscordTimes.exe` (1 187 840 bytes, the same size and section layout as Community 1.2's) and
its own data. This file lists how it differs from the Community 1.2 install Razdor targets, from a
byte comparison of the two executables and their data files, read in Ghidra/objdump. Addresses are
the same in both executables unless stated. Confidence tags as in the other specs: **code** (read
in the executable), **data** (read in the ini files), **unknown**.

## 0. What it is

- The executable is an **older Community Update build**: the vanilla game plus an earlier version
  of the Community patch. Its patch section `.mod` (0xc25000) is byte-identical to Community 1.2's
  up to 0xc25a53 and ends at 0xc283c6, where 1.2's runs on to 0xc2aa18; the `.bonus` section is
  identical. The CODE section differs in 74 places: about 40 are Community 1.2 hooks the build does
  not have, the rest are constants and a few UI changes. **code**
- The mod's own changelog describes these older rules (Stun 25%, Fortify +20%, Splash 70/30,
  Concentration growing the power) and adds data: new units, items and spells, mana payment for
  Elemental units, event extensions. **data**
- How Razdor can tell the builds apart: Community 1.2 has a `JMP` (`E9`) at 0x484466 (the magic
  drain hook) and the strings `ManaDrain`, `MinMagicPower`, `MinSpellLife`; this build has the
  original `IMUL` there and none of those strings. **code**

## 1. Battle rules that differ

| Rule | Community 1.2 (Razdor now) | Evolution build | Evidence |
|---|---|---|---|
| Magic drain | Patch block c2851a: per-type `ManaDrain`/`MinMagicPower`, else `DecSpell…`/`MinSpell…` from `_Global.ini` | The original block 0x484466–0x484683: Life −DecSpellLife then floor 15; Elemental −DecSpellelemental, then 0 for a negative Undead or Rogue caster, else floor 15; Death −DecSpellDeath, Undead floor 25; then MP < 0 → 0. Concentration adds the drain (hooks c25c75/c25c91/c25cad). No `MinSpell…` keys in its `_Global.ini`. | **code** |
| `_Global.ini` loader | `DecSpellDeath` → 0x4ed3c8, `DecSpellelemental` → 0x4ed3c4, then the `MinSpell…` hook (0x4e45ba) | `DecSpellDeath` → 0x4ed3c4, `DecSpellelemental` → 0x4ed3c8, no `MinSpell…`; its drain reads them crosswise, so each school still gets its own key | **code** |
| Per-type `ManaDrain`, `MinMagicPower`, `Evasion` | Read by the units loader (0x48f580, 0x48f5ce) | Not read | **code** |
| Fortify | defence += max(1, DB × **25** / 100) × min(turn − 1, 5) | DB × **20** / 100 (c25a51) | **code** |
| Stun, melee | initiative −30% (c27e5a) | **−25%** (c27e93) | **code** |
| Stun, magic | −30% (c26e1f) | **−21%** (c26e44) | **code** |
| Stun, shot | −30% (c2899a) | **none**: the shot path returns to the game at 0x48b2cf instead of c2899a | **code** |
| Splash | the hit target ×80%, the neighbours ×40% (c270ae, c2731a, c27337, magic c27374) | ×**70%** and ×**30%** (constants 0xb3333333 / 0x4ccccccd) | **code** |
| FirstShot | +30 initiative on turn 1 (+30 more with building defence ≥ 10), fix c28935 | the fix is absent (c26e95 runs the original code): the +60 variant at c26e9a is reached. Exact value **unknown** (to read) | **code**, value **unknown** |
| On-hit block on magic hits | c29984 (PoisonArmorIgnore, Bleed, …) | absent (returns at 0x48b1ed) | **code** |
| Flying (legal cells) | 0x485554 | absent | **code** |
| NoHeal target filter | 0x485669 | absent | **code** |
| PoisonArmorIgnore piercing | 0x485992, 0x4859f4 | absent | **code** |
| Assault ×2/3 | 0x485a78 | absent | **code** |
| Evasion | 0x485b26 | absent | **code** |
| Bleed (table shift, loss at action start) | 0x489f62, 0x48a677 | absent | **code** |
| EternalGift (bless/curse by school) | 0x48a9f1, 0x48aa6d, 0x48aa95, 0x48aef1, 0x48af64, 0x48b0ac | absent | **code** |
| PreventiveStrike | 0x48b214, 0x48b2f6 | absent | **code** |
| Bonus tokens | 52 bonuses (+ Assault, EternalGift, FateGift, …: c28e85, c29001) | the build's token table stops before them; unknown tokens are not parsed (Assault, EternalGift, Evasion, FateGift, Flying, NoHeal, PreventiveStrike are not in the executable) | **code** |
| Elemental bless/curse speed steps (483680) | power < 20 → 0, < 45 → 1, < 100 → 2, else 3 | a case on 20–44 → 1, 45–99 → 2, 100–255 → 3, anything else (≥ 256 or < 20) → 0 | **code** |

## 2. Map, events and UI

| Area | Community 1.2 | Evolution build | Evidence |
|---|---|---|---|
| Event opcodes | 22 extended opcodes (0xc27862 compares up to 0x16) | 16 (`0x10`); several opcode hooks absent (0x471b61, 0x4a822d, 0x4a9bfd, 0x4ab29d, 0x4ab350, 0x4ab39d) | **code** |
| Event condition 14 loop counter (c280b6) | read unsigned (`MOVZX`) | read **signed** (`MOVSX`) | **code** |
| Map marker colours, animation speed option | hooks at 0x49e282…, 0x4afd45, 0x4b006a, 0x4b8c48, 0x4bf4d3, 0x4d02c5, 0x4d3390, 0x4dc113 | absent; the options window has 4 rows, not 5 (0x4bf343, 0x4d0225, 0x4d3308) | **code** |
| Bonus lists in the info card | 59 entries (0x4dcb0f, 0x4e3e31), bleed and evasion lines (0x4924c1, 0x4e3027, 0x4e44fc) | 44 entries, no bleed/evasion lines | **code** |
| Army record update at 0x496c00 | written when +0x16B3 differs from +0x3788 | written when they are equal (the test is inverted). Purpose **unknown** | **code** |
| Windows (0x4d9733, 0x4d979e, 0x4be99d, 0x4bf0c9, 0x4bf250) | 1.2's layout | older layout values | **code**, details **unknown** |

## 3. Data

| File | Difference | Evidence |
|---|---|---|
| `_Global.ini` | no `MinSpellLife`, `MinSpellElemental`, `MinSpellDeath`; `BattleEndTurn` 20 (1.2: 25); `ShotWeaponRange` 80 (1.2: 60); the army composition lists name the new unit ids | **data** |
| `Rus_Units.ini` | 161 unit types (1.2: 102); no unit uses `ManaDrain`/`MinMagicPower` | **data** |
| `Rus_Artefacts.ini` | 192 items (1.2: 167) | **data** |
| `Rus_Spells.ini` | 42 sections (1.2: 2) and a key `DeltaPercentHits` 1.2 does not have: a different layout, to read | **data** |
| `Rus_DiscordTimes.ini` | no `Bonus22`…`Bonus52` names, no `SBleed`, `AnimationSpeed`, `OptionSld…`; has `Option0`…`Option3` | **data** |
| Mod mechanics from its changelog | mana payment for `Nature=Elemental` units (buying, healing, resurrection, daily pay); events that change events, alliances and army speed; equipment/spells/named units set per army and building; XP grants; spell checks on armies | **data**; the code is **unknown** (not yet located) |

## 4. Open

- FirstShot's value in this build (c26e9a path), the magic on-hit block's absence in practice, and
  what the mod's new event features do in code.
- Whether `Rus_Spells.ini`'s layout and the mana payment need loader changes in Razdor.
- Everything above is from the executable and data alone; no replay of the build under Wine yet.
