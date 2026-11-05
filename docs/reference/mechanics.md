# Discord Times — mechanics and data-file notes for the Razdor remake

> **Superseded in part:** the rules read directly from the executable are in
> [original-mechanics/](original-mechanics/README.md) (battle, world and AI, economy, spells,
> events). Where they disagree with this file, they win; §8 below still lists what Razdor
> does today.


Written in our own words from the game's shipped help, the editor manual, the Community
Update notes and, where those say nothing, a reading of `DiscordTimes.exe`
(Delphi, Community Update 1.2 / game 1.8.1, `.mod` patch section included).

Confidence tags:
- **[doc]**: stated in the shipped help, editor manual, modder notes or UI strings.
- **[exe]**: read from the executable's code. The formula is reliable, but what a field
  means is our interpretation.
- **[inf]**: inferred from data values or naming. Plausible, not verified.
- **[unk]**: unknown.

Sources used: `Help/Как Играть.htm`, `Help/Документация к Редактору Сценариев.doc`,
`Дополнительные файлы/*` (modder note, changelog, new-editor guide, ID notebooks),
`Rus_*.ini`, `_Global.ini`, `DTMapEdit_Rus.Ini`, `Rus_MapEdit.ini`, `DiscordTimes.exe`.

---

## 0. Ini file format (all data files)

- Encoding: Windows-1251, CRLF. The Community Update extras (`Дополнительные файлы`) are already UTF-8. [doc/inf]
- `//` starts a comment line. `[Section]` headers. `Key=Value`. Values may be empty
  (for example `TimeWork=` means "instant / no duration"). [doc]
- `Rus_Units.ini` / `Rus_Artefacts.ini` sections are named `[<GlobalIndex> <Name>]`, and the
  entity is keyed by `GlobalIndex`. Every spell section in `Rus_Spells.ini` has the same
  name, `[Заклинание]`, so a spell's ID is its **order in the file**. The file ends with an
  empty template section and `[MapEditorSpecialOptions] Generated=1`, which must not be
  deleted (the file says so). [doc]
- Maps and events refer to units, items and spells by numeric ID, so the load order must be kept. [inf]
- Parser: `ini_parse.py` in the scratch dir. It handles duplicate section names and returns a list.

---

## 1. Units (`Rus_Units.ini`, 102 entries)

### 1.1 Fields

| Key | Meaning | Tag |
|---|---|---|
| `GlobalIndex` | Unit type ID (1–102). IDs 1–3 are the three hero classes. | doc |
| `Name`, `Descript` | Display name and card text. | doc |
| `IconIndex` | Portrait index into `Graphics/Objects/Persones.ugs` / `Icons.ugs`. | inf |
| `Cost` | Hire price in gold. It is also the base for wages, healing and resurrection (not for XP: the tactical cost comes from the stats). | doc/exe |
| `CostMultipler` | Editor label "Коррекция" next to "Сила" (strength). Percent correction of the unit's **tactical cost**, a strength computed from its stats (original-mechanics/experience.md §1): `strength × CostMultipler / 100`. Values 50–100. | exe |
| `CostGoldDiv` | Divisor stored per type (1–5; undead 2–5). Not used by the wage code. Probably divides the unit's personal money, which the winner takes as loot. | inf |
| `StartExpirience` | XP needed for the first level-up of this class. | exe |
| `LevelMultipler` | Percent growth of the XP requirement per level (140 for most units, 150/160/170 for heroes). | exe |
| `Hits` | Max HP. | doc |
| `AttackBlow` | Melee attack. If >0 the unit can fight in melee. | doc/exe |
| `AttackShot` | Ranged attack. If >0 the unit is a shooter. | doc/exe |
| `MagicPower` | Magic power ("сила магии"). If >0 the unit is a caster. | doc/exe |
| `DefenceBlow` / `DefenceShot` | Flat defence against melee / ranged attacks. | doc/exe |
| `ProtectLife/Death/Elemental` | **Percent** resistance to hostile magic of that school (editor: "% Защита от магии …"). | doc/exe |
| `Initiative` | Turn order. Higher acts first. | doc |
| `Manevres` | Actions per turn ("количество действий"). 1–3. | doc |
| `Regen` | % regeneration (editor "% Регенерация"). When it applies (per battle turn or per day) is unverified. | doc/unk |
| `Vampirizm` | % vampirism: part of damage dealt comes back as HP. | doc/inf |
| `Magic` | `LifeMagic` / `ElementalMagic` / `DeathMagic` (editor: Жизни / Стихий / Смерти). Magic school. Warriors can carry one too (Paladin, Church Guard). | doc |
| `MagicDirection` | `ToAll` / `ToEnemy` / `ToAlly` (editor "На всех / На чужих / На своих"): whether the caster targets enemies, allies or both. | doc |
| `Nature` | Creature type. Editor order: Normal(0, default), `Undead`(1), `Elemental`(2), `Rogue`(3), `Animal`(4), `Hero`(5), `People`(6). It changes magic effects (see 3.3). Community Update: `Nature=Elemental` units are **paid in mana** (hire, heal, resurrect, daily wage). | doc/exe |
| `Bonus` | One special ability (enum, see 1.3). | doc |
| `Surrender` | Editor label "Плен" (captivity). Present on priests, nuns, mages and townsfolk (10–100). Probably the chance or amount that makes a beaten unit surrender. Surrendered enemies give the victor **mana** ("they pray for you"). | inf |
| `NextUnitN`, `NextUnitNLevel` (N=1..3) | Upgrade tree: the unit can be promoted to class `NextUnitN`. The player may promote any unit that has gained a level; only the AI checks `NextUnitNLevel` (always 1), against its 0-based level (experience.md §4). | exe |
| `d-<Stat>` | Stat gain per level for Hits, AttackBlow, DefenceBlow, AttackShot, DefenceShot, MagicPower, Initiative, Manevres, Protect*, Regen, Vampirizm. | doc |
| Community: `Evasion=1..100` | "Неуязвимость": ignores this % of physical damage, applied last: `dmg = max(1, dmg·(100−Evasion)/100)`. | doc/exe |
| Community: `MinMagicPower`, `ManaDrain` | Per-unit floor and per-turn loss of magic power in battle (they override the globals). | doc |

The exe also recognises some tokens that vanilla data never uses: `Nature=Animal/Hero`,
bonuses `AddPayment`, `FlankStrike`, and many Community bonus tokens (list in 1.3). [exe]

### 1.2 Unit classes

There is no class field. The role follows from which stats are above zero. [exe]
- **Warrior**: `AttackBlow>0`. Melee only, and only from the front row.
- **Shooter**: `AttackShot>0`. Ranged.
- **Mage**: `MagicPower>0` with `Magic` and `MagicDirection`. Heals and blesses allies, and strikes or curses enemies (see 3.3).
- Some units have both melee and magic (Paladin, Church Guard, Inquisitor).

Heroes: Knight (id 1, warrior), Archmage (id 2, Elemental mage, ToEnemy), Ranger (id 3, shooter, Regen 5). [doc]

Hero class bonuses [doc; knight part also exe]:
- **Knight**: the army takes 10% less **non-magic** damage (`dmg·90/100`, hard-coded 90). The knight can also use global spells, but slowly.
- **Archmage**: casts global spells twice as fast and for 50% of the mana. The army gets no other bonus. In battle the archmage slows enemies (fewer actions and less initiative).
- **Ranger**: the army moves 20% faster on the map, and wounded units heal 20% of max HP per day.

### 1.3 Bonus enum (value = UI `BonusN` index)

| # | ini token | Effect (own words) | Tag |
|---|---|---|---|
| 1 | SpearDefense | Long weapon: on battle turn 1 its melee defence is tripled. | doc+exe |
| 2 | HorseAtack | Fast attack: +1 action on the first battle turn. | doc |
| 3 | ArmorIgnore | Piercing: its attacks ignore the target's defence (building defence still counts). | doc+exe |
| 4 | ArmyMedic | Medic: the whole army heals 15% of HP per day. | doc |
| 5 | Merchant | Trader: selling gives +50%, buying costs −30%. | doc |
| 6 | DeathCurse | Whoever kills this unit dies at once. | doc |
| 7 | GodAnger (UI "Кара") | +10 damage on top, ignoring all defences (also adds to magic strikes). | doc+exe |
| 8 | GodStrike (UI "Гнев") | +20 damage on top, ignoring all defences. | doc+exe |
| 9 | Unvulnerabe | Every physical hit does exactly 1 damage. | doc+exe |
| 10 | VampirsGist | Dark gift: ignores the target's defence. Incoming physical damage ×2/3. | doc+exe |
| 11 | OldVampirsGist | As 10, plus +1 action on turn 1. | doc |
| 12 | Evasive | Incoming physical damage ×2/3 (UI says "70%"). | doc+exe |
| 13 | Ghost | Immune to physical harm (takes 1 per hit), and its killer dies. | doc+exe |
| 14 | Artillery | Barrage: always acts first in battle, and its damage ignores defence. | doc |
| 15 | Garrison | Inside a castle or fort: stats ×2, and physical damage ×2/3 (code: needs building defence ≥10). | doc+exe |
| 16 | AddPayment | Rear service: the army's wages −30%. | doc |
| 17 | Poison | After a solid hit the target loses 15% HP every battle turn. | doc |
| 18 | Dead | Corpse: shots do only 30% damage. | doc+exe |
| 19 | FastDead | As 18, plus +1 action on turn 1. | doc+exe |
| 20 | Counterblow | Hits back when struck in melee. | doc |
| 21 | FlankStrike | Doubles its attack on a "flank" strike (the long strike through empty cells, see 2.3). | doc+exe |
| 22–52 | Community tokens (index order of the exe's table) | Hunger, Berserk, Exhaustion, Drying, CtrPoison, Suicide, Caster (−20% spell time and cost), Splash (80% to the target and 40% to its neighbours), Fortify, Dominate, Concentration, PoisonS (25%/turn), Stun, Potent (magic ignores protection), FirstShot, Bastion, Flying (can hit any row), Flock, Bleed, HoldLine (does not work), ArmorBreaker, PoisonArmorIgnore (#45), FasterAttack, NoHeal, PreventiveStrike, Neutralize, KillingStrike (<25% HP dies), BloodThrist, Assault (#50), EternalGift (buffs last the whole battle), FateGift. | doc |

### 1.4 Experience and levels

The full rules, with the exe's addresses, are in
[original-mechanics/experience.md](original-mechanics/experience.md). In short [exe]:

- The exe counts levels from 0 and shows them +1; Razdor counts from 1 as the interface does.
  XP to the next level: **`round(StartExpirience × (LevelMultipler/100)^(level−1))`** (Razdor
  numbering). Example: a Militia unit needs 60, then 84, then 118 …; a fresh sorceress
  (580) shows "0 / 580".
- Each level adds `d-*`; protections, regeneration and vampirism instead close the gap to
  100 by `d` percent (at most 99). Magic power needs a school. HP is kept as it is (an
  unhurt unit stays full).
- **Tactical cost** is a strength computed from the stats (a toughness from HP and
  exponentials of the defences, an attack value from the best attack, actions and
  initiative, bonus terms) × `CostMultipler`/100.
- **Battle XP**: pool = `enemy's starting strength div 20 × share of own starting HP not
  lost`; a survivor's share is `pool/4/N₀ × ((4 − row) + row × useful actions / all
  actions)`, at least 1 (N₀ counts the dead too). `MainExpCorrection` and `ExpCorrection`
  feed a term the exe computes and never uses.
- The player's units (victory only) gain `share × HeroExpirienceModificator × F × the
  beaten army's correction / 10⁶`, at most 5256 (Community); F is 120, 100 with
  "impossible difficulty". AI armies gain `share × AIExpiriencePercent/100` only in
  battles between AI armies.
- Promotion: any of the player's units with a level (not the hero), free, back to level 1
  with no XP. The AI promotes by its own pick after each gain.
- Event XP goes to the hero only, as it is. The hero preset has no starting XP (offset 8 is
  gold, 12 mana).

### 1.5 Wages (daily upkeep) [exe + doc]

Let `C = Cost`. The code has two branches, chosen by a per-unit hiring kind (1 or 2). Which
units fall in which kind is **[unk]**. Razdor's guess: kind 2 (mercenaries) for `Nature=Rogue` units, kind 1 for the rest (section 8).
- Kind 1: `wage = round(C / CostRecrutDiv × f)`, where `f = 0.25` if C≤50, `0.5` if C≤100, `0.75` if C≤150, and `1` otherwise. `CostRecrutDiv=2`, so a Militia unit (C=50) costs 6 gold/day.
- Kind 2: `wage = C / CostMercenaryDiv` (=2).
- The hero is free. A unit left in a garrison stops asking for pay from its second day there. [doc]
- When gold runs short, the unpaid units **do not fight** until they are paid. A village
  innkeeper option can restore them. `MaxTimeNotUpkeep=10080` minutes (7 days): how long an
  army keeps going unpaid, probably before the units leave. [doc/inf]
- Rear Service (`AddPayment`) −30%. Elemental-nature units are paid in mana (Community). [doc]

### 1.6 Healing and resurrection [doc + exe]

- Heal in a town, a castle (own or friendly) or a church. Cost ≈ `Cost × HealingConst% ×
  (missing HP / max HP)`, where `HealingConst=50`. It takes `HealingTime=60` minutes of game time. [doc/exe/inf]
- Resurrection only in a **town or church**: `Cost × ResurectConst%` (300% = 3× cost). Allowed
  within `MaxTimeResurection=10080` minutes (7 days) of death. Otherwise the body can only be buried. [doc/exe]
- Garrisons heal `GarrisonAutoHeal=10%` of HP per day. Medic units give 15% per day, and the Ranger hero 20% per day. [doc]
- Potions (see 4) heal instantly on the army screen.

---

## 2. Battle

### 2.1 Formation [doc + exe]

- Each side has **3 rows × N columns**: row 1 = front (swords icon), row 2 = back (bow icon),
  row 3 = reserve (tent icon). **N = 4 in vanilla.** It becomes 6 only with the Community
  option "wide front row" (`OptValue11`). An army holds at most 12 units (4×3 = 12). [doc/exe]
- The front row screens the back row. When every front-row unit is dead, the back row
  becomes the front. [doc]
- The back row gets **+`Row2Def`=5 defence against shots only**. [doc+exe]
- Units in the reserve cannot be attacked and cannot act. The reserve is for pulling wounded
  units out and bringing fresh ones in. [doc]
- In your own building, defence also gets the building's extra defence (set per garrison in the editor). [doc/exe]
- Preferred column order (AI and auto placement) goes from the centre outwards: 3,2,4,1 for
  4 columns and 4,3,5,2,6,1 for 6. [exe]

### 2.2 Turn order and actions

- The first move goes to the side with the highest-initiative unit. Units act by initiative,
  highest first. The attacking army gets **+1 initiative**. [doc]
- Each unit has `Manevres` actions per turn. An action is an attack, a heal or bless, or a
  move. Space skips the rest of the unit's turn. [doc]
- Movement [exe]:
  - From the front or back row, a unit moves to an empty cell in column c−1, c or c+1 of the front or back row.
  - A unit in the reserve can move to any empty cell of the front or back row.
  - Moving to any empty reserve cell is allowed only when a certain per-unit flag is set. The flag's condition is [unk].
- The battle ends when one side has no units left, or when the turn counter reaches **`BattleEndTurn`=25**. What happens in that undecided case is [unk]. [exe]
- **You cannot retreat** or leave to the map before the battle ends (only "exit to menu" or "restart"). Pulling units into the reserve is the only way to save them. [doc]

### 2.3 Who can hit whom (enemy cells) [exe]

Let the attacker stand in column c.
- **Warrior** (needs `AttackBlow>0`, must be in **row 1**):
  - It can hit occupied enemy **front-row** cells at c−1, c and c+1 (a normal strike).
  - If all three are empty, it gets a **long/flank strike** at the nearest occupied enemy
    front-row cell to the right (scanning c+2, c+3 …) and the nearest to the left (c−2, c−3 …).
    That strike **halves the target's defence**, and the FlankStrike bonus doubles the attack.
  - A warrior in row 2 or 3 cannot attack.
- **Shooter** (`AttackShot>0`):
  - From **row 2** it can target any enemy in rows 1–2.
  - From **row 1** it can target any enemy in rows 1–2 only if the three enemy front cells
    c−1..c+1 are all empty. Otherwise it can target only those adjacent front-row enemies.
- **Mage, hostile** (`MagicPower>0`, direction includes enemies, i.e. not `ToAlly`):
  - From row 2 it can target any enemy in rows 1–2.
  - From row 1 it can do so only if the three opposite cells are empty. Otherwise it cannot cast.
- **Mage, friendly** (direction includes allies): it can target an own unit in row 1 or 2
  that is wounded, or one of its buff states. Wounded targets are healed, others are blessed. [exe/inf]
- The reserve (row 3) can never be targeted.

### 2.4 Physical damage [exe; "attack minus defence" is doc]

For attack kind `K` (melee, flank-melee or shot):

```
atk = attacker.Attack(K) + attacker.bonusAttack            // AttackBlow or AttackShot
      (Splash bonus: ×0.8 on the main target, ×0.4 on neighbours)
def = target.Defence(K) + target.bonusDefence + target.buildingDefence
      (+ Row2Def if K = shot and the target is in row 2)
if SpearDefense and battle turn 1 and K is melee:  def ×= 3
if flank strike:  def /= 2; if attacker has FlankStrike: atk ×= 2
if attacker has ArmorIgnore / PoisonArmorIgnore / VampirsGist / OldVampirsGist:  def = 0
dmg = atk − def, at least 1                            // no random roll
dmg ×= 2/3 if target has VampirsGist, OldVampirsGist or Evasive
dmg ×= 2/3 again if target has Garrison and building defence ≥ 10
dmg ×= 3/10 if K = shot and target is Dead or FastDead
dmg ×= 90/100 if the target's army has a Knight hero
dmg  = 1 if target is Unvulnerabe or Ghost
dmg += 10 (GodAnger) or 20 (GodStrike) for the attacker's bonus
dmg  = max(1, dmg · (100 − target.Evasion)/100)       // Community
```

- There is **no random damage range**: damage is deterministic.
- An alternative percentage formula, `atk × (1 − def/100)`, exists in the code, but real
  battles set the subtractive mode. The other mode is probably used for estimates. [exe]
- Counterattacks happen only through bonuses (Counterblow, PreventiveStrike). [doc]
- The AI estimates "hits needed to kill" as `ceil(targetHP / dmg)`. `CrazyAI` (random
  scatter in target choice) is read from the ini but **never used** by this build. [exe]

### 2.5 Death and the hero

- A unit at 0 HP is dead. It stays in the army as a corpse until it is resurrected (in a town
  or church, within 7 days) or buried. Dead units cannot hold items. [doc]
- The hero cannot die while some unit of the army survives. After the battle the hero turns
  out to be alive but badly wounded (an event condition checks "the hero has only 1 HP"). If
  the whole army dies, the scenario is lost. [doc]
- An AI feudal lord cannot be killed for good while he still owns a building: he retreats
  there and recovers. To finish him, capture all his buildings. [doc] (The exe has no
  retreat: a beaten lord respawns at a building he owns after his days, ai.md §12.)
- AI armies can respawn after an editor-set number of days, either the leader alone or the whole army. [doc]

### 2.6 Rewards

- The winner takes the loser's gold divided by `VictoryGoldDiv` (2), with no minimum, in a
  fight against the player's army (exe 0x4c50ec). `MinVictoryGold` (25) applies only to
  AI-against-AI fights (`original-mechanics/economy.md §3`). [doc]
- The loser's items become trophies. Surrendered units give mana. Capturing a castle or fort
  changes its owner. Clearing ruins gives the treasure. [doc]
- Editor flag: an army can "carry no personal money", and then there is no gold to take. [doc]

---

## 3. Magic

### 3.1 Resources and who casts

- The two resources are **gold** and **mana** ("магическая энергия"). Both belong to the hero
  and cannot be dropped or stored elsewhere. Mana comes from villages (peasants pray at
  midnight), from surrendered enemies, from events and from the witch option in villages. [doc]
- **Global spells** are cast by the hero from the spell book on the world map, before a
  battle, onto the whole own army (`Target=Hero`) or a whole enemy army (`Target=Enemy`).
  They cannot be cast during battle. **Casting costs game time**, so the target may run off
  or reach you first. [doc]
- **Battle magic** comes from mage units using their own `MagicPower`, not from the hero's mana. [doc/exe]
- The spell book has a capacity limit (message "no room in the book"); the size is [unk].
  Spells are **learned for gold** at a sanctuary or library in a town, church or altar, and
  **cast for mana**. [doc]
- The Archmage casts twice as fast for half the mana. The Caster bonus gives −20% time and cost. [doc]

### 3.2 `Rus_Spells.ini` fields (34 spells + template)

| Key | Meaning |
|---|---|
| `Name` | Spell name. |
| `CostGold` | Price to learn it. |
| `CostMana` | Mana cost to cast. |
| `Type` | School: `Life` / `Elemental` / `Death`. |
| `TimeWork` | Duration in game hours at caster level 0. Empty means instant. 9999 in practice means permanent. It may scale with the caster's level [unk]. |
| `TimeCast` | Casting time in game hours at caster level 0. |
| `Target` | `Hero` (own army) or `Enemy`. The exe also knows `OneEnemy`. |
| `Icon1..3`, `ColorC1..3` | Three layered icon images plus a colour tint for each. |
| `Effect1..3` | Visual effects (bottom, back, front): `file,r,g,b,duration_ms,yOffset,scale×1000,start_ms`. |
| `DeltaFixedHits` | Instant HP change: + heals, − damages (for example Lightning −15, Chain lightning −40). |
| `DeltaPercentHits` | Instant % HP change. Recognised by the exe, unused in vanilla. |
| `d-<Stat>` | Flat change for the duration. |
| `p-<Stat>` | Percent change for the duration. |
| `p-LifeLose` | Percent life drain ("отбирает жизнь"), used by scripted curses. |

At most three modifiers fit on the spell screen. Stats that can be modified: Hits, AttackBlow,
DefenceBlow, AttackShot, DefenceShot, MagicPower, Initiative, Manevres, Protect*, Regen,
Vampirizm. Several spells (cost 0 or 1) are **scenario-only** (Ghost gift, Gate guard, Rockfall,
Armageddon, Transfer ritual …) and are applied by events. [doc/inf]

Village option 1 ("priest casts a very long-lasting good spell instead of tribute") gives
spells beyond their normal duration. [doc]

### 3.3 Battle magic by school [exe]

The caster's power `P = MagicPower` (with bonuses). For **hostile** effects,
`P = round(P × (1 − Protect_school(target)/100))`. Friendly effects are not reduced.

Actions: **strike** (damage), **curse** (debuff), **heal**, **bless** (buff).

| | Life | Elemental | Death |
|---|---|---|---|
| Strike dmg | P; **×2 vs Undead**; ×0.75 vs Elemental | 0.75·P | P; ×0.5 vs Undead; ×0.75 vs Elemental |
| Heal | +P; none on Undead or Elemental | +P/2 | works **only on Undead** |
| Bless | def +(3P/(2·BlessMainSpell)+1) = P/4+1; atk +3P/(2·BlessNextSpell) = P/8. None on Undead or Elemental. | +actions f(P); initiative +(P/WizardMainSpell+1) = P/7+1 | atk +(P/BlessMainSpell+1) = P/6+1; def +P/BlessNextSpell = P/12 |
| Curse | def −(1+P/(⅔·CurseMainSpell)) = −(1+P/3); atk −P/10 | actions −f(P); initiative −(1+P/7) | atk −(1+P/CurseMainSpell) = −(1+P/5); def −P/CurseNextSpell = −P/10 |

- `f(P)` = 0 if P<20, 1 if P<45, 2 if P<100, 3 if P≥100. That is how the Archmage "freezes" enemies.
- A curse cannot push remaining actions below 0.
- Heals are capped at max HP. Damage is capped at current HP.
- The attack buff is dropped on targets that have no attack.
- GodAnger/GodStrike add +10/+20 to magic strikes as well.
- Which hostile action a mage uses is [inf]: the preview code picks *strike* for Life casters and *curse* otherwise.
- **Magic power drains in battle** by `DecSpellLife/Death/Elemental` (2/2/5) per turn, down
  to a floor of `MinSpellLife/Death/Elemental` (15/0/15). Community settings can override
  this per unit. The help's "the archmage loses 5 power per turn" matches. [doc/exe]
- Buffs and curses last a limited time. The Community bonus EternalGift makes them last the whole battle. The exact duration is [unk].

---

## 4. Artifacts and items (`Rus_Artefacts.ini`, 167 entries)

| Key | Meaning |
|---|---|
| `GlobalIndex`, `Name`, `Descript` | ID, name, text. |
| `Icon` | Image file, for example `A000.Tga`. |
| `Cost` | Base price. A **negative price marks a personal item** that cannot be taken away (family relic, −1700). [inf] |
| `Type` | `BlowWeapon` (warriors only), `ShotWeapon` (shooters only), `Staff` (mages only), `Armor`, `Helm`, `Shield`, `Ring`, `Amulet`, `Potion`, `Item` (trade goods, cannot be equipped). [doc] |
| `Bonus` | Grants a unit bonus from 1.3 while worn. |
| `Magic` | Grants or changes the magic school (for example Death staff → DeathMagic). |
| `d-<Stat>` | Flat addition. |
| `p-<Stat>` | Percent change. |
| `f-<Stat>` | **Fixed value**. On gear it sets the stat to that value (armour `f-DefenceBlow=26`, sword `f-AttackBlow=55`, `f-Manevres=2`). On potions it is the healing amount (`f-Hits=75`). Application order with d-/p- is [unk]. [inf] |

Rules [doc]:
- A unit wears **4 items**. The editor note says "the leader puts on all 4 artifacts at once".
- It holds **only one weapon** and **no two items of the same type**.
- Class limits: warrior weapons, shooter weapons, mage staffs, and "dark forces only" items.
- The dead cannot hold items.
- Potions are dropped onto a unit's card and are not kept in the inventory. Their effect,
  except healing, lasts until the end of the **next battle**.
- The hero has a separate backpack inventory; its size is [unk].
- Selling pays **`ItemSaleCost`=25%** of the price ("4× less than buying"). Merchant: +50% on sale, −30% on purchase.
- Prices differ between buildings, based on the building's relation to the player (a bad relation raises prices).
- Markets have fixed stock plus random items within an editor price range. Churches usually have a max price of 245, so they sell potions and church amulets.

---

## 5. World, economy, time

### 5.1 Time [doc]

- Game time runs **only while the army moves, waits or casts a spell**. Standing still freezes time.
- AI armies share the same clock and move at the same time as you.
- The clock uses minutes (events store absolute minutes since year 0) and shows day, month and year.
- Wait buttons: 1 hour, or 4 hours (the 4-hour wait cannot be cut short).
- Villages refill **at 00:00**. The daily report (income, wages, unpaid units) and the autosave come at **12:00**, as
  the gameplay footage shows (video notes, §1).
- The calendar shows year, month (1–12) and day (**0–29**) and whole hours: `1204 год, 5 месяц, 19 день, 9 час`.
- The path preview shows the travel time.
- Community keys: F1/F2 quick load, F3 save, F4/F5 endless time skip.

### 5.2 Movement and map [doc/unk]

- Terrain textures (editor order): shallows/fords, coastal water, deep sea, lava fields, road,
  grass lowland, grass plain, dry plain, swampy ground, impassable bog, sand dunes, clay,
  stony ground, scorched land, snow, impassable snowdrifts. The exe enum is Shallow, Water,
  DeepWater, FlameLand, Road, Toto, LowLand, Land, Plain, Swamp, DeepSwamp, Desert,
  Badground, Rock, Dust, Snow, Ice.
- Mountains and rocks block. Hills slow. Normal trees slow. Large trees ("непроходимые чащи") block. Deep water blocks on foot.
- The **per-terrain movement cost is not in the data files** and was not found in the exe [unk].
- Fog of war: unexplored ground is dark and **counts as impassable** until the hero has seen it.
  Scripted "lanterns" reveal areas (radius up to 24, at most 5 events per point). [doc]
- Army speed: Ranger +20%. The editor sets a per-army "speed correction" (about −3..+5).
  Community events can change speeds.
- Grid: plain 32×22 px cells, 8 neighbours (evidence in `dtm-format.md` §4) [inf].
- Contact: you meet another army when it is on an adjacent cell. Click it to talk or fight.
  Hostile armies attack you as you pass by. [doc]
- Ships: rented at a **shipyard for `ShipCost`=250 gold**. The ship takes you anywhere on the
  coast and waits where you land. Pirate and merchant AI ships exist. [doc]

### 5.3 Buildings (editor types) [doc]

| Type | What it does |
|---|---|
| Town | Quests and rumours, market, hiring, healing and resurrection, spell library. Some income. Its garrison is AI-only. |
| Village | Tribute: gold (and mana) at midnight, accumulating up to an editor maximum. Can pay the owner a daily income (rarely used). Barracks AI-only. Instead of tribute it may offer one of 5 options: a long-lasting blessing, healing the army, furs worth more than the tribute, a magic power ritual, or paying off unpaid units. |
| Castle | Must be captured. Hire from the garrison, heal, leave troops. Small steady gold income. Can have extra garrison defence. Quests. A friendly castle only lets you hire from its garrison and heal. |
| Fort | A small castle. Rogue armies whose home it was may suddenly retake it. |
| Church | Learn spells, hire monks, buy potions and anti-evil amulets, heal, **resurrect**. |
| Tavern | News and quests. |
| Market | Buy and sell items. News and quests. |
| Smithy | No function (news and quests only). |
| Shipyard | Rent a ship. |
| Altar | AI hiring. May have a shop or library. |
| Entrance (dungeon) | No function. |
| Ruins | Garrison plus treasure. The defenders use the treasure items against you. Some ruins are hidden under mountains or forest and show grey on the minimap. |
| Bridges (stone and wooden), obelisk | No function. |

Building screens: main hall (description and quests), barracks (hire and heal), garrison
(leave troops; named units cannot be left), shop, sanctuary (spells). You enter a building
by stepping onto its cell. Only the building the hero stands in can be managed. [doc]

Garrison fights run automatically while you are away. Troops in buildings get a defence bonus. [doc]

Hiring stock: each building lists up to 6 unit types, each with a start count and a max. Stock
regrows over time (`MaxDayCountForNewUnit=10` days). The "all types" flag lets the player hire
any listed type. [doc/inf]

### 5.4 Income

- Castles (and optionally towns and villages) have an editor-set daily gold income. Villages
  have gold and mana income with a maximum stock. [doc]
- The hero screen shows the daily income and the daily wages. [doc]
- The editor can give an AI army extra gold income. [doc]

### 5.5 Factions and relations [doc]

- Four groups: player (green), ally, neighbour, enemy. The editor also uses green, blue, yellow and red.
- Each group has a relation to every other group, from −3 to +3 (0–100%). Relations drive AI hostility and prices.
- Community events can change an army's or building's group and relations at runtime.

### 5.6 AI (`_Global.ini`) [doc]

- Behaviour styles: **feudal** (like the player: income, tribute, wages, hiring, shopping),
  **rogue** (no wages, no tribute, hires rogue units, retakes its forts), **peasant** (wanders).
- Target models: standard, aggressive, passive, hoarding, trading. The `Min/Max…Target` lists
  hold one priority per model for attacking, random wandering, "talking" to friendly armies,
  healing, garrisons, shopping and villages.
- Other settings: `AIDistance0..2` view ranges, `ZeroDensity`, `NeedUpkeepDay=5` (the AI keeps
  5 days of wages in reserve), aggression, patrol radius, and flags (ignored by AI, chases only
  the player, no random targets, no talking, no interest in buildings).
- **Ini bug**: `_Global.ini` spells the key `MixHealingTarget`, but the exe reads
  `MinHealingTarget`, so that line has no effect. [exe]
- `[AIArmyGeneration]` lists the unit IDs the generator may use per army theme (Normal, HolyArmy, Piesant, Rogue, Assasin, Undead, Hero, Vampires).
- `ShotWeaponRange=60`: shooters with ranged attack of 60 or more count as cannon ("пушкарь"), probably for visuals and sound [inf].

---

## 6. Scenario scripting (editor manual + Community guide)

- **Engine loop** [doc]: on every tick, check all events; run the first whose conditions
  hold; repeat until none fires. So one event can enable another within the same tick.
- **Event types**: global (checked anywhere), local (checked when the player is at a given
  map cell or building), quest (goes into the journal), rumour (a local event the player must pick).
- **Time window**: start date and hour, how long it stays active, and a repeat every N days.
  Flags: *subordinate* (runs only when another event triggers it), *relative* (no own start
  time; another event sets it), *repeatable*.
- **Conditions**:
  - hero class
  - events that happened with answer Yes, happened with answer No, or have not happened
  - flag variable set or unset (`/name` means unset)
  - a yes/no confirmation question (optionally repeatable after Yes)
  - the player beat certain armies
  - meeting a given army (needed to talk to friendly armies instead of fighting)
  - hero stats: level, gold, "holiness and mana", number of units, army strength
  - who owns buildings, artifacts, or named units (player, green, blue, yellow, red, or "not the player")
  - army beaten by anyone, army active or inactive, army at its home building
  - Community: a named unit's class, active spells on armies, army position
- **Results**:
  - text (no text means a silent event), a picture, a chained subordinate event, closing a quest
  - resources: XP, gold, mana
  - moving a relative event's start time, forcing a delay on the player, setting or clearing flags (`+name`, `-name`)
  - adding units (optionally taken from another army), removing units ("added unit" / "any unit"; last joined leaves first, optionally sent to another army or moved next to the hero)
  - learning spells, giving or taking items
  - revealing map areas (lanterns), showing, activating or deactivating armies, changing patrol radius
  - changing the hero's class (class bonuses are lost), starting a battle with an army
  - "no meeting with army" (clears the meeting flag, preventing loops), applying a spell to the player's army
- **Community extensions**: the "no meeting" flag plus a patrol-change value selects an opcode,
  and the resource fields carry its arguments:
  - 1–5: edit or compare fields of other events (relative offset)
  - 6: equip items on an AI unit (or a building's garrison with a negative ID)
  - 7: replace a unit type in a slot
  - 8: set army speed
  - 9: change faction
  - 10: change relation
  - 11: apply permanent spells to a unit or army
  - 12: place a named unit in a slot
  - 13: give XP
  - 14: check spells
  - 15: campaign branch (map number and variant)
  - 16: remove spells from the book
  - 17: change the army's map model
  - 18: random flag RAND 1..9
  - 19: set an AI target or check an AI position
  - 20: teleport the player
- **Scenario settings**:
  - name and description; the **victory event** and **defeat event**
  - hero start: XP, mana, gold, start building (optionally per class), items, spells, army
  - relations between groups
  - campaign: first map or next map, the next scenario's name, and what carries over from the previous map
- **Map file** (`Maps_Rus/*.DTm`, binary): the ID notebooks record record sizes: army 89 bytes, building 358 bytes, lantern or event point 99 bytes, event about 165 bytes plus text. See the notebooks for field offsets if a map loader is needed.

---

## 7. Prototype guess (before Stage 2) → real rule

Stage 2 replaced the left column with the right one for units, battle, items and wages;
see section 8 for the choices Razdor makes where the original is unknown.

| Topic | Razdor now | Original |
|---|---|---|
| Formation | 2 rows × 6 | **3 rows × 4** (front, back, reserve). 6 columns only with the Community "wide row" option. 12 units max. |
| Reserve | none | Row 3: cannot attack or be attacked. Used to rotate wounded units out. |
| Damage | random min..max roll, minus armour, min 1 | **Fixed** `Attack − Defence`, min 1. Separate melee and ranged defence. No roll. |
| Armour / defence | one "armor" value | `DefenceBlow` and `DefenceShot` separately. Back row gets +5 **vs shots only**. |
| Flank | diagonal with empty straight-ahead cell → ×2 | Diagonals (c±1) are normal hits. The "flank" is a long strike to the nearest front-row enemy when **all three** opposite cells are empty. It **halves the target's defence**. ×2 attack only with the FlankStrike bonus. |
| Shooters | may target anyone | From the back row: anyone in the enemy front or back row. From the front row: only adjacent enemies, unless none are adjacent. Never the reserve. |
| Magic | ignores armour | `MagicPower × (1 − Protect%/100)` by school, with nature multipliers (Life ×2 vs undead). Buffs, curses, heals and strikes follow the table in 3.3. Mage power drains each turn. |
| Healer | heal 10, range 4 | Heals by its magic power (Life: `P`, not on undead). Can also bless. |
| Counterattack | none | None by default. The Counterblow and PreventiveStrike bonuses add one. |
| Turn order | initiative desc, ties → player | Initiative desc. **Attacker +1 initiative**. |
| Actions | Ranger 2, others 1 | `Manevres` 1–3 per unit (most shooters and mages 2). Ranger hero 2, Archmage 2, Knight 1. |
| Battle cap | 20 rounds, stalemate | `BattleEndTurn` = **25**. No retreat option. |
| Hero stats | Knight 60 HP 10–14 dmg armour 5 | Knight 80 HP, atk 45, def 15/10, init 9, 1 action. Archmage 50 HP, magic 25, prot 35%, init 26, 2 actions. Ranger 65 HP, shot 30, def 5/5, init 19, 2 actions, regen 5. |
| Hero bonuses | none | Knight: army −10% physical damage taken. Archmage: spells 2× faster and ½ mana. Ranger: +20% map speed, +20% HP/day heal. |
| Hero death | hero dead = defeat | The hero survives (wounded) while any unit lives. Defeat means the whole army is wiped. |
| XP / levels | out of scope | Core system: XP to next level = `StartExp·(LevelMult/100)^L`, `d-*` stat gains, upgrade trees. |
| Wages | fixed per type (3–5) | Derived from Cost (see 1.5). Unpaid units skip battles. Garrisoned units stop costing pay after 1 day. |
| Castle heal | free on entry | Costs gold (~50% of Cost × wounded fraction) and 60 min. Resurrection 3× Cost, town or church, ≤7 days. |
| Church | free full heal | Paid healing, resurrection, spells, potions, monks. |
| Village | tribute 10 gold **or** priest heal | Tribute of gold **and mana** at 00:00, up to an editor max. Priest, hunter, witch and innkeeper alternatives. |
| Items | 4 slots, any unit, 1 of each type, pack 16 | 4 slots and 1 of each type are right. Weapon and staff class limits. Only one weapon. Potions used from the army screen (effect lasts until the next battle ends). `f-` fixed stats. |
| Sell price | 50% | **25%** (`ItemSaleCost`). Merchant +50% sell, −30% buy. Prices depend on faction relation. |
| Market restock | weekly Monday | Fixed stock plus random items in a price range. The restock rule is [unk]. |
| Loot | random items | The loser's gold ÷2 (min 25), all its items, mana from surrendered units. Ruins: an editor treasure. |
| Time | moves only when walking | Also passes when **waiting** (1 or 4 h) and when **casting global spells**. |
| Day tick | midnight | Villages refill at midnight; the daily report (income, wages) is at noon (footage). |
| Terrain cost | road 30 min … swamp 3 h | Not in the data [unk]. 16 textures plus hills and trees (which slow), mountains and thickets (which block). |
| Ships | out of scope | Shipyard rent 250 gold, any coast. |
| Fog of war | out of scope | Unexplored ground is impassable until seen (in since Stage 5, see §8.2). |
| Global magic | none | Spell book: learn for gold, cast for mana and time, on a whole army, before battles. |

---

## 8. Razdor implementation choices

Where the sections above say [unk] or [inf], Razdor (`src/rules/`) makes these choices.
Each is marked *(guess)* in the code.

- **Levels** follow original-mechanics/experience.md (`src/rules/experience.rs`,
  `units.rs`): level 1 as hired (the exe's 0); promotion is open to any non-hero unit with a
  level, free, back to level 1 with no XP and HP kept; items the new class cannot wear go to
  the pack *(guess)*. The loader moves two upgrade options into slots 1 and 3, as the
  original's does, so the AI's Militia and Infantry picks never land on an empty slot.
- **Item modifiers** (economy.md §5): each worn item's `f-` above 0 replaces its stat in slot
  order (a later slot wins); the potions' `d-`, the items' `d-`; the potions' `p-`, then each
  item's `p-` in turn, compounding (truncated each time); lasting spells come after (their
  `d-` added, their `p-` compounding; the original adds spells' `d-` before the items' `p-`).
  An item's `Magic` replaces the unit's school. Potions: `f-Hits` heals at once; other
  modifiers last until the next battle ends.
- **Tactical cost** and **battle XP**: as in experience.md §1 and §3, with the whole pool
  term (the exe's damage-exchange fields are never written, so they are 0). A Razdor "wait"
  spends the unit's remaining actions as passes. The tactical cost of a type at a level is
  cached. The difficulty factor F is read from `Rus_DiscordTimes.ini` `[Options]
  OptValue10` (100 when set, else 120); the demo uses 100 and a player modifier of 100.
  An army's experience correction of 0 is read as 100 *(guess: no shipped army has 0)*.
- **Wages and daily healing**: see the Stage 4 notes below (`src/rules/economy.rs`). A Medic
  heals every wounded unit of its army 10% at midnight, the Ranger hero his army 15% at
  noon; both can happen the same day.
- **Battle** (`src/rules/battle.rs`, `formation.rs`) follows original-mechanics/battle.md,
  every row of its "Razdor now → original" table: the wide row is front 6 / back 4 / reserve
  2 (drawn 2 × 6, the reserve at the back row's ends), the threshold turn order with ties and
  +1 initiative to the player, modifiers reset at every turn start (EternalGift writes the
  battle stats instead), automatic mage actions (curse unless the target already has a
  negative modifier, else strike; heal the wounded, else bless once a turn), one reserve move
  in or out per unit and turn, collapse after deaths and after the actor's last action, the
  turn limit as a victory after the first action of turn 25, surrender (only the surrendering
  units' `Surrender` becomes mana), Knight −20%, Poison as regeneration −20, the piercing sets
  per path, one bonus per unit, the deterministic scored AI and the code's numbers for every
  Community bonus. The remaining choices:
  - `OptValue9` ("improved enemy AI") comes from the install, and the settings window can
    change it (easy = the normal level, expert = improved); AI-vs-AI battles keep level 0 and
    no `Splash` (`Battle::set_simulation`).
  - The AI's magic scoring follows the notes, which are approximate for Elemental casters
    (the front-row hits cap is left out); a target with 0 Manevres counts as 1 in the melee
    score *(guess)*.
  - `Assault`'s turn-1 doubling is kept for the battle *(guess: it doubles the battle stats,
    which are not reset)*.
  - Where a unit is several kinds at once, a cell's action is magic over a shot over melee
    (later codes overwrite earlier ones in 484c4c); Flying's melee only where nothing else
    fits.
  - `Splash` on hostile magic: each neighbour gets its own curse-or-strike. `Suicide` happens
    after the splash hits.
  - Units standing on a blocked cell (old saves) move to the first free cell when a battle
    starts.
- **Community unit fields**: `Evasion` (applied last, 2.4), `MinMagicPower` and `ManaDrain`
  (per-unit floor and drain of magic power) are implemented.
- **World grid, movement, time** follow original-mechanics/world.md (`src/rules/map.rs`,
  `world.rs`, `game.rs`): 8-neighbour squares; the planner weighs an orthogonal step 2 and a
  diagonal 3 and prices the cell entered (`cost × weight`); walking charges the cell left,
  `cost × speed` minutes, ×1.5 diagonally. Speed: knight and archmage 5, ranger 4; AI armies
  `max(1, 5 − correction)`, one less when led by the Archmage unit. Each hero step and each
  30-minute wait tick plays over 150 ms of real time (1 h = 2 ticks, 4 h = 8); AI armies bank
  the minutes of every hero step and tick (up to 200) and step when they cover `cost(next) ×
  speed`. The built-in demo keeps its own hex grid (odd rows shifted right, every step
  weight 2).
- **Terrain values** (cost units; minutes on foot = value × 5): road 3, stony soil 4; grass,
  dry plain, sand, clay, scorched land 5; snowy ground 6; marsh 8. Shallows (2) and coastal
  water (1) are water: never walked, sailed at their value (10 and 5 minutes). Deep sea, lava,
  the impassable swamp and snowdrifts block everyone.
- **Objects**: hills 1–3 set a base of 2 (class 4: 3) that the terrain adds to; trees +4,
  dead trees +6, class 12 +4 on their own cell; mountains 5–7, rocks 8 and thickets 11 block.
  A massif (classes 1–8) covers a square of `sprite div 10` cells a side whose bottom-right
  cell is its own. Nothing keeps a road open under an object.
- **Buildings**: the footprint (`size_x × size_y` up and left of the anchor, one more row
  above when wider than tall) is road on foot and for ships; only the building clicked (or the one a
  route ends in) is entered, on any of its cells; buildings crossed on the way are not visited
  (the player's rule); enemy buildings (attitude < 0) are routed around unless clicked; bridges are walked, never entered.
  Only castles and forts of attitude ≤ 0 (not his) and ruins not yet his bar the hero's
  route, unless clicked or the one he stands in; stationary guards (patrol radius 0) bar it
  too, except the one clicked; at sea he does not pass under bridges. A garrison fights when
  he enters (its extra defence counts); winning takes the place, whatever it is (castle, fort, ruins; owner = player,
  faction 1, its income and mana count from the next noon) and gives ruins' treasure. AI
  armies stand at the footprint's centre `(x0 + sx/2, y0 + sy/2)`.
- **Villages** start with one day's tribute; at midnight it grows by
  `round(income × √(1 − stock/max))`, capped (slower as it fills). Entering the village takes all of it at once (no button; economy.md §3, the footage's "tribute already collected"), unless the village makes an offer: then accepting it empties the village, declining takes the tribute.
- **Armies**: model 7 / byte 63 = off the map at start; ships (byte 72) sail, see §8.6. Hostile = the original's
  relation below 0 (0x4a0868, `world::relation`): the player's attitude to the army's faction
  (header matrix) and the army's own to the player; so an enemy-faction army whose own
  attitudes were left at 0 (РК1's mage) is hostile. Contact on neighbouring cells (diagonals
  too), right after a step of the hero: hostile ones attack, friendly ones greet when their
  talk counter is above 0 (world.md §4.3). Word 17 is the army's starting gold, byte 80 ×
  10 its daily income. The demo's gangs chase the hero within 6 cells and patrol, resting
  30–180 min between legs *(Razdor's demo rule)*. Everything else is the AI's (§8.8).
  Troops: the middle byte of each triple is levels above the first; the leader is a troop of
  its own. Loot: see "Victory loot" in the economy notes below.
- **Hero start**: exactly the preset's cell (no relocation); the preset's start building
  (byte 16) and every building flagged for his class (building byte 353 + class) become his.
  A preset on the water ("Тихая пристань") starts him aboard a ship *(guess)*. Gold (preset
  offset 8), mana (offset 12), troops, artifacts and spells from the preset; no starting XP
  (experience.md §5).
- **Sight**: 9 / 8 / 10 cells (knight / archmage / ranger), a circle in cells (+0.6 for the
  soft edge, M); lanterns the same in cells. Unexplored cells are impassable to the hero only.

Stage 4 (buildings and economy, `rules/town.rs`, `rules/game.rs`):

- **Building tabs**: every building has a main hall; towns, castles, forts, churches and altars a
  barracks (villages have one for the AI only); the player's castles and forts a
  garrison; a building with goods a market, with spells a sanctuary; villages a tribute
  tab. A building of attitude below 0 still trades, heals and hires (the footage shows
  trading at a market of attitude −2); the hire tab and the village tribute have no
  attitude test (`original-mechanics/economy.md`, Razdor-now table). A garrison still to be beaten
  opens no window. An ill-disposed castle or fort with no garrison is taken by walking in.
- **Main hall**: lists the building's quest and rumour events (`World::local_events`);
  local events fire by themselves and are not listed. Running them is the event engine's.
The economy follows `original-mechanics/economy.md` (its "Razdor now" column lists every
rule); `src/rules/economy.rs` holds the formulas. In short:

- **Wage kinds**: everything the player hires, his starting army and all AI troops are
  recruits (kind 1, the cost brackets, Delphi rounding: halves to even); the hero is kind 0
  and units an event added kind 3, both free. Kind 2 (`Cost / CostMercenaryDiv`) exists for
  an AI army hiring in a foreign building, which Razdor does not track yet. Corpses are
  billed. Kind-1 elementals are paid in mana (Community); out of mana, all of them go
  unpaid. Rear Service (`AddPayment`): gold wages ×178/256, ×78/256 when the player's daily
  income is 0 (a Community quirk kept as it is).
- **Noon**: income (the player's buildings ×F/100, plus the stock of villages linked to his
  castles) comes in, all wages go out. If the gold is then negative, the cheapest units get
  their wage back and go unpaid until it is not, and the gold is set to 0; free units are
  not refunded *(guess)*. Only on such a short noon, every unit last paid more than
  `MaxTimeNotUpkeep` ago leaves (its items to the pack). The village innkeeper pays everyone.
- **Garrison**: never paid; heals `GarrisonAutoHeal`% at midnight. Scenario garrisons of
  buildings the player owns at the start are his. Capacity: the formation's.
- **Barracks regrowth**: at midnight each slot below its maximum gains one unit with chance
  `1 / (MaxDayCountForNewUnit div max)` (always when that is 0 or 1). The editor's "all
  types" flag is read but has no effect.
- **Difficulty factor F** (100 with "impossible difficulty", else 120): healing
  `max(1, Round(missing/max × Cost × HealingConst% × 100/F))` and resurrection
  `Round(Cost × ResurectConst% × 100/F)` are paid at once and take no game time; sales pay
  `Cost × ItemSaleCost × F / 10000` (a Merchant half again); the player's building income
  ×F/100. Castle and fort stocks are not modelled: an owned building pays its daily income.
- **Corpses and resurrection**: the dead stay in the army (HP 0, still in their cell) and
  drop their items into the pack (what does not fit is lost). Raising (towns and churches)
  restores full HP and is possible until `MaxTimeResurection` minutes after death; later the
  body is buried automatically. The army screen can bury (dismiss) a corpse.
- **Market prices**: `Round(base × m)` with m = 1.7, 1.45, 1.25, 1.1, 1.0, 0.9, 0.75 for
  attitude −3..3 (a building the player owns counts as 3), then a Merchant takes 30% off.
  Spells cost `CostGold`, hires `Cost`. Items worth 1 or less (personal items) cannot be sold.
- **Market stock**: every midnight the fixed goods still unsold stay and the random ones are
  drawn anew: the building's count minus the fixed goods, towns first taking `count div 5 +
  1` healing potions (items 98–100) and maybe one of 96/97/114/115; the rest are market items
  priced in `[max(min, 5), min(max, 5000)]` (the top +1 one time in five), at most twice the
  same (once above 500); the list is sorted by price. The draw is uniform in the window
  (the original leans towards its top; not reproduced).
- **Backpack**: 256 items (a scrolling 5-wide grid). **Spell book**: 15 spells.
- **Victory loot**: from an army, `gold / VictoryGoldDiv` (no minimum) plus its daily wage
  total (not for peasant armies or armies whose units carry no money *(guess)*) and its
  items; an army whose home castle or fort stands empty loses it to the player. Between AI
  armies the loser gives all its gold below `MinVictoryGold`, else `gold / VictoryGoldDiv`. A
  beaten garrison pays its treasure (ruins), the building's stock and one day's income.
  Surrender mana follows original-mechanics/battle.md §5: only the units of a side that
  surrenders give their `Surrender` value.
- **Village offers**: entering a village with gold waiting (not the one that made the last
  offer, until its tribute is taken) rolls in order for the innkeeper (1/2), the priest
  (1/3, spell 1), a long blessing (1/6, one of spells 3/5/7/9/11 for 10× or 5× its time),
  furs (1/6, item 135) and the witch (1/6, 300–500 mana), each with its conditions; the
  first that passes is offered, never the same kind twice in a row. Accepting it empties the
  village. A Rogue hero gets nothing from villages.
- **Not used yet**: `CostGoldDiv`, "dark forces only"
  items, named units that cannot be left in a garrison.

### 8.1 Event engine (Stage 6, `src/rules/events.rs`)

`EventEngine` keeps the script state (answers, firings, relative start times, flags, journal,
a pending question); the game implements `EventWorld` (queries and effects). Outcomes name
events only; texts are read from the scenario at runtime.

- **Scope**: global events are checked anywhere; local events and quests only while the player
  stands at a building or point that lists them; in a **village or shipyard every listed event
  fires on entering, a rumour too** (exe 4ac1a1); elsewhere rumours only when the player picks
  one from `rumours()`; subordinate events only through a chain. An unlisted local event never
  fires on its own *(guess)*. Quests listed in other buildings fire on entering too (the
  original offers them in the main hall with "take the quest"; not built).
- **Window** (economy.md §6): never before the start; with a repeat of R minutes, on every
  (R/1440)-th day since the start, for `max(duration, 1)` **hours** from the start's time of
  day; without one, until `start + max(duration, 1)` hours, or with no end for duration 0
  while it never fired. Relative events are ordinary events whose start
  (a far-future "never" in the files) is moved to `now + delay hours`.
- **Repeats**: an event without "once" fires again on every later check (not twice in the same
  minute; a duration-0 event not within 60 minutes) while its window is open and its
  conditions hold. A once-event is done once it fired, a No answer included. **A chained event
  fires as it is**: its conditions, window, place and "once" are not checked (exe: 4ab1ec hands
  the chained id to the dialog 4a8ae8 or applies it at once, never through the check 4a7b80).
  РК1's ending relies on it: on the "No" path the inn's event chains the victory event, whose
  own condition is the Yes.
- **Loop**: fire the first eligible event (global events in file order, then the local events
  of the point or building the hero stands on, in its list order; a building's only on
  entering it), start over, until none fires; at most 256 firings per run (then `LoopGuard`).
  Chains are cut at depth 32.
- **Questions**: the engine stops with `Question(id)` until `answer(yes)`. A No applies
  nothing but counts as happened (it uses up a once-event); opening a question clears the
  last No. After a Yes the question is asked again only with "repeat after Yes" *(guess)*.
  "Happened with Yes" = fired and the last answer not No; "with No" = the last answer No.
- **Flags** are counters: `+X` sets `X1` or raises the digit, `-X` lowers it and removes it at
  0; `=X` holds for any digit (or an exact `=X2`). The Community "set a digit" patch is not in.
- **Conditions**: signed thresholds mean `≥ n` (positive) or `≤ |n|` (negative); squad count
  and army strength are checked when non-zero, level/gold/mana only with the "current stats"
  box; the level is compared 0-based as in the exe (a threshold of 2 means level 3). Owner
  code 6 is "not the player" (includes nobody). **Buildings** (exe 4a815d): a slot counts only
  with an owner code; 1 = the player's, 6 = not his, 2–5 = the building's faction is code − 1
  (1 the player's side … 4 enemy), whoever holds it, so code 2 also matches the player's own
  buildings (a captured building takes faction 1). **Units** (4a8237): each slot takes a unit
  of the player's army that no earlier slot took, so three alike need three units (РК2's
  mines want three peasants); "not the player's" holds while no such unit is left over; the
  match is the unit type when no named character is asked for, else that named character
  whatever his type (0xFF with no name: a unit an event added); other owners look in an army
  of that side. For units and artifacts owner code 0 is read as the player *(guess; no map
  uses it)*. Id lists behind a check box are ignored when the box is off. All listed ids must
  match.
- **Meeting** (exe 0x68dc7c): "meet army" holds only for the army being met right now: it is
  set when the hero meets an army (`EventEngine::meet`), and cleared when the run that followed
  ends (0x4ac39e) or by a "no meeting" result (0x4ab286), for the rest of that run too. A
  meeting event therefore fires once per meeting; before, a meeting counted for good, and
  every repeatable meeting event (РК3's "+Talk" on each lord, РК1's suzerain) fired again on
  every step after the first meeting.
- **Results order**: flags (`+X`/`-X`), world effects, quest to journal, quest completed,
  relative event, victory/defeat (ends the engine), then the chained event. "Move to hero"
  moves the army the removed units go to (else the one added units come from) *(guess)*.
- **Community extensions** (Community editor guide; `extension()` detects them). An event
  with "no meeting" and patrol value 1–20 runs that **opcode**: its XP, gold and mana fields
  are arguments (`x`, `g`, `m`), never resources, and its patrol change is not made. The
  squad-count, strength and stats conditions are skipped for opcode events (1–5 and 19 use
  them as arguments). Where an argument names a holder: 0 = the player's army, 1–255 = an
  army id, negative = building `−n`. A unit is its index in the army (0 = leader / hero);
  −1 = everyone.
  - **1–5, event editing**: first setting from (patrol value = action, `x` = shift to the
    target event, `g` = the field's byte offset in the event record, `m` = value); a second
    setting when the squad-count condition is 1–5 (it is the action; gold condition = shift,
    level = offset, holiness-and-mana = value). 1 adds, 2 sets (clamped to the field's
    range; `EventEngine::event_field` reads it); 3, 4, 5 are conditions: the event does not
    fire when the target's value is greater than / not equal to / less than the value. Only
    offsets where a field starts count; a missing event or unknown offset is ignored (a
    comparison with it passes *(guess)*). Edits are kept in the save and put back on load.
  - **6** the unit wears the event's four "items added" (0 = empty slot), whatever the
    rules; the player's old items go to the pack. AI units have no own items in Razdor: an
    army adds them to its items (its loot), a garrison takes none *(recorded, no battle
    effect)*.
  - **7** unit type `m` in slot `g` (the player's unit keeps level, XP, items, HP fraction;
    an AI troop its level).
  - **8** speed code `g`: 1 → +5 … 5 → +1, 6 → −1 … 8 → −3 *(guess: the guide gives only 1
    and 8)*; AI armies get the terrain factor of that correction. The hero's own speed is a
    recorded no-op (it lives in game.rs).
  - **9** group `g` (1 player … 4 enemy): the army or building takes that group's attitude
    towards the player from the scenario's relations; group 1 means attitude 3 and a
    building becomes the player's *(guess)*.
  - **10** relation `m` (−3..3) towards group `g`: only the relation to the player (0) is
    kept; others are a recorded no-op (no AI diplomacy yet).
  - **11** the "spells learned" list lasts for good on the holder, replacing its lasting
    spells; spells are per army in Razdor, so a single unit's list goes on its army
    *(guess)*; garrisons hold none.
  - **12** slot `g` becomes named character `m` (its unit type from the scenario's list).
  - **13** `m` XP to unit `g` (or every unit, the dead included): the player's units and
    AI troops bank it towards their levels as the exe does (experience.md §5).
  - **14** (condition) all listed spells last on the holder (order free).
  - **15** records the campaign branch (`x` = map number, `g` = variant); the event's chain
    usually fires the victory event. `Game::next_map()` then names the map: the scenario's
    next-map name with its leading `N-V` replaced (or `N-V` alone) *(guess: the guide's
    "N-V" naming)*, plus what the header's carry-over flags (0x110) keep: gold, mana for
    "gods' favour" *(guess)*, fame (flag only), hero level and XP with the spell book, the
    hero's worn items, the pack, the living army (paid as of the new start). Without a branch the scenario's next map is named as
    it is; `None` before a victory or with no next map. `Game::apply_carry_over` starts the
    next map's game with it (the hero's level, XP and book, else level 1 and the new map's
    book; his worn items; gold added, mana set, the pack; the army with its levels and XP).
    **The flags always carry over** (the exe stashes its flag string with the army, 4b5ef8,
    and puts it back on the next map, 4b5ff8): РК3's king rewards the band and the "swamp
    king" beaten in РК2 by their flags. The hero's class, name and unit type go too.
    `Game::from_campaign` starts the next map with all that **before its opening events run**
    (РК2 checks at once that the herald came along). The victory screen offers "Next map" when
    the named file is in the install.
  - **16** the listed spells leave the spell book.
  - **17** model `g` for army `x` (the hero's figure: no-op).
  - **18** flag `RAND` + one character, drawn from codes `x..=g` (cp1251); an existing
    `RAND?` flag is replaced. The guide's example gives 41 and 57 for RAND1–RAND9, which
    only works as 49–57 (the characters 1–9): codes are used as given.
  - **19** army `x` heads for cell (`g`, `m`): its post moves there and it walks there
    (then patrols around it) *(guess)*; condition: army (strength field) stands on cell
    (gold field, holiness-and-mana field).
  - **20** the hero moves to cell (`x`, `g`) (or the nearest passable one within 8), stops
    and looks around (the fog lifts there).
  - "No meeting" + a spell (no opcode): that lasting spell is lifted from the player's army
    instead of cast. "No meeting" + named squads: the class check the guide adds is what the
    named-unit condition already does for the player's squad (type and name must match);
    AI armies keep their named character per army, so their class is not checked.
  None of the 15 shipped maps uses any of them.

**In the game** (`src/rules/script.rs`: `Game` is the `EventWorld`; UI in `src/ui/story.rs`):

- **When it runs**: at the scenario's start; after every slice of game time (each 5-minute
  step of a walk, of a wait, a heal or resurrection, an event's delay); when the hero steps
  into a building or onto an event point; after a battle; after an answer; after a rumour.
  A message, question, quest notice or the end stops the walk (and a wait) so it is read;
  time does not pass while a dialog is open.
- **Place**: the building the hero stands in (its 1-based scenario id), else the event point
  on his cell.
- **Meetings**: meeting an army on the road (friendly greeting, a hostile army's attack, or
  the hero stepping onto it) runs the events with that meeting in force before the battle;
  the battle happens only if no event fired and the army is still on the map and hostile. A
  greeting stops the walk only when an event fired. A friendly army greets again once its
  talk counter, −500 after a greeting, has climbed above 0 (a repeatable meeting event fires
  once per greeting).
- **A building taken from its garrison is entered**: its events are checked right after the
  won battle, as when the hero walks in (the original opens the building's window, 4bbc84,
  which runs the scan). Before, РК2's mine forts asked for the peasants only on a second
  visit.
- **Queries** *(guesses)*: squad count = living units, the hero included; army strength =
  sum of the living units' tactical cost with items (the exe's army strength, experience.md §1); a building's owner is
  the player or not, plus its faction (a neutral building keeps its faction from the file);
  a named unit's holder is a side code (the player 1, factions 1–4 → 2–5); an army's named
  character is its `named_character`; "beaten by anyone" = beaten by
  the player or in an AI battle (§8.8; `Game::army_beaten_by_anyone`, the query in
  `script.rs` switches to it once the event-opcode work there is merged); an army waiting off the map is "at home", one on the map
  is at home within a cell of its home building's entry.
- **Effects**: gold is added as it is (a debt is settled at noon), mana never goes below 0;
  XP goes to the hero; an added unit (kind 3, no wage) takes a free cell of its row (none if the army is full) and, taken from an army, keeps its
  level there; "unit added by an event" and "any unit" remove the last such one to join,
  its items going to the pack; a unit sent to an army joins that army's troops; items go to
  the pack (from the pack, else from whoever wears it, when taken); spells go into the book
  (15 at most); activating brings a waiting army onto the map at its post (ships stay off);
  "move to hero" also activates it, next to the hero *(guess)*; the hero's new class keeps
  level, XP and items and loses the class bonuses; a battle an event starts is against the
  army as it stands after all the event's effects; a delay passes time (armies move, the
  noon report comes) with the hero standing still. A spell cast on the army takes effect at
  once and for free through the world-spell path (§8.3), lasting 10× its `TimeWork` (5× from
  8 hours). Lanterns and shown armies are recorded as reveals (x, y, radius; a lantern
  without a radius reveals 5 cells, an army 3 *(guess)*) for the fog of war.
- **Rumours** are heard for free and listed in the main hall with the building's quests; a
  rumour that costs something says so in its own event (gold condition and result), as in the
  original (economy.md: no fixed rumour price exists in the code).
- **Texts** are read from the scenario at runtime: the title without its flag script, the
  question (or the message when the question text is empty), the message. `#HERONAME` is
  the hero's class name (there is no name entry) *(guess)*; `#N`, `#G`, `#Ok` seen in some
  titles look like editor notes and are shown as they are. The picture byte shows that unit's
  portrait; an event's own picture is decoded as RGB565 *(guess)*. Rewards shown with the
  message are the event's gold, mana, XP, items and units.
- **The end**: once the victory or defeat event's window is closed, the victory or defeat
  screen follows, with that event's title.

### 8.2 Fog of war and minimap (Stage 5, `src/rules/fog.rs`, `src/ui/minimap.rs`)

`Fog` is a plain explored bitset (`w`, `h`, `enabled`, `bits: Vec<u64>`) kept in `Game::fog`.
Explored cells stay explored; there is no "seen before" state (the video).

- **Sight** (world.md §3): the hero explores a circle in cells around him: 9 cells for the
  knight, 8 for the archmage, 10 for the ranger (the original's 18/16/20 half-cells), by the
  original's half-cell stamp (241, 293 and 349 cells). On the 32×22 px screen that is an
  ellipse. He looks around at
  the start and after every step.
- **Lanterns**: points with model 8 and the "active at start" flag light their radius when the
  game starts (9 on the 15 shipped maps). A radius `r` (cells, capped at 24) explores the same
  kind of disc. `Game::reveal(x, y, r)` lights one later; `fog::lantern(scenario, point_id)`
  gives an event's lantern cell and radius.
- **Movement**: unexplored cells are impassable to the hero's planner and no target for a
  click (world.md §1.3, §3; the AI ignores the fog): a click into the dark does nothing.
- **Armies** move in the dark as before (the original AI ignores the fog; chases are not
  changed); they are only hidden, and so are their tooltips. Map objects and buildings with no
  explored cell within two cells of them are not drawn; nearer ones show faded through the
  soft edge.
- **Look**: unexplored cells are black; the edge is a feathered band about two cells wide on
  each side of the border (a box blur of the explored mask, eased), so a sliver of the dark
  side shows through as the original's soft ellipse.
- **Minimap**: toggled with the bottom-bar "Map (M)" button or M, in the top-right corner of the
  map view. The whole map scaled to at most 360 px, only explored cells in their terrain colour
  (blocked land darker), buildings as small icons coloured by side (player green, ally blue,
  neighbour yellow, enemy red, neutral grey, `fog::Side`), the hero as a blinking white dot and
  the view as a light rectangle. A click on it moves the camera there (as in the video); a click
  on the map to walk returns the camera to the hero.
- **Demo**: the built-in demo plays without fog.

### 8.3 World spells (Stage 7, `src/rules/magic.rs`, `src/ui/spellbook.rs`)

- **Where**: from the spell book on the world map (bottom-bar "Spells (B)" or B), never in
  battle. `Target=Hero` spells go on the hero's own army; `Enemy` on a whole hostile army,
  `OneEnemy` on its leader (first unit) only, within **3 cells** of the hero on explored
  ground *(guess: the original's range check is not decoded)*.
- **Cost and time** (economy.md §4): `CostMana div d` mana and `TimeCast × 2 div d`
  half-hour steps, d = 2 for the Archmage; otherwise a `Caster` in the living army makes both
  `floor(× 0.8)`. The two do not stack. No level scaling.
- **Casting passes time** in 5-minute slices like a wait: armies move, the noon report comes,
  the scenario's events run. The mana is taken when the spell completes: a hostile army
  reaching the hero interrupts the cast (the battle follows) and a target that left the range
  or the map loses it, both for free; not enough mana left then: no spell *(guess)*.
- **Duration**: exactly `TimeWork` hours from the moment the spell is ready (9999 is simply
  long); empty or 0 is instant. Casting a spell already on the army adds another `TimeWork`
  to what is left. A unit holds 4 lasting spells; with none free, nothing happens (effects
  are kept per army, with a leader-only flag, so the slots count per army and for the
  leader). Effects are dropped when time passes beyond their end.
- **Stats**: while a spell lasts, its `d-` values are added to each unit's stats (after items
  and potions), then each spell's `p-` compounds in turn. This happens when a battle starts
  (both sides: the hero's army and a hostile army under the hero's curses; leader-only
  spells on the first unit). A higher maximum HP raises the unit's HP by the same amount for
  that battle, a lower one caps it *(guess)*. AI-vs-AI battles ignore leader-only spells.
- **Instant hits**: `DeltaFixedHits` is added, then `DeltaPercentHits`% of the maximum (a
  gain) or of the current HP (a loss), on every living unit of the target (the leader only
  for `OneEnemy` and `p-LifeLose` spells). Healing is capped at the maximum and does not
  raise the dead. Wounds can kill: a unit of the own army becomes a corpse (the hero keeps
  1 HP *(guess)*); an enemy troop falls; wounds stay with the troop and do not heal on the
  map *(guess)*; an army with nobody left counts as beaten by the player, with no loot
  *(guess)*.
- **`p-LifeLose`** (scenario-only curses) holds the leader alone: a negative value is a
  lasting percent loss of maximum HP; a positive value lifts the life-draining curses from
  him *(guess: the curse and the spell lifting it come as a −20 / +20 pair)*.
- **Events and villages** that cast a spell on the army use the same path, for free and at
  once; an own-army (`Hero`) spell lasts `TimeWork × 10` hours, × 5 from 8 hours.
- **Potions** keep their Stage 4 rule: modifiers last until the end of the next battle.
- **Demo spells**: `data/spells.ini`, our own (a heal, two blessings, a fire bolt, a curse),
  taught at St. Beor's church and Greywall; the demo's Archmage starts with two
  (`StartSpells=`); demo villages give 10 mana a day as tribute.

### 8.4 Saves (Stage 7, `src/rules/save.rs`, `src/ui/saves.rs`)

- **Format**: the whole `Game` serialised with serde as JSON, compressed with bzip2 (a
  scenario game of РК1 is about 3 KB). The file has the meta (name, kind, scenario
  reference, scenario title, hero class, in-game date, real save time) and the game.
  `FORMAT_VERSION` guards the layout; other versions are refused.
- **What is not stored**: the content (units, items, spells: rebuilt from the demo data or
  the install), the map, the buildings' and armies' texts, the scenario's event list,
  places and victory/defeat events (`#[serde(skip)]`). The save names its scenario: the demo,
  or the map's file name plus a 64-bit FNV-1a hash of the file's bytes. Loading reads that
  file again from `RAZDOR_DT_DIR`; a missing install, a missing map or a map whose bytes
  changed is refused with a message. A game whose units, items or buildings no longer exist
  in the data is refused too.
- **Where**: `RAZDOR_SAVE_DIR` if set, else the platform data folder (`dirs::data_dir()`:
  `$XDG_DATA_HOME/razdor/saves` or `~/.local/share/razdor/saves` on Linux,
  `~/Library/Application Support/razdor/saves` on macOS, `%APPDATA%\razdor\saves` on
  Windows); manual saves in `manual/` (a save of the same name is replaced), autosaves in
  `auto/`. Never the repo or the game folder.
- **Autosaves** (as in the footage): before every battle ("Battle - <army or building>")
  and at every 12:00 report (named by the in-game date, "1204.06.03, 12 h"); the newest 10
  are kept, older ones deleted *(guess: the original's count is not known)*.
- **Loading** a save made before a battle starts that battle again; a question the scenario
  was asking is asked again. The windows of the moment (reports, story dialogs) are not
  saved.
- **Screens**: bottom-bar "Save" / "Load", the Esc menu (back, save, load, main menu) and a
  "Load a game" button on the title screen. The load window has the original's two tabs,
  saves and autosaves, newest first, with the scenario, the hero and the in-game date.

### 8.5 Sounds and music (`src/dt/sound.rs`, `src/ui/audio.rs`, `src/ui/jukebox.rs`)

- **Files**: `_Sounds.ini` `[Backgrounds]` (music) and `[SFX-Effects]` name files in
  `Sounds/`, matched ignoring case. `.wav` files are 8-bit mono PCM (22050 Hz, one at
  11025 Hz). `.raw` files are headerless signed 16-bit little-endian mono; their rate is not
  stored. **22050 Hz** *(guess: the ini's comment says all PCM is 22050 Hz, and the spectrum
  of the music rolls off just below 11 kHz, as a 22050 Hz recording would)*;
  `RAZDOR_MUSIC_RATE` overrides it. An odd trailing byte is dropped.
- **Music by screen** *(guess, the footage has no sound)*: `BkgMenuMain` on the title,
  scenario and class screens and the title's load window, looped; `BkgMap1..7` on the world
  map and every window over it, shuffled, all seven before any repeats and never the same
  twice in a row; `BkgBattle1/2` in battle, a random one first, then alternating;
  `BkgTriumph` once after a won battle (the map music follows) and once on the victory
  screen; `BkgDefeat` once on the defeat screen. A new battle cuts the triumph short.
  `BkgAuthors` is unused (no credits screen). Tracks follow each other by their length from
  the sample count; there is no crossfade.
- **Effects** *(guess where the ini's name leaves it open)*: `InterfaceButtonDown` on every
  button; `InterfacePanelDown` when a window (building, army, journal, spell book, menu,
  save, load) or a non-event dialog opens; `Global-Event-1..3` in turn for scenario event
  windows; `MainMenuPress` on picking a scenario or class; `Global-Battle` when a battle
  begins; `Battle-Fight` for melee and long strikes, `Battle-Shoot` for shots,
  `Battle-Strike` for shots of units with ranged attack ≥ `ShotWeaponRange` (cannon),
  `Battle-Cure`, `Battle-Bless`, `Battle-Sorcery` for curses and magic strikes; `Card-Move`
  for deployment moves and steps in battle; `Unit-Upgrade` for level-ups and promotions;
  `InterfaceCastSpell` then `Spell-Good` (own army) or `Spell-Evil` (enemy army) for a world
  spell; `Item-<Type>` when an item is bought, equipped or drunk; `Item-Gold` whenever gold
  goes up (loot, income, tribute, sales, events). `MainMenuSelect-*` (hover bells) and
  `InterfaceBarScroll` are not used yet.
- **Settings**: music 60%, effects 80% by default; steps of 10%; N mutes the music. Kept in
  `audio.json` in the save folder.

### 8.6 Ships (`src/rules/ships.rs`)

- **Water** (world.md §1): terrain codes 0–2. Shallows (2) and coastal water (1) are sailed
  and never walked; deep sea blocks ships too. The hero's sea is that water outside building
  footprints; AI ships sail the original's SHIP map (that water, plus every footprint as road).
- **Buying** (world.md §8): a friendly shipyard's "Ships" tab sells a ship for `ShipCost`
  gold. As in the original no ship appears: the hero's planner switches to the MIXED map
  (water at its cost, land 5×, footprints 6), so from the shipyard he routes onto the water
  next to it; stepping out onto the water puts him at sea, leaving on land loses the
  purchase. A new purchase loses the old ship (one at a time).
- **Sailing**: at sea every route is planned on MIXED. No speed of its own: a step at sea
  takes the cost of the cell left × the hero's speed (coastal 5 minutes, shallows 10, a
  footprint 30; the ranger 4, 8 and 24). Bridges close to him only when he clicks land or
  stands on a bridge, and a bridge is no target at sea. **Landing**: land or a building
  (not a bridge) ahead ends the route on it and the ship is parked on the water he left; a
  click on it (a target although water costs nothing on foot) takes him aboard again. The
  original's land test reads a cell further south (its bug, kept): he is sometimes stopped
  on open water, or steps ashore without parking the ship and loses it.
- **Start at sea**: a preset on the water ("Тихая пристань") starts the hero aboard a ship
  *(guess)*.
- **Fog**: the hero sees as far at sea as on land; routes need explored water as they need
  explored land.
- **Scenario ships**: an army placed on water (not on a bridge) is a ship army for good
  (ai.md §13); army byte 72 (1 hero ship, 2 pirates, 3 merchants) only picks its picture.
  Ships run the same AI as land armies on the SHIP map (land seeds are dropped there).
  Merchants' attitude to the hero is raised to at least 0 for his own encounters (РК4's
  merchant is marked −2 in its file) *(guess)*. Events bring waiting ships onto the water.
- **Saves**: the ship (cell, aboard) is part of the game state; the water mask is rebuilt
  from the map.
- **Reachability** (env-gated test): from every class's start, walking and renting a ship
  at every shipyard reached, every building of every shipped map is reachable except: the
  second tutorial's church (a ring of dense thickets and bog); the first tutorial's three
  villages (across a river crossed by fords, which are water now); РК3/РК5's altar (shut in
  by massif squares and a lake without a shipyard); РК7's eastern island (ringed by deep sea,
  which ships no longer sail). Scripted events are not considered.

### 8.7 Hero name

- The class screen takes a name (up to 24 characters, typed); `#HERONAME` in the
  scenario's texts becomes it, `#HEROCLASS` the class's name. Empty: the class's name.

### 8.8 AI armies (`src/rules/ai.rs`)

The AI follows original-mechanics/ai.md one to one, its slips included; the few points it
leaves open are listed at the end of its table.

- **Style**: army byte 59 (0 feudal, 1 rogue, 2 peasant; it equals the model byte 4/5/6 of
  every such army of the shipped maps, and model-7 armies carry it too), else the model.
  **Target model**: byte 85, the index into the `_Global.ini` priority lists; a missing key
  reads 0 (the shipped misspelt `MixHealingTarget` leaves the healing minimum at 0).
- **Who**: every scenario army, ships included (an army placed on water sails the SHIP map).
  The demo's gangs keep their simple rules (chase the hero, patrol).
- **Step clock**: armies bank the minutes of every hero step and wait tick (at most 200) and
  step when they cover `cost(cell left) × speed` (×1.5 diagonally); with no path an army
  steps in place on its own cell's cost. Every step is an arrival. Stationary guards do
  nothing at all; an army healing stands still for `HealingTime`.
- **Planning**: at an arrival, when its countdown of `AIGetPathDistance` steps ran out or any
  party is that near. Seeds: buildings by their score, armies and the hero by their cached
  battle score (hostile) or talk counter (friendly), four wander points; a danger (a
  negative score) pushes two repulsion cones onto a multiplier map instead; forbidden
  buildings, stationary guards and the armies near an ignored one are closed. One flood
  from all seeds in the original's pass order, the path read by steepest descent.
- **Scores**: an army's against another from a simulated battle (the battle engine on both
  sides; cached per pair, rescored within `AIDistance0..2` when dirty); a building's from its
  village, purchase, attack and garrison parts, rescored at noon, midnight and after battles.
  Lower is better, 0 no interest.
- **Arrival**: talk counters, attacks on hostile neighbours (tripled scores when they shelter
  in someone else's building), greetings (−500 both ways); in a building: assault and
  capture, village gold (feudal), shopping, healing and resurrection, hiring, garrison buying
  and reshuffle. AI armies keep their units' records: worn items, corpses (dropped after
  `MaxTimeResurection`), pay.
- **Noon** at the first arrival after 12:00 (its income, its castles' and forts' stock, its
  linked villages'; feudal wages, short gold leaving the cheapest unpaid), **midnight**: the
  village average and every building rescored.
- **AI battles**: one battle with the full engine, only the attacker's paid units; HP and
  deaths written back, XP with the AI's promotion rolls, the asymmetric loot of ai.md §10,
  worn items pooled and handed out by tactical gain.
- **Reports**: a battle or capture within the hero's sight (his 8–10 cells), or at a
  building of his, is an event (a message line) and goes into `Game::ai_log` (30 kept).
- **Beaten armies** leave the map; with a home and a respawn delay (byte 70, days) they
  come back after it at the centre of their home (a feudal one that lost it: its first town,
  castle or fort; none, never; a rogue takes over a village, shipyard, altar or ruins home),
  the whole record when the AI beat them, the leader alone (unless byte 83) when the player
  did.
- **Performance**: simulated battles with the same sides are played once (`SimCache`), and
  a flood that cannot change the path (an ignored army's own cell is closed) stops as soon
  as the path is known; the results are the original's.

## Appendix: `_Global.ini` `[GlobalOptions]` quick reference

| Key | Value | Use |
|---|---|---|
| WizardMainSpell | 7 | Elemental bless/curse initiative divisor. |
| BlessMainSpell / BlessNextSpell | 6 / 12 | Life and Death bless divisors. |
| CurseMainSpell / CurseNextSpell | 5 / 10 | Death curse divisors. Life curse uses ⅔·CurseMain and a fixed 10. |
| DecSpell* / MinSpell* | 2,2,5 / 15,0,15 | Per-turn magic power loss and floor (Life, Death, Elemental). |
| CrazyAI | 0 | Read but **unused**. |
| Row2Def | 5 | Back-row bonus vs shots. |
| BattleEndTurn | 25 | Turn limit. |
| HealingConst / HealingTime | 50% / 60 min | Heal cost and heal time. |
| ResurectConst / MaxTimeResurection | 300% / 10080 min | Resurrection cost and time window. |
| NeedUpkeepDay / MaxTimeNotUpkeep | 5 days / 10080 min | AI gold reserve / how long an army goes unpaid. |
| VictoryGoldDiv / MinVictoryGold | 2 / 25 | Gold loot. |
| CostRecrutDiv / CostMercenaryDiv | 2 / 2 | Wage divisors. |
| MaxDayCountForNewUnit | 10 | Barracks restock speed. |
| GarrisonAutoHeal | 10 | % HP healed per day in a garrison. |
| ShotWeaponRange | 60 | Threshold for cannon-type shooters. |
| MainExpCorrection / ExpCorrection | 30 / 50 | Feed a strength-ratio XP term the exe never uses (experience.md §3). |
| AIExpiriencePercent / HeroExpirienceModificator | 100 / 50 | XP scaling for AI-vs-AI battles / all the player's units. |
| ItemSaleCost | 25 | Item sell %. |
| `[Costs] ShipCost` | 250 | Ship rent. |
