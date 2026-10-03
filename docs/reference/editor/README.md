# How the original map editor works

A specification of the original Discord Times map editor, read from `DTMapEdit.exe` of the
Community Update (Delphi, image base 0x400000), cross-checked with its forms, its ini files, the
editor manual and the shipped maps. Written in our own words for interoperability: formulas,
tables and prose only. No code, disassembly or game text is reproduced. Addresses are virtual
addresses in that build, given as evidence so a claim can be rechecked.

Confidence tags: **code** (read in the code), **data** (consistent with the data files, the
manual or the shipped maps, not traced), **unknown** (not determined).

| File | Covers |
|---|---|
| [main-window.md](main-window.md) | Start-up, the menus and tools, mouse and keys, scrolling, drawing order and overlays, the minimap, the brush and placement routine with every default, pick-up and drop, deleting and what it renumbers, burning, quick save, the sorted lists |
| [records.md](records.md) | Every record window: army, building (pages by type), unit editor and its cost formula, artefact editor and its auto-price, named characters, scenario parameters and hero presets, the event editor (defaults, copy, move, delete), points and AI targets, options |
| [newmap.md](newmap.md) | The new-map generator: seeding, the height fractal, blur, the terrain classes by weights, map types, rivers, the coast band, swamp, mountains, hills and forests, in draw order |
| [worldgen.md](worldgen.md) | The world generator: buildings and roads by sector, bridges, junction buildings, ruins, the economy, armies and garrisons, and the random army builder |
| [mapcheck-files.md](mapcheck-files.md) | The map check (20 rules), what loading and saving change, the file variants by extension, the text dump, the playability score, the editor's other files |
| [testers.md](testers.md) | The battle tester and the AI viewer, and every rule where the editor's copies of the battle engine and world AI differ from the game |

Each file ends with a **"Razdor editor now → original"** table: the work list for Razdor's
editor. The map file format itself is in [../dtm-format.md](../dtm-format.md).

## How far this is verified (status 2026-10-03)

Razdor's editor on `dt-original` was rebuilt from these specs in six reviewed steps (files and
map check, record windows, main window and tools, new-map generator, world generator, testers).
As with the game, this is static reading plus tests from the specs' numbers: no map was made in
the original editor and compared cell by cell with Razdor's, so "same map for the same seed" is
the aim the code follows, not a measured fact. The specs still list 34 unknowns, most of which
need the original editor running to settle. `dt-feat` does not have this editor work yet; carrying
it over (fixing the original editor's bugs there instead of copying them) is the next step.
