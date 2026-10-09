# Discord Times (Aterdux, 2004) graphics formats

These notes are for interop only: a remake loads art from the user's own install at runtime.
Reference decoder: `gfx_decode.py` (Python + Pillow + numpy); the Rust port `src/dt/gfx.rs` is bit-identical to it on all 421 files. `python3 gfx_decode.py FILE... OUTDIR`.
All 421 `.lit/.ugs/.spi` files in the Community Update install decode without errors. Each format below
was checked by eye against the gameplay video.

All integers are little-endian. Confidence levels: **H** = verified by exact file-size accounting and visual
check, **M** = consistent but not proven, **L** = guess.

---------------------------------------------------------------------------------------------------
## 1. UGS: 16-bit ARGB4444 sprite sequences (scrambled)

### Pixel format (H)
Each pixel is one u16 `v`. The file stores a bit-rotated ARGB4444 value XOR'd with a key:

    stored = rotl16(ARGB4444, 3) ^ 0xAAAA
    decode: p = rotr16(v, 3) ^ 0x5555        (0x5555 == rotr16(0xAAAA, 3))
            A = p>>12 & 15, R = p>>8 & 15, G = p>>4 & 15, B = p & 15   (scale by 17)

- `0xAAAA` decodes to (0,0,0,A=0), so it is the fully transparent background.
- Alpha is 4 bits. Map sprites use real partial alpha for soft shadows and anti-aliased edges.
- **Spell and effect animations** (`Spells/*.ugs`, and partly `Battle/*.ugs`): in Spells, A=0 in every pixel
  and the colour is in RGB. Draw them **additively** (black = nothing). The `_Global.ini [BattleEffects]` lines
  `EffectN=name,dR,dG,dB,dA,dY` give per-effect colour/alpha correction and a Y offset (from the ini comment).

### Container variants
**a) Plain sequence (H):** repeated `[u16 w][u16 h][w*h u16 pixels]` to EOF, rows top-down.
Frame counts come from the file size.

| File(s) | frame size | frames | contents |
|---|---|---|---|
| `Units/*.ugs` (Knight, Mage, Peasant, Rogue, Necromant, Zombie, Ghost, Hero-*) | 64×64 | 64 | world-map party sprites |
| `Units/Ship-Merchant, Ship-Pirat` | 128×128 | 64 | ships |
| `Objects/Icons.ugs` | 92×92 | 102 | colour unit face/bust portraits |
| `Objects/Items.ugs` | 53×53 | 167 | artefact/item icons |
| `Spells/*.ugs` (P-*, S-*) | 128×128 | 50 | spell animations (additive) |
| `Battle/--*.ugs` | 220×110 | 25 (UPGRADE 50) | battle effects (`_Global.ini [BattleEffects]`) |
| `Windows/Ask, Clock, House, Swords.ugs` | 40–48 sq. | 50 | animated UI icons |
| `Windows/MB2, MM_Icons, Stnd-1/2, Title_RUS` | – | 1 | UI stills, **not scrambled: grey + alpha bytes** (below) |
| `Windows/Way_Arrows.ugs` | 32×22 | 8 | path arrows, 8 directions |

Unit sprite layout (M): the 64 frames form 8 rows of 8. Each row is one facing direction and holds an
8-frame walk cycle. I have **not** checked the direction order; row 0 looks like facing away/north.

**Grey stills (H):** the five single-frame files of `Windows` above hold `[u16 w][u16 h]` then one grey byte
and one alpha byte per pixel, unscrambled (read as ARGB4444 they give colour noise). The game tints them:
`MB2` (228×251) = the main menu's two oval button frames (round, and pointed for "Выход") around a
translucent marble middle; `Title_RUS` = the silver "Времена Раздора" logo; `MM_Icons` = white minimap
symbols (houses, towers, castles, churches, skull, anchor) in three sizes; `Stnd-1/2` = standards (skull, towers).
The animated `Windows/Ask, Clock, House, Swords` are ordinary scrambled frames.

**b) `Objects/Persones.ugs` (H):** 102 records of `[u16 w][u16 h][u16 w][u16 h][pixels]`. Frame sizes vary
(about 115–162 × 340–420). These are the full-body sepia portraits in the hire/army screen. Frame order
matches `Icons.ugs`.

**c) `Objects/Objects.ugs`: world-map objects (H for layout, M/L for the meaning of the extra fields).**
It is a slot table. A slot is either one `u32 0` (empty) or a record:

    u32 cat, u32 idx, u32 cell, u32 w, u32 h, extra[..], u16 w, u16 h, w*h pixels

`cell` is the next power of two ≥ w (texture-atlas size, H). The length of `extra` depends on the section.
The decoder finds it by searching for the repeated `(u16 w, u16 h)`:
- **Section A (terrain decorations, 4-byte extra):** B,G,R average colour of the sprite (M; matches the mean
  of opaque pixels, probably the minimap colour) plus one byte ≈ coverage (L).
  cat 1 = grass hills, 2/4 = single green/yellow hill (idx 23), 3 = rocky hills, 5 = mountains,
  6 = dark mountains, 8 = stones/boulders, 9 = small trees and bushes (deciduous, autumn, fir, snowy,
  grass tufts, palms…), 10 = dead trees, 11 = big trees. idx is sparse, grouped in tens by size/variant.
- **Section B (buildings, 8-byte extra = u32 a, u32 b):** a,b are the building's footprint in cells (H: every building of the 15 shipped maps
  has exactly its picture's (a, b) as its size, bytes 289/290). cat 1 = cities (4), 2 = villages (8), 3 = castles (6), 4 = forts/towers,
  5 = tavern, 6 = market stalls, 7 = churches, 8 = misc (smithy, huts, standing stones, ruined city),
  9 = harbours/lighthouses and piers, 10 = altars/stone circles, 11 = cave, 12 = ruins,
  13 = stone road/bridge pieces, 14 = wooden bridges.
  Total: 370 records. See `png/objects_sec0.png` and `png/objects_sec1.png`.
  **Keys (H):** section A `(cat, idx)` = a `.DTm` map object's `(class, sprite)`, and section B `(cat, idx)` =
  a building's `(picture type, picture variant)` (bytes 5 and 4). Every object and building of all 15 shipped maps
  resolves to a sprite this way (checked by `dt::gfx` tests).

---------------------------------------------------------------------------------------------------
## 2. LIT: Aterdux's JPEG-like DCT image (or raw YCbCr)

Header (H), 16 bytes: `"LIT\0"`, `u32 width`, `u32 height`, `u32 flags`.
Flags work as bit fields (H; the values seen are 0, 2, 4, 6, 8, 10):

| bit | meaning |
|---|---|
| 4 | **raw**: then `w*h*3` bytes, interleaved Y,Cb,Cr per pixel, no padding. Bit 2 has no size effect here (types 4 and 6 have the same layout). |
| 2 | DCT: chroma subsampled 4:2:0; planes padded to a multiple of 16 |
| 8 | DCT: a 4th plane (alpha) follows at full (padded) Y resolution |
| none | DCT 4:4:4, planes padded to a multiple of 8 |

**DCT plane (H):** `u8 quant[64]` (natural raster order within the 8×8 block), `u8 rank[64]` (a permutation:
for each natural position, the index where its coefficient is stored; the encoder builds it per image),
then `64 × nblocks` bytes stored **coefficient-major**: all blocks' rank-0 byte (DC) first, then all
rank-1 bytes, and so on. Blocks run in raster order over the padded plane.
- DC byte is **unsigned**; AC bytes are **int8**.
- `coef[pos] = byte[rank[pos]][block] * quant[pos]`, then an orthonormal 8×8 IDCT (block mean = DC·q0/8).
  No level shift.
- Planes come in order Y, Cb, Cr[, A]. Each has its own 128-byte table header. With 4:2:0, chroma is
  (padW/2)×(padH/2); upsample by duplicating pixels. File size is exactly
  `16 + Σ(128 + plane_bytes)` (checked on every file).
- Colour: JFIF YCbCr→RGB with Cb, Cr centred on 128 (H: textures, portraits and UI match the video).
- The alpha plane is decoded the same way (clamp 0..255). Some type-8 spell icons have constant alpha 255.
  Those are masked by a separate `Spell-IconMask/Alpha.lit`.

The raw type is used for fonts (`SanSerif*.lit`, `Benguiat*.lit`: glyph sheets with blue bounding boxes,
which hold the glyph metrics), small frames, check boxes and "…Alpha" masks. Alpha variants of an image
(`X-Alpha.lit`, `Big_Frame_1a`, `CBoxUn-Alpha`, `army-alpha`, `smb-alpha`) are separate greyscale
LIT files, used as a mask for the matching colour file (M).

`Windows/Win-black.lit` is actually a plain 24-bit BMP (480×480) with the wrong extension. The decoder handles this.

### What's where
- `Textures/*.lit`: world-map terrain textures, **256×242** (detail.lit 256×256). They tile seamlessly
  in both axes (wrap-edge difference ≈ neighbour-pixel difference). The engine appears to map them across
  the hex terrain in world space, not per hex (M). The exe has a string table in this order (index = likely
  terrain code, M): 0 Shallow, 1 Water, 2 DeepWater, 3 FlameLand, 4 Road, 5 LowLand, 6 Land, 7 Plain,
  8 Swamp, 9 DeepSwamp, 10 Desert, 11 Badground, 12 Rock, 13 Dust, 14 Snow, 15 Ice.
  The video shows Land = green grass and Plain = yellow field. `detail.lit` is a greyscale detail overlay
  (its use on the map is unknown; Razdor does not draw it yet).
  **Transitions (M):** the video shows no cell edges: neighbouring surfaces fade into each other over about one
  cell (fields into grass, roads as soft bands, sand along rivers). Razdor draws this by mixing, per pixel, the
  textures of the four nearest cell centres with bilinear weights (`src/ui/terrain.rs`).
  `Textures/Water/TEXTURE000–031.BMP` are standard 64×64 24-bit BMPs: 32 water frames (10 per
  second) multiplied twice into the ground of water codes 0–2, one frame over 3×3 cells (exe
  string `Graphics\Textures\water\texture0%d`; engine.md §7).
- `Spells/*.lit` (100×100): spell book icons. `Windows/*.lit`: every UI window, frame, button, cursor,
  font, splash screen, building interior (`S_*.lit`, `BI_*.lit`), and hero portraits (`Hero0/hero1/hero2.lit`, 160×160).
- `Logo/AE_Logo.lit`: Aterdux logo (type 2).

---------------------------------------------------------------------------------------------------
## 3. SPI: editor palette icons (H)
`Editor/*.spi`: 1936 bytes, headerless **22×22 BGRA**, rows top-down, 8-bit alpha. There are 33 of them,
round spell/ability badge icons (22 filled and 11 empty black badges).

## 4. Standard files
`Editor/*.tga|TGA`: 32-bit RGBA Targa (unit/editor markers, grid hex masks 32×22, cursors).
`Editor/*.bmp`: 24/32-bit BMP. Pillow reads both directly.

---------------------------------------------------------------------------------------------------
## 5. Linking data to art
- **Unit portraits (H):** `Icons.ugs[n]` (92×92 colour bust) and `Persones.ugs[n]` (sepia full body), with
  `n = GlobalIndex − 1` from `Rus_Units.ini` (the section number `[N Name]`). Checked: 1 Рыцарь → frame 0
  (plumed knight), 72 Король → 71 (king), 73 Принцесса → 72, 25 Пушкарь → 24 (cannon),
  99 Архилич → 98, 100/101 Посланник Смерти → 99/100 (scythes), 102 Чумной доктор → 101 (plague doctor).
  **`IconIndex` is NOT the frame index into these files.** Example: Рыцарь IconIndex=43, but frame 43 is a
  different unit. IconIndex runs 0..99 and repeats for variants; its target is **unknown** (maybe an older
  ordering or a battle-figure table).
- **Artefacts (H):** `Rus_Artefacts.ini Icon=Axxx.Tga` → `Items.ugs[xxx]` (167 frames, 0..166).
  Example: A000 = sword "Победитель".
- **Map party sprites:** file names match the exe list (`Hero-Knight, Hero-Mage, Hero-Ranger, Hero-Ship-Vesla,
  Peasant, Knight, Necromant, Zombie, Ship-Merchant, Ship-Pirat, …`), with 8 directions × 8 frames.

## 6. Open questions
- Direction order of the unit rows, and the frame-to-time mapping for spell/battle animations (50 and 25 frames).
- Exact meaning of the Objects.ugs section-B `(a,b)` fields and the section-A 4th byte.
- What `IconIndex` indexes.
- Whether LIT's IDCT rounding matches the original exactly. The output is visually indistinguishable,
  so this only matters for bit-exactness.

## Samples (verified visually)
`png/Icons_sheet.png`, `png/icons_lab0/1.png` (with indices), `png/Persones_sheet.png`, `png/Items_sheet.png`,
`png/Knight_sheet.png`, `png/Peasant_8x8.png`, `png/objects_sec0.png`, `png/objects_sec1.png`,
`png/S-Fire_additive.png`, `png/m1.png` (LIT alpha, UI, logo, SPI), `png/m2.png` (splash, town,
paper, inventory), `png/m3.png` (SPI row, MM_Icons, arrows, battle effect). `png/all/` holds every file
converted (sheets for multi-frame files). These are local conversions of the user's install; never commit them.
