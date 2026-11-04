# Building a GUI campaign editor on `.rzpack`

What an editor of Razdor content packages has to know: the data model, the references between
its parts, how to read, write, check and test a package, and the design traps the reference
campaign «Пепельная Марка» ran into. The field-by-field syntax is in
[`rzpack-format.md`](rzpack-format.md) and is not repeated here. The design behind the format is
[`docs/superpowers/specs/2026-11-02-v2-packages-design.md`](../superpowers/specs/2026-11-02-v2-packages-design.md).

Worked examples: `examples/packs/hello-story/` (one map, small) and
`examples/packs/pepelnaya-marka/` (seven maps, a branching campaign, a Lua battle).

## 1. What a package is

A package is a **folder of TOML files and PNG pictures**. A `.rzpack` is the same folder zipped
(`razdor-pack pack`). An editor works on the folder, the source of truth, and packs it only to
publish. Everything the engine reads is text, so it diffs and merges in git, and an editor and a
hand edit can live side by side.

```
package.toml            id, version, format, title/description (text keys), author, license, base
campaign.toml           optional: the map graph, what carries between maps
texts/<lang>.toml       key → text, one file per language (ru, en, …)
units/*.toml            the package's own units (≤ 55)
items/*.toml            the package's own items (≤ 56)
maps/<id>/map.toml      a map: hero, buildings, armies, objects, lanterns, victory
maps/<id>/terrain.png   the map's ground: one pixel per cell, one colour per terrain kind
triggers/*.toml         [vars] and [[trigger]]: the story's logic
dialogs/*.toml          [[dialog]] with [[dialog.choice]]: the story's windows
scripts/*.lua           optional: field battle rules (sandboxed Lua)
```

The game never ships in a package: `dt:` aliases refer to the player's install (units, items,
spells, portraits). A file with a game extension (`.ini`, `.dtm`, `.ugs`, `.lit`, …) is an error.

## 2. The data model

```
Package
├── Manifest (package.toml)
├── Texts: lang → key → string
├── Units[]  (id, base dt: unit, stats)         ← referenced by troops, give_unit, speaker
├── Items[]  (id, …)                            ← referenced by give_item, has_item
├── Vars[]   (name, initial bool|int)           ← referenced by set/add/var conditions, campaign ifs
├── Triggers[] (id, map?, when, if[], then[], once, chance)
├── Dialogs[]  (id, title, text, speaker, choices[] (id, text, if[], then[]))
├── Maps: id → Map
│   ├── header: title, description, start_date, relations[4][4], terrain_palette
│   ├── hero: cell, classes, gold, mana, troops[]
│   ├── terrain.png (50/100/200 square)
│   ├── objects[]   (kind, cell, sprite?)
│   ├── buildings[] (id, type, name, cell, size?, faction, income, gold_max, garrison[], recruit[])
│   ├── armies[]    (id, name, cell, faction, model, ship, gold, troops[], leader?)
│   ├── points[]    (id?, cell, radius, active)          lanterns; an id makes it a travel entry
│   └── victory     (defeat_all | defeat_army | capture | reach)
└── Campaign: start, title, carry{hero,army,pack,gold,vars}, maps[] (id, revisit, next[] (to, if[]))
```

The in-memory form is `pack::load::Package` (`src/pack/load.rs`). Each parsed entry is wrapped
in `Located<T>`, which holds the file and 1-based line it came from, so an error can point at the
source.

### Identity and scope

| Thing | Id rule | Scope |
|---|---|---|
| map | folder name, `[a-z0-9-]+` | package |
| unit, item | `[a-z0-9_]+` | package; `dt:<alias>` for the base game |
| var | name in `[vars]` | package-wide, shared by every map; carried by the campaign |
| trigger | `id` | package; `map = "<id>"` limits it to one map, none means every map |
| dialog | `id` | package |
| dialog choice | `id` | its dialog |
| quest | `quest = "<id>"` in a `quest` action | package (the diary's entry; it is not a trigger or a dialog) |
| building, army, point | `id` | its map; triggers name them per map |
| named character | `named = "<id>"` in give_unit / leader | package, numbered by first mention (≤ 32) |
| text key | dotted string | per language file; every language should have every key |

### The reference graph

An editor needs this for autocomplete, safe rename, safe delete ("used by 3 triggers") and
"go to definition".

| From | Field | Points to |
|---|---|---|
| any title/name/text/description | string | a text key |
| troops, garrison, recruit, leader, give_unit, add_units, spawn_army troops, has_named_unit `unit`, dialog `speaker` | unit | unit id or `dt:` alias |
| give_item, has_item | item | item id or `dt:` alias |
| cast_on_army, has_spell, remove_spell_from_book | spell | `dt:spell/<alias>` |
| set, add, random_var, var cond, var_changed, campaign `if` | var | `[vars]` name |
| dialog action, `ending`, dialog_choice event | dialog (+ choice) | dialog id / choice id |
| quest, complete_quest | quest | a quest id: `quest` actions of the package add it (any trigger or dialog choice); `complete_quest` only names one that some `quest` adds |
| hero_meets_army, army_defeated, army_arrives, army_at, army_alive, army_goal, army_speed, army_figure, remove_army, teleport_army, start_battle, set_faction/set_relation `of` army, holders `in = "<army>"` | army | army id of the trigger's map, or an id a `spawn_army` creates |
| hero_enters_building, building_captured/lost, building_owner, go_to_building, teleport_army `to`, `garrison = "<b>"` | building | building id of the trigger's map |
| victory `defeat_army` / `capture` | army / building | its map |
| travel `at`, campaign entry | point | `[[point]] id` of the target map |
| travel, victory `next`, campaign `next.to`, `start` | map | map id |
| start_battle `rules = "lua:<name>"` | script | `scripts/<name>.lua` |
| has_named_unit `named`, give_xp/give_item `to = "named:<id>"`, remove_unit | named character | a `named =` given somewhere |

A trigger without `map` is checked against every map, so its army and building names must exist on
each map where it can fire. Give every trigger a `map` unless it is truly global.

## 3. Reading and writing

**Reuse the engine's code.** `razdor` is a library crate. An editor written in Rust can call the
same functions the game uses:

| Need | Function |
|---|---|
| read a folder or `.rzpack` | `pack::files::Files::load(path)` |
| parse into the model, with problems | `pack::load::Package::parse(files) -> (Package, Report)` |
| all checks (`razdor-pack check`) | `pack::check(&package) -> Report` |
| compile a map against an install | `pack::compile::compile(&package, &install, lang, map_id)` |
| open a map as the game does | `pack::open::open(path, map, lang, &install)` |
| headless play | `rules::sim` (the `razdor-pack sim` code) |
| deterministic zip | `pack::archive` (`razdor-pack pack`) |
| the trigger vocabulary | `pack::triggers::{EVENTS, CONDITIONS, ACTIONS}` and the argument lists in `pack::triggers` |

An editor in another language (web, Tauri, Godot) can shell out instead:

- `razdor-pack check <path> --json` returns the diagnostics as JSON. Each one is
  `{severity: "error"|"warning", file, line|null, message, hint|null}`. Exit codes: 0 means no
  errors, 1 means errors, 2 means a usage or read problem.
- `razdor-pack sim <path> --map <id> --days N --json --trace [--choices …] [--set var=value]`
  returns the day-by-day story trace as data.
- `razdor-pack info <path>` gives the manifest, the maps, the size and the SHA-256.
- `razdor-pack pack <folder> -o out.rzpack` packs the folder.

**Write TOML that survives hand edits.** Authors also edit the files by hand, and the reference
maps carry comments. Write through a format-preserving TOML library (`toml_edit` in Rust; the crate
today only uses `toml` with serde, which drops comments) and change only the tables you edit. Keep
the file order the engine depends on:

- `units/` and `items/` order fixes their ids (200 + index), and saves store those ids. Never
  reorder or delete in the middle of a published package. Append, or raise the version.
- Inside a `[[dialog]]`, the `[[dialog.choice]]` order is the order shown.
- `[[map.next]]`: the first route whose `if` holds wins.
- `then` actions run in order.

Unknown fields are errors in `package.toml` and in the maps, so an editor must not invent keys.
Store editor-only data (canvas positions of graph nodes, colours, notes) in a separate file the
engine ignores, for example `.editor/layout.json`. A top-level folder other than the listed ones is
a warning and is not read, so put it there or outside the package.

Some map blocks are generated: the reference maps have `# --- generated by … ---` … `# --- end
generated ---` blocks written by `tools/pepelnaya-marka-dress.py`. An editor should keep them as
opaque runs of objects, or own the whole object list and drop the markers.

## 4. The map canvas

- **Coordinates.** `[x, y]` cells from 0 at the top left, x to the right, y down. A map is 50,
  100 or 200 cells square, taken from `terrain.png`'s size.
- **Terrain** is the PNG. Each pixel's exact RGB is one of 16 terrain kinds (palette in
  `rzpack-format.md`). `[terrain_palette]` may recolour kinds. Paint with the palette as a tile
  brush. Any other colour is an error. Water (fords included) is never walkable: a river crossing
  needs a `stone_bridge` or `wooden_bridge` building on each water cell of the crossing.
- **Buildings have footprints.** `cell` is the bottom-right cell. The footprint runs `w` cells left
  and `h` up, plus one extra row above when it is wider than tall (`rules::world::footprint_cells`;
  call it, don't reimplement it). Defaults per type are the original's (village 4×4, town 9×9,
  castle 4×4, fort/tavern/church/ruins 2×2, …). `size` overrides them. Draw the footprint, not the
  anchor. The hero enters from any footprint cell.
- **Placement errors to show live:** two footprints overlapping; a footprint off the map; the hero,
  an army or a lantern inside a footprint; anything but a bridge on water; an army on water without
  a ship; the hero or a town on water. Warnings: an object inside a footprint, a footprint on
  water or blocked ground, and **unreachable story cells** (`pack::reach`). A building entrance, a
  `hero_enters` cell or area, a teleport target, a goal or patrol cell, or a travel point that the
  hero's start cannot reach on foot is flagged. Run reach on every edit; it is cheap.
- **Overlays the canvas should offer:** `hero_enters` areas (`at = [x, y, w, h]`) as rectangles
  named after their trigger, with overlaps highlighted (two areas on one cell fire two dialogs at
  once). Army goals and patrols as arrows. Spawn points of `spawn_army`. Lantern radii.
- The canvas draws the compiled scenario with the game's renderer (`src/ui/studio/canvas.rs`);
  the old `.DTm` editor's model is gone.

## 5. The story editor

A trigger is a row: **When** (one event) → **If** (all conditions, in order) → **Then**
(actions, in order), plus `once` (default true) and `chance`. Every condition and action is an
inline table with one kind key and its arguments. That suits a form per kind generated from the
vocabulary tables in `pack::triggers` (the check already gives "did you mean …" for wrong keys).
Show the trigger's `map`, and filter the list by the map open on the canvas.

**Dialogs** are windows with up to five choices. Each choice has its own `if` (shown only then)
and `then`. The usual pattern is a choice with no actions plus a trigger
`when = { event = "dialog_choice", dialog, choice }` that does the work. The editor should show
those triggers under the choice they answer, as one unit.

**Variables** are declared once in a `[vars]` table (any triggers file) with an initial value whose
type (bool or int) fixes the type. A "find usages" view per variable (set by …, read by …) is the
main tool for debugging branching stories.

**A flow view helps.** Draw dialogs as nodes, choices as edges, and the variables they set as
labels, so a writer sees the branches.

## 6. The campaign graph

`campaign.toml` is a directed graph:

- **Nodes** are maps. `start` is the entry node. `revisit = true` keeps the map's state when the
  hero leaves.
- **Edges** are `[[map.next]]`, each with `to` and `if` conditions. On a victory, the first edge
  whose `if` holds is taken, so edge order matters. Show it as a numbered priority.
- **Hidden edges** live in triggers and must be drawn too: `{ travel = "<map>", at = "<point>" }`
  (leave mid-map), `{ victory = true, next = "<map>" }` (overrides the graph), and
  `{ ending = "<dialog>" }` (ends the campaign there).
- **Carry** (`[carry]`): hero, army, pack, gold and vars move between maps (all true by default).
  Variables are package-wide, so a chapter's choices reach every later map's triggers and
  dialogs, which is how branches work.

The check refuses a graph with no path from `start` to an end, a `next` to a map that isn't in the
campaign, a travel to a missing point, and conditions that name an army, a building or a place
(those belong to a map). It warns about a map nothing reaches.

## 7. Checking and testing from the editor

1. **On every save:** `pack::check` (or `razdor-pack check --json`). Each diagnostic has `file`
   and `line`, so the editor can jump to the row. Errors block packing and playing; warnings don't.
2. **Story trace:** `razdor-pack sim --map <id> --trace --json`. The AI plays the map for N days
   with the story attached and records which trigger fired, which variable changed, and which
   dialog opened, by day. `--choices dialog=choice,…` and `--set var=value` drive a branch.
   `--campaign` walks the graph. The sim's hero **does not walk**, so `hero_enters` triggers never
   fire there. `--auto-win-after` forces victories to walk the graph.
3. **Real play test:** the playthrough harness (`src/pack/playthrough.rs`, test-only today). The
   hero walks a route with the AI running, battles are fought by the battle engine, dialogs are
   answered by a script, and the carry is real (`playthrough_campaign.rs`). This is the only check
   that catches "the bandits attack before the dialog" or "this fight can't be won". Exposing it as
   a CLI (`razdor-pack route <path> <route.toml>` with a list of `walk_to`, `wait`, `hire`, `answer`
   steps and per-fight win rates over seeds) would give an editor a "test this route" button. Not
   built yet.
4. **Play:** `razdor --pack <folder> --map <id>` starts the game on the map. An editor can launch it
   as "Play from here".
5. **Pictures without a window:** `RAZDOR_PACK=<path> RAZDOR_SCENE=map:<id>` renders the map off
   screen (the `offscreen-snapshot` skill), which is useful for thumbnails.

## 8. Design traps (lint candidates)

Each of these broke «Пепельная Марка» in real play while its event-only tests were green. An
editor should warn about them. Most can be detected from the files alone.

| Trap | What happens | Lint |
|---|---|---|
| A story army with a hunting model (`bandits`, `ghosts`, …) and no goal | DT's AI hunts the hero from day one, before the story's dialog | An enemy army named by a `hero_enters`/dialog trigger and with no `army_goal` on `game_start`: suggest `go_to` its own cell |
| `hero_enters` area that doesn't cover its army | The army meets the hero before the area fires | The area should contain the army's cell plus a ring |
| Two `hero_enters` areas overlapping | Both dialogs open at once; a meeting can be skipped | Highlight the overlap |
| A neutral army (attitude 0) on the route | A step onto it is a fight, even with «Let pass» | Warn on neutral armies on reachable paths |
| A friendly army on the route or escorting | It stops the hero's walk each time unless the player turns on route-around | Warn on escorts; mention the option in the dialog |
| The hero standing still | An arriving AI army never attacks a standing hero (the original's rule); an `attack hero` goal waits beside him | Tell the author the fight starts on the hero's next step, or use `start_battle` |
| `start_battle` timing | A story battle is the only way to force a fight | Prefer it for set-piece battles (sieges) |
| An army sent into a town or tavern (`go_to_building`) | A sheltered army can't be engaged: a "beat it" victory never comes | Warn when the target of `army_defeated` can shelter |
| Victory by capturing a `town` | Towns aren't captured on a step | Use a castle or fort for a capture goal |
| Altars and churches as targets | Never a contact target: can't be fought or captured | Warn on `building_captured` of those types |
| Ghosts (and liches) in early fights | Near-immune to blows: a non-magic squad can't win | Warn when an enemy has ghosts and the hero's carried squad has no magic |
| `income` without the building being capturable or held | No gold ever arrives; the squad deserts (wages ~6–50/day per recruit, a week unpaid and they leave) | Show a per-map gold budget: income of reachable buildings minus the expected squad's wages |
| Victory loot | The winner gets a share of the beaten army's `gold` plus its wage bill: a big army pays a lot | Show the expected loot next to each army |
| Water | Never walkable, fords included | Bridges on every water cell of a crossing; reach shows the rest |
| A story choice that is certain death | e.g. "fight" against a force the squad can't beat ends the game | A route test per choice; failing that, mark the choice as dangerous in its text |
| A once-only offer gated on gold | If the player is short when it fires, the branch is lost for good | Repeat the trigger (`once = false`) while the offer is open |
| A council or choice with a «later» answer that fires once | The map is left with no way forward (a softlock) | Re-offer each week until answered |
| Named companions dying | A later map's scene with them never happens | Show where each `named` is required; check the `has_named_unit` alternatives |

## 9. Limits

From the engine (`rzpack-format.md` has them all): 55 package units, 56 items, 32 named
characters, 255 armies a map, 6 troop slots per list, 5 choices per dialog, a building size of 1 to
9, a map size of 50, 100 or 200, 64 MiB, 4,096 files and 32 MiB per file per package. A `var_changed`
chain stops at 200.

## 10. Gaps an editor would want filled

- **A JSON dump of the parsed model** (`razdor-pack dump --json`), so an external editor doesn't
  parse TOML twice. The structs are serde already; `Located` would need to serialise its file and
  line.
- **A JSON Schema** of every file (generated from the serde structs, e.g. with `schemars`), for
  form generation and editor-side validation.
- **The route test as a CLI** (section 7.3).
- **Line numbers on every diagnostic.** Most have them; some map-level checks don't.
- **`art/` is not read yet** (format 1), so a package can't ship its own pictures beyond event scenes.
- **Format-preserving writes** (`toml_edit`) aren't in the crate yet.
