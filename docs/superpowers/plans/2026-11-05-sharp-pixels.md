# Sharp pixels (sharp bilinear) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Draw all of DT's art with "sharp bilinear" sampling (each original pixel a crisp, evenly sized square at any scale) instead of today's bilinear smoothing, for everyone, with no option to go back.

**Architecture:** One new module `src/ui/sharp.rs` holds a GLSL `sharp_uv()` function (GLSL ES 1.00, so phones run it), a material that uses it, and `sharp::draw()`, a drop-in for `draw_texture_ex` that passes the texture size and the draw's scale as uniforms. Art draws switch to it; the existing special shaders (additive, silhouette, grey, terrain) get the same `sharp_uv()` call. Textures stay `FilterMode::Linear` (the shader needs bilinear hardware sampling).

**Tech Stack:** Rust, macroquad 0.4.16 (`load_material`, `gl_use_material`), GLSL `#version 100`.

**Spec (agreed with the owner in chat, 2026-11-05):**
- Sharp bilinear is the default for everyone; there is no smooth option and no setting.
- Sharp: units, buildings and map objects, terrain, portraits, figures, items, windows, battle and spell art.
- Stays as it is: the minimap and the fog of war (soft on purpose), Razdor's own UI text, and textures already set to `FilterMode::Nearest`.
- Before the push to `main`: off-screen screenshots of every changed screen, old vs new, on desktop and two phones. They are posted in the Discord reports channel (the owner asked for that) and wait for the owner's yes. Then run the UI regression check (diffs reviewed, the changed references accepted) and the mobile UI check, and add a changelog line, a wiki note (ru + en) and the `docs/flows/assets-and-drawing.md` update.

## Global Constraints

- Machine: every cargo and heavy command is `CARGO_BUILD_JOBS=12 nice -n 19 taskset -c 8-19 …`; never CPUs 0–7. Full test runs add `RAZDOR_PERF_BUDGET_SCALE=2`. Scratch dirs and the agent's `CARGO_TARGET_DIR` go under `~/.cache/razdor-scratch/sharp-pixels/`.
- Work in an agent worktree (Agent `isolation: "worktree"`). Commit there, and **do not push**: the push waits for the owner's approval of the screenshots (Task 5).
- Shaders: `#version 100`. Any shader doing texel math declares `precision highp float;` inside `#ifdef GL_FRAGMENT_PRECISION_HIGH` (else `mediump`), with highp varyings for uv. `lowp`/`mediump` uv breaks on textures over ~1000 px (the credits scroll is 455×2840).
- Code, comments and commit messages are in English; the changelog and wiki ru page are in Russian.
- Never commit game-derived images. Screenshots stay outside the repo (scratch, the Discord post).

## Review Focus

1. **Scale below 1 (minification)**, e.g. a 92 px portrait drawn at 60 px on a phone: `sharp_uv` must clamp scale to ≥ 1 and give plain bilinear, with no NaN or inverted clamp. Test `sharp_uv_minified_is_plain_bilinear` (Task 1).
2. **Atlas sub-rectangles** (`source: Some(rect)`): the scale is `dest_size / source.size`, not `dest_size / texture size`. Test `scale_uses_source_rect` (Task 1).
3. **Batching:** `set_uniform` and material switches break macroquad batching (quad_gl.rs 870, 963), so every sharp draw is its own draw call. A big map's world view with many objects must not get noticeably slower. Measure in Task 4 with `RAZDOR_PROFILE` on the biggest community map; if frame time rises by more than 10 %, cache the last uniforms and skip `set_uniform` when they're unchanged.
4. **Flipped and rotated draws** (`flip_x`, `rotation`, `chrome::tex_src_flipped_x`) keep working. Scale uses the absolute dest size. Test `scale_ignores_flip_sign` (Task 1).
5. **Shader compile failure** on an old GPU or driver: `sharp::draw` falls back to plain `draw_texture_ex` (today's look), logs once with `razdor::diag!`, and never panics.

---

### Task 1: `sharp` module: GLSL function, material, `draw`

**Files:**
- Create: `src/ui/sharp.rs`
- Modify: `src/ui/mod.rs` (add `pub mod sharp;` in alphabetical order)

**Interfaces:**
- Produces:
  - `pub const SHARP_UV_GLSL: &str`: GLSL source defining `vec2 sharp_uv(vec2 uv, vec2 tex_size, vec2 scale)`. It is pasted into other fragment shaders with `concat!`/`format!`, so it is a plain function with no `#version` and no precision line.
  - `pub fn draw(tex: &Texture2D, x: f32, y: f32, color: Color, params: DrawTextureParams)`: the same signature and behaviour as `draw_texture_ex`, but with sharp sampling.
  - `pub fn draw_scale(tex_size: Vec2, params: &DrawTextureParams) -> Vec2`: the per-axis scale for the uniform. Source size is `params.source.size()` or `tex_size`; dest size is `params.dest_size` or the source size; the result is `abs(dest) / source`.
  - `pub fn set_uniforms(m: &Material, tex: &Texture2D, params: &DrawTextureParams)`: sets `TexSize` and `Scale` on any material that includes `SHARP_UV_GLSL` (used by Task 3).
  - Uniform names: `TexSize` (Float2), `Scale` (Float2).

- [ ] **Step 1: Write the failing tests** in `src/ui/sharp.rs` `#[cfg(test)] mod tests`, against a Rust mirror `fn sharp_uv_ref(uv: Vec2, tex_size: Vec2, scale: Vec2) -> Vec2` that follows the GLSL line for line (keep it next to the const with a comment "must match SHARP_UV_GLSL"):
  - `sharp_uv_texel_centre_unchanged`: for tex 4×4 and scale 3, uv at a texel centre (0.375, 0.625) returns the same uv.
  - `sharp_uv_inside_texel_snaps_to_centre`: for tex 4×4 and scale 3, uv at texel (1.2, 2.2), i.e. uv (0.3, 0.55), returns the centre of texel (1, 2): (0.375, 0.625).
  - `sharp_uv_edge_band_blends`: for tex 4×4 and scale 3 (blend band ±0.5/3 ≈ 0.167 texel around each texel edge), texel x = 2.05 (0.05 past the edge at 2.0) gives a texel coordinate strictly between 1.5 and 2.5, and x = 2.3 gives exactly 2.5.
  - `sharp_uv_minified_is_plain_bilinear`: for scale 0.6 (and scale 1.0), the result == the input uv, with no NaN.
  - `scale_uses_source_rect`: `draw_scale(vec2(512,512), source 64×32, dest 192×96)` == (3, 3).
  - `scale_ignores_flip_sign`: a negative dest size gives a positive scale.

- [ ] **Step 2: Run them; they fail** (functions missing).
  `CARGO_BUILD_JOBS=12 nice -n 19 taskset -c 8-19 cargo test --release sharp`

- [ ] **Step 3: Implement.** The GLSL (`s = max(scale, 1.0)`):
  ```glsl
  vec2 sharp_uv(vec2 uv, vec2 tex_size, vec2 scale) {
      vec2 s = max(scale, vec2(1.0));
      vec2 texel = uv * tex_size;
      vec2 base = floor(texel - 0.5) + 0.5;      // centre of the texel to the lower-left
      vec2 f = texel - base;                      // 0..1 between two texel centres
      vec2 t = clamp((f - 0.5) * s + 0.5, 0.0, 1.0);  // sharp step, width 1/s texel
      return (base + t) / tex_size;
  }
  ```
  The material is a vertex shader like `chrome::ADD_VERTEX` but with `highp` varyings when available. The fragment shader is `gl_FragColor = color * texture2D(Texture, sharp_uv(uv, TexSize, Scale));`. It is loaded once (thread-local `OnceCell<Option<Material>>`, the same pattern as the materials in `chrome.rs` around line 1043), with the default (alpha) blend pipeline. `draw` does `gl_use_material`, `set_uniforms`, `draw_texture_ex`, `gl_use_default_material`; on a compile error it falls back as in Review Focus 5. Make the Rust mirror use the same formula, and fix the test expectations to it if the step width maths differ by float rounding (compare with 1e-5).

- [ ] **Step 4: Tests pass**, the same command, plus `cargo clippy --release` clean (same prefix).

- [ ] **Step 5: Commit** `src/ui/sharp.rs src/ui/mod.rs`: "Sharp pixels: the sharp-bilinear sampler and sharp::draw".

### Task 2: Route the plain art draws through `sharp::draw`

**Files (modify):** `src/ui/chrome.rs` (`tex` 1378, `tex_src` 1382, `tex_src_flipped_x` 1386, `tile` and `three_slice` if they call `draw_texture_ex`, lines 1642/1736/1747 if they draw DT art), `src/ui/world_view.rs` (1113, 1319, 1362, 1366, 1491, 1823, 3225), `src/ui/battle_view.rs` (522, 1423), `src/ui/items_view.rs` (494), `src/ui/dialog.rs` (1103), `src/ui/cursor.rs` (214, if the cursor is DT art), `src/ui/hub_view.rs` (811, 910; only if they draw map previews from DT art), `src/ui/assets.rs` (135, 147, 162, 233, 252; only the ones drawing DT pictures).

**Not touched:** `minimap.rs`, the fog/mask draw at `world_view.rs:1073`, `dt_font.rs`, `script_font.rs`, `editor/*`, and every texture whose filter is `Nearest`. Read each call site and decide by what it draws; when unsure, skip it and list it in the report.

**Interfaces:** Consumes `sharp::draw` (Task 1).

- [ ] **Step 1:** Replace each kept call `draw_texture_ex(t, x, y, c, p)` with `sharp::draw(t, x, y, c, p)`. Change nothing else. Textures created with `FilterMode::Linear` stay Linear.
- [ ] **Step 2:** `cargo build --release` and `cargo clippy --release` are clean, and `cargo test --release` passes (with `RAZDOR_DT_DIR` from `.env`, unquoted, and `RAZDOR_PERF_BUDGET_SCALE=2`).
- [ ] **Step 3:** Snapshot check (the `offscreen-snapshot` skill: Xvfb, its own `XDG_RUNTIME_DIR`, pinned to CPUs 8–19). Render the world map, a battle, a town and the main menu at 1920×1080 and 1366×768, zoom in on a unit and a building, and confirm the edges are crisp and even. Save the pictures under the scratch dir.
- [ ] **Step 4: Commit** "Sharp pixels: DT art drawn with sharp bilinear".

### Task 3: The special shaders sample sharp too

**Files (modify):** `src/ui/chrome.rs` (the `ADD_FRAGMENT`, `SILHOUETTE_FRAGMENT` and `GREY_FRAGMENT` shaders, `GROUND_RING_FRAGMENT` if it samples art, and their draw functions), `src/ui/terrain.rs` (`FRAGMENT`: the atlas and water sampling).

**Interfaces:** Consumes `SHARP_UV_GLSL` and `sharp::set_uniforms` (Task 1).

- [ ] **Step 1:** In each fragment shader that samples DT art, add the precision header from Global Constraints, insert `SHARP_UV_GLSL`, add the `TexSize`/`Scale` uniforms to its `MaterialParams`, and sample with `sharp_uv(...)`. In each draw function that uses those materials, call `sharp::set_uniforms` before drawing. Leave `SUBTRACT_TEXT_FRAGMENT` (text) alone.
- [ ] **Step 2: Terrain.** The atlas is sampled at a computed position within a padded slot. Apply `sharp_uv` with `tex_size = AtlasSize` and `scale = screen px per texture px` (derive it from the existing `CellPx` and the dest size, or add a `Scale` uniform set in `TerrainLayer::draw`). Do the same for the water texture with its own size. Check at an uneven zoom (e.g. 1.5×) that no seams appear between cells. Snapshot before and after.
- [ ] **Step 3:** Build, clippy and tests are clean, as in Task 2 Step 2. Snapshot a battle with a spell effect (additive), the silhouette (a unit behind an object) and a greyed-out picture.
- [ ] **Step 4: Commit** "Sharp pixels: additive, silhouette, grey and terrain shaders".

### Task 4: Performance, docs, changelog, wiki

**Files:** `docs/flows/assets-and-drawing.md` (a short "Sharp pixels" paragraph under "Drawing helpers": `sharp::draw` for DT art, what stays smooth, the highp rule), `changelog/unreleased.md`, the wiki pages (the `wiki` skill, ru + en, the page about differences from the original or graphics).

- [ ] **Step 1: Perf.** Run `RAZDOR_PROFILE` (offscreen-snapshot skill) on the world view of the largest map in `/mnt/ai_models/razdor-community/` before (on `origin/main`) and after. Report the frame time and draw-call count. If frame time rises by more than 10 %, add the uniform cache from Review Focus 3, then re-measure.
- [ ] **Step 2:** Add the docs paragraph. Changelog line (Russian, player words): `- Графика теперь чёткая: пиксели оригинальных картинок не размываются при увеличении и одинакового размера при любом масштабе.`
- [ ] **Step 3:** Wiki note via the `wiki` skill (ru + en). No screenshot is needed.
- [ ] **Step 4: Commit** "Sharp pixels: docs, changelog, wiki", including this plan file (`docs/superpowers/plans/2026-11-05-sharp-pixels.md`).

### Task 5: Owner approval, regression checks, push

- [ ] **Step 1:** UI regression check: `python3 -I tools/ui-regress/ui_regress.py run` (pinned). Every scene with DT art is expected to differ. Review the diffs for anything other than sharper pixels (shifted layout, missing art, seams, black boxes).
- [ ] **Step 2:** Mobile check (the `mobile-ui` skill) on two phones, portrait and landscape. Nothing in sizes should change; confirm there's no new text under 12 sp and no new touch target under 40 dp.
- [ ] **Step 3:** Old-vs-new screenshot pairs: world map, battle, town, main menu, unit panel and item window on desktop, plus world map and battle on both phones. Post them to the Discord reports channel (`reports_post`, preview first, then `confirm=true`; the owner asked for this) with a Russian note. **Wait for the owner's yes.**
- [ ] **Step 4:** After the yes, accept the changed UI-regression references, then `git fetch && git rebase origin/main`, the full tests, and push to `main` (the CLAUDE.local.md push recipe as indicozy).
- [ ] **Step 5:** Clean up: the worktree and its branch, the scratch dir `~/.cache/razdor-scratch/sharp-pixels/` and the spike's `~/.cache/razdor-scratch/hd-upscale/`, the Xvfb processes, and `cargo clean --profile dev`.
