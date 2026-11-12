# Handoff: map sprites in a static GPU buffer (backlog 1605, new step 5) — 2026-11-12

Status: **plan only, not started, waiting for the owner's go.** Ships after 1.5.0, on branch `gpu-sprites`,
merged by a PR once every stage is done and tested (the owner's exception to plain trunk). Don't start builds or
off-screen runs while the owner plays.

## Where things stand

- Steps 1+2 (sort once, chunks + heap merge): dropped, slower. Local branch `dropped/1605-steps-1-2`.
- Step 3 (conservative cull before the picture quad): **never built**. Ceiling 0.6–1.2 ms zoomed out. Fallback only.
- Step 3a (view-projection once per frame): dropped, rounds disagree. Local branch `dropped/1605-step-3a-viewproj-cache`.
- Step 4 (fog and reveals from a cell texture in the sharp shader, `ui/sprite_fog.rs`): on main `741e3f8f`. No gain
  alone; the static buffer below needs it (vertices no longer depend on the fog).
- Step 5 stage 1 (placement harness, `src/ui/sprite_gpu.rs`, `world_view/gpu_placement_tests.rs`): on `gpu-sprites`
  `2a8e4230`; 2.6 M pairs, worst 0.0027 px.
- Step 5a-i (per-run meshes rebuilt every frame, behind `RAZDOR_GPU_SPRITES=1`): on `gpu-sprites` `b1867e21`, **parked,
  slower** (zoom 0.05 2.5D world pass 4.2–4.6 → 5.4–6.2 ms, draws 173 → 221). Cause: meshes refilled every frame and
  runs cut short by buildings and figures. Full numbers: `docs/perf/2026-11-12-atmosphere-performance.md`, section
  "stage 5a-i"; ledger row under Dropped.
- Pending: re-time 5a-i once on a quiet machine (scripts and builds kept in
  `/mnt/ai_models/razdor-scratch/sprite-gpu-5a/`, `timing/run_timing.sh`, 12 GB; delete after).

## The idea (the owner, 2026-11-12)

The pictures are already on the GPU (the map atlas). Trees, rocks, plants and buildings never move in the world. So
upload their corners once, already in draw order, and per frame send only the camera. The CPU stops doing per-sprite
work for everything static.

Why the order can be fixed: `sprite_key` (`src/ui/world_view.rs:4455`) is
- not turned (flat, or 2.5D with yaw 0): `cell_pass_key(tile, layer)`, independent of the camera;
- turned: `-(floor(world_foot · forward(yaw) · 64)) · LAYERS + layer`, with `world_foot = (foot + cam.origin) / cam.scale`,
  so it depends on the yaw only, not on pan or zoom.

So the static order changes only on: rotation (yaw), turned ↔ not turned, map load, end of turn / fog change that adds
or hides sprites (if hidden sprites are left out of the buffer; alternatively keep them in and let the fog texture
hide them), and a building built, razed or changing picture. **First task: a test proving the keys of static items are
equal across pans and zooms at a fixed yaw** (float rounding of `(foot + origin) / scale` may flip a 1/64 bucket; if it
does, compute the key from the world foot directly, as 5a-i had to do for the fog cell).

## Design

1. **Static buffer.** One vertex + index buffer per (map, yaw-or-flat, layer set) holding every `Drawable::Object`
   and `Drawable::Building` in sorted order, packed as in 5a-i (`deco_packing`: foot in world units, corner offset,
   fog cell, texel; `SHARP_VERTEX_BILLBOARD` in `src/ui/sharp.rs`). Keep each item's sort key and its index range.
2. **Per frame.** Build only the moving items (`Army`, `Ship`, `Hero`, `Spell`) with their keys, sort them (a few
   dozen), and binary-search each into the static key array. Draw: static range [0, k1) → figure 1 → static range
   [k1, k2) → … → last static range. One draw call per figure on screen plus one per gap.
3. **Ranged draws.** Macroquad's `Mesh`/batcher can't draw part of a buffer. Use miniquad directly
   (`get_internal_gl()`, a `Bindings` with the static vertex/index buffers, `apply_pipeline`, `draw(base_element,
   num_elements, 1)`), flushing macroquad's batch before each range. GLES2 only: 16-bit indices, so split buffers at
   65,536 vertices (16,384 sprites; the whole «Другой берег» has ~12,800).
4. **Rebuild on rotation.** While the yaw changes, rebuild every frame (about today's cost); when it stops, the buffer
   is reused. Optional later: rebuild only after the yaw settles for one frame.
5. **Moved into the shader** (uniforms + per-vertex data, each proven picture-identical):
   - detail fade at 2–3 px (`detail_visibility`): per-vertex size, visibility from the zoom uniform, same 8-bit
     truncation as the CPU;
   - LOD copies (`decoration_lod`): both texel rects per vertex, choose by zoom (half/quarter below 12/6 px a cell);
   - tree sway (`draw_leaning`, 5b): phase and amplitude per vertex, time uniform; `ctx.anim == 0` keeps it still;
   - sun colour per corner (`form_colour_in`) and the lit material (`LIT_MAIN`, `ShadowK`), night whole-sprite
     switch (`lit_whole`): uniforms per frame, colour baked per corner where it depends on the sprite only;
   - fog: already a texture (step 4).
   Anything not yet in the shader keeps today's CPU path for that frame's case (as 5a-i did), so stages can land one
   at a time behind `RAZDOR_GPU_SPRITES=1`.
6. **Culling.** None: the GPU clips off-screen sprites. Picks (`PICKS`, `front_pick`) are untouched (decorations are
   never picked; buildings' pick records still come from the CPU, which keeps their quads).
7. **Out of scope at first:** bent views (trailer `bend`, first person, eyes view: per-sprite near fade and
   magnification). They keep the CPU path (5c later, if at all).

## Stages and proof (each one)

| Stage | Content | Est. |
|---|---|---|
| S0 | Key-stability test (pan/zoom at fixed yaw), miniquad ranged-draw spike drawing one static range | 0.5 day |
| S1 | Static buffer for decorations + buildings, figures interleaved, simple case only (as 5a-i's eligibility) | 0.5–1 day |
| S2 | Detail fade + LOD copies in the shader (unlocks the whole-map case) | 0.5 day |
| S3 | Sway, sun colour, lit/night | 0.5–1 day |
| S4 | Timing on a quiet machine, phone check (Samsung budget phone via the benchmark button), PR | 0.5 day |

Proof per stage (same bar as 5a-i): the 96-PNG matrices explored and partly explored
(`/mnt/ai_models/razdor-scratch/perf-retain/matrix.sh`, 48 runs × 2 frames), mid-run reveal, `RAZDOR_PICK_DUMP=1`
digests, max 2 levels per channel; timings interleaved, two rounds, RTX 4060 only (find its card by PCI
`0000:05:00.0`), base = `gpu-sprites` without the env switch. Stop and report if S1 shows no gain at zoom 0.05 2.5D:
that is the deciding measurement.

Expected gain: zoomed out, most of the 4 ms (zoom 0.05) to 12–18 ms (whole map) world-pass CPU work, 2.5D on the 4060;
largest on weak phones.

## Rules for whoever picks it up

- Agents on Haiku, each in its own worktree; narrow brief per stage (they stop near ~260k context).
- `CARGO_BUILD_JOBS=12 nice -n 19 taskset -c 8-19`; CPUs 0–7 and the RTX 5060 Ti are the owner's.
- Jobs write a done-file; poll in the foreground; never end a turn while a job runs.
- `cargo test --profile fasttest <module>::` while iterating; `cargo test --release` with `RAZDOR_DT_DIR` and
  `RAZDOR_PERF_BUDGET_SCALE=4` before the push; clippy `-D warnings`; Android check
  (`cargo check --release --target aarch64-linux-android --bin razdor`).
- Every measured idea gets its row in `docs/perf/ideas-ledger.md`; backlog 1605 gets a dated line per stage
  (`/mnt/ai_models/razdor-reports/1605/report.md`, status now `parked`).
- Message the owner on Telegram at each stage result.
