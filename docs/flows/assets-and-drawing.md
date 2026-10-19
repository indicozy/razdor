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
- Decoding ahead (commit c04fa1b): `chrome::set_install` calls `gfx::prefetch` with
  `WINDOWS_AHEAD` (the paper, portraits and icon sheets, the load window's book, building
  tabs) on a thread named "art ahead". `gfx::decode_file` takes a picture decoded ahead
  from the `Ahead` map; if the game asks first it decodes itself and marks it `wanted`
  so the thread skips it. Slow decodes are logged ("slow load") with `RAZDOR_PROFILE=1`.
  Add a file to `WINDOWS_AHEAD` when profiling shows a window's first open decoding it.
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

`src/ui/world_view.rs`: `draw_world` draws only the visible cells — `draw_terrain`
(`TerrainLayer::draw` in `src/ui/terrain.rs`: one shader quad mixing the four nearest cell
textures from a 4×4 atlas, water frames multiplied in), then decorations
(`draw_object`) and buildings (`draw_building`) in one pass over the cells as the original's
(0x4c8864, 0x4c9b5b: row by row, left to right; in a cell its massif object, its plant, then
the building whose footprint covers it if not drawn yet, so a building comes at its top row's
left cell and every later object covers it), armies (`draw_army`, `draw_figure`,
`draw_ship`), the hero (`draw_hero`), the route (`draw_route`), and the tooltip
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

## Sound and music

- `src/dt/sound.rs`: `SoundTable::from_ini` (`_Sounds.ini`: `[Backgrounds]` music keys,
  `[SFX-Effects]`), `parse_wav`, `.raw` music as 16-bit PCM at an assumed 22050 Hz
  (`RAZDOR_MUSIC_RATE` overrides; `raw_to_wav`).
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
3. If it is big and a window needs it on first open, add it to `WINDOWS_AHEAD`.
4. Check it offscreen (`RAZDOR_SCENE` + `RAZDOR_SNAPSHOT`, see
   [testing-and-tools.md](testing-and-tools.md)); a decoder change gets a unit test on
   synthetic bytes plus a real-file test gated on `RAZDOR_DT_DIR`.

## Traps

- Never commit decoded output, screenshots of original art, or install files.
- The Python reference decoder in the local notes still has the two LIT bugs; trust
  `src/dt/gfx.rs` and its tests.
- Alpha is not premultiplied in `gfx::Image`; use `chrome::premultiplied` when a texture is.

<!-- refs verified by: python3 -I docs/flows/verify.py docs/flows/*.md -->
