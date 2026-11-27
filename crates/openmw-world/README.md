# razdor-openmw

Converts a Discord Times map into an OpenMW plugin. The input is the JSON of `razdor-dtm-json`,
format version 1 (`src/dt/dtm_json.rs` in this repo). The design is in
`docs/specs/2026-11-27-openmw-world.md`.

Milestone 2 writes the land of the whole map and of its one-cell margin ring (9×9 cells for the
50×50 sample, 81 LAND records). The heights come from the surfaces, the hills, the mountains, the
flattened building footprints, the bridge decks and a seeded noise (spec §3.3, §3.4). The normals
and WNAM follow from the heights. Each surface has one generated ground texture. There are no
objects, no references and no scripts yet (M3 and M4).

## Usage

```
razdor-openmw <map.json> -o <out dir> [--name <id>] [--tile-units 512|1024|2048] [--template <path to template.omwgame>] [--debug-heightmap <pgm path>]
```

- Writes `<out dir>/<name>.omwaddon` (default name `razdor_map`) and `<out dir>/textures/razdor/surf_*.tga`.
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
- `tes3`: the TES3 writer and reader for the records of M1 and M2, and the Windows-1251 strings.
- `convert`: the map to plugin and textures; one CELL and one LAND for every cell of the map and of
  the margin ring.
