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
| `Rus_Spells.ini` | 40 spells (1.2: 33), then the empty editor template and `[MapEditorSpecialOptions]`, as in 1.2 (42 sections in all, 1.2: 35). The key `DeltaPercentHits` (9 spells) is read beside `DeltaFixedHits` (3 spells; 1.2 has 8). Same layout, more spells. | **data**; the reader is the same (below) |
| `Rus_DiscordTimes.ini` | no `Bonus22`…`Bonus52` names, no `SBleed`, `AnimationSpeed`, `OptionSld…`; has `Option0`…`Option3` | **data** |
| Mod mechanics from its changelog | mana payment for `Nature=Elemental` units (buying, healing, resurrection, daily pay); events that change events, alliances and army speed; equipment/spells/named units set per army and building; XP grants; spell checks on armies | **data**; the code is **unknown** (not yet located) |

### 3a. How the original loads the mod's data (code, 1.2's executable)

The loaders are the same code in both builds (the `.mod` patch section is identical), and the
mod's data loads with them without a warning in Razdor:

- **Spells** (0x4e36cc opens the files; the per-spell reader 0x49ac64): each `[section]` is read
  for `CostGold`, `CostMana`, `Type`, `TimeWork`, `TimeCast`, `Target`, the icons and effects,
  then `DeltaFixedHits` (0x49b328, stored at +0x284) and `DeltaPercentHits` (0x49b340, +0x288).
  Both keys are read in both builds; Razdor keeps them in separate fields
  (`SpellDef::delta_fixed_hits`, `delta_percent_hits`). A section is a spell when `Name`, `Type`
  and `CostGold` are all non-empty (0x4e0c60). The trailing empty `[Заклинание]` template is not
  one. **Confirmed**: the layout is not a loader difference; the mod has 40 spells (1.2: 33).
- **Units, items**: the counts only differ (161, 192). Every upgrade target and every army
  composition id resolves; the mod's units and items use only the 21 vanilla bonus tokens
  (`Bonus=`), so no token is unknown to the mod's loader.
- **Texts** (`Rus_DiscordTimes.ini`, read through 0x472440): a key the file lacks reads as an
  empty string, the original's default for every text. The bonus names are looked up as
  `Bonus` plus the decimal index (0x4e36cc, the loop at 0x4e3dd7, the number built by 0x471c94),
  so the mod's missing `Bonus22`…`Bonus52` read empty. The mod's option rows are `Option0`…
  `Option3` (**inferred**: its exe holds the literal `Option` and no `OptionSld`, so the key is
  built from `Option` and the row number, not yet read in its code); 1.2's are `OptionSld0`…
  `OptionSld4`.
- **Maps** (`Maps_Rus`, 15): every map parses, and every unit, item and event id it names is
  in the mod's content (`rules::map_refs`); each starts a game (`rules::replay`).

Razdor's loader changes for the mod: the options window reads the mod's row labels and draws
four sliders (no battle-animation row) when the install has `Option0` and no `OptionSld0`
(`ui/main_menu.rs`). A Community install is unchanged.

## 4. Open

- FirstShot's value in this build (c26e9a path), the magic on-hit block's absence in practice, and
  what the mod's new event features do in code.
- The mana payment (mod changelog) and the options window's exact 4-row layout in the mod
  (its row offsets and window height are unread; Razdor keeps the 1.2 positions of the
  checkboxes, leaving one row free). `Rus_Spells.ini` needs no loader change (§3a).
- Everything above is from the executable and data alone; no replay of the build under Wine yet.

## 5. Razdor now → original

Razdor: `src/dt/install.rs` (`build`, `drain_rule`), `src/dt/data.rs` (`Build`, `apply_build`),
`src/rules/battle.rs` (the gates, `stun_percent`, `splash_factors`, `actions_for`), `src/rules/events.rs`
(`opcode14_unit`, `run_opcode`), `src/rules/script.rs`. The build is set at load from the exe; Community
1.2 is the default and unchanged.

| Rule on this build | Razdor now | Status |
|---|---|---|
| Build detection | `E9` at 0x484466 → Community 1.2; the IMUL → older build; no exe → `MinSpellLife` decides | Matches |
| Magic drain | The original's block 0x484466–0x484683 (`Battle::drain_old`) | Matches |
| Fortify | defence + max(1, DB × 20 / 100) × min(turn − 1, 5) (c25a51) | Matches |
| Stun on a blow | −25% of the current initiative (c27e93) | Matches |
| Stun on a spell | −21% (c26e44) | Matches |
| Stun on a shot | none (the shot path returns at 0x48b2cf) | Matches |
| Splash | the hit target ×70% (0xb3333333), the neighbours ×30% (0x4ccccccd), the 1.2 multiply-high method; a negative attack keeps its sign (1.2's fix) | Matches the constants; the sign rule not checked on this exe (OPEN_QUESTIONS) |
| Elemental speed steps (483680) | 20–44 → 1, 45–99 → 2, 100–255 → 3, else 0 | Matches |
| FirstShot | 1.2's rule (+30, +30 with building defence ≥ 10) | Open: the value on this build is unread (OPEN_QUESTIONS) |
| Flying (legal cells, 0x485554) | off | Matches |
| NoHeal target filter (0x485669) and NoHeal marks | off | Matches |
| PoisonArmorIgnore piercing (0x485992, 0x4859f4) | off; its poison is kept | Matches |
| Assault ×2/3 (0x485a78) and its turn-1 stat ×2 | off | Matches |
| Evasion (0x485b26) and the loader (0x48f5ce) | off; the type's Evasion is not read | Matches |
| Bleed (0x489f62, 0x48a677, the on-hit bleed) | off | Matches |
| EternalGift (0x48a9f1…0x48b0ac) | off: blessings and curses change this turn's modifiers | Matches |
| FateGift (on-hit) | off | Matches |
| PreventiveStrike (0x48b214, 0x48b2f6) | off | Matches |
| On-hit block after a spell (c29984) | not run (it returns at 0x48b1ed) | Matches |
| Bonus tokens | Assault, EternalGift, FateGift, Flying, NoHeal, PreventiveStrike read as no bonus; the others as on 1.2 | Matches |
| Per-type ManaDrain, MinMagicPower, Evasion | not read at load | Matches |
| Event opcodes 16–21 | do nothing; the cell test of opcode 19 and the named-squad check of 21 are off | Matches |
| Event condition 14's unit byte (c280b6) | read signed | Matches the read; no observable difference in Razdor's unit model (OPEN_QUESTIONS) |
