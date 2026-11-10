# Atmosphere: day and night, mist, moving water (design)

Settled with the owner in a grilling session, 2026-11-10. Source idea list:
`/mnt/ai_models/razdor-ideas/experience-ideas.md` (outside git).

## Scope and shape

- One release, each feature in its own commits (reviewable and revertible alone).
- **Style only.** No rule changes anywhere: sight, fog-of-war state, movement, combat, AI and saves
  behave as the original at every hour. Nothing the rules let the player see may be hidden.
- **Adventure map only**, flat and 2.5D. Battles and full-screen windows keep the original look.
- Three Settings rows in the Map group, all off by default (a fresh install looks like DT):
  1. «Освещение»: off / game clock / real-time cycle / fixed hour (cycle length and hour as sub-values).
  2. «Туман вместо черноты»: off / on.
  3. «Живая вода»: off / on.
- UI change: the owner approves off-screen screenshots (desktop + phones, flat + 2.5D, dawn / noon /
  dusk / night) before anything is pushed to `main`. Work stays on a local branch until then.

## 1. Day and night

- **Clock.** Game-clock mode reads the game's clock (`rules/clock.rs`) and eases the shown hour toward
  it (a long step does not jump dusk to night in one frame); the light stands still when time does.
  Real-time cycle: one day per N real minutes. Fixed hour: a chosen hour. Flicker and drift animate in
  real time in every mode.
- **Sun curve.** Continuous: brightness and warmth are smooth functions of the hour (only noon equals
  the original exactly). Night floor: the darkest night keeps the map at ~55–60 % brightness, blue
  tinted. No darkness slider, no highlight pass.
- **Sun direction.** Fixed azimuth: the direction the original's painted shadows imply (building
  pictures carry painted shadows, `BUILDING_SHADOW_ALPHA`). Only elevation and colour change with the
  hour. The sun is fixed **to the camera** (view space): at every 2.5D turn the sprites light the same
  and agree with their painted shadows; the ground's normals are turned by the camera yaw into that
  frame. Painted shadows deepen a little at low sun and soften toward ambient at night.
- **Light sources** (one table in code, easy to extend; no modder format yet): towns and villages get
  warm window glows; every army on the map (hero's and others') carries a small torch glow, so armies
  stay readable at night. Glows fade in from dusk, out at dawn. Additive light pools, no new sprites.
  No campfire (the trailer's 1:03 shot is reworded to torchlight around a resting army).
- **Relief.** Per-pixel normals computed by Razdor at load and cached: brightness → blurred height →
  Sobel → normals (the Laigter / SpriteIlluminator approach). Used on the flat map and in 2.5D
  (grooves raked by a low sun). In 2.5D each card is also lit by its own facing (see §2).

## 2. 2.5D: buildings stand on their ground (cards)

The owner's report: in 2.5D a village sprite stands as one card, so houses painted at the top of the
picture (further back on the ground in the original's oblique view) float in the air.

- A per-pixel depth map of each map sprite gives its ground line. The sprite is cut into a few
  vertical **cards** (one per house / tree clump / rock); each card stands at the ground point where
  its own bottom meets the ground and sorts like today's sprites. Works on GL ES 2, no extensions.
- Part of 2.5D, no own switch. A sprite with no cut data yet stands as one card, as today.
- **Where the depth comes from** (the owner's choice, "both"):
  - Base game and Community Update: the owner's offline run of Depth Anything V2 Large on the RTX 4060
    (≈20–30 s for all 370 `Objects.ugs` sprites). Only **numbers** ship: per-sprite cut lines and
    ground offsets keyed by the sprite's pixel hash, in a small file under `data/`, compiled in. No
    pixels, no depth images. (CLAUDE.md exception, the owner's decision 2026-11-10.)
  - Anything else (mods, community maps): Razdor's built-in Depth Anything V2 Small (ONNX, ~50 MB
    fp16, CPU), downloaded on demand from a Razdor release asset with a pinned hash, run once per
    unknown sprite, cached by hash next to the built mod folders. Estimated: desktop CPU ~1 min for a
    whole install, laptop iGPU 30–60 s, phone 3–4 min at 266 px input; mods cost only their changed
    sprites.
  - Licences to verify before shipping: V2 Small (believed Apache-2.0) is re-hosted with its notice;
    V2 Large (believed CC-BY-NC) is used offline only, its output reduced to numbers.
- Normals for lighting do not come from the AI depth (that is not shipped): see §1 Relief.

## 3. Mist instead of black

- Replaces only the black of unexplored cells; parts exactly where the black opens today (with its
  ease); explored land never fogs over again. No visible-now state.
- Stacked drifting translucent noise sheets a little above the ground (different heights, speeds);
  tall things poke through the upper sheets; sprites on unexplored edges keep fading as today. The
  flat view sees the same sheets from above. Sheet count chosen by measurement (looks per cost) on
  desktop and phone.
- Colour from the lighting palette (gold dawn, blue night, warm near glows); neutral grey-white with
  lighting off.

## 4. Moving water

- Over the original's 32-frame texture (kept): a gentle ripple distortion, soft pulsing shore foam
  along water/land edges (from the terrain grid), sky colour from the lighting palette (gold dusk,
  moon sheen at night), a fading wake behind sailing armies.
- **Rivers flow** on a best guess: from the landlocked end toward the nearest large water (gradient of
  distance to large water); ties broken by river length and by nearby high ground (flow away from
  it). A few wrong rivers are accepted.

## Order of commits

1. Lighting: sun curve, tint, glows, the Settings row and its modes.
2. Depth pipeline: offline tool, cut data for base + Community Update, built-in model runtime + cache.
3. 2.5D cards.
4. Relief: classic normals, groove and card-facing lighting.
5. Mist sheets.
6. Water: ripple, foam, sky colour, wakes.
7. River flow.

Performance budgets are measured, not assumed (mist sheet count, water and lighting cost on desktop
and a phone) and reported with the screenshots.

## Deferred to the ideas doc

Night effects on play (own switch, experimental track); lighting in battles (tinted to the hour at
the battle's start, frozen); a shader campfire and a campfire sprite; a darkness slider and a
deep-night highlight pass; a travelling sun with Razdor's own cast shadows (and the sun fixed in the
world, which needs them); the thin mist veil and the full mist close (the latter a rule change);
procedural water.
