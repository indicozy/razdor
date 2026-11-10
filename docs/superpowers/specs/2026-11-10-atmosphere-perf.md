# Atmosphere: performance and the mist sheet count (2026-11-10)

Measured on the atmosphere branch's `--no-default-features` release build, off-screen under
Xvfb. Source CSVs and crops live outside git (`/home/indicozy/.cache/razdor-scratch/atmosphere/perf/`).

## Method

- Scenes: `map:РК1-Начало пути` (idle) and `walk:РК1-Начало пути:3,0`, flat (`RAZDOR_VIEW=flat`) and
  2.5D (`RAZDOR_VIEW=25d`), at 1280x800 and 2400x1080.
- Each run: `RAZDOR_PROFILE_FRAMES` (one row per frame), `RAZDOR_QUIT_AFTER=360`, the first 30 frames
  dropped. Median and p95 of `work_ms` (the frame's CPU work, the Xvfb timing of the probe).
- Configs (relative to "off"): lighting at 12 h and 7 h (relief is part of lighting; there is no switch
  for relief alone); mist with 1, 2, 3 and 4 sheets (`RAZDOR_MIST=1`, `RAZDOR_MIST_SHEETS=n`); live
  water (`RAZDOR_LIVE_WATER=1`); all on (7 h, mist 3, water).
- Renderer: Mesa software GL (llvmpipe) under Xvfb, pinned to CPUs 8-19 at nice 19. The repo has no
  headless EGL path (no note in `docs/flows/testing-and-tools.md` or the skills), so this is not a GPU or
  phone measurement. Read the numbers as relative and in software GL only.

## Caveats (why these numbers do not settle a sheet count)

1. **Contention.** During part of the matrix a `tools/depth/detect_points.py` job from another session
   ran on CPUs 8-19 (about 110 % CPU, pinned to the same set).
2. **Bimodal 2.5D frames.** In 2.5D the same scene sits either near 1.2-1.8 ms or near 5-8 ms per frame,
   and the mode does not follow the config (light 7 h and mist 3 were in the fast mode; off, light 12 h and
   mist 1, 2, 4 in the slow one). The 5-8 ms mode was reproduced in two repeats of the 2400x1080 2.5D
   configs (off and mists 1-4, median 4.9-5.2 ms each, p95 5.4-13.4 ms), and the fast mode did not recur
   in them. The cause is not identified (a possible cause: the ground cache refill cadence, or
   the other job); it must be found before 2.5D numbers mean anything.
3. **Walk scenes are almost idle.** `walk:…:3,0` moves the hero for 1-5 of 360 frames; the rest are idle
   frames. The walk tables measure the idle map, not walking.
4. **Flat view is stable.** Flat medians are 0.3-0.6 ms in every config; no feature shows a cost there.

## Tables (median / p95, ms, 330 frames each)

### map 1280x800

| config | flat median / p95 | 2.5D median / p95 |
|---|---|---|
| off | 0.58 / 0.69 | 1.71 / 2.19 |
| light 12 h | 0.35 / 0.52 | 1.48 / 1.92 |
| light 7 h | 0.34 / 0.52 | 3.72 / 10.38 |
| mist 1 | 0.37 / 0.65 | 1.59 / 2.00 |
| mist 2 | 0.52 / 0.66 | 1.55 / 1.96 |
| mist 3 | 0.51 / 0.66 | 7.08 / 13.31 |
| mist 4 | 0.33 / 0.50 | 1.62 / 2.09 |
| water | 0.32 / 0.51 | 1.18 / 1.72 |
| all on | 0.53 / 0.69 | 1.78 / 2.50 |

### map 2400x1080

| config | flat median / p95 | 2.5D median / p95 |
|---|---|---|
| off | 0.54 / 0.85 | 5.30 / 5.99 |
| light 12 h | 0.53 / 0.76 | 5.62 / 17.27 |
| light 7 h | 0.47 / 0.70 | 1.78 / 2.34 |
| mist 1 | 0.53 / 0.67 | 5.72 / 16.57 |
| mist 2 | 0.38 / 0.57 | 5.65 / 16.25 |
| mist 3 | 0.40 / 0.56 | 1.82 / 2.36 |
| mist 4 | 0.40 / 0.57 | 8.18 / 19.44 |
| water | 0.39 / 0.55 | 5.80 / 11.34 |
| all on | 0.38 / 0.67 | 5.73 / 16.44 |

### walk 1280x800 and walk 2400x1080 (idle map, see caveat 3)

| config | 1280 flat | 1280 2.5D | 2400 flat | 2400 2.5D |
|---|---|---|---|---|
| off | 0.55 / 0.65 | 0.92 / 1.54 | 0.35 / 0.54 | 14.80 / 19.74 |
| light 12 h | 0.36 / 0.57 | 0.93 / 1.56 | 0.34 / 0.52 | 1.17 / 2.07 |
| light 7 h | 0.33 / 0.54 | 0.92 / 1.56 | 0.35 / 0.53 | 5.21 / 9.89 |
| mist 1 | 0.33 / 0.53 | 0.92 / 1.47 | 0.35 / 0.42 | 5.09 / 10.60 |
| mist 2 | 0.34 / 0.52 | 0.94 / 1.56 | 0.35 / 0.51 | 5.13 / 5.77 |
| mist 3 | 0.33 / 0.51 | 0.96 / 1.40 | 0.35 / 0.51 | 1.32 / 2.44 |
| mist 4 | 0.34 / 0.51 | 0.98 / 1.49 | 0.35 / 0.52 | 5.45 / 12.15 |
| water | 0.32 / 0.50 | 0.91 / 1.53 | 0.36 / 0.53 | 5.31 / 13.16 |
| all on | 0.34 / 0.52 | 0.99 / 1.59 | 0.62 / 0.81 | 5.46 / 13.78 |

### Repeats: 2400x1080 2.5D map, mist sheets (two repeats, median / p95)

| config | r1 | r2 |
|---|---|---|
| off | 5.08 / 5.67 | 5.08 / 6.25 |
| mist 1 | 5.17 / 7.72 | 5.14 / 13.35 |
| mist 2 | 5.16 / 9.16 | 4.85 / 5.57 |
| mist 3 | 5.14 / 8.97 | 4.85 / 5.42 |
| mist 4 | 5.11 / 11.69 | 5.18 / 7.84 |

## Reading

- Flat: no measurable cost for lighting, mist (any sheet count), water, or all on.
- 2.5D: the medians in the repeats are the same for 1 to 4 sheets (within 0.3 ms); the p95 spread is
  noise from the contention and the bimodal mode. The single-run table's mist 3 (fast) and mist 4
  (slow) differences are the same bimodal artefact, not a sheet cost.
- Water and lighting show no consistent cost either (light 7 h and water are in the fast mode in some
  runs and the slow one in others).

## Recommendation

- **Sheet defaults stay 3 on the desktop and 2 on a phone.** The data give no cost difference between
  1 and 4 sheets on the desktop, so the looks decide: 3 keeps the fine sheet at scale 1.1 and the
  upper sheets' parallax. The phone's 2 sheets are not measured here (no phone, and software GL is not
  a phone GPU); keep 2 until a phone run shows the cost.
- Looks per cost: the four crops (`crop-sheets{1..4}-{flat,25d}.png`, 2400x1080, night at 23 h, mist on)
  are for the owner's comparison. No default changes.
- Before a 2.5D cost claim: find the cause of the bimodal mode on a quiet machine and rerun the
  2.5D matrix. A phone run (the `mobile-ui` skill's device path) is needed for the phone sheet count.

## Eyes view

The hero's torch glow is skipped in the first-person eyes camera (`eyes_cam`), so it no longer floats in
front of the lens. Armies and towns keep their glows.

## Mist in 3D (raised sheets and frontier banks)

The tilted views now draw the mist as raised sheets (`RAZDOR_MIST_RAISED`: 3 on the desktop, 2 on a phone)
and frontier banks (`RAZDOR_MIST_BANKS`: at most 300 on the desktop, 120 on a phone). The flat view is
unchanged (pixel-identical with the mist on and off). Method: Xvfb with software GL, pinned to CPUs 8-19,
`RAZDOR_PROFILE_FRAMES` CSV, 200 frames of map `РК1-Начало пути` at 1280x800, noon, 2.5D yaw 45. Medians of
the probe's CPU work (`work_ms`, the frame's build time) per run; the wall total (`total_ms`) is the
software rasteriser's share too and swings by tens of ms between runs of one build, so it is not a cost
measure here. Two repeats each:

| configuration | work_ms, run 1 | work_ms, run 2 |
|---|---|---|
| base (before), desktop | 1.03 | 1.18 |
| new, desktop (3 raised, 300 banks) | 3.19 | 3.58 |
| base (before), phone density (`RAZDOR_DP=2.625`) | 1.19 | 1.25 |
| new, phone density (2 raised, 120 banks) | 3.22 | 3.81 |
| new, phone density, banks off | 1.61 | 1.73 |

Reading: the banks cost about 1.9 ms of CPU build per frame on both (one standing draw and material switch
each); the raised sheets about 0.4 ms (their fill is the rest of the wall-time difference, which software GL
cannot price against a phone GPU). The flat view and the first-person view's CPU build are unchanged within
noise (flat work 0.6 ms either build). A single run once read 16 ms on the phone density; it did not repeat
(two later runs at 3.2 and 3.8 ms), and it is noted here as the noise the matrix above is measured against.

Not measured: the phone's GPU fill (no phone here; the owner's device is the test). If a phone run shows
the raised sheets too costly, `RAZDOR_MIST_RAISED=1` keeps one (the sheet at 0.35 cells) and the banks
alone carry the wall.

## Banks in one batch (the contour and the outward wall)

The frontier banks are now drawn as one batch per depth run (four runs a frame, `BANK_BANDS`), not one
standing draw each, and they follow the contour of the shown darkness at 0.5. Same scene and method as above
(Устье Трейна, 2.5D yaw 45, noon, median of the probe's `work_ms`, 200 frames per run):

| configuration | work_ms, runs |
|---|---|
| before (per-bank standing draws), desktop | 3.56, 3.26 |
| after (one batch per run), desktop | 1.06, 2.74, 1.62 |
| after, banks off (same binary), desktop | 1.40, 1.36 |
| before (per-bank), phone density | 2.33, 3.60 |
| after, phone density | 1.03 |

Reading: the banks cost about 0.2 to 0.3 ms of CPU build after (the difference to banks off), against about
2 ms before; the target was under 0.5 ms. The runs still swing by about a millisecond between one build's
runs, so the figures are medians of a few runs, not a timing to the tenth of a millisecond. Caps: 600 on the
desktop and 240 on a phone (were 300 and 120): a batch costs little per bank, and the contour now has two
banks a crossing. The raised-sheet cost is unchanged (see above).
