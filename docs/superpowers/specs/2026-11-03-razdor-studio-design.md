# Razdor Studio: a Godot-style editor for `.rzpack` packages

Status: design agreed with the owner on 2026-11-03 (grilling session). No code yet.

Related: the package format [`rzpack-format.md`](../../reference/rzpack-format.md), the editor
guide [`rzpack-editor-guide.md`](../../reference/rzpack-editor-guide.md) (the data model, the
reference graph, the checks and the design traps this editor builds on), the v2 design
[`2026-11-02-v2-packages-design.md`](2026-11-02-v2-packages-design.md), and the asset format
project [`2026-11-03-package-assets-design.md`](2026-11-03-package-assets-design.md).

## Goal

A modern visual editor for Razdor 2.0 content packages, laid out like the Godot 4 editor: a
map canvas, a scene tree, an inspector and a code view of the same files, kept in sync. It
replaces the old `.DTm` map editor. It works hand in hand with an AI agent (Claude Code on the
author's own subscription), which edits the same files and can see and test the result.

## Decisions

| # | Decision |
|---|---|
| 1 | The studio edits v2 packages only (the `.rzpack` folder: TOML, PNG, Lua). |
| 2 | The old `.DTm` editor (`src/ui/editor/`, the main-menu entry, the `.DTm`-only parts of `src/editor/`) is removed when studio slice 1 lands. Its format-free core (geometry, the undo command stack, palette logic) moves into the studio. A changelog line says so; the wiki page `tools/map-editor.md` is rewritten for the studio. `.DTm` maps stay editable in 1.x releases and the original DTMapEdit. |
| 3 | A `.DTm` → `.rzpack` import comes in a later 2.x release, after 2.0. Not a priority; it needs its own design (DT events → the trigger vocabulary, `dt:` aliases, what is lossy). |
| 4 | Stack: Rust; egui 0.31 through `egui-macroquad` 0.17 / `egui-miniquad` 0.16 inside Razdor's own macroquad window; `egui_dock` (the release matching egui 0.31) for docking; the map canvas draws with the game's own map renderer; `toml_edit` for format-preserving writes; `notify` for watching the folder; a small code-editor widget of our own (syntect colouring through `egui_extras`, line gutter, diagnostic markers). Rejected: Qt 6 (no way to embed the macroquad renderer, a C++ toolchain in every pipeline, a cxx-qt bridge), Slint and iced (the canvas would need a second renderer, weaker docking), eframe standalone (a second renderer), Godot with gdext (a second engine to maintain), Tauri or web (a split codebase). Known cost: egui stays pinned to 0.31 until `egui-miniquad` catches up, or we vendor and bump the bridge (~600 lines). |
| 5 | The studio is part of the game binary: `razdor --studio [<package folder>]` and a «Редактор» button in the main menu. It sits behind a `studio` cargo feature, on in desktop builds (Linux, Windows, macOS), compiled out of Android and iOS for now. Other platforms stay possible because macroquad runs there. It uses the same install (`RAZDOR_DT_DIR`) for `dt:` references and art. |
| 6 | Files are the truth, as in Godot. Details below. |
| 7 | Layout after Godot 4. Details below. |
| 8 | Look: modern, close to Godot 4's default dark theme (flat dark grey panels, one accent colour, line icons, a clean sans-serif font with full Cyrillic). Nothing of DT's look; DT art appears only inside the canvas. Every studio screen goes through the UI approval rule: off-screen screenshots to the owner before a push. |
| 9 | Interface languages: Russian and English through the UI catalog (`data/lang/ru/studio.txt`, `data/lang/en/studio.txt`), following the game's language setting. The inspector shows a translated label and the raw format key; the code view shows the files as they are. |
| 10 | Built in slices, each usable on its own. Details below. |
| 11 | Built on `v2-packages` as v2's trunk, in agent worktrees rebased onto it. The studio is mostly new files (`src/studio/`), so it stays out of the way of `v2-finish`. |
| 12 | AI: Claude Code on the author's own subscription, no API keys and no model calls inside the studio (consistent with v2 decision 1). The agent edits the package files; the studio gives it a thin MCP server for context and running, and an embedded terminal dock running the author's own `claude`. Generators (ComfyUI for images, audio and 3D, Blender) come in as Claude Code's own MCP servers. Details below. |
| 13 | Package assets (`art/`, `sounds/`, `models/`) are a separate project with its own design: [`2026-11-03-package-assets-design.md`](2026-11-03-package-assets-design.md). The studio previews any file from day one and gets asset pickers once the format reads them. |

## Files are the truth (decision 6)

- The package folder on disk is the only source of truth. The studio holds an in-memory
  document per open file, parsed through `pack::load` for the model and through `toml_edit` for
  writing.
- Each open file is a tab with a dirty marker `(*)`. Edits stay in memory until Ctrl+S or
  "Save all"; Play saves everything first.
- A visual edit (drag, inspector field, brush) patches the file's `toml_edit` document in place:
  only the touched tables change, comments and `# --- generated by … ---` blocks survive. The
  model is re-parsed from the patched text, so the visual views and the code view never hold
  two different truths.
- A text edit is re-parsed after a short pause (~300 ms). While a file does not parse, its
  visual views freeze on the last valid state under a banner ("stale: `maps/brod/map.toml`
  line N: …"), and visual edits to that file are blocked. Other files stay editable.
- Selection is linked both ways: selecting an object on the canvas or in the scene tree scrolls
  the code view to its table and highlights it; the cursor inside a table selects its object.
  This needs a source span per table in the parsed model (`Located` grows a byte range).
- One undo stack per package covers visual edits, text edits and outside changes.
- Outside changes (another editor, an agent, `git checkout`) are seen through `notify`. A clean
  tab reloads silently and records the change as one undo step with its diff; a dirty tab asks
  "file changed on disk: reload, keep mine, show diff".
- Editor-only data (graph node positions, notes, dock layout per package) lives in
  `.editor/`, which the engine ignores. The user's dock layout lives in the user's settings.
- Engine rules the writer must keep (from the editor guide §3): never reorder or delete
  `units/` and `items/` entries in the middle (their order fixes saved ids), keep
  `[[dialog.choice]]`, `[[map.next]]` and `then` order, never invent keys.

## Layout (decision 7)

```
┌─ Menu: Package  Edit  Map  Story  Test  Help ────── [▶ Play map] [▶▶ Play campaign] ┐
├─ Main screens: [ Map ] [ Story ] [ Campaign ] [ Script ] [ Texts ] ─────────────────┤
│ FileSystem       │                                          │ Inspector            │
│──────────────────│              main screen                 │  fields of the       │
│ Scene (open map) │                                          │  selection           │
│  Hero, Buildings │                                          │  ── used by ──       │
│  Armies, Points  │                                          │                      │
│  Objects,        │                                          │                      │
│  Triggers here   │                                          │                      │
├──────────────────┴──────────────────────────────────────────┴──────────────────────┤
│ Bottom: [Problems] [Story trace] [Output] [Find usages] [Claude]                    │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

| Godot | Studio |
|---|---|
| FileSystem dock | the package folder; previews for PNG (and, once the asset format reads them, sounds and `.glb`) |
| Scene tree | the open map's objects grouped by kind, plus the triggers bound to that map |
| Inspector | forms for the selection, generated from the format's fields and the trigger vocabulary (`pack::triggers::{EVENTS, CONDITIONS, ACTIONS}`); a "used by" list from the reference graph; safe rename and safe delete |
| 2D / Script main screens | **Map** (canvas), **Story** (triggers and dialogs), **Campaign** (map graph), **Script** (the TOML/Lua text of the open file), **Texts** (key × language table) |
| Output / Debugger | **Problems** (`pack::check`, click to jump to file and line), **Story trace** (`sim --trace`), **Output**, **Find usages** |
| F5 / F6 | Play campaign / Play this map (`razdor --pack <folder> [--map <id>]`) |

All docks are `egui_dock` tabs: they move, split, tab and float.

### The map canvas

From the editor guide §4: cells from the top left; terrain is `terrain.png` painted with the
16-kind palette as a brush; buildings drawn by their footprint (`rules::world::footprint_cells`,
never reimplemented); live placement errors (overlapping footprints, off-map, things inside a
footprint, anything but a bridge on water, an army on water without a ship); the reach overlay
(`pack::reach`, run on every edit); overlays for `hero_enters` areas (overlaps highlighted),
army goals and patrols as arrows, `spawn_army` points, lantern radii.

## Slices (decision 10)

1. **Map, Script, Problems.** Open a package folder; FileSystem and Scene docks; the Map canvas
   with the game's renderer, the terrain brush, placing and moving the hero, buildings,
   armies, points and objects with live placement errors and the reach overlay; Inspector forms
   for those; the Script screen with linked selection; Problems from `pack::check`; save,
   undo, outside-change reload; Play map. The old `.DTm` editor is removed in this slice.
2. **Story.** The trigger list with When / If / Then forms from `pack::triggers`; dialogs with
   the triggers that answer their choices shown under each choice; the variables view (find
   usages); `hero_enters` areas edited on the canvas; the Story trace panel.
3. **Campaign and Texts.** The campaign graph (ordered `next` edges as numbered priorities,
   hidden edges from `travel`, `victory … next` and `ending`), the carry settings; the key ×
   language table with missing keys marked.
4. **Lints and tests.** The design-trap lints of the editor guide §8; the route test as a CLI
   (`razdor-pack route <path> <route.toml>`) and a "test this route" button; the per-map gold
   budget.
5. **Later (2.x): the `.DTm` import** (decision 3).

The AI pieces (below) come in alongside: the MCP server with slice 1 (it reuses slice 1's
check, snapshot and selection), the Claude dock right after.

## AI (decision 12)

The author's agent is Claude Code running on their own subscription (or any agent that speaks
MCP). The studio never calls a model itself.

- **Edits go through files.** The agent edits TOML, Lua and assets in the package folder like
  any outside editor. The studio picks them up through the watcher; each change is one undo step
  with a diff the author can review and revert. There are no edit tools in the MCP: one write
  path.
- **A thin MCP server** gives the agent what files cannot:
  - `get_selection`: the selected objects, the open map and screen;
  - `snapshot`: a PNG of the canvas, a map or a studio screen, drawn by the game's renderer
    (the agent is multimodal and looks at its result);
  - `query_refs`: the reference graph (who uses var `oath`, what names army `bandits`);
  - `check`: `pack::check` diagnostics;
  - `sim`: the story trace (`razdor-pack sim --trace --json` semantics);
  - `route`: the route test (slice 4);
  - `focus`: point the studio's view at an object or a file line;
  - `preview_asset`: an animated preview of a sprite or model (once the asset format exists).
  Transport: the running studio listens on a local socket; `razdor --studio-mcp` is a stdio
  bridge to it that Claude Code starts. Local only.
- **The Claude dock** is an embedded terminal running the author's own installed `claude` in
  the package folder, started with `--mcp-config` pointing at the studio's MCP. Nothing else is
  started if `claude` is missing; the dock says how to install it.
- **Generators** (ComfyUI for images, audio and 3D via Hunyuan3D or TRELLIS workflows, Blender)
  are Claude Code's own MCP servers, configured by the author. Assets they produce land in the
  package folder and follow the asset rules and the `razdor-assets` skill of the asset project.
- A `razdor-studio` Claude Code skill ships with the studio: how to use the MCP tools, the
  editor guide's traps, and "check, snapshot, look, fix" as the working loop.

## Phase 2: the complete tool (agreed 2026-11-03, second grilling)

UX overhaul: see 2026-11-04-studio-ux-design.md (the workspaces, the Map tools, the Inspector, the shortcuts and the milestones UX1-UX6 that rework this phase's screens).

Slice 1 (map, script, problems, the MCP and the Claude dock) is done. The owner's goal now: a
complete tool, at least everything DTMapEdit does, as GUI. The specs of the original editor are on
the paused `dt-editor` branch, `docs/reference/editor/*.md` (main window and tools, record windows,
new-map and world generators, map check and playability score, testers); its Rust rebuild of the
generators is in that branch's `src/editor/`.

| # | Decision |
|---|---|
| P1 | The studio keeps its Godot look. Every DTMapEdit feature becomes a form or a tool; nothing stays text-only (the Script view remains for whoever wants it). |
| P2 | **Map tools:** all of DTMapEdit's tool pages (terrain, hills, forests, buildings, items on the ground) and modes (move, tree replacement, ignore mountains, burn everything), brushes 1-6 and 9, a minimap dock, the passability grid, patrol-zone and fog overlays. Extras: rectangle and fill, copy/paste of a region with its objects, a stamp library of saved regions (`.editor/stamps/`, or the user's settings for stamps shared between packages). Hills and forests paint and erase many `[[object]]` entries with DT's variant rules. What the format cannot hold is listed as a format gap, never faked. |
| P3 | **Scenario forms:** the map header (title, description, start date, relations, terrain palette, victory), the campaign graph (nodes, ordered `next` edges, hidden edges from travel/victory/ending) and `[carry]`, named characters (where each `named` is given and required), and the texts table (key × language, missing keys marked). |
| P4 | **Story editor:** trigger and dialog forms generated from the vocabulary (`pack::triggers`), plus an editable flow graph over the same files: dialogs and triggers are nodes, choices are output ports, "sets var X" / "if var X" are labelled edges; dragging a choice onto a dialog creates the wiring trigger. Node positions live in `.editor/layout.json`. A variables view with find usages. |
| P5 | **Units and items:** forms for the fields the format has (base + overrides), with DT's cost formula (the game's strength evaluator → tactical cost → gold in bands; `records.md` §5.2, artefact auto-price §6.3) as a live suggested price with a "use" button, and the level 1-5 table. Format gaps (per-level gains, nature, magic school, upgrades) go to a format decision, not the editor. |
| P6 | **Generators:** `dt-editor`'s new-map generator (relief fractal, rivers, coasts, terrain classes, mountains, hills, forests) and world generator (buildings by sector, roads, bridges, economy, armies) ported to write a package map (`terrain.png` + `map.toml`), with the original's dialogs (size, map type, ratios, seed). Later extensions: more map types, regenerate a region, keep story objects; the long-term aim is a rogue-like, D&D-like RPG. |
| P7 | **Testers on the real engine, not the original editor's diverging copies:** the battle tester (two armies from the map, the hero's squad or built from units; N battles over seeds with Razdor's battle engine; win rate, average losses; one battle replayed in the game's battle view) and the AI viewer (the map's AI run N days on the canvas with a timeline slider, army trails, story triggers marked as they fire; reuses the sim). |
| P8 | **Two scores, live, each with a breakdown whose items select their objects or cells:** **Richness**, DT's playability formula (`mapcheck-files.md` §6: content points, events → triggers and dialogs, near duplicates and text length, terrain variety rings, building spacing, the curve) adapted to packages; **Health**, from play tests: every story place reachable, each scripted fight winnable by the expected squad (the battle tester's runs), the per-map gold budget against wages, dead ends (a missable once-only offer, a «later» that never returns, a required named character who can die), and the design traps of the editor guide §8. Health warns, it never blocks. |
| P9 | Order, each a milestone: map tools → scenario forms → story editor → units and items → generators → testers → scores. Each milestone is pushed to `v2-packages` after review; the owner reviews the screenshots of all of them at the end (owner, 2026-11-03), which gates the v2 → `main` merge. |

## Engine changes the studio needs

- `Located` with a byte span per table (linked selection, jump to line); line numbers on every
  diagnostic (editor guide §10).
- `toml_edit` writes for every file kind the studio edits.
- The map renderer callable into a rectangle of a larger window (the canvas) and into an
  off-screen target (`snapshot`).
- The route test as a CLI (slice 4).

## Testing

- Unit tests on the document model: a visual edit produces the minimal `toml_edit` patch and
  keeps comments; a round trip of every example package (`hello-story`, `pepelnaya-marka`)
  through load → no-op edit → save is byte-identical.
- Off-screen snapshots of each studio screen (the `offscreen-snapshot` skill) for the owner's
  approval and the UI regression check.
- An MCP smoke test: start the studio off-screen, call each tool, check the replies.

## Open (defaults proposed)

- Font: Inter or Noto Sans (both cover Cyrillic, OFL) shipped with the studio.
- Icons: Lucide (ISC) as an icon font or SVGs rasterised at build time.
- Whether `egui_dock`'s floating windows are worth it in a single macroquad window, or docks
  only tab and split.
- Phones: the studio is compiled out; a read-only package viewer on phones may come later.
