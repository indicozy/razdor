# Contract for the parallel campaign work (2026-11-02)

Engine agents and content agents work at the same time. This file fixes the names they share,
so content can be written before the engine part lands. An engine agent implements exactly
these names (it may add, never rename); a content agent uses only these names plus what
`docs/reference/rzpack-format.md` already documents.

## New trigger actions and events (engine: "wishes" agent)

AI army goals (wish 14) and speed (wish 5):

```toml
{ army_goal = "<army-id>", go_to = [x, y] }          # walk to a cell, then stand
{ army_goal = "<army-id>", go_to_building = "<id>" } # walk into a building (attack it if hostile)
{ army_goal = "<army-id>", follow = "hero" }          # or follow = "<army-id>"
{ army_goal = "<army-id>", patrol = [[x, y], [x, y]] } # loop through the cells
{ army_goal = "<army-id>", attack = "hero" }          # or attack = "<army-id>"
{ army_goal = "<army-id>", clear = true }             # back to the original AI
{ army_speed = "<army-id>", percent = 150 }           # 100 = normal
```

Event: `when = { event = "army_arrives", army = "<id>", at = [x, y] }` (also fires for
`go_to_building` with `building = "<id>"` instead of `at`).

Map figures (wish 12):

```toml
{ hero_figure = "dt:<unit alias>" }     # or hero_figure = "default"
{ army_figure = "<army-id>", unit = "dt:<unit alias>" }
```

Garrisons as targets (left out of step 3): wherever an action or condition takes
`army = "<id>"`, it also takes `garrison = "<building-id>"` (spells, units, items, xp,
named-unit checks).

## Campaign graph (engine: "campaign" agent)

`campaign.toml` at the package root:

```toml
start = "brod"

[carry]          # what moves from map to map (all default true)
hero = true      # level, xp, spells, the hero's items
army = true      # the squad, with named units and their items
pack = true      # unworn items
gold = true
vars = true      # every package variable

[[map]]
id = "brod"
[[map.next]]
to = "marka"

[[map]]
id = "marka"
revisit = true   # leaving and returning keeps this map's state (armies, buildings, fog, fired triggers)
[[map.next]]
to = "abbey"
if = [ { var = "ally", op = "==", value = 1 } ]
[[map.next]]
to = "salt"
if = [ { var = "ally", op = "==", value = 2 } ]
[[map.next]]
to = "camp"
if = [ { var = "ally", op = "==", value = 3 } ]
```

- The first `next` whose `if` holds is taken; none: the campaign ends with the map's victory.
- `{ victory = true, next = "<map-id>" }` overrides the graph for that victory.
- `{ travel = "<map-id>" }` leaves the current map for another one without a victory (for the
  hub map 2 and returning to it); the current map's state is kept when that map has
  `revisit = true`.
- `{ ending = "<id>" }` ends the campaign with a final dialog `<id>` and the credits.

## Shared campaign variables (declared once in `triggers/_campaign.toml`)

| var | type | meaning |
|---|---|---|
| `ally` | int | 0 none, 1 the Order of the Ash, 2 the marsh folk, 3 the Free Companies (chosen in «Марка») |
| `crown` | int | standing with the Crown's garrison, -5..5 |
| `order` | int | standing with the Order, -5..5 |
| `companies` | int | standing with the Free Companies, -5..5 |
| `kord` | int | 0 unmet, 1 hired, 2 betrayed, 3 friend |
| `ilm` | int | 0 novice, 1 inquisitor path, 2 heretic path |
| `tess_joined` | bool | Tess walks with the hero |
| `varga_alive` | bool | Varga lives (false once she falls for good) |
| `gates_open` | int | waygates opened so far |
| `caravans_lost` | int | caravans lost on the roads |
| `villages_burned` | int | villages burned by anyone |
| `taint` | int | how far the hero's host is tainted |

Chapter 1's own variables (`dread`, `marsh_trust`, `varga_joined`, `toll_talked`, `mill_burned`, ...)
stay in `triggers/brod.toml` and carry over too.

## Files per chapter (content agents)

Each chapter owns its files and touches no other chapter's:
`maps/<id>/`, `triggers/<id>.toml` (every trigger with `map = "<id>"`), `dialogs/<id>.toml`,
`items/<id>.toml`, `units/<id>.toml`. Text keys are prefixed with the map id (`marka.*`) and
appended to `texts/ru.toml` and `texts/en.toml` in one block per chapter, headed by a
comment `# --- <id> ---`, at the end of the file.

Map ids: `brod` (1), `marka` (2), `abbey` (3а), `salt` (3б), `camp` (3в), `gates` (4),
`barrows` (5).
