# Discord Times data: ini files, maps, saves and options

This file covers how the Community Update (Unstable) `DiscordTimes.exe` (Delphi, image base
0x400000) reads its data: the six ini files and the rules of its ini reader, the loading of a
`.DTm` map as the code does it, the save and autosave files and their names, the options, the
map and save lists, and the main in-memory records at the level a reimplementation needs. The
rules are in our own words; addresses are virtual addresses in that build, given as evidence
only. The byte layout of the map file itself is in [../dtm-format.md](../dtm-format.md); this
file says what the game does with it.

Confidence tags:
- **code**: confirmed by reading the code.
- **data**: consistent with the data files, the texts or the footage, but not traced in code.
- **unknown**: not determined; Razdor keeps a documented guess.

## 0. Conventions

**Army slots.** Army records are 0x3827 bytes at `0x75a940 + k·0x3827`. Slot 0 is the hero,
slots 1..N are the map's armies in file order (N = army count), slot N+1 is the player's ship.
A garrison uses the same record, allocated per building. **code** (0x4b2504, 0x4b66d8)

**Cells.** The map has two grids. Terrain codes are bytes in a (W+2)×(H+2) array with a copied
one-cell border, so cell (x, y) is at `(y+1)·(W+2) + x+1`. The 18-byte cell records are in a
(W+8)×(H+2) array **without** that offset: cell (x, y) is at `y·(W+8) + x`. **code** (0x4b25a2,
0x4b2606)

**Time.** The game clock counts **centi-minutes** since the scenario start (0x68dcb8); the
absolute minute is `clock div 100 + start`, with `start = header 0x38 + 1`. So a scenario starts
one minute after its header time. **code** (0x4b42e0)

**Rounding.** Integer `div` truncates toward zero. `Round` (0x402dd0) rounds halves to even.

## 1. The ini reader

All six ini files go through one reader (create 0x471fb4, read line 0x473a6c, section 0x472390,
key 0x472440, integer 0x47274c, list field 0x472548 / 0x472874, write 0x472a74, save 0x472b94).
Its rules differ from the Windows ini API in several ways. **code**

1. **Lines.** A line ends at a carriage return; the byte after it is skipped without looking at
   it (it is assumed to be the line feed). Every other byte below 0x0D is dropped wherever it
   is (tab and line feed included), so tabs disappear and a file with bare line feeds reads as one single line. Trailing spaces
   are cut. No line is ever skipped: comments and blank lines are kept (the comment filter is
   guarded by a flag at 0x4ecd00 that is 0 and never set).
2. **Sections.** A line is a section header when it contains `[` and, after it, `]`, anywhere
   in the line (not only at its start). The name is the text between them. A section runs to
   the line before the next header.
3. **Selecting a section by name** compares every header exactly (case-sensitive) and keeps the
   **last** match. If no section matches, the previously selected section stays selected, so
   the following reads come from the wrong section.
4. **Reading a key.** Within the current section, every line with an `=` is split at the first
   `=`; the part before it must equal the key exactly: case-sensitive, and **not trimmed**
   (`Cost =5` is the key `Cost ` and does not match `Cost`). The **last** matching line wins.
   The value is everything after the `=`, untrimmed. A missing key reads as the empty string.
5. **Integers.** All spaces are removed; then every decimal digit is taken in order and **every
   other character is skipped** (`12a3` reads 123, `1.5` reads 15, `3 // note 7` reads 37). If
   the original value contains a `-` anywhere, the result is negated. Empty or missing reads 0.
   Only the first (length mod 256) characters are scanned; overflow wraps at 32 bits.
6. **List fields** (`a,b,c`). Field 1 is the text up to the first comma, field n the text after
   the (n−1)-th comma up to the next one. A field past the end reads as empty, so as 0.
7. **Writing** replaces the value of an existing `key=` line in the current section; a key that
   is not in the file is **not added**. Saving rewrites the whole file from the stored lines with
   CRLF endings (comments survive; tabs, trailing spaces and stray control bytes do not).

What follows from the rules for data: a key name is matched exactly as the game spells it,
including its typos (`MixHealingTarget` in `_Global.ini` never matches the `MinHealingTarget`
the code asks for, so that priority is 0 for every AI model; `DecSpellelemental` is spelled
the same way in the file and the code, so it works). **code** (0x4e4678)

## 2. Start-up order

`entry` (0x4e7424) opens `_Sounds.ini`, `_Global.ini`, `RUS_DiscordTimes.ini` (texts and
options), `RUS_Units.ini`, `RUS_Artefacts.ini` and `RUS_Spells.ini`, from the game folder, and
reads the shared texts and `[GlobalOptions]` (0x4e36cc, continued through 0x4e4501..0x4e49de).
The splash screen (0x4e5c9c) then loads, in order: the colour table, terrain and object
graphics, the interface, **units** (0x4e0448), backgrounds, the map and save lists with their
caches (0x4e15cc, 0x4e21d8, 0x4e2970), the grey table and the **options** (0x4b8bb4). Artefacts
(0x4e0ab8), spells (0x4e0c60) and battle effects (0x4e1148) are loaded once, on the first map
or save load. **code**

## 3. `RUS_Units.ini` (0x4e0448, 0x48efd0)

**Which sections are units.** A section is a unit when its `Name`, `StartExpirience` and `Cost`
are all non-empty; `GlobalIndex` is not part of the test. The table has one 0xCE-byte record
per section, zeroed. **code**

**Two passes, two indexes.** Pass 1 stores the unit at record `GlobalIndex − 1` (a missing
`GlobalIndex` writes before the table). For each qualifying section it also takes the **next**
portrait from `Graphics\Objects\Persones.ugs` (u16 width, u16 height, pixels) and the **next**
92×92 icon from `Icons.ugs`: pictures are matched by the order of the unit sections in the
file, not by `GlobalIndex`. Pass 2 reads the upgrades into the record of the **section's
position** (counting all sections), not of its `GlobalIndex`. The shipped file has
`GlobalIndex` = position for all 102 sections, so the three agree there. **code**, **data**

**Keys** (record offsets in the notes): `Name`, `Descript` (stored with six leading spaces),
`Cost`, `CostMultipler`, `CostGoldDiv` (a byte), `Bonus`, `Surrender` (a byte),
`StartExpirience`, `LevelMultipler`, `Nature`, `Magic`, `MagicDirection`, `Hits`,
`AttackBlow`, `DefenceBlow`, `AttackShot`, `DefenceShot`, `MagicPower`, `ProtectLife`,
`ProtectDeath`, `ProtectElemental`, `Regen`, `Vampirizm`, `Initiative`, `Manevres`, the
per-level `d-` keys for those thirteen stats, and `NextUnit1..3` / `NextUnit1Level..3Level`.
`IconIndex` is never read. **code**

**Enumerations** (exact, case-sensitive; anything else gives 0):
- `Nature`: Undead 1, Elemental 2, Rogue 3, Animal 4, Hero 5.
- `Magic`: LifeMagic 1, ElementalMagic 2, DeathMagic 3.
- `MagicDirection`: ToEnemy 1, ToAlly 2. `ToAll` (used by 23 shipped units) is not a name the
  exe knows: it reads as 0, the value the battle code treats as "all".
- `Bonus` (one per unit), 1..52: SpearDefense, HorseAtack, ArmorIgnore, ArmyMedic, Merchant,
  DeathCurse, GodAnger, GodStrike, Unvulnerabe, VampirsGist, OldVampirsGist, Evasive, Ghost,
  Artillery, Garrison, AddPayment, Poison, Dead, FastDead, Counterblow, FlankStrike (1–21,
  vanilla), then the Community names Hunger, Berserk, Exhaustion, Drying, CtrPoison, Suicide,
  Caster, Splash, Fortify, Dominate, PoisonS, Concentration, Potent, Stun, FirstShot, Bastion,
  Flying, Bleed, PreventiveStrike, Flock, ArmorBreaker, NoHeal, FasterAttack,
  PoisonArmorIgnore, HoldLine, Neutralize, KillingStrike, BloodThrist, Assault, EternalGift,
  FateGift (22–52). **code** (0x48efd0, 0xc25126, 0xc255a1, 0xc25af7, 0xc27d94, 0xc28e85)

**Attack role** (record +0xC4), computed at load: 4 (melee) if `AB > AS` and `AB > MP`; then 7
(shooter) if `AS > AB` and `AS > MP`; then 0x11 (caster) if `AB/2 < MP` and `AS/2 < MP`
(floating point); a later test overrides an earlier one; none gives 0. **code** (0x48f5d4)

**Community keys** (absent from the shipped file, so every unit takes the default):
- `ManaDrain` is read only when `MagicPower ≠ 0`; 0 or missing takes the school default
  `DecSpellLife` / `DecSpellelemental` / `DecSpellDeath` of `_Global.ini`. **code** (0xc283dc)
- `ManaDrain`'s school default is taken only when the unit has a school (`Magic` ≠ 0); a caster
  without a school keeps the zero it starts with. **code** (0xc2840c)
- `MinMagicPower` is likewise read only when `MagicPower ≠ 0` (a unit without magic power skips
  both keys). 0 or missing takes `MinSpellLife` / `MinSpellElemental` / `MinSpellDeath`;
  when that default is used for a Death caster of Nature Undead, it gets +25. **code** (0xc283dc,
  0xc284e6, 0xc28579)
- `Evasion` is stored per type as a byte (values above 255 keep their low byte). **code** (0xc2a7b5)
- Tripwire: the fourth unit whose `LevelMultipler` is 200 or 160 makes the game jump to address
  0 (a crash). All shipped units use 140. **code** (0xc26c4c)

**Upgrade slots.** `NextUnitN` is matched by exact name against every unit's `Name` (last match
wins) and stored as a 1-based type. Then the three slots are normalised (A, B, C = slots 1–3):
a lone option in A or C moves to B; options in A+B become A+C; options in B+C become A+C. In
memory, one option is always in the middle and two options always on the sides. **code**
(0x4e080b)

An empty or missing `NextUnitN` is compared too: it equals the (empty) name of any record that
was left unfilled (a section that is not a unit, or a hole in the `GlobalIndex` numbering), so
in such a file the slot points at that empty record. The shipped file has no such record, so
there it stays empty. **code** (0x4e0448)

## 4. `RUS_Artefacts.ini` (0x4e0ab8, 0x498eb4)

- The record is `GlobalIndex − 1`. The next 53×53 icon of `Graphics\Objects\Items.ugs` is
  consumed for **every** section, item or not. **code**
- An item needs non-empty `Icon`, `Name`, `Type` and `Cost`; otherwise its record gets only the
  raw icon pictures and its index, nothing else (the icon is consumed either way). The `Icon`
  key is only tested for being non-empty; the picture itself always comes from `Items.ugs`. Each filled item adds 1 to the item count (0x4ed470).
  **code**
- `Type`: BlowWeapon 0, ShotWeapon 1, Armor 2, Helm 3, Shield 4, Staff 5, Amulet 6, Ring 7,
  Potion 8, Item 9; **an unknown type is a Potion (8)**. **code** (0x4990b6)
- `Bonus` takes the 52 names above. `Magic` gives the item's school. `f-` (replace), `d-` (add)
  and `p-` (percent) keys for the stats are read (meanings in economy.md §5). **code**
- The highest `Cost` of all items is kept (0x4ed474); it caps market price windows (§10.6).
- Tripwires: an item whose `Cost` is 32000, or whose `d-Manevres` is 4, hangs the game in an
  endless loop. **code** (0xc26c18, 0xc26c08)

## 5. `RUS_Spells.ini` (0x4e0c60, 0x49ac64)

- On load, `[MapEditorSpecialOptions] Generated` is read. When it is 0 the game writes
  `Generated=1` back into the file (and saves it) and writes the editor's 22×22 spell icons into
  `Graphics\Editor`; Razdor never writes the install. **code**
- The spell record is the **section position** (all sections counted); a spell needs non-empty
  `Name`, `Type` and `CostGold`. In the shipped file sections 0–32 are the 33 spells. **code**
- Keys: `Icon1..3` with `ColorC1..3` (the picture), `CostGold`, `CostMana`, `Type` (Life 1,
  Death 3, anything else Elemental 2), `TimeWork`, `TimeCast`, `Target` (Enemy 0, OneEnemy −1,
  anything else 1), `DeltaFixedHits`, `DeltaPercentHits`, `p-LifeLose`, the `d-` and `p-` stat
  keys, and `Effect1..3` (list: sprite file, R, G, B, start, a sixth value, scale ×1000,
  duration; an empty sprite name means no effect). Semantics: economy.md §4. **code**

## 6. `_Global.ini`

- `[GlobalOptions]`: every key and its target is listed in the notes; the readers are in
  battle.md, economy.md and experience.md. Rules that come from loading: the Knight damage
  factor is set to 80 by the code, not read; `HealingConst` is kept as `100 / value` and
  `ResurectConst` as `value / 100` (doubles); the five AI behaviour models each read 14 priorities
  from list fields 1..5 of the same keys. **code** (0x4e4501..0x4e49de)
- `[BattleEffects] Effect0..6` (list: file, R, G, B offsets, alpha offset, a sixth value):
  effects 0–5 have 25 frames, effect 6 has 50, of 220×110. A colour channel is `nibble × 17 +
  offset` clamped to 0..255; alpha the same, raised to at least 8 when any colour nibble is
  non-zero, then stored divided by 8. **code** (0x4e1148)
- `[Costs] ShipCost` is read when the hero enters a shipyard. **code** (0x4bbc84)

## 7. `_Sounds.ini` (0x4e2e80, 0x4e2d38)

`[Backgrounds]`: BkgMenuMain, BkgAuthors, BkgMap1..7, BkgBattle1, BkgBattle2, BkgTriumph,
BkgDefeat (music). `[SFX-Effects]`: InterfaceButtonDown, InterfacePanelDown,
InterfaceCastSpell, InterfaceBarScroll, MainMenuSelect-1, MainMenuPress, Global-Event-1..3,
Global-Battle, Unit-Upgrade, Spell-Good, Spell-Evil, Battle-Fight, Battle-Shoot, Battle-Cure,
Battle-Bless, Battle-Strike, Battle-Sorcery, Card-Move and the eleven Item-* keys.
`MainMenuSelect-2` and `-3` are never read. A value is a file under `Sounds\`; a name containing
`raw` is headerless PCM (music streamed), any other name a wave file. Nothing is loaded when the
"no sound" option is on. **code**

## 8. `RUS_DiscordTimes.ini`: texts and options

**Texts.** At start the game reads `[Tutorial]` (`Completed` = 1 means done; `Tutorial_MapName`),
and the shared labels of `[Buttons]`, `[SaveGame]` (`AutoSaveBattle`, the autosave prefix),
`[Time]`, `[GameMenu]`, the hint section, the army bonus texts (`Bonus1..58`, `Hint1..4`),
`[NewHero]` (`Bonus1..3`) and `[Skills]`. Every window reads its own section when it is built.
The tutorial counts as done when `Completed=1` **or** when any save file exists. When an event
whose title contains `end_tutorial` fires, the game writes `Completed=1`. **code** (0x4e36cc,
0x4e2970, 0x4ac9fc)

**`[Options]`** (read 0x4b8974, 0x4b8bb4, 0x4b8c4f; written 0x4bf42c, 0x4bf4d8): **code**

| key | effect |
|---|---|
| OptValue4 | 1 = load no music and sounds (needs a restart) |
| OptValue5 | 1 = show the frame rate |
| OptValue6 | 1 = hover hints off |
| OptValue7 | 1 = follow the selected army automatically |
| OptValue8 | 1 = autosave (§14) |
| OptValue9 | 1 = improved enemy AI in battle |
| OptValue10 | 1 = "impossible" difficulty: factor F = 100 on the player's income from towns, castles and forts (×F/100); 0 → F = 120 |
| OptValue11 | 1 = wide front row: 6 units per row, else 4 |
| MusicVolume, SoundVolume | 0..100, as fractions value/100 |
| ScrollSpeed | s = value/100; scroll factor `(1 − s)·1.5 + 0.5` |
| WalkSpeed | w; step delay `(100 − w)·250 div 100 + 150` ms, half of it kept too |
| AnimationSpeed | Community: battle animation speed, percent |
| ColorMarkPlayer … ColorHarbor | Community: 11 minimap colours |

The OK button writes the five sliders (each `round(slider × 100)`) and OptValue4..11 (1 or 0)
and rewrites the whole file; values whose keys are missing from the file are lost (§1, rule 7).
It then re-reads the flags, so the row width changes at once in memory. **code**

## 9. The stream container (maps, saves, caches)

All binary files go through one stream object (open 0x473188, flush 0x4736b0). **code**

- **Header**, 12 bytes: 4 magic bytes, u16, byte, byte, u32 uncompressed size. The magic is
  accepted when bytes 0, 2, 3 are `A`, `p`, `f`.
- **Legacy layout** (byte 1 = `I`, the maps' `AIpf`): bytes 4–5 are ignored; byte 6 is the
  compression code (a value above 10 means bzip2 level `value − 10`; the maps have 19 = level
  9); byte 7 is a scramble mode; the rest of the file is one bzip2 stream.
- **New layout** (any other byte 1; the game writes `AEpf`): u16 = block size in KiB; byte 6 =
  level + 0x10; byte 7 = 0. With block size 0 one chunk follows; else chunks of that many bytes.
  A chunk is a u32 compressed length and a bzip2 stream. The writer emits `size div block + 1`
  chunks, so a size that is an exact multiple of the block ends with an empty chunk. Exception:
  a size of exactly one block is written as a single chunk, while the reader expects two and
  tries to read a second chunk past the end of the file (the effect was not tested; a save of
  exactly 64 KiB is practically impossible). **code** (0x4736b0, 0x473188)
- **Scramble** (byte 7 / mode flag): mode 1 XORs byte i with `(i + 1) mod 256`, mode 2 with
  `Random(256)`. Nothing the game ships or writes is scrambled.
- A file without the magic is read raw (that is how the ini files are read). A size mismatch
  after decompression sets an error flag that no loader checks.
- Strings in all three file kinds are NUL-terminated (read up to a NUL or the end of the data).

Saves use bzip2 level 1 and 64 KiB blocks; the list caches use one chunk. **code** (0x4b66d8,
0x4e1b98, 0x4e1d94)

## 10. Loading a `.DTm` map (0x4b2504)

### 10.1 Order of operations — code
1. The old map is freed (0x4b2204). The 0x12F-byte header is read. If header byte 9 is below
   `'4'`, the function returns **with nothing loaded**.
2. Header byte 0x121 is overwritten with the current wide-row option and sets the row width.
3. Section sizes come from the header; record counts are `size div record size`. The events get
   room for 4 spare records.
4. Terrain, objects, buildings, armies, points and events are read in that order by the header
   sizes. Then the stream **seeks to the header's text offset (0x18)** (the text marker is
   never read or checked) and reads the strings.
5. Strings: title as is; description, and all three strings of every building, army and event,
   with **every run of two or more spaces collapsed to one**; in the third string of buildings
   and armies `#HERONAME` is replaced by the hero's name at this moment; then the named
   characters (count = header 0xEE; at most 32 fit in memory). Event messages keep their
   escapes for display time.
6. The scenario picture (header 0x11C bytes) is **skipped**, then each event whose picture size
   (event +163, a full 32-bit value) is positive gets that many bytes.
7. The terrain run-length data is expanded (§10.2), objects and buildings are put into the cells
   (§10.3), the three cost maps and the minimap are built (world.md §1), the fog and explored
   maps are created empty.
8. **All army records are cleared.** The clock starts (§0); the autosave flag is cleared.
9. The hero (§10.4), the map's armies (§10.5), the ship slot, the buildings (§10.6), then every
   army's units are set to full health and recomputed and their wages computed. Every army,
   the hero's (record 0) included, and every garrison is then put through a battle side and
   back (0x49855c with all living units, then 0x4988c0): the side is **auto-arranged**
   (battle.md, 483b3c) and its grid becomes the army's formation. So the hero's starting army
   stands as the auto-arrange puts it, not where AddUnit put it (reserve first). His building
   defence is still 0 here (he is put on his cell after the loop, 0x497c68). A campaign map
   that carries the army over copies the old army's units and formation back after the load
   (0x4b5b64).
10. The AI buffers are cleared and the AI initialised; the hero's gold and mana are set; lit
    lanterns reveal the fog (§10.7).

### 10.2 Terrain — code
The data is (value, run − 1) byte pairs; the pair count is `size div 2`. Writing starts at the
inner cell (0, 0) and moves right; after the last inner column of a row it jumps to the first
inner column of the next row. There is no bounds check. Then the border is copied: the top row
from row 0, the bottom row from row H−1, then for every row (borders included) the left column
from column 0 and the right column from column W−1.

### 10.3 Objects and buildings — code
- An object's word is `class·256 + sprite`. Classes 1–8 go into the cell's first layer, class 0
  and classes 9 and up into the second. One object per layer per cell: a later object in the file
  replaces an earlier one.
- A building's file position is the bottom-right cell of its footprint. The loader converts it
  to the top-left (`x − sx + 1`, `y − sy + 1`) and stores that. The **anchor cell is the
  bottom-right (file) cell**: every footprint cell (one extra row above when `sx > sy`) points to
  it, and it holds the building id.
- A point's cell holds the point id.

### 10.4 The hero — code
- By class (0 knight, 1 archmage, 2 ranger): sight 18 / 16 / 20 half-cells, speed 5 / 5 / 4,
  world-spell time divisor 1 / 2 / 1.
- The hero's unit record is the class template prepared by the new-game screen; his army is on
  the map, faction 1, with attitudes from row 0 of the header's relation matrix. The next noon is
  `(start div 1440 + 1)·1440 + 720`.
- From the class's preset (dtm-format.md §3): the start building (byte 16) is given to the
  player (owner = player, faction and attitudes copied from the hero); the six troop triples are
  added, **skipping unit ids 1–3** (the hero types); the three items (bytes 41–43) go into the
  pack; the six spells (bytes 44–49) are learned; gold is set from the low 16 bits of bytes 8–11
  (signed), mana likewise from bytes 12–15; the hero stands exactly at bytes 37/39.
- A class is offered on the new-game screen only when its preset's start x or start y is
  non-zero (0x4c1804); the other bytes of the preset do not matter. The hero window opens on
  the first offered class (knight, archmage, ranger order), and with none offered it does not
  open. A class that is not offered cannot be picked at all: its portrait is disabled and the
  hit test (0x4743c8) skips it, and the window has no keys for the class (interface.md §5).
  Checked in the running game on Устье Трейна, the one shipped map with a class left out (the
  archmage; his preset still names start building 26 and a troop).
- **The one way round it is a campaign.** Loading the next campaign map (0x4b5b64) keeps the
  class (`0x68dccc`) and runs the load above with it, without looking at whether that map
  offers the class. With an empty preset the hero then stands exactly at its cell (0,0) (no
  search for a free cell), the preset's start building (byte 16) is still given if it is not
  0, gold and mana are set from the empty preset (0) and its troops, items and spells add
  nothing, then the carry-over flags bring back gold, mana and the army as usual (§15); the
  buildings flagged for the class (bytes 353–355) are given and the attitudes come from row 0
  as always. No shipped campaign leaves a class out of a later map, so this needs a
  user-made campaign; not checked in the running game.

### 10.5 Armies — code
For each 89-byte record, in file order, into slot k = index + 1 (offsets are dtm-format.md §7):
- **Leader** (byte 26 ≠ 0): added at level byte 27 with the named character of byte 58. His
  three items (bytes 50–52) are first checked: an id above `item count + 1` becomes 0.
- **Troops**: each triple adds `count` units of `unit` at `level`, **skipping unit ids 1–3**.
- **Items** 50–52 are then given to the army: each goes to the unit that gains most from wearing
  it, or into the 12-slot pack.
- **Speed** `max(1, 5 − byte 13 − (1 if the leader byte 26 is 2))`, byte 13 read as a signed
  byte (a value of 200 counts as −56).
- **Position** exactly at bytes 0/2 (no search for a free or passable cell). The army is **at
  sea** when its cell is water (terrain 0–2) and not a bridge; its ship look comes from byte 72.
- **On the map** unless byte 63 is set.
- Gold = signed 16 bits of bytes 17–18; base daily income = byte 80 × 10; respawn delay =
  byte 70 days; patrol box = position ± byte 61, clamped to the map.
- Byte 84 ≠ 0: every unit gets that spell for good (end time 1 036 800 000 minutes).
- A random think delay of `Random(3000)` (0..2999) is drawn per army (0x4832fc).
- Bytes 4–12, 15–16, 21–24, 46–49, 53–57, 73–75 and 86–88 are not read.

### 10.6 Buildings — code
- Every building flagged for the chosen class (bytes 353–355) is given to the player like the
  preset's start building. Bridges are not excluded.
- The owner byte (292) is taken as it is: **0 means the player**, 0xFF nobody, k the army k.
  (In the shipped maps owner 0 occurs on bridges and on several buildings of the last campaign
  map.) **code**, **data**
- **Garrisons** exist for towns, castles, forts and ruins (types 1, 3, 4, 12): the six triples
  (skipping unit ids 1–3), the extra defence of byte 332.
- **Markets** (types 1, 6, 7): the sign of each of the first 12 artefact words is flipped, so a
  positive item id in the file becomes a fixed good (a negative one would become a non-fixed
  one), and the restock flag is set when any word ends up negative; when the random
  goods count (byte 295) is non-zero, the price window is fixed up (maximum capped by the highest
  item cost; a minimum not below the maximum becomes 0; a maximum of 0 becomes the highest item
  cost) and the stock is rolled at once (economy.md §2).
- **Villages** (type 2) start with one day's gold and one day's mana in stock.
- **Ruins** (type 12): no market; the garrison's gold is the maximum price (bytes 335–336); the
  first 5 artefact words go into the garrison's pack when it has no units, else onto its units.
- All other types have their artefact words cleared.

### 10.7 Points — code
A point with its "active at start" byte (40) and a radius (38) reveals the fog around it with
`radius × 2` half-cells. The model byte (8 lantern / 9 event point) is not checked here.

## 11. The new-game map list (0x4e15cc, 0x4e21d8)

- All `Maps_Rus\*.DTm` files are listed; maps whose header byte 9 is below `'4'` are rejected.
  The list reader also seeks to the text offset and reads title, description (a fixed placeholder
  when empty), campaign name and next map, skips the other strings and keeps the scenario
  picture. **code**
- Order: sort key 0 for the tutorial map (its file name contains `Tutorial_MapName`), else the
  map width, plus 5000 for later campaign maps (kind 2); ascending selection sort. **code**
- Shown: the tutorial, standalone maps by title, and for each first campaign map the campaign
  name followed by the chain of titles reached through the next-map names (exact file-name
  match). Later campaign maps are not listed on their own. A cycle of next-map names would hang
  the list. **code**
- `dtmaps.cache.tmp` and `dtsaves.cache.tmp` in the game folder cache the parsed headers and
  strings; an entry is reused while the file's size and modification time are unchanged. A
  reimplementation can skip them. **code**

## 12. Saving (0x4b66d8, 0x4c0b68)

### 12.1 Files and slots — code
- Saves live in `Saves\` with the extension `.sav`. The save window has **12 slots** for manual
  saves; autosaves are at most 12.
- **Manual file name** = the save's display name with every character replaced by `_` unless it
  is a digit, a Latin letter, a cp1251 letter 0xC0–0xFF, a space, or one of
  `! # $ & ' ( ) , - . = @ [ ] ^ _ ~` (so the letters 0xA8 / 0xB8 become `_` too). If that file
  exists, `[1]`, `[2]`, … is inserted before `.sav` until the name is free. Saving over an
  existing slot deletes its old file first. A save can also be deleted by hand from the load
  window (§13.2).
- **Autosave file name** = `dt_autosave_<n>.sav`, n the autosave's position (1..12) when it was
  created; overwriting an autosave deletes the old file and reuses its name.
- Each slot keeps: file name, display name, map title, real date text, file size and time, a sort
  key and the kind (1 manual, 2 auto). Lists are sorted **newest first** by the sort key, a
  pseudo-second count with 372-day years and 31-day months:
  `(year − 1970)·32 140 800 + month·2 678 400 + day·86 400 + h·3600 + m·60 + s` (local time).
- The real date text is `year, month, day, hour:minutes` with the `[Time]` words and the minutes
  padded to two digits (0x49c94c).
- The in-memory buffer is 1, 2 or 4 MiB by map width 50, 100 or 200.

### 12.2 Content — code
The file is the container of §9 around this sequence (the reader mirrors the writer):

1. Display name, map title, real date (strings; the loader skips them).
2. The map header (0x12F bytes), with byte 0x121 = the wide-row option and byte 0x12E = the kind.
3. The minimap-shown byte, written three times.
4. Point under the hero, current dialog event, current building, hero class, hero sight, hero
   speed (32-bit each).
5. Four clock values **negated**: game clock, last processed time, tick base, route start.
6. Pack (256 items, count, scroll), spell book (256 spells, count, page), gold, mana, the shown
   income and wage, the camera x/y.
7. Six 16-bit images (each written as width, height, then the image with its own width and
   height again): the LAND, MIXED and SHIP cost maps, the fog map (2W+2 × 2H+2), the explored
   map (W × H), the minimap (W+4 × H+4).
8. Terrain array (with its two dimensions), cell array (with its two dimensions).
9. Maximum scroll x/y; counts of buildings, armies, points, events.
10. Each building record, followed by its garrison army record when it has one.
11. The point records.
12. Each event record, followed by its picture bytes when it has one.
13. The quest journal: count, then 5-byte entries (event id, a byte).
14. Strings: title, description, three per building, army and event, the named characters.
15. Two 256-entry tables of cell indices per army slot.
16. The army records of slots 0..N.
17. Strings: hero name, campaign name, next map, the event flag string.
18. The ship's army record (slot N+1).
19. The restart snapshot (§15): flags, hero army record, hero class, pack, book, their counts,
    gold, mana, the map file.
20. The shipyard the hero arrived at by sea (−1 none).

Gold is stored twice (item 6 and inside the hero's army record); the army record holds the
gold's integrity check too (§15).

### 12.3 The save window (0x4c0dd0, 0x4c0a80, 0x4c0b68) — code
- The window shows 12 fixed rows, one per manual-save slot; each row is an editable name field.
  An empty slot shows the "new save" placeholder in grey; a used slot shows its display name.
- **One click on a row selects that slot and makes its name editable.** The previously selected
  row gets its normal label back. If the clicked row still shows the placeholder, the field is
  emptied, so a new save starts with a blank name (there is no suggested name); a used slot
  keeps its name, ready to be edited. The row is redrawn highlighted. (0x4c0dd0)
- On the same click the info column next to the rows is rebuilt: the selected slot previews the
  current map title and the real date of the computer (the same date text as §12.1, after a
  short label), the other slots show their saved map title and date. (0x4c0dd0, 0x49c94c)
- A click never writes anything. **Save** writes the selected slot: with no slot selected it
  does nothing; with an empty name it just resets the window (nothing is saved); otherwise it
  stores name, map title and date and writes the file (§12.1). (0x4c0b68)
- **There is no overwrite confirmation.** Saving into a used slot replaces it at once (its old
  file is deleted first). (0x4c0b68, 0x4b66d8)
- Cancel and the close button close the window (in the main menu they also redraw the screen;
  in a game they go through the side-window switch). (0x4c0a44)
- Whether Enter in a name field saves is **unknown**; the window builder sets no key handler on
  the rows, so probably not.

## 13. Loading a save (0x4b771c, 0x4c01fc)

- The old map is freed first; on the first load in a session the artefacts, spells and battle
  effects are loaded. **code**
- Header byte 0x121 sets the wide-row option and the row width **for the session** (not written
  to the ini). The world-spell divisor is recomputed from the class. The start minute is
  recomputed from the header. **code**
- Clock values: a value ≤ 0 is the new format (negate it); a positive value is the old format
  in absolute minutes and becomes `(value − start)·100`. **code**
- The maximum scroll is recomputed (the saved one is ignored). In every cell the second
  transient mark byte is cleared, and the first one only when its low four bits are 1. In every
  building all 64 artefact words whose absolute value exceeds the item count become 0.
  **code**
- Items 17–20 of §12.2 are each read only if the file has more data (older saves): a missing hero
  name gets a fixed placeholder text, missing strings stay empty, the ship slot stays empty, the
  shipyard is −1. **code**
- Afterwards the runtime is rebuilt: path and planner state cleared, the AI re-initialised at
  the loaded time, every AI army gets a new think delay `Random(3000)`, armies redrawn, the
  hero's ship restored when he is at sea. The game always resumes on the world map: a battle that
  was running when the autosave was written is not restored. **code**

### 13.1 The load window (0x4c08e0, 0x4c01fc, 0x4c0018, 0x4c0090) — code
- Two tabs (manual saves, autosaves), each listing at most 12 rows, newest first; a row has the
  real date on the left and the display name with the map title on the right. There is no
  scroll bar, and loading or deleting only accepts the first 12 rows.
- **One click on a row only selects it**; it never loads. Selecting a save highlights it and
  shows two small signs on that row: **Load** and **Delete**. Clicking an empty row clears the
  selection and hides both signs. (0x4c08e0)
- A save is loaded by the **Load** button or the Load sign on the row (both the same handler);
  with nothing valid selected they do nothing. (0x4c01fc)
- Hovering the Delete or Load sign shows its hint text, only when the hint option sends hints to
  the window. (0x4c0018, 0x4c0090)

### 13.2 Deleting a save (0x4c04ac, 0x4bfe8c, 0x4bfde0) — code
- The Delete sign opens a Yes/No question centred over the load window, with a title and a
  text naming the save (the save's display name is put into the text's placeholder). In a game
  (not during a battle) the panel buttons are disabled while it is open; the load window around
  the box is greyed and the box gets a drop shadow. (0x4c04ac)
- **Yes**: the selected save's file in `Saves\` is deleted (a failure is ignored); its slot is
  removed from the slot table and the later slots move up one place; the total count and the
  count of the tab being shown (manual or auto) drop by one; the list is re-sorted newest first,
  the selection is cleared, the box closes and the load window is reopened with the refreshed
  list. (0x4bfe8c, 0x4b8854)
- **No** (or the end of Yes): the box closes and the window it was opened from is shown again;
  for the load window the list is refilled and the previous selection, if any, highlighted
  again; in a game the panel buttons are re-enabled. (0x4bfde0)
- The saves cache file is not rewritten on delete; it is written again at exit. (0x4e1d94)
- The same Yes/No box serves other questions (restart, exit); only the restart Yes runs the
  mission restart (§15), never the No path. (0x4bfe64)

## 14. Autosave (0x4b7410, 0x4cd1fc, 0x4cd558)

- **When.** A flag (0x68dc94) is set when a battle starts (from an event or by contact) and when
  the noon report opens. The autosave is written, if the option is on and the flag is set, by the
  per-frame handler of the battle window and of the event window (so on the first frame of the
  battle and of the noon report), and at a noon without a report. Writing clears the flag; map
  and save loads clear it too. In practice: **one autosave as each battle opens and one as each
  noon report opens.** **code**
- **Name.** In a battle (the interactive-battle flag is set when the battle window opens,
  0x4d235d): the `[SaveGame] AutoSaveBattle` text, a space, and the opponent's name (the army's
  name, or the building's name for a garrison, cut at the first `#` and trimmed). Otherwise the
  game time: `Y.MM.DD, H` followed by a space and the `[Time] cHour` word, with the month
  1-based and the day **0-based**; month and day get a leading zero only when the value printed
  is below 10 for the month and when the day index is below 9 for the day, so day index 9 prints
  as `9` (a padding slip). Under 60 minutes since year 0 the `cLessAtHour` text is used. **code**
  (0x49ce20)
- **Slot.** In a battle, the autosave with the same name is reused; otherwise the autosave with
  the same name **and** the same map title. With no match a new autosave is made while there are
  fewer than 12, else the 12th of the list (the oldest, since the list is sorted newest first) is
  overwritten. When several match, the last one in list order is taken. **code**
- An autosave writes the date, kind 2, then the file as in §12. The kind of an autosave file is
  read back from header byte 0x12E (0 is read as manual). **code**

## 15. Campaign carry-over and restart (0x4b5b64, 0x4b5ef8, 0x4b5ff8)

The rules of what carries over are in economy.md (campaign carry-over) and experience.md. What
matters for loading and saving: **code**
- Before the next map loads, the spell effects of every unit of the hero's army are cleared.
- The next map is loaded normally (§10); then the hero's unit record replaces the new one, and
  header bytes 0x110..0x116 bring back gold, mana, level/XP with the book, worn items, pack and
  the whole army.
- **Gold is set, not added**: the gold function (0x4ab150) in "set" mode stores the old amount
  and a fresh integrity value (gold XOR the clock, kept in the hero's army record together with
  that clock). Its other mode is the ordinary **add** used by every gold change: it first checks
  the integrity value (skipped while the stored clock is 0) and, on a mismatch, punishes the
  player (gold 5 + Random(20), the hero replaced by a level-0 unit of type 0x3A, the rest of the
  army removed); then it adds the amount, clamps a negative result to 0 and refreshes the
  integrity value. In the Community build two one-shot flags set elsewhere can send the next
  such addition to mana instead (also clamped at 0). **code** (0x4ab150, 0xc26085, 0xc25dd9)
- The event flag string is not touched by a map load, so the flags carry over; a new game clears
  them.
- Then the **restart snapshot** is taken: the hero's army record, class, flags, pack, book, gold,
  mana and the map's file name. It is written into every save. Restart reloads that map and puts
  the snapshot back.

## 16. In-memory records

Offsets are for orientation; the full layouts are in the notes. **code**

**Unit** (0x1DB bytes): type (0-based) +0, XP +4, last gain +8, level (0-based) +0x10, named
character +0x14, usable worn slots +0x18, HP +0x20 (−1 unhurt, 0 dead), four spell effects
{spell, end minute} +0x24, worn items +0xCD, three 0x40-byte stat blocks: current with items
+0xDD, the level's without items +0x11D, a copy for battle and display +0x15D; hire kind +0x19D
(0 hero or leader, 1 map troop, 3 event), time of death +0x1A1, paid +0x1A5, last paid +0x1A6,
tactical costs +0x1AA / +0x1AE. A stat block has the layout of the unit type's stats: Nature,
Hits, AB, DB, AS, DS, school, MP, direction, three protections, Regen, Vampirism, Initiative,
Manevres, bonus.

**Army** (0x3827 bytes): unit count, 12 units, then the world state (speed, at-sea, on-map,
faction, attitudes, home building, style, patrol, gold, income, wages, position, planner and
path, the 12-item pack, the AI switches). The DTm fields map onto it as in §10.5.

**Building** (0x166 bytes): the DTm record (dtm-format.md §6) with x/y turned into the top-left
cell and these runtime fields in its unused bytes: gold stock (286), owner (292, 0 = player),
garrison pointer (342–345), restock flag or timer (346–349), mana stock (352).

**Cell** (18 bytes): first-layer object, second-layer object, building id at the anchor, point
id, army occupant, two transient mark bytes, the anchor cell index.

---

## Razdor now → original

| Topic | Razdor now | Original | Where |
|---|---|---|---|
| Ini keys | Matches: exact and case-sensitive, key untrimmed, last match wins | exact and case-sensitive, key untrimmed, **last** match wins | `dt/ini.rs` |
| Ini sections | Matches: `[`…`]` anywhere in a line; last match; a missing section keeps the previous one selected (`Ini::select`, used for `_Global.ini`) | `[`…`]` anywhere in a line; last match; a missing section keeps the previous one selected | `dt/ini.rs` |
| Ini comments, line ends | Matches for the install's files (`Ini::from_cp1251`); Razdor's own `data/*.ini` may also end lines at a bare line feed | nothing skipped (harmless: such keys never match); CR ends a line, the next byte is dropped; tabs removed | `dt/ini.rs` |
| Ini integers | Matches: digits picked out of the text, `-` anywhere negates; a missing key reads 0 (in `_Global.ini` too) | digits picked out of the text, `-` anywhere negates (`1.5` → 15) | `dt/ini.rs` `loose_int` |
| Unit entries | Matches (records): sections with `Name`, `StartExpirience` and `Cost`, stored by `GlobalIndex`; upgrades by section position. Pictures are presentation, left out | sections with `Name`, `StartExpirience` and `Cost`; pictures by section order; upgrades by section position | `dt/data.rs` `parse_units` |
| Upgrade slots | Matches: lone slot 1 or 3 → 2; options 1+2 and 2+3 → 1+3, after the names are resolved | also lone slot 3 → 2; options 1+2 and 2+3 → 1+3 | `normalise_upgrade_slots` |
| Bonus / enum names | Matches: exact; unknown → none | exact; unknown → 0 (no bonus) | `Bonus::known_token`, `Nature::parse` |
| MagicDirection `ToAll` | Matches (same outcome): parsed as its own value, meaning all | not matched; reads as 0, which the code treats as "all" (same outcome) | `MagicDirection::parse` |
| Artefact `Type` | Matches: unknown type = Potion; entry needs Icon, Name, Type, Cost | unknown type = Potion; entry needs Icon, Name, Type, Cost | `dt/data.rs` |
| DTm container | Matches: magic `A?pf`; legacy layout (byte 1 `I`) one bzip2 stream, scramble mode 1 undone; new layout in chunks; a size mismatch not checked. Scramble mode 2 (the game's random numbers) is refused | magic `A?pf`; bytes 6/7 are compression code and scramble mode | `dt/container.rs` |
| DTm strings | Matches: header byte 9 < `'4'` → not loaded; sections by the header sizes (`size div record` records); seeks to the header's text offset; strings end at a NUL or the end; nothing else checked | seeks to the header's text offset; nothing checked; header byte 9 < `'4'` → nothing loaded | `dt/dtm.rs` `parse_payload` |
| Event picture size | Matches: 32-bit, read when positive | 32-bit | `dt/dtm.rs` |
| Space runs in texts | Matches: collapsed in building, army and event strings; titles and named characters kept | collapsed to one space at load (not in titles of the map) | `dt/dtm.rs` / `rules/world.rs` |
| Troops of unit ids 1–3 | Matches: skipped in presets, armies and garrisons (leaders excepted) | skipped in presets, armies and garrisons (leaders excepted) | `world.rs` `dt_entries` |
| Start time | Matches: header minute + 1, a header time of 0 included (minute 1 of year 0) | header minute + 1 | `World::from_scenario` |
| Empty army records | Matches: kept, on or off the map by byte 63, so events can fill and call them | every record fills its slot | `World::from_scenario` |
| Army spell (byte 84) | Matches: every unit's first slot holds it, ending at minute 1 036 800 000 | the same | `World::from_scenario` |
| Garrison triples | Matches: read only for towns, castles, forts and ruins | types 1, 3, 4, 12 | `World::from_scenario` |
| Lanterns at start | Matches: active with a radius, the model not checked | the same | `fog::start_lanterns` |
| Village start stock | Matches: one day's gold and mana | confirmed: one day's gold and mana | `World::from_scenario` |
| Army placement | Matches: exactly the file cell; at sea when that cell is water and not a bridge; off the map only by byte 63 | exactly the file cell; at sea when that cell is water | `World::from_scenario` |
| Building owner byte 0 | Matches: the player | the player | `World::from_scenario` |
| Faction-1 buildings | Matches: only the preset's start building and the class-flagged buildings are given, with the hero's four attitudes as they are | only the preset's start building and the class-flagged buildings; their attitudes are the hero's | `World::from_scenario`, `give_to_player` |
| Start buildings | Matches: bridges not excluded | not excluded | `World::start_buildings` |
| Save format | bzip2 JSON `.rzsave`, map re-read on load | binary dump of the whole state (§12) | `rules/save.rs` (by design; reading original saves is not supported) |
| Manual save names | slug of letters/digits/`-`, same name replaces | cp1251 sanitising, `[n]` suffix on a clash, 12 slots | `save::slug`, `save::write` |
| Load window click | a click selects; a click on the selected row loads; Enter loads; the Load sign is drawn but not clickable; the list scrolls | a click only selects; loading only by the Load button or the Load sign; at most 12 rows per tab, no scrolling | `ui/saves.rs` `load_screen`, `Book::rows` |
| Deleting a save | Matches: Delete sign on the selected row, Yes/No question, file removed, list refreshed | the same flow: the file is deleted, the selection cleared, the other saves keep their order | `ui/saves.rs` `load_screen` |
| Save window | a typed name (pre-filled with map title + game date) matched by name; a click on a save takes its name | 12 fixed slots; a click selects a slot, an empty slot starts blank, a used one keeps its name; the preview shows the real (computer) date; no overwrite question in either | `ui/saves.rs` `save_screen` |
| Autosaves | Matches: at most 12; reused by name before a battle, by name and map title otherwise (the oldest match); else a new one while fewer than 12, else the oldest overwritten | at most 12, reuse by name (+ map), else the oldest overwritten | `save::write_autosave` |
| Autosave moments | Matches: before every battle (as its window opens) and as each noon report opens, only with `OptValue8` on (without an install Razdor always autosaves); a noon without a report (no wages, no income) writes none | as each battle window and each noon report opens, option on; a silent noon only writes a pending one | `ui/saves.rs`, `Game::pass_slice` |
| Autosave date name | Matches: day index 9 printed without the leading zero; under 60 minutes the "less than an hour" text (Razdor's own wording) | day index 9 printed without the leading zero; `cLessAtHour` under 60 minutes | `save::date_name` |
| Battle autosave name | Matches: the army's or building's name cut at the first `#`, trailing spaces trimmed (no leader-name fallback) | the same | `save::autosave_foe` |
| Restart | Matches: a campaign map restarts from what it was handed over (kept in saves), a new-game map from its preset in the starting class | the restart snapshot of §15 | `Game::restart` |
| Carry-over gold | Matches: set to the old amount | set to the old amount, not added | `Game::apply_carry_over` |
| Carry-over bytes | Matches: the next map's header bytes 0x110–0x116 choose what is taken (until 0.3.10 Razdor read the old map's, so РК3 → РК4 kept the army and pack) | the map just loaded (the next one) | `Game::apply_carry_over` |
| Play options | Matches: `OptValue9`/`10`/`11` and `[Tutorial] Completed` on when they read 1 (loosely); `OptValue11` picks the 6- or 4-wide front row, and a loaded game keeps the width it was saved with, which holds for the rest of the session (next new game, restart, campaign map) | flags read at start, 1 = on; the wide row 6 or 4 per row; a save load sets the row width for the session | `dt/install.rs` `PlayOptions`, `Content::from_dt`, `save::restore` |

## Unknowns

- The exact use of the unit field +0x1C (the hero's class, 3 for map troops, the type for an AI
  leader) and of the cells' transient mark bytes. **unknown**
- The sixth value of the spell and battle effects. **unknown**
- Whether anything reads the stream's size-mismatch flag; the loaders do not. **unknown**
- The 3-letter button keys and the hint section name of the texts ini were not decoded (labels
  only). **unknown**
- A save of a map whose width is not 50, 100 or 200 starts with an uninitialised buffer size.
  Harmless in practice (the stream grows), not tested. **unknown**
