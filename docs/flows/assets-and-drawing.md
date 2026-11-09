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
  as `minimap::fog_texture`, the same texture the map view draws, so a step of the hero only
  remakes the dark.
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
(`minimap::FOG_RGB`, black, read by `terrain::edge_fog_colour`): thin, patchy wisps, darker and
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
  `standing` makes each sprite a billboard through its foot (`view25::billboard_model`). With the
  setting off, `in_view`, `ground` and `standing` only call their closures: the flat view is the same
  code path.
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
- Simplifications: the F3 overlay is off in 2.5D (its labels are placed on the flat map).

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
