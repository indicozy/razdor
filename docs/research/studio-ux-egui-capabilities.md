# Razdor Studio UX: what egui 0.31 and egui_dock 0.16 cover, and what needs custom code

Research note for the planned Studio UX (indicozy/razdor#39, a child of the map #36). Question:
for each decided UI item, which egui / egui_dock API or third-party crate covers it, what must be
custom code, a rough cost, and the pitfalls specific to egui running inside macroquad's window.

Scope: code reading of the `v2-packages` tree (branch `research/studio-ux-egui-capabilities`
from `origin/v2-packages`) and the local sources of the exact crate versions. Nothing was compiled
and the app was not run. Items marked UNVERIFIED are not confirmed by code or by a source.

## 1. Versions in use

From `Cargo.lock` (the resolved versions) and `Cargo.toml` [2][3]:

| Crate | Resolved | Declared in Cargo.toml | Note |
|---|---|---|---|
| egui | 0.31.1 | not direct; pulled by egui-macroquad and egui_dock | `serde` feature is NOT enabled (no serde in the lock's egui entry) [3] |
| egui_dock | 0.16.0 | `0.16`, optional (`studio`) | `serde` feature off in Razdor [2][4] |
| egui-macroquad | 0.17.3 | `0.17.3`, optional | its Cargo.toml asks macroquad `0.4.14`, lock has 0.4.16 [5] |
| egui-miniquad | 0.16.0 | not direct | the input and draw bridge [6] |
| macroquad | 0.4.16 | `0.4` | |
| miniquad | 0.4.11 | not direct | `RawId` for raw GL texture ids [7] |
| epaint | 0.31.1 | not direct | `Shape::dashed_line`, `ColorImage` [8] |

Razdor's studio is desktop-only and feature-gated (`Cargo.toml` `studio` feature, `src/ui/mod.rs:16-17`).

## 2. Findings table

Cost: S = under about a day, M = a few days, L = a week or more, with a prototype. Sources in
brackets refer to section 6.

| Item | egui / egui_dock API or crate (version) | Custom code needed | Cost | Pitfalls for our setup |
|---|---|---|---|---|
| 1. Workspaces, each with its own dock layout (save and restore per workspace) | `egui_dock::DockState<Tab>` per workspace [4]; `DockState` derives serde only with egui_dock's `serde` feature, which also turns on egui's `serde` [4]. Razdor builds the default with `layout()` (`src/ui/studio/mod.rs:1274`). | A workspace switcher (tab strip), one `DockState` per workspace, a serialised form of the layout (tab keys, split fractions), and a per-user settings location. Razdor's `Tab` is a Copy enum of unit variants (`mod.rs:90-91`), so it can derive serde. | M | The `.editor/layout.json` in the package holds only campaign-graph positions (`docs/flows/testing-and-tools.md` 266-267), not dock layouts; do not reuse that file for dock layouts. Dock layouts are personal settings, so they belong in the app's settings, not in the package [repo rule]. Tab ids must be stable, see pitfall 7. Enabling serde changes lock entries (egui gains serde). |
| 2. Photoshop-like Map workspace: tool strip (left), options bar (above canvas), layers and palette (right), minimap | Ordinary egui panels (`SidePanel`, `TopBottomPanel`) around the canvas; the Map tab already has a tool list and option widgets (`src/ui/studio/map_tab.rs:47-83` tool enum and list, `:907-1006` option rows, `:1017` a palette). Minimap is `Tab::Minimap` (`mod.rs:101-102`). | Icon buttons for the tool strip (see item 6 notes on icons), the options bar layout, and the rule that each tool shows its own options. | M | The canvas is macroquad, drawn after egui's `ui()` and before `egui_macroquad::draw()` (`mod.rs:605-613`): egui fills of panels are painted over the map, so the canvas area must have no fill. The tool buttons need icons: no egui-0.31 icon crate was confirmed for Lucide (pitfall 11). |
| 3. Layers panel with eye and lock per fixed layer, plus an overlay group | No egui layer widget. Use `ui.selectable_label` / icon buttons in a list (`Ui::horizontal`, `Ui::selectable_label`, both used already, e.g. `map_tab.rs:950`). | A layer model (ids, kind, visible, locked) in the studio model (`src/studio/`), the filter that the canvas and tools apply (paint, select, copy only visible unlocked layers), and the drawing order. | M | The canvas draws in macroquad (`src/ui/studio/canvas.rs:274` `draw_map`); a hidden layer must be skipped in macroquad draw code, not only in the list. Eye and lock state is studio state: it must be on the undo history or stay out of it (decide). |
| 4. Brush footprint cursor drawn over the canvas (size, square or round, footprint preview) | No egui API needed: the brush outline already exists and is drawn with macroquad in the map draw (`map_tab.rs:1344-1352`, footprints toggle `:1284-1286`). The cursor cell comes from the egui response (`map_tab.rs:391`). | Round brush shape (macroquad `draw_circle_lines` or a cell test), `[` `]` keys (see pitfall 8), and the ghost of the footprint. | S | The cursor cell and the drawn outline come from the same frame's egui response, so they agree (pitfall 3). The OS pointer is not changed by the studio (pitfall 1), so the brush outline is the only cursor feedback. |
| 5. Selections: marquee (M), lasso (L), magic wand (W); Shift add, Alt subtract; marching ants | No egui selection widget. Marching ants: `epaint::Shape::dashed_line` and `dashed_line_with_offset` (animated offset) [8]; egui `Painter` draws over the map because egui is drawn last [6]. Lasso and wand: custom polygon and flood fill. | The selection mask in `src/studio/` (cell set), the lasso path, flood fill for the wand, the Shift/Alt modifier rules, the ants animation (an offset per frame), and clipping of painting and fill to the mask. | L | Ants need a repaint each frame to move. The main loop already redraws every frame (`src/main.rs`, `next_frame().await` with no idle sleep; see pitfall 4), so no extra scheduling is needed; the cost is per-frame drawing (UNVERIFIED: not measured). Draw ants with the egui painter (above the map) or with macroquad (below egui); pick one and keep it in one layer. |
| 6. Sprite-thumbnail palette (thumbnails from the player's install, made at runtime) | Option A (portable): `egui::ColorImage::from_rgba_unmultiplied` plus `Context::load_texture` [8], then `egui::Image` or `Painter::image`. Option B (GL only): `egui::TextureId::User(u64)` mapped to a raw GL id by egui-miniquad (`painter.rs:308`) [6], with miniquad's `RawId::OpenGl` [7]. | Option A: a thumbnail cache (lazy, LRU or bounded) that builds `ColorImage` from the sprite RGBA; the palette grid. Option B: a helper that gets the GL name of a macroquad `Texture2D`. | M (A) / S (B, GL only, UNVERIFIED) | Sprites are decoded to RGBA on the CPU and uploaded with `Texture2D::from_rgba8` (`src/ui/dt_art.rs:105`); the RGBA is available at load, so option A needs no GPU readback (whether the RGBA is kept after upload was not checked). Dropping a `TextureHandle` frees the egui texture (`painter.rs:181` `free_texture`). Thumbnails must never be committed (repo rule); they are made at runtime. |
| 7. Godot-style inspector: header (icon, name, type), field search, collapsible sections, label column, markers and resets | `egui::CollapsingHeader` [3] (already used: `src/ui/studio/scene.rs:37-61`), `egui::Grid` (used in `records.rs`), `TextEdit`. Field forms exist: `src/ui/studio/forms.rs`, `story_forms.rs`, `inspector.rs`. | Field search filter over the form rows, the reset and changed markers (compare with the base value), the header with the icon, and per-section open state kept outside egui memory if it must persist. | M | `CollapsingHeader::new(text)` takes its id from the label text (`collapsing_header.rs:401`). `scene.rs:38-61` puts the counts in the label ("Buildings (3)"), so the section's open state resets whenever a count changes. Use `id_salt` with a fixed key (pitfall 6). egui memory is not persisted (pitfall 6). |
| 8. Drag-reorder lists | Built in: `Ui::dnd_drag_source` and `Ui::dnd_drop_zone` (`egui-0.31.1/src/ui.rs:2772`, `:2824`), `Context::is_being_dragged` [3]. Third party: `egui_dnd` 0.12.0 requires `egui ^0.31.0` [9]. | The list widget, the drop indicator, the write of the new order as one undo step. Razdor has Up and Down buttons today (`story_forms.rs:53-56`). | S | Payloads must be `Any + Send + Sync` [3]. The drag state lives in egui memory, which is not persisted (fine for drags). Keep the drop target inside the same dock tab; a drag across tabs needs a shared id (UNVERIFIED). |
| 9. Command palette (Ctrl+Shift+P actions, Ctrl+P jump to object) | No egui-0.31 crate found. `egui-palette` 0.6.0 needs `egui ^0.35` and 0.1.0 needs `^0.34.1` [9]; the 0.2.0 requirement was not checked. Fuzzy matching: `nucleo-matcher` (a dependency of egui-palette 0.6.0) [9]; its version for us was not checked. | A small modal widget: `egui::Window` or `Area` with a text field, a fuzzy-ranked list (`nucleo-matcher` or a simple subsequence match), arrow-key navigation, Enter to run. A command registry shared with item 11. | M | Opening on Ctrl+P while a text field has focus must be gated by `wants_keyboard_input` (pitfall 8). The palette's text field gets `Event::Text`; egui-miniquad drops printable chars when Ctrl is held (`egui-miniquad` `char_event`, lib.rs), so the palette input must not depend on Ctrl. |
| 10. Tooltips with name and shortcut | `Response::on_hover_text` (`response.rs:805`), `on_hover_ui` (`:552`), `on_hover_text_at_pointer` (`:788`), `Context::format_shortcut` (`context.rs:1477`) [3]. | A function that builds "Name (Ctrl+S)" from the binding table; a setting to show file keys inline (item in the map). | S | Razdor's tooltips hard-code shortcut text today (e.g. `map_tab.rs:976` "Ctrl+C copies the rectangle", `mod.rs:171` "Delete key"). With rebinding (item 11) these go stale; move all shortcut text to `format_shortcut`. Tooltips are shown by egui, so they are drawn over the macroquad canvas (correct z-order). |
| 11. Rebindable shortcuts (serialise bindings, match, conflicts) | `egui::KeyboardShortcut` (`Copy, Hash, Eq`, serde derive only under egui's `serde` feature) [3]; `InputState::consume_shortcut` (`input_state/mod.rs:574`) and `Modifiers::matches_logically` [3]. | The binding table (action to shortcut), its serialisation (needs egui `serde`, pitfall 12), the editor widget (capture the next key), conflict detection, and a single dispatch point. Existing hard-coded checks: `mod.rs:1058-1091`. | M | Matching uses egui's key events; egui-miniquad maps only a fixed set of keys (`egui-miniquad` `input.rs:42-110`); a key outside that set never reaches egui and cannot be bound. Command+C, Command+V and Command+X are not Key events (pitfall 9), so a binding for them does not fire today. No egui-0.31 rebinding crate was confirmed: `egui-keybind` 0.8.0 needs `^0.33` [9]; `egui-keybinds` search result 3.3.8 needs `^0.21` [9] (version list inconsistent, not usable); older versions not checked. |
| 12. Theme from tokens (colours, font, spacing, sizes), dark now, light later | `egui::Visuals` (already built in `theme()`, `mod.rs:1300-1325`, set every frame at `mod.rs:924`), `Context::style_mut` and `Context::set_style` (`context.rs:1869`, `:1880`) [3], `Context::set_fonts` (`context.rs:1776`) [3]. | A token struct (colours, rounding, spacing, text sizes) that maps to `Visuals`, `Style.spacing`, `Style.text_styles`; a light token set; font definitions only if a custom font is wanted. | S (tokens to Visuals) / M (custom font) | No custom font is set anywhere (grep: no `set_fonts`, no `FontDefinitions`), so egui's default fonts are used; Cyrillic coverage of those fonts was not checked. `theme()` runs each frame, so a token change applies at once. Text scaling (accessibility) is a separate choice: egui has `set_zoom_factor` (`context.rs:2017`) but the studio never calls it; zoom would also scale the canvas through `pixels_per_point` (pitfall 5). |

Extra items the map lists elsewhere, short answers:

| Item | API or crate | Custom code | Cost | Pitfalls |
|---|---|---|---|---|
| Status bar (cell, terrain, tool hint, save state) | `TopBottomPanel::bottom` | text only | S | none specific |
| Start screen | already exists (`src/ui/studio/start.rs`) | content | S | none specific |
| Icons (one open set, Lucide or Phosphor) | `egui-phosphor` 0.9.0 requires `egui ^0.31` [9] (a font: needs the font registered with `set_fonts`, API not checked); `egui-lucide` 0.1.0 requires `^0.36`, 0.2.0 `^0.37` [9] (no 0.31 release found) | font or SVG loading code | M | Lucide has no egui-0.31 crate that was found; Phosphor has one. A font-based icon set needs `set_fonts` (pitfall 12). |

## 3. Per-item notes

- Item 1 (workspaces). `DockState` has one tree per instance; a workspace is one `DockState`, and
  the screen keeps a `Vec` or map of them and draws the active one with `DockArea::new` (currently
  `mod.rs:1015`). Serde would be `egui_dock` with the `serde` feature plus egui's `serde` feature
  [4][3]. Tabs that appear in two workspaces are fine because each `DockState` is separate, but the
  canvas, the selection and the undo history are shared and sit in `StudioScreen`, so the workspace
  switch must not reset them (`mod.rs` fields `canvas`, `selection`, `ws`).
- Item 2 (Map workspace). The Map tab already has the tool palette and options (`map_tab.rs`). The
  change is layout (a left strip and a top bar) and icons, not new input code. Any layout change is a
  UI change under the owner's approval rule (repo `CLAUDE.md`), so this is a decision point, not only
  a cost.
- Item 5 (selections). Ants and the lasso are drawn above the map (egui painter) or below egui
  (macroquad); the selection itself is a model change. A selection that covers a hidden or locked
  layer must be refused for painting and fill (repo spec: "acts on visible, unlocked layers").
- Item 6 (thumbnails). The data path is `DtArt::object` and the sprite RGBA (`dt_art.rs:105`); the
  studio palette would call it on demand. Option B avoids a second copy but depends on the GL
  backend: egui-miniquad passes `TextureId::User` as a raw OpenGl id (`painter.rs:308`), so on a
  non-GL backend it would be wrong. Option A is the portable choice; it is the one to prototype.
- Item 9 (command palette). A palette needs an egui modal. egui's `Window` or `Area` is enough; the
  command list is the same registry as the shortcuts (item 11), so build the registry once.
- Item 11 (shortcuts). Shortcuts are checked after `take_input` and before the docks
  (`mod.rs:928-929`), so the terminal and the shortcuts share one order. egui's `KeyboardShortcut`
  matching ignores extra Shift and Alt (`matches_logically`, `data/input.rs:784`, `input_state/mod.rs:574`
  docs), so the most specific binding must be checked first.

## 4. Our-setup pitfalls (verdicts from the code)

1. Cursor icon ownership. Verdict: applies; the logic is clear from code, the result is UNVERIFIED at runtime.
   - egui-miniquad applies egui's `cursor_icon` every frame: `show_mouse(true)` and `set_mouse_cursor`
     (egui-miniquad `lib.rs:225-232`) [6].
   - Razdor's own pointer code runs after the frame: `ui::cursor::draw()` is called in the main loop
     after `app.frame()` (`src/main.rs:263`), and `cursor::draw` (`src/ui/cursor.rs:163`) calls
     `show_mouse(false)` and draws the game's pointer when the install has its art, else sets the
     system icon (`cursor.rs` `show_system`).
   - `cursor::begin_frame()` runs only in `frame_content` (`src/ui/mod.rs:1384`, function at `:1343`).
     `App::frame` returns early for the studio (`src/ui/mod.rs:1312-1315`), so the studio never resets
     the cursor state. The studio never calls `cursor::set`, `cursor::system`, or `set_cursor_icon`
     (grep of `src/ui/studio/`). So the shape `WANT` and the system icon `SYSTEM` are whatever the last
     non-studio screen left them as.
   - Consequence (code reading): with the install present, the game's arrow may be drawn over the
     studio and the system pointer hidden, and egui's pointer icons (text, resize, pointing) are
     overridden the same frame. egui_dock sets no cursor icon at all (grep of egui_dock 0.16.0: no
     `set_cursor_icon`), so splitter resize cursors never show. Needs a runtime check.
2. Input capture. Verdict: applies, handled by egui layering.
   - The canvas is `allocate_exact_size(..., Sense::click_and_drag())` (`map_tab.rs:347`, and the
     other canvas at `:1058`). Its hover, press and drag come from the egui `Response`, so windows,
     menus and popups above it win without extra code.
   - `wants_pointer_input` is not used in the studio (grep: no hits). `wants_keyboard_input` gates the
     copy, paste, Escape and Delete handling (`mod.rs:1072`, `:1084`).
   - The Claude terminal takes the keys first when it has focus (`mod.rs:928`, `claude.rs:90-113`),
     then the shortcuts run (`mod.rs:929`).
3. Event ordering and latency. Verdict: no extra frame of lag between egui and the canvas.
   - `egui_macroquad::ui` first replays the miniquad events into egui (`egui-macroquad` `lib.rs`,
     `Egui::ui`, `repeat_all_miniquad_input`) [5], then runs the egui closure.
   - The map is drawn after that closure and before `egui_macroquad::draw()` (`mod.rs:605-613`).
     The canvas cell (`map_tab.rs:391`) and the footprint outline use the same frame's response.
   - Scope: this covers egui and the canvas. Macroquad's own input (`mouse_position`, `is_key_pressed`)
     is not used by the studio (grep of `src/ui/studio/`: no `mq::mouse` or `is_key_pressed`), so
     there is no second source of pointer state to disagree.
   - Note: egui may run the UI closure twice in one frame (the relayout pass, comment `mod.rs:597-599`).
     Shortcuts consume their events in `show`, so the second pass sees them consumed; this is fine as
     long as new code consumes events the same way.
4. Repaint scheduling. Verdict: applies in the easy direction.
   - The main loop calls `next_frame().await` every iteration with no idle sleep or frame limiter
     (`src/main.rs`, loop from about line 240 to 290; grep for `sleep`, `fps`, `idle`: no hits in
     `main.rs`). So the window redraws continuously and marching ants need no repaint request.
   - The only `request_repaint` in the studio is `newmap.rs:165`; it is not needed for the main loop.
   - The redraw rate (vsync, macroquad default) was not checked; per-frame cost of the ants was not
     measured.
5. DPI and pixels_per_point. Verdict: consistent today.
   - egui-miniquad tracks the native DPI scale and sets `native_pixels_per_point`
     (egui-miniquad `lib.rs:175-183`) [6].
   - The canvas converts egui points to pixels with `ctx.pixels_per_point()` (`map_tab.rs:348-349`,
     `:370`). The studio never calls `set_pixels_per_point` or `set_zoom_factor` (grep). Any future
     egui zoom (for text scaling) would change `pixels_per_point` and must update the canvas
     conversion together; Razdor's own interface scale (Ctrl+0) is a separate system and does not touch egui.
   - High-DPI behaviour on a real display: UNVERIFIED.
6. DockState serde and tab identity. Verdict: applies to item 1.
   - egui_dock's serde derives are behind its `serde` feature (`dock_state/mod.rs:29`, etc.) [4]; Razdor
     does not enable it (`Cargo.toml:39`).
   - `Tab` is a Copy enum of unit variants (`mod.rs:90-91`), so the derive is cheap.
   - Tab ids: `TabViewer::id` defaults to the title text (`egui_dock` `tab_viewer.rs:31-34`) [4]. The
     studio's `title` is `tr(...)` (`mod.rs:126-132`, `:1390-1391`) and does not override `id`
     (grep: no `fn id(` in `mod.rs`). So egui's tab state changes with the language. Override `id`
     with a fixed key per tab.
   - CollapsingHeader ids come from the label text (`collapsing_header.rs:401`) [3]; see item 7.
   - egui memory is not persisted: the egui_macroquad integration does not call persistence (`egui-macroquad`
     `lib.rs`, no persistence call) [5]. Any state that must survive a restart (layouts, open sections)
     needs our own store.
7. Keyboard focus with text fields and shortcuts. Verdict: applies.
   - Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y and Ctrl+S are consumed before the docks (`mod.rs:1060-1066`,
     `:1064`) and are not gated by `wants_keyboard_input`. Ctrl+Z inside a text field is taken by the
     studio, not by the field; the code comment says this is intentional (`mod.rs:1055-1057`).
   - The plain-key tool shortcuts that the map lists (M, L, W, `[`, `]`, Shift, Alt) must be gated by
     `wants_keyboard_input` or they will fire while typing. No plain-key shortcuts exist today.
   - Escape and Delete are gated (`mod.rs:1079`, `:1084`).
8. Copy and paste shortcuts do not fire. Verdict: applies, a defect by code reading; UNVERIFIED at runtime.
   - egui-miniquad's `key_down_event` turns Command+X, Command+C and Command+V into `Event::Cut`,
     `Event::Copy` and `Event::Text(clipboard)`, and does not push a `Event::Key` for them
     (egui-miniquad `lib.rs:317-332`) [6].
   - `consume_shortcut` only matches `Event::Key` (`num_presses` / `consume_key`,
     `input_state/mod.rs:574` and `:~600`) [3]. So the studio's Command+C and Command+V checks
     (`mod.rs:1073-1080`) never see a press.
   - The menu buttons still work (`map_tab.rs:976-980`), but their tooltips promise Ctrl+C and Ctrl+V
     (`map_tab.rs:976`, `:979`), and `arm_paste` by Ctrl+V is therefore not reachable from the key.
   - The Claude terminal depends on `Event::Copy` (`claude.rs:112`), which shows the event is delivered;
     only the `KeyboardShortcut` route is dead.
   - Fix direction (not implemented): handle `Event::Copy` and `Event::Paste`/`Event::Text` in
     `shortcuts`, or bind on the key event path. Needs a prototype or a test with a synthetic event.
9. Keyboard mapping limits. Verdict: applies to item 11.
   - Only the keys listed in `egui_key_from_mq_key` reach egui (`egui-miniquad` `input.rs:42-110`) [6].
     `[` and `]` are mapped (`OpenBracket`, `CloseBracket`, `input.rs:102-103`), so the brush keys work.
     Letters M, L, W, P, S, Y, Z are mapped (`input.rs:84-98`).
10. Modifiers. Verdict: no issue on Linux and Windows.
    - `command` is Ctrl on non-macOS (`egui-miniquad` `input.rs:34-38`). Mac behaviour not checked.
11. Icons. Verdict: decision needed. egui-phosphor 0.9.0 matches egui 0.31 [9]; Lucide crates found
    target egui 0.36 and 0.37 [9]. Using Phosphor needs its font registered; the exact API was not
    checked (UNVERIFIED).
12. Native-only features. `egui-miniquad` does not implement the IME, the screen reader or the
    `OpenUrl` / `CopyImage` commands fully: `CopyImage` is a no-op and IME is ignored (`lib.rs`, the
    `platform_output` match, `:205-224`). Not an issue for the planned UI unless the palette needs IME.
13. Marching ants and the egui painter. Verdict: works. `Shape::dashed_line_with_offset` (epaint
    `shape.rs:188`) [8] gives the animated offset. Clip rectangles must be set for the canvas area,
    or the ants draw over the panels around the canvas (egui `Painter` clips to its own clip rect).

## 5. Open questions and what needs a prototype

- Runtime cursor behaviour (pitfall 1). Needs an off-screen check: `offscreen-snapshot` cannot show
  the OS pointer, so a live run on the desktop is needed (owner's permission rule applies).
- The Ctrl+C and Ctrl+V dead shortcut (pitfall 8). Needs a test with a synthetic `egui::Event::Copy`
  and a check of how `consume_shortcut` behaves, then a real run.
- Whether egui-miniquad's paste path (`Event::Text`) reaches a focused text field and the canvas
  paste (`arm_paste`).
- Thumbnail path: option A (`ColorImage`) memory and time for a palette of a few hundred sprites;
  option B (GL id) only if the GL backend is guaranteed (it is on Linux and Windows here; mac uses
  Metal in miniquad 0.4 [7], so option B is GL-only; check the miniquad `RawId::Metal` case).
- Whether `DtArt` keeps the RGBA after upload (`dt_art.rs:105`), which decides whether option A
  duplicates memory.
- Phosphor's font registration API for egui 0.31 (`egui-phosphor` 0.9.0 source not read).
- A command palette: no egui-0.31 crate found. 0.2.0 of `egui-palette` was not checked. Candidate
  `nucleo-matcher` version compatible with the rest of the tree was not checked.
- Rebinding: no egui-0.31 crate found. The `egui-keybinds` versions list returned only 0.1.x while the
  search said 3.3.8 (requires `^0.21`); this inconsistency was not resolved.
- Light theme token set and Visuals::light (not re-grepped in egui 0.31.1 source; expected to exist).
- Drag across dock tabs (item 8): UNVERIFIED.
- Per-frame cost of marching ants and the frame rate of the main loop (vsync not checked).
- DockState persistence location and format (a new settings file): a design decision, not a code question.
- Cyrillic coverage of egui's default fonts (pitfall 12): not checked.
- docs.rs pages were not fetched; API claims rest on the local source of the same crate versions.

## 6. Recommended order (from the evidence above)

1. Fix the copy and paste shortcut path (pitfall 8) and the cursor ownership (pitfall 1) first; both
   affect every later item.
2. Stable tab ids and a fixed key for CollapsingHeader sections (pitfalls 6 and item 7); cheap and
   needed before any layout is saved.
3. A shared command and shortcut registry (items 9, 10, 11), then the palette on top of it.
4. Token-based theme (item 12) and the icon decision (item 11 of the notes), since they touch every screen.
5. Workspaces with saved dock layouts (item 1), then the layers model (item 3), then selections (item 5).

Note: items 2 and the Map workspace layout change any screen's look and need the owner's approval
before a push (repo `CLAUDE.md`, UI rules).

## 7. Sources

1. indicozy/razdor issue #39, "What egui can do for the planned UI" (`gh issue view 39 -R indicozy/razdor`, read 2026-11-04).
2. indicozy/razdor issue #36, "Razdor Studio UX: the spec for a human-friendly editor" (`gh issue view 36 -R indicozy/razdor`, read 2026-11-04).
3. egui 0.31.1 source, local: `/home/indicozy/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.31.1/` (files cited: `src/data/input.rs`, `src/input_state/mod.rs`, `src/context.rs`, `src/ui.rs`, `src/response.rs`, `src/containers/collapsing_header.rs`, `src/drag_and_drop.rs`).
4. egui_dock 0.16.0 source, local: `/home/indicozy/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui_dock-0.16.0/` (`Cargo.toml` serde feature; `src/dock_state/mod.rs`; `src/widgets/tab_viewer.rs`).
5. egui-macroquad 0.17.3 source, local: `/home/indicozy/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-macroquad-0.17.3/src/lib.rs`.
6. egui-miniquad 0.16.0 source, local: `/home/indicozy/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-miniquad-0.16.0/src/{lib.rs,input.rs,painter.rs}`.
7. macroquad 0.4.16 and miniquad 0.4.11 source, local: `.../macroquad-0.4.16/src/texture.rs`; `.../miniquad-0.4.11/src/graphics.rs` (`RawId`, `TextureId::from_raw_id`).
8. epaint 0.31.1 source, local: `.../epaint-0.31.1/src/shapes/shape.rs` (`dashed_line`, `dashed_line_with_offset`), `.../epaint-0.31.1/src/image.rs` (`ColorImage::from_rgba_unmultiplied`).
9. crates.io API, fetched 2026-11-04: `https://crates.io/api/v1/crates/<name>/<version>/dependencies` and `/versions`, and `https://crates.io/api/v1/crates?q=<term>` search. Results used: egui-palette 0.6.0 (`egui ^0.35`), 0.1.0 (`^0.34.1`); egui_dnd 0.18.0 (`^0.37`), 0.12.0 (`^0.31.0`); egui-keybind 0.8.0 (`^0.33`); egui-keybinds 3.3.8 (`^0.21`, from search only); egui-phosphor 0.9.0 (`^0.31`), 0.14.0 (`^0.36`); egui-lucide 0.2.0 (`^0.37`), 0.1.0 (`^0.36`). Search for "egui command palette" also lists egui-pop 0.3.0 (not an official egui crate).
10. Razdor repo, this branch: `Cargo.toml` (lines 16, 18-19, 38-39), `Cargo.lock` (egui, egui_dock, egui-macroquad, egui-miniquad, macroquad, miniquad entries).
11. Razdor source: `src/ui/studio/mod.rs` (lines 90-132 Tab; 597-613 frame and egui calls; 923-931 show; 1015 DockArea; 1058-1091 shortcuts; 1274-1297 layout; 1300-1325 theme; 1390-1391 title), `src/ui/studio/map_tab.rs` (47-83 tools; 347-349, 370, 391 canvas; 907-1006 options; 976-980 copy and paste; 1058; 1284-1286 footprints; 1344-1352 brush outline), `src/ui/studio/canvas.rs` (108-125 terrain texture; 274 `draw_map`), `src/ui/studio/claude.rs` (90-113 terminal input; 112 Copy), `src/ui/studio/scene.rs` (37-61 CollapsingHeader), `src/ui/studio/story_forms.rs` (53-56 Up and Down), `src/ui/studio/newmap.rs` (165 request_repaint), `src/ui/dt_art.rs` (105 `Texture2D::from_rgba8`), `src/ui/cursor.rs` (cursor state and `draw` at 163), `src/ui/mod.rs` (16-17 studio gate; 1302-1316 `App::frame` and studio dispatch; 1343-1384 `frame_content` and `cursor::begin_frame`), `src/main.rs` (240-290 loop; 263 `cursor::draw`).
12. Razdor docs: `docs/flows/README.md`, `docs/flows/testing-and-tools.md` (lines 198-320, Razdor Studio; 233-236 and 266-267 `.editor/layout.json`), `docs/superpowers/specs/2026-11-03-razdor-studio-design.md` (referenced by issue #36; not opened in this pass).
