# `.rzpack` content package format (format 1)

This is the implemented subset of the package format of Razdor 2.0 (slice 1). The design is in
[`docs/superpowers/specs/2026-11-02-v2-packages-design.md`](../superpowers/specs/2026-11-02-v2-packages-design.md).
Anything not listed here is not read yet. The code is `src/pack/`; the tool is `razdor-pack`
(`src/bin/razdor-pack/`). Example: `examples/packs/hello-story/`.

## Files

- A package is a folder. A `.rzpack` is a zip archive of that folder: the entries are the
  package-relative paths (`/` separators), with no parent-folder entry.
- An entry with an absolute path, a `..` component, a backslash or a `:` in a path component
  (such as a drive letter) is an error, and the entry is not read.
- Text files are UTF-8; configuration is TOML.
- Top level: the files `package.toml`, `README.md`, `LICENSE`, and the folders `texts/`,
  `units/`, `items/`, `maps/`, `dialogs/`, `triggers/`, `art/`, `scripts/`. Any other top-level
  entry is a warning and is not read. Read in format 1: `texts/`, `units/`, `items/`, `maps/`,
  `triggers/` and `dialogs/` (the last two in the section below).
- Game files are an error anywhere in a package: any file whose extension is `ini`, `dtm`,
  `ugs`, `lit`, `spi`, `exe` or `dll` (any case). A package refers to the base game with `dt:`
  references and never ships the game's files.

## Reading limits and paths

- A package holds at most 64 MiB of files in all (uncompressed), 4,096 files and 32 MiB per file
  (`pack::files`, `MAX_PACKAGE_BYTES`, `MAX_FILES`, `MAX_FILE_BYTES`). Past one of them the read is
  refused with a message that names the limit. A `.rzpack` above 64 MiB is refused before it is read;
  its entries' declared sizes are checked before any is read, and each read stops at the file limit
  whatever the archive declares (a zip bomb is refused, not unpacked).
- A folder package nests at most 32 levels. A symbolic link in it is read only when it points to a
  file inside the folder. A link out of the folder, to a folder, or to a device is left out with a
  problem, so the package does not check.
- A path to a network share (`\\host\share`, `//host/share`, `\\?\UNC\…`) or to a device (`\\.\…`)
  is refused before anything touches the file system (`files::refused_path`): a package is read from
  this computer only.

## How a save finds its package

A save of a package game names the package by its id, version, content hash, map and the path it was
read from (`ScenarioRef::Pack`). The path is a hint. A load opens a package only from:

- the list `packages.json` in Razdor's data folder (`platform::app_dir()`, `pack::trust`): every
  package the player opened with `razdor --pack` (or `RAZDOR_PACK`), with its canonical path, id,
  version and hash, the newest first, at most 512 entries;
- the Hub's `packages/` folder (`<hub>/packages/`): its folders and `.rzpack` files. The Hub installs a
  catalog package there (`<id>-<version>.rzpack`) or imports a file the player picks, and records it in the
  trusted list too. A newer version is added beside the older ones: no installed version's file is removed
  by an install or an import, so a save of an older version still finds its file. The same id and version
  with other bytes is refused (raise the version instead).

A candidate is opened and must match the save's id, version, hash and map (`Opened::same_package`;
the path is not compared). The save's path is opened only when it canonicalises to a listed path of
that same package. When no candidate matches, the load says that the package is missing and asks
the player to open it first (`razdor --pack <path>`). A save made before the list existed needs one
`razdor --pack` run of its package before it loads.

## `package.toml`

Unknown fields are errors.

| Field | Required | Rule |
|---|---|---|
| `id` | yes | `[a-z0-9-]+` |
| `version` | yes | `x.y.z`, numbers only |
| `format` | yes | `1` |
| `title` | yes | a text key |
| `description` | no | a text key |
| `author` | yes | text |
| `license` | yes | `"MIT"` (any other value is an error) |
| `base` | no | `"community-update"` (default) or `"evolution"`; another value is a warning |
| `requires` | no | array of `"<package-id>@<version-req>"`, default `[]` |
| `rules` | no | `"dt"` (default); any other value is an error: only the DT ruleset exists yet |

A version requirement is `*`, or an operator (`^` by default, `~`, `=`, `>`, `>=`, `<`, `<=`)
and one to three numbers, e.g. `^1.0`. Whether an installed package satisfies it is decided
when packages load, not by `check`.

## Texts: `texts/<lang>.toml`

The language is the file name without `.toml` (`texts/ru.toml`, `texts/en.toml`). A nested
table gives dotted keys: `[unit.knight] name = "…"` is the key `unit.knight.name`. Every value
must be a string. A package needs at least one texts file.

Every text key used (the manifest's `title` and `description`, a unit's or item's `name` and
`description`) must exist in at least one language (error), and in every language (warning
listing the languages that lack it).

**Named characters' names.** A named character (`named = "<id>"`) takes its display name from the text key
`named.<id>`; else from `<map>.named.<id>` of the map the story runs on; else from the first key `<block>.named.<id>`
of any chapter in key order (so a later chapter can name the character it brings: `[marka.named] ilm = "Brother Ilm"`);
else the id itself. The rule is `rules::triggers::named_text`.

## Units: `units/*.toml`

Each file holds `[[unit]]` tables (any file under `units/` with a `.toml` extension; other files
there are not read). Unknown fields are errors.

| Field | Required | Type |
|---|---|---|
| `id` | yes | id |
| `base` | yes | reference to a unit (`dt:` alias or a unit of this package) |
| `name` | yes | text key |
| `description` | no | text key |
| `cost`, `hits`, `attack_blow`, `attack_shot`, `magic_power`, `defence_blow`, `defence_shot`, `protect_life`, `protect_death`, `protect_elemental`, `initiative`, `manevres`, `regen`, `vampirism`, `evasion` | no | integer: overrides the base unit's value |

The values are not range-checked yet. Fields that are not listed are errors.

## Items: `items/*.toml`

`[[item]]` tables, the same way. Fields: `id`, `base` (a reference to an item), `name`,
`description` (optional), `cost` (optional integer).

## Ids and references

- An id is `[a-z0-9_]+`. Ids are unique per kind within a package (units among units, items
  among items).
- A reference is one of:

| Form | Meaning |
|---|---|
| `dt:<alias>` | a unit (same as `dt:unit/<alias>`) |
| `dt:unit/<alias>` | a unit of the base game |
| `dt:item/<alias>` | an item of the base game |
| `dt:spell/<alias>` | a spell of the base game |
| `<id>` | an id of this package, of the same kind |
| `<package>:<id>` | an id of another package; the package must be in `requires` |

`dt` is reserved for the base game. A package never names itself with a prefix.

### The `dt:` alias table

`data/pack/dt-aliases.toml` maps each alias to a base game entry, in three sections (`unit`,
`item`, `spell`). A value is the unit's or item's `GlobalIndex`, or a spell's 1-based position in
its file. The table is generated from the install and Razdor's English translations by
`razdor-pack gen-aliases` and compiled into the binary. An alias is the English name, lowercased,
with every run of other characters as `_`, trimmed; a name without an English translation is
`<kind>_<id>`; duplicates get `_2`, `_3`, … in id order. The table is not hand-edited.

Russian section names as aliases (a fallback mentioned in the design) are not implemented yet.

## Checks

`razdor-pack check <folder-or-.rzpack> [--json]` runs these rules. An error makes the package
unusable; a warning does not.

1. `package.toml` exists and parses; the manifest rules above.
2. At least one `texts/<lang>.toml`.
3. Text keys: exist in at least one language (error) and in every language (warning).
4. Ids are valid and unique per kind.
5. Bases resolve: a `dt:` alias exists for the right kind (an error suggests the closest aliases,
   at most three, within edit distance 3); a local id exists in the package; a `<package>:<id>`
   names a package in `requires`. A unit's chain of local bases must not form a cycle.
6. Top-level entries are known (warning otherwise).
7. No game files.

Syntax errors (TOML, UTF-8, unknown fields, wrong types) are errors with their line.

Output: one line per diagnostic, `error: units/a.toml:12: message (hint: …)`, then a summary
line. With `--json`: `{"ok": …, "diagnostics": [{"severity", "file", "line", "message", "hint"}]}`.
Exit status: `0` no errors, `1` errors, `2` a usage or read problem.

## Triggers and dialogs

Story logic in TOML: `triggers/*.toml` (variables and `[[trigger]]`) and `dialogs/*.toml`
(`[[dialog]]` with choices). The runtime is `src/rules/triggers.rs`, the reader and checks
`src/pack/triggers.rs`. Every condition, action and event is an inline table with one **kind key**
(`{ var = "dread", op = ">=", value = 3 }`); its other keys are its arguments. A wrong key is an
error with the closest name (`did you mean …`). Numbers are whole numbers.

### Shape

```toml
# triggers/ford.toml
[vars]                       # declared once per package, any triggers/*.toml file
dread = 0                    # a whole number
ford_held = false            # true or false

[[trigger]]
id = "ford_guard"
# map = "first-light"        # optional: the map this trigger applies to (its id); none: every map
when = { event = "hero_enters", at = [12, 7] }     # a cell; or at = [x, y, w, h] an area
if = [ { var = "ford_held", value = false }, { random = 50 } ]   # all must hold
then = [ { set = "ford_held", value = true }, { dialog = "ford" } ]
# once = false             # repeat (the default is once)
# chance = 80              # percent, rolled after the conditions hold
```

```toml
# dialogs/ford.toml
[[dialog]]
id = "ford"
title = "ford.title"         # text keys (texts/<lang>.toml)
text = "ford.text"
speaker = "dt:swordsman"     # optional: its portrait

[[dialog.choice]]            # 0 to 5 choices; none means one «Далее» button
id = "pay"
text = "ford.pay"
if = [ { var = "dread", op = "<", value = 3 } ]    # optional: shown only then
then = [ { give_gold = -20 } ]                      # optional actions
```

### Events (`when`)

| Event | Keys | Fires |
|---|---|---|
| `game_start` | | when the story starts (`Game::start_story`) |
| `day_start` | | each midnight |
| `week_start` | | each midnight of a day that is a multiple of 7 from year 0 (the clock has no week) |
| `new_moon` | | each midnight of a day that is a multiple of 28 from year 0 (no moon is modelled) |
| `hero_enters` | `at` | the hero steps onto a cell, or into an area |
| `hero_enters_building` | `building` | the hero enters a building |
| `army_defeated` | `army` | the player beats an army (battle end) |
| `army_arrives` | `army`, and `at` (a cell) or `building` (a building id), either or neither | an army with an `army_goal` reaches its cell (`go_to`, each `patrol` cell) or walks into its building (`go_to_building`); raised at the end of the AI's tick, at the arrival's minute |
| `building_captured` | `building` | the hero takes a building (walking in, or its garrison beaten) |
| `dialog_choice` | `dialog`, `choice` | the player picks that choice |
| `var_changed` | `var` | a variable changes value |
| `building_lost` | `building` | the player's building changes owner: the AI takes it (after the AI's tick), or a `set_faction` takes it |
| `sim_victory` | | developer only: `razdor-pack sim --auto-win-after` raises it before a map's victory, so a story can play its last dialog (the game never raises it) |

### Conditions (`if`): all must hold, checked in order

| Kind | Arguments | Meaning |
|---|---|---|
| `var` | `op` (default `==`; `==`, `!=`, `<`, `<=`, `>`, `>=`), `value` | a variable against a value (flags: only `==` and `!=`) |
| `random` | | true with that percent (the package generator) |
| `hero_at` | the place: `[x, y]` or `[x, y, w, h]` | the hero stands there |
| `army_at` | `army` (a spawned or scenario army id), `at` | the army is on the map at that place |
| `has_named_unit` | `named` (the character's id), `unit`, and `in` (the hero's army by default, or an army id) or `garrison` (a building id), `value` (true by default; `false` for the absence) | a named character of that unit type is in that army or garrison, and alive (a named character who died in battle is not) |
| `has_item` | `in` (`player`, the default, `pack`, an army id) or `garrison` (a building id) | the item is in the pack or worn by that holder's units (a garrison's pack too) |
| `gold` | `op` (default `>=`) | the hero's gold compares with the number |
| `army_alive` | `value` (true by default; `false` for the absence) | the army exists and is not beaten (`value = false`: it is gone) |
| `building_owner` | `faction` (`player`, `ally`, `neighbour`, `enemy`, or 1 to 4) | the building's faction |
| `has_spell` | `in` (`player` or an army id; default `player`), or `garrison` (a building id), `all` (default false) | a unit holds the spell (in its slots, not expired); `all` for every unit |

### Actions (`then`): in order

| Kind | Arguments | Effect |
|---|---|---|
| `set` | `value` | a variable gets a value (its change fires `var_changed`) |
| `add` | `value`, and optionally `min` and `max` | a number variable grows; with `min` and/or `max` the result is clamped to them (`{ add = "crown", value = 1, min = -5, max = 5 }`) |
| `random_var` | `min`, `max` | a number variable gets a random value in the range |
| `dialog` | | opens the dialog (its choices visible now) |
| `give_gold` | | the hero's gold (negative takes; never below 0) |
| `give_item` | `to` (`pack` default, `player`, an army id: its loot, `named:<id>`: a free worn slot, else the pack), or `garrison` (a building id: its troops, else its pack) | an item |
| `give_unit` | `named` (optional character id) | a unit joins the hero's army (a full army drops its weakest unit, as DT) |
| `give_xp` | `to` (`player` default, an army id, `named:<id>`), or `garrison` (a building id) | experience |
| `spawn_army` | `at = [x, y]`, `faction`, `model` (0 to 12, default 5), `troops` (list of units) | a new AI army on the map, steered like a scenario army (its goal comes from `army_goal`), with its id (its name); an id in use spawns nothing |
| `remove_army` | | the army leaves the map (it can come back by an event, as DT's) |
| `teleport_hero` | cell `[x, y]` | the hero's army moves there |
| `teleport_army` | `at = [x, y]` or `to = "<building>"` | an army moves to a cell or a building |
| `set_faction` | `faction`, `of` (`army` default, or `building`) | a faction for an army or a building |
| `set_relation` | `towards` (`player`, `ally`, `neighbour`, `enemy`), `value` (−3 to 3), `of` | the relation of an army or a building towards a side |
| `add_units` | `troops`, and the target: the army's name, `of = "building"` with the building's name, or `"garrison"` with `garrison = "<building>"` | units join an army or a garrison |
| `cast_on_army` | `in` (`player` or an army id) or `garrison` (a building id), `minutes = n` or `permanent = true` | the spell in each unit's slot (a slot that is free, or the same spell's) |
| `remove_spells` | the holder (`player` or an army id); or `"garrison"` with `garrison = "<building>"` | every spell of its units goes |
| `heal_army` | `"player"` (the hero's army only) | every unit of the army healed in full, and the dead raised (the cheat «Heal»'s rule): a carried army comes back whole at a chapter's start |
| `remove_spell_from_book` | the spell | the hero's spell book drops it |
| `army_goal` | the army is the value; exactly one of: `go_to = [x, y]`, `go_to_building = "<building>"`, `follow = "hero"` or an army id, `patrol = [[x, y], …]` (at least two cells), `attack = "hero"` or an army id, `clear = true` | the army walks to the goal instead of the AI's own choice (see below) |
| `army_speed` | the army is the value, `percent = n` (10 to 1000, 100 normal) | the army's map steps take 100/n of their time (saved) |
| `hero_figure` | `"default"`, a unit `dt:<alias>`, or `{ model = "<army model>" }` | the hero is drawn with that unit's map figure, or an army model's (saved) |
| `army_figure` | the army is the value, `unit = "dt:<alias>"` or `model = "<army model>"` | the army is drawn with that unit's map figure, or an army model's (saved) |
| `remove_unit` | the named character is the value, `in` (the hero's army by default, or an army id) or `garrison` (a building id) | the named character leaves that holder; his worn items go to the pack |
| `victory` / `defeat` | `= true` | the scenario ends (after the dialogs shown; the first ending holds) |
| `victory` | `= true`, `next = "<map>"` | the same victory, and the campaign goes to that map (the override of `campaign.toml`, «Campaigns») |
| `travel` | `= "<map>"`, `at = "<point-id>"` (optional) | the campaign leaves this map for that one, at its point (else its start), once the dialogs are read («Campaigns») |
| `ending` | `= "<dialog-id>"` | the dialog shows, then the campaign ends with it («Campaigns») |
| `quest` | `= "<quest-id>"`, `title = "<text key>"`, `text = "<text key>"` | an active diary entry («Активные»): added, or its texts updated when the id is already active (its date stays, no duplicate); the texts are shown in the game's language (`story_text`) |
| `complete_quest` | `= "<quest-id>"` | that quest's entry moves to «Выполненные», dated now; a quest not active (or unknown) does nothing |
| `restore_squad` | `= true` | the hero's squad is healed to full and its dead raised, at no cost (a night's rest at the hub's healers; the town's raise without its price) |
| `restore_army` | `= "player"` | the hero's squad is healed to full and its fallen members stand up again, for nothing (the chapter's restoration at its start; a carried army has no dead at a hand-over, so the heal is what it does there) |
| `start_battle` | the army is the value, `rules = "dt"` or `"lua:<script>"` | the army meets the hero now, in that battle (a field battle for a script; «Field battles») |

Units and items are `dt:<alias>` or the package's own ids; spells are `dt:spell/<alias>`. A
«holder» is `player`, `pack`, an army id, `named:<id>` or a garrison (`garrison = "<building>"`).
Where a table takes `in` or `to`, the `garrison` key is its alternative (not both). Where the
holder is the action's own value (`remove_spells`, `add_units`), the value is `"garrison"` and the
key names the building.

**Armies' goals and paces.** `army_goal` gives an AI army a goal: it no longer takes the original
AI's choice of destination. On each arrival it plans its way to the goal's cell with the AI's own
routes (land, or the sea for a ship; its search may take up to 60 000 nodes, where the AI's own takes
4000, so a route across a 200×200 map is found) and walks it on the same step clock, so the speed rules,
ships, bridges and fog behave as before. A `go_to` army stands on its cell; a `go_to_building`
army walks into the building and stands there (its arrival rules, as the AI's, take over inside);
`patrol` loops through its cells; `follow` keeps next to the hero or an army as it moves;
`attack` walks to the hero or an army and, next to it, attacks as an AI army does (a meeting, or a
battle). `clear = true` gives the army back to the original AI. A goal whose target is gone or has
no route drops back to the original AI, and nothing fires. `army_speed` multiplies the time of its
steps (200 is twice as fast, 50 twice as slow); a slower army banks more of the hero's minutes, so
every step is still reached. Armies without a goal and paces of 100 are the original's, unchanged.

### Moments in the game

| Moment | Where (the game's own events run first) |
|---|---|
| `game_start` | `Game::start_story`: the events wait in `drain_events` |
| `day_start`, `week_start`, `new_moon` | `Game::pass_slice`, after the noon report, per midnight (`story_midnight`) |
| `hero_enters` | `Game::tick`, after each step that moved him (`hero_step`) |
| `hero_enters_building` | `Game::enter_building`, after `Arrived` and the tribute |
| `building_captured` | `Game::hero_step` (walking in), `Game::resolve_battle` (a garrison beaten) |
| `army_defeated` | `Game::resolve_battle`, for each army newly beaten by the player |
| `dialog_choice` | `Game::answer_story_dialog`, after the choice's actions |
| `var_changed` | inside the moment that changed the variable; a loop guard of 200 runs |

A story with no triggers and no dialogs attaches nothing: no draw, no state, no save field. The
package generator is its own (`TriggerState::rng`, seeded from the story); `rules::rng` is never
drawn for a package. Package armies and named characters take the ids 200 up (56 each at most).

### Checks (`razdor-pack check`)

Unknown events, conditions, actions and keys (with the closest name); undeclared variables;
values of the wrong type; unknown dialogs and choices; a dialog that no trigger or choice opens
(a warning); more than five choices; a text key missing in every language (a `quest`'s title and
text too); a `complete_quest` of a quest id that no `quest` action adds (with the closest id); unknown
`dt:` units, items and spells; a package that is not in `requires`; a variable declared twice; a
trigger id used twice; a `map` that is not in the package.

**The diary.** A `quest` and a `complete_quest` are the package's journal entries (`rules::journal`,
`Entry::story`). Every dialog that is shown is also recorded in «Сообщения», with its title and text,
so a closed dialog can be reread (the same dialog in the same minute is kept once). Across maps an
active quest stays active (the original's journal empties per map; the package's does not), and a
`quest` of that id on the next map updates it and lists it under that map. Saves keep all of it.

Per map (the map checker, once the map's size is known; `triggers::check_map_refs`). The triggers
that apply to a map are those without `map` and those naming it; the dialogs they open, and the
dialogs those open, are checked on that map too. On each such map: every army and building a
trigger or a dialog names is on the map (its `[[army]]` or `[[building]]` id), or is a
`spawn_army` of the package (spawned armies are known on every map); every cell lies inside the
map (`hero_enters` and `hero_at` places, `army_at`, `teleport_hero`, `spawn_army`, `teleport_army`).
A trigger without `map` is therefore checked on every map of the package.

### Running a story

A story attaches to its map when a game of that map starts (`pack::open::Opened::start`: the
origin is set, the story is attached and its `game_start` moment raised, so the opening dialogs
wait in `drain_events`, as the scenario's opening events do). `pack::compile` gives the ids a
story names (`MapNames`, the `rules::triggers::Resolve` of a compiled map): a map's army is its
`[[army]]` position + 1 and a building its `[[building]]` position + 1, as the scenario numbers
them; the package's units and items are their compiled ids; a `spawn_army` name is a package army
from 200 up, in order of first mention. The story's `army_names` and `building_names` name them in
the trace.

**Saves.** A package game saves like the others: its save names the package (`ScenarioRef::Pack`:
id, version, content hash, the path it was read from, and the map's id). The hash is FNV-1a over
the package's files (`pack::open::package_hash`). A load reads the package from that path again
(`ui/pack_play.rs` `ensure_pack`), checks the hash and the map, attaches the story and keeps the
saved state (variables, fired triggers, the dread's generator, spawned armies). A package that is
gone or changed refuses the load with «the package {id} {version} … is missing or has changed: open
it again from its file», and nothing panics. A save of a map of the install is unchanged, and an
older save has no package field.

**The trace.** `razdor-pack sim --trace` keeps what the story did, with the game minute of each step
(`TriggerState::trace`, never saved): the triggers that fired and on which moment, the variables'
changes, the dialogs that opened with their visible choices, the choice the sim took («Далее» for
none), the armies that came onto the map, and the ending. It is kept only while asked for.

### Not in this step

Branching campaigns (10) are «Campaigns» above. Named characters can be in an army or a garrison
for `has_named_unit` to find, but no action yet puts one there (only the hero's squad gets named
units, by `give_unit`). Goals and paces on a demo gang (an army not steered by the AI) do nothing.

## Maps: `maps/<id>/map.toml` and `maps/<id>/terrain.png`

A map is a folder: `map.toml` (below) and `terrain.png`. The id is `[a-z0-9-]+` (the folder name).
Any other file in a map folder is a warning and is not read. A map folder without `map.toml` is
an error. Numbers in cells count from 0 at the top left (`[x, y]`, x to the right, y down).

```toml
title = "first_light.title"          # text key: the map's name in the list
description = "first_light.description"   # text key, optional
start_date = "1300-04-01"           # YYYY-MM-DD on the game's clock (30-day months, 12-month years)
relations = [[3, 3, 0, -3], [3, 3, 0, -3], [0, 0, 3, -3], [-3, -3, -3, 3]]   # player, ally, neighbour, enemy; −3..3

[terrain_palette]                   # optional: the colours that differ from the default palette
grass_plain = "#60a040"

[hero]
cell = [12, 26]                     # the start cell, the same for every offered class
classes = ["knight", "archmage"]    # knight, archmage, ranger (the others are not offered)
gold = 300
mana = 20
troops = [{ unit = "dawn_knight", level = 1, count = 3 }]   # up to six slots

[[object]]                          # hills, mountains, stones, trees, dead_trees, thicket
kind = "trees"
cell = [33, 28]

[[building]]                        # id unique; type from the list below
id = "dawnwatch"
type = "town"
name = "town.dawnwatch.name"        # text keys
description = "town.dawnwatch.description"   # optional
cell = [10, 24]                     # the bottom-right cell of the footprint
size = [9, 9]                       # optional: width and height in cells, 1 to 9 (Footprints below)
faction = "player"                  # player, ally, neighbour, enemy
income = 40                         # gold a day: an owned castle or fort pays its stock at each noon (towns pay nothing)
gold_max = 160                      # optional: the most the stock holds; four days' income when left out
garrison = [{ unit = "dawn_knight", level = 1, count = 4 }]     # up to six
recruit = [{ unit = "dt:militiaman", start = 4, max = 8 }]      # up to six

[[army]]                            # id unique among the armies
id = "raiders"
name = "army.raiders.name"
cell = [33, 17]
faction = "enemy"
model = "feudal"                    # feudal, bandits, peasants, inactive, necromancer, ghosts, zombies
ship = "pirate"                     # optional: none (default), hero, pirate, merchant; an army on water needs one
gold = 150
troops = [{ unit = "dt:swordsman", level = 1, count = 2 }]      # one to six
leader = { unit = "dt:mounted_knight", level = 2, name = "leader.raider_chief.name" }   # optional named leader

[[point]]                           # a lantern
id = "gate"                         # optional: a campaign's travel enters the map here
cell = [25, 20]
radius = 3                          # 0 to 24
active = true                       # lit from the start

[victory]                           # required: see the kinds below
kind = "defeat_all"
message = "first_light.won"         # text key of the victory window, optional
```

A unit reference is a `dt:` alias or a unit of the package (the same grammar as above). A troop
slot's `level` is the level the game shows (1 is the first); the file stores it from 0. A count of 0
is an error. A hero class cannot be a troop (the engine reads unit ids 1 to 3 as the hero classes).
Another package's units are not loaded yet.

**Victory** (`kind`). Defeat is always the original's: the hero falls with his army.

| `kind` | Fields | Compiles to | Note |
|---|---|---|---|
| `defeat_all` | `message` | a global event: `beaten_armies` = the enemy armies (faction `enemy`) | at most **two** enemy armies (one event checks two); more are an error for now |
| `defeat_army` | `army`, `message` | a global event: `beaten_armies` = that army | the army is beaten when the player defeats it |
| `capture` | `building`, `message` | a global event: `buildings` = that building, owner the player | the building must not be the player's from the start |
| `reach` | `cell`, `message` | a local event on an event point (model 9) at the cell | fires when the hero steps onto the cell |

The victory event shows its window in the victory look (its title is the map's name with `#VICTORY`),
which the game uses when the map has no next map.

**Terrain.** Every pixel's colour is one terrain kind (exact RGB). The map's size is the picture's:
50, 100 or 200 cells on each side. Any other colour is an error that names the pixel: `pixel (x, y)
is #rrggbb: not a terrain colour`. The default palette, one colour per engine surface:

| kind | colour | kind | colour |
|---|---|---|---|
| `shallows_fords` | `#8fd0e0` | `marsh` | `#5f7a4a` |
| `coastal_water` | `#3a7fbf` | `impassable_swamp` | `#3d4f32` |
| `deep_sea` | `#1d4f8a` | `sand_dunes` | `#e6d28a` |
| `lava_fields` | `#c83a1a` | `clay_soil` | `#a0704a` |
| `road` | `#b08850` | `stony_soil` | `#8a8a80` |
| `grass_lowland` | `#9cc46a` | `scorched_land` | `#4a3a33` |
| `grass_plain` | `#6fa84a` | `snowy_ground` | `#f0f4f8` |
| `dry_plain` | `#c9b56a` | `impassable_snowdrifts` | `#c8d8e8` |

Forests are objects (`trees`) over grass, not a terrain kind. Water (`shallows_fords`, `coastal_water`,
`deep_sea`) is for ships. Water is never walkable, fords included: a walker crosses a river only on a bridge
(`stone_bridge`, `wooden_bridge`), one 1×1 building on each water cell of the crossing, in a row along it.
Lava, the impassable swamp and the impassable snowdrifts block everything. A `[terrain_palette]` entry
changes one kind's colour; two kinds may not share a colour.

**Placement rules** (`razdor-pack check`). The hero, a town and a reach cell stand on land. An
object is not on water. Only bridges (`stone_bridge`, `wooden_bridge`) stand on water among the
buildings. An army on water needs a ship; a ship sails water. No cell is outside the map, and no two
buildings share one. A cell of the story the hero's start cannot reach on foot is a warning (`reach`): a
building's entrance, a `hero_enters` or other trigger cell, a teleport, a patrol or goal cell (from its army's
cell), a travel point (a `[[point]]` with an `id`). A lantern without an `id` is not walked to.

**Footprints.** A building covers a footprint of `size = [width, height]` cells (1 to 9 each): its
`cell` is the bottom-right cell, the footprint runs `width` cells left and `height` up from it, plus
one more row above when it is wider than tall (the original's rule, `rules::world::footprint_cells`).
A walker enters the building from any of its cells, and every footprint cell is walkable at road speed.
Without `size` a building takes its type's default, the most common size of that type in the original
maps: castle 4×4, village 4×4, town 9×9, fort, tavern, market, church, ruins and dungeon_entrance 2×2,
shipyard 3×4, altar 3×3, smithy and the bridges 1×1 (`compile::DEFAULT_FOOTPRINTS`; palace and obelisk
keep 1×1, no original map has one). Checks: no two footprints share a cell, a footprint stays on the
map, the hero's start and every army and lantern are off every footprint (errors); an object inside one
is drawn over the building, and a footprint over water or blocked ground would be paved into a road
(warnings).

**Ids and units of a package.** A package's units are numbered from 200 in `units/` order, its items
from 200 in `items/` order (`src/pack/compile.rs`). The base game's units end at 102 and its items at
167. A troop slot holds a unit in one byte, so a package has at most **55 units** (200 to 254; 255 and
254 are the removal codes of the engine's events); an event holds an item in one byte, so at most
**56 items** (200 to 255). A package unit draws the picture of its root base unit.

**Building types:** palace, town, village, castle, fort, tavern, market, church, smithy, shipyard,
altar, dungeon_entrance, ruins, stone_bridge, wooden_bridge, obelisk. Each has a default footprint
(see Footprints); `size` in a `[[building]]` overrides it.
**Object kinds:** hills (class 1, sprite 10), mountains (5, 10), stones (8, 10), trees (9, 0),
dead_trees (10, 108), thicket (11, 0, impassable); `sprite` overrides the sprite.

**Compiling.** The map compiles to a `.DTm` scenario in memory (`pack::compile`): the header
(size, start date, the hero presets of the offered classes, relations), the terrain, objects,
buildings (their names and garrisons), armies (named leaders become named characters), points, and
the one victory event. A compiled scenario survives `to_payload` and `parse_payload` unchanged. The
game's own limits apply: 32 named characters, 255 armies, 6 troop slots per list.

## Campaigns: `campaign.toml`

An optional file at the package's top: the graph of its maps. Without it the package plays as
before (`razdor --pack` opens its first map, and nothing changes maps). The code is `src/pack/campaign.rs`
(reading, checks, the routes) and `src/rules/campaign.rs` (the game); the design is in
`docs/flows/campaign-scenario.md`.

```toml
start = "brod"                 # the map a new campaign starts on
title = "campaign.title"       # text key, optional: the campaign's name in the new-game list

[carry]                        # what moves from map to map; every flag defaults to true
hero = true                    # level, xp, spells and worn items of the hero
army = true                    # the squad: named units with their levels and items
pack = true                    # unworn items
gold = true                    # gold, and the god's favour (mana) with it
vars = true                    # every variable, and the fired-once triggers that name no map

[[map]]
id = "brod"
[[map.next]]
to = "marka"
if = [ { var = "ally", value = 1 } ]     # all of its conditions must hold
[[map.next]]
to = "salt"
if = [ { var = "ally", value = 2 } ]

[[map]]
id = "marka"
revisit = true                 # leaving keeps the map's state; coming back restores it
[[map.next]]
to = "salt"
```

**Graph.** `start` and every `[[map]] id` are maps of the package (`maps/<id>/`), each listed once. A
map's victory (the map's own `[victory]`, or a story's `victory = true`) goes to its first `[[map.next]]`
whose `if` holds (in file order), and a route with no `if` always holds. When none holds, the campaign
ends there: the victory screen. A map with no `next` ends the campaign on its victory too. A story's
`victory = true, next = "<map>"` overrides the graph for that victory.

**Conditions** (`if`) are the trigger conditions, with the variables declared in `triggers/*.toml`
(`var`, `gold`, `has_item`, `has_named_unit`, `has_spell`, `random`). A campaign names no army, building
or place: `army_at`, `army_alive`, `building_owner` and `hero_at` are refused (the map's own triggers
hold those).

**Travel and endings.** `{ travel = "marka", at = "gate" }` in a story's `then` leaves the current map
once the dialogs are read; the hero comes in at the map's `[[point]]` with that `id`, else at the map's
start. The current map keeps its state when it is a `revisit` map. `{ ending = "the_end" }` opens that
dialog, then the campaign ends with it: the victory screen shows the dialog's title, and the ending is
recorded in the save. A travel or ending is checked: its map is a campaign map, its point exists, its
dialog exists.

**Checks** (`razdor-pack check`). Errors: a map that is not in the package, a `next` or a victory to a
map that is not in the campaign, an undeclared variable or a value of the wrong kind in an `if`, an
`if` that names an army, a building or a place, a travel to a point the map lacks, a `start` that is not a
campaign map, no path from `start` to a map that can end the campaign. Warnings: a package map that is not
in the campaign, a campaign map no route, travel or victory reaches.

**What carries** (on every change of map, the victory's or a travel's). Per `[carry]`: the hero (level,
xp, spells, worn items; off: the new map's hero), the squad with its named characters (off: the new map's
squad), the pack (off: the new map's), gold and mana (off: the new map's), and the variables with the
fired-once triggers that name no map (off: the new map's defaults). The journal and the flags always carry,
as in the original. The hero's map sheet (`hero_figure = …` in a story, the story's own choice of his
figure) resets to the default on every change of map; the story sets it again if it needs it. The
original's own hero figure (set by an event) carries as before. A named character keeps its id over the package's maps: the package numbers them once,
by first mention in all its triggers and dialogs. A map is entered as the original's campaign hand-over
(`docs/flows/campaign-scenario.md`): its opening events and its story's `game_start` run on the first entry.

**Revisits.** Leaving a `revisit = true` map keeps its state: the world (armies, buildings, objects, relations),
the fog, the event engine, the armies beaten, and the map's own fired triggers and spawned armies. An army's
goal, speed and figure (`army_goal`, `army_speed`, `army_figure`) are part of its record in the world, so they
are kept too. Entering it
again restores that state and runs no opening events again, and the hero comes in at the travel's point or at
the start. The game's clock goes on from the point it left the map (the stored state is the world's and the
map's dynamic parts only; the statics come from the map again).

**Saves.** A save mid-campaign keeps the campaign: the map it is on, the stored states of the revisit maps it
left, the clock and any ending. The carried values are the game's own, saved as always. A save before
campaigns loads as before.

**Sim.** `razdor-pack sim <package> --campaign [--days <n>=60] [--choices …] [--trace] [--auto-win-after <days>]`
plays the campaign from its start: each map for `--days`, the victory or travel goes to the next map with what
carries, the ending stops it. `--auto-win-after n` fires each map's victory after n days (a developer switch:
the sim's hero does not walk, so the graph is walked this way). `--trace` prints each map's entry values (gold,
squad, named characters, the pack, the variables) and what the story did on it, by day; `--json` gives the same
as data.

## Commands

- `razdor-pack check <folder-or-.rzpack> [--json]`: the rules above; exit 0 with no errors, 1 with
  errors, 2 on a usage or read problem.
- `razdor-pack sim <folder-or-.rzpack> [--map <id>] [--days <n>=30] [--json] [--trace]
  [--choices first|random|<dialog>=<choice>,…]`: the AI plays the map headless for N days, as the
  community map smoke test does (`rules::sim`, shared with it), with the package's story attached.
  It reports the day of victory or defeat (the scenario's or the story's), a panic (caught) or a
  softlock. Without `--map` every map is played. `--trace` prints every day (`day 3: fired …`,
  `x = 2`, `dialog … opens: a, b`, `the sim answers … with a`, `army … comes onto the map`, `the
  story ends: victory`); `--json` gives the same as data, its `trace` a list of `{day, what}`.
  `--choices` answers the dialogs: `first` (the default: the first choice shown, «Далее» when
  none), `random` (a seeded rule: the same answers every run), or `dialog=choice,…` (the choice
  named, if it shows, else the first). Needs `RAZDOR_DT_DIR`. Exit 1 on a panic, a softlock or a
  check error.
- `razdor-pack sim … --set var=value` (repeatable): forces a story variable before the story starts, so its
  `game_start` triggers see it (a developer switch to drive branches). On a campaign it is set for the first map and
  carries on. `--auto-win-after` raises `sim_victory` before a map's victory is taken (a developer moment).
- `razdor-pack play <folder-or-.rzpack> [--map <id>]`: runs `razdor --pack <path> [--map <id>]`, the
  `razdor` binary next to this one.
- `razdor --pack <folder-or-.rzpack> [--map <id>]` (or `RAZDOR_PACK`, `RAZDOR_PACK_MAP`): starts a
  new game on the package's map, after the install's map list. The package is checked and compiled
  on the install; the game opens on the hero window, and the package's story starts with it. A
  package game saves: its save names the package (see «Running a story» and «How a save finds its
  package»), and a load finds it again among the packages the player opened.

Off-screen: `RAZDOR_PACK=<path> RAZDOR_SCENE=map:<map-id>` renders the package's map (the
`offscreen-snapshot` skill).

## Not in format 1

`art/` and `scripts/` are reserved: they are allowed at the top but not
read yet. Also not yet: `render`, the other packages' units in maps, and `defeat_all` with more than two enemy armies.

## Packing, the Hub and uploads

- `razdor-pack pack <folder> [-o out.rzpack]` runs `check` (an error refuses it) and zips the folder
  deterministically: entries in path order, one timestamp, deflate, fixed modes. The same folder gives the same
  bytes and SHA-256 (`pack::archive`; test `the_same_folder_packs_to_the_same_bytes_and_sha256`).
- `razdor-pack info <folder-or-.rzpack>` prints the manifest, the maps, the size and the SHA-256.
- The Hub (`docs/flows/app-and-ui.md`, «Content packages»): a catalog entry `kind = "package"`, a `.rzpack`
  installed to `<hub>/packages/`, «Импорт пакета…» for a file (checked, marked not checked by the Hub),
  Play from the card, the report button (clipboard and the Discord invite), `status = "unlisted"` hidden from the list.
- Uploads (`tools/community-pack/`: `package_ci.py`, `build.py --packages`, the inactive workflow in
  `ci/package-pr.yml`, and the owner's steps in its README): the gate on a pull request to razdor-community.

The example's terrain is painted by `tools/pack-example-terrain.py` (stdlib only).

## Scripts (Lua)

Slice 3 (in progress). A script is `scripts/<name>.lua`, Lua 5.4 run in a sandbox (`src/lua/`,
feature `lua`). The sandbox, the limits and why they exist:

- **Libraries:** `base` (without `dofile`, `loadfile`, binary `load`, and `collectgarbage` except
  `"count"`), `table`, `string` (without `dump`), `math` and `utf8`. `io`, `os`, `package`,
  `debug` and `coroutine` are not loaded.
- **`require`** loads only `scripts/<name>.lua` of the package (names `[a-z0-9_]+`), once each.
- **`math.random`** is the game's seeded generator; `math.randomseed` is refused. There is no clock.
- **`pairs`** visits keys in an unspecified order; `razdor.sorted_pairs(t)` gives a fixed one
  (keys of numbers, strings and booleans).
- **`print`** goes to the game's log: at most 2 KB of each argument, 4 KB of each line, 64 KB in all.
- **Budgets per call:** 5,000,000 instructions and 64 MB of memory. A `pcall` cannot swallow them.
- **Wall-clock backstop:** 2 s per call. It is not an input to the result (a call aborted by it
  is an error, not a different outcome); it exists because C library work is not counted.

**String matching is limited, and why.** Lua's pattern matcher is C code: the instruction budget
does not run inside it, and its cost grows like n^(items+1) in the subject length n. So:

- a subject is at most 64 KB; a pattern at most 256 bytes;
- a pattern has at most 4 *backtracking items* (each quantifier `* + - ?`, and each `.` or
  `%class` a quantifier follows; a set `[...]` is one class with no count of its own);
- a subject of n bytes is allowed only while n^(items+1) is at most 2e8: about 14 KB for one item,
  584 bytes for two, 64 KB for none;
- the features that the cost count does not see are refused: back-references `%1` to `%9`, the
  balanced match `%b` and the frontier `%f` (`pattern feature not allowed in Razdor scripts`).
  `%%` is an escaped percent; `%%1` is allowed;
- `string.rep` makes at most 1 MB, `table.concat` at most 1 MB.

`gmatch` checks the deadline at every step, and `gsub` before each replacement made by a function
or a table. A single C call that is already running is not interrupted.

The field battle hooks (`setup`, `on_turn_start`, `on_damage`, `can_enter`, `check_end`, `ai_turn`)
are described under «Field battles» below.

## Field battles

A field battle replaces the DT battle for one fight: a small board, the same DT units (their stats
and the DT damage formula), and the package's Lua deciding the terrain and, when it asks, the
actions. The rules are `src/rules/field/` (no Lua), the Lua side `src/lua/field.rs`, the game's seam
`src/rules/game/battle_field.rs`; the flow is in `docs/flows/field-battle.md`.

**Choosing it.** An army's battle in its map: `battle = "dt"` (the default) or `battle = "lua:<script>"`
(`[[army]]` in `map.toml`). A trigger's action `{ start_battle = "<army>", rules = "dt" | "lua:<script>" }`
starts the meeting with that army now and overrides the army's own. `razdor-pack check` reads both
and requires the script to load and to define `setup` and `check_end`.

**The script.** `scripts/<script>.lua` in the package, the same sandbox as the rest of the package's
Lua (its libraries, its budgets, `require` of the package's own scripts). Each hook is a global
function, and each is optional:

| Hook | Called | Returns |
|---|---|---|
| `setup(s)` | once, when the battle opens | nil, or `{ tiles = {…}, place = {…}, max_turns = n, vars = {…} }` |
| `on_turn_start(s)` | at the start of each turn, after the order is set | nil, or `{ tiles = {…}, log = {"text.key", …}, vars = {…} }` |
| `on_damage(s, d)` | before each blow or shot lands | nil (the DT damage), or the damage as a whole number |
| `can_enter(s, m)` | before a unit steps onto a cell | nil, or `false` to refuse the step |
| `check_end(s)` | after each action, and at the turn limit | nil (play on), `"defenders"`, or `"attackers"` |
| `ai_turn(s)` | when an AI unit (`s.active`) acts, for both sides | nil (the Rust AI), or `{ move = {x,y}, attack = {unit = id} \| {gate = {x,y}} }` |

**The view `s`** (read only: a write is a Lua error; the tables are proxies):

| Field | Meaning |
|---|---|
| `s.round`, `s.max_turns` | the turn now, and the turn limit (12 unless `setup` sets it) |
| `s.active` | the id of the unit that acts (nil outside a unit's action) |
| `s.board.w`, `s.board.h` | the board's size (at most 12 × 10) |
| `s.board.tiles` | the tiles as names, row by row: the tile at `x, y` is `tiles[y * w + x + 1]` |
| `s.board.gates` | `{x, y, hp}` for each standing gate |
| `s.units` | the units, `s.units[id + 1]` is unit `id` (0 to n-1, defenders and attackers together) |
| `s.vars` | the package's variables the battle keeps (see below) |

A unit: `id`, `side` (`"defenders"` = the hero's side, `"attackers"`), `name`, `level`, `hp`, `max_hp`,
`attack_blow`, `attack_shot`, `defence_blow`, `defence_shot`, `initiative`, `manevres`, `shot_range`
(the farthest shot in tiles, 6 for a shooter; a battlement adds 2 more when the shooter stands on one),
`at` (`{x, y}`, nil when off the board) and `alive`.

**Tiles.** `"open"` (1 manevre a step), `"wall"` (blocks movement and fire), `"gate"` with `hp` (blocks
until broken: a blow takes the unit's blow off its HP, and at 0 it is rubble), `"battlement"` (walkable;
a shooter on it shoots over walls and reaches 2 tiles further), `"rubble"` (2 manevres a step). A
`setup` or `on_turn_start` entry is `{x = n, y = n, tile = "wall"}`, and a gate is `{…, tile = "gate", hp = 60}`.

**Helpers** (in `razdor`; they read the board as it is now, and avoid the cells units stand on):

| Helper | Gives |
|---|---|
| `razdor.path({x,y}, {x,y})` | the cheapest path as a list of `{x, y}` (both ends included), or nil |
| `razdor.distance(a, b)` | the Manhattan distance, a whole number |
| `razdor.line_of_fire(a, b)` | whether a shot from `a` reaches `b` (a clear line; a battlement shoots over walls) |
| `razdor.vars` | the package variables (read only; the same as `s.vars`) |
| `razdor.sorted_pairs(t)` | `pairs` in a fixed order (as elsewhere in the package) |

**Variables.** `vars = {name = value}` returned by a hook is kept by the battle and saved with it;
the next hook sees it in `s.vars`. A script keeps its memory this way, not in Lua globals (a load
starts a new Lua state). Values are numbers, strings or booleans.

**Rules the script does not change.** A move costs the unit's manevres (rubble 2, a unit cannot pass
another, a cell with a unit on it cannot be entered). Melee is a blow on an adjacent unit or gate; a
shot needs a clear line within the shooter's range. The side with nobody standing loses. At the turn
limit `check_end` decides, and if it says nothing the defenders win. A unit that cannot act (its move or
target is off the board, blocked or out of reach) waits.

**Errors.** A hook that raises an error, returns a value of the wrong kind, or returns a table with a
metatable, nested deeper than 4 levels, with more than 4,096 entries in one table or 16,384 in all, or
with a table that appears twice, ends the battle as a defenders' win. The log and the save hold the
error with its script and line (`scripts/siege.lua:12: …`). Nothing panics.

**The end.** When the battle is decided, the game settles it as a DT battle does (`resolve_battle`):
deaths, experience, loot and the campaign. The units' HP is written back after each action.

**Checks (`razdor-pack check`).** Every `scripts/*.lua` must load (a syntax error names its line). A
battle that names a script must find it, and it must define `setup` and `check_end`.

**Sim (`razdor-pack sim`).** A field battle is played on the AI (both sides) when it starts. With
`--trace` the sim prints each field battle's rounds and winner; `--seed <n>` sets the seed of the
story (and so of the field battles). The example `siege-test` map is the test: `razdor-pack sim
examples/packs/pepelnaya-marka --map siege-test --trace --seed 1`.
