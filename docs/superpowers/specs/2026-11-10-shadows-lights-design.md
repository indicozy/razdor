# Shadows, lights and a living map (design)

Settled with the owner, 2026-11-10 (grilling S1–S8 and follow-ups). Builds on the atmosphere work
(`2026-11-10-atmosphere-design.md`). Its own release, after atmosphere. Style only: no rule,
path, AI or save change. Analysis: `2026-11-10-shadows-perf-analysis.md`.

## Settings (all under «Освещение», shown only when lighting is on)

- «Тени»: «Нарисованные» (default; today's look: the original's painted shadows, sun fixed to
  the camera) / «От солнца».
- «Качество света»: high / medium / low; default high on desktop, medium on phones.
  - high: 8 point lights with shadows and relief;
  - medium: 4 point lights with shadows, point-light relief from the 2 nearest;
  - low: window and torch glows only (no light shadows, no light relief). Sun shadows at every level.
- «Живая карта» (its own row in the Map group, off by default): §5.

## 1. Sun shadows («От солнца»)

- **Real-time**, not baked (S1; the analysis: baking costs 160–210 MB RAM, 10–27 min first load on
  a phone and the depth model on every install, for no better shapes).
- Painted shadows removed (alpha band 0 < a ≤ 0.76 of building and decoration pictures).
- The sun travels, fixed in the WORLD: east at 6:00, south at 12:00, west at 18:00; elevation from
  the sun curve; shadow length `clamp(1/tan(elev), 0.25, 2.6)` sprite heights; in 2.5D the shadows
  turn with the map. Relief light comes from the same sun.
- Casters (S2): buildings and decorations (trees, rocks, hills, mountains). Figures keep their
  painted shadows (figure sun shadows: later).
- **Per card, in both views**: a sprite with cut data casts one sheared silhouette per card from
  that card's own foot (a village's houses each cast their own; a single silhouette from the
  sprite's foot smears the whole picture, as the analysis showed). Sprites without cuts cast
  from their foot. The silhouette is the blurred alpha of opaque pixels (≥ 0.8), made once at
  atlas build (an R8 "shadow atlas", ~3.5 MB); per frame one quad (or the card's triangles) per
  caster on the ground, before the standing sprites and under the fog/mist.
- Night (S4): shadows fade at dusk; from ~20:00 a faint (~12 %) shadow from a fixed south-west
  moon until dawn.
- Shadows fall on water at half strength.

## 2. Light sources

Revised by the owner (2026-11-10): no window detection needed for lights.

- **One light per building**: a single point light at the centre of the building's footprint,
  radius about the sprite's own size. It lights the ground and neighbours, casts the light shadows
  (§3) and rakes the relief (§4). Works for every building, mods included.
- **An emissive layer on top**: at dusk and night a second pass over each building sprite adds warm
  light where the picture's own pixels look like openings (dark, low-saturation pixels inside the
  opaque body; never the painted-shadow band), computed in the shader from the sprite itself — no
  new art, nothing derived ships.
- The detector data (`data/lights/objects.json`, `tools/depth/detect_points.py`) is optional: the
  living map's chimneys, flags and mill sails (§5), and an optional boost of known windows.
- **The hero and every army carry a torch**: about an arm's length (≈ 0.4 of the figure's height)
  from the head, forward along the figure's facing, about 60° above horizontal; it swings with
  the heading and, in 2.5D, follows the screen facing as the sprite row does. Its pool leads the
  way.
- The old centre blob glows are removed.

## 3. Light shadows (S5)

Per-light silhouette quads: for each of the brightest lights on screen (capped by quality), every
caster within ~3 cells gets its silhouette pushed away from the light, longer when closer, fading
with distance. Dusk and night only. Same shadow atlas as the sun.

## 4. Point lights on relief (S7)

Each point light adds a warm pool falling off with distance, and the relief lighting takes the
nearest lights into account (grooves and walls facing a torch catch it). A small loop over the
capped lights in the sprite and ground shaders, at night only.

## 5. Living map («Живая карта»)

Shader-drawn, no new art; detector-found points (same detector run as §2): chimney smoke (soft
rising puffs), turning windmill blades (the blades are their own card from the depth cut, turned
about the hub), flags waving (a cloth wave on the flag's box), bird flocks (small V shapes
crossing the explored sky/ground now and then). Carts on roads need new sprites: later.

## Performance

Measured per quality level on desktop and on the owner's OnePlus (the phone test is the owner's
help). Budgets set from those numbers.

## Later (ideas doc)

Figure sun shadows; carts; weather by terrain (rain, snow, heat haze, cloud shadows); sounds by
place and time.
