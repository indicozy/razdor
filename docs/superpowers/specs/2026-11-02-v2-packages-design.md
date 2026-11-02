# Razdor 2.0: content packages and an open engine

Status: design agreed with the owner on 2026-11-02 (grilling session). No code yet.

## Goal

Anyone with an idea can make new maps, stories, dialogs, units, items, spells, upgrade trees
and new mechanics for Razdor, using their own AI agent. Content ships as one Razdor-native
file, a **package** (`.rzpack`). Export back to Discord Times (`.DTm` + mod) is a later,
best-effort feature.

The engine becomes version 2.0.0: the rules are cut into replaceable systems with Lua seams.
This ends the maintenance-mode phase declared on 2026-10-28.

**Hard requirement:** `.DTm` maps, Community Update and Evolution Mod play exactly as in 1.x.
Every step of the refactor keeps the difftests, the community map smoke test and `max_bench`
green, and 1.x saves still load.

## Decisions

| # | Decision |
|---|---|
| 1 | The "agent" is external: Razdor ships a spec, JSON Schemas, the `razdor-pack` tool and an agent skill; the creator uses their own agent (Claude Code, Codex, local Qwen, ...). No LLM inside the game. |
| 2 | Razdor-first: packages may use mechanics DT cannot express (Lua). Export to DT is best-effort, later. |
| 3 | Packages mostly build on the player's install (reference its units, art, terrain, texts) and may bring their own art (including generated sprites). A package with no references is standalone. Package content is MIT-licensed; a package never contains the original's files. |
| 4 | On disk a package is a folder of text files and pictures; it ships as one compressed archive (`.rzpack`, zip). |
| 5 | Data files are TOML, with published JSON Schemas. Terrain is a separate indexed PNG (one colour per terrain type). |
| 6 | Two layers of logic: declarative TOML triggers (the common case; mirror DT's events; checkable and exportable) and sandboxed, deterministic Lua 5.4 (`mlua`, vendored) for new mechanics. Lua gets no clock, files or network; randomness only from the game's seed. |
| 7 | Delivered in slices (below). |
| 8 | References are namespaced ASCII ids: `dt:` for base content via a shipped Latin alias table generated from Community Update (`dt:swordsman`; Russian section names accepted as a fallback), `<package>:<id>` across packages. Display names live in per-language text tables, never in ids. |
| 9 | Spec, schemas, loader and `razdor-pack` live in the razdor repo. A separate MIT starter-kit repo holds a template package, the agent skill and examples. |
| 10 | `razdor-pack` has four commands, all in slice 1: `check`, `render`, `sim`, `play` (below). |
| 11-12 | Distribution: open uploads to the Hub plus local import (below). |
| 13-14 | Engine architecture: Rust core of replaceable systems; DT's systems stay in Rust (below). |
| 15 | The format and the v2 system interfaces are designed together around two example packages; slice 1 is implemented first, then the refactor. |
| 16 | v2 work happens in worktrees and lands on `main` in small green steps (trunk rule kept, weekly 1.x releases continue). New features sit behind a toggle or a separate binary until 2.0.0. Revisit if this hurts. |
| 17 | A package may declare its base (`community-update` default, `evolution`, another Hub mod) and depend on other packages by id and version range. An adaptor/addon model for the original's mods is a later idea, not designed now. |

## Package layout (slice 1, sketch)

```
my-story/
  package.toml        # id, version, format = 1, title, author, license = "MIT",
                      # base = "community-update", requires = ["elves-pack@^1.0"], rules = "dt"
  texts/ru.toml       # display texts per language, keyed by id
  texts/en.toml
  units/*.toml        # base = "dt:swordsman", changed stats, names via text keys
  items/*.toml
  maps/<map>/map.toml # size, objects, buildings, armies, points, start, victory/defeat
  maps/<map>/terrain.png
  dialogs/*.toml      # dialog trees with choices, conditions, effects
  triggers/*.toml     # when -> if -> then; DT event conditions and results plus new ones
  art/*.png           # the creator's own pictures (slice 2)
  scripts/*.lua       # slice 3
```

The exact schema of the implemented subset is in [`docs/reference/rzpack-format.md`](../../reference/rzpack-format.md).

## The `razdor-pack` tool

- `check`: schema, every reference (ids, art, text keys), reachability (every dialog and
  trigger can fire, a win condition exists and is reachable), missing translations. Errors as
  `file:line` with a fix hint; `--json` for agents.
- `render`: off-screen PNGs of the whole map, a unit card, a dialog window (reuses the
  off-screen snapshot machinery), so a multimodal agent can look at its work.
- `sim`: AI plays the map headless for N days (as the community map smoke test): panics,
  softlocks, unreachable win, battle-balance stats.
- `play`: opens the package in Razdor.

## Engine: how packages run

- A package is compiled at load time into the engine's model through a translation layer, so
  the engine's internals can change without changing the package format.
- v2 cuts `src/rules/` into systems with clear interfaces: world turn loop, movement, battle,
  AI, economy, events, towns, heroes. DT's implementations stay in Rust (speed, tested
  parity); a `.DTm` never touches Lua.
- A package selects its ruleset in `package.toml` (`rules = "dt"` default) and can, in order of
  depth:
  1. configure data seams: numbers and lists fixed in code today (formation size, resource
     types, hero classes, terrain types and costs, the day/week structure, map size);
  2. hook steps from Lua (damage, battle turn order, morale, AI target choice, income,
     victory checks);
  3. replace a whole system with a Lua implementation behind its interface (e.g. a different
     battle system). Hot paths (battle maths, pathfinding, AI search) stay in Rust and are
     offered to Lua as a library.
- The refactor is a step-by-step ("strangler") refactor on `main`, never a rewrite.

## Slices

1. **A map with a story** (1.x): one map, dialogs with choices, TOML triggers, units and items
   derived from base ones, `check`/`render`/`sim`/`play`, the agent skill and starter kit,
   `.rzpack` in the Hub (upload and local import).
2. **A world**: campaigns of linked maps with the hero carried over, upgrade trees, spells,
   the creator's own PNG art; depth-1 data seams.
3. **New mechanics**: Lua with depth-2 hooks; then the first replaceable system (battle).
   2.0.0 ships when Lua and the first replaceable system work.
4. **Later**: export to `.DTm` + mod (best-effort), a `.DTm` -> package converter (gives agents
   real examples), the original-mod adaptor.

## Distribution

- Submit: a PR to `razdor-community` adding the `.rzpack`, or the Discord bot's `/publish`
  (attach the file; the bot opens the PR and credits the Discord name).
- Gate (CI): `check` and `sim` pass, size <= 50 MB, MIT and author in the manifest, no
  original game files (hash guard against the install's files).
- Publish: auto-merge on green CI, catalog rebuilt. A new version of a package id is accepted
  only from the same author.
- Takedown: the owner can unlist (`status = "unlisted"`; stays installed for those who have
  it). The Hub card has a "report" button posting to the Discord reports thread (feeds the
  reports backlog).
- Local: any `.rzpack` opens directly (Hub "Import", drag and drop, "open with"), marked as
  unchecked.

## Trigger vocabulary: what mapmakers asked for

Triggers (TOML) must cover everything DT's events and the Community patch's opcodes do (a
package converted from a `.DTm` loses nothing), plus a mapmaker's wishlist (Discord,
2026-11-02, answering the owner's call for engine ideas). Mapmakers on the server also
asked that the Community patch's planned functions and current maps be studied first.

| # | Kind | Wish |
|---|---|---|
| 1 | result | remove spells from the player's army, an AI army or a garrison |
| 2 | condition | a named unit of a given class is present (e.g. infantryman Ivan, not any Ivan) in the player's army, an AI army or a garrison |
| 3 | result | give or replace personal or ordinary items of a given unit (player, AI army, garrison) |
| 4 | result | add or change named or ordinary units in any AI army or garrison |
| 5 | result | change an AI army's speed |
| 6 | result | change the faction of an AI army or building, and its individual relations |
| 7 | result | cast or change spells (permanent or timed) on a whole army or garrison, or one unit (the player's too) |
| 8 | result | give or change experience of AI armies and garrisons |
| 9 | condition | spells on units, a whole army (player, AI) or a garrison |
| 10 | campaign | branching campaigns (several possible next scenarios) |
| 11 | result | remove spells from the player's spell book |
| 12 | result | change the map figure of the hero or an AI army (knight, bandit, undead, ...) |
| 13 | result | a random value for a flag or counter |
| 14 | condition / result | an AI army's position; set an AI army's goal |
| 15 | result | teleport the player's or an AI army to a cell or a building |

All of these fit the declarative trigger layer (slice 1 for those on the map, slice 2 for
branching campaigns); none needs Lua.

## Text in any script

Package texts may be in any language and script (the owner, 2026-11-02): Cyrillic, Latin,
Indic scripts, Arabic and Hebrew, Chinese and Japanese, and so on. Razdor already shapes
complex scripts and falls back to bundled and system fonts (`src/ui/script_font.rs`); the
gaps to close:

1. Right-to-left: bidi reordering of mixed lines (Arabic + digits + Latin), right-aligned
   RTL paragraphs, wrapping in visual order.
2. Phones: look up system fonts on Android (`/system/fonts`) and iOS, or bundle Noto subsets
   for the major scripts, so Arabic, CJK and other Indic scripts don't vanish there.
3. Typing such text in input fields (save names, chat-like fields, the future editor).
4. `razdor-pack check` warns when a text uses characters no bundled font covers (it may not
   show on phones).

## 2.5D view and 3D models (decisions 18-20, 2026-11-02)

- **What (18):** a 2.5D camera over the world map: a tilted perspective view with zoom; the
  original's art stands up as billboards; packages may give units, buildings and objects glTF
  models. A `.DTm` can be viewed this way too, as an option; the flat view stays the default
  and unchanged. Battles stay 2D (a 3D battle view may come later as a replaceable system).
  Rules never see the view: they stay on the grid.
- **How (19):** on macroquad/miniquad: the `gltf` crate for models, Razdor's own shaders (lit
  models, billboards, terrain), GPU skinning. The renderer sits behind its own system
  interface in v2.
- **Model rules (20):** glTF 2.0 binary (`.glb`), textures embedded; Y up, facing +Z, one map
  cell = 1.0, pivot at the feet in the cell's centre (a unit ~0.8 tall, a building up to 3x3
  cells). Budgets: unit <= 5,000 triangles, object <= 3,000, building <= 20,000; textures
  <= 1024x1024, at most 2 per model; at most 40 bones. Clips: `idle` required for units,
  `walk` on the map (else the model glides); `attack`, `hit`, `die` reserved for a 3D battle.
  `units/*.toml` gets `model = "art/models/x.glb"`; without one the sprite is a billboard.
  `razdor-pack check` checks conventions, budgets and clips; `razdor-pack render` makes a
  turntable PNG.
- **Steps:** (1) the tilted camera with billboards and picking, behind a setting, off by
  default; (2) static glTF models (load, budget check, lit shader, turntable render);
  (3) skinned animation (`idle`, `walk`); later camera rotation (needs depth sorting).

## Open (not yet discussed; defaults proposed)

- **Example packages**: (1) a plain story map; (2) one with a non-DT mechanic, proposed: a
  different battle grid. To be picked by the owner.
- **Languages**: any one language required; Russian recommended for the Hub; the agent skill
  can translate.
- **Own art**: sizes and palette matching DT's pictures; a style guide in the starter kit.
- **Agent skill**: targets Claude Code first (`CLAUDE.md`/`AGENTS.md` in the starter kit),
  usable by any agent that runs shell commands; a getting-started page for non-programmers.
- **Lua limits**: instruction and memory budget per call; profile on phones.
- **Saves and versions**: a save records package id, version and hash; a package update
  mid-campaign keeps the old version until the campaign ends; `format` bumps only on
  breaking changes, older formats keep loading.
- **Visual editor**: creation is agent + text for now; the map editor may open packages later.
- **Wiki**: ru/en pages for creators once slice 1 ships.
- **Trigger wishes left out of slice 1 step 3 (2026-11-02)**: AI army speed (wish 5), an AI army's
  goal (14, the goal half: the army's position is in), the hero's map figure (12), branching
  campaigns (10, slice 2). Garrisons only where `add_units`, `set_faction` and `set_relation`
  take `of = "building"`: spells on garrisons (1, 7, 9), named characters in AI armies and
  garrisons (2), items and experience of garrisons (3, 8). Their holders need the garrison's
  troops as a holder kind in `rules::triggers` (the code has the troops, not yet the holder).
  What the runtime needs to meet a real map: the map format's army and building ids (`Resolve`),
  and the save's package id and version (a saved story is attached again after a load).
