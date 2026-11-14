# Flow: the install's files, decoding and drawing

**Read this first.** Razdor ships no original content. Every picture, sound, text and map is
read at runtime from the player's Discord Times install (`RAZDOR_DT_DIR` or the located
folder) and decoded in memory; nothing decoded is ever written to the repo. The pure
decoders live in `src/dt/` (no macroquad, fully unit-tested); `src/ui/` turns their RGBA
images into textures and caches them. Formats are specified in
`docs/reference/graphics-formats.md` and `docs/reference/dtm-format.md`.

## From install file to pixels

```
install file ──► src/dt/gfx.rs decode_file ──► gfx::Image (RGBA, not premultiplied)
                    │ (.lit / .ugs / .spi / .bmp)
                    ▼
     src/ui/chrome.rs art / win / win_fx  (window art, cached per path × Fx)
     src/ui/dt_art.rs DtArt               (portraits, figures, items, terrain, map objects)
                    ▼
     macroquad Texture2D ──► chrome::tex / chrome::tex_src / draw_texture_ex
```

- `src/dt/install.rs`: `DtInstall::load` reads the data inis (units, items, spells), lists
  the maps (`list_maps`, `MapEntry`) and the play options (`PlayOptions`). `find_path`
  matches file names case-insensitively (the install is a Windows folder).
- `src/dt/ini.rs` (`Ini::from_cp1251`), `src/dt/text.rs` (cp1251), `src/dt/markup.rs`
  (the original's text marks), `src/dt/dtm.rs` (maps, `Scenario`), `src/dt/container.rs`
  (the `AIpf` container), `src/dt/data.rs` (units/items/spells/battle effects).

## Image formats (`src/dt/gfx.rs`)

- **LIT** (`decode_lit`): a DCT image — header, then Y, Cb, Cr[, A] planes, each with a
  quantisation table and a coefficient rank table (`lit_plane`). Two traps found by players:
  - every coefficient byte is **signed**, the DC slot too (the original's `CBW` at
    0x4778b4). Reading slot 0 unsigned drew grey stripes on the mana icon, the battle's
    sword cells (`wb1.lit`), the selection ring and 11 more pictures (commit 84a84fd;
    test `lit_dct_slot_zero_is_signed`);
  - the colour path is the original's byte pipeline: planes rounded to bytes
    (`plane_bytes`), chroma block edges smoothed (`deblock`), 4:2:0 chroma doubled with a
    1-2-1 filter (`upsample`) — commit 325ecd1, test `lit_chroma_smoothing`. Grey pictures
    must come out exactly grey.
  `real_files_match_reference_decoder` pins checksums of real files (needs the install).
- **UGS** (`decode_ugs`): sprite sequences of 16-bit pixels (`ugs_pixel`); `Objects.ugs`
  has its own record layout (`decode_objects_ugs`, `ObjectSprite`, `ObjectSprites`).
- **SPI** (`decode_spi`, editor icons) and **BMP** (`decode_bmp`, water frames; some
  `.lit` files are really BMPs, and a mod may save its pictures so). 8-bit BMPs are read
  through their palette as the original's loader reads them (0x47754c), 24 and 32 bits as
  they are.
- `gfx::quantize_565` rounds a picture to the original's 16-bit surfaces;
  `gfx::key_bottom_left_565` keys a picture loaded without alpha by its bottom-left pixel's
  16-bit colour, as the original draws the ability icons (0x48e8f0).
- Install locations as constants: `UNIT_PORTRAITS`, `UNIT_FIGURES`, `ITEM_ICONS`,
  `MAP_OBJECTS`, `TERRAIN_TEXTURES`, `WATER_FRAMES` / `water_frame_file`.

## Caching and decoding ahead

- `chrome::art(rel, fx)` decodes a file once and caches the texture per path and per
  `chrome::Fx` (Plain, KeyBlack, Glow, Grey, Shade, SwapGreenBlue, ShadeStrong,
  KeyCorner for the ability icons, HeroPicked / HeroOffered / HeroLocked for the new-hero
  window's portraits, `chrome::hero_portrait`, 0x4d7d30). `chrome::win(name)` = `Graphics/Windows/<name>.lit` drawn plainly with its own
  alpha. Prefer `Fx::Plain`: a black key was once used to hide the decoder's sign bug and
  ate dark pixels (commit 66d29ff).
- A map event's own picture is unscrambled ARGB4444 (`dtm::event_picture_rgba`, 0x4d1693;
  `ui::story` draws it); it was once read as RGB565 and showed as orange noise.
- `DtArt` (`src/ui/dt_art.rs`) caches by `Key` (portrait, figure, item, terrain, object,
  building) and builds one `Atlas` for every map object and building sprite so the map
  draws in few batches.
- **The decode store** (`src/dt/preload.rs`, `decode_shared`): every install picture is
  decoded once and shared by `Arc` by every caller (`gfx::decode_file` is the owned form).
  Two background workers ("art ahead") decode the queued paths in order (`preload::preload`).
  A caller that asks for a picture a worker is decoding waits for it; one still queued is
  taken off the queue and decoded there. Slow main-thread waits and decodes are logged
  ("slow load") with `RAZDOR_PROFILE=1`. Snapshot runs (`RAZDOR_SNAPSHOT`) queue nothing and
  decode on demand, so their frames do not depend on timing.
- **What is queued, at start** (`ui::assets::announce_install`, once per install): first
  `chrome::set_install` queues `WINDOWS_AHEAD` (the menus and the title, the window frames and
  buttons, fonts, cursors, the battle's art and sheets, the paper, portrait and icon sheets,
  the load window's book, building tabs). Then `DtArt::preload_all` queues the rest of the
  install, most likely first: the terrain textures, the water frames and every
  `Graphics/Units/*.ugs` figure sheet; the map objects, the minimap's icons and frame; the unit
  portrait and item icon sheets; then every other `.lit`/`.ugs`/`.spi`/`.bmp` under `Graphics/`
  in path order. The store keeps at most `picture_cache::CAP_BYTES` of decoded RGBA (1 GiB on
  the desktop, 256 MiB on the phones): a queued picture past it is not kept and the rest of the
  queue is dropped (those decode on demand, as before). A snapshot run queues nothing.
  The queue's end is logged (`art ahead: queue done, N pictures, X MiB decoded, Y s`).
  Measured on the desktop (the install's 456 pictures, 176 MiB decoded, cold cache,
  `RAZDOR_NO_PICTURE_CACHE=1`): the queue is done in about 0.4 s with the two workers.
- **Textures are made on the main thread** (macroquad/OpenGL): `chrome::warm(4.0)` runs each
  frame and uploads the lit pictures of `WINDOWS_AHEAD` (`WARM`), then the install's other
  `Graphics/Windows/*.lit` (`other_lit_windows`), that the store has ready, through
  `chrome::art(rel, Fx::Plain)`; `DtArt::warm` then uploads the terrain textures and the unit
  portrait and item icon textures queued by `preload_all`, both within one 4 ms budget per frame
  (`preload::upload_within_budget`; one upload per frame at least). Other `Fx` variants and the
  map's atlas and figure sheets are made at first use, from the decoded picture (no decode
  then). The minimap's ground (terrain, objects, bridges) is made once per map when the game starts
  (`minimap::prebuild`, keyed by the map's identity, not the fog); the fog's dark is drawn over it
  as `minimap::fog_texture`, the same texture the map view draws; it holds the shown darkness (see «The fog
  fades» below), so a step of the hero only changes the dark and the texture is uploaded when a texel changed.
  Snapshot runs make no early textures.
- **The persistent picture cache** (`src/dt/picture_cache.rs`): a decoded picture's frames
  (raw RGBA with their sizes) are kept in `platform::cache_dir()` + `pictures` + one subfolder
  per release and format, `pictures/<version>-f<format>` (e.g. `pictures/1.1.3-f1`): desktop
  `<data>/razdor/cache/pictures/…` (`%APPDATA%\razdor` on Windows, `~/.local/share/razdor` on Linux);
  Android the app's private cache (`Context.getCacheDir()`, not the shared storage, so the Files
  app's Recent tab does not list them); iOS `Library/Caches/Razdor`. One `.rzpc` file per
  install file. At start `picture_cache::prune` (background thread) removes the other releases'
  subfolders, the old flat `.rzpc` files of the pre-subfolder layout and stale `.tmp` files
  (not this process's). `platform::tidy_app_dir` removes the old `<app_dir>/cache/pictures`.
  The key is a hash of the file's path, size and modification time, the cache format and
  Razdor's version: a changed file or a new release misses. A miss is decoded and
  written by a background thread (temp file, then rename). Writes reserve their bytes from a
  process-wide budget (`Budget`, counted from the folder on first use) before writing, so
  parallel writers cannot overshoot the cap; a failed write gives them back. A truncated,
  corrupt or stale entry is ignored and rewritten. The folder is capped (1 GiB on the desktop,
  256 MiB on the phones: past it nothing new is written); clearing it resets everything. Off in
  snapshot runs and with `RAZDOR_NO_PICTURE_CACHE=1`.
  Only data derived from the player's own install is in it: never write it into the install
  folder or the repo.
- **What leaves the decode store (phones only):** on the desktop the store keeps every picture
  (as above). On the phones (`platform::MOBILE`) the GPU shares the phone's RAM, so a picture
  whose pixels are already in a texture would be held twice. There a picture leaves the store
  once nothing holds it and its cache entry exists (`preload::kept`, `preload::release`,
  `preload::mark_cached`). Its holds are the pending builds: each entry of the warm queues
  (`chrome::WARM`, `DtArt::warm`) holds its picture from the moment it is queued until its
  texture is made (`preload::hold` before the decodes start, `preload::release` after the
  upload). Every other consumer (`gfx::decode_file` and its owned copies, `chrome::art` and each
  `Fx`, the sheets, the terrain and figure caches, the map atlas, the building mask, the sharp
  sessions) takes a copy or builds from a picture that is in the store only while its write to
  the cache is pending, so nothing else needs a hold. The cache entry is marked when the
  background write finishes (`mark_cached`, only if the file exists: a full or failed cache keeps
  the picture). A later need of a dropped picture is a re-read of its `.rzpc` (no decode), logged
  as `slow load: re-read … (dropped from the store)` with `RAZDOR_PROFILE=1`. `forget_ready`
  clears the holds with the rest. The test switch `RAZDOR_DROP_PICTURES=1` runs this rule on the
  desktop (and lets snapshot runs keep the picture cache, so their frames can be compared).
- **Why a picture can be decoded twice (fixed):** the chrome cache is per path and per `Fx`,
  and decoding was per call, so a file used with two `Fx` (SteelLine) was decoded twice.
  The decode store is keyed by path alone, so each file is decoded once.
- Textures are created on the main thread only (macroquad/OpenGL); threads may only make
  `gfx::Image`s.

## Drawing helpers (`src/ui/chrome.rs`)

- `chrome::tex(t, rect, color)`, `chrome::tex_src(t, src, dst, color)` (a sub-rectangle),
  `chrome::tile`, `chrome::three_slice`.
- Blend modes as closures: `chrome::premultiplied`, `chrome::additive` (spell effects),
  `chrome::multiply`, `chrome::subtractive`, `chrome::silhouette`.
- Composite widgets: `chrome::empty_cell` (battle/army cells; front-row swords `wb1`,
  bow `wb2`), `chrome::wounds`, `chrome::glow_frame`, `chrome::badge`,
  `chrome::spell_icon`, `chrome::effect` / `chrome::animation` (multi-frame effects),
  `chrome::battle_effect`.
- Pixel transforms on `gfx::Image` before upload: `chrome::subtract`,
  `chrome::scale_channels`, `chrome::lock_portrait`, `chrome::transform`.
- Pictures the original makes from another at start-up are made the same way and cached by
  `chrome::visual_texture`: the brown text box's paper, and the card strips
  (`chrome::strip_picture`: plain, the hero's red-brown and a named character's blue, all cut
  from `Win-paper`, 0x4de0f0).
- The unit cards' strip (`unit_sheet::stat_strip`) and the unit panel's text
  (`unit_sheet::draw`) are placed in the original's pixels scaled to the card or panel (94 px
  card, 262 px panel), each text by its cell's top as the original does (`dt_font::cell_baseline`
  turns that into Razdor's baseline), with the sheet scale snapped by `dt_font::whole_scale`.

- **Sharp pixels** (`src/ui/sharp.rs`): DT art is sampled "sharp bilinear", so each original
  pixel is a crisp square of one size at any scale. `sharp::draw` is the drop-in for
  `draw_texture_ex` (the same placement and parameters); `sharp::fragment` builds a shader
  with `sharp_uv` (GLSL ES 1.00, highp uv, the `TexSize` and `Scale` uniforms, `sharp_tex`
  to sample), and `sharp::material` loads it. A blend (`with_material`) keeps its own
  material and its art draws sharp inside it (`sharp::in_blend`); the world's painter pass
  keeps one material set for all its art (`sharp::session`), and its uniforms are set only
  when they change, because macroquad breaks its batch at every material switch and every
  uniform set. The battle window (`BattleView::frame`, `frame_custom`) is one session too.
  Inside a session a plain draw (text, a texture without sharp sampling, a helper's own
  material) calls `sharp::plain_draw` first: it sets the default material once for the run of
  plain draws, and the next sharp draw sets the session's again. White shapes need no call,
  and a blend context keeps its own material. Text goes through `plain_draw` in `widgets`,
  `dt_font`, `script_font` and `chrome`, so a new plain texture or text draw in a session
  needs the same call. Textures stay `FilterMode::Linear` (the step needs the hardware bilinear
  inside its band). Stays smooth on purpose: the minimap and the fog, `Nearest` textures
  (window lines, custom PNGs, event pictures), the editor, the hub's catalog pictures and
  the TrueType fonts. The bitmap font draws its non-whole-scale sheet sharp (`dt_font`); a
  whole-number scale keeps the Nearest sheet. The terrain shader and the water frame sample
  sharp too (`terrain.rs`, its screen scales set in `TerrainLayer::draw`). Shaders that do
  texel math use highp (a Global Constraint: the credits scroll's 455×2840 texture breaks
  under mediump). A device whose fragment stage lacks highp fails to compile the sharp
  material: `sharp::draw` then draws plain, once logged with `diag!`.

## Window frames and lines

Every outline the original draws round a window or panel is one routine (0x48d1d4) with the
colour −1: the colours are `SteelLine.lit`'s texels read one by one, the picture taken flat
(a run past the row's end goes on with the next row), by an index that runs on round the
frame: top left to right, right side down, bottom right to left, left side up. A 2 px line's
second pixel lies inside the first in the same colour, and each side overwrites the corners
the side before it drew. Razdor draws this in `chrome::steel_frame` (the original's pixels
at a given scale) and `chrome::silver_frame` / `chrome::silver_line` (a width in screen
pixels: two original pixels or more is the 2 px line, else 1 px). `chrome::opx()` is one
original pixel on screen (0.9375 × `k`). Without the install the lines are plain silver.

- Every window (0x48d3f8, `chrome::window`): a 2 px frame round it and a 2 px line from
  (2, 27) to (w − 2, 29) under the title (`chrome::window_frames`).
- Windows made with inner frames (0x48d710 with its flag; the 634×516 window ae2520 of the
  scenario list, load/save, exit, village and others, the 634×556 Community window c2953a of
  the options, the leave-battle window, and the event window, 0x4aaa4b / 0x4aaa79): a 2 px
  line from (2, h − 56) to (w − 2, h − 54) over the buttons and a 1 px frame from (4, 31) to
  (w − 4, h − 58) round the content (`chrome::window_inner_frames`).
- The army, building, battle and hero windows (896×640, made without inner frames): a 2 px
  line right of the left panel from under the title line to the frame (x 264) and 2 px lines
  over and under the hint strip across the right side (y 319 and 346, 4cd330, 4bb7c4,
  4d1ce8); `window_layout::frame` draws them after the tab's content, at Razdor's 836×600
  layout's column and strip.
- The brown text box (0x48e4e8): Win-paper less (148, 138, 156) at 16-bit levels,
  Big_Frame_1a / 2a (loaded −223, −215, 0) taken off 4 px in from the sides, the four 34 px
  corners when asked, a 1 px frame. The hero window's boxes ask for no corners (0x4d1814 →
  0x48e750: item description, quest list and quest text; `chrome::text_box_no_corners`); the
  army window's item description does (0x498c2c). The hall's quest list on light paper takes
  the same side ornaments off, which show gold there (0x4bedec).
- The scenario list's map (0x4c1c09..0x4c1c63): `MapImageFrame` (295×162) copied as it is,
  the map's 267×134 picture 14 px in over its opening, no line round either; Razdor draws
  both at its window's scale.
- Worn-item slots (0x4d8638): 55 px, 8 px in from the panel fill's top and sides, each side's
  pair touching; an empty slot is ae2860 (`chrome::empty_item_slot`: the 1 px steel ring over
  40% black), a worn item its picture alone (0x492f24).
- Standard buttons show their pressed picture while held, not on hover (479eb8).

## The world map

**Past the map's edges** (the setting «Продолжать карту за краем», `Settings::map_extended`,
off by default; `world_view::set_edge_pref` takes it every frame): off, the original's black.
On, the ground goes on past each border, continuing the border's own row or column (the shader
clamps the cell index, `code_at`). From the border, the fog takes the war fog's own colour
(`minimap::FOG_RGB`, black, read by `terrain::edge_fog_colour`; the mist's grey with «Туман вместо черноты», see «Mist»): thin, patchy wisps, darker and
lighter with two soft value-noise layers of different scales drifting in different directions
(`Clock`, the water clock), thickening into solid by `EDGE_SOLID` (5 cells) and solid at
`EDGE_FULL` (7 cells), as `fog_opacity` (the shader's `smoothstep`, the noise at its brightest).
The fog is drawn after the ground and before the war fog, so the unexplored black and the edge
fog are the same colour and meet with no seam. `draw_terrain` covers the whole drawn ground, so
the fog reaches every edge; in the 2.5D view the same ground pass gives a black wall at every
turn. No scenery is copied past the edges. The war fog's texture clamps to the border's status
past the map (`minimap::draw_fog_extended` / `draw_extended`). The camera may reach `EDGE_FULL`
cells past each edge (`Camera::looking_in`, the scroll range included), and a map narrower than
the view is centred; off, the view stays on the map as before. Visual only: no rule, path, click
or AI change. Hex maps (the built-in demo) keep black.

`src/ui/world_view.rs`: `draw_world` draws only the visible cells — `draw_terrain`
(`TerrainLayer::draw` in `src/ui/terrain.rs`: one shader quad mixing the four nearest cell
textures from a 4×4 atlas, water frames multiplied in), then decorations
(`draw_object`) and buildings (`draw_building`) in one pass over the cells as the original's
(0x4c8864, 0x4c9b5b: row by row, left to right; in a cell its massif object, its plant, then
the building whose footprint covers it if not drawn yet, so a building comes at its top row's
left cell and every later object covers it; then the cell's figures: armies (`draw_army`,
`draw_figure`, `draw_ship`) and the hero (`draw_hero`), keyed by their cell, so a figure north
of a building is covered by it, as the castle's walls cover a hero behind them), the route (`draw_route`), and the tooltip
(`draw_tooltip`; its opaque box in the original's three styles is `widgets::tooltip_panel_styled`,
the layout as interface.md §7.4). Buildings are anchored like the original (right edge on the cell; see
PLAYTEST_NOTES "Мосты сдвинуты"). A figure inside a building is not drawn, as the
original's drawn-occupant byte (0x497abc): `draw_army` and `draw_hero` skip it when
`Game::army_hidden` / `Game::hero_hidden` say so (`World::hides_figure`: standing in a building
other than a bridge, or crossing between building cells; gliding in from outside it is seen
until the step ends). Its one-cell ring (`draw_mark`) stays, under the building picture. The
hero's row is the last step's direction while he stands (`Game::standing_heading`, 0x75c050;
the frame is the standing one), and the galley's the same; before any step it faces the viewer.
The minimap is `src/ui/minimap.rs`. Flat, its view is the light box of `world_rect`. Tilted (2.5D, first person,
eyes), `Camera::minimap_sight` hands it the camera: the ground the frame sees is the picked ground under its edges
(`minimap::sight_footprint`, four points an edge, a ray that meets no ground stops at `view25::FAR` or the bend's
horizon), clipped to the minimap and drawn with the box's colours, and the hero's marker (`draw_sight`, a light
triangle centred on his place, as long as a house icon (the `Grid(0, 2)` cell at the map's size) and 1.7 times as long
as its base, at least 24 dp on a phone, its tip where the camera looks, `look_direction`).
Clicks still centre the map: the minimap is a plan, not turned.

The small trees and bushes (object class 9) sway: `draw_object` draws them through
`draw_leaning`, the top corners moved by `rng::tree_sway` (a triangle wave of ±2.5 px over
3.57 s, 0x4c99d0) of the plant's phase, and its jitter (`rng::plant_jitter`), made once when the object is built (`Decoration::new`), not per draw.

Map figures (`draw_figure`) are drawn premultiplied (`chrome::premultiplied`): the sheets hold
premultiplied colour, as the original's ONE / INVSRCALPHA blend reads them (0x4c9ef6); the
Ghost's glow is light with alpha 0, invisible under straight alpha. Standing armies animate
while the map is idle (no walk, no wait): `idle_sprite_frame` takes the frame from
`src/rules/idle.rs` (`idle_frame`: the figure's 20-step table at 250 ms; `ship_idle_frame`;
`hero_ship_idle_frame` for the hero's galley at rest) by the wall clock since the army's stamp,
with its offset `AiMind::idle_offset`. The offsets come from the loads
(`Rng::map_load_idle_offsets`, `Rng::save_load_draws`) and the stop (`Game::armies_snap`, which
also counts `idle_restarts`); `stamp_idle` stamps each army at the first frame drawn and again when
its restart count changes, and `MapView::forget_shows` drops the stamps of the last game.

**The map's resolution** (`src/ui/map_scale.rs`; the setting «Разрешение карты», `Settings::map_render_scale`,
1.0, 0.75 or 0.5, cycled by `map_scale::next` in the Map group, always shown; pushed by `main_menu::apply_options`
into `world_view::set_map_render_scale`). Below 100% for low-end phone GPUs, `map_scale::draw` draws the world
(`draw_world` and the flat view's fog) into a target of `ceil(screen × scale)` physical pixels and lays it over the
screen with the copy composite (`render_cache::blit_screen`, Linear filtering), then the frame draws the rest at
native resolution: the bottom bar, the minimap, windows, tooltips (`draw_tooltip`), the cursor and text. The
same goes for the photo mode's world, the map under a window (`backdrop_world`, both backdrops) and their dim; the
window backdrops follow the setting too, as they are the same world drawing.
- The map under a window is frozen (`ui::map_freeze`, the owner's decision 2026-11-13; a weak phone GPU drew
  the whole map each frame under the settings window, 8 FPS). Every window's backdrop goes through
  `world_view::draw_map_backdrop` (`backdrop_lit`, `settings_backdrop`, `backdrop`, `window_backdrop`). The frame a
  window opens over the map draws the map live, then copies the screen into a screen-sized target
  (macroquad's `Texture2D::grab_screen`, a GPU copy, before the bar is drawn). The frames after show that copy
  (`render_cache::blit_screen`) while its key holds: `world_view::freeze_key` is the battle's key without the
  animation clock, plus the render scale, the towns (kind, picture, owner, cleared ruins, living garrison), the
  game clock and the settings' look (`map_freeze::look_of`: the whole `Settings` but the volumes and the
  speed sliders). A changed key or screen size copies once more, on the frame after the change. The copy is
  released a frame after the last frame with a window over the map (`map_freeze::begin_frame`, at the start of
  `App::frame`), so a frame with no backdrop releases it. A screen above 8192 px a side is drawn live. Not
  frozen: the battle's backdrop (`battle_backdrop`, its own cache) and the watching screen. Why a copy of the
  screen and not a draw into the map's own target: `render_cache::fill` and `map_scale::draw` do not nest (the
  inner `with_base` ends by sending the camera back to the screen, and inside a fill the map's scale is dropped).
- The target is the base of the camera: `render_cache::with_base` makes `back_to_2d` (and the end of every
  `fill_space`) return to it, not to macroquad's default (screen) camera, so `in_view`, the 3D views (`View3d`, whose render
  pass is `render_cache::draw_target`), `ground` and `standing` stay in it. A new camera reset during the world
  drawing must go through `back_to_2d`, or it draws to the screen.
- Screen maths stays in native screen coordinates (picking, `tile_under_pixel`, `Camera::screen_at`, the
  tooltip); only the rasterisation is smaller. The sharp shader's `Scale` is multiplied by the target's pixels
  per 2D unit (`sharp::set_raster`, set by `map_scale::draw`), so its band follows the pixels drawn. The ground
  cache's composite reads `gl_FragCoord` against the base target's size (`render_cache::base_pixels`).
- The target is kept by size (`map_scale::target_for`, made again when the physical size or the scale changes).
  The world is not drawn scaled while the screen is panned (`ui::pan::shift` non-zero: the pan's offset camera
  would land on the screen), past 8192 px, or at 100%, which is the old path with no target.
- A profiling switch overrides the setting (`map_scale::in_use`, read once): env `RAZDOR_MAP_SCALE=0.75|0.5`
  on the desktop, the file `profile-map-scale-75` / `profile-map-scale-50` in the app folder on Android.

**The 2.5D view** (`src/ui/view25.rs`: the tilted camera's maths, pure and tested). The setting
«Вид карты» (`audio::Settings::map_tilt`, off by default, pushed by `main_menu::apply_options` into
`world_view::set_tilt_view`); the env `RAZDOR_VIEW=25d|flat` overrides it for snapshots, with
`RAZDOR_VIEW_PITCH` (the start tilt), `RAZDOR_VIEW_YAW` (the start turn) and `RAZDOR_VIEW_SPRITE` (the
sprites' size). PageUp and PageDown tilt 5° a press within 35–75°.

- `looking_in` gives the flat camera a `Tilt`: a `TiltCamera` fitted to the view, so the cell under
  the target is as wide as a flat cell, and the ground the view shows (`plan`) and the ground the
  drawing culls to (`cull`). The pan's clamp (`Camera::looking_in`) keeps the 2.5D view's reach on the map
  (`ground_margins`: the bottom edge's chord for the sides, the ground to the top and bottom edges, turned by the
  yaw), so every edge comes into view at every turn (test `panning_to_any_map_corner_shows_it_at_every_turn`); the
  flat view keeps its box. The drawing runs in the flat camera's 2D space, `in_view` sets the 3D
  view (`View3d`: depth off, no viewport) on the cull footprint (`tilted_space`). `ground` lays that
  space on the ground for the terrain, the hills, marks, the route, bridges and the fog;
  `standing` makes each sprite a billboard through its foot (`view25::billboard_model`). A sprite's
  foot is the centre of the ground it stands on, the point it turns about and sorts by (`sprite_key`),
  not the cell's corner, where the picture hangs: a building the centre of its footprint (`building_foot`);
  a hill or mountain the centre of the cells it covers (`object_cells`: `object_side` cells square up
  and left, `object_foot`); a plant its cell's centre moved by its jitter, as its picture is
  (`plant_shift`); anything else its cell's centre. The sprite grows about that same foot (its base is
  the foot), so a grown picture keeps its ground centre in place: at the sprites' size 1.5 a picture's
  bottom edge moves down by half its footprint's offset from the foot (a grown castle's front comes lower
  at yaw 0). Tests: `every_standing_sprite_keeps_its_ground_centre_at_every_turn` (every decoration class,
  sprite and building picture of the install, drift under 2 px at 8 yaws). With the
  setting off, `in_view`, `ground` and `standing` only call their closures: the flat view is the same
  code path.
- **Cards** (a building sprite cut into pieces, each on its own ground: the owner's report, a village's
  houses floating in the air). **Off by default** (the owner's decision, 2026-11-12: a building stands as one whole
  picture in every view, as in v1.4.1). `dt_art::building_cards_on` (`RAZDOR_BUILDING_CARDS=1`, dev-only) turns them on:
  `Atlas::building_cards` returns None otherwise, so `card_building` is None and every path below is off, the depth
  model's worker (`depth_tick`) does not start, and the settings row «Модель глубины для модов» is hidden
  (`Row::DepthModel`). The text below describes the code with the switch on. `data/cards/objects.json` (numbers only, made offline by `tools/depth`) keys
  each sprite by its pixel hash (`dt::cards::sprite_key`, FNV-1a 64); `cards::builtin` parses it once and
  `DtArt::map_atlas` keeps the cards of each building sprite that has an entry of the same size
  (`Atlas::building_cards`). In the tilted view only, a building with cards is drawn as one `Drawable::BuildingCard`
  per card, each on its own foot (`card_foot`, sorted like a sprite) and standing as a billboard, its mesh
  (`card_mesh`, the sprite's uvs in the atlas, `sharp` sampling). Its shadow lies on the ground under the fog
  (`draw_building_shadow`, `chrome::shadow_only`: the sprite's shadow pixels only, with the painted shadows' `ShadowK`).
  **Groups** (the owner's decision, 2026-11-11: no rotation of assets, everything faces the camera). A card building
  is one group: `Drawable::BuildingGroup`, one item on the building's foot (`building_foot`), drawn as one billboard
  (`standing`, `draw_building_group`) whose cards keep their yaw-0 picture layout. Each card's foot moves to where its
  ground point is seen at the current pitch (`layer_offset`: vertical `d·(sin p − 1)` in sprite rows, `d` the card's
  foot row minus the group's, so the flat look at pitch 90 is the picture); a turn slides a layer sideways by
  `LAYER_PARALLAX` (0.08) times `d·sin yaw`, at most `LAYER_PARALLAX_MAX` (4) sprite pixels. `layered_cards` applies
  it to the verts and the foot together, for the drawn mesh, the shadows and the windows; a flag moves with its card.
  **In a bent view** (first person) the vertical move would float the layers off the rolled sheet, so there
  `draw_layers_on_sheet` stands each layer through `standing_at` at its own ground foot (`sheet_layers`: `card_foot`,
  unslid), so `standing_model` puts it at `Bend::point` of that ground point; only the sideways slide stays. The
  fade (`first_person_fade`) and the horizon are the building's, decided on its foot, so its layers fade and hide
  together; a layer whose own point is past a down bend's horizon is skipped. The shadows, drawn on the flat ground
  picture, use the cards as they are there.
  Tests: `layer_tests` (flat look, the ground line of back and front layers, the bounded slide),
  `tilt_tests::a_layer_stands_on_the_bent_sheet_at_its_own_ground_foot`.
 A bridge, a picture with no
  entry, and the flat view keep one billboard as before. The hover's gold outline goes under each card.
  The bundled `one_card` list (`data/cards/objects.json`) names the base buildings left with no entry (one region):
  they count as shipped, one billboard, no model. **Buildings with no shipped cut** (mods, community maps: `depth`, feature `depth-model`). `DtArt::map_atlas`
  reads each one's cached cut (`depth::cached`, the cache folder `cards` under `platform::cache_dir`, keyed as
  the shipped data) into the atlas's overlay (`queue::Overlay`), and queues the rest (`queue::unknown`).
  `DtArt::depth_tick` runs once a frame of the world map, and only when it is tilted. Once the model file is there
  (`depth::fetch::model_file`), it starts `depth::Service`: a loader thread loads the small plan (the download
  already checked the file), then `depth::worker_count` workers share that plan (half the desktop's cores, at most
  4; 2 on the phones), each cutting one sprite at a time at the lowest thread priority the platform gives. Each frame
  it hands the workers the queue, up to two per worker, drains their answers into the overlay, and sets
  `depth::unknown` (the count the Settings row reads). Input: `queue::input_size`, 266 px up to 120 px on the longer side, else 518 px: the same file,
  a second plan made on first use (`DepthModel::plan`). Nothing is fetched there. The model comes only from the
  Settings row **«Модель глубины для модов»** (`main_menu::Row::DepthModel`, shown in 2.5D while `depth::unknown`
  is above zero): its value is `depth::fetch::status` (Absent, Downloading, Failed, Installed), its click
  `depth::fetch::start`, a background download of `DEPTH_MODEL_URL` (the `depth-model-v1` asset of the community
  repository) into `depth::fetch::models_dir` (`app_dir/models`, the phones' cache folder). `fetch_verified` checks the
  size and SHA-256 before the file takes its place; a failure removes the partial file, and the row offers it again.
  `RAZDOR_DEPTH_MODEL=<file>` uses a file as it is (no download). Results are cached as they come (`cut`, `cache`).
- Picking: `map_pointer` is the ground under the pointer (NaN over the sky), `tile_under_pixel` the
  cell under a pixel at any turn (tested in `tilt_tests`). Overlays on the window (tooltip, battle
  marks, effects, the travel time) use `Camera::screen_at`. A figure is picked by its cell.
  **What the pointer is over, tilted** (the player's report, 2026-11-12: the ground under a house, a castle's
  tower or an army's body was missed). The sprites' pass of `draw_world_in` records each pickable standing sprite
  (the hero, an army not hidden in a building, a location's building or each of its card layers; decorations and hills
  are not picked) as the four corners of its drawn picture: `picture_quad` in `standing_at`, the identity from
  `standing_picked`, into `PICKS` (cleared each frame; the bent views' sprites' pass keeps it). `sprite_under` gives the
  front-most quad that holds the pointer (the last drawn; the record of the last frame, as the clicks read it), and
  `pointed_tile` turns it into a cell (`pick_cell`: the hero's or an army's cell, a location's centre cell or else its
  first footprint cell that can be targeted, so a click on a tower walks into the castle). The pointer shape, the hover
  ring and outline, the tooltip (`pick_tooltip`) and the click use it; a miss takes the ground cell as before. Flat, nothing
  is recorded and `pointed_tile` is the ground cell. A quad hits only where its texel is opaque (alpha at least 128):
  `texel_uv` inverts the quad's bilinear map (Newton) to the texel in the sprite's source rectangle, and `opaque_hit`
  reads the alpha from `sprite_alpha` (a CPU copy kept when the atlas and the figure sheets are made, `sprite_texture`,
  never read from the GPU). A transparent texel falls through to the pick behind, then the ground. A sprite with no mask
  (a texture made elsewhere, a procedural figure) hits as its quad. A figure's source is its sheet frame (`FIGURE_SRC`,
  set by `draw_figure`); a building's is its atlas picture (`atlas_src`). Card layers share the group's picture, so
  they share its source.
- **Turning.** Q and E turn 45° (`TILT_YAW_STEP`); the shown yaw eases to its target (`ease_yaw`,
  12 a second, the short way round, snapped within 0.05°). Q and E cut a walk only on the flat map
  (`cuts_walk`). Screen moves (keys, edge, stick, drag, slide) go through `Camera::screen_to_world_delta`
  and `turn_ground`. Turned, standing sprites sort back to front by their foot's depth (`sprite_key`,
  `is_turned`; at yaw 0 the cell pass), every side of the cull takes the larger margin, and a figure's
  facing row follows the screen (`Camera::facing`).
- **Two-finger turn** (setting «Поворот двумя пальцами», `Settings::map_twist`, off by default; `gestures::set_twist`).
  `ui::input` sets the mapper's `twist_on` only with the setting and the 2.5D view on. The fingers must
  turn 12° past their touch (`TWIST_LATCH`) before the motion is a twist: a pinch or a slide keeps its
  wheel and pan. Then each 45° of turn is a step, quantized while the fingers move (the eased view
  follows); the leftover under a step goes to the nearest step when the fingers lift (`settle_twist`).
- **Compass.** `compass_rect` shows the button while the view is turned away from north up
  (`compass_shows`, the flat view never). Its place is the first of: beside the bottom bar (right,
  then left), above it at its right or left end, the top right corner, clear of the minimap window
  and its army list (`compass_place`); phones 48 dp, the desktop 40 px. Its needle is north on the
  screen (`view25::north_on_screen`). A tap (`tapped_in`) sets the yaw target to 0 (`reset_tilt_yaw`);
  it hides once the view is back. Its rectangle counts as the minimap's pointer (`on_minimap`), so the
  map takes no click under it.
- **Sprites' size** «Размер фигур» (`Settings::map_sprite_size`, 1.5 by default, cycled 1×, 1.5×, 2×
  by `next_sprite_size`; `set_sprite_size` pushes it; `sprite_size()` is what `standing` scales by).
- **Detail when the whole map is zoomed out** (the owner's request, 2026-11-11). `detail_visibility`
  (physical px, the shorter side): an object under 2 px is not drawn, one under 3 px fades in (a
  smoothstep; the flat view fades it through `SPRITE_VIS`, the 2.5D one through `standing`'s `vis`). At
  1280×800 on the Discord Times map the whole-map zoom's objects are 3 px and up, so nothing is skipped there;
  at 800×600 the 2–3 px ones fade. `zoom_lod(scale)` is the animations' shared level of detail (0 under 8 screen
  px a cell, 1 from 16, linear between): at 0 the living map is off (`living_on`: the flags, the chimney smoke
  and the birds), the birds and the ship's wakes fade with it, and the water's ripples and foam may use it too
  (`water.rs` is its own agent's). `RAZDOR_MAP_FIT=1` (snapshots) zooms to the whole map's floor, which the
  settings' zoom range cannot reach. Measured with `RAZDOR_PROFILE_FRAMES` (draws, verts) at 1280×800:
  the flat whole-map frame is 1123 draws and 64,400 verts, the 2.5D one 193 and 76,251.
- **The map atlas is cut to boxes** (`src/ui/atlas_cut.rs`, the perf plan's item 6). Each sprite keeps its place in
  `gfx::pack_atlas`'s layout, so every texel is the one it was, but a picture is drawn only as its box: its opaque
  texels grown by one, with the lit windows' emission counted (`atlas_cut::picture_box`). `DtArt::map_atlas` gives each
  sprite a `Slot` (the box's texels `rect`, its offset `off` and `size` in sprite pixels, the texel of the sprite's
  pixel (0, 0) `base`, and the sprite's `full` size). Placement still uses the whole sprite (`building_sprite_origin`,
  `object_sprite_origin`, the foot); a draw places the box at `origin + off * zoom`. The shading centre, the lean and the
  card and part meshes use `full` and `base` (a `Frame`), so they are the whole sprite's. The shadows have their own
  slots in the same texels (`DtArt::shadow_decoration` / `shadow_building`: the box the blur reaches), and the lit mask
  covers each sprite's whole rectangle as before. The living body is cropped to the box.
  **Where the box is not exact** (measured on card1 against origin/main, 48 shots, `RAZDOR_SCENE` «Другой берег» and РК3,
  flat / 2.5D / first / eyes, near and whole map, plain / sun / night): the swaying trees (`draw_leaning`), a picture drawn
  with the sun's form shading (`draw_picture_form` with a sun), and a picture drawn at night (`lit_whole`, the lights'
  night factor) take the whole sprite rectangle. Each of these drew up to 114 levels apart at isolated texels with the
  box. The shadow boxes are exact (full-box shadows changed nothing). With these rules every shot is within 2 levels
  and has no pixel above 7 (PSNR at least 77 dB). The rest of the picture uses the box, so by day the difference is
  float rounding of the quad's corners.
- **Smaller map sprites** (on by default; `RAZDOR_SPRITE_LOD=0` turns them off): `lod_copies` stacks the half and
  quarter copies of every map sprite (box filter, `downscale`, cut like the full atlas) under the full picture in the
  map atlas's own texture, so a switch of copy keeps the batch. `d` = 2 under 12 screen px a cell, 4 under 6
  (`sprite_lod_divisor`); `draw_object` draws a decoration from `Atlas::decoration_lod(d)` at the full sprite's place
  and size, with the copy's texels (`Slot::tex`). When the stacked texture passes `GL_MAX_TEXTURE_SIZE` (queried), the
  atlas is the full one alone and the draws use the full sprites. The buildings are not switched. Memory on
  «Другой берег»: the full atlas 2048×1684 (13.8 MB of RGBA) becomes 2048×2298 (18.8 MB) with the copies. Draw calls
  and verts do not change.
- **The border** (`draw_border`): black lines on the ground before the sprites, a 0.15-cell line on the
  fog grid's edge and a 0.07-cell one 0.35 cell outside (at least 1.5 px). The fog, drawn after the
  world, darkens it where the ground is unexplored. Past the map's edges the ground is black (the
  flat fog's rectangles, laid on the ground), unless «Продолжать карту за краем» is on: then the
  ground and its fog go on past the edges (a short strip, then the fog) in the tilted view too (see «The world
  map» above).
- The settings rows: «Вид карты» always in the Map group; «Размер фигур» and «Поворот двумя
  пальцами» beneath it with the 2.5D view on (`main_menu::Row`, the Map group's rows, drawn by `group_pane` on both platforms).
- **The fog under the sprites** (the owner's report: a castle or a church sliced by black). In the 2.5D view
  the war fog and the event reveals (`draw_fog_flat`, `draw_showing_flat`) lie on the ground in `draw_world_in`
  after the ground's marks, route and border and before the standing sprites, not after the world as in the flat
  view (`MapView::draw` draws them there, flat only). The hills, drawn first in the flat view, come after them.
  Each map sprite (a decoration, a building, a hill; `standing_map`, `standing_picked_map`, `draw_building_group`
  with `Shading::Map`) fades into the fog on its foot's cell, so what is behind it shows through instead of a black
  silhouette. Since backlog 1605 step 4 the fog is the shader's: `ui/sprite_fog.rs` keeps a table of one 16-bit visibility
  per map cell (`Table::compose`: the visibility, `sprite_fog::visibility`, a smoothstep from 0.15 to 0.85 of 1 less
  the darkness there, the shown darkness (`minimap::with_shown_darkness`, unrounded) composed with the reveals' alpha,
  `1 − (1 − fog)(1 − reveal)`). The table is a texture (`sprite_fog::texture`, nearest, clamped, the red and green bytes
  the high and low byte) bound as `FogVis` by the sharp materials (`sharp::FOG_GLSL`, `fog_vis()`, which truncates to 8
  bits as the CPU alpha did). A map sprite's draws run in `sharp::with_fog_cell` with its foot cell's texture
  coordinate, which the sharp draws put in the vertices' normal `zw` (`fog_normal`), so the vertices do not change when
  the fog does. The CPU reads the same table for the cull: a hidden cell (or a foot past the edge with no edge fog) draws
  and picks nothing. `sprite_fog::sync` runs in `draw_world_in` after the war fog's draw (`sync_sprite_fog`): the table is
  made again only when the fog's stamp (`minimap::shown_stamp`, moved by every changed cell) or the reveals move; the
  reveals are made again every frame while a showing is opening (its mask is rewritten each frame by
  `draw_showing_flat`), and the texture is uploaded only when a value changed. Armies, the hero, spells, the smoke and
  the lit windows keep the CPU shade (`Shading::Cpu`, `Shade::at`). A straight-alpha sprite keeps its
  colour and scales its alpha (`faded`); a premultiplied one (the figures) scales every channel (`faded_premultiplied`).
  At 0 it is not drawn. Off the map the ground is black (visibility 0) unless «Продолжать карту за краем» is on.
  Razdor's visual only: the flat view is pixel-identical. Sprites' fallback drawings (no DT art: procedural
  ships, units by `draw_unit`) are not faded.
- **The 2.5D fog's box**: `draw_fog_flat` on a tilted camera draws `minimap::draw_fog_boxed`, only the sub-quad over the
  texels that are not fully transparent (the box grown by one texel a side; nothing when none is dark). The same
  pixels as the whole quad, fewer fragments. The flat map view and the minimap keep `minimap::draw_fog`.
- **«Анимация карты»** (the owner's decision 2026-11-11, `src/ui/map_anim.rs`, `Settings::map_anim`: `Mode` On, Near, Off;
  the row «Анимация карты» in the Map group, cycled On → «Только вблизи» → Off; env `RAZDOR_MAP_ANIM=on|near|off` overrides it).
  The map's animations are scaled by one level: 1 with On, 0 with Off, `world_view::zoom_lod` with Near (0 under 8 px a
  cell, 1 from 16). `map_anim::begin(cam.scale)` sets it at the start of each map frame (`map_frame`, before anything
  animated) and moves the animation time on: `map_anim::now()` is the frame clock less the time the level was under 1
  (`LOST`), so the animations resume where they stopped, and with On it is the frame clock to the bit. Every animated
  part reads that time, not the frame clock: the trees' sway (`clock_ms`, `draw_leaning`, scaled by the level), the
  armies' idle frames (`idle_sprite_frame`), the water (`terrain.draw`'s water time, and `water::live()`, which is false
  at level 0), the living map's flags and smoke (`living_on` needs level > 0), the mist's clouds (`mist::clock`), the
  weather's motion (`draw_weather`) and the torches' flicker (`draw_daylight`: at level 0 the middle of its range, steady).
  The wakes go with the water. The fog's reveal is instant under Off (`minimap` `track`: a fresh update when the
  fog changes). Under Off the idle frames are paced as «Экономия батареи» does (`frame_cap::current`). Off is not a
  rule: nothing a map or a mod sees changes.
- **The fog fades** (the owner's request, 2026-11-10: a smooth reveal, not a pop). `minimap::FogShown` (per thread)
  keeps each cell's shown darkness (0..255, unrounded) and eases it toward its target (`darkness_into` of the fog
  with its held cells dark): an exponential approach with `FADE_TAU` (0.2 s), monotonic, snapped within half a
  level (`ease_toward`). A cell that opens waits `WALK_DELAY` (0.05 s) per cell of distance from the hero before its
  ease starts, so a walk's strip opens outward from him; a cell that closes goes at once. The reveal cells of a
  place being shown (`Showing::reveal_cells`, passed as the `settle` cells of `draw_fog_flat`) jump to their target
  each frame: the radial mask (`draw_showing_flat`) draws their circle, so the fog under it does not fade twice.
  `forget_shows` (a game loaded or started, the map's opening) snaps everything (`snap_fog`). `track` does the
  update; it runs only when the fog or held cells change or a cell still eases, so an idle map does no per-cell
  work and uploads nothing (`fog_texture` uploads the texture when a texel changed, counted in the frame probe's
  rebuilds). The flat map, the minimap window and the 2.5D sprites all read the same values. When the explored bits
  change (a walk's reveal), `track` measures again only the box of the changed cells grown by the feather and one cell
  (`changed_box`, `darkness_from_bits`), the same values the whole measure gives; a new fog, a fog that is off or a box
  over a quarter of the map takes the whole measure (`darkness_into`). The unit test
  `the_shown_targets_stay_the_full_darkness_through_walks_reveals_and_held_cells` pins it.
- **The first-person view** («Вид карты» = «От первого лица», the owner's decision, 2026-11-10). Its
  setting is `audio::Settings::map_first_person` within the tilted view (`map_tilt`), pushed by
  `main_menu::apply_options` into `world_view::set_first_person`; `world_view::first_person()` is the
  view, `tilt_view()` stays true for both tilted views. The far ground's shape «Даль» is
  `Settings::map_horizon` and `map_globe` (`Row::FarGround`, shown only with the view): `map_globe` on «Шар» (the
  ground curves down in every direction from the hero, like a small planet: a circular horizon), else `map_horizon`
  on «Горизонт» (it curves down like a planet, the sky above), else off «Вверх» (the far ground rolls up to
  top-down). `Settings::far_ground` gives the shape as a `world_view::FarGround`; the row cycles Up, Horizon, Globe.
  It is visual only: no rule reads it.
  - **In the settings** (the owner's decision, 2026-11-10: shown to players again): «Вид карты» cycles four views,
    flat, 2.5D, «От третьего лица» (the camera behind the hero, `first_person`) and «От первого лица» (through the
    hero's eyes, `eyes_setting`); the far ground «Даль» row shows for both first-person views. `Settings::map_view`
    and `set_map_view` read and write the three flags (`map_tilt`, `map_first_person`, `map_eyes`), so an old file
    reads the same view and an older build still reads the file. Developer overrides in
    `RAZDOR_VIEW`: `fp-chase` forces the view behind the hero (the saved camera choice does not count), `fp-eyes`
    forces the eyes camera; `first` (or its alias `third`) and `eyes` force them as in the snapshots.
    The curve («Начало изгиба», «Сила изгиба», shown with either first-person view): `view25::CameraTune` per camera,
    `Settings::map_third_tune` and `map_first_tune` (pushed by `world_view::set_tunes`). Its `flat` is the bend's flat
    part (cells, 0 to 30) and the arc's radius is the far ground's default divided by `strength` (`CameraTune::radius`;
    ¼ to 4, a log2 slider). The defaults are the constants (`THIRD_TUNE`, `FIRST_TUNE`), so the default look is the same.
    Both curves are in the cached pictures' keys (`curve_bits`). The camera's height, tilt and field of view
    («Высота камеры», «Наклон камеры», «Угол обзора») are the same `CameraTune` (the eyes' pitch may be negative:
    it looks up, with no bend above the sky's horizon), with «Изгиб и камера» back to the constants.
    The third person's auto-turn («Камера поворачивается за героем», `Settings::map_auto_turn`, `world_view::set_auto_turn`):
    while the hero walks (`Game::moving`) and the player has not turned since his walk started (Q, E, the twist or the
    compass set `AUTO_PAUSED`; a new walk ends it), `follow_walk` sets the yaw's target to his walk. The shown yaw eases
    at `AUTO_TURN_RATE` (5 a second, a time constant of 0.2 s, exponential so no overshoot and the same at any frame
    rate); a reversal eases through the short way round. Standing, the target does not move. At the gentlest strength a low pitch can see sky
    above the horizon at the frame's centre (the eyes look down 2 degrees): the planet's horizon, not a pick bug. `RAZDOR_VIEW_HORIZON=down|up|globe` (`1` is
    `down`) overrides «Даль»; `RAZDOR_VIEW_GLOBE_RADIUS=<cells>` tunes the globe's radius for the snapshots.
  - The camera: `view25::TiltCamera::first_person(look, scale, frame, yaw, far, tune)` (`far` is the `FarGround`, `tune` the
    third person's `CameraTune`). Its pitch and field of view are the settings («Наклон камеры», 5 to 60 degrees, default
    `FP_PITCH` 12; «Угол обзора», 35 to 90, default `FP_FOV` 55); PageUp and PageDown tilt the 2.5D view only. `tune.height`
    («Высота камеры», ×0.5 to ×2.5, default 1) multiplies the fitted distance, so the hero's ground stays where it is. It looks at the view's
    centre (the hero when followed) from beyond it, so the centre stands at two thirds of the frame's height;
    the distance is fitted so the hero's ground is drawn at the flat map's scale (the depth
    `frame.h / 2 / (tan(fov/2) · scale)`). Yaw, Q and E, the two-finger twist and the compass are as in 2.5D.
  - The bend: `view25::Bend` with a signed `max_deg`. Up (`FP_BEND_UP` 78°, radius `FP_RADIUS` 24: a wide roll, the
    tight radius 8 turned the far ground square-on within ten cells, too fast) rolls the sheet to square-on with the eye; down (`FP_BEND_DOWN` -60°, radius `FP_RADIUS_DOWN` 60) curves it away.
    The down bend's `limit` is its horizon, where the eye's sight line grazes it (`Bend::with_horizon`): past
    it the sheet is not drawn, not picked, and `Bend::point` stands a point on the horizon's line (`hidden`
    keeps a sprite there from showing). A tight radius puts the horizon near mid-frame; a wide planet's curve
    would lift it, and the footprint would need the ground to the horizon (about 170 units).
    The globe (`Bend::globe`, `FP_RADIUS_GLOBE` 40, `RAZDOR_VIEW_GLOBE_RADIUS`) curves the sheet down in every
    direction: its distance is radial (`reach`), and its horizon depends on the bearing (`horizon_at`): the eye
    stands behind the pivot, so the sight line grazes the arc nearest straight ahead and farther out at the sides
    (the closed form `A sin a + B cos a = R`). Past its bearing's horizon `point` clamps to that horizon's line, so
    the silhouette is the true one at every side (a single forward limit would leave a hole of sky at the sides).
    `Bend::limit` stays the horizon ahead, for the sky's line and the mist's fade. Its picking is `pick_globe`: the
    ray is marched over the disc of the sheet's reach, the sheet at `globe_drop` high. The globe's pivot is the hero's
    ground (`look`), not the camera's target, so the flat disk of `FP_FLAT` holds the hero and the land at his feet
    stands at height 0 as in «Горизонт»; the sheet then falls away on every side. Its footprint is the square about the
    pivot out to the farthest horizon (`TiltCamera::globe_footprint`): the frame's edges alone would miss the far rim
    beside the middle, and the picture would leave its sprites floating in the sky. `screen_at` puts a globe's cells on
    the sheet too (the flat projection is only right near the pivot). Radii: `FP_RADIUS_GLOBE` 40 (the default, the rim at
    the flat view's horizon), `EYE_RADIUS_GLOBE` 150 (the eyes view's horizon about 14 cells away).

  - Drawing: `draw_world` gives a bent view one `in_view`: `ground_in_bent` (the terrain) and `draw_world_in`
    (the ground overlays, then the sprites) run in one picture, the sheet is drawn once, and the sprites stand
    on it. The picture covers `TiltCamera::ground_footprint`, which for a bent camera is the bounding box of the
    sheet the frame's edges meet (`bent_footprint`), so it reaches the top of the frame. `bent_target` rounds the
    picture to 64 px and keeps four sizes, capped at 8192. A bent `in_view` runs its closure twice (the ground's
    pass and the sprites' pass): raw 2D draws inside it would land in the wrong space, so draw through `ground` or
    `standing` only.
  - Picking: `TiltCamera::pick` marches the ray against the sheet's height at its forward distance (`height_at`, a
    closed-form inverse `s_of_u`) in steps of 0.1 and bisects the first crossing (`pick_bent`, `first_meet`); past the horizon
    there is none, so the sky sees no ground (NaN in `map_point_at`). Tests: `pick_undoes_the_bent_sheet...`,
    `a_click_on_the_hero_in_the_first_person_view...`.
  - The sky: `draw_sky` fills the frame dark (`SKY`) for 2.5D and «Вверх»; for «Горизонт» a gradient from light
    blue at the top to pale at the projected horizon line, which the ground covers below it.
    Without a horizon, with the camera covering the whole target, the 2.5D sky is the frame's `clear_background(SKY)`
    (the same picture): every caller of `draw_world` clears just before it, with nothing drawn between.
  - Near fade (the owner's report, 2026-11-10: with the old fade, every tree and building between the camera and the
    hero vanished, the whole lower half of the view). Only a sprite that covers him fades. `draw_world_in` sets
    `HERO_ON_WINDOW` (his drawn rectangle on the window and his depth along the view) before any sprite of the pass,
    the hills included. `standing` projects the sprite's picture (its 2D rectangle under its billboard model,
    `picture_on_screen`): if it overlaps his rectangle widened by `FP_HERO_MARGIN` (0.1 of his width) and stands in
    front of him along the view (the depth `sprite_key` sorts by: a sprite drawn over him is one in front of him,
    `covering_gap`), it fades out across `FP_NEAR_BAND` (0.01 of his distance from the eye, `first_person_fade`): gone
    in front of him, whole behind him. Any other sprite stays whole. The depth, not the eye distance: in the bent
    view a building can be nearer the camera on screen, drawn over him, and still farther by eye distance. The
    pictures are the rectangles the draws use (`object_picture`, `building_picture`, `army_picture`, `hero_picture`,
    `spell_picture`). `NEAR_LIMIT` is his distance from the eye (set at the start of `draw_world_in`). The hero never
    fades (`without_near_fade`). The trailer's near fade keeps its distance fade (`near_fade`).
  - Fog, sprite fade by fog, the smooth reveal and the UI are as in 2.5D; the trailer's hide-UI (`trailer_clean`)
    is only with `RAZDOR_TRAILER_CAM`. Off the map (past its north and south edges) the ground is black, as in 2.5D.
  - **The eyes view** («Камера» = «Глазами героя», `audio::Settings::map_eyes`, pushed by `main_menu::apply_options`
    into `world_view::set_eyes`; `RAZDOR_VIEW=eyes` for snapshots, `fp-eyes` for the developer override).
    `world_view::eyes_view()` says whether the camera stands at the eyes now: the setting, or a shot's `eye`.
    The camera is `view25::TiltCamera::eyes`: the eye `FIRST_TUNE.height` (0.7 cell, «Высота камеры», 0.3 to 3) above the
    ground under `target` (`eye_at`), looking `FIRST_TUNE.pitch` (2°, «Наклон камеры», -5 up to 30) along the yaw, its
    field of view `fov` («Угол обзора», default `FP_FOV` 55), with no bend of its own; `eye_bend` adds the sheet
    and its horizon. `world_view::eyes_tilt` builds it on the hero's displayed place (a shot's target in a run),
    so it glides with his walk. The yaw follows his next step (`follow_walk`, before `ease_tilt_yaw`); standing,
    Q, E, the twist and the compass turn it. The far ground: «Вверх» rolls up (`EYE_RADIUS_UP` 8, `EYE_BEND_UP` 78°,
    after `EYE_FLAT` 2.5); «Горизонт» curves down (`EYE_RADIUS_DOWN` 60, `EYE_BEND_DOWN` -60°) to the horizon that
    `Bend::with_horizon` finds from the eye. The hero's figure, his ring and the route are not drawn (`eyes_cam`).
    A standing sprite fades out within `EYE_NEAR` (2.5 cells) of the eye (`near_fade_limit`, a fixed distance, not
    the hero's, who stands at the eye); a sprite whose foot is behind the eye is not drawn (`standing`). Picking is the
    eye camera's own inverse (`pick_bent`); tests `the_eyes_camera_stands_at_its_height_and_picks_its_sheet_at_both_bends`
    (view25) and `a_click_on_the_ground_in_the_eyes_view_picks_its_cell_at_both_bends` (world_view). Known: a partly
    faded tree over the bright sky looks translucent (the fade's band is 2.5 to 3.75 cells).
**Lighting** (the setting «Освещение», `Settings::map_light`, off by default; `daylight::set_prefs` takes
it every frame, from `display::follow_settings`): `daylight::light_at` maps the hour to a tint and a glow
strength (a sine of the sun's elevation, a smooth step for the brightness, a bell for the dawn and dusk
warmth, a night floor). The hour comes from the game clock (`ease_hour`: the clock's own steps are followed at once, so the light and the shadows do not trail a
travel; a jump of 1–6 h is moved toward at 24 h a second; a load snaps), a real-time cycle, or a fixed hour; `RAZDOR_LIGHT_HOUR` overrides it
for snapshots, and `RAZDOR_LIGHT_MODE=game|cycle` (with `RAZDOR_LIGHT_CYCLE`, minutes a day) picks the clock of a snapshot run. `world_view::draw_daylight` runs right after
`draw_world` and before the flat `draw_fog` (after it with the mist on, see «Mist»): a multiply tint over the map area, then the point lights' pools
(«Point lights» below) in screen space (`Camera::screen_at`). Off, it draws nothing. The frame's
light is read once, before the world (`world_view::frame_light_of`), because the painted shadows' factor below needs it there too.
**No relief** (this round, the owner's report: the grooves looked overcooked). The ground and the sprites take no
shading from the light any more: the old per-pixel relief (the luminance as height, the slope from the neighbours,
`sharp::RELIEF_GLSL`, the terrain's `ground()` shade) and its normal maps are gone, with the light's direction, the
raking strength and the cavities. What the light still does: the tint and the glows (above), the sun's and the moon's
cast shadows (below), the point lights' pools and silhouettes (below). The painted shadows (alpha 0 < a ≤ 0.76)
keep their factor: `daylight::frame_shadow_k` (`shadow_k_at`: 1.15 at low sun, 1.0 at noon, 0.55 at night; 0 in Sun
mode, where the sun casts them) is the lit material's `ShadowK`, set once a frame by `sharp::session_lit`, and the
same factor scales `chrome::shadow_only`. The lit material is that alpha and nothing else, so the pass batches as the
plain one does; `None` (the light off) is `sharp::session`. The ground cache's key does not hold the light, and
the ground is the same by day and by night. The ground (`TerrainLayer::draw`) takes no light uniforms.
Not lit: the battle and window
backdrops (`draw_world(…, None)`), the fog, the marks and the route. Known limits: an atlas sprite's neighbour can be
a neighbouring sprite's edge copy (the atlas gutter is one clamped pixel), so an outermost pixel may pick up that edge;
the 2.5D ground is lit without the pitch's foreshortening.
**Steady brightness** (the setting «Ровная яркость», `Settings::map_steady`, on by default (a file without the key too), shown and acting only with the light on; `daylight::set_steady`
every frame from `display::follow_settings`; `RAZDOR_STEADY=1|0` for snapshots): the light keeps its colour (blue at night, warm at dawn and
dusk) and not its brightness. `daylight::lit` (in `frame_light`) passes the tint through `steady_light`, which rescales it to the luminance
`STEADY_LUMA` (0.85, Rec. 709 weights) with the channel ratios kept; `steady_rgb` makes up on the other channels what a clipped one loses.
0.85 is the highest target at which the night still reads blue (about 0.75, 0.87, 1.0); the day is then 15 % under the original's plain noon.
The sky (`daylight::sky_rgb`, the water's) keeps the day's luminance (`steady_rgb(sky_at(h), luma(SKY_DAY))`), its hue only. Not held: the
glows (the windows, the torches' pools and the point lights: a lit night still lifts the frame above 0.85, as intended), the painted shadows'
factor (`shadow_k_at`, 0.55 at night and 1.15 at dusk: it sets the shadows' contrast, not the map's brightness) and the cast shadows (`casts_at`).
Off, `light_at` and the frames are as before (the tests pin both).
**Cast shadows** (the setting «Тени», `Settings::map_shadows`, `daylight::ShadowMode`, under «Освещение»;
«Нарисованные» is the default and keeps the picture as above). «От солнца» (`ShadowMode::Sun`, `RAZDOR_SHADOWS=sun`
for snapshots) removes the painted shadows (`ShadowK` 0 in the lit material) and casts real-time silhouettes instead:
`daylight::casts_at(hour)` gives the sun's shadow (its bearing away from the sun, east at 6:00, south at noon, west at
18:00; length `1/tan(elevation)` on a stylised shadow elevation (`shadow_elevation`: 12° at dawn and dusk, 40° at noon, so the dawn and dusk shadows hit the 2.6 cap), clamped to 0.25–2.6; alpha 0.6 by day, fading through the dusk)
and the moon's (fixed in the south-west, alpha 0.15 from 20:00 to dawn). The cast shadows lie in the map's axes, the same at any turn. In
`world_view::draw_world_in`, before the hills, `draw_shadows` draws each caster in view on the ground (the point lights' silhouettes too, «Point lights»): a building
with cards casts one quad per card from the card's own foot, anything else one picture from its foot. `shadow::Projection::point`
maps a picture pixel to the ground (its x stays, it moves along the bearing by its height above the foot times the length);
the quad's texture is the picture's blurred silhouette in the shadow atlas (`DtArt::shadow_texture`, built on the first
sun frame: the map atlas's layout, RGBA, the silhouette in alpha, since macroquad's public API has no R8). Each quad is the cool
colour `shadow::TINT` with the alpha `light · fog visibility of the foot's cell · (½ on water)`, drawn with the default pipeline (the texture's colour is white, the vertex colour carries the tint), drawn with the ground's fog and the standing
sprites over it. Figures keep their painted shadows. Lighting off or Painted: nothing of this runs.

  *Penumbra and tone* (this round): each caster is drawn twice, the sharp silhouette (`DtArt::shadow_texture`) weighted by 1 − t and a soft copy (`DtArt::shadow_soft_texture`, three [1 2 1] passes, built with the sharp one) weighted by t, where t is the height above the foot as a share of the picture (`shadow::Projection::picture_mesh_ramp`, `card_mesh_ramp`: per-vertex colour): crisp at the base, softer at the tip. The colour is `shadow::TINT` (0.42, 0.40, 0.45, warmer than the old blue-grey) with the alpha eased by `shadow::EASE` (0.85). *Self-shading* (this round): in Sun mode by day, a picture's quad (`world_view::draw_picture_form`, cards in `draw_card` via `card_mesh_form`) carries per-corner colours from `form_colour`: the face turned away from the sun (`daylight::form_sun`, the screen direction of the sun by `cam.facing`) darkens up to 40 % (low sun) to 20 % (noon); a shaded object shades itself, not the objects behind it. The ground shadow keeps its alpha (`shadow::EASE` 1.0) and is drawn by a multiply pass (`shadow::draw_multiply`: dst × mix(1, tone, strength × silhouette), tone (0.48, 0.50, 0.62), no alpha-over; the sun's strength is 0.9 by day, `daylight::SUN_ALPHA`), so grass in shadow stays green. The lit material (only the painted shadows' `ShadowK`, `sharp::session_lit`) is used whenever the light is on, by day and by night.
**Point lights** (the setting «Качество света», `Settings::map_light_quality`, `lights::Quality`: high by default on the desktop, medium on a phone (`display::follow_settings`); a row under the lighting rows, shown with the light on; `RAZDOR_LIGHT_QUALITY=high|medium|low` for snapshots). `world_view::point_lights` builds the frame's lights each map frame (`lights::set_frame`, read by `lights::with_frame`): one per non-bridge building explored near the feather, at its footprint's centre, its reach its sprite's size (`BUILDING_RADIUS`); a torch in the hand of the hero (not in the eyes view, not aboard) and of each army in view (not a ship), its reach `TORCH_RADIUS` 1.6 cells, at `lights::torch_offset` (0.4 of the figure's height from the head, along its heading, 60° above horizontal; the heading is the sheet's row, `cam.facing`). The strength is the night's factor (`daylight::Light::glow`). `lights::Frame` sorts them nearest the view's centre; the nearest `quality.lit()` (8 high, 4 medium, none low) cast shadows. **Pools**: a building's light is a warm disc of twice its reach on the ground (`draw_building_pools`, in `draw_world_in`'s ground pass, before the sprites, so the building's own picture covers it: the light lies around the building and on what stands near it, never on the building itself); a torch's pool (`draw_daylight`, after the world) flickers; both fade with the fog on their cell. The building's own light also does not shade its picture. **Shadows** (`draw_shadows`): for each lit light, every caster in view within its reach lays its silhouette (`lights::cast`: away from the light, longer and darker nearer it, `CAST_ALPHA` 0.8 times the falloff; a light's own building casts nothing), under the fog, with the sun's; they show at dusk and night. **Lit windows** (`dt::windows`, the building mask's G; `ui::dt_art::DtArt::building_emissive`, built on the first lit frame): per building picture, the enclosed dark openings are found once from the picture's own pixels: candidates are opaque pixels darker than the median by 0.10; a 4-connected component is a window if it is 3–80 px, its bounding box is 0.25–4 in aspect with at least 45 % fill, and every pixel of its outer ring is opaque and at least 70 % of it is brighter than the component by 0.08; a door (taller than wide) may be 200 px and have ground under its bottom ring row. A component inside a window or door box of `data/lights/objects.json` (the detector's, by the sprite's pixel key) is kept with looser rules (2 px, half the ring). Kept openings are G 255, the ring around them G 90 (the halo), then one [1 2 1] blur pass clipped to the body feathers the edges (×1.25); the mask is sampled linearly, so G interpolates. Roofs: a component in the top 45 % of the body, brighter than 0.18, whose colour is the roof's dominant hue (30° buckets, saturation ≥ 0.15) is rejected unless a box marks it; non-boxed components need 0.12 contrast. Drawn after the map's night tint (`sharp::windows_pass`, `world_view::draw_building_windows`, an additive pass over each building picture or card, visible cells only): the glow is warm (1.0, 0.78, 0.38) × G × night × 1.6 with a slight flicker per 40-pixel cell, in candle orange (core (1.0, 0.70, 0.30), halo (0.9, 0.45, 0.15), 1.2 by the night). The tint is drawn after the world, so a glow added before it could never reach yellow at night; that is why the windows are a pass of their own. Figures never glow (they are not in the building mask). The ground's cache key does not take the lights: the ground draws none. Off the light, nothing of this runs.
**Mist** (the setting «Туман вместо черноты», `Settings::map_mist`, `mist::MistMode`: «Нет» (off, the default: the
original's black, no mist drawn), «Простой» (`Flat`), «Облака» (`Clouds`) and «Клубы» (`Billows`, the last in the
choice list: the row cycles Off → Flat → Clouds → Billows → Off); an old file's bool `true` reads as «Облака»
(`mist::de_mode`); `display::follow_settings` pushes it with `mist::set_pref`, `RAZDOR_MIST=1|flat|billows` for
snapshots). `world_view::draw_fog_flat` draws the same fog texture (`minimap::fog_layer_box`: the box of the dark
texels, the rest is alpha 0) on the same quad and texels as the black, through `src/ui/mist.rs`. Its alpha is the
shown darkness `d` (opaque where `d` is 1, unexplored; clear where it is 0), so the edge is the fog texture's own
bilinear edge, and `mist::mist_alpha` is the formula the clouds pin.
- «Простой» (`mist::draw_plain`): one pass, the colour `MIST_RGB` (0.74, 0.77, 0.82) times the frame's light, with
  the alpha `d`. No noise: the cheap look, for phones.
- «Облака» (`mist::draw_cloud`, the default look when on): one pass with one pre-baked, mipmapped, periodic noise
  texture (`bake_noise`: two octaves in red and green, 1024 texels over 64 lattice cells, quintic and blurred, so no
  texel creases) read twice per octave at the cell and at its sun-side neighbour. The masses (`UNIT` 32 cells) and
  the puffs (`PUFF` 13 cells, rotated 37° so the grids never line up) plus three slow sine waves (`bank`) make a
  density; its top (`smoothstep(0.42, 0.68)`) and sun bump shade near-white clouds (`CLOUD_RGB`), with a tiny
  dither. The drift is a whole number of lattice units per `CLOCK_PERIOD`, so the Clock wraps seamlessly. The
  tilted views draw it on the ground too; a bent view's sprite run takes the frame's light (`set_frame_tint`),
  the other views get the daylight after the world.
- «Клубы» (`mist::draw_billows`, `Billows`): the first look, back for the players who knew it: the domain-warped
  billows of 9f0eebd9 (`FRAGMENT_BILLOWS`: up to three sheets of fractal noise warped by two fields, shaded by their
  slope toward the upper-left light, times `MIST_RGB` and the frame's light). Sheets: 3 on the desktop, 2 on a phone
  (`billow_sheets`). The flat ground is one pass over the fog quad; the tilted views draw the lowest sheet on the
  ground (`mist::draw_ground`) and the raised sheets over it (`mist::raised`, `Camera::draw_raised_mist`: a quad in
  the 2.5D ground pass, a mesh lifted straight up in a bent view's sprite run, `view25::raised_ground_model`,
  `Bend::lift`, `Bend::above_horizon`). Not back: the frontier banks (the owner removed them). The fbm's lattice hash
  is read from a 64 × 64 texture (`bake_lattice`, 16 bits a cell, nearest) instead of the sine hash per octave: the
  same values, about a quarter less frame time on software GL (`docs/OPEN_QUESTIONS.md` for the sheets' answer).
- The off-map fill: with a mist look on, the fog's extended pass (`Camera::draw_fog_flat`, the branch of «Продолжать
  карту за краем») always runs: its quad goes past the map's texels (`fog_texels`), and the shaders read the
  darkness there as 1 (`darkness()`, the `Open` uniform, set when the source rectangle runs past the texture): the
  area beyond the edges is unexplored, so the mist covers it, flat and tilted, up to the visible ground. The
  black rectangles around the map are not drawn then. The minimap keeps its black. With the mist «Нет» nothing
  changes (byte-identical to main).
- The reveals under the mist (backlog 1628): a place being shown (`Showing`, `draw_showing_flat`) draws its radial mask as
  black only with the mist off. With a mist look on, `world_view::reveal_darkness` takes each open reveal's darkness per cell
  (the mask's value, `Showing::reveal_value`, the same the mask draws and the sprites' table reads), and `mist_layer` draws
  the fog texture composed with it (`minimap::fog_layer_over`: `1 − (1 − fog)(1 − reveal)`, `over_reveal`) on the flat ground,
  the tilted ground and the raised sheets alike. So the circle fades from the mist to the land and is never black; a settled
  place is the plain fog texture again. The mask's own update stays (the sprites' fade reads it). Mist «Нет» is untouched.
- The edge: past the map's edges the edge fog takes the mist's colour (`terrain::edge_fog_colour_for`), so they meet.
- Off, nothing changes: the snapshots with the mist off are byte-identical to the build before the mist's work.
- Simplifications: the F3 overlay is off in 2.5D (its labels are placed on the flat map).

**Moving water** (the setting «Живая вода», `Settings::map_live_water`, off by default; `water::set_prefs` takes
it every frame from `display::follow_settings`; `RAZDOR_LIVE_WATER=1` turns it on). Style only. Over the original's
32-frame water texture (kept, the ground pass multiplies it in as before):
- **The per-cell water texture** (`terrain::flow_texels`, baked once per map in `TerrainLayer::cells`, rebuilt only when
  the codes or the flow change): one RGBA8 texel per cell, linear, clamped. Red and green are the river's flow
  (`128 + 127 · flow`, still water exactly 128); blue is its drift speed (`water::river_speed`: 1 next to a bank,
  easing to 0.35 as the water widens by `bank_distance`, 0 on still water and on the wide water); alpha is the water mask (255 on the water
  cells, `map::is_water`, 0 on land). Linear filtering gives the shore its soft edge: 1 deep, 0.5 on the shore line,
  0 on land. The ground shader reads one texel at the pixel's cell centre `(p + 0.5) / MapSize`, where `p` is the
  cell coordinate (the same one the edge fog uses). The unlit shader (`LEGACY_FRAGMENT`) gets this texture for its
  unused live-water mask.
- **In the shader** (`NEW_FRAGMENT`, the `LiveWater` branch; the old four-fetch shader ignores it): on the water
  pixels only, a ripple moves the water frame's uv by `0.015` of a frame (a few texels), the shore foam
  (`shore_foam`, its band mirrored by `water::foam_edge`) is laid on the final colour (after the four cells are blended, so
  it sits on the visible coast): the water's share of the four cells (`cov`, the colour blend's own weights, 0.5 on the coast,
  1 deep) gives the distance `s` from the waterline. The main line is near-white (alpha up to 0.85), full for a few
  hundredths of a cell at the waterline and faded out by a third of a cell into the water; its inner edge laps between the
  waterline and 0.1 cell (`0.5 + 0.5 sin` of a 1000-cycle wave, 6.4 s, the phase varying along the shore); a noise breaks it
  into patches; a fainter second line lies at 0.36–0.41 cell on a wave of its own. Still water (`Flow` zero: a lake, the sea)
  foams at full strength, a river at 0.55. The foam is drawn after the four cells are blended, before the mist, so the
  unexplored land covers it, and it is zero on the land side of the waterline. The water takes
  the sky's colour (`daylight::sky_rgb`: day, dusk and dawn warmth, moonlight at night; the day's with the lighting
  off) by `0.18` of its mask. Only the full pass and the cache's overlay draw it (`Mode::Full`, `Mode::Overlay`);
  the cached ground stays as it was. `Clock` is the water clock.
- **The de-tiling** (`water_detiled`, `water_corner`, `cell_byte`; the frame's 3-cell tile showed as a grid when zoomed out):
  the frame is sampled at the four cell centres round the pixel (the ground's centres, whole numbers), each with its
  own cell's offset (`water::detile_corner`: a fraction of the frame from the cell's noise-table byte, 0..255) and, on
  still water only, a mirror (a mirror would reverse a river's drift). The four are blended by the smooth partition
  `water::detile_weights` (each is 1 at its own centre, so the pattern stays sharp inside a cell and fades into the
  neighbour's across the boundary, no seam). The flow and the ripple are unchanged (they sit in the pixel's uv before the
  offsets). Then a low-frequency noise (`wnoise` at 0.04 and 0.03 cell scale, drifting by whole lattice periods) moves the
  water's brightness by ±8 % and its warmth by ±5 %. Costs about ten more texture fetches per water pixel (on Mesa's software GL, about +10 to +15 % frame time on the live
  water). Off with the setting.
  The foam's patch noise (`foam_patch_at`, periods of about 12 and 27 cells) takes the same per-cell offsets and weights.
  Not de-tiled yet: the glints (`wnoise` at 6×, near zoom only; period about 11 cells).
- **The water's detail by zoom** (`water::lod_detail`, the uniform `Detail`): the map's on-screen cell over the zoom-1
  cell (`cell_px.x`, the map's PX), 1 from zoom 0.7 up, 0 at 0.45 and below (the zoom's floor is 0.4), smooth between.
  It scales the ripple and the foam's lapping (the line stands at its middle phase when 0); the texture, the colour and
  the foam stay. The sparkle and the shore waves fade with it too.
- **The water's noise** (`wnoise`, the ripple and the foam's lap and patches): one fetch of `Noise`, a wrapped 64×64
  texture of the hash lattice (`water::noise_texels`, uploaded by `terrain::noise_texture`, linear). A lookup at the
  lattice point's centre plus the smoothed fraction is the hash's value noise to within a byte (`water::wnoise_ref`,
  tested). The mist and the unlit shader keep the hash `vnoise`, so the look with the water off does not move.
- **Rivers flow** (`rules::flow::flow_field`, a best guess, pure): a water cell flows toward its neighbour with the
  smallest distance to the large water (the interior of a lake or the sea, or water on the map's border; the
  interior stands still). A piece of water with no large water in it (landlocked) flows away from its cell nearest
  a hill or mountain (classes `HILLS` to `DARK_MOUNTAINS`), or from its first cell when the map has none. The field
  is built per map (`world_view::map_flow`, kept while the surface codes and the hills stay) and drawn as the `Flow`
  texture (`terrain::flow_texels`: rg = 128 + 127 · flow, so still water is exactly zero; b = the drift speed).
  **The drift** (flow map, two phases): the picture moves along the flow at the water's speed (`water::river_speed`:
  1 next to a bank, easing to 0.35 as a river widens, 0 on the wide water: a lake or the sea, whose rim flows inward
  but stands still, `water::wide_water`). Two phases half a 20 s cycle apart (`t0 = fract(Clock · 0.05)` and
  `t0 + ½`, mixed by `|2·t0 − 1|`) move the surface 6 cells a cycle at full speed (two 3-cell tiles, so the two
  phases show the same pattern and the crossfade does not ghost), and the ripple and the foam's noise drift with
  it. The sine terms of `Clock` (the foam's lapping) keep the wrap they had (a known leftover; the mist's drift does
  too).
- **Sparkle** (`water::glint`, the uniform `Glint`, the terrain's `Bank` texture for the wide water): small points on
  the still water, drawn where a noise of the lattice texture at 6 per cell peaks (a high threshold, 0.87–0.95),
  twinkling at 2000 cycles of the clock period and drifting by whole lattice periods per wrap. Colour: white by day,
  the dawn and dusk warmth, the moon's silver at night; strength 0.5 at noon, 1 with the sun low, 0.55 at night;
  full on the wide water, 40 % on a river, faded by `Detail`. The mist and the war fog cover them.
- **Shore waves** (the same `Bank` texture: red the distance from the bank in steps / 8, green the wide water): on the
  wide water only, soft lines roll from the open water toward the land, a crest every 1.1 cells moving 1.1 cells a 4 s
  cycle (`Clock · 0.25`), fading in from 0.1–0.3 cell out of the waterline and out by 1.4 cells, so they run into the
  foam. Faded by `Detail`.
- **The shore's baked data** (once per map, `TerrainLayer::cells`): `Flow` (above), `Bank` (`water::bank_distance`,
  `wide_water`). The lake's rim flows inward (the same rule as a river), so coasts still drift toward the open water
  in the flow field; the drift speed keeps the wide water still.
- **The redraw rate** («Частота анимации воды», `Settings::map_water_rate`, `water::FrameRate`: every frame, every
  other, every third; every frame by default, the row shows with «Живая вода» on). Below every frame, the water
  (`TerrainLayer::water_picture_prepare`, called by `world_view::draw_ground` before the overlay) is drawn into a target of the screen's size when it is due
  (`water::redraw_due`, the picture's age in frames), and every frame the two last drawings are blended in time over
  the cached ground (`water::picture_mix`: the frame shows the water one interval ago, moving on smoothly, 0 at a
  redraw, 1 one interval later). The target is cleared to alpha 0 and the discarded pixels stay transparent; the
  composite is alpha-blended (`make_blended_material`: the default pipeline blends nothing). A new view, map, size or camera
  (`ground_key`, the tilt included) redraws at once and drops the older picture. The fill runs under the same 3D camera
  as the ground's cache (`render_cache::fill` around `in_view`), so the 2.5D and first-person views have the picture too. The env `RAZDOR_WATER_EVERY=1|2|3`
  overrides the setting in the snapshots.
- **The water's size** («Качество воды», `Settings::map_water_quality`, `water::Quality`: high 1, medium 0.75, low 0.5
  of the screen in each direction; high by default; the same flat-map picture as the rate, the row shows with the
  water on). The picture target is `scale` × the screen (`water::scale`, the env `RAZDOR_WATER_SCALE=<0.25..1>`
  overrides it), the overlay is drawn in it with its sampling density scaled (`draw_layer`'s `res`), and the
  composite upscales it linearly. At full size and every frame the overlay is drawn directly, as before.
- **Wakes** (`world_view::draw_wakes`, `water::Wake`): per sailing army in motion (and the hero's galley), its last
  12 display positions, one every 0.12 s, drawn on the ground behind it as soft white ellipses (the glow's texture,
  additive) fading from alpha 0.35 to 0 over 1.5 s and growing from 0.3 to 0.7 cell. Explored cells only. Recorded per
  army index and cleared with `MapView::forget_shows`.

**Weather** (the setting «Погода», `Settings::map_weather`, off by default; `display::follow_settings` pushes it
with `weather::set_pref`; `RAZDOR_WEATHER=1` turns it on for snapshots, `RAZDOR_WEATHER_DAY=<n>` fixes the day).
Style only; `src/ui/weather.rs`. Each cell's biome comes from its terrain code (`weights`: snow 14 and 15,
swamp 8 and 9, lowland 5 to 7, desert 10 and 13, fire lands 3 and 11), written as two mask textures per map
(`mask_texels`, from the terrain's cached cells: `TerrainLayer::weather_masks`, rebuilt only when the codes
change). The masks are nearest-filtered and blended by four taps in the shader (`mask4`). Each game day's spell
per biome is `spell`, a hash of the map's seed (`map_seed`), the day and the biome: `Biome::chance` (snow 60 %,
swamp rain 45 %, lowland rain 20 %, heat 80 %, ash 70 %, clouds 70 %), heavy on 40 % of those days. `Schedule::level`
blends each midnight over one game hour; `frame_levels` eases the shown levels in real time (`LEVEL_TAU`) and
forgets them when the weather is off or the map is forgotten (`forget`). `world_view::draw_weather` runs once a
frame after the flat fog and the daylight, before the clash marks and the UI: the projection maps each pixel to
its ground cell (`Projection::matrix`: a homography, flat from the camera's scale, 2.5D the ground plane's fit to
`view25::ground_homography`, four corner picks by `TiltCamera::pick`), the
fog's shown darkness masks the weather to explored ground, and `weather::draw` composes snow, rain, heat haze,
ash and cloud shadows in one fragment pass (the layers at level 0 are skipped; two particle depths on a phone,
three on the desktop, `display::dp_px`; the flakes and streaks grow with the zoom). The cell coordinate is centred on whole numbers (cells span ±½), so a mask lookup is
`(p + ½) / MapSize`, and the world's y is divided by the grid's row height first. Known: the heat haze is wavering
warm light, not a distortion (no frame readback); the bent first-person and eyes views use the plane's fit (hidden
from players); the cloud shadows darken the sprites too. The heat haze's shimmer rises over the hot ground and follows the
sun (`sun_at`: the lighting's shown hour, else the game clock; none at night). Sand gusts run in a heavy spell on an
8 s gate (on for a few seconds, then off): ochre streaks and drifting veils. Ash adds a faint dark haze.
**Living map** (the setting «Живая карта», `Settings::map_living`, off by default; `living::set_prefs` takes it
every frame from `display::follow_settings`; `RAZDOR_LIVING=1` for the snapshots). Style only, no rule reads it.
The points are the offline detector's boxes (`dt::lights`, `data/lights/objects.json`, keyed by `cards::sprite_key`,
see `tools/depth/README.md`). `dt_art::Atlas::living` gives each building sprite its chimneys and its flags (a flag's
pole on the box's edge nearer the sprite's centre). **Smoke**: `living::push_chimney` makes stateless puffs from each
chimney's top (`puff_fraction`: the puffs of one chimney spaced through `PUFF_LIFE`, 6 s; each rises 1.5 cells, grows
and fades; `wind` is shared). All of a frame's puffs are one `living::Batch` drawn over the sprites (`draw_smoke`),
each building's puffs placed by the billboard of its foot (`living::Space`, the sprite's own model); 7 a chimney on the
desktop, 4 on the phones. **Flags**: a flag box waves when its pixels look like cloth (`living::cloth_look`: at least
three quarters opaque, at least half of its opaque pixels blue or grey, as the castle's blue-and-white banners are; a
figure has skin and gold in an irregular outline and stays still). A sprite with a waving flag is drawn from a copy
with the flag's box cleared (`living::cleared`, made at atlas build; its texture is made on first use, `Living::body`),
and the flag is redrawn over it (`part_mesh`): the free end swings 3 px (`flag_sway`), the pole side stays. In the card
path a flag goes with the card whose outline holds its anchor (`owning_card`), not the nearest foot: a card pivots
about its own foot, so a flag drawn under another card's billboard would shift by (size − 1) times the feet's distance.
**Birds**: `living::flock_at`, one chance a 40 s slot (a flock 20 to 60 s after the last), 14 s trips across the visible
world rectangle, 3 to 7 birds in a V (`bird_points`); drawn in screen space after the world (`draw_birds`), each bird
skipped over unexplored cells. Not drawn in the bent (first-person) view. Off, the map is pixel-identical (checked
against the commit before the feature). The mill sails do not turn: the depth cut gives no reliable sails card
(`docs/OPEN_QUESTIONS.md`).
- **Drawing cost** (the speed-up pass, 2026-11-12; the picture is the same). `TileMap::objects_in_rect` reads the
  objects of a column and row rectangle from 16 x 16-cell chunks built with the map, so the world pass visits only the
  drawn chunks, not every object. `draw_world_in` reads the screen's DPI, the map animation's time and level and the form
  sun once into a `FrameCtx`, which `draw_object` and `draw_picture_form` take. A hill's flow key is the map's generation,
  not a hash of every hill each frame. `Fog` has a change counter (`mark`, the only writer of the bits,
  renews it; serde skips it and equality ignores it) that the battle backdrop, the minimap and the shadow map key on, in
  place of hashing the explored set. The sharp draw refills one thread-local mesh per use (`SPRITE_MESH`, `SCALED_MESH`,
  `sharp.rs`) and computes a rotation's sin and cos once per sprite.

- **Atmosphere cost** (2026-11-12, `docs/perf/2026-11-12-atmosphere-performance.md`). Each location's fog test
  (`location_explored`: a cell of its footprint explored near the feather) is kept per location until the fog's
  change key moves. The locations' passes (the sprites, the windows of the lit buildings, the point lights) used to
  scan the fog around every cell each frame; the answers do not change between cells revealed.

## Sound and music

- `src/dt/sound.rs`: `SoundTable::from_ini` (`_Sounds.ini`: `[Backgrounds]` music keys,
  `[SFX-Effects]`), `parse_wav`, `.raw` music as 16-bit PCM at an assumed 22050 Hz
  (`RAZDOR_MUSIC_RATE` overrides; `raw_to_wav`).
- `read_sound`: a `Sounds/<stem>.ogg` (Ogg Vorbis, `parse_ogg`, Razdor's own addition) with the
  same stem as the named `.raw`/`.wav` is played in its place, so a mod's re-encoded sounds win
  over the base install's.
- `src/ui/audio.rs`: screens call `audio::cue(Cue::…)` (a per-frame queue, de-duplicated);
  `Audio::frame` plays them and drives the music. The effects are read when the install is known
  (`Audio::new` at start, `Audio::set_install` when a game is downloaded, imported or a mod is switched:
  `needs_load` decides, the same folder keeps its sounds). `audio::cue_on_release` repeats a cue on
  the mouse release, as the original's hire button does.
- `src/ui/jukebox.rs`: which track plays for a `Mood` (menu, map rotation, battle,
  triumph, defeat), crossfades (`Fade`, `SCREEN_FADE`, `ROTATION_FADE`). The map
  rotation's picks are draws of the game's generator (`rules::music`, `App::rotate_music`),
  so they matter for parity with the original's random state.
- A music file that fails to read is handled by `apply` in `src/ui/audio.rs`: an I/O error
  (`transient`: the storage can be briefly unavailable after the app sat in the background)
  is retried after a wait of 5 s doubling to 60 s (`Jukebox::retry_later`, Razdor's own);
  a malformed file or one that cannot be played is dropped for the run (`Jukebox::failed`).
- `RAZDOR_NO_AUDIO=1` silences; `RAZDOR_AUDIO_LOG=1` logs every sound.
- The output (`src/ui/audio/output.rs`): macroquad's everywhere but Windows. On Windows macroquad's
  audio context is private and quad-snd opens the default device once, when its context is made,
  so Razdor opens its own quad-snd context there (`output::open`, the WASAPI backend of the
  `quad-snd` crate). Sounds belong to the context they were loaded in: a `Sound` is deleted from
  its own output only, so a sound that outlived its output does nothing.
- Following the default output (Windows; `src/ui/audio/follow.rs`): a watcher thread reads the
  default render endpoint's id (MMDevice `GetDefaultAudioEndpoint`, `GetId`) every 2 s
  (`output::current`). `Audio::frame` calls `follow_output` first: `Follow::wants` says yes once
  for each changed id (never for the same id again). Then the backend is retired (its sounds
  stopped and deleted, `Backend::retire`), the output reopened (`output::open`) and the sounds
  reloaded (`Backend::load_dir`). The music starts again from its beginning; the settings stay.
  A failed move waits 2 s, then 4 s, and gives up after 3 tries for that id (a new default
  restarts the count). Known limits: quad-snd has no way to stop its old output thread, so each
  switch leaves one idle thread that plays nothing; the watcher cannot see a stream error inside
  quad-snd, so a stream that dies without a default change is not re-opened. Not tested on a
  Windows machine with two outputs (the logic is unit-tested in `follow::tests`).

## Fonts

All text goes through `widgets::measure` / `widgets::text` in the current face
(`dt_font::with_face`: `Face::Body`, `Bold`, `Title`, `Subtitle`) and the font mode
(`dt_font::FontMode`, the settings' «Шрифт», set every frame by `display::follow_settings`;
`RAZDOR_FONT_MODE` overrides it for snapshots):

- `Original` (default): `dt_font::current_ttf` is `None`, so `dt_font::with_current` draws the
  install's sheet (`Face::file`: `SanSerif.lit`, `SanSerif_Bold.lit`, `Benguiat.lit`) as a
  `BitmapFont`: nearest filtering, a whole-number scale (`BitmapFont::scale`, the nearest
  integer to matching the TrueType capital height, at least 1), pen and cell tops on whole
  pixels. `chrome::shadow_text` draws the face's shadow sheet (`Face::shadow_file`) 1 sheet
  pixel down-right first (`dt_font::draw_shadow`). Glyphs a sheet lacks come from PT Sans
  (`widgets::measure_ttf`).
- `Modern`: `widgets::face_font` gives the bundled TrueType face (`MODE_FILES`:
  PT Serif, Becker, Kurale); `MODE_MISSING` swaps the few
  characters a face lacks for look-alikes.
- `RAZDOR_FONT*` files win in every mode.
- Any script (Razdor's own, for maps translated by `razdor-translate`): `widgets::measure_ttf`
  and `text_ttf` ask `script_font::runs` for the text's runs by font. `None` (the current
  TrueType font has every character and none is of a complex script, `script_font::complex`)
  is the fast path: drawn by macroquad as before. Otherwise each run is the current font
  (`Pick::Primary`, macroquad) or a font shaped by rustybuzz and rasterised by glyph index
  into cached textures (`Pick::Fallback`, `script_font::draw` / `measure`, same baseline and
  macroquad's display-scale rounding). The chain (`Chain::new`, `fallback_for`): PT Sans,
  Noto Sans (subset: Latin extended, Greek, Vietnamese), Noto Sans Devanagari, then system
  fonts found once per missing character (`system_candidates`: `fc-match -s :charset=…` on
  Linux, script fonts of `C:\Windows\Fonts` and `/System/Library/Fonts`); CJK is never
  bundled. A complex script's run (Devanagari, Arabic…) is shaped even in the current font
  (Kurale has Devanagari). Spaces and marks stay in the run before them (`sticky`), so a
  Hindi sentence is shaped whole. A font's empty or wrong glyphs do not count
  (`has_glyph`; `widgets::wrong_glyphs`: Becker's Latin-1 letters are Cyrillic). In the
  original's mode `BitmapFont::pieces` hands the sheet's missing characters to the fallback
  as whole runs, not one by one. The `fonts` snapshot scene (`snapshot::draw_overlay`) shows
  samples in many scripts in each face with their measured widths underlined.

Traps: never call `widgets::measure`/`text` from inside a `with_current` closure (it holds the
sheet cache's borrow; use `measure_ttf`). Sheet widths change with the mode, so lay text out
by `measure`, not by fixed pixel widths. Inside a frame (`dt_font::with_native_metrics`, the
whole content) the original mode ignores the size asked for and draws the face's own sheet
size, so a Razdor word standing in for one of the original's word pictures (the title menu's
words in English, `main_menu::word`) goes through `chrome::engraved_centered`: Becker at the
picture's capital height in every mode, with the picture's dark rim.

## What a mod can replace

The original finds a unit's and an ability's pictures as follows, and Razdor does the same
(issue #6):

- **Engravings and busts.** `Objects/Persones.ugs` (sepia full figure) and `Objects/Icons.ugs`
  (92×92 bust) hold one frame per unit section of `Rus_Units.ini` *in file order*: a section
  counts when its `Name`, `StartExpirience` and `Cost` are non-empty (0x4e0448). The k-th such
  section gets frame k, whatever its `GlobalIndex`; the loader stores it at that id
  (`data::unit_picture_frames`, `DtInstall::unit_picture_frame`). A mod that adds units must
  add their frames at the same positions; frames are `[u16 w][u16 h][u16 w][u16 h]` then
  w·h scrambled ARGB4444 pixels (Persones) and `[u16 w][u16 h]` + 92·92 pixels (Icons).
  The unit panel draws the engraving unscaled (at the window's 15/16), the whole picture
  centred in the original's 266 × 436 box under the panel's top (`unit_sheet::figure_rect`,
  0x492f24); never place it by its pixels or pin it to the top.
- **Engraving blend.** Drawn with the frame's own alpha at full strength (492f24 → 0x476a3c);
  a frame whose opaque pixels carry a low alpha nibble looks faded in the original too.
- **Ability icons.** `Windows/Bonus<N>.lit` for N = 1…52, N being the bonus's place among the
  52 names the unit loader knows (0x48efd0); a missing file is a blank 25×25 picture in the
  original (0x4dca8f–0x4dcb13). New bonus *names* are not possible: an unknown `Bonus=` is no
  bonus. A mod changes an icon by replacing the `Bonus<N>.lit` of an existing bonus, as a
  LIT or as a BMP under that name (0x48cd90 picks the loader by the first byte). The icons
  are loaded without their alpha (0x48d02c) and drawn with the colour of their bottom-left
  pixel transparent, compared at 16 bits (0x48e8f0 → 0x4764c0, 0x47669c), so a mod's white
  background goes as the shipped violet does (`chrome::trait_icon`, `Fx::KeyCorner`).

## How to add a picture

1. Find the file and how the original draws it (Ghidra / `docs/reference/graphics-formats.md`;
   `parity-check` skill): which file, frame, blend, alpha.
2. Load it through `chrome::win` / `chrome::art` (window art) or a `DtArt` method (unit,
   item, map art). Do not add a new cache.
3. Every picture under `Graphics/` is queued at start, so a new picture needs no list entry for
   its decode. Add it to `WINDOWS_AHEAD` only when its texture must be up before the menus, or to
   the upload list of `chrome::warm` when its first open must find the texture made.
4. Check it offscreen (`RAZDOR_SCENE` + `RAZDOR_SNAPSHOT`, see
   [testing-and-tools.md](testing-and-tools.md)); a decoder change gets a unit test on
   synthetic bytes plus a real-file test gated on `RAZDOR_DT_DIR`.

## Traps

- Never commit decoded output, screenshots of original art, or install files.
- The Python reference decoder in the local notes still has the two LIT bugs; trust
  `src/dt/gfx.rs` and its tests.
- Alpha is not premultiplied in `gfx::Image`; use `chrome::premultiplied` when a texture is.

<!-- refs verified by: python3 -I docs/flows/verify.py docs/flows/*.md -->
