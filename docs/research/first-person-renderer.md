# Research: can Razdor's renderer draw a first-person world map? (indicozy/razdor#47)

Date: 2026-11-14. Base: `origin/main` at 859c2ac1. Read-only on the code; no builds, no game run.

## Summary

- **Most of the ticket already exists.** The 2.5D view (tilted perspective camera, billboards, cards,
  fog, sky) and a first-person view («От первого лица», two cameras: a chase camera behind the hero and an
  eyes camera at eye height, with a bent far ground, a horizon or a planet-like globe) are shipped and
  documented in `docs/flows/assets-and-drawing.md`. The open parts of #47 are narrower: a real depth
  buffer (today depth is off and sorting is CPU painter's order), the 8-yaw pre-rendered sprites (against
  an owner decision), and a phone budget that is not yet measured for the billboard path.
- **macroquad 0.4.16 (the fork) can do all of it on desktop GL.** `Camera3D` is perspective with a depth
  test; `draw_mesh` draws textured triangles in 3D; custom materials (`load_material`) can add alpha
  test and fog. The only fork patch that matters for billboards is `push_cpu_model_matrix`, which keeps
  one draw call for many billboards.
- **Android is GLES 2.0 (GLSL ES 1.00), 16-bit depth, no instancing.** Any new shader must be GLSL 100.
  Depth precision at 16 bits over a first-person far range is the main technical risk.
- **Budget (measured, not estimated):** the low-end phones are CPU-bound on the sprite stage, not on
  the ground or the GPU. Galaxy A04 (PowerVR GE8320): whole map 2.5D 102 ms CPU, near 25 ms CPU. Galaxy
  A13 (Mali-G52): whole map 67–79 ms CPU in the sprite stage, near 12–13 ms. A per-sprite CPU path will
  not hold 60 fps on the A04 whole map; a GPU-placed batch path (backlog 1605 step 5) is the route, and
  its first try was slower on desktop.
- **Texture memory for 8 views is the big cost.** The whole map's sprites at 1x fit one atlas of 13.8 MB
  (Другой берег). 8 views would be roughly 8× that before box-cutting (about 110 MB, an estimate). The
  atlas box cut and half/quarter copies reduce it; 2x sprites are four times the 1x cost.
- **Recommendation:** do not start with 8 pre-rendered yaws. Start with a depth-tested billboard
  experiment on the existing eyes camera (alpha-test material, 16-bit depth checked on the A04), measure
  billboard counts on the A04 with the frame probe, and then decide the yaw question with the owner.

## 1. What macroquad 0.4.16 (Razdor's fork) offers

Version pin: `Cargo.toml:25` `macroquad = "0.4"`, overridden at `Cargo.toml:91-98` by a `[patch.crates-io]`
to `indicozy/macroquad` rev `351b2b3e84f5dbbb3f6bb312afb8587abe6f5f7e` (crate version 0.4.16, branch
`razdor-stack`) and `indicozy/miniquad` rev `3c37a7bb6abb7302df41f60001066fde99806bdb` (miniquad 0.4.11,
`Cargo.lock:922-925`). The fork source is on disk at
`~/.cargo/git/checkouts/macroquad-86eb4fa6e4a0ceb1/351b2b3` and miniquad at
`~/.cargo/git/checkouts/miniquad-fde9d6afb95fee92/3c37a7b`. Upstream 0.4.16 (crates.io copy) has zero
occurrences of `push_cpu_model_matrix`; the fork adds it.

Primary sources for the public API (docs.rs pages for 0.4.16; the local fork source is the authority for
behaviour, and I did not fetch these pages this session):
- `Camera3D`, `Projection`: https://docs.rs/macroquad/0.4.16/macroquad/camera/struct.Camera3D.html
- `draw_mesh`, `Mesh`: https://docs.rs/macroquad/0.4.16/macroquad/models/fn.draw_mesh.html
- `load_material`, `gl_use_material`: https://docs.rs/macroquad/0.4.16/macroquad/material/index.html
- Source (GitHub): https://github.com/not-fl3/macroquad (upstream), fork `indicozy/macroquad`.

Facts from the source (fork rev 351b2b3 unless noted):

| Need | What macroquad gives | Where |
|---|---|---|
| Perspective camera | `Camera3D` with `Projection::Perspective`, `perspective_rh_gl` | upstream `src/camera.rs` (`Camera3D` ~l.187-260); Razdor builds its own matrix with `Mat4::perspective_rh_gl` (`src/ui/view25.rs:358-365`) |
| Depth test | `set_camera` sets `depth_test(camera.depth_enabled())` per camera. `Camera3D::depth_enabled` is `true`; `Camera2D` and Razdor's `View3d` return `false`. | `camera.rs:279-293`; `Camera3D` impl `camera.rs:265-267`; Razdor `src/ui/world_view.rs:1536-1550` |
| Depth pipeline | 3D triangles use a pipeline with `depth_write: true`, `depth_test: LessOrEqual`; the 2D default has no depth. | fork `src/quad_gl.rs:440-460` (triangles depth pipeline) |
| Depth clear | `clear_background` uses `PassAction::clear_color`, which sets depth to 1.0 as well. | fork `src/window.rs:22-26`; miniquad `src/graphics.rs:550-556` |
| Textured triangles in 3D | `draw_mesh(&Mesh { vertices, indices, texture })`, `Vertex::new(x, y, z, u, v, color)` (position is Vec3). | fork `src/models.rs:47-53`; Razdor uses it in `src/ui/shadow.rs:54`, `src/ui/living.rs:158` |
| Textured quad in 3D | `draw_texture_ex` is a 2D call; for 3D use a `Mesh` quad. No 3D texture draw helper. | fork `src/texture.rs:517` |
| Alpha blending | Default colour blend is source-alpha over (`SourceAlpha`, `OneMinusSourceAlpha`). | fork `src/quad_gl.rs:407-412` |
| Alpha test | The default fragment shader is `gl_FragColor = color * texture2D(Texture, uv)`, with no `discard`. Alpha-tested billboards need a custom material. | fork `src/quad_gl.rs:1103-1110` |
| Custom shaders | `load_material(ShaderSource::Glsl{vertex, fragment}, MaterialParams{uniforms, textures, pipeline_params})`, `gl_use_material`. Razdor wraps it in `src/ui/sharp.rs:239-281` (`material_in`, `material_fog`, `bind_fog`). | fork `src/material.rs:122-150` |
| CPU model transform | `push_cpu_model_matrix` multiplies the vertices on the CPU before batching, so the draw calls are shared. The billboards use it. | fork `src/quad_gl.rs:882-890`; Razdor `src/ui/world_view.rs:2020-2026`, `2526` |
| Render targets | `RenderTarget` plus `Camera2D.render_target`; Razdor's `render_cache` uses them. | upstream `src/camera.rs` (Camera2D), Razdor `src/ui/render_cache.rs:11-87` |
| Batch limits | Per draw call: 10,000 vertices and 5,000 indices by default (`draw_call_vertex_capacity`, `draw_call_index_capacity`). That is about 833 quads per batch. `geometry()` silently clamps a single call to these limits (with a warning). Razdor does not change them (no `gl_set_drawcall_buffer_capacity` call in `src/`). | fork `src/lib.rs:920, 933`; `src/quad_gl.rs:912-918` |
| Index type | `u16` indices only. | fork `src/quad_gl.rs:912` (`indices: &[u16]`) |

Desktop GL vs GLES:
- **Desktop GL** (Linux, Windows, macOS): everything above works. `Features.instancing` is on for GL3+.
- **Android**: miniquad's Android path calls `create_egl_context` (`src/native/android.rs:458`), which
  requests `EGL_CONTEXT_CLIENT_VERSION 2` (`src/native/egl.rs:240`), so the context is **GLES 2.0**. It
  requests a 16-bit depth buffer (`src/native/egl.rs:194`). The GL setup marks GLES 2 with `gl2`, which
  turns instancing off (`src/graphics/gl.rs` ~l.834-840) and offers GLSL 100 only (`gl.rs:845`).
  The repo's own GPU path already pins "GLES2 GLSL ES 1.00" in a test
  (`docs/perf/2026-11-12-atmosphere-performance.md:482`).
- **Web**: not a target of this ticket.

## 2. What Razdor already has (3D camera, depth, card sorting)

**Yes, a 3D camera is in use, and it already covers first person.** The 3D maths lives in
`src/ui/view25.rs` (pure, tested, no drawing):
- `TiltCamera` (`view25.rs:282-300`): a target, distance, pitch, yaw, fov, an optional `bend` (the
  curved ground) and an optional `eye_at` (the eyes camera).
- `view_proj` (`view25.rs:356-366`): for the eyes camera `look_at_rh(eye, eye + look_dir)` with a
  perspective, near `EYE_CLIP` 0.05 and far `4·(distance + FAR)`; for the others a look at the target
  with `near = distance·0.01`. `FAR` is 240 (`view25.rs:207`).
- `billboard_model` / `billboard_model_about` / `raised_ground_model` (`view25.rs:686-730`): a sprite is a
  plane through its foot facing the eye.
- `Bend` (`view25.rs:738` on): the far ground curves down ("Горизонт") or in every direction ("Шар"), with
  `pick_bent` / `pick_globe` for picking (`view25.rs:585-660`).
- The first-person constants: `FP_PITCH` 12, `FP_FOV` 55, `FP_FLAT` 3 (`view25.rs:217-219`); eyes
  `EYE_HEIGHT` 0.7, `EYE_PITCH` 2, `EYE_NEAR` 2.5 (`view25.rs:236-241`).

The drawing seam is `src/ui/world_view.rs`:
- `View3d` (`world_view.rs:1534-1550`): a `Camera` impl with `depth_enabled() == false`, no viewport, the
  matrix `window_matrix · view_proj`. **This is the one place depth is switched off.** The comment there
  says the map is painted back to front, as the flat view is.
- `in_view` (`world_view.rs:1577-1592`): sets `View3d` and runs the closure on the tilted cull space.
  `in_view_sprites` (`1597-1607`) is the second run for a bent view. `bent_view` (`1613-1636`) draws the
  ground into a picture and lays it on the bend (`draw_sheet`), then the sprites on the sheet.
- `standing` / `standing_at` (`world_view.rs` ~2010-2030): billboards through `push_cpu_model_matrix`.
  The comment there records the reason: a GPU model per sprite meant one draw call per sprite, and on the
  OnePlus 13 (Adreno) the 2.5D frame was 17.7 ms against 8.3 ms flat.
- Terrain: `TerrainLayer::draw` is one shader quad mixing four cell textures from a 4×4 atlas
  (`docs/flows/assets-and-drawing.md`, world map section, `src/ui/terrain.rs`); the ground under 3D is
  placed with `ground_model` / `ground_homography` (`view25.rs:675`, `view25.rs:2030`).
- Fog: `sprite_fog` (per-cell texture, read by the sprite shader, `src/ui/sprite_fog.rs`) and the
  `material_fog` materials (`src/ui/sharp.rs:275-281`).
- Sky: `draw_sky` and the frame's `clear_background(SKY)` (flow doc, the sky paragraph).

**Depth buffer: not used for the map.** `depth_enabled` is false in `View3d`, and the
world is painted back to front. Sort order comes from `sprite_key` (the foot's depth, computed on the
CPU). The per-card and per-sprite order is the sorted item list in `draw_world_in`
(`docs/flows/assets-and-drawing.md`, world map section). Razdor has no `depth_test` use anywhere else in
`src/` (grep: only `chrome.rs`, `sharp.rs`, `render_cache.rs`, `terrain.rs`, `shadow.rs` set
`PipelineParams`, none enable depth).

**How cards are drawn today** (`src/dt/cards.rs`, `src/ui/dt_art.rs`, `docs/flows/ground-classifier.md`):
- `data/cards/objects.json` is keyed by `dt::cards::sprite_key` (`src/dt/cards.rs`, FNV-1a 64 over
  `u32le(w) ++ u32le(h) ++ rgba`), and each card has `foot`, `verts`, `tris` and a `ground` flag
  (`cards.rs:14-35`, `parse` at `cards.rs` ~l.93). Data only, no pixels: permitted by `CLAUDE.md`.
- Cards are **off by default** (owner decision 2026-11-12); `dt_art::building_cards_on`
  (`src/ui/dt_art.rs:202`, `RAZDOR_BUILDING_CARDS=1`, dev-only).
- A card building is one group, one billboard on the building's foot (`building_foot`), with each layer
  shifted by `layer_offset` and `LAYER_PARALLAX` (flow doc).

**Reusable for first person:**
- Ground points and feet: `card_foot`, `building_foot`, `object_foot`, `sprite_key` ordering.
- Per-card triangles: `Card.verts` / `Card.tris` (`cards.rs:14-35`) map straight to a `Mesh` (`card_mesh`).
- Sprite keys for any per-sprite cache (already used by the depth model and by `living`/`lights`).
- The bent-ground maths (`Bend::point`, `Bend::lift`, `raised_ground_model`) for placing objects on the
  curved ground.
- The eyes camera and its near fade (`HERO_ON_WINDOW`, `FP_HERO_MARGIN`) are exactly the problem a
  depth-tested first-person view would otherwise have to solve.

## 3. Where the world map draws and the seam for a second view mode

- Entry: `world_view::draw_world` → `draw_world_in` (`src/ui/world_view.rs`, file of ~9.4k lines). The
  item list is sorted once per frame, then drawn with `draw_terrain` first, then decorations, buildings,
  figures and the hero, row by row (`docs/flows/assets-and-drawing.md`, «The world map»).
- View state: `audio::Settings::map_tilt`, `map_first_person`, `map_eyes` → `main_menu::apply_options`
  → `world_view::set_tilt_view` / `set_first_person` / `set_eyes` (flow doc, «The first-person view»).
  `world_view::eyes_view()` and `first_person()` answer the question each frame.
- The camera object: `Camera::tilt` (`Tilt` with `cam: TiltCamera`, `cull`), set up by `looking_in`. Both
  the flat and tilted views share the same `Camera`, the same input, and the same state. This is the seam.
- **Top-down view that rotates in 8 steps:** the yaw is already free (`TiltCamera::yaw_deg`, any value,
  `view25.rs:286-288`), and Q/E and the two-finger twist change it (flow doc). Snapping to 8 steps is a
  settings or input change, not a renderer change. The owner's 2026-11-11 decision says "no rotation of
  assets, everything faces the camera", so the 8 steps must not turn sprites.
- **Second view mode without a separate code path:** add a `ViewMode` enum next to `map_tilt` /
  `map_first_person` / `map_eyes`, or extend `TiltCamera` with a `billboard_mode` flag. Keep `in_view`
  and `standing` as the only places that know the camera. Phones use the same code (the flows keep phones
  and desktop on one path; the only phone switch is `display::follow_settings`).

## 4. Budget: billboards, ground tiles, texture memory

Measured (all numbers from `docs/perf/` or the flow doc; the device and mode are given):

| Scene | Device | Frame | Draws / verts | Source |
|---|---|---|---|---|
| Galaxy A04, PowerVR GE8320, 720×1510, 2.5D near | phone | 40 ms (25 FPS), CPU 25 ms | not recorded | `docs/perf/2026-11-12-galaxy-a04.md` |
| Galaxy A04, 2.5D whole map | phone | 119.5 ms (8.3 FPS), CPU 102 ms, sprite stage ~100 ms | not recorded | same |
| Galaxy A04, flat whole map | phone | 89 ms (11.1 FPS), CPU 51 ms | not recorded | same |
| Galaxy A13, Mali-G52 MC2, 1080×2273, 2.5D near | phone (32-bit build) | 38.5 FPS, CPU 12–13 ms sprite stage | not recorded | `docs/perf/2026-11-12-galaxy-a13.md` |
| Galaxy A13, 2.5D whole map | phone (32-bit) | 12.1 FPS, sprite stage 67–79 ms CPU | not recorded | same |
| RTX 4060 via VirtualGL, 1024×768, Другой берег, 2.5D zoomed out | desktop GPU | work 5.68 ms, world 5.19 ms | 180 draws, 16,348 verts | `docs/perf/2026-11-12-atmosphere-performance.md:175-181` |
| same, whole map 2.5D | desktop GPU | work 7.40 ms | 216 draws, 54,408 verts | same |
| same, whole map, third person | desktop GPU | work 13.91 ms | 213 draws, 58,492 verts | same |
| Monitor 5060 Ti, first person | desktop GPU | 11.2 ms (~88 FPS) after batching; draws 3,767 → 337 | — | `docs/perf/ideas-ledger.md` (2026-11-11 row) |
| Adreno (OnePlus 13), 2.5D vs flat | phone | 17.7 ms vs 8.3 ms before the CPU matrix patch | — | `src/ui/world_view.rs:2020-2026` comment |
| Desktop, zoomed out 2.5D, 4,169 decorations placed on the GPU | desktop GPU | world +30 % (slower) | 173 → 221 draws | ideas-ledger, 1605 step 5a row |

What these say:
- **Billboard count:** the phone cost is per sprite on the CPU (the placement math, the cull, the fade,
  the shade), and it dominates the whole map. No per-sprite cost and no visible-sprite count per frame are
  recorded in the docs, so a per-billboard microsecond figure cannot be derived from them. What is known is
  the whole-map CPU total (100 ms on the A04) and that the sprite stage is most of it. **To measure with the
  frame probe on the A04:** visible billboards per frame, and CPU ms per billboard, for a first-person view;
  then the budget (a 60 fps target leaves 16 ms of frame time in total).
- **Draw calls:** the batch is 833 quads (5,000 indices). Each run of billboards that breaks at a
  non-billboard item (figure, hero, building) costs one draw call and a material switch. The GPU placement
  experiment showed that interleaving costs more than it saves (ideas-ledger, 1605 step 5a). A
  first-person scene with many interleaved items needs an order that does not break runs, or depth
  testing (which lets one batch cover all depths).
- **Ground tiles:** the terrain is one shader quad per layer, not per-tile meshes (`TerrainLayer::draw`).
  A first-person ground is one textured mesh over the visible cells: about 2 triangles a cell. For a
  view of 25×25 cells that is 1,250 triangles, far under the batch limit. The cost is fill rate over the
  far ground, which the bend and the far fade already reduce. No per-tile measurement exists for 3D.
- **Texture memory:** the map's sprites at 1x fit one atlas of 2048×1684 RGBA = 13.8 MB
  (`docs/perf/2026-11-12-atmosphere-performance.md`, item 7 section); with half and quarter copies it is
  2048×2298 = 18.8 MB. VRAM for a map scene: 36–63 MB on the 4060 (atmosphere doc, the Table at
  line ~55).
  - A sprite of 310×256 at 1x is 310·256·4 B = 317 KB per view. 8 views = 2.5 MB per sprite; at 2x (620×512)
    it is 10.2 MB per sprite. Sprite counts are not in the docs I read, so the total is a formula:
    `total ≈ N × 8 × area × 4 B`. With the atlas figure as a base (13.8 MB for all of the Другой берег
    sprites at 1x, before the box cut), 8 yaw views are roughly 8 × 13.8 ≈ 110 MB, before the box cut
    (`src/ui/atlas_cut.rs`) and before any reuse of the views. Estimate only.
  - The box cut keeps each sprite's opaque box (+1 px); it removes most of the transparent area, which is
    the largest saving available for 8 views.
  - `GL_MAX_TEXTURE_SIZE`: the atlas falls back to the full sprite when over the limit
    (`docs/perf/2026-11-12-atmosphere-performance.md:301-303`). Eight views need either multiple atlases or
    an array of pages. The GLES 2 limit on a low-end phone is not recorded here; check it on the A04.
  - Compressed textures (ETC2/ASTC) are rejected: lossy and against "same as DT" (ideas-ledger).

## 5. Risks and unknowns

1. **Depth precision on GLES 2 (16-bit depth).** The eyes camera's depth range is 0.05 to
   `4·(distance + 240)`, a ratio above 10⁴. With a 16-bit buffer the far half of the view will z-fight.
   Razdor's current `View3d` avoids this because depth is off. A depth-tested view needs a tighter far
   plane, a reversed or logarithmic depth in the shader, or it must stay painter's order. Unverified on a
   device.
2. **Alpha-tested billboards with depth.** The default fragment has no `discard` (`quad_gl.rs:1109`). A
   depth-tested billboard must `discard` transparent texels in its own material (Razdor has `discard`
   precedent in `src/ui/chrome.rs:1026` and `src/ui/terrain.rs:319`). Without it, transparent corners
   write depth and hide the sprites behind them. Also needs `depth_write` and a sort for the blended
   edges.
3. **GLES 2 constraints.** No instancing (`gl.rs` features), GLSL 100 only, `u16` indices, no VAO. The GPU
   path must stay GLSL ES 1.00 (`docs/perf/2026-11-12-atmosphere-performance.md:482`).
4. **Fork dependency.** `push_cpu_model_matrix` exists only in the fork (rev 351b2b3). Upstream 0.4.16
   lacks it. Any depth or batching work must stay on the fork or be upstreamed.
5. **Batch cap.** 10,000 vertices / 5,000 indices per draw call, silently clamped (`quad_gl.rs:912-918`).
   Large meshes (a whole first-person ground) must be split by hand.
6. **Owner decisions in conflict.** "No rotation of assets, everything faces the camera" (2026-11-11) and
   "a building stands as one whole picture in every view" (2026-11-12) limit the 8-yaw idea; a yaw view set
   is a change to the look and needs the owner's approval (`CLAUDE.md`, UI rule). A first-person view
   change is also a UI change for the owner's screenshots.
7. **Measurement gap.** No per-billboard cost or draw count exists for the phones in `docs/perf/`: the
   A04 and A13 tables do not record draws or verts. Add them to the frame probe before any budget
   decision.
8. **Sorting vs depth.** With depth on, the card and sprite order for blended edges still matters.
   Cards that overlap in depth (a flag and its card) need the same ordering as now.
9. **Ticket premise.** The ticket asks for a first-person mode; the repo has one (eyes and chase). The
   question for the owner is what the ticket adds on top: depth, yaw views, or phone budget.
10. **Unknown:** the sprite count N and the view count per sprite were not in the docs. The 8-view memory
    total above is a formula, not a measurement.

## Next steps (not done here)

- Measure draws and verts on the A04 for the current 2.5D and eyes views (frame probe), with
  `RAZDOR_PROFILE` as in `docs/flows/testing-and-tools.md`.
- Test a 16-bit depth buffer with an alpha-tested billboard material on the eyes camera, on the A04
  (not on the owner's desktop: no window on :0, per the owner's rule).
- Count the sprites that a first-person view would draw at each zoom (N), then compute the texture total.
- Decide the yaw question with the owner before any asset turns.
