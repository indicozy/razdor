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
  entry is a warning and is not read. Only `texts/`, `units/` and `items/` are read in format 1.
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

## Not in format 1

`maps/`, `dialogs/`, `triggers/`, `art/` and `scripts/` are reserved: they are allowed at the top
but not read yet. Also not yet: `render`, `sim`, `play`, `pack` (the `.rzpack` builder), the Hub.
