# `.rzpack` content package format (format 1)

This is the implemented subset of the package format of Razdor 2.0 (slice 1). The design is in
[`docs/superpowers/specs/2026-11-02-v2-packages-design.md`](../superpowers/specs/2026-11-02-v2-packages-design.md).
Anything not listed here is not read yet. The code is `src/pack/`; the checker is
`razdor-pack check` (`src/bin/razdor-pack/`). Example: `examples/packs/hello-story/`.

## Files

- A package is a folder. A `.rzpack` is a zip archive of that folder: the entries are the
  package-relative paths (`/` separators), with no parent-folder entry.
- An entry with an absolute path, a `..` component, a backslash or a `:` in a path component
  (such as a drive letter) is an error, and the entry is not read.
- Text files are UTF-8; configuration is TOML.
- Top level: the files `package.toml`, `README.md`, `LICENSE`, and the folders `texts/`,
  `units/`, `items/`, `maps/`, `dialogs/`, `triggers/`, `art/`, `scripts/`. Any other top-level
  entry is a warning and is not read. Read in format 1: `texts/`, `units/`, `items/`, `triggers/`
  and `dialogs/` (the last two in the section below).
- Game files are an error anywhere in a package: any file whose extension is `ini`, `dtm`,
  `ugs`, `lit`, `spi`, `exe` or `dll` (any case). A package refers to the base game with `dt:`
  references and never ships the game's files.

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
| `building_captured` | `building` | the hero takes a building (walking in, or its garrison beaten) |
| `dialog_choice` | `dialog`, `choice` | the player picks that choice |
| `var_changed` | `var` | a variable changes value |

### Conditions (`if`): all must hold, checked in order

| Kind | Arguments | Meaning |
|---|---|---|
| `var` | `op` (default `==`; `==`, `!=`, `<`, `<=`, `>`, `>=`), `value` | a variable against a value (flags: only `==` and `!=`) |
| `random` | | true with that percent (the package generator) |
| `hero_at` | the place: `[x, y]` or `[x, y, w, h]` | the hero stands there |
| `army_at` | `army` (a spawned or scenario army id), `at` | the army is on the map at that place |
| `has_named_unit` | `named` (the character's id), `unit` | a named character of that unit type is in the hero's army |
| `has_item` | `in` (`player`, the default, or `pack`) | the item is in the pack or worn by his army |
| `gold` | `op` (default `>=`) | the hero's gold compares with the number |
| `army_alive` | | the army exists and is not beaten |
| `building_owner` | `faction` (`player`, `ally`, `neighbour`, `enemy`, or 1 to 4) | the building's faction |
| `has_spell` | `in` (`player` or an army id; default `player`), `all` (default false) | a unit holds the spell (in its slots, not expired); `all` for every unit |

### Actions (`then`): in order

| Kind | Arguments | Effect |
|---|---|---|
| `set` | `value` | a variable gets a value (its change fires `var_changed`) |
| `add` | `value` | a number variable grows |
| `random_var` | `min`, `max` | a number variable gets a random value in the range |
| `dialog` | | opens the dialog (its choices visible now) |
| `give_gold` | | the hero's gold (negative takes; never below 0) |
| `give_item` | `to` (`pack` default, `player`, an army id: its loot, `named:<id>`: a free worn slot, else the pack) | an item |
| `give_unit` | `named` (optional character id) | a unit joins the hero's army (a full army drops its weakest unit, as DT) |
| `give_xp` | `to` (`player` default, an army id, `named:<id>`) | experience |
| `spawn_army` | `at = [x, y]`, `faction`, `model` (0 to 12, default 5), `troops` (list of units) | a new army on the map, with its id (its name); an id in use spawns nothing |
| `remove_army` | | the army leaves the map (it can come back by an event, as DT's) |
| `teleport_hero` | cell `[x, y]` | the hero's army moves there |
| `teleport_army` | `at = [x, y]` or `to = "<building>"` | an army moves to a cell or a building |
| `set_faction` | `faction`, `of` (`army` default, or `building`) | a faction for an army or a building |
| `set_relation` | `towards` (`player`, `ally`, `neighbour`, `enemy`), `value` (−3 to 3), `of` | the relation of an army or a building towards a side |
| `add_units` | `troops`, `of` (`army` default, or `building`: a garrison) | units join an army or a garrison |
| `cast_on_army` | `in` (`player` or an army id), `minutes = n` or `permanent = true` | the spell in each unit's slot (a slot that is free, or the same spell's) |
| `remove_spells` | the holder (`player` or an army id) | every spell of its units goes |
| `remove_spell_from_book` | the spell | the hero's spell book drops it |
| `victory` / `defeat` | `= true` | the scenario ends (after the dialogs shown; the first ending holds) |

Units and items are `dt:<alias>` or the package's own ids; spells are `dt:spell/<alias>`. A
«holder» is `player`, `pack`, an army id or `named:<id>`.

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
(a warning); more than five choices; a text key missing in every language; unknown `dt:` units,
items and spells; a package that is not in `requires`; a variable declared twice; a trigger id
used twice. The cell bounds are not checked yet: the map's size is known only once maps are read.

### Not in this step

Wishes of the trigger vocabulary left out of slice 1 step 3 (design: "Trigger vocabulary"):
AI army speed (5), an AI army's goal (14, the goal half), the hero's map figure (12), branching
campaigns (10). Garrisons take part only where the table says so: `add_units` and `set_faction` /
`set_relation` (`of = "building"`). Spells on garrisons (1, 7, 9), named characters in AI armies or
garrisons (2), items and experience of garrisons (3, 8) are not in format 1 yet.

## Not in format 1

`maps/`, `art/` and `scripts/` are reserved: they are allowed at the top but not read yet. Also not
yet: `render`, `sim`, `play`, `pack` (the `.rzpack` builder), the Hub.
