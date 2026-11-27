# razdor-openmw

Converts a Discord Times map into an OpenMW plugin. The input is the JSON of `razdor-dtm-json`,
format version 1 (`src/dt/dtm_json.rs` in this repo). The design is in
`docs/specs/2026-11-27-openmw-world.md`.

Milestone 2 writes the land of the whole map and of its one-cell margin ring (9×9 cells for the
50×50 sample, 81 LAND records). The heights come from the surfaces, the hills, the mountains, the
flattened building footprints, the bridge decks and a seeded noise (spec §3.3, §3.4). The normals
and WNAM follow from the heights. Each surface has one generated ground texture.

Milestone 3 adds the scenery and the buildings: one STAT per mesh, one CELL reference per tree,
dead tree, thicket, stone, mountain rock, building and bridge deck, and the Collada meshes they use
(`meshes/razdor/*.dae`, generated, no texture files). Trees and dead trees are jittered in their
tile with one of two shapes each; a stone sits on its massif's centre; a mountain gets one rock at
its foot. A building is one reference whose mesh holds all its parts: a castle is a curtain wall
with a gate, four corner towers and a keep; a town or village is 5 to 9 or 3 to 5 houses on a fixed
slot grid, so they never overlap. A bridge is one deck slab at the deck height. There are still no
ACTI or LIGH records and no doors (later, spec §3.10, Q6, Q7).

Milestone 4 adds the armies and the Lua runtime (spec §3.5, §3.8, §3.9, §4). Every unit card and every
leader of every army (active or not) is a CREA record: a leader is `rz_army_<id>` (named after the army),
a unit card `rz_u<unit>_l<level>`, shared between armies. The stats are the spec's placeholder formula
(`actors::stats`). The body model is the generated `meshes/razdor/figure.dae` (a box figure that does
not animate); `--template-assets` uses `land_racer.dae` instead (the Template's example folder must be
on the data path). A ship army's raft is a STAT reference in its cell, at z = 0.
The converter writes `scripts/razdor/map_data.lua` (the map, the passability rows, the buildings' boxes,
the start and the armies with their units), `<name>.omwscripts` and the runtime copied from `lua/`:
`common.lua` (tiles, RNG), `global.lua` (start, clock, spawn, checks, deaths, respawn, the guard's
teleport), `player.lua` (passability guard), `army.lua` (leader patrols, followers on AI Follow).

Options: `--archetype knight|archmage|ranger` (default: the map's first hero), `--time-scale N` (120),
`--no-guard`, `--template-assets`.

## Usage

```
razdor-openmw <map.json> -o <out dir> [--name <id>] [--tile-units 512|1024|2048] [--template <path to template.omwgame>] [--debug-heightmap <pgm path>] [--units <units.json>]
```

- `--units` reads the install's units JSON (`razdor-units-json`, version 1; game data, never committed):
  the CREA stats and names are the install's, mapped as in spec 3.8 ("units notes"). Without it the
  placeholder formula is used.

- Writes `<out dir>/<name>.omwaddon` (default name `razdor_map`), `<out dir>/textures/razdor/surf_*.tga`
  and `<out dir>/meshes/razdor/*.dae`.
- `--debug-heightmap` also writes the global height grid as a binary PGM (`P5`, one pixel per land
  vertex, north at the top, the lowest height black). The file is always PGM; convert it to PNG
  outside the crate. It holds numbers made from the game's map, so keep it out of git.
- The output is game content (the map's layout and texts). Never commit it. Keep it outside the repo.
- `--template` sets the master's size in the plugin header. Without it the size is 0.
- Exit codes: 0 done, 1 a file could not be written, 2 bad input or bad usage.
- Output is deterministic: the same map and options give byte-identical files (no timestamps, no
  hash-map order, integer arithmetic, a splitmix64 noise keyed by the map's `generator_seed`).

Tests: `cargo test --profile fasttest -p razdor-openmw`, and `cargo clippy -p razdor-openmw
--all-targets -- -D warnings`. The tests that call `esmtool` print a note and pass when `esmtool` is
not on `PATH`. The tests use synthetic maps only.

## The OpenMW Game Template

The plugin depends on `template.omwgame` only, which the Template ships as its one master.

- Source: <https://gitlab.com/OpenMW/example-suite.git> (git LFS). The dump below was made from the
  clone at commit `a41b44d9403ff3f8a1505c1b4bc152c4dd623b64`; the harness does not pin it (owner,
  Q11).
- The file was dumped with `esmtool dump -C` (and, the same count, `esmtool dump`). It has 1852
  records:

| record | count | record | count |
|---|---|---|---|
| GMST | 1521 | SKIL | 27 |
| MGEF | 143 | SOUN | 107 |
| DIAL | 28 | STAT | 5 |
| GLOB | 10 | SCPT | 2 |
| BODY, CELL, CLAS, DOOR, LAND, LTEX, NPC_, RACE, REGN | 1 each | | |

The Template has one LAND and one CELL at (0, 0), and one LTEX (`rock_01`, index 0). Our plugin
writes its own LTEX table, and its own CELL and LAND at the same cells. That the later plugin wins
for the same cell is not checked yet (spec §2.3); the OpenMW smoke test in a later milestone checks it.

## Orientation (esmtool)

`esmtool dump -C` prints each LAND's cell coordinates and its `MinHeight` and `MaxHeight`. It does
not print the VHGT deltas or the VTEX values. The M1 probe (one vertex raised to 800 at row 1, column
2 of cell (0, 0), one texel changed at row 0, column 3) shows the raised height in cell (0, 0) and
in no other cell. It cannot show the row or column order.

The crate assumes row 0 = south and column 0 = west, for both the vertex grid and the texel grid
(spec §2.2, marked unverified). The reader in `src/tes3.rs` reads the plugin back with the same
rule, so it only checks the crate against itself. OpenMW's own behaviour is still to be confirmed
by the smoke test (milestone 4).

## Modules

- `units`: the units JSON (format and version checked, by id). Stats at a 0-based level, and the
  game's percent growth for the protections. The mapping to CREA stats is in `actors::real_stats`.
- `input`: reads the JSON (format and version checked), the objects and buildings, and decodes the
  terrain (`hex-rows` or `numbers`). `kept_objects` applies DT's one-massif-and-one-plant-per-tile rule.
- `grid`: tile, world, cell and vertex coordinates, footprints.
- `passable`: DT's on-foot map (world.md §1.1) per tile: surface values, hills, plants, massifs,
  building footprints at road cost. Computed and tested in M2; the player's guard reads it from M4.
- `heightmap`: the surfaces' base heights, the coast ramp, the margin ring (deep sea next to water,
  a rock ridge next to land), the bilinear blend and two blurs, the flat footprints (footprint plus
  a one-tile apron), the bridge decks, hills, mountains, noise, quantising to 8 and clamping the
  neighbour steps to ±1016. Also the normals and the PGM picture.
- `textures`: the 18 LTEX entries, the VTEX block order, the TGA writer.
- `mesh`: the placeholder shapes as Collada 1.4.1 text (one colour per part, flat normals, Z-up, units
  as OpenMW's). The shapes are tree (two kinds), dead tree, thicket, stone and rock by massif size,
  bridge deck, castle, palace, fort, town and village houses, tavern, church, smithy, market,
  shipyard, altar, obelisk, dungeon entrance and ruins. Each builder counts its parts (`pieces`),
  and tests check them. Integer shapes only: an 8-gon table, no `sin`/`cos`.
- `placement`: the references per cell, in the order of the kept objects and then the buildings;
  the yaw table (16 entries from π); the town slot grid and its hash-chosen houses; the ground
  height at a point (bilinear over M2's land).
- `tes3`: the TES3 writer and reader for the records of M1 and M2, and the Windows-1251 strings.
- `events`: the scenario for `map_data.lua` (the exporter's `events`, `points`, building zones with
  their footprint tiles, the info, the hero's party and gold, the army units), written as Lua literals
  with the JSON names (`lua_value`), and the journal records: a DIAL `rz_q<id>` (type journal) with two
  INFOs, index 10 (taken) and 100 (finished, `QSTF`).
- `convert`: the map to plugin, textures and meshes; one CELL and one LAND for every cell of the map
  and of the margin ring, the CELLs of the references too, and one STAT per mesh (after LTEX).

## Milestone 6 notes (the one command, the windows' backdrop)

- `textures/razdor/panel.tga`: a 4x4 dark texel the event windows stretch behind their text (85%
  opaque, 18 px padding, the text 1 to 4 sp larger than in M5). The converter writes it with the ground
  textures; `convert` tests count it.
- Russian text draws in the Template's own fonts: the event windows show the Russian text of the RK1
  sample as written, so no Morrowind `mygui` files and no extra font are needed (spec Q9).
- `tools/openmw-smoke/play.sh` runs the whole pipeline from a `.DTm` or `.json` in one folder (spec 6).

## Milestone 5 notes (events, windows, journal)

- Lua: `lua/events.lua` is the engine (pure logic, reaching the world through `env`); `global.lua`
  builds `env` and runs the clock (a scan every 30 DT minutes), the meetings (within 1024 units of a
  leader) and the army registry; `player.lua` sends each tile change as `RazdorStep`, builds the
  windows (`RazdorShow`/`RazdorClose`) and sends `RazdorAnswer`; `menu.lua` (MENU script) quits.
- Log lines: `RAZDOR_EVENT <id>` once per firing, `RAZDOR_ANSWER <id> <yes|no|ok|quit>`,
  `RAZDOR_VICTORY <id>` / `RAZDOR_DEFEAT <id>`, `RAZDOR_JOURNAL rz_q<id> stage=<n>`, `RAZDOR_HOLD <army>
  fight=<n>` (a waiting meet event holds a hostile army at fight 0), `RAZDOR_MEET <army>`,
  `RAZDOR_SQUAD <army> k= x= y=` (body positions at the check), `RAZDOR_NOTE` for results not played.
- The journal index is the second field of the INFO's DATA (bytes 4..8). OpenMW 0.51 rejects `INDX`
  and `INTV` as "Unknown subrecord" (esmtool), but `addJournalEntry(10)` and `(100)` find their entries
  when DATA holds the index there (checked in the game). The first try (index in DATA bytes 0..4)
  gave "unknown journal index".
- Not played (logged as `RAZDOR_NOTE`): counter flags (`^`), `cast_spell`, lanterns (Q7), delay. Patrol,
  show army, move to hero, battle, artifacts, spells and class change
  are played since the follow-up below.
- Deviations: no frame (the Template has no menu textures, so the windows use a plain backdrop instead,
  see M6 below); `Герой` stands in for the hero's name; a unit's `named` test reads the party's names only.

## Pause and tracked results (M5 follow-up)

- An event window pauses the world (`world.pause('razdor_window')` in `env.show`, `unpause` in
  `env.close`): the clock, the scans, the armies and the meetings wait while it is open. Test drivers
  answer their windows on real time (`onFrame` and `core.getRealTime`), because simulation timers stop
  with the world.
- Results played and kept in the saved event state (`s.radius`, `s.hostile`, `s.artifacts`, `s.spells`,
  `s.class_unit`), each logged:
  - `patrol_army` + `patrol_delta`: the radius becomes `max(0, radius + delta)` (`RAZDOR_PATROL <id> radius=N`).
    The leader's destinations use it; an army without patrol on (`patrols`) still does not move.
  - `show_army`: the army is activated if it is off the map, then logged (`RAZDOR_SHOW_ARMY`). This is an
    approximation: DT only scrolls the view there.
  - `move_to_hero`: a spawned army's bodies go to the first open tile next to the hero (`RAZDOR_MOVE_TO_HERO`).
    A ship and an army off the map are not moved. DT picks the cheapest of the 8 cells; this takes the first.
  - `start_battle_with`: the army is hostile from now on (saved; a reload and a respawn keep it), activated
    if needed, moved next to the hero, and its fire is held at 0 by a waiting meet event (`RAZDOR_BATTLE`).
    The battle is not fought: it is played at once, not queued.
  - `artifacts_add` / `artifacts_remove` (the hero's pack, `RAZDOR_HERO artifact+|- <id>`), `spells_learned`
    (`RAZDOR_HERO spell+ <id>`, once each), `new_hero_class` (`RAZDOR_HERO class unit=<id>`; only the unit id is
    kept, the archetype tests still use the start class: open).
  - The artifacts condition reads the pack; codes 2-5 (the AI armies' worn items) and code 0 fail, since the
    worn items are not tracked. The hero's start artifacts and spells are seeded (M7 notes).
- The M5 synthetic scenario has events 17-26 for these results; `tools/openmw-smoke/m5/EXPECTED.txt` lists the
  order and the log lines they must leave (`run.sh --expect`).

## Milestone 4 counts on the RK1 sample (game-derived; not committed)

`esmtool dump -C` on the converted sample: 23 STAT (21 scenery and building meshes, the figure and the
raft), 81 LAND, 81 CELL, 18 LTEX, 30 CREA (15 army leaders and 15 unit cards) and 1127 references (the M3
1126 and the ship's raft). Armies: 15 in the map, 2 active (army 1 on land, army 9 a ship on the shallows,
no patrol); the rest are skipped as inactive. Army 15 starts on a closed tile and is moved to the nearest
open tile outside the boxes. Two runs give byte-identical files (48 files, SHA-256 compared).

Smoke run (`tools/openmw-smoke/run.sh`): `RAZDOR_START tile=39,43` and the harness's `RAZDOR_SMOKE` at
the same tile; `RAZDOR_ARMY_SPAWNED 1 actors=4` (leader + 3 unit bodies) and `RAZDOR_ARMY_SPAWNED 9
actors=2`; the skips for armies 2 to 8 and 10 to 15; `RAZDOR_CHECK` finds no body in a building box or on a
closed tile. A probe onto a water tile is undone by the guard in about a quarter of a second
(`RAZDOR_GUARD`, back to the start tile).

## Milestone 3 counts on the RK1 sample (the sample file is game-derived and not committed)

`esmtool dump -C` on the converted sample: 21 STAT, 81 LAND, 81 CELL, 18 LTEX and 1126 references
(645 trees, 52 dead trees, 368 thickets, 21 stones, 28 mountain rocks, 4 villages, 2 castles, 1
tavern, 1 church, 2 ruins, 2 bridge decks). The kept-object count of the sample is 1201, the same
as its raw count. Two runs give byte-identical plugins and meshes.

## Milestone 7 notes (hall list, army strength, hero kit)

- Hall: an ordinary building (not a village or shipyard) that fires nothing on the step opens its hall: the
  quests (kind 3) and rumours (kind 4) that pass the full check, as buttons («Взять», «Слух»), then Close.
  `RAZDOR_HALL <zone> quests=n rumours=m`, and `RAZDOR_HALL take <id>` when one is picked (the event's own
  `RAZDOR_EVENT` follows). Spec 3.11 "Hall" and events.md §10.
- Army strength (events.md §4 condition 11, experience.md §1): `stats_check` gates the `army_strength` test
  (signed, as the others). The strength is the sum of the unit costs of every record, the dead included, so
  it is fixed by the army's units. The party is the hero's units (not the hero). The costs are in
  `map_data.lua` (`costs`, from `events::unit_costs`): the install's `cost` from `--units` (at least 1), else
  the placeholder 50 per unit. `army_units[id]` holds the leader and each troop line's count. The log at start:
  `RAZDOR_STRENGTH party=n` and `RAZDOR_STRENGTH army=<id> n=<n>`.
  Deviation: units.json's `cost` is the price, not DT's tactical cost (experience.md §1 needs the full stat
  block and the cost multiplier, which the file does not carry). The unit's level is not in its cost. Both are
  open (spec Q-list); the test runs on these numbers.
- Hero start kit: the chosen hero's `artifacts` and `spells` (JSON `heroes`, one entry per copy) are written
  into `hero` and seeded into the tracked pack and spells at start: `RAZDOR_HERO start archetype=… gold=…
  mana=… artifacts=n spells=n`.
