# `.rzpack` for AI authors: format changes that make agents faster and safer

Status (2026-11-04): the owner said yes to all of it. Built on `v2-packages`: B (content-independent seeds),
A (`tests/*.toml`, `razdor-pack test`, MCP `run_test`; the whole reference campaign tests in ~2 s),
D (`razdor-pack schema`, JSON Schema, did-you-mean, MCP `schema`) and G (the three lints). Not started:
C + E + F (format 2: scoped names, scenery out of map.toml, texts per map, `razdor-pack upgrade`); the
studio session adapts the studio after it.

Written after «Пепельная Марка» 0.3 (seven maps, ~400 triggers), which was authored almost
entirely by coding agents working in parallel. Most of the time did not go into writing the
story: it went into finding hidden couplings between chapters and into slow verification. This
proposal fixes the format where it made agents slow or wrong. It keeps TOML, the trigger model
(when / if / then), and every existing action.

## 1. What went wrong, measured

| # | Problem | What it cost on «Пепельная Марка» |
|---|---|---|
| 1 | Names are package-wide: a spawn named `kord_band` in the camp silently took over the Marka army with the same name. | ~4 hours of bisecting by an agent; 19 tests in three other chapters failed with no visible link. |
| 2 | The story seed is the FNV hash of every package file (`src/pack/open.rs` `story_seed`): a text edit in one chapter reshuffles every battle in all chapters. | Tests that passed by luck failed after unrelated edits; agents could not tell balance from noise. Several bisects started on this false lead. |
| 3 | Content can only be verified by Rust tests (`src/pack/playthrough*.rs`): 1.5–5 min per run, an engine rebuild per change, and the fast `sim` does not walk the hero. | Most agent wall time. The `sim` four-routes test reported a false loss on the marsh route. |
| 4 | No discoverable vocabulary: actions and events are listed only in prose in `rzpack-format.md`. | Three agents invented the same heal-and-revive action under three names (`restore_squad`, `restore_army`, `heal_army`). |
| 5 | `map.toml` is ~13,700 lines, of which ~2,700 `[[object]]` tables are trees and rocks; the ~25 armies and buildings that matter are buried in it. | Agents read it in slices, edit by line number and miss entities. |
| 6 | Texts for all maps live in two files (`texts/ru.toml`, `texts/en.toml`). | Every parallel merge conflicted there. |
| 7 | Surprising semantics: `once = true` on a greeting means the second meeting is a battle; ending conditions captured at map start. | One softlock (the inquisitors blocking the abbey gate) and one frozen-ending bug. |

## 2. Changes, in order of value per effort

### A. Package tests: `tests/*.toml` and `razdor-pack test` (fixes 3)

A package carries its own playthrough scripts, run by the engine headless in seconds, no Rust:

```toml
# tests/marsh_route.toml
campaign = true            # play through campaign.toml from its start; or map = "marka"
seeds = 15                 # battle seeds 1..15; the test passes on ≥ min_rate of them
min_rate = 0.7

[[step]]
walk = [52, 44]
[[step]]
choose = { dialog = "patrol_greet", choice = "pass" }
expect = { battle = false }
[[step]]
walk_to_building = "reedmere"
[[step]]
wait_days = 3
[[step]]
expect = { var = "marsh_trust", op = ">=", value = 3, alive = "tess", map = "salt" }
[[step]]
expect = { ending = "end_free" }
```

- Steps: `walk`, `walk_to_building`, `walk_to_army`, `wait_days`, `choose`, `hire_all`, `fight` (auto-resolve with the seed), `expect` (vars, squad size, gold, map, ending, dialog seen, battle happened, max gap between beats).
- `razdor-pack test <pkg> [--only name] [--json]` runs them in parallel, one line per test and seed, and on failure prints the log (as `Run::log` does now). Target: a full campaign route in under 10 s.
- The existing harness (`src/pack/playthrough.rs`: `walk_to`, `wait_days`, `hire_all`, battles) is the engine; this is a reader for it plus a CLI. The Rust playthrough files of «Пепельная Марка» move to `tests/*.toml` over time.
- The studio's MCP server (`src/studio/mcp/tools.rs`) gets a `run_test` tool beside `check` and `sim`.
- Removes the misleading part of the `sim` (its auto-win without walking): routes are checked by walking.

### B. Seeds that do not depend on content (fixes 2)

- Battle and story RNG are seeded from `(package id, map id, run seed)`, never from the content hash. The content hash stays for what it is used for: identifying the package in a save (`ScenarioRef`).
- A test fixes the run seed; the same seed gives the same battles after any unrelated edit.
- Small engine change; saves keep their own RNG state, so old saves are unaffected.

### C. Names scoped by map (fixes 1)

- Every army, building, spawn, dialog and area id belongs to its map. A trigger in `triggers/marka.toml` names `kord_band` and means Marka's. A cross-map reference is explicit: `camp:kord_band` (rare: carry-over is by vars, not names).
- Variables stay package-wide (they are the carry), declared once in `[vars]`.
- `razdor-pack check` errors on an unqualified name that does not exist on the trigger's map, with the closest names as a hint.
- Format bump to `format = 2`. The engine keeps reading `format = 1` as today. «Пепельная Марка» is converted by a one-off script; the studio (`campaign_graph`, `characters::rename_plan`, `query_refs`) resolves names per map.

### D. A machine-readable vocabulary (fixes 4)

- `razdor-pack schema [--json]` prints every event, condition and action: its keys, types, defaults, one example, one sentence. It is generated from the parser (`src/pack/triggers.rs`) so it cannot drift.
- A JSON Schema file for trigger, dialog and map TOML is emitted from it, so editors and agents get completion and validation.
- `check` on an unknown action suggests the nearest known one (`heal_army` → "did you mean `restore_army`?").
- The MCP server gets a `schema` tool.
- `docs/reference/rzpack-format.md` keeps the prose; its tables are generated from the same source.

### E. Terrain and scenery out of `map.toml` (fixes 5)

- `map.toml` keeps only what a story touches: hero, armies, buildings, areas, victory. Target: a few hundred lines.
- Scenery (trees, rocks, decorations) moves to `scenery.toml` or a compact grid file next to `terrain.png`. The studio writes it; agents rarely need to read it.
- Format 2 only; a converter splits format 1 maps.

### F. Texts per map (fixes 6)

- `texts/<lang>/<map>.toml` plus `texts/<lang>/common.toml`. Keys stay as they are. `check` still reports a key missing in any language.
- Parallel agents working on different maps then never touch the same text file.

### G. Explicit semantics and lints (fixes 7)

- `once` is split into `once = "ever" | "per_meeting" | "per_visit"`, or a lint warns on `once = true` with `hero_meets_army`.
- Conditions are always evaluated when the trigger fires; a lint warns when a value is read into a var at `game_start` and only used much later.
- A softlock lint: an army that greets on a building's only approach must have a choice that moves it, or check warns.

## 3. Order and cost

| Step | What | Size | Format bump |
|---|---|---|---|
| 1 | B: content-independent seeds | small (a day) | no |
| 2 | A: `tests/*.toml` + `razdor-pack test` + MCP `run_test` | several days | no (additive) |
| 3 | D: `schema`, JSON Schema, did-you-mean | 1–2 days | no |
| 4 | C + E + F: format 2 (scoped names, split map, per-map texts) + converter + studio update | about a week | yes |
| 5 | G: lints | small, ongoing | no |

Steps 1–3 are additive and give most of the gain. Step 4 touches the studio, which another session
is building on the same branch: plan it with that work, after the studio's current UX milestones.

## 4. How to know it worked

Re-run a narrative or balance pass on one chapter of «Пепельная Марка» with an agent, before and
after:

- the agent's wall time;
- the number of full Rust test runs it needed;
- the failures it chased that were not in its chapter;
- whether it invented an action that already exists.

The target is half the wall time and no cross-chapter chases.
