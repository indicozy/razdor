# A DT map as an OpenMW world: design spec (2026-11-27)

Status: M1 and M2 are built in `crates/openmw-world`. The owner's answers of 2026-11-27 are
in the last column of the §7 table and are applied in the sections they name. Owner decisions already made: the converter lives in this
repo as its own workspace crate, it reads the JSON v1 of `razdor-dtm-json` (not `.DTm`), the target
is OpenMW 0.51 on the public-domain OpenMW Game Template, and the player needs no Morrowind.

The output of the converter holds the map's texts and layout. It is game content: it is written
outside the repo (`/mnt/ai_models/razdor-scratch/openmw/<map>/`) and is never committed. Tests in
the repo use synthetic maps only.

How facts are marked: **[V]** checked against a primary source on 2026-11-27 (the installed
`openmw` 0.51.0 package, its `lua_api/` files and `--help` output, or OpenMW's source on GitHub);
**[R]** read by a research agent from OpenMW's source at tag `openmw-0.51.0`, from the docs or from
the Template's files, and not re-checked here; **[U]** from memory, the named milestone must settle
it. The installed Lua API is revision 129 **[R]**; the "latest" pages of the online docs describe
a newer one, so the package's `lua_api/` files are the reference. UESP's format
pages refused automated reads, so the record layouts below come from OpenMW's `components/esm3`
readers ([source](https://github.com/OpenMW/openmw/tree/master/components/esm3)); UESP stays the
human reference ([Mod File Format](https://en.uesp.net/wiki/Morrowind_Mod:Mod_File_Format)).

## 1. How a DT map works

Sources: `docs/reference/dtm-format.md`, `docs/reference/original-mechanics/world.md` and
`events.md`, `src/dt/dtm_json.rs`. Only what a 3D world needs.

**Grid.** A map is W×H square tiles (50, 100 or 200 a side), 8 neighbours, no elevation. `(x, y)`
is (column, row); row 0 is the top (north). A diagonal step costs 1.5× an orthogonal one. The map
edge is a hard border.

**Surfaces.** One code per tile, 16 codes. Passability and cost come from the original's tables
(world.md §1.1):

| code | name (JSON `legend`) | on foot | by ship |
|---|---|---|---|
| 0 | shallows_fords | blocked | 10 min |
| 1 | coastal_water | blocked | 5 min |
| 2 | deep_sea | blocked | blocked |
| 3 | lava_fields | blocked | – |
| 4 | road | 15 min | – |
| 5, 6, 7 | grass_lowland, grass_plain, dry_plain | 25 min | – |
| 8 | marsh | 40 min | – |
| 9 | impassable_swamp | blocked | – |
| 10, 11, 13 | sand_dunes, clay_soil, scorched_land | 25 min | – |
| 12 | stony_soil | 20 min | – |
| 14 | snowy_ground | 30 min | – |
| 15 | impassable_snowdrifts | blocked | – |

Minutes are game minutes per orthogonal step at hero speed 5. All water is closed to a walker,
the shallows too: water is crossed by bridge or by ship.

**Objects.** `(x, y, sprite, class)`. A tile keeps at most one object of classes 1–8 and one
plant (classes 9–12); a later record replaces an earlier one of the same group. Classes: 1 hills (walkable, slower),
5 mountains (blocked), 8 stones (blocked), 9 trees (+20 min), 10 dead trees (+30 min), 11 thicket
(blocked). Classes 1–8 are *massifs*: a square of side `f = sprite div 10` tiles whose
bottom-right tile is the object's tile. Plants cover their own tile. A plant or massif on water
also blocks ships.

**Buildings.** 16 kinds: palace, town, village, castle, fort, tavern, market, church, smithy,
shipyard, altar, dungeon entrance, ruins, stone bridge, wooden bridge, obelisk. The stored `x, y`
is the bottom-right tile of a `size_x × size_y` footprint, plus one extra row above when
`size_x > size_y`. Every footprint tile is walkable at road cost, bridges included (a bridge is a
1×1 building on water; chains of them may run diagonally). A building has a faction (1 player,
2 ally, 3 neighbour, 4 enemy), an owner army (255 = none), a garrison (up to 6 squads), a linked
building (a village's castle), gold and mana income, barracks, goods, and a list of local events.
The hero *enters* a building when his walk ends on its footprint; a garrison is fought before
that step. Bridges and obelisks open no window, but an obelisk's events still fire.

**Armies.** One record per army: tile, model (feudal, bandits, peasants, inactive, …), a leader
unit with a level, up to 6 squads `(unit, level, count)`, faction and attitudes, aggression, a
home building, a patrol flag with a radius (a box `home ± radius`; radius 0 = stands still), an
`inactive` flag (off the map until an event activates it), respawn days, a ship type, a speed
correction. Unit ids point into the install's ini data; the JSON has no unit stats.

**Points.** A tile with an id, a radius, an `active` flag and up to 5 events. A point with a
radius reveals the fog around it when active at start or when an event lights it (the radius is
in tiles, at most 24). A point with events fires them when the hero stands on its tile. The JSON
has no `model_name` for points: use `radius`, `active` and `events`.

**Fog.** The hero reveals 8–10 tiles around him (by class); explored ground stays visible;
unexplored tiles cannot be walked by the hero. Armies ignore the fog.

**Clock.** Minutes since year 0; a day is 1440. Time passes only while the hero walks or waits.
The map starts at `info.start_time`.

**Events.** Kinds: global (may fire anywhere), local (in the building or point that lists it),
quest (goes to the journal), rumour (offered in a building's hall). The engine scans after every
hero step, on contact with an army, on each 30-minute tick of a wait, and after a dialog closes.
One scan takes the first passing event: globals in file order, then the point under the hero,
then the building he is in. Silent events apply at once and the scan repeats.
- *Eligibility*: not `subordinate` (those run only as a chain), the firing guard, `start_time`
  reached, the repeat and duration window, `once`, the hero archetype.
- *Conditions* (all set ones must hold): army size and strength, level, gold, mana, building
  ownership, a unit in some army, artifacts, armies defeated by the player or by anyone, other
  events answered yes / no / not happened, the army being met, an army active or at home, a
  yes/no question.
- *Results*: a message window, gold, mana, experience, spells, units added or removed, artifacts,
  armies activated or deactivated, patrol change, lanterns lit, an army shown or moved to the
  hero, a battle started, a quest completed, a *relative* event (opens another event after a
  delay), a *chained* event (runs at once, unchecked), a new hero class.
- *Flag script* in the title after `%`: `+X` sets, `-X` clears, `=X` requires, `=/X` requires
  unset. Tests are substring searches in one flag string. The JSON gives them parsed
  (`flags.set`, `clear`, `require_set`, `require_unset`, `raw`).

**Heroes and start.** Three presets (knight, archmage, ranger): start tile, gold, mana, squads,
artifacts, spells, a start building. A class is offered only when its start x or y is non-zero.
The start building and every building with `start_for[class]` belong to the player.

**Victory and defeat.** `info.victory_event` and `info.defeat_event` are event ids: when one
finishes, the map ends (the next campaign map, or the menu). Losing the whole army also ends the
game without firing the defeat event.

## 2. The OpenMW target

### 2.1 What is written offline and what Lua does

OpenMW's Lua cannot make cells or land. `world.createRecord` covers activators, containers,
creatures, doors, lights, misc items, NPCs, statics and a few item types; `world.createObject`
makes instances, which start disabled until `:teleport` places them **[V]** (`lua_api/openmw/world.lua`).
Online pages of the same API: [world](https://openmw.readthedocs.io/en/latest/reference/lua-scripting/openmw_world.html),
[types](https://openmw.readthedocs.io/en/latest/reference/lua-scripting/openmw_types.html),
[core](https://openmw.readthedocs.io/en/latest/reference/lua-scripting/openmw_core.html),
[ui](https://openmw.readthedocs.io/en/latest/reference/lua-scripting/openmw_ui.html),
[engine handlers](https://openmw.readthedocs.io/en/latest/reference/lua-scripting/engine_handlers.html),
[events](https://openmw.readthedocs.io/en/latest/reference/lua-scripting/events.html),
[script kinds and `.omwscripts`](https://openmw.readthedocs.io/en/latest/reference/lua-scripting/overview.html).
A journal entry is added by `types.Player.quests(player)[id]:addJournalEntry(stage)` only "if
there is an entry at the specified stage", so quest texts must be DIAL/INFO records in a content
file **[V]** (`lua_api/openmw/types.lua`).

| Offline, in `<name>.omwaddon` | At runtime, in Lua |
|---|---|
| LAND and CELL records, LTEX | the player's start (teleport), gold, party |
| STAT, ACTI, LIGH, MISC, CREA base records | every army instance (spawn, patrol, activation, respawn) |
| references of scenery, buildings, doors, bridges | |
| DIAL/INFO journal records of the quests | the event engine, flags, the DT clock |
| | the passability guard (§3.5), messages, questions, victory and defeat |

### 2.2 Files the converter emits

```
<out>/
  <name>.omwaddon            TES3 plugin: records and cells
  <name>.omwscripts          registers the Lua scripts
  scripts/razdor/*.lua       the fixed runtime (copied from the crate)
  scripts/razdor/map_data.lua   generated: tiles, buildings, armies, points, events, texts
  meshes/razdor/*.dae        generated placeholder meshes
  textures/razdor/*.tga      generated flat-colour ground and placeholder textures
  openmw.cfg.snippet         the lines to add to a standalone openmw.cfg
  manifest.json              what was written, with sizes and SHA-256 (for tests)
```

Record order in the plugin: TES3, LTEX, STAT, ACTI, LIGH, MISC,
CREA, DIAL with its INFOs, then all CELLs, then all LANDs. LTEX comes before LAND.

Layouts the writer needs (little-endian; a record is `name[4] size u32 zero u32 flags u32`, a
subrecord `name[4] size u32`) **[V]** unless marked:

| record | subrecords, in order |
|---|---|
| TES3 | `HEDR` 300 B: version f32 = 1.3, type u32 = 0, author char[32], description char[256], numRecords u32. Then per master `MAST` zstring, `DATA` u64 file size |
| LTEX | `NAME` id, `INTV` u32 index, `DATA` texture path (relative to `textures/` **[U]**) |
| LAND | `INTV` i32 x, i32 y; `DATA` u32 flags (1 heights and normals, 2 colours, 4 textures); `VNML` 65·65·3 i8; `VHGT` f32 offset + 65·65 i8 deltas + 3 pad = 4232 B; `WNAM` 81 i8; `VCLR` 65·65·3 u8 (optional); `VTEX` 256 u16 |
| CELL | `NAME` empty for an exterior; `DATA` flags u32 (0x02 has water), gridX i32, gridY i32. Then per reference: `FRMR` u32 (low 24 bits index from 1, top byte 0 for the plugin's own references **[U]**), `NAME` base id, optional `XSCL` f32, `DATA` 24 B: position xyz and rotation xyz f32, radians |
| STAT | `NAME`, `MODL` (relative to `meshes/` **[U]**) |
| ACTI | `NAME`, `MODL`, `FNAM` display name |
| LIGH | `NAME`, `MODL`, `FNAM`, `LHDT` 24 B: weight f32, value i32, time i32, radius i32, colour rgb0, flags i32 |
| MISC | `NAME`, `MODL`, `FNAM`, `MCDT` 12 B |
| CREA | `NAME`, `MODL`, `FNAM`, `NPDT` 96 B (type, level, 8 attributes, health, mana, fatigue, soul, combat, magic, stealth, 3 attack min/max pairs, gold; all i32), `FLAG` i32, `AIDT` 12 B **[U]** |
| DIAL | `NAME` id, `DATA` 1 B type (4 = journal **[U]**) |
| INFO | `INAM` id, `PNAM` previous id, `NNAM` next id, `DATA` 12 B (the journal index is the second i32 **[U]**), `NAME` text, optional `QSTN`/`QSTF` 1 B |

Land details **[V]** (`loadland.cpp`):
- 65×65 vertices per cell, 128 units apart; a cell is 8192 units. Index `row·65 + col`.
  Row 0 is the south edge and column 0 the west edge **[U]** (M1's test cell proves it).
- `VHGT`: `offset` is the first height ÷ 8. Each row's first delta is taken from the row below's
  first vertex; every other delta from the vertex to its left. A height is the running sum × 8.
  So heights are multiples of 8 and two neighbours differ by at most 127 × 8 = 1016 units.
- `VTEX`: 16×16 texels of 512 units. The file stores them in 4×4 blocks of 4×4: the value at
  file position `(y1·4 + x1)·16 + (y2·4 + x2)` belongs to row `y1·4 + y2`, column `x1·4 + x2`.
  The stored value is the LTEX index + 1; 0 is the default texture.
- `WNAM` (the 9×9 world-map heights) is ignored on load; we still write it by OpenMW's own rule
  (height ÷ 128 above zero, ÷ 16 below, clamped to i8).
- A cell without a LAND record has the default height −2048, far under the water.
- A reference's `XSCL` is clamped to 0.5–2.0 on load **[V]** (`cellref.cpp`), and is uniform.
  One cube stretched to a footprint is therefore not possible: §3.7 generates one mesh per size.

Text encoding: TES3 strings are single-byte. DT's texts are Russian, so the plugin's strings are
written in Windows-1251 and the config sets `encoding=win1251` (`openmw --encoding`,
`esmtool -e win1251` **[V]**). Lua files are UTF-8.

### 2.3 The Game Template base

The [OpenMW Game Template](https://openmw.readthedocs.io/en/stable/reference/modding/openmw-game-template.html)
is CC0 (its font and its GPLv3 MyGUI config files excepted **[R]**), needs OpenMW 0.47 or later, and lives in
[OpenMW/example-suite](https://gitlab.com/OpenMW/example-suite). The repo's folders **[V]** (its
file tree):
- `game_template/data`: `template.omwgame`, the player model `meshes/BasicPlayer.dae`, sky, water,
  UI textures and icons; one rock texture (`tx_rock_01.dds`). No ground set, no buildings.
- `the_hub/data`: `the_hub.omwaddon`, ground textures `textures/ground/{grass,dirt,road}_01.dds`,
  `rock_01.dds`, temple and museum meshes, a chest, doors, basic weapons, sounds.
- `example_animated_creature/data`: `landracer.omwaddon` with `meshes/land_racer.dae`, the only
  animated creature. `example_static_props/data`: a barrel.

`template.omwgame` holds **[R]** (parsed by the research agent; M1 repeats it with `esmtool
dump` and writes the list into the crate's README): game settings, magic effects, skills and
sounds; one race `DefaultRace`, one class `DefaultClass`, the NPC `Player`, one body part, one
region `Island`; marker statics; one LTEX `rock_01` (index 0); **one LAND and one exterior CELL
at (0, 0)**; two start-up scripts. It has no creature, light, activator or container, no gold
item and no building.

What follows from that:
- The converter depends on `template.omwgame` only (its one `MAST`). Our LAND and CELL at (0, 0)
  must replace the Template's; a later content file winning per cell is **[U]** (M1 checks it in
  the smoke run or shifts the map by one cell).
- LTEX indices are per plugin; we write our own full table and do not use the Template's.
- OpenMW's gold item id is `gold_001` **[U]**; the Template has none, so the plugin defines a
  MISC record with that id.
- It may use the `the_hub` and `land_racer` *files* when their folders are on the data path
  (`--template-assets`), and falls back to its own generated textures and meshes when not.
- The docs also ask to copy the Template's `resources/mygui` files (skins and a font) over
  OpenMW's. Owner (Q9, 2026-11-27): OpenMW and its resources are not modified in any way. M6 only
  investigates whether Cyrillic renders with the stock resources and reports the result.

### 2.4 Launching a standalone game

A separate config folder keeps the owner's own OpenMW setup untouched
([paths and config layers](https://openmw.readthedocs.io/en/latest/reference/modding/paths.html)).
`openmw.cfg` there:

```
replace=config
replace=data
replace=content
resources=/usr/share/openmw/resources
user-data=<run dir>/userdata
data=/usr/share/openmw/resources/vfs
data=<example-suite>/game_template/data
data=<example-suite>/the_hub/data                 # optional placeholder assets
data=<example-suite>/example_animated_creature/data   # optional
data=<out>
content=builtin.omwscripts
content=template.omwgame
content=<name>.omwaddon
content=<name>.omwscripts
encoding=win1251
```

Run: `openmw --replace=config --config <config dir> --skip-menu --start "<cx>,<cy>" --no-grab
--no-sound` (all options exist in 0.51 **[V]**; OpenMW's own CI starts its integration tests
this way under `xvfb-run` **[R]**). The `[Models]` lines of the Template's page (the
`BasicPlayer.dae` animation sources, the sky meshes) and its `[Game] default actor pathfind half
extents` go into `settings.cfg` in the same folder.

Start **[R]** (`engine.cpp`, `worldimp.cpp`): `--skip-menu` without `--new-game` skips character
generation and honours `--start`; `"x,y"` is an exterior cell, and the player lands in its centre
on the ground. With `--new-game` the Template's start script runs and `--start` is ignored. With
neither, the player is put at (4096, 4096, 0), which may be under our land. `<cx>,<cy>` is the
cell of the hero's start tile; the global script then moves the player to the exact tile (§3.9),
so the design holds whichever way the game was started. The `sStartCell` setting is not used.

Still **[U]**, for M6: the exact `replace=` set, and whether `data=…/vfs` and
`content=builtin.omwscripts` must be listed when the global config is replaced. The global config
also loads `vfs-mw` (Morrowind-specific built-in scripts); the standalone config leaves it out.

Lua scripts are registered by the `.omwscripts` file, one line per script with its kind, as the
built-in file does **[V]** (`resources/vfs/builtin.omwscripts`):

```
GLOBAL: scripts/razdor/global.lua
PLAYER: scripts/razdor/player.lua
CUSTOM: scripts/razdor/army.lua
```

OpenMW builds its navigation mesh at runtime from the land and the collision shapes, so no path
grid records are written **[R]**
([navigator settings](https://openmw.readthedocs.io/en/latest/reference/modding/settings/navigator.html)).
M4 checks that a wandering army walks around a building.

Models: OpenMW loads `.nif`, `.dae` and OSG's native formats, not glTF. For a `.dae` the
collision shape is the subtree of a node named `Collision` when there is one, else the whole
visible mesh **[R]** (`bulletshapemanager.cpp`;
[custom models](https://openmw.readthedocs.io/en/latest/reference/modding/custom-models/index.html)).
The generated placeholders are Collada text, so no NIF writer is needed.

## 3. Mapping

### 3.1 Scale: 1 tile = 1024 units, 8 tiles per cell

| constraint | 512 units | **1024 units** | 2048 units |
|---|---|---|---|
| ground texels per tile (a texel is 512) | 1 | 2×2 | 4×4 |
| land vertices per tile side | 4 | 8 | 16 |
| tile in metres (70 units ≈ 1 m) | 7.3 | 14.6 | 29 |
| a 4×4 castle | 29 m | 58 m | 117 m |
| loaded 3×3 cells, in tiles | 48 | 24 | 12 |
| LAND records, 200×200 map | 169 | 625 (≈ 19 MB) | 2500 |

The tile must be a multiple of 512 so that tile borders fall on texel borders. At 512 a one-tile
hill has 4 vertices to be a hill with, and a castle is the size of a house. At 2048 a 200×200 map
is 50 cells a side and a road is 29 m wide. At 1024: a road or a one-tile bridge is 14.6 m wide
against an actor about 128 units tall, a massif has 8 vertices per tile to shape a slope, and the
loaded 3×3 cells show 8 to 16 tiles each way, close to the hero's 8–10 tiles of sight. The CLI
keeps `--tile-units 512|1024|2048`; the default and every number below assume 1024 (`T`).

### 3.2 Coordinates

- The map's south-west corner is the world origin, so the map covers cells `0..ceil(W/8)−1` by
  `0..ceil(H/8)−1`. A ring of one margin cell surrounds it (§3.4).
- Tile `(x, y)` → centre `X = (x + 0.5)·T`, `Y = (H − y − 0.5)·T`. DT's row 0 is north, TES3's
  Y grows north, hence the flip.
- Vertex `(i, j)` of the global grid is at `(128·i, 128·j)`; cell `(cx, cy)` owns vertices
  `64·cx .. 64·cx + 64` (edges are shared and must hold equal heights in both cells).
- A footprint with anchor `(x, y)`: west `(x − sx + 1)·T`, east `(x + 1)·T`, south
  `(H − 1 − y)·T`, north `(H − y + sy − 1)·T`, one more `T` north when `sx > sy`.
- DT has no facing. Buildings face south (−Y), as their sprites face the viewer.

### 3.3 Heights

DT has no elevation, so heights are made up from the surface and the massifs. Water level is 0
(exterior water is fixed near 0 in OpenMW **[U]**; M2 reads it off the smoke test).

1. **Base per tile** (units):

| surface | base | surface | base |
|---|---|---|---|
| deep_sea | −1200 | marsh | +24 |
| coastal_water | −400 | impassable_swamp | −56 |
| shallows_fords | −200 | sand_dunes | +48 |
| lava_fields | +40 | clay_soil, scorched_land | +96 |
| road | +64 | stony_soil | +160 |
| grass_lowland | +56 | snowy_ground | +128 |
| grass_plain, dry_plain | +96 | impassable_snowdrifts | +280 |

   Every water tile is at least 200 deep at its centre, so an actor swims there and cannot wade
   across (the shallows are closed on foot in DT).
2. **Coast.** A land tile's base is multiplied by `min(1, d / 2)`, `d` = its distance in tiles to
   the nearest water tile (8-neighbour distance), with a floor of +24: beaches slope to the water.
3. **Vertices.** Each vertex takes the bilinear blend of the four nearest tile centres. Then two
   passes of a 3×3 box blur over the vertex grid.
4. **Flat footprints.** Every vertex inside a building footprint is set to the footprint's mean
   height (at least +48). The flat area is the footprint plus a one-tile apron (M2). Buildings are
   Morrowind-scale and may extend past their footprint (Q1), so the flat ground grows to fit them;
   M3 tunes the apron. A bridge tile keeps its water bed; the land tiles next to a bridge are
   raised to the deck height +64 along the shared edge so that a walker steps onto the deck.
5. **Hills** (class 1–4): a smooth bump over the massif square, peak `160·f`, zero at its edge.
   The slope stays under 30°, so hills are walkable, as in DT.
6. **Mountains** (class 5–7), added after the blur so they stay steep: a ridge over the massif
   square, peak `900·f` plus noise. OpenMW's actors cannot climb above 46°; the peak is above
   `f·512·tan 46° = 530·f`, so the sides are too steep to walk.
7. **Noise.** ±12 units of value noise on land, seeded from `info.generator_seed`.
8. **Quantise** to multiples of 8, then clamp neighbour differences to ±1016 by lowering the
   higher vertex, repeated until stable.
9. **Normals** (`VNML`) from the final heights by central differences, scaled to 127.

Stones (class 8) do not change the land; they are rocks with collision (§3.6).

### 3.4 The map edge

Tiles outside the map (the rest of the last cells and the margin ring) copy the nearest edge
tile's kind: next to water they are deep sea; next to land they rise into a border ridge
(+2000 units over two tiles, rock texture). Beyond the margin there is no LAND, which OpenMW
shows as open sea.

### 3.5 Passability

OpenMW has no per-tile passability: a player can swim, jump and climb where a DT hero cannot.
The gap is closed in two layers.

- **Shape** (soft): deep water, steep mountains, rocks and thicket blocks with collision.
- **Guard** (hard, on by default, `--no-guard` turns it off): the converter computes DT's own
  on-foot map (world.md §1.1: surface values, plants, massifs, footprints at road cost, the
  bottom-right massif rule clipped to the map) and writes it into `map_data.lua` as one hex digit
  per tile. The player script reads the player's tile five times a second. On an open tile it
  remembers the position. On a closed tile it puts the player back on the last remembered
  position and shows one short message. A bridge tile is a footprint tile, so it is open.

The guard is also what turns "the player moved" into DT's "a step ended": a change of tile is
the trigger of the event scan (§3.11). AI armies are not guarded in v1; they keep to their patrol
box. Ships are not modelled (Q4): water stays closed on foot, and a map that needs a ship cannot be
finished.

### 3.6 Ground textures and scenery

**Textures.** 18 LTEX records, indices 0–17: one per surface code (index = code), 16 = rock
(mountains and the border ridge), 17 = packed dirt (building footprints). Each points at a
generated `textures/razdor/surf_<name>.tga`, 64×64, a flat colour of our choice with ±6 noise.
With `--template-assets`, grass, road, dirt and rock point at the `the_hub` textures instead. A
tile sets its 2×2 texels to its surface's index; mountain massif tiles use 16, footprints 17,
bridge tiles keep the water's.

**Objects.**

| class | world object | collision | placement |
|---|---|---|---|
| 1 hills | none (heights only) | – | – |
| 5 mountains | heights, plus one rock STAT per massif on the peak | yes | massif centre |
| 8 stones | rock STAT, one mesh per `f` (1–6), sized `0.8·f·T` | yes | massif centre |
| 9 trees | tree STAT (cone on a trunk) | trunk only | tile centre, jitter up to ±T/4, yaw one of 16, scale 0.8–1.25 |
| 10 dead_trees | bare tree STAT | trunk only | same |
| 11 thicket | thicket STAT: three trees over a `T × T × 600` block | the block | tile centre, yaw 0 |

The converter first applies DT's rule of one massif and one plant per tile (the later record
wins), so reference counts follow the kept objects. Jitter, yaw and scale come from a hash of `(seed, class, x, y, index on the tile)`. Scale stays
inside OpenMW's 0.5–2.0 clamp. Sprite ids pick nothing in v1 beyond the massif size.

### 3.7 Buildings, roads, bridges, water

- **Building** → Morrowind-scale buildings, not boxes stretched to the DT footprint (Q1). A town
  is a cluster of houses, a castle a proper castle, a palace, church, tavern and so on a building
  of its kind. Each is centred on the DT footprint and may extend past it; the flattened ground
  (§3.3) grows to fit. Meshes are generated per `(kind, size_x, size_y)`, with the kind's height
  (castle and fort 1200, town and palace 900, church 800, village, tavern, market, smithy,
  shipyard 420, altar and obelisk 300, ruins 200, dungeon entrance 260) and a colour by kind. One
  STAT reference per building. Scale and layout are tuned in M3.
- **Door** → none (Q6: no door window). Stepping into a building's footprint is entering it: it
  fires the building's events (§3.11). Obelisks and altars keep their own activator on the body.
  Interiors are not built (Q6).
- **Garrison** → guards on the building's footprint, spawned by Lua like armies (§3.8), hostile
  when the building's faction is hostile to the player. The building counts as taken when they
  are dead or there are none, and the player steps into it.
- **Roads** are surface code 4: the road texture and +64 base height. No mesh.
- **Bridge** (kinds 13, 14) → a deck STAT, a slab `(T + 256)` square and 32 thick with its top
  at +64, centred on the tile. The 128-unit overhang makes decks of a diagonal chain overlap at
  their corners (a 256×256 patch), so the chain is walkable. Stone and wooden decks differ in
  colour only.
- **Water** is OpenMW's own water plane; nothing is placed. Parked ships and ship armies: not in
  v1.

### 3.8 Armies

- One CREA record per army (`rz_army_<id>`, `FNAM` = the army's name) and one per garrison.
  Instances are made by Lua (`world.createObject`, then `:teleport('', position)`), never placed
  offline: 13 of the sample's 15 armies start inactive, activation and respawn need the same
  code anyway, and Lua can then track each instance without searching cells.
- An army is one OpenMW actor per DT unit card (Q5): a squad of count N is N actors, each its own
  actor, and the army's actors are shown and move together as a squad. The leader is the first
  actor. The actors of one army stand in a ring around the leader (radius 160 units) at spawn.
- **Placeholder stats** (the JSON has unit ids, not stats): per body `level = 1 + unit level`,
  `health = (30 + 20·level)·sqrt(count)`, attack `4 + 3·level` to `8 + 6·level`, combat skill
  `40 + 10·level` capped at 100. `--units <file.json>` may later give real per-unit numbers and
  models (Q3). Model: `land_racer.dae` when the Template's example is present, else a generated
  box figure that does not animate.
- **Attitude**: `info.relations[player][faction]` below 0 → `AIDT` fight 100 (attacks on sight);
  otherwise fight 0.
- **Movement** (the army's local script, `interfaces.AI.startPackage`): `patrols` with a radius
  → a Wander package of distance `radius·T` around the home tile; radius 0 or no patrol → stands.
  Targeting models, hunting, hiring and income are DT's strategic AI and are not ported.
- **Defeat**: the engine sends a dead actor's local scripts the `Died` event **[R]**; the army
  script passes it to the global script (`types.Actor.isDead` is the fallback **[V]**). An army
  is defeated when its leader is dead. `respawn_days` > 0 → the army is spawned again at its
  home after that many DT days.
- **Meeting**: an army within `1.5·T` of the player is "met" (§3.11). In the sample 4 of the 9
  meet events name a hostile army, so hostility must wait: while an eligible event names an army
  in `meet_army` (events.md §9, the "meeting event waiting" mark), that army's bodies are held
  at fight 0. The mark is recomputed after every scan; when it drops, the fight setting returns.
- **Ship armies** (`ship` ≠ 0, or an army whose tile is water): spawned and not skipped (Q4).
  Ships are not modelled yet, so the army's actors float on the water surface (height 0) and
  stay on their water tile; they are not moved to land. The army follows the same rules as any
  other. The sample has one (active, hostile, on the shallows).
- **Blocked tiles** (not ship armies, Q4): an army, a garrison body or the hero's start on a tile
  that is closed on foot (the sample has an inactive army on impassable swamp) or inside a building's box is
  spawned on the nearest open tile outside any box, searched in rings; ties go to the lower
  direction number of world.md §1. Its home stays the original tile for events.

### 3.9 The player

- `--archetype knight|archmage|ranger`; the default is the first preset whose start x or y is
  non-zero. The choice is written into `map_data.lua`; it gates events and the start tile.
- At the first start the global script teleports the player to the start tile's centre, on the
  ground. The handlers `onNewGame` and `onPlayerAdded` exist **[R]**; which of them fires under
  `--skip-menu` without `--new-game` is **[U]** (M4). The script keeps a saved "started" flag, so
  either may do it.
- Gold: the preset's gold as `gold_001` items in the inventory (§2.3). Mana, experience, artifacts and
  spells: Lua counters and lists in v1.
- The party is a Lua list of `(unit, level, count, named)` from the preset's squads. Each entry
  is one follower body with a Follow package, built like an army body. Events add and remove
  entries; "losing the whole army" is the player's own death.
- The buildings of the start (`start_building`, `start_for`) are owned by the player from the
  start.

### 3.10 Points, lanterns and fog

- There is no fog of war in v1: the 3D view distance takes its place (Q7).
- A point is invisible. If it has events, standing on its tile offers them to the scan. A point
  on a footprint tile (the sample has two) cannot be stood on, the box is solid: its events are
  offered when the player enters that building, and its beacon stands on the box's roof.
- No lanterns (Q7): a point with a radius gets no beacon and no LIGH, and `light_lanterns` does
  nothing. Points are kept for their events; only the lantern beacons are dropped. This reading of
  Q7 is an assumption until the owner says otherwise. `--debug-markers` still draws points.

### 3.11 Events in Lua

`map_data.lua` holds the events as plain tables with the JSON's field names. The fixed runtime
interprets them; nothing is compiled per event.

**The clock.** DT minutes = `info.start_time + (game time − game time at start) / 60`. The game
time scale is set once (`world.setGameTimeScale`) to `--time-scale`, default 120: a DT day lasts
12 real minutes. In DT a grass tile costs 25 minutes; here a tile at a run should cost less (a
guess of about 6, the Template's run speed is **[U]**; M4 measures it), so windows counted in
days are as generous as in DT or more (Q2).

**When the scan runs** (TES3 has no trigger volumes; all triggers are Lua):
1. the player's tile changes (the guard's poll, §3.5);
2. the player steps into a building's footprint (Q6), or stands on an obelisk's or bridge's tile;
3. an army is met;
4. every 30 DT minutes;
5. a message or question window closes.

The scan follows events.md §2–§3: globals in file order, then the point under the player, then
the entered building's events (local events only, all kinds in villages and shipyards); the first
passing event is taken; silent events apply and the scan repeats; a shown event stops the scan
until its window closes.

**Scope**, taken from the sample map: its victory and defeat events are reached through 11 of
its 54 events, and the fields on that path are the v1 minimum.

| | v1 | later |
|---|---|---|
| eligibility | `subordinate`, `once`, firing guard, `start_time`, `repeat`, `duration`, `archetype` | – |
| flags | set, clear, require, require-unset, with the substring rule | `^` counters |
| conditions | `happened_yes`, `happened_no`, `not_happened`, `defeated_armies`, `beaten_armies`, `buildings` + owner, `units` + named + owner, `gold`, `meet_army`, `army_active`, `confirm_question` | `squad_count`, `army_strength`, `level`, `holiness_mana`, `artifacts`, `army_inactive`, `army_at_home` |
| results | message, `gold`, `mana`, `experience`, `units_add`/`_remove` (+ named), `artifacts_add`/`_remove` (a list), `activate_armies`, `deactivate_army`, `chained_event`, `relative_event` + delay, `completes_quest`, `light_lanterns`, `no_meeting`, `repeat_after_yes`, `picture` 200/201 | `patrol_army`, `cast_spell` (v1: a message line), `spells_learned`, `new_hero_class`, `move_to_hero`, `show_army`, `hero_one_hp`, `start_battle_with`, `generate_battle_army`, `units_from_army`, `removed_units_to_army`, `delay_hours`, community opcodes |

**Windows.** A message is a centred window with the title, the text and OK (`openmw.ui` with the
built-in `mwui` templates; the world is paused while it is open). A question adds Yes and No.
`#HERONAME` is replaced by the player's name. Event pictures are not exported and not shown.

**Quests.** Every quest event gets a journal DIAL `rz_q<id>` with two INFOs: index 10 (the
title, question and message) and index 100 (flagged finished). Taking the quest calls
`addJournalEntry(10)`; `completes_quest` calls `addJournalEntry(100)`. DT removes a completed
entry; here it moves to the finished list.

**Hall.** Stepping into a building runs the scan with that building entered (Q6: no door window). If nothing fires, a list
window offers the building's eligible quests and rumours (title only); picking one opens its
question or its message, as the original's hall does.

**Saves.** All state (flags, fired counts and last-fired times, answers, the party, ownership,
army instances, lit points, the clock offset) lives in the global script and goes through
`onSave` / `onLoad`.

### 3.12 Victory and defeat

When the victory or the defeat event finishes, its window is the last one: on OK the player's
controls are switched off and an end window offers "Quit": `types.Player.sendMenuEvent` to a menu
script that calls `menu.quit` (both exist in 0.51 **[V]**; OpenMW has no end screen of its own). The next campaign
map is not loaded (Q8). The player's death is OpenMW's own death screen.

## 4. Crate design

- **Crate** `razdor-openmw` at `crates/openmw-world`, added to the workspace `members`, not to
  `default-members`. Licence and edition as `crates/wire`. Dependencies: `serde`, `serde_json`
  only. It does not depend on the root crate, so it builds without macroquad.
- **Binary** `razdor-openmw <map.json> -o <out dir> [--name <id>] [--archetype …]
  [--tile-units 1024] [--time-scale 120] [--template <template.omwgame>] [--template-assets]
  [--no-guard] [--debug-markers]`. Exit code 2 with a message on stderr for a wrong `format` or
  `version`, a terrain row of the wrong length, or an id out of range. `--template` is used only
  to write the master's size into `MAST`/`DATA` (0 without it).

| module | job |
|---|---|
| `input` | serde types for JSON v1 (`deny_unknown_fields` off, `version == 1` checked), the hex-rows and numbers terrain decoders |
| `grid` | tile ↔ world ↔ cell ↔ vertex transforms, footprints, massif squares |
| `passable` | DT's on-foot map (world.md §1.1) |
| `heightmap` | §3.3 and §3.4, integer heights in units, normals, WNAM |
| `textures` | the LTEX table, per-cell VTEX with the block order, the TGA writer |
| `mesh` | Collada text for boxes, slabs, trees, rocks (one function per shape) |
| `placement` | references per cell with stable FRMR indices |
| `actors` | CREA records from armies and garrisons, the stat formula |
| `tes3` | the writer: record and subrecord framing, the records of §2.2, cp1251 encoding; and a small reader of the same records for tests |
| `lua` | `map_data.lua` (a Lua table writer with string escaping), copies of the runtime scripts, `.omwscripts`, the cfg snippet |
| `lua/*.lua` | the fixed runtime: `global.lua` (state, clock, events, armies), `player.lua` (guard, windows), `army.lua` (AI, death), `menu.lua` |

- **Why our own TES3 writer.** crates.io's `tes3` is 0.1.0 from 2021.
  [Greatness7/tes3](https://github.com/Greatness7/tes3) is newer and says it reads and writes
  every esp structure, LAND included (`Plugin::save_path` **[R]**, not run by us). But it is
  unpublished (a git dependency, "APIs unstable, pin a commit" **[V]**), edition 2024 with Rust
  1.88, pulls rayon and glam, keeps a cell's references in a `HashMap` **[R]**, and covers far
  more than the 12 record types we emit. Our records are fixed-layout and small; a writer of a few hundred
  lines has no git dependency in a public repo's lock file. `esmtool` is the independent reader;
  [tes3conv](https://github.com/Greatness7/tes3conv) (plugin ↔ JSON) is a second one for debugging.
- **Determinism.** The same JSON and options give byte-identical files. Rules: no `HashMap`
  iteration (sorted `Vec`s or `BTreeMap`); one splitmix64 generator keyed by
  `(generator_seed, purpose, x, y, n)`; heights in `i32`; no `sin`/`cos` from the platform (yaw
  from a 16-entry table, bumps from polynomials); floats written with a fixed format; no
  timestamps, host names or paths in any output.
- **Keeping in step with the exporter.** A test in the root crate (a dev-dependency on
  `razdor-openmw`) exports a synthetic `Scenario` with `dtm_json::scenario_to_string` and parses
  it with `input`. A layout change that the converter cannot read fails there.
- **CI.** The workspace's own test and clippy steps cover the crate; it has no crate-only step
  (`cargo test --release --workspace`, `cargo clippy --workspace --all-targets`). CI has no OpenMW: tests that call `esmtool`
  or `openmw` skip when the program is missing.

## 5. Testing

**Unit tests, synthetic JSON only** (small maps built in the test code, no game content):
- `grid`: tile (0, 0) of a 50×50 map is at `(512, 50688)`; footprints of 4×4, 2×1 (extra row)
  and 1×1; a massif of side 2.
- `passable`: one tile of every surface code; a tree, a hill, a mountain and a thicket; a bridge
  on water is open; a mountain square reaching past the west edge is clipped.
- `heightmap`: every water tile centre is ≤ −200; a footprint is flat; shared cell edges are
  equal; all heights are multiples of 8; no neighbour difference above 1016; a mountain's side
  is steeper than 46° and a hill's is not.
- `textures`: the VTEX block order against a hand-written 16×16 example; LTEX index + 1.
- `tes3`: VHGT encode → decode gives the heights back; every record written is read back equal
  by the crate's reader; cp1251 round trip.
- `lua`: string escaping (quotes, backslashes, newlines, Cyrillic); the passability rows.
- Determinism: two runs give equal SHA-256 for every file.

**Round trip with OpenMW's tool** (skips without `esmtool`): `esmtool dump -C -e win1251
<name>.omwaddon` exits 0 and its output has the expected counts of LAND, CELL, LTEX and
references; `esmtool -r TES3 dump` lists the subrecords in the order of §2.2.

**Off-screen smoke test** (`tools/openmw-smoke/run.sh`, not part of `cargo test`): builds the
standalone config of §2.4 in the scratch folder, starts `Xvfb :<free display>` with its own
`XDG_RUNTIME_DIR`, runs OpenMW there with `LIBGL_ALWAYS_SOFTWARE=1` and a test-only
`smoke.omwscripts` whose player script, 10 seconds after the start, prints
`RAZDOR_SMOKE tile=<x>,<y> z=<height>` to the log and, 5 seconds later, calls `core.quit()`
(made for tests **[V]**). The harness waits for the line (a bounded poll), takes the picture with
`xwd -root` on that display (OpenMW has no script call for a screenshot **[R]**), waits for the
exit, then stops Xvfb. Pass: the line's tile is the hero's start tile, the picture is not one
flat colour, and the log has no `Error` line naming our files. OpenMW's CI runs under Xvfb
**[R]**; llvmpipe on this machine is **[U]** until M4 runs it. If it is too slow, the RTX 4060 is used
headless (EGL), as Q10 allows.

**Machine rules** for every agent brief: heavy work runs as `CARGO_BUILD_JOBS=12
$(ai-resources prefix) cargo …` with `CARGO_TARGET_DIR=/mnt/ai_models/razdor-scratch/openmw/target`;
OpenMW runs under the same prefix; never a window on display `:0`; converter output, the Template
clone and screenshots stay under `/mnt/ai_models/razdor-scratch/openmw/`; tests are
`cargo test --profile fasttest -p razdor-openmw` plus the clippy line of §4; long jobs write a
done-file and are waited for with a bounded poll; scratch is removed when the task ends.

## 6. Milestones

Each is one agent task on branch `openmw-2026-11-27`, with its tests and clippy clean. Each
checkpoint is committed on its own (Q11: no pinned Template commit).

| | work | accepted when |
|---|---|---|
| **M1** | The crate, `input`, `grid`, `tes3` writer and reader for TES3, LTEX, CELL, LAND. Flat land at +96, one texture per surface (generated TGAs), no margin. Also: clone the Template to scratch (it uses git LFS), dump `template.omwgame` and list its record types in the crate's README. | The unit tests of `input`, `grid`, `textures`, `tes3` pass. For a synthetic 16×16 map: `esmtool dump -C` exits 0 and shows 4 LAND, 4 CELL and 18 LTEX. A second synthetic map with one raised vertex and one odd texel proves the row and column orientation in `esmtool`'s output. |
| **M2** | `heightmap`, `passable`, the margin ring, normals, WNAM. | The `heightmap` and `passable` tests pass. The sample export converts in under 5 s and `esmtool dump -C` reads its 81 LAND records. Two runs are byte-identical. |
| **M3** | `mesh`, `placement`: scenery, building boxes, doors, bridge decks, STAT, ACTI, LIGH records. | Reference counts in `esmtool dump -C` equal the map's: one per tree, dead tree, thicket, stone massif, mountain massif, one building per building (no door, Q6), one deck per bridge. Every reference lies inside its cell. A unit test walks a diagonal chain of three bridge tiles and finds overlapping decks and a land edge at deck height at both ends. Every `.dae` parses as XML. |
| **M4** | `actors`, `lua` data, `global.lua` with the clock, the start teleport, the party, army spawn, patrol, defeat and respawn; `player.lua` with the guard. | Unit tests for the stat formula and `map_data.lua`. Under the smoke harness: the player stands on the start tile; the log reports the active armies spawned, none of the inactive ones, and each ship army spawned on the water surface; no body stands inside a building box or on a closed tile; a scripted teleport onto a water tile is undone by the guard within a second. |
| **M5** | The event engine for the v1 column of §3.11, windows, journal records, the hall list, victory and defeat. | A Lua test file run by the smoke harness drives a synthetic map through: a global event with a message, a yes/no question each way, a flag set and tested, a relative event after a delay, a chain, an army activated and then defeated, a quest taken and completed (the journal has both stages), the victory event. The log has one `RAZDOR_EVENT <id>` line per firing, in the expected order. |
| **M6** | `tools/openmw-smoke/run.sh`, the standalone config, `docs/flows/testing-and-tools.md` section, the settled **[U]** items written back into this spec. | The smoke test passes on the sample map from a clean scratch folder with one command, and leaves a screenshot showing land, water and at least one building. No process is left running. |

M1 to M3 need no running OpenMW. M4 needs the smoke harness in a first form, so the harness's
start-and-log part may move from M6 into M4; M6 then adds the screenshot and the docs.

## 7. Open questions for the owner

| | question | recommended default | owner's answer (2026-11-27) |
|---|---|---|---|
| Q1 | Tile size: 512, 1024 or 2048 units? | 1024 (§3.1); the option stays. Tile 1024 stays, and the grid and map size stay as DT's. Towns, castles and other buildings are Morrowind-scale (a town is a cluster of houses, a castle a proper castle), centred on the DT footprint; they may extend past it and the flat ground grows to fit (§3.3, §3.7, M3). |
| Q2 | The clock: real time at a fixed scale, or DT's rule that time passes only while the hero moves? | Real time, scale 120. The step rule would freeze the armies whenever the player stands still. Real time, game time scale 120 (§3.11). |
| Q3 | Unit stats: a formula from level and count, or a table exported from the install's ini data? | The formula in v1. A `razdor-units-json` exporter later (its output is game data, never committed). The formula for the prototype; a units exporter later. |
| Q4 | Blocked ground: let the player swim and climb, or hold him to DT's passability? | Hold him (the guard), because maps gate their quests with rivers and thickets. Ships come after v1. Ships come later. Armies with a ship float on the water surface for now, not skipped (§3.5, §3.8, M4). |
| Q5 | Battles: OpenMW's real-time fight with one body per squad, or something closer to DT's tactical battle? | Real time with bodies in v1; it is a placeholder for the look, not for the rules. Real-time fights. Every DT unit card in an army is its own OpenMW actor (a troop of count N is N actors), shown and moving together as a squad (§3.8). |
| Q6 | Building interiors: a window on the door, or real interior cells? | A window in v1 (events, quests, rumours). Hiring, markets and healing are not ported yet. No door window. Stepping into a building's footprint fires its events (§3.7, §3.11). |
| Q7 | Fog of war: none, or a Lua overlay on the map? | None in v1; lanterns become beacons. No fog of war and no lanterns. Lantern beacons are not converted (§3.10); points stay for their events (an assumed reading). |
| Q8 | Campaign: load the next map after a victory? | No in v1: one map per run, the end window names the next map. One map per run. |
| Q9 | Russian text: do the fonts in use draw Cyrillic, and are the Template's `mygui` files needed? | Unknown until M6. If needed, the harness overlays them on a scratch copy of the resources folder and adds a free Cyrillic font there; nothing under `/usr/share` changes and the converter ships no font. OpenMW and its resources are not modified. Only investigate whether Cyrillic renders (§2.3, M6). |
| Q10 | May the smoke test use the RTX 4060 (headless EGL) when llvmpipe is too slow? | Start with llvmpipe on CPUs 8–19; ask before using a GPU. The smoke test may use the RTX 4060 (headless EGL) when llvmpipe is too slow (§5). |
| Q11 | The Template is cloned from GitLab at test time. Pin a commit? | Yes: the harness pins one commit id and keeps the clone in scratch. No pinned Template commit. Commit at every checkpoint and milestone (§6). |
