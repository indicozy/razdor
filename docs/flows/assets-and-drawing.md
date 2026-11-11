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
The minimap is `src/ui/minimap.rs`.

The small trees and bushes (object class 9) sway: `draw_object` draws them through
`draw_leaning`, the top corners moved by `rng::tree_sway` (a triangle wave of ±2.5 px over
3.57 s, 0x4c99d0) of the plant's phase, the third draw of its jitter (`rng::plant_jitter`).

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
  drawing culls to (`cull`). The drawing runs in the flat camera's 2D space, `in_view` sets the 3D
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
  houses floating in the air). `data/cards/objects.json` (numbers only, made offline by `tools/depth`) keys
  each sprite by its pixel hash (`dt::cards::sprite_key`, FNV-1a 64); `cards::builtin` parses it once and
  `DtArt::map_atlas` keeps the cards of each building sprite that has an entry of the same size
  (`Atlas::building_cards`). In the tilted view only, a building with cards is drawn as one `Drawable::BuildingCard`
  per card, each on its own foot (`card_foot`, sorted like a sprite) and standing as a billboard, its mesh
  (`card_mesh`, the sprite's uvs in the atlas, `sharp` sampling). Its shadow lies on the ground under the fog
  (`draw_building_shadow`, `chrome::shadow_only`: the sprite's shadow pixels only, with the relief's `ShadowK`). A bridge, a picture with no
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
  Each standing sprite (`standing`, its visibility from `Shade::at`) fades into the fog on its foot's cell, so
  what is behind it shows through instead of a black silhouette: the visibility is `sprite_visibility`, a smoothstep
  from 0.15 to 0.85 of 1 less the darkness the ground shows on that cell: the fog's shown darkness there
  (`minimap::shown_darkness`) composed with the reveal's alpha there (`Showing::alpha_at`), as the GPU blends them.
  Both are the values this frame's ground was drawn with (the fog and the mask are drawn before the sprites), so an
  object opens as the reveal's circle passes it. A straight-alpha sprite keeps its
  colour and scales its alpha (`faded`); a premultiplied one (the figures) scales every channel (`faded_premultiplied`).
  At 0 it is not drawn. Off the map the ground is black (visibility 0) unless «Продолжать карту за краем» is on.
  Razdor's visual only: the flat view is pixel-identical. Sprites' fallback drawings (no DT art: procedural
  ships, units by `draw_unit`) are not faded.
- **The 2.5D fog's box**: `draw_fog_flat` on a tilted camera draws `minimap::draw_fog_boxed`, only the sub-quad over the
  texels that are not fully transparent (the box grown by one texel a side; nothing when none is dark). The same
  pixels as the whole quad, fewer fragments. The flat map view and the minimap keep `minimap::draw_fog`.
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
  rebuilds). The flat map, the minimap window and the 2.5D sprites all read the same values.
- **The first-person view** («Вид карты» = «От первого лица», the owner's decision, 2026-11-10). Its
  setting is `audio::Settings::map_first_person` within the tilted view (`map_tilt`), pushed by
  `main_menu::apply_options` into `world_view::set_first_person`; `world_view::first_person()` is the
  view, `tilt_view()` stays true for both tilted views. The far ground's shape «Даль» is
  `Settings::map_horizon` and `map_globe` (`Row::FarGround`, shown only with the view): `map_globe` on «Шар» (the
  ground curves down in every direction from the hero, like a small planet: a circular horizon), else `map_horizon`
  on «Горизонт» (it curves down like a planet, the sky above), else off «Вверх» (the far ground rolls up to
  top-down). `Settings::far_ground` gives the shape as a `world_view::FarGround`; the row cycles Up, Horizon, Globe.
  It is visual only: no rule reads it.
  - **In the settings** (the owner's decision, 2026-11-10: shown to players again): «Вид карты» cycles flat, 2.5D
    and «От первого лица», and with the view its «Даль» and «Камера» rows show. Developer overrides in
    `RAZDOR_VIEW`: `fp-chase` forces the view behind the hero (the saved camera choice does not count), `fp-eyes`
    forces the eyes camera; `first` and `eyes` force them as in the snapshots. `RAZDOR_VIEW_HORIZON=down|up|globe` (`1` is
    `down`) overrides «Даль»; `RAZDOR_VIEW_GLOBE_RADIUS=<cells>` tunes the globe's radius for the snapshots.
  - The camera: `view25::TiltCamera::first_person(look, scale, frame, yaw, far)` (`far` is the `FarGround`). Pitch `FP_PITCH` (12),
    field of view `FP_FOV` (55), fixed (PageUp and PageDown tilt the 2.5D view only). It looks at the view's
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
    The camera is `view25::TiltCamera::eyes`: the eye `EYE_HEIGHT` (0.7 cell) above the ground under `target`
    (`eye_at`), looking `EYE_PITCH` (2°) down along the yaw, with no bend of its own; `eye_bend` adds the sheet
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
warmth, a night floor). The hour comes from the game clock (eased, `ease_hour`), a real-time cycle, or
a fixed hour; `RAZDOR_LIGHT_HOUR` overrides it for snapshots. `world_view::draw_daylight` runs right after
`draw_world` and before the flat `draw_fog` (after it with the mist on, see «Mist»): a multiply tint over the map area, then the point lights' pools
(«Point lights» below) in screen space (`Camera::screen_at`). Off, it draws nothing. The frame's
light is read once, before the world (`world_view::frame_light_of`), because the relief below needs it there too.
**Relief** (part of «Освещение», no setting of its own; off with the light). `daylight::relief_at(hour)` gives a
`Relief`: the light's direction on the screen (x right, y down, z toward the viewer; from the lower right, so it agrees
with the painted shadows, and the elevation follows the hour), the raking strength (0.65 at dawn, 0.25 at noon), the
cavities' darkening (0.35 by day, 0.2 at night) and the painted shadows' factor (`ShadowK`: 1.15 at low sun, 1.0 at
noon, 0.55 at night). By night the moon (elevation 0.6, strength 0.3) lights it; the blend runs over the sun's sine from
-0.5 to 0, so it does not jump at dawn or dusk. `daylight::frame_relief` reads it for the hour `frame_light` just set.
No normal maps: each texel's height is its luminance, the slope comes from the four neighbours at ±1.5 texels
(`RELIEF_R`) and the cavity from the four at ±4.5 texels (`sharp::RELIEF_GLSL`, whose relief_shade function does the shade). A pixel's shade is
`mix(1, dot(N, L) / L.z, Strength) · (1 − Cavity · c)`, clamped; flat ground is exactly 1. The sprites: `draw_world_in`
runs its pass in `sharp::session_lit` (the lit material, its uniforms set once a frame, so the pass batches as the
plain one does; `None` is `sharp::session`). Every sprite of the pass is lit, and a transparent neighbour counts as the
pixel's own height, so a sprite's edge has no rim. The painted shadows (alpha 0 < a ≤ 0.76) have their alpha times
`ShadowK` there and in `chrome::shadow_only`. The ground: the land codes only, never the water (`ground()` in
`NEW_FRAGMENT`); `Strength` 0 skips the maths, so the lighting off draws the old pixels. In 2.5D the ground's light is
turned by `Relief::turned(yaw)` (the sun is fixed to the camera, the ground's axes turn with it). The ground cache's
key holds the relief to 1/256, so a cycle refills the cache a few times a game hour. Not lit: the battle and window
backdrops (`draw_world(…, None)`), the fog, the marks and the route. Known limits: an atlas sprite's neighbour can be
a neighbouring sprite's edge copy (the atlas gutter is one clamped pixel), so an outermost pixel may pick up that edge;
the 2.5D ground is lit without the pitch's foreshortening.
**Cast shadows** (the setting «Тени», `Settings::map_shadows`, `daylight::ShadowMode`, under «Освещение»;
«Нарисованные» is the default and keeps the picture as above). «От солнца» (`ShadowMode::Sun`, `RAZDOR_SHADOWS=sun`
for snapshots) removes the painted shadows (`ShadowK` 0 in the lit material) and casts real-time silhouettes instead:
`daylight::casts_at(hour)` gives the sun's shadow (its bearing away from the sun, east at 6:00, south at noon, west at
18:00; length `1/tan(elevation)` on a stylised shadow elevation (`shadow_elevation`: 12° at dawn and dusk, 40° at noon, so the dawn and dusk shadows hit the 2.6 cap; the relief's light uses the same elevation), clamped to 0.25–2.6; alpha 0.6 by day, fading through the dusk)
and the moon's (fixed in the south-west, alpha 0.15 from 20:00 to dawn). `daylight::sun_relief` lights the map in the
world's axes (the ground keeps them; `world_view::lit_axes` turns the sprites' light back into the screen's). In
`world_view::draw_world_in`, before the hills, `draw_shadows` draws each caster in view on the ground (the point lights' silhouettes too, «Point lights»): a building
with cards casts one quad per card from the card's own foot, anything else one picture from its foot. `shadow::Projection::point`
maps a picture pixel to the ground (its x stays, it moves along the bearing by its height above the foot times the length);
the quad's texture is the picture's blurred silhouette in the shadow atlas (`DtArt::shadow_texture`, built on the first
sun frame: the map atlas's layout, RGBA, the silhouette in alpha, since macroquad's public API has no R8). Each quad is the cool
colour `shadow::TINT` with the alpha `light · fog visibility of the foot's cell · (½ on water)`, drawn with the default pipeline (the texture's colour is white, the vertex colour carries the tint), drawn with the ground's fog and the standing
sprites over it. Figures keep their painted shadows. Lighting off or Painted: nothing of this runs.

  *Penumbra and tone* (this round): each caster is drawn twice, the sharp silhouette (`DtArt::shadow_texture`) weighted by 1 − t and a soft copy (`DtArt::shadow_soft_texture`, three [1 2 1] passes, built with the sharp one) weighted by t, where t is the height above the foot as a share of the picture (`shadow::Projection::picture_mesh_ramp`, `card_mesh_ramp`: per-vertex colour): crisp at the base, softer at the tip. The colour is `shadow::TINT` (0.42, 0.40, 0.45, warmer than the old blue-grey) with the alpha eased by `shadow::EASE` (0.85). *Self-shading* (this round): in Sun mode by day, a picture's quad (`world_view::draw_picture_form`, cards in `draw_card` via `card_mesh_form`) carries per-corner colours from `form_colour`: the face turned away from the sun (`daylight::form_sun`, the screen direction of the sun by `cam.facing`) darkens up to 40 % (low sun) to 20 % (noon); a shaded object shades itself, not the objects behind it. The ground shadow keeps its alpha (`shadow::EASE` 1.0) and is drawn by a multiply pass (`shadow::draw_multiply`: dst × mix(1, tone, strength × silhouette), tone (0.48, 0.50, 0.62), no alpha-over; the sun's strength is 0.9 by day, `daylight::SUN_ALPHA`), so grass in shadow stays green. Not done: a stronger relief on building faces in Sun mode (the day lit material has no building mask). The day's lit material is the base's own text (`sharp::DAY_LIT_MAIN`): by day, with no night, the pass and the terrain draw exactly as before the lights (`sharp::session_lit`, `world_view`'s point gates); only the night uses the lit material with the point lights and the building mask.
**Point lights** (the setting «Качество света», `Settings::map_light_quality`, `lights::Quality`: high by default on the desktop, medium on a phone (`display::follow_settings`); a row under the lighting rows, shown with the light on; `RAZDOR_LIGHT_QUALITY=high|medium|low` for snapshots). `world_view::point_lights` builds the frame's lights each map frame (`lights::set_frame`, read by `lights::with_frame`): one per non-bridge building explored near the feather, at its footprint's centre, its reach its sprite's size (`BUILDING_RADIUS`); a torch in the hand of the hero (not in the eyes view, not aboard) and of each army in view (not a ship), its reach `TORCH_RADIUS` 1.6 cells, at `lights::torch_offset` (0.4 of the figure's height from the head, along its heading, 60° above horizontal; the heading is the sheet's row, `cam.facing`). The strength is the night's factor (`daylight::Light::glow`). `lights::Frame` sorts them nearest the view's centre; the nearest `quality.lit()` (8 high, 4 medium, none low) cast shadows, light the relief and glow inside the building picture; `quality.relief()` (8, 2, 0) rake the relief. **Pools**: a building's light is a warm disc of twice its reach on the ground (`draw_building_pools`, in `draw_world_in`'s ground pass, before the sprites, so the building's own picture covers it: the light lies around the building and on what stands near it, never on the building itself); a torch's pool (`draw_daylight`, after the world) flickers; both fade with the fog on their cell. The building's own light also does not shade its picture (`sharp`: no point relief on the building mask) (the ground's cache key, the relief and the shader's uniforms do not read the flicker). **Shadows** (`draw_shadows`): for each lit light, every caster in view within its reach lays its silhouette (`lights::cast`: away from the light, longer and darker nearer it, `CAST_ALPHA` 0.8 times the falloff; a light's own building casts nothing), under the fog, with the sun's; they show at dusk and night. **Relief** (`sharp::POINT_GLSL`, in the lit sprite material and in `terrain`'s neighbours shader): each lit light's direction from the pixel (its height half its reach) against the relief's normal, positive only (the faces turned away keep their colour), at the night's factor; `sharp::session_lit` takes `lights::uniforms` once per pass (`LightA`, 8 vec4 slots; `LightI`: the count, the relief's count, the night; the fourth is unused), and the terrain's `Dest` gives its quad's place in the pass's pixels. **Lit windows** (`dt::windows`, the building mask's G; `ui::dt_art::DtArt::building_mask`, built on the first lit frame): per building picture, the enclosed dark openings are found once from the picture's own pixels: candidates are opaque pixels darker than the median by 0.10; a 4-connected component is a window if it is 3–80 px, its bounding box is 0.25–4 in aspect with at least 45 % fill, and every pixel of its outer ring is opaque and at least 70 % of it is brighter than the component by 0.08; a door (taller than wide) may be 200 px and have ground under its bottom ring row. A component inside a window or door box of `data/lights/objects.json` (the detector's, by the sprite's pixel key) is kept with looser rules (2 px, half the ring). Kept openings are G 255, the ring around them G 90 (the halo), then one [1 2 1] blur pass clipped to the body feathers the edges (×1.25); the mask is sampled linearly, so G interpolates (R, the building flag, is tested at 0.5). Roofs: a component in the top 45 % of the body, brighter than 0.18, whose colour is the roof's dominant hue (30° buckets, saturation ≥ 0.15) is rejected unless a box marks it; non-boxed components need 0.12 contrast. Drawn after the map's night tint (`sharp::windows_pass`, `world_view::draw_building_windows`, an additive pass over each building picture or card, visible cells only): the glow is warm (1.0, 0.78, 0.38) × G × night × 1.6 with a slight flicker per 40-pixel cell, in candle orange (core (1.0, 0.70, 0.30), halo (0.9, 0.45, 0.15), 1.2 by the night). The tint is drawn after the world, so a glow added before it could never reach yellow at night; that is why the windows are a pass of their own. Figures never glow (they are not in the building mask), and the lit material's building test reads R. The ground's cache key takes the lit lights' places (to 1/16 cell), reaches and strengths. Figures never glow or take the relief (they draw in the premultiplied blend, not the lit material). Off the light, nothing of this runs.
**Mist** (the setting «Туман вместо черноты», `Settings::map_mist`, off by default; `display::follow_settings`
pushes it with `mist::set_pref`, `RAZDOR_MIST=1` for snapshots): `world_view::draw_fog_flat` draws the same fog
texture (`minimap::fog_layer`, the same quad and texels) through the material of `src/ui/mist.rs` instead of the
plain black. Its alpha is `mist::mist_alpha(d, w)`: opaque where the shown darkness `d` is 1 (unexplored), clear
where it is 0, and only in the opening's edge band the value noise `w` makes it patchy. Its colour is `MIST_RGB`
(0.74, 0.77, 0.82) times the billows' shade (0.65 to 1.15) and a faint cool-to-warm tint. Four sheets of domain-warped fractal noise ("billows": `warp` shifts each sheet by two fbm fields that evolve on the Clock, the warped fbm is the height; 4 octaves, 2 for the warp; 3 sheets on the desktop, 2 on a phone, `Sheets`, `RAZDOR_MIST_SHEETS=1..4` overrides) at scales 0.18 to 1.1 drift at different speeds on the terrain's clock (`Clock`; every speed is a whole number of lattice periods per `CLOCK_PERIOD`, so the loop is exact, no crossfade) and each billow is shaded by its slope toward a light from the upper left (0.65 to 1.15 of the mist colour). The wisp at the opening's edge is sheet 0's warped height. The sheets sit at
heights 0.3 to 1.2 in the flat view (`Parallax`, zero there). The tilted views (2.5D and first-person) draw sheet 0 alone on
the ground (`mist::draw_ground`, `Sheets` 1, no parallax: opaque where unexplored, as above) and the others as **raised sheets**
(`world_view::draw_raised_mist`, `mist::raised`, `Sheet` = 1..3 in the shader): translucent copies of the same quad at
`mist::raised_height` 0.35, 0.75 and 1.2 cells (`RAZDOR_MIST_RAISED`, default 3 on the desktop, 2 on a phone). Their alpha is
0.42 × the darkness, less the higher they stand, and only the billows above the mean show (`smoothstep(0.44, 0.72)`); a
raised sheet's rim fades over the last two texels of the fog texture, so it does not end in a hard edge past the map. In 2.5D a
raised sheet is the ground's model lifted straight up (`view25::raised_ground_model`); in the bent view it is a mesh of its own on
straight up from the sheet (`bent_mist_meshes`, `view25::Bend::lift`, the same lift as the flat sheet; lifted and faded out
over the last three cells before the horizon, alpha 0 past it). Along the sheet's normal it was wrong: on a steep part of the
roll the far mist's texels moved toward the eye and covered the explored ground there. The raised sheets are drawn over the ground's mist and under the reveals, as the mist is.
- **Bent views and explored land** (the owner's report: the first-person view covered the explored land). In a bent view the
  weather draws in the ground's picture (`draw_world_in`, under the sprites) with the flat projection of the sheet's own ground:
  the screen-level homography is the flat plane's, which misplaces it on the roll. A bank that would stand in front of explored
  ground (`bank_hides_explored`: the land 1 and 2 cells ahead along the view is explored) is left out in a bent view only.
  The flat and 2.5D views are byte-identical to before.
- **Banks at the contour** (2.5D and first-person): `mist::contour` finds where the shown darkness crosses `mist::FRONTIER`
  (0.5) between two 4-neighbour cells (marching squares on the cell centres: `Crossing`, its fraction `t`, which side is dark).
  `world_view::push_banks` stands two banks a crossing, 0.8 cell (`BANK_OUT`) past the contour into the unexplored side, at
  ±0.28 cell along it, so their soft sides overlap into one wall. Position, width (1.7 × 0.85..1.25) and height (1.5 × 0.8..1.25)
  vary with `unit_hash` of the crossing's cells, so the wall does not move or step as the camera does. The banks are nearest-first
  up to `mist::banks_cap` (600 on the desktop, 240 on a phone; `RAZDOR_MIST_BANKS` overrides), sorted like standing sprites on the
  dark cell (`sprite_key`, `LAYER_BANK`) and cut into `BANK_BANDS` (4) runs: each run is one `Drawable::BankBand`, drawn at the
  place of its farthest bank. `draw_banks` builds one batch per run: each bank's quad through its billboard (`standing_model`,
  applied on the CPU to the corners) into one vertex buffer, under the bank material (`mist::banks`, one draw, then
  `sharp::leave_blend`), with no per-bank material or uniform change. A vertex's colour carries the bank's darkness (red) and
  seed (green). The contour is computed over the cull plus `MARGIN` (4 cells): cut at the cull's edge, the wall had gaps.
  The bank's colour and shade are the ground sheets' (`MistRgb` × 0.65..1.15), and its foot fades in over the lower 30 % (the
  shader's `top` factor), so it rises out of the mist beneath it, with the raised sheets over its lower half. The flat view is
  unchanged (pixel-identical with the mist on and off; the 2.5D view with it off too). The colour is the
  fork's `color0` attribute: raw bytes, so the shader divides it by 255 (the default shader does too); an undivided value clamps
  every fragment to opaque.
Known: a reveal's animation in the first-person view draws its raised sheets over the circle (the picture holds the reveal); in the
down horizon the far raised mist shows above the horizon as a soft band (the unexplored land is there, beyond the bend).
The black rectangles past the map stay black; the minimap keeps its black (`minimap::draw_fog` is not used
for it). Past the map's edges the edge fog takes the mist's colour (`terrain::edge_fog_colour_for`), so they meet.
With the mist on, the flat view draws `draw_daylight` after the flat fog, so the mist takes the night's tint; off,
the order is as above. The ground cache (`ground_key`) and the battle backdrop key hold the setting.
- Simplifications: the F3 overlay is off in 2.5D (its labels are placed on the flat map).

**Moving water** (the setting «Живая вода», `Settings::map_live_water`, off by default; `water::set_prefs` takes
it every frame from `display::follow_settings`; `RAZDOR_LIVE_WATER=1` turns it on). Style only. Over the original's
32-frame water texture (kept, the ground pass multiplies it in as before):
- **The water mask**: `terrain::water_mask_texels`, one texel per cell (red 255 on the water cells, `map::is_water`),
  built with the surface-code textures (`TerrainLayer::cells`, rebuilt only when the codes change), linear, clamped.
  Linear filtering gives the shore its soft edge: 1 deep, 0.5 on the shore line, 0 on land. The ground shader reads
  it at the pixel's cell centre `(p + 0.5) / MapSize`, where `p` is the cell coordinate (the same one the edge fog uses).
- **In the shader** (`NEW_FRAGMENT`, the `LiveWater` branch; the old four-fetch shader ignores it): on the water
  pixels only, a ripple moves the water frame's uv by `0.015` of a frame (a few texels), the foam band
  (`water::foam_band`, mirrored from the shader) is added as a pulse on the shore's water side, and the water takes
  the sky's colour (`daylight::sky_rgb`: day, dusk and dawn warmth, moonlight at night; the day's with the lighting
  off) by `0.18` of its mask. Only the full pass and the cache's overlay draw it (`Mode::Full`, `Mode::Overlay`);
  the cached ground stays as it was. `Clock` is the water clock.
- **Rivers flow** (`rules::flow::flow_field`, a best guess, pure): a water cell flows toward its neighbour with the
  smallest distance to the large water (the interior of a lake or the sea, or water on the map's border; the
  interior stands still). A piece of water with no large water in it (landlocked) flows away from its cell nearest
  a hill or mountain (classes `HILLS` to `DARK_MOUNTAINS`), or from its first cell when the map has none. The field
  is built per map (`world_view::map_flow`, kept while the surface codes and the hills stay) and drawn as the `Flow`
  texture (`terrain::flow_texels`: rg = 128 + 127 · flow, so still water is exactly zero). The shader scrolls the
  ripple and foam noise along the flow on the water (two phases of 7 cells a 20 s cycle, `t0 = fract(Clock · 0.05)`
  and `t0 + ½`, mixed by `|2·t0 − 1|`, so the scroll does not jump when `Clock` wraps at 1000 s). The ripple's and the
  foam's sine terms of `Clock` itself still jump at that wrap (a known leftover; the mist's drift does too). A lake's rim cells flow inward (the same rule), so
  coasts drift toward the open water; that is part of the best guess.
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
## Sound and music

- `src/dt/sound.rs`: `SoundTable::from_ini` (`_Sounds.ini`: `[Backgrounds]` music keys,
  `[SFX-Effects]`), `parse_wav`, `.raw` music as 16-bit PCM at an assumed 22050 Hz
  (`RAZDOR_MUSIC_RATE` overrides; `raw_to_wav`).
- `read_sound`: a `Sounds/<stem>.ogg` (Ogg Vorbis, `parse_ogg`, Razdor's own addition) with the
  same stem as the named `.raw`/`.wav` is played in its place, so a mod's re-encoded sounds win
  over the base install's.
- `src/ui/audio.rs`: screens call `audio::cue(Cue::…)` (a per-frame queue, de-duplicated);
  `Audio::frame` plays them and drives the music. `audio::cue_on_release` repeats a cue on
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
