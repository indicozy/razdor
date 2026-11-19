# Improved map ground (experimental, off by default): decisions, 2026-11-19

Status: decision document. Nothing is built. Sources: three scout reports (ground pipeline, objects, techniques; the
techniques report made no web lookups, so none of its links is cited here), checked against the code on `main`
(40035ca1). Line numbers are the ones read on that commit.

The request (a map maker, Discord): the adventure map's ground should look hand-painted: narrow winding paths, soft
organic field edges, ponds with natural shores, streams, ground continuing over hills, meadow detail. The owner's
frame: one experimental setting, off by default, engine only, no change to `.DTm` (one surface code per cell).

## 0. The owner's answers (2026-11-19, these override the sections below)

- F1 natural edges: yes, prototype and show (with the shore limit rendered at 0.15 and 0.25).
- F2 winding paths: test. F3 meadow tone: show a picture. F4 hill feet: render the probe and show.
- Thin streams: implement (overrides the rejection in §1; question 2 is answered (b)); show them for review.
- New: cloud shadows, soft "cow's skin" light and dark patches drifting over the map (a pass over ground and objects).
- New: more map content from DT's own small sprites (flowers, bushes, reeds, stones) placed by the engine, draw-only;
  a probe catalogues the candidate sprites first.
- Grass: no drawn blades on top of the texture. The wish is the grass texture itself varied and moving with the wind
  (backlog 1602); after F1/F3.
- Contact shadows: later, once the assets are split into chunks (backlog 1604).
- Tilt-shift and colour grading: shown again from the old branch, no decision yet.
- Edges look picked (12:48): the "middle row" = edges with shore limit 0.25 and tone 0.15, ramp H 0.5. One settings
  row for the edges: «Улучшенная земля» Выкл / Мягко (edges, H 0.5) / Сильно (edges, shore 0.25, tone 0.15), off by
  default. Cloud shadows: 25% dark, "like Minecraft cloud shadows".
- Clouds (13:02): the default look is best (block 1.0, scale 5, dark 0.25, soft 0.15, blue tint); slow drift when
  still, faster while the hero walks.
- Paths/streams first try (d82d5f6d, §2.2's in-quad design) REJECTED by review: centrelines can't bend (the edges
  warp is zero on the quad sides, a quad sees 4 cells), 1/32-cell edges look like circuit traces, staircases stay
  steps, a dark patch beside narrowed roads, streams a flat band. Redo: CPU-baked road graph at map load, Chaikin-
  smoothed branches, rasterised distance texture, soft edges; streams second as another channel of the same bake.
- Road bake (branch worktree-agent-a8f52ba220fce7246, 4bd1035f): tests green, 40 real maps pass the invariants, 200×200
  bakes in ~40 ms at 8 texels/cell (1600 px). Dumps show staircases still wobble and turns barely round: the 0.22
  road-centre limit is below a staircase's 0.354 offset from its diagonal. Next: allow road centres up to 0.36 from the
  line and draw the path at half-width ≥ 0.38 (grass centres stay ≥ 1.0 away on staircases); soften the edges of fill
  blocks; add a map-size guard before baking (dtm allows 8192², ~4 GB at 8 texels).
- Owner, 15:20: add noise/randomness to the textures themselves (grass and the other land). Planned after the paths:
  per-cell anti-tiling (a hashed offset/flip of the texture per cell, blended across cell borders without seams) and
  fine brightness/hue noise at 1–3 cells, on land surfaces only, baked into the ground cache.
- Shipped next (16:19): winding paths in Сильно from the CPU road bake (road-centre limit 0.36, non-road 0.50, up to
  4 Chaikin passes, fills with a soft edge, maps over 1024 cells skipped), a `Paths` texture (7 samplers of 8), edge
  softness 0.45 and half-width 0.55 (the owner's pick: as soft as today's road blend), a bilinear "safe" ramp beside
  blocked ground; anti-tiling (`NAT_DETAIL`) in Мягко and Сильно, fine grain in Сильно. The anti-tiling's visible
  effect is small on DT's dense maps. Open: the inner corner of a 90° turn on a one-cell road stays angular.
- Wind in the grass (`LIVE_GRASS`, backlog 1602): prototype on a branch, env `RAZDOR_NATURAL=…,wind`; per-frame
  overlay over grass like live water; a fixed east direction for now; the owner to choose its direction source.

## 1. Decision summary

Ships, in this order, all inside the ground shader (`src/ui/terrain/neighbours.glsl`), each in its own `#if` block:

| # | Feature | Answers | Effort | Needs the owner's fresh yes |
|---|---|---|---|---|
| F1 | Natural edges: the border between two ground types follows a slow curve, coasts and road edges included | field edges, pond shores, banks | S–M | the idea has the owner's yes as the first step (2026-11-19, relayed by the coordinating session); the look still needs the screenshot yes. It must be visibly stronger than the dropped «Мягкие переходы» (§2.1) |
| F2 | Winding paths: a 1-cell road is drawn as a path about half a cell wide through its cells' centres | narrow winding paths | M | no (new) |
| F3 | Meadow tone: large soft lighter and darker patches on grass, sand and soil | meadow detail (part) | S | no (new) |
| F4 | Hill feet: a probe first, then a decision | ground over hills | probe S, build M | decided after the probe |

Not shipped:

- Thin streams (water drawn narrower than its cell). All three water codes are blocked on foot and shallows are a
  ship lane (world.md, the terrain table: code 0 "blocked" on foot, 2 by ship). A thin stream would read as
  crossable and would hide the lane. Streams get F1's natural banks only; the water keeps its cells.
- Joining diagonal water cells into one stream. The original has no corner-cutting rule: a diagonal step between two
  blocked cells is allowed (0x482a58), so two water cells that touch at a corner are a real crossing and must not be
  drawn closed. F1 leaves the corner exactly as open as today's blend.
- Procedural scatter meshes and contact shadows (`gfx-scatter`, 20e8ebd4): look declined (ledger row 2026-11-12).
  Contact shadows have their own backlog entry (1604); they are not part of this setting.
- Tilt-shift, colour grade, water depth (sand tint, deep darkening, wet band), «Ультра» (`gfx-post-dropped`,
  506d6894): look declined. Not revived. F3 is a ground-texture variation, not a screen grade (§2.3).
- Relief or height shading of the ground: removed by the owner (ledger 2026-11-11, "relief (groove) shading removed").
- Spline centrelines, SDF mask textures, a per-cell "feature" texture, chunk baking: not needed. F1 and F2 work from
  the four codes the shader already has (`k`, neighbours.glsl:188) plus one baked byte per cell in a free channel of
  an existing texture (§2.2), so the texture count stays 6.
  The ground cache already draws the ground once per still frame.
- Wang or corner tiles (new art), stochastic tiling (3 fetches a layer, breaks structured textures), building cards
  (parked), per-object tone jitter and wind (objects; outside this request).
- Flower dots: not in the plan. They are scatter in another form; question 4 asks whether the owner wants a trial.

Scout findings corrected here: the terrain scout's first step (noise-warped borders plus a wet shore band) repeats
two looks from `gfx-post-dropped` («Мягкие переходы» and the wet band of «Глубина воды»); the wet band is dropped,
the borders are redesigned and flagged. Its road and stream plan (a baked connection mask in a seventh texture) is
replaced by a quad-local rule with no new texture.

## 2. Features

Every block compiles into all three kinds of variant alike (ground, overlay, full): the overlay redraws the land
pixels of wet quads over the cached ground, so each feature's code must give the same colour there as in the ground
pass, or a seam shows around every pond.

Common facts (checked): the ground is one quad; a pixel at cell position `p` reads the four codes of the cells whose
centres surround it (`k`, :188) and blends four atlas fetches by the plain fractions `fx`, `fy` (:197–198, the blend
at :259–264). When the four codes are equal it takes one fetch (:259). The ground pass (`Mode::Ground`) is cached and
scrolled; the overlay redraws wet pixels over it; a moving, tilted or bent view draws `Mode::Full` every frame. The
same shader serves every view, in cell units, so each feature works in the flat, 2.5D and bent views unchanged.
Noise comes from the `Noise` texture (64×64, bound for every draw at terrain.rs:1043), read with highp coordinates;
no `sin` hash (poor in mediump), no clock, world position only, so the scroll cache stays exact.

### 2.1 F1 Natural edges

Look: where two ground types meet, the border line bends slowly (about one bend per two cells) instead of running
along the cell edge, and the transition is narrower than today's cell-wide ramp. Fields, ponds, river banks and the
outer edge of roads all get it.

How: inside the quad, bend each fraction before the blend with a bias curve whose midpoint comes from the noise:
`f' = f / ((1/t − 2)(1 − f) + 1)`, `t = 0.5 + S · n(p)` per axis, `n` in −1..1 from two `Noise` fetches (one per
axis) at about 0.45 cycles per cell. The curve keeps 0 at 0 and 1 at 1 and is monotonic for any `t` in 0..1; it
moves the 0.5 line (the visible border) to `f = 1 − t`, that is by `S` cells. Then narrow the ramp:
`smoothstep(0.5−h, 0.5+h, f')` with `h` per pair (tuned in milestone 1; start 0.25). `k` is not fetched again and
is not warped. The all-equal fast path stays first, so the inside of a field costs nothing extra.

What differs from the dropped «Мягкие переходы» (`gfx-post-dropped`, terrain.rs diff; in the side-by-side from
that branch's review (local, not in git; a 2.5D scene of a town on a river) the two halves cannot be told apart at
normal zoom, since every border in view is a coast or a road, which it skipped): that one
sharpened the weights 2.5× and added ±0.45 of noise at 1.7 cycles per cell, which only roughened the old straight
line over a few pixels; it skipped every quad that held water or a road; it was a uniform in every shader, used
`vnoise` (the `sin` hash) and sat in the «Среднее» and «Высокое» presets. F1 moves the line itself by up to 0.3
cell (about 10 px of a 32 px cell) as a slow curve, covers coasts and road edges, is a compile-time block, and is
in no preset. Milestone 1's pictures must show a difference that is plain at normal zoom; if they do not, raise
`S` to its limit before showing them.

Reused from that branch: the shape of the plumbing only (the settings field and its row, the help text's wording,
the dev switch in `follow_settings`, the flow-doc paragraph, the wiki lines in `differences/map-look.md`). Not
reused: the weight formula, the `Soft` uniform (a define block instead), `vnoise`, the preset entries, and its
switch of the unlit path (that legacy shader no longer exists on `main`).

Truthfulness guard (the rule every later feature keeps): the warp maps each quad onto itself and is zero on the
quad's sides (see the last limit), so a border can move only inside the quad between four cell centres, no code from a fifth cell can
appear, nothing can vanish and nothing can join. Limits:

- `S` is a continuous function of the pixel, never a per-quad choice (a per-quad choice would step the border by
  up to 5 px on the side two quads share). Let `c` be the blocked share at the pixel: the plain bilinear blend
  (unwarped `fx`, `fy`) of 1 for the blocked codes (0, 1, 2, 3, 9, 15) and 0 for the others. Then
  `S = mix(0.30, 0.15, smoothstep(0.0, 0.15, min(c, 1 − c)))`. `h` is one constant.
- Away from any passable/blocked border (`c` near 0 or 1) the border moves at most 0.30 cell; a square of ±0.2 cell
  around a cell's centre always shows the cell's own surface.
- Along a passable/blocked border the line moves at most 0.15 cell (5 px of a 32 px cell): a move of more would
  need `S > 0.15`, which holds only where `min(c, 1 − c) < 0.15`, that is at least 0.35 cell from the border, and
  `S` never exceeds 0.30. So a square of ±0.35 cell around a cell's centre always shows that cell's own class. This
  argument is for a straight border; the corner patterns are checked by the test below, and `S` is lowered if one
  fails. Shores get their visible change mostly from the curve's length (one bend per two cells) and the narrower
  ramp; question 1 asks whether the shore limit may go up.
- What is fixed on a quad's sides: `fx'` is 0 and 1 on the two vertical sides, `fy'` on the two horizontal ones;
  the other fraction is a function of `p` alone (the noise and `c` are continuous), so it is the same from both quads.
- The diagonal corner between two water cells keeps today's saddle (the warp does not close it).

Water and the cache: `wet`, the overlay's `discard` and `overlay_can_draw` (terrain.rs:227) depend on `k` only, and
`k` is unchanged, so they need no change. The ground pass and the overlay must compute `f'` with the same code (one
shared function), or a seam shows at the waterline. The foam's `cov` (:271) uses `fx`, `fy` and so follows the new
shore by itself. The live water's sky tint uses the unwarped mask (`fd.a`); a mismatch of at most 0.15 cell at 18%
strength is accepted, to be checked in the snapshots.

Cost: 2 noise fetches plus a little arithmetic, only in quads with mixed codes. Phones: on (the ground is cached);
the walk and scroll runs of the perf suite decide. Texture units stay at 6 of 8.

Plug-in: neighbours.glsl:197–198 (the weights), a `NAT_EDGES` block with its own noise reader (`wnoise` at :76 is
inside `LIVE_WATER`; do not move it, the default text must not change). `Variant` (terrain.rs:709) gains the flag;
`Variant::key` (:726) has two free bits of its `u8`, so widen it to `u16` (the list at :603 is a `Vec` of pairs, any
key width works). `GroundCache` keeps the flags its picture was drawn with and redraws on a change, as `cheap` does
(:509, :916–918).

Risk: medium (look; declined family). Effort: S–M.

Verification:
- A test that pins the default: the preprocessed text (the tests' `preprocess`, terrain.rs:1530, which drops the
  blocks whose define is 0 and the define lines) of every existing variant (64 keys, :1594) hashed before the
  change, the same hashes after it with the new flags off. The new defines go after the existing five
  (`every_variant_text_is_fully_assembled` checks that the text starts with the `LIVE_WATER` define).
- A Rust mirror of the warp with tests for the limits above (range kept in 0..1, monotonic, ends fixed, the
  the largest move of the 0.5 line). A sweep over all code patterns of two adjacent quads: the four colour
  weights agree on the shared side; and over all 16 blocked patterns of a quad: no blocked class inside ±0.35 cell
  of a passable centre, and the reverse.
- `the_ground_pass_holds_the_land_only` (terrain.rs:1566) stays as is for the flags-off variant.
- Off state: snapshots byte-identical to `main` (the precedent in the ledger), and the UI regression check
  (`python3 -I tools/ui-regress/ui_regress.py run`) with no diff.
- On state: off-screen A/B snapshots (the `offscreen-snapshot` skill) of a field edge, a pond, a 1-cell river, a
  coast with live water on and off, in the flat view, 2.5D, and one phone size; `RAZDOR_SCROLL_CHECK=1` with the
  feature on (scrolled cache equals a fresh fill within 2 levels).

### 2.2 F2 Winding paths

Look: a road one cell wide becomes a path about half a cell wide that runs through the centres of its cells and
bends with F1's warp; the neighbouring ground comes up to the path on each side. Wide roads and squares stay filled.

How (from the quad's four codes): in a quad with at least one road code (4) and at least one other code:
- the path is the set of pixels within half-width `w` (0.25 cell ± a slow noise of 0.04) of: each quad side whose
  two ends are road; the diagonal, only when exactly its two ends are road; a disc at each road centre. Distances
  are taken at the warped position, with a smooth union so corners are round;
- outside the path the pixel shows the blend of the quad's non-road codes, their weights renormalised (continuous
  across quads: on every line where it could jump, the pixel is under the path);
- a quad of four road codes is the fast path: filled as today (2×2 blocks and squares keep their look).

Truthfulness guard:
- A road cell's centre is always road; a non-road cell's centre is never road (the nearest path line is at least
  0.5 cell away and `w ≤ 0.3`).
- Beside blocked ground the road is not narrowed, and the switch is continuous, not per quad (a per-quad switch
  leaves a seam through the road cell's centre at every turn or dead end next to water). One baked byte per cell,
  "safe": 255 when no cell of its 3×3 block has a blocked code (0, 1, 2, 3, 9, 15), else 0. It goes into the free
  blue channel of the `Bank` texture (`bank_texels`, terrain.rs:466; Linear, already bound for every draw at
  :1044), so one fetch at the pixel gives a smooth `g` from 0 to 1, and the colour is
  `mix(today's blend, the narrowed picture, g)`. `g > 0` only in a quad that holds a safe cell, and such a quad has
  no blocked code, so blocked ground is never painted into a road cell; a road along water, lava or swamp widens
  back to its full cell over about half a cell. Bridge ends are covered by the same rule. This is the one piece of
  baked data in the plan; the texture count stays 6.
- Diagonal links are drawn because a diagonal step is a real step in the original (weights table 0x4ecfd4).
- The margin of a road cell shows grass but walks at road speed. That is a small error in the player's favour;
  the route line and the step time are unchanged. Accepted.

Edge and zoom: the path's edge is a fixed ramp one texture pixel wide (1/32 cell); there are no derivatives in
`#version 100`. A path a few screen pixels wide would shimmer when zoomed out and on the far ground of the tilted
and bent views (drawn uncached every frame), so the narrowing fades with the pixel size: a new uniform, the screen
pixels per cell at the view's nearest row (for the flat view the camera's scale), scales `g` to 0 below about 12
px per cell. A tilted view uses one value for the frame; if the far rows still shimmer in the milestone's 2.5D
snapshots and walk recording, F2 is flat-view only.

Cost: one `Bank` fetch and arithmetic (up to 5 segment distances), in quads that hold a road. Phones: on, same
condition as F1.

Plug-in: neighbours.glsl, a `NAT_PATHS` block that replaces the blend at :259–264 for such quads; `bank_texels`
gains the safe byte (a pure function, with a test); one uniform added to `UNIFORMS` (terrain.rs:168).
Requires the blend path: with «Смешивание земли» off (`CHEAP_GROUND`, :255) the whole setting is inert.

Risk: medium. Known ugly cases to tune on real maps: staircase roads (drawn as rounded steps, not a diagonal), a
road with grass on one side and water on the other (thin on one side, full on the other), roads ending at a town.
Effort: M.

Verification: a Rust mirror of the path mask with tests for the two centre rules, the blocked-code rule and the
agreement of adjacent quads on their shared side (all patterns of road, land and blocked); A/B
snapshots of a straight road, a turn, a staircase, a junction, a dead end, a road along a shore, a town's approach,
on the campaign and on two community maps with wide roads (Community Update, Evolution); the F1 checks again.

### 2.3 F3 Meadow tone

Look: grass, dry plain, sand, clay and stony soil get broad soft patches a few per cent lighter or darker and a
little warmer or cooler (period 8–20 cells), so a big field stops looking like one repeated tile.

How: one `Noise` fetch at low frequency, made once in `main` (not inside `ground()`, which runs up to four times
a pixel), multiplies the land colour by 1 ± 0.05 with a slight hue shift. Never on
water, lava, road or snowdrifts. No dots, no blades, no meshes, no sway: this is not the declined scatter, and it is
part of the ground texture, not a screen grade.

Cost: one fetch on every land pixel, the fast path included. This is the only feature that touches every pixel, so
its phone policy is strict: it ships on phones only if the walk and scroll runs on the budget phone show no
regression; otherwise it is compiled out when `razdor::platform::MOBILE` is set and the row's help says so.

Plug-in: `main` (neighbours.glsl:172), after the blend, a `NAT_TONE` block; the land share of the pixel (from `k`
and the weights) scales it, so water, lava and road pixels are untouched. Truthfulness: 5% tone cannot change which surface a
cell reads as; checked by eye in the snapshots. Risk: low. Effort: S.

Verification: the F1 off-state checks; A/B snapshots of a large field at three zooms; a row in `docs/perf/ideas-ledger.md` with the phone numbers.

### 2.4 F4 Hill feet (probe, then decide)

Neither scout looked at the pixels of the hill sprites' bases. Hills, mountains and rocks are sprites drawn after
the ground; the ground shader cannot reach them. The candidate is a second quad per massif from the same atlas slot:
its lowest rows tinted toward the mean colour of the surface under the footprint (a CPU colour per code, as the
minimap's `minimap_ground`), masked by the sprite's own alpha, in the same sharp session, respecting fog, outside
pointer picks and the sort.

Decision now: only the probe (off-screen snapshots of each massif class on grass, sand and snow at 2× zoom, flat and
2.5D, plus the base rows' alpha read from the CPU image). Build it only if the probe shows a hard rim or a colour
mismatch that a tint can hide without a halo. Risk if built: medium to high. Effort: probe S, build M.

## 3. The setting

- A `Settings` field (`src/ui/audio.rs`, next to `map_ground_blend` at :381), `map_natural_ground: bool`, serde
  default `false`, so an existing file reads off. Not `ExperimentalRules`: that set is gameplay, is stored in saves
  and marks the run; this is a look, the same kind as `map_live_water` and `map_mist`. Confirmed.
- Label (the owner approves the text with the screenshots): «Улучшенная земля (эксп.)». Help: «Razdor's: извилистые
  тропы, неровные края полей и берегов, оттенки лугов. Только вид: клетки, проходимость и вода те же. Не работает
  без «Смешивание земли».» Row in the Map group after `Row::GroundBlend` (main_menu.rs:1795, :1831, :1961, :2472).
- One switch for the player. The features are separate define blocks and separate dev switches for A/B
  (`RAZDOR_NATURAL=edges,paths,tone`, read in `display::follow_settings` next to `RAZDOR_GROUND_BLEND`,
  display.rs:867), but no sub-rows: the Map group is already long and the features share one look.
- Presets (`quality_preset.rs`, `Look` at :31): «Оригинал» sets it off (`Some(false)`); «Низкое», «Среднее» and
  «Высокое» do not say it (`None`), so they neither turn it on nor turn into «Своё» because of it. This differs
  from `map_ground_blend` (in no preset) for one reason: «Оригинал» must never show while a non-original look is on.
- Inert cases, stated in the help and the wiki: «Смешивание земли» off; hex maps and installs without art (the old
  per-cell path, `draw_terrain`); the minimap (it keeps the per-cell colours).
- Plumbing list: `audio.rs` field, default literal (:471) and a defaults-and-serialization test (as :1381);
  `display.rs` push (:912) and env override (:867); `terrain.rs` static (`set_natural`, as `set_blend` :327), the
  `Variant` flags and key, `variant_text` defines (:733), `GroundCache` flags; `neighbours.glsl` blocks;
  `main_menu.rs` row, help and the row-list test (:4107); `quality_preset.rs`; `data/lang/ru/game.txt` and the
  English catalog; `docs/flows/assets-and-drawing.md` (the ground shader's variants section);
  `changelog/unreleased.md` (Russian); the wiki `differences/map-look.md`, ru and en (the `wiki` skill); public text
  through the humanizer; the `mobile-ui` check for the new row; `docs/perf/ideas-ledger.md` rows.

## 4. Milestones

Each one lands on `main` by itself, only after the owner's yes on its screenshots. Work is done in a worktree.

1. **Natural edges, dev switch only** (S–M). The variant flag, the cache flag, the `NAT_EDGES` block, the tests of
   §2.1, `RAZDOR_NATURAL=edges`. No settings row, no player text. Screenshots: off and on pairs of four scenes
   (field, pond, river, coast), flat and 2.5D, with two tunings side by side (a soft edge and an edge snapped to
   the ground texture's pixels), each next to the old `sbs-soft-edges.png` framing so the stronger effect is
   plain. The owner picks a tuning.
2. **The setting** (S). The row, presets, catalogs, flow doc, changelog, wiki. Screenshots: the settings row on
   desktop and both phones, portrait and landscape; the `mobile-ui` report. Ships F1 to players.
3. **Winding paths** (M). `NAT_PATHS`, its tests, the road scenes of §2.2 on the campaign and community maps.
4. **Meadow tone** (S). `NAT_TONE`, the phone timing, the ledger row.
5. **Hill feet probe** (S), then its own decision and, if yes, its own milestone.

Before milestone 2 ships: the phone walk and scroll runs with the setting on, and the UI regression check with it off.

## 5. Open questions for the owner

1. The shore's warp limit. (a) 0.15 cell, as decided here: the waterline stays within 5 px of the cell edge;
   (b) 0.25 cell: clearly rounder ponds, but up to a quarter of a land cell beside water may be drawn as water
   (its centre never).
2. Streams: (a) as decided here, water keeps its cells and only the banks become natural; (b) allow a 1-cell
   shallows chain to narrow to about 70% of the cell, accepting that it looks easier to cross than it is.
3. Roads beside water or swamp stay full width on that side (the guard in §2.2). (a) keep the guard; (b) narrow
   them too, with sand or grass painted between the path and the water.
4. Meadow detail: (a) tone patches only (F3); (b) also try small flower dots painted into the grass texture
   (flat view, no meshes, no sway), knowing it is close to the declined scatter.
5. The row's name: (a) «Улучшенная земля (эксп.)»; (b) «Улучшенная графика (эксп.)», the wider name, if later
   object features (hill feet) should live under the same switch.
