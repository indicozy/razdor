# Weather by biome (design)

Decided by Claude for the owner, 2026-11-10; the brief is the owner's «Погода» request.
Branch `atmo-weather` (on top of `atmosphere-final`); not on `main`.

## Scope and shape

- **Style only.** No rule reads it: sight, fog state, movement, combat, AI, saves and the day's
  schedule of events are as before. The weather is recomputed from the map and the game clock,
  never saved.
- **Adventure map only**, flat and 2.5D. Battles and windows keep their look.
- One Settings row in the Map group, off by default: «Погода» (`Settings::map_weather`, serde
  default false), after «Живая вода». Help: "Razdor's: weather by the land: snow on snow and ice,
  rain on swamps and lowlands, heat haze over deserts, ash over fire lands, cloud shadows
  everywhere. Style only." Env `RAZDOR_WEATHER=1` turns it on, `RAZDOR_WEATHER_DAY=<n>` fixes the
  day index of the schedule (snapshots). Off: nothing is computed or drawn (pixel-identical).

## Biomes

The map's terrain codes (`rules::map::Surface`, the `.DTm` code = the index of
`gfx::TERRAIN_TEXTURES`) name the biomes by their texture:

| Biome | Codes (texture) | Weight |
|---|---|---|
| snow | 14 (Snow), 15 (Ice) | 1 |
| swamp rain | 8 (Swamp), 9 (DeepSwamp) | 1 |
| lowland rain | 5 (LowLand), 6 (land), 7 (plain) | 1 |
| heat haze | 10 (Desert), 13 (Dust) | 1 |
| ash | 3 (FlameLand), 11 (Badground) | ash 1, haze 0.5 |
| clouds | everywhere | 1 |

Water and road and rock get no weather (clouds only).

**The mask**: one texel per cell, built with the surface-code textures (`terrain::TerrainLayer::cells`,
rebuilt only when the codes change, like the water mask), linear, clamped. Two textures:
`weather` (RGBA = snow, swamp rain, heat, ash) and `lowland` (R = lowland rain). The shader reads
them at the ground point under each pixel.

## The day's weather

- `weather::Spell` per biome per game day: None, Light (0.5), Heavy (1.0). A deterministic hash
  (splitmix64) of (map seed, day index, biome) decides: weather on with the biome's chance, heavy with
  40 % of those. Chances: snow 60 %, swamp rain 45 %, lowland rain 20 %, heat haze 80 %, ash 70 %,
  clouds 70 %. The map seed is an FNV-1a hash of the map's size and codes.
- **Easing**: a day's spell runs the whole day. Each midnight blends from the day before's level to
  the next's over one game hour, centred on midnight (smoothstep), so no level jumps.
- The shown levels ease toward the target in real time (exponential, `LEVEL_TAU` 0.8 s), so a wait
  of several game hours (30-minute ticks every 150 ms) does not step the weather.
- The day index is the game clock's day, or `RAZDOR_WEATHER_DAY` when set.

## Drawing

One screen-space pass after the world, the flat fog and the daylight (after the mist too), before the
clash marks and the UI, over the map's drawn area. The fragment shader does everything (no CPU particle
lists) and its layers are skipped when their level is 0.

- **Ground point per pixel.** Flat: `(pixel − view + origin) / scale`. 2.5D (planar): the ray through
  the pixel (`TiltCamera::view_proj`'s inverse, as `pick`) hits Y = 0; the matrix is a uniform. Past the
  map or past the horizon: no weather. Bent views (the first-person and eyes views, hidden from players)
  use the plane's hit as an approximation (the sheet is curved there): a known limit, documented.
- **Visibility**: the fog's shown darkness at the ground point (`minimap::fog_layer`); weather falls only
  over explored ground, and the mist's cells included.
- **Snow**: flakes drifting down-left with a wobble, two depth layers (phones: one).
- **Rain**: slanted streaks falling fast, two layers, a little lighter than the ground; heavy rain also
  darkens the ground a little.
- **Heat haze**: **faked**, not a distortion. Reading the drawn frame back would cost a full-frame GPU
  readback each frame, so the haze is soft warm light bands wavering over the ground.
- **Ash**: slow grey-orange specks, rising a little. A heavy heat spell adds sand specks sideways
  (the dust gusts), the same speck layer in sand colour.
- **Cloud shadows**: fbm blobs in the ground's own coordinates, drifting with the terrain clock, darkening
  the ground under them. Drawn in screen space at the ground point, not through `ground` in the world
  pass: the same place on the ground, one pass, and a cloud's shadow over a tree is right too (a
  deliberate change from the brief).
- **Light**: the particles and haze take the frame's daylight tint (`daylight::Light::tint`): dark at
  night, gold at dusk.
- **Cost**: one pass; layers are skipped at level 0; phones use fewer layers (`display::dp_px`). The
  fragment cost is measured and reported with the renders.

## Verify

- Unit tests: biome mapping, the schedule's determinism and its percentages over 1000 days, the
  midnight blend's continuity and monotonicity, the level easing.
- Off-screen renders (Xvfb, pinned to CPUs 8–19): a snow map, a swamp map and a desert map if the
  install has them, flat and 2.5D, dawn / noon / dusk / night, weather on, into a contact sheet under
  the scratch folder.
- Weather off: pixel-identical to the build before this work (the baseline binary's shots).
- `cargo clippy`, the touched modules' tests, the `--no-default-features` build, the flow doc's
  «Weather» paragraph (`verify.py`).

## Not in this change

- A wiki page and the owner's approval of the screenshots (a UI change) come before `main`.
- Weather in battles; weather that the rules read (none, by design).
- Shadows of the clouds on the sprites' own pictures, and a real heat distortion (see above).
