# Performance ideas ledger

Every performance idea that was tried or measured, kept or not, with its numbers, so it is not tried
again blind. One row per idea, newest at the top of each table. Details and full tables live in the
dated perf docs next to this file; a row links to its section.

Rule (the owner, 2026-11-12): any performance idea that is tested gets a row here, whatever the
verdict: landed, dropped, blocked or "no measurable change". Write the scenes, the machine (GPU,
headless or monitor), the numbers before → after, the commit or branch, and why it was kept or dropped.

Machines: "4060" = RTX 4060 headless through VirtualGL (find its `/dev/dri/cardN` by PCI
`0000:05:00.0`; the number changes between boots). "monitor" = the owner's RTX 5060 Ti at
2560×1440, vsync off. CPU ms = the frame probe's `work_ms`. Measurements on a loaded machine
(several agents at once) are noted as such: compare interleaved runs only.

## Landed

| Date | Idea | Scenes / machine | Before → after | Commit | Notes |
|---|---|---|---|---|---|
| 2026-11-12 | 1605 step 4: map sprites' fog and reveals read from a cell texture in the sharp shader (`ui/sprite_fog.rs`) | «Другой берег» zoomed out / whole map, 2.5D and third person, РК3 start, 4060 (VirtualGL), interleaved | world pass 2.16 → 2.17, 3.38 → 3.44, 7.26 → 7.05, 10.19 → 10.33, 0.48 → 0.49 ms; draws unchanged; pictures byte-identical (192 PNGs, mid-run reveal too) | `741e3f8f` | No gain alone (within ±3%); kept because step 5 (static vertex buffers) needs the fog out of the vertices. The reveal table is made again every frame while a showing opens. [perf doc, step 4] |
| 2026-11-12 | Map sprite copies (half, quarter) stacked in the atlas, used below 12 / 6 px a cell | «Другой берег» whole map, 4060 | sprite GPU flat 0.35 → 0.14 ms, 2.5D 0.64 → 0.39 ms; draws unchanged (940 / 216); VRAM +5 MB (atlas 13.8 → 18.8 MB) | `740b6cb1` | Look change at whole-map zoom (softer canopies, PSNR ~28 dB), owner approved. Falls back to the full atlas over `GL_MAX_TEXTURE_SIZE`. [atmosphere doc, item 7] |
| 2026-11-12 | Trim map sprites to their opaque box (+1 px) | «Другой берег» zoomed out, 4060 | GPU sum 5.85 → 5.85 ms flat, 4.29 → 4.29–4.43 2.5D: no measurable gain on desktop | `78b7a99e` | Kept for phone GPUs (fill rate); ≤2-level float diffs, owner approved. First version cut sun/night shadows at the box (up to 114 levels): fixed by giving shadows the full rect. [item 6] |
| 2026-11-12 | Phones: drop a decoded picture from the store once its texture exists | desktop with the phone path forced (Xvfb) | peak RSS title 269 → 270, map 317 → 321, battle 410 → 397 MiB: within noise (~15 MiB) | `daaf4791` | Not proven. To be measured on the Samsung budget phone (with vs without); revert before 1.5.0 if no clear saving. [next-optimizations, item 3] |
| 2026-11-12 | Atmosphere: the fog test per location cached until the fog changes | РК3 / «Другой берег», 4060 | light step ~+0.25 → ~+0.006 ms (unstable A/B on a loaded machine) | `ee2914d7` | 48 shots byte-identical. Gain to be confirmed on a quiet machine. [item 4] |
| 2026-11-12 | Fog check once per location per frame (was 441 calls a frame in 2.5D) + building cards off | РК3 start 2.5D, monitor | CPU 0.42 → 0.21 ms (= v1.4.1) | `69212f33` | The building cards were turned off at the owner's request at the same time. [2.5D close-up cost] |
| 2026-11-12 | Bench reads the 4060's VRAM once per run (was every second) | — | — | `ad129661` | Polling `nvidia-smi` every second hung the 4060's GSP firmware (Xid 119). Not a speed idea; a safety rule. |
| 2026-11-12 | CPU-leak fixes: FrameCtx, jitter per object, chunked culling, mesh reuse, fog counter, frame state | whole map, monitor | 2.5D whole map 116 → 142 FPS (off), third person 68 → 83 | `6aab8b23` | Up to +25%. [atmosphere doc, final table] |
| 2026-11-11 | CPU pass: view-projection once per sprite, the form sun once, no per-sprite Vecs | 2.5D whole map, monitor | 14.2 → 12.0 ms | `a0b301ab` | |
| 2026-11-11 | Water: noise table, per-map bake, zoom LOD | first person, monitor | 7.7–7.9 ms before and after the new look | batch 4 | The new water look cost nothing. |
| 2026-11-11 | Fog redesign: one pass, baked noise | all | −44…−55% fog cost (software); ~1 ms on the 5060 Ti | batch 2 | |
| 2026-11-11 | Relief (groove) shading removed | 2.5D lighting, software | terrain stage 43 → 16 ms | batch 1 | |
| 2026-11-11 | Sprite batching | first person / 2.5D, monitor | draws 3767 → 337 / 636 → 185; first person → 11.2 ms (~88 FPS) | batch 1 | |
| 2026-11-11 | Sun shadows baked into a world-space map, stepped once per game hour | first person, monitor | 33 → ~15 ms (30 → ~65 FPS) | batch 1 | Was the #1 atmosphere cost. |

## Dropped, blocked or no gain

| Date | Idea | Scenes / machine | Result | Where | Why not |
|---|---|---|---|---|---|
| 2026-11-12 | 1605 step 3a: the window's view-projection built once per frame, not per sprite (`picture_quad`) | «Другой берег» zoomed out / whole map, 2.5D / third, 4060, interleaved vs `a220850b`, two rounds | Byte-identical 96/96; round 1 −6…−12 %, round 2 +0…+15 % on the same cases: no reliable gain | dropped, local branch `dropped/1605-step-3a-viewproj-cache` | Not landed: the rule needs a measured gain, and the rounds disagree. |
| 2026-11-12 | 1605 steps 1+2: static order sorted once per map and turn, then 16×16 chunks with a heap merge | «Другой берег» zoomed out / whole map, 2.5D / third, 4060, interleaved | Byte-identical, but SLOWER: 2.90 → 3.32, 5.86 → 7.47, 9.28 → 10.33, 14.29 → 15.84 ms | local branch `dropped/1605-steps-1-2`; numbers `a220850b` | The merge per frame costs more than the one sort it replaces; the sort was only 5–10% of the pass. Step 1 alone: no change (2.96 → 2.98 …). |
| 2026-11-12 | 1605: lazy pick quads (build the pointer's quads only when asked) | four cases, 4060 | ~0 ms: the PICKS push is free | `4e295fdc` (doc) | The picture quad is also the off-window cull; only a cheaper conservative cull could skip it (ceiling 0.6–1.2 ms, step 3). |
| 2026-11-12 | Record and replay the whole world sprite pass while nothing changes | zoomed out 2.5D still frame, 4060 | — (not built) | backlog 1605 | Ten kinds of draw calls with material switches; byte-identical replay = a large refactor. A cached sprite layer instead breaks army/building order. Superseded by GPU buffers (1605 step 5). |
| 2026-11-12 | Effects warm-up: draw the first frame without the atmosphere | РК3, effects on, 4060 | load 0.36 → 0.14 s, but frames 2–3 stalled 65 and 109 ms | not landed | A hitch right after the map appears is worse than 0.2 s more load. The background-thread bake needs `Send` inputs: not done. |
| 2026-11-12 | Ground scatter on the GPU (grass, shrubs, pebbles) | РК3 2.5D, 4060 | +0.02 ms (few) / +0.08 ms (many); buffers 1–4 MB | branch `gfx-scatter` | Look declined by the owner. Contact shadows from it → backlog 1604. |
| 2026-11-12 | Post effects: tilt-shift, colour grade, water depth, soft borders, «Ультра» preset | РК3 2.5D, 4060 | not measured | local branch `gfx-post-dropped` | Look declined by the owner. |
| 2026-11-12 | Folded building cards (villages, castles cut into cards; v6 castle strips) | РК3 2.5D, monitor | part of the 2.5D close-up +0.16 ms (with the fog check) | off by default (`RAZDOR_BUILDING_CARDS=1`), branch `castle-feet` | Off at the owner's request (look); measured card work was only ~12 µs a frame with one card building in view. |
| 2026-11-12 | Compressed textures on phones (ETC2/ASTC) | — | not tried | — | Lossy; would blur DT's pixel art (the "same as DT" rule). |
| 2026-11-11 | A cached picture of the whole flat map | flat whole map, monitor | 6.3 → 16–31 ms (slower) | dropped | Tree sway and water refilled it 24×/s. |
| 2026-11-11 | «Анимация карты» Выкл, «Разрешение карты» 50% as speed-ups | zoomed out, monitor | no FPS change | settings kept for battery | The zoomed-out cost is CPU, not pixels. |

## Open (being tried)

| Since | Idea | Who / where | Expected |
|---|---|---|---|
| 2026-11-12 | 1605 step 3: conservative cull before the picture quad | sprite-pass agent | ≤0.6–1.2 ms zoomed out |
| 2026-11-12 | 1605 step 5: GPU vertex buffers per chunk, the camera in the shader | after step 4 | removes most per-sprite placement (2–6 ms zoomed out) |
| 2026-11-12 | Art sheets and window pictures kept for the whole session (`DtArt`, `chrome::visual_images`) | not started | ~40–60 MB on phones (estimate) |
