# Editor UX patterns for Razdor Studio

- **Question:** how other editors lay out their workspaces (panels, tools, shortcuts, object picking, brush feel), and what Razdor Studio should copy or avoid, per ticket #37 and the owner's map decisions in ticket #36 (not reopened here).
- **Date:** 2026-11-04. Sources accessed that day. Numbers in brackets point to the Sources list at the end.
- **Scope:** eight editors: Tiled, LDtk, Godot 4 TileMap, Krita, Photoshop, RPG Maker MZ, VCMI (Heroes III), Warcraft III World Editor.
- **How sources were picked:** first-party manuals and docs first. Where a page could not be fetched (Adobe returned 403 on every helpx page; some Krita and Tiled guessed URLs returned 404), the claim comes from a search excerpt and is marked as such. Community pages are used only where no primary source exists.
- **Images:** no images from Discord Times are used or needed. This is text research only.

## Summary

- **Tool letters and modifiers are the most consistent cross-editor pattern.** Tiled and Godot use Shift for "line" or "add", and Ctrl for pick or remove. Photoshop uses Alt to subtract [19]. The owner's Alt-subtract choice matches Photoshop, but Tiled and Godot use Ctrl to remove from a selection [3][8]. Document this in the shortcut help.
- **Eye and lock per layer is standard.** Tiled's layers view and Krita's layers docker both use an eye toggle for visibility and a lock for editing [2][16]. Krita separates alpha lock from edit lock [16]. Razdor's per-layer eye and lock fit this.
- **Separate definitions from instances.** LDtk edits a layer or entity definition once and every instance follows [10][13]. This is the best model for "layer data with ids that user layers can join later": keep layer definitions in data and per-map layer instances keyed by id.
- **Inspector: Godot is the reference.** It has a field search, collapsible sections, a revert icon for values that differ from the default, and a favourites menu [9]. Razdor's markers and resets match this.
- **Brush size with [ ] is established.** Photoshop's [ ] keys are documented in Krita's Photoshop guide [17], and Krita's brush size is Shift-drag [15]. Razdor's [ ] keys and Shift-click line match Tiled and Godot [3][8].
- **Object placement preview is common.** VCMI shows a transparent preview under the cursor, and right-click cancels [26]. Tiled object tools and LDtk show a preview too [6][11].
- **Terrain and random variants are first-class in Tiled and Godot.** Godot has Patterns and Terrains tabs and a scattering value [8]. Tiled has terrain sets, a terrain brush, and random mode with per-tile probability [3][4]. This supports the owner's density and scatter decision.
- **Test workspace:** no reviewed editor has a dedicated test workspace. RPG Maker MZ puts a Test command in the event right-click menu [23]. That is the closest precedent for «Тест» as a per-event or per-map action.
- **Icon set: Lucide.** One stroke width, a 24 px grid, ISC and MIT licences [30][31]. Phosphor is MIT and offers several weights [32][33]. Rationale in the Open questions.

## Per-editor survey

### 1. Tiled (mapeditor.org)

- **Sources:** [1][2][3][4][5][6]. Shortcut page [7] returned 404.
- **Layout:** the fetched manual pages describe layer types and the layers view (eye and lock) but do not describe default panel positions or a minimap. Default placement is **unverified** from these pages.
- **Tools and shortcuts:** stamp brush (B), terrain brush (T), bucket fill (F), shape fill (P), eraser (E), rectangle select (R), magic wand (W), select same tile (S) [3]. Flip X and Y; rotate Z and Shift+Z [3]. Ctrl+Z undoes [4]. Ctrl+1 to Ctrl+9 stores a stamp, 1 to 9 recalls it [3].
- **Action search and file jump:** Ctrl+Shift+P opens the action search and Ctrl+P opens a project file, per the intro page's summary [1]; the shortcut page that would confirm them returned 404 [7], so treat these as **single-source**.
- **Modifiers in tile editing:** Shift draws a line between two clicks (stamp) and erases across all layers (eraser, Tiled 1.5+). Ctrl+Shift draws an ellipse from the first point (stamp). Selection: Shift adds, Ctrl subtracts, Ctrl+Shift intersects [3]. Alt is used by the terrain brush and shape fill [3].
- **Object picking and placement:** each object type has an insert tool (R, I, C, P, T, V, X). A preview follows the cursor; Escape or right-click cancels. Select Objects (S) picks by click or lasso. Ctrl toggles grid snapping, arrow keys nudge a pixel, Shift+arrow moves a tile [6].
- **Brush feel:** the terrain brush routes between terrains that have no direct transition, so the edge stays connected [4]. Random Mode and Terrain Fill Mode weight tiles by probability [3][4].
- **Properties:** custom properties sit in a context-sensitive Properties view with a "+" to add a typed field. Types include bool, int, float, color, file, object and string. Enums and classes are project-level types [5].
- **Copy:** stamp capture on right-click; stored stamps (Ctrl+1..9); terrain sets with probabilities; typed properties with a class on a tile.
- **Avoid:** Ctrl-to-subtract conflicts with the owner's Alt choice; Alt's meaning changes between tools (centre a shape vs. 180-degree terrain edit) [3].
- **Source quality:** primary (project's own manual, current stable). Shortcut page missing (404), so shortcuts come from the per-tool pages only.

### 2. LDtk (ldtk.io)

- **Sources:** [10][11][12][13][14].
- **Layout:** the main side panel has a top button bar (project definitions, world map, cheat sheet), a layer instance list below it, and a palette whose contents change with the active layer. Compact bar mode is toggled with Tab [10]. The reviewed pages do not describe a separate level view or an entity inspector. **Unverified.**
- **Tools and shortcuts:** H opens the shortcut cheat sheet; Tab toggles compact mode [10]. No other LDtk shortcuts were verified.
- **Object picking and placement:** entities live in an entity layer. Entities can be a rectangle, circle or tile, can have a count limit (adding a second `PlayerStart` moves the first), and carry typed custom fields [11].
- **Brush feel:** IntGrid layers store integer cell values. Auto-layers and rules paint tiles from patterns of IntGrid values [13][14]. Search excerpts only; no primary page fetched for rules.
- **Copy:** definitions versus instances; each level holds one instance per layer definition [13]. Field types with limits and default values [11].
- **Avoid:** LDtk's entity-field inspector and layout are not covered in what I could fetch; do not copy a layout that I could not check.
- **Source quality:** primary (project docs). Some pages are old (about 2021 to 2023 per search metadata) and the inspector is **unverified**.

### 3. Godot 4 TileMap editor and Inspector

- **Sources:** [8][9].
- **Layout:** the TileMap editor is a bottom panel that opens when a TileMapLayer node is selected. Its toolbar sits at the top, with a tile palette and Patterns and Terrains tabs. Layer buttons are in the top right [8]. Inspector: header with the node's icon and name, a search bar, collapsible sections, a revert icon on changed values, and Expand Non-Default and Favorite actions [9]. The Inspector page does not describe drag-to-reorder. **Unverified.**
- **Tools and shortcuts:** Selection (click or drag rectangle; Shift adds, Ctrl removes; Del deletes; Ctrl+C and Ctrl+V copy and paste; Right-click or Escape cancels a paste). Paint (drag to place; right-click erases; Shift before pressing draws a line; Ctrl+Shift before pressing draws a rectangle; Ctrl-click picks). Line, Rectangle, Bucket Fill (with a Contiguous checkbox, no shortcut listed), Picker (Ctrl-click) [8].
- **Object picking and placement:** tile palette selection, then paint, line, rectangle or bucket. Random and scattering apply to Paint, Line, Rectangle and Bucket Fill; Eraser ignores them [8].
- **Brush feel:** Scattering above 0 leaves some cells empty; randomness is a per-tool option [8].
- **Copy:** Shift-line, Ctrl+Shift-rectangle, Ctrl-pick, Patterns and Terrains as tabs, the revert icon in the inspector [8][9].
- **Avoid:** Godot uses Ctrl to remove from a selection, which conflicts with the owner's Alt-subtract [8]. The inspector "favourite" feature adds a second place to look for a field; keep it out of v1.
- **Source quality:** primary (Godot docs, stable branch).

### 4. Krita

- **Sources:** [15][16][17][18].
- **Layout:** dockers and toolbars are movable. The toolbox holds the tools. Tool options sit in a docker by default and can move next to the brush toolbar. Status bar shows selection mode, active brush preset, colour space and image size. Workspace chooser sits at the top right of the toolbar. Ctrl+Enter searches actions [18]. Dockers are opened from the Settings menu [18]. Canvas layout details were not found. **Partly verified** (search excerpts).
- **Tools and shortcuts:** B selects the brush tool. Shift-drag resizes the brush; the bracket keys are also listed for brush size [15][17]. Ctrl temporarily switches to the colour sampler [15]. Shift+Ctrl+L toggles assistant snapping [15]. Ctrl+Shift+G makes a clipping group [16]. Ctrl+E merges down; Ctrl+Shift+E merges all [16][17]. Alt+Shift intersects a selection [16][17].
- **Layers:** the Layers docker has an eye toggle for visibility, an alpha lock (paint layers only), and an edit lock that blocks changes. Multiple layers can be edited together in the properties window (F3) [16].
- **Object picking and placement:** not applicable to Krita (paint application).
- **Brush feel:** Smoothing (stabilizer) modes and a Pixel mode [15]. Strokes are smoothed by default options; the stabilizer finishes a line even when the hand stops.
- **Copy:** brush size by Shift-drag and bracket keys; layer eye and lock; alpha lock as a per-layer protection; workspaces saved and reloaded.
- **Avoid:** Krita's clipping-group and alpha-inheritance model is not needed for a map editor. Keep stabilizer smoothing out of the map brush: tile strokes should be exact.
- **Source quality:** primary (Krita manual 5.3.0). Several pages were not fetchable (404 on guessed URLs); the interface and some layer details come from search excerpts and are marked.

### 5. Photoshop (Adobe)

- **Sources:** [19][20][21]. All helpx pages returned 403 to the fetch tool; the claims below come from search excerpts only.
- **Layout:** toolbar on the left, options bar above the canvas, panels on the right. This is the owner's model for «Карта». The layout detail was **not verified** from a fetched Adobe page; the owner's decision already covers it.
- **Tools and shortcuts:** Marquee, Lasso, Magic Wand. Add with Shift; subtract with Alt (Windows) or Option (Mac); intersect with Alt+Shift [19]. Options bar buttons do the same job [19]. Magic Wand's Contiguous option picks non-touching areas when off [20]. Shortcut letters from the keyboard shortcuts PDF [21] could not be read.
- **Brush size:** [ ] keys are documented in Krita's Photoshop guide [17], not in Adobe's own pages I could read.
- **Copy:** Shift add, Alt subtract, Alt+Shift intersect, options bar mode buttons; Contiguous as a toggle.
- **Avoid:** nothing specific from the excerpts. Adobe's layer panel details (eye and lock) are **unverified** from a fetched page; the eye and lock pattern is confirmed by Tiled and Krita instead.
- **Source quality:** primary in principle (Adobe help), but not fetched. Treat the selection modifiers as search-excerpt claims that need a quick check in the live app before they go into the help text.

### 6. RPG Maker MZ

- **Sources:** [22][23][24][25]. The database page [25] is a search excerpt only.
- **Layout:** Map editor: toolbar with the Map mode and drawing tools, Map List at bottom left, Map View on the right, Tile Palette with tabs A to E [22]. Event editor: an event page with tabs; the event command list (top to bottom) with a right-click menu [23]. Database: a side menu of data types, a list on the left and parameters on the right [25].
- **Tools and shortcuts:** map tools are Pencil, Rectangle, Ellipse, Flood Fill, Shadow Pen, Region; Shift during drawing disables autotile temporarily; layer selection is Auto or Layers 1 to 4 [22]. Event command list: Shift-click selects a range [23]. The event right-click menu includes Test [23].
- **Object picking and placement:** events are placed on the map, with an appearance condition set (switches, variables, self switches, items, actors). When several pages qualify, the highest-ID page wins [23]. Events have triggers: action button, player touch, event touch, autorun, parallel [23].
- **Brush feel:** Pencil, Rectangle and Ellipse draw a dragged shape; Flood Fill fills connected same-type tiles [22].
- **Copy:** the Test command in the event menu (for «Тест»); page-based events with conditions; command list with branch and loop blocks [24].
- **Avoid:** the event command list is a long flat script; for Razdor's «Сюжет» use it only as a reference for conditions and pages. The Region tool's exact behaviour is **unverified**.
- **Source quality:** primary (official MZ help, rpgmakerofficial.com). The database layout comes from a search excerpt, not a fetched page. The event trigger values are confirmed by the official page; a third-party MCP page was not used.

### 7. Heroes of Might and Magic III map editor (VCMI)

- **Sources:** [26][27]. No official Heroes III editor manual was found: **gap**. VCMI's guide is the best available source; VCMI is a community reimplementation, not the original.
- **Layout (VCMI guide):** toolbar (New, Open, Save, Fill), a brush panel, an object browser with a terrain filter, a preview area on the left, an inspector tab for object properties, and a Map menu [26]. The guide has a screenshot but no textual panel positions.
- **Tools and shortcuts:** brush sizes 1, 2 and 4 (1x1, 2x2, 4x4); [ ] for non-additive rectangle selection; E for object erase; Delete; right-click cancels placement [26]. Ground/underground toggle, grid and passability overlays are buttons [26].
- **Object picking and placement:** drag an object from the browser onto the map as a transparent preview; release to place. Select an object with a deselected brush and edit it in the inspector. Multi-object editing is not supported [26].
- **Brush feel:** terrain brush selects tiles (left-click selects, right-click removes the selection), then applies terrain; Fill places obstacles that may spill into neighbours [26].
- **Copy:** drag-from-browser placement with a preview; 1/2/4 brush sizes; the inspector as a per-object panel.
- **Avoid:** [ ] means rectangle selection in VCMI and brush size in Photoshop and Krita [17][26]. Razdor should follow Photoshop (owner's choice) and document it. Also avoid the single-object-only inspector: Razdor should support multi-select for field edits in Данные at least.
- **Source quality:** the VCMI guide is first-party for VCMI. The wiki is archival [27]. For the original H3 editor, **no primary source was found**.

### 8. Warcraft III World Editor

- **Sources:** [28][29]. No Blizzard manual found in search; the reference is the Warcraft III wiki and a Hive Workshop tutorial (search excerpt). **Gap:** no official Blizzard documentation was reachable.
- **Layout:** separate editors reached from function keys: Terrain (F3), Object Editor (F7), Trigger Editor (F4), Sound (F6), Campaign (F5), AI and Object Manager (both listed under F8 on the wiki page, **unclear**) [28]. The wiki says the Trigger Editor is the most advanced part [28].
- **Object editor:** tabs for units and items; with the expansion, destructibles, doodads, abilities, buffs and upgrades [28]. A Hive tutorial lists the same categories [29].
- **Trigger editor:** a visual trigger editor. Hive Workshop says the editor reads UI files such as UI\TriggerData.txt [29] (search excerpt).
- **Object picking and placement:** the terrain editor places units, doodads and regions [28].
- **Copy:** the object editor's categories as a list of data types; the trigger editor's event-and-condition model is a reference for «Сюжет».
- **Avoid:** the wiki says Blizzard does not support the editor, and it recommends HiveWE as a replacement [28]. Avoid a trigger editor that is only a text-like tree without a way to find a trigger by name (the Object Manager cross-reference is the useful part, but its details are **unverified**).
- **Source quality:** the wiki is secondary; the Hive pages are community. No Blizzard primary source was reachable.

## Cross-cutting patterns

| Pattern | Editors (sources) | Razdor note |
|---|---|---|
| Eye and lock per layer | Tiled [2]; Krita [16] | Owner's decision, confirmed |
| Shift = add or line; Ctrl = remove or pick | Tiled [3]; Godot [8] | Owner's Alt-subtract follows Photoshop [19]; say so in help |
| Alt = subtract/intersect in selections | Photoshop [19]; Krita [16][17] | Owner's choice matches |
| Shift-click straight line | Tiled [3]; Godot [8] | Owner's choice matches |
| [ ] brush size | Photoshop, via Krita [17] | Owner's choice matches |
| Transparent placement preview; right-click cancel | Tiled [6]; VCMI [26]; LDtk [11] | Add to object placement |
| Definitions vs instances | LDtk [10][13] | Use for layer and object data with ids |
| Terrain or pattern with random and scatter | Godot [8]; Tiled [3][4] | Owner's density and scatter decision |
| Inspector with search, sections, revert | Godot [9] | Owner's inspector decision |
| Action search (Ctrl+Shift+P) | Tiled [1] (single-source); Krita uses Ctrl+Enter [18] | Owner's choice matches Tiled |
| Test or play action from the editor | RPG Maker MZ [23] | Precedent for «Тест» |
| One undo per stroke | Tiled terrain Ctrl+Z [4] (general only) | **Unverified** as a rule in any reviewed source |
| Gap-free strokes | none reviewed | **Unverified**; design it, do not cite |

## Per-workspace recommendations for Razdor Studio

### Карта

- **Layout:** owner's model (left tool strip, options bar above the canvas, layers and palette on the right, minimap). Research adds: Tiled and Godot keep the palette and layers together in one bottom or side area, and Krita's status bar shows the active tool and size [18][8]. Put the tool hint and cell coordinates in a status bar, as the owner asked.
- **Tools and shortcuts to borrow:** B (brush), E (eraser), F (fill), R, M, L, W for the selection tools, S for select same (Tiled's letters [3]), and Shift for line. Use Shift+click for line and Ctrl+Shift for rectangle, as in Godot [8] and Tiled [3].
- **Objects:** pick from a palette with sprite thumbnails and drag or click to place with a transparent preview; right-click or Escape cancels [6][26]. Density and scatter as in Godot's scatter setting [8].
- **Avoid:** Krita's stabilizer smoothing for the brush [15]; Ctrl-to-subtract in selections (see the owner's Alt choice); a single-object-only inspector in the map (VCMI [26]).
- **Draws from:** Photoshop (layout, selections), Tiled (tool letters, layers, object placement), Godot (paint modifiers, scatter), VCMI (preview, 1/2/4 sizes).
- **Owner decisions covered:** layers, selections, brush keys, palette, theme, status bar. Research adds the tool-letter set and the preview rule. **Conflict check:** none found. The owner's Shift-click line and [ ] keys match the sources.

### Сюжет

- **Layout:** a node or list view for triggers and events on the left, the selected event's conditions and commands on the right (as in RPG Maker's event page and command list [23]). Keep the map visible in a side panel so a trigger zone can be picked on the map.
- **Tools and shortcuts:** use a Test command in the event menu as in RPG Maker MZ [23]. Shift-click selects a range of commands [23].
- **Objects:** conditions and triggers are picked from a list with search; a trigger zone is picked on the map (WC3's trigger and region model [28]).
- **Avoid:** a long flat command list with no grouping (RPG Maker's list gets long [23][24]); a trigger editor with no way to find a trigger by name (WC3's Object Manager is the cross-reference that works [28], details unverified).
- **Draws from:** RPG Maker MZ (pages, conditions, triggers, Test), WC3 (trigger model), LDtk (definitions versus instances for triggers [10]).
- **Owner decisions:** none on this workspace in #36. **Conflict check:** none.

### Данные

- **Layout:** RPG Maker's database: a type list at the side, the entry list in the middle, and the parameters on the right [25]. Godot's inspector supplies the field pattern: search, sections, labels, revert icon [9]. The owner's inspector decision covers this.
- **Tools and shortcuts:** F4 and F5 move between entries, as the RPG Maker database excerpt says [25]. Search filters fields, as Godot does [9].
- **Objects:** data types are the entries (units, buildings, texts). LDtk's typed field model with limits and defaults supports this [11]. File keys in the tooltip (owner's decision) need no reference.
- **Avoid:** flat grids of fields with no sections; the inspector must not show a field without its file key in the tooltip (owner's decision). Single-object-only edits (VCMI's limit [26]).
- **Draws from:** Godot (inspector), RPG Maker (database), LDtk (field types), VCMI (per-object properties, but with multi-select added).
- **Owner decisions covered:** inspector, markers, resets, drag-to-reorder lists. Research adds Godot's revert-icon and favourite pattern. **Conflict check:** drag-to-reorder is not in Godot's inspector page; it stays a Razdor-specific choice. Not a conflict with the owner's decision.

### Тест

- **Layout:** no reviewed editor has a dedicated test workspace. The closest precedent is RPG Maker's Test command in the event menu [23]. Put a Test action in the status bar or the command palette (Ctrl+Shift+P) rather than in its own panel.
- **Tools and shortcuts:** a single Play or Test key, reachable from the action search (Ctrl+Shift+P, per Tiled [1], single-source).
- **Objects:** test runs start from the placed start point or the selected event, as in RPG Maker's Test command [23]. Start positions are data with ids (LDtk's entity limit [11]).
- **Avoid:** a separate full-screen test window that hides the editor's status bar; no source supports it.
- **Draws from:** RPG Maker MZ (Test command). **Source gap:** no editor reviewed has a test workspace; the owner's decision (remember the panel layout) stands without a precedent.

## Open questions

- **Icon set (chosen: Lucide, for the owner to confirm).** Lucide: 2020 icons on the site [31], a 24 px base size with 2 px stroke, and a single style that suits a dark tool strip [31]. Licences: the code is ISC and the Feather-derived icons are MIT [30]. Phosphor: MIT [33], about 1240 icons (an inference from a code example [33]), several weights (Duotone is confirmed; others unverified [32]). Lucide is simpler: one weight, one stroke, and the license notice for the Feather-derived icons goes into the repo's third-party notices. This is a research recommendation; the owner's map decision allows either.
- **Tiled default panel layout and minimap:** not verified from primary pages. Check the live app or the Tiled UI pages before copying its layout.
- **Tiled shortcut page:** [7] returned 404. The shortcut letters come from the per-tool pages [3]; the full list is unverified.
- **Photoshop layout and shortcuts:** all helpx pages returned 403. Selection modifiers come from search excerpts [19]. Verify in the app before quoting in help text.
- **Krita canvas and interface details:** partly from search excerpts. The brush size keys are confirmed by one Krita page [17].
- **LDtk entity inspector and level view:** not in the fetched pages. Unverified.
- **RPG Maker MZ database and map layout:** database detail from a search excerpt [25]; map layout from the official page [22].
- **Original Heroes III editor:** no primary documentation found. VCMI's guide [26] is the only usable source, and VCMI is not the original.
- **Warcraft III editor:** no Blizzard documentation reached. The F-key list and the Object Manager's F8 binding are from the wiki [28] and need a check against the game.
- **One undo per stroke and gap-free strokes:** no reviewed source states these as rules. Design them as Razdor requirements, not as borrowed patterns.

## Sources

Accessed 2026-11-04 unless noted.

1. Tiled, manual introduction, https://doc.mapeditor.org/en/stable/manual/introduction/ (fetched; no panel layout detail)
2. Tiled, Layers, https://doc.mapeditor.org/en/stable/manual/layers/ (fetched)
3. Tiled, Editing Tile Layers, https://doc.mapeditor.org/en/stable/manual/editing-tile-layers/ (fetched)
4. Tiled, Terrain, https://doc.mapeditor.org/en/stable/manual/terrain/ (fetched)
5. Tiled, Custom Properties, https://doc.mapeditor.org/en/stable/manual/custom-properties/ (fetched)
6. Tiled, Objects, https://doc.mapeditor.org/en/stable/manual/objects/ (fetched)
7. Tiled, Shortcuts, https://doc.mapeditor.org/en/stable/manual/shortcuts/ (404; not used)
8. Godot 4, Using TileMaps, https://docs.godotengine.org/en/stable/tutorials/2d/using_tilemaps.html (fetched)
9. Godot 4, Inspector dock, https://docs.godotengine.org/en/stable/tutorials/editor/inspector_dock.html (fetched)
10. LDtk, Interface overview, https://ldtk.io/docs/general/editor-components/ (fetched)
11. LDtk, Entities, https://ldtk.io/docs/general/editor-components/entities/ (fetched)
12. LDtk, Layers, https://ldtk.io/docs/general/editor-components/layers/ (search result only)
13. LDtk, Auto layers, https://ldtk.io/docs/general/auto-layers/ (search excerpt)
14. LDtk, Rules, https://ldtk.io/docs/general/auto-layers/auto-layer-rules/ (search excerpt)
15. Krita Manual 5.3.0, Freehand Brush Tool, https://docs.krita.org/en/reference_manual/tools/freehand_brush.html (fetched)
16. Krita Manual 5.3.0, Layers docker, https://docs.krita.org/en/reference_manual/dockers/layers.html (fetched)
17. Krita Manual 5.3.0, Introduction from Photoshop, https://docs.krita.org/en/user_manual/introduction_from_other_software/introduction_from_photoshop.html (fetched)
18. Krita Manual 5.3.0, Basic Concepts (interface), https://docs.krita.org/en/user_manual/getting_started/basic_concepts.html (search excerpts; page not fetched). Settings menu for Dockers: https://docs.krita.org/en/reference_manual/main_menu/settings_menu.html (search result)
19. Adobe Photoshop help, Add to and subtract from selections, https://helpx.adobe.com/photoshop/how-to/add-to-subtract-from-selections.html (403 on fetch; search excerpt)
20. Adobe Photoshop help, Magic Wand tool, https://helpx.adobe.com/photoshop/using/tool-techniques/magic-wand-tool.html (403; search excerpt)
21. Adobe Photoshop, keyboard shortcuts PDF, https://helpx.adobe.com/content/dam/help/en/photoshop/using/default-keyboard-shortcuts/photoshop-keyboard-shortcuts.pdf (403; not read)
22. RPG Maker MZ help, Editing Map Designs, https://rpgmakerofficial.com/product/MZ_help-en/01_07_01.html (fetched)
23. RPG Maker MZ help, Map Event Settings, https://rpgmakerofficial.com/product/MZ_help-en/01_09_03.html (fetched)
24. RPG Maker MZ help, Flow Control commands, https://rpgmakerofficial.com/product/MZ_help-en/01_10_03.html (fetched)
25. RPG Maker MZ help, Database, https://rpgmakerofficial.com/product/MZ_help-en/01_08.html (search excerpt)
26. VCMI, Map Editor guide, https://vcmi.eu/modders/Map_Editor (fetched; community project, not the original H3 editor)
27. VCMI wiki (archival), https://wiki.vcmi.eu (search result)
28. Warcraft III wiki, Warcraft III World Editor, https://warcraft.wiki.gg/wiki/Warcraft_III_World_Editor (fetched; secondary)
29. Hive Workshop, Modifying the World Editor's Trigger Editor, https://www.hiveworkshop.com/threads/tutorial-modifying-the-world-editors-trigger-editor.46040/latest (search excerpt; community)
30. Lucide, license, https://lucide.dev/license (fetched)
31. Lucide, home (icon count, style), https://lucide.dev/ (fetched)
32. Phosphor Icons, https://phosphoricons.com/ (fetched; title only, licence not shown)
33. Phosphor Icons, core repository, https://github.com/phosphor-icons/core (fetched; MIT)
