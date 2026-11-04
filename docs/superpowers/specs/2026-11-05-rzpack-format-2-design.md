# `.rzpack` format 2: design

Status: spec only (2026-11-05). The owner put the work on hold: "let's do that later, just write specs".
Nothing here is built. The case for it is in `docs/ideas/rzpack-for-agents.md` (§1 rows 1, 5, 6; §2 C,
E, F). Steps A, B, D and G of that plan are on `v2-packages`. This spec covers the rest: scoped names,
scenery out of `map.toml`, texts per map, the converter, and what the studio must change.

## 1. Goals and non-goals

Goals:

- A name written in one map's files can never mean another map's army, building or dialog by accident.
- `maps/<id>/map.toml` holds only what a story touches, a few hundred lines, so an agent can read it whole.
- Parallel authors (people or agents) working on different maps never edit the same file.
- Every format 1 package keeps working unchanged, and converts to format 2 losslessly with one command.

Non-goals:

- No change to the trigger model (when / if / then), the actions, the events, the dialog model,
  `campaign.toml`, units, items, scripts or tests.
- No change to what the engine runs: format 2 is a loader change. The compiled story, maps and names
  of a converted package are identical to the original's.
- No change to the original game's maps (`.DTm`) or their scripts.

## 2. Today (format 1), measured on «Пепельная Марка» 0.3.1

| Part | Today | Problem |
|---|---|---|
| Triggers | `triggers/<map>.toml`, each trigger with `map = "<id>"`; without `map` a trigger applies to every map. | The scope is per trigger, written by hand, and easy to forget. |
| Dialogs | `dialogs/<map>.toml`; dialog ids are package-wide. | Two maps cannot both have a dialog `greet`. |
| Armies and buildings | Declared per map in `map.toml`. | A trigger names them by bare id. |
| Spawns | `spawn_army = "<name>"` in any trigger; spawned names are package-wide and win over a map's army (`triggers::check_map_refs` now errors on a clash). | The `kord_band` bug: the camp's spawn took over Marka's army. |
| Scenery | `[[object]]` tables in `map.toml`: on `marka`, 2,699 of the file's ~13,700 lines' tables. Across the package: ~11,600 `trees`, ~11,200 `dead_trees`, ~2,300 `stones`, ~500 `thicket`, ~100 `hills`, 5 `mountains`; some with `sprite = n`. | The ~25 entities that matter are buried; agents read the file in slices. |
| Texts | `texts/ru.toml`, `texts/en.toml`: one file per language, all maps (~1,560 lines in `ru`). | Every parallel merge conflicts there. |

## 3. Format 2

`package.toml` says `format = 2`. The engine reads `format = 1` exactly as today, and `format = 2` as below.
`format` greater than the engine's own is refused, as today.

### 3.1 Layout

```
package.toml
campaign.toml
units/*.toml, items/*.toml, scripts/*.lua, tests/*.toml     (unchanged)
vars.toml                       the package's [vars] (all story variables, declared once)
common/
  triggers.toml                 triggers that apply to every map (rare)
  dialogs.toml                  dialogs any map may open (rare)
maps/<map>/
  map.toml                      settings, hero, armies, buildings, areas, victory
  scenery.toml                  trees, rocks, decorations (compact, see 3.3)
  terrain.png                   (unchanged)
  triggers.toml                 this map's triggers (no `map` key)
  dialogs.toml                  this map's dialogs
texts/<lang>/
  common.toml                   package, units, items, common dialogs, campaign texts
  <map>.toml                    one map's texts
```

- A map is a folder that holds everything about that map. The per-trigger `map = "…"` key is gone in
  format 2: a trigger's map is the folder its file is in. `common/triggers.toml` replaces "no `map` key".
- `[vars]` move to `vars.toml`, one place, because variables are the campaign's carry and are package-wide.

### 3.2 Scoped names

Every army, building, area, spawn and dialog id belongs to a scope: a map, or `common`.

- **Resolution.** In a map's own files (`maps/<m>/triggers.toml`, `maps/<m>/dialogs.toml`) a bare id is
  looked up on map `m`, then in `common`. In `common/*`, a bare id is looked up in `common` only, and a
  map's things are named qualified.
- **Qualified names.** `<map>:<id>` names another map's army, building, area or dialog. That is rare,
  because the carry between maps is by variables. `common:<id>` is accepted and means the same as a
  bare id resolved to `common`.
- **Spawns.** `spawn_army = "x"` in map `m`'s triggers creates `m:x`; it may not reuse the name of an army
  declared on `m` (error), and may reuse a name another map uses (fine: different scope).
- **Same id on two maps** is allowed: two maps may each have a `patrol` and a dialog `greet`.
- **Variables** stay package-wide and are never qualified.
- **Errors.** `check` errors on a bare id that is not on the map nor in `common`, with the closest ids on
  that map as the hint (the did-you-mean of step D), and on a qualified id whose map or id is unknown.
- **Runtime.** Unchanged. The compiler already turns names into indices per opened map; it resolves with
  the rules above. A save stores indices and uids, not names, so saves are unaffected.

### 3.3 Scenery

`maps/<m>/scenery.toml`, one line per kind, cells as `[x, y]` pairs, in map order:

```toml
trees      = [[4,30],[7,33],[10,28],[5,40]]
dead_trees = [[12,37],[9,46]]
stones     = [[4,9]]

# Objects with more than a kind and a cell (a sprite, a size) stay as tables:
[[object]]
kind = "trees"
cell = [40, 12]
sprite = 8
```

- The reader merges the kind lines and the tables into the same list of objects as format 1 reads today,
  in the same order: first each kind's line in the file's order, then the tables. The converter writes the
  file so that this order equals the original order. If the order matters to drawing, the converter keeps
  the original order and writes every object as a table: see open question 3 in §7.
- `map.toml` in format 2 rejects `[[object]]` (an error pointing to `scenery.toml`).
- Expected size on `marka`: `map.toml` from ~13,700 lines to a few hundred; `scenery.toml` about 20 lines,
  long ones.

### 3.4 Texts

- `texts/<lang>/<map>.toml` holds the keys that map's files use; `texts/<lang>/common.toml` holds the rest:
  the package, units, items, the campaign, common dialogs, and any key used by two or more maps.
- Keys are unchanged (`marka.army.kord_band.name`, `story.title` …): only the file they live in changes.
- A key is defined once per language: a key in two files of the same language is an error.
- `check` still reports a key missing in a language, and an unused key as a warning (as today).
- The loader builds the same `texts: lang → key → text` map as today, so nothing downstream changes.

## 4. The converter: `razdor-pack upgrade`

`razdor-pack upgrade <folder> [--dry-run] [--out <folder>]` turns a format 1 package into format 2.

1. Refuse if `check` has errors (convert only a clean package), or if `format` is already 2.
2. Triggers: move each trigger to `maps/<map>/triggers.toml` by its `map` key (drop the key); a trigger
   without `map` goes to `common/triggers.toml`. Keep each file's comments and order (`toml_edit`).
3. Dialogs: a dialog goes to the map whose triggers or dialogs open it. A dialog opened from two or more
   maps, or from none, goes to `common/dialogs.toml`.
4. `[vars]` from every trigger file to `vars.toml`.
5. Names: rewrite every reference so that it resolves to the same thing under §3.2. A bare reference that
   format 1 resolved to another map's army, or to a spawn made on another map, becomes `<map>:<id>`. Report
   each rewrite.
6. Scenery: `[[object]]` of each `map.toml` into its `scenery.toml` (§3.3).
7. Texts: split each language by which map's files use each key (§3.4).
8. Set `format = 2`. Run `check` on the result. Compile every map of the original and of the result and
   assert they are identical: the same compiled story (triggers, dialogs, names, vars), the same map
   objects in the same order, the same texts per language. If not, write nothing and print the difference.

`--dry-run` prints the plan (files written, references rewritten, keys moved) and writes nothing. Without
`--out` the package is rewritten in place. The tool never deletes a file it did not move.

## 5. The studio

The studio reads and writes this format in many places. It is owned by the studio session, which adapts
it after format 2 lands. The parts that change:

| Studio part | Change |
|---|---|
| `doc.rs`, `edit.rs` (TOML edits by path) | New paths: a map's triggers and dialogs under `maps/<m>/`; `vars.toml`; `common/`. |
| `records.rs`, `story_graph.rs`, `campaign_graph.rs` | Read triggers per map folder instead of by the `map` key. |
| `characters.rs` (`rename_plan`), `query_refs` (MCP), `uses` | Resolve names per scope (§3.2); a rename on one map does not touch another map's same-named thing. |
| `texts.rs` | Read and write `texts/<lang>/<map>.toml` and `common.toml`; a new key goes to the file of the map being edited. |
| `map.rs`, `place.rs`, `brush.rs`, `paint.rs` | Scenery is read from and written to `scenery.toml` (the brush paints scenery). |
| `newpkg.rs` | New packages are made in format 2. |
| Health, scores | Unchanged in meaning; adjust paths in their findings. |
| Opening a format 1 package | Offer «Обновить формат» (runs the converter on a copy, then opens it), or open it read-write in format 1. |

## 6. Plan

One branch, in this order, each step committed with its tests green:

1. Loader reads format 2 (layout, `vars.toml`, `common/`, scenery, per-map texts), producing the same
   `Package` as format 1. Tests: a small format 2 package compiles the same as its format 1 twin.
2. Scoped resolution (§3.2) in the compiler and `check`, for format 2 only; format 1 keeps today's rules.
3. `razdor-pack upgrade` with the lossless assertion (§4), tested on `hello-story` and «Пепельная Марка».
4. Convert the examples; `check` 0 errors, `razdor-pack test` all ok, line counts recorded.
5. Docs: `rzpack-format.md` (format 2 section), `rzpack-editor-guide.md`, the studio's
   `data/studio/claude-skill.md`, `docs/flows/` where package layout is described.
6. The studio session adapts the studio (§5).

Estimated: steps 1–5 about a week; step 6 is the studio session's.

## 7. Open questions

1. `common/` dialogs and triggers: keep them, or forbid package-wide triggers in format 2 (every trigger
   belongs to a map, and a campaign-wide rule is written on each map)? Keeping them is simpler to convert.
2. Should a qualified reference to another map be allowed at all? Today «Пепельная Марка» may have none
   once `kord_band` is fixed; the converter will report how many it finds. If zero, consider forbidding it.
3. Does object order affect drawing or anything else? If yes, scenery keeps a table per object (bigger
   file, still out of `map.toml`).
4. Texts: one file per map per language, or one file per map with both languages side by side
   (`[ru] …`, `[en] …`)? Side by side is easier for a translator to keep in step; per language matches
   today and the studio's text tools.
