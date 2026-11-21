# CRT post-process shader: research for Razdor

Date: 2026-11-21. Scope: one full-screen pass over the whole frame (map, menus, text), drawn into a
render target and blitted with a fragment shader. Targets: GLSL ES 1.00, `precision mediump float`,
no `texelFetch`, no dynamic loops; Mali/Adreno phones and desktop. Project license GPL-3.0-only.

Sources were read in `libretro/glsl-shaders` (master, `crt/shaders/`) on 2026-11-21. License lines
are quoted from the file headers.

## 1. Candidates

GPL-3 rule of thumb: "GPL v2 **or later**" is fine (we take it under v3). "GPL-2.0-only" is not.
A bare "License: GPL" with no version (EasyMode, crt-aperture) is ambiguous: do not copy code from
those without asking the author; reimplementing the idea is fine.

| Shader | Features | Passes | Mobile cost | License (header quote) | File |
|---|---|---|---|---|---|
| **Lottes CRT** (`crt-lottes`) | Gaussian scanlines, 4 mask types (aperture, slot, shadow), curvature, hard pixel filter, linear-light, bloom via beam width | 1 | Medium: ~2 tex taps + 3-tap horizontal x 2 rows (6 taps), `pow` per tap; the "fast" CRTS variant is heavier code | `// PUBLIC DOMAIN CRT STYLED SCAN-LINE SHADER // by Timothy Lottes`; also `crt-lottes-fast` (CRTS): `LICENSE = UNLICENSE (aka PUBLIC DOMAIN)` | https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-lottes.glsl |
| **crt-pi** (davej) | Scanline weight, optional 3-tap scanline multisample, 2 mask types (RG-B 2px and RGB 3px), barrel curvature, gamma via `pow` or cheap square/sqrt, bloom factor (brightness comp) | 1 | Very low: 1 tex tap, no loops. Designed for Raspberry Pi 1 GPU | `Copyright (C) 2015-2016 davej ... GNU General Public License ... either version 2 of the License, or (at your option) any later version.` | https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-pi.glsl |
| **zfast_crt** (SoltanGris42) | Quilez-style sharper scaling, scanline weight depends on luminance (`LOWLUMSCAN`/`HILUMSCAN`), simple 2/3px mask (`FINEMASK`), brightness boost, no gamma `pow` | 1 | Lowest: written for Mali-400 (comments mention its precision); `mediump` | `Copyright (C) 2017 Greg Hogan (SoltanGris42) ... either version 2 of the License, or (at your option) any later version.` | https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/zfast_crt.glsl |
| **crt-easymode** | Flat CRT, Gaussian-ish scanlines, mask with dot size/stagger/size params, halation (separate variant), gamma in/out params | 1 (plus `-halation` 3 passes) | Medium (horizontal taps loop-like blocks) | `CRT Shader by EasyMode / License: GPL` (no version) | https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-easymode.glsl |
| **crt-aperture** | Aperture grille focus, sharpness params, glow width | 1 | Medium | `CRT Shader by EasyMode / License: GPL` (no version) | https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-aperture.glsl |
| **crt-geom** (cgwg, Themaister, DOLLS) | Curvature (sphere), corner rounding, vignette-like, dot mask, scanline sharpness, gamma, oversampling | 1 | Medium-high: many `pow`/`sqrt` and 4x4 taps (`OVERSAMPLE`) | `Copyright (C) 2010-2012 cgwg, Themaister and DOLLS ... either version 2 of the License, or (at your option) any later version.` | https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-geom.glsl |
| **crt-hyllian / crt-nobody** | Scanlines, mask, filtering tied to the source pixel grid, brightness boost | 1 | Medium | `Copyright (C) 2011-2020 Hyllian ... Permission is hereby granted, free of charge...` (MIT text) | https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/hyllian/crt-hyllian.glsl , `.../crt-nobody.glsl` |
| **crt-caligari** | Phosphor-spot scanline, soft, brightness-compensating | 1 | Low | `Phosphor shader - Copyright (C) 2011 caligari ... either version, (at your option) any later version.` | https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-caligari.glsl |
| **fakelottes** (hunterk) | Cheap scanlines + curvature + Lottes masks as `#define` switches | 1 | Low-medium | No license header in the file (derived from Lottes PD, but hunterk's own code: unclear) | https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/fakelottes.glsl |
| **crt-guest-advanced** | Everything: halation, bloom, glow, convergence, slot/aperture masks, afterglow, deconvergence, gamma, vignette | 8+ | Too heavy for mobile; also needs history/multi-pass | `Copyright (C) 2018-2025 guest(r) ... either version 2 of the License, or (at your option) any later version.` (slang version) | https://github.com/libretro/slang-shaders/blob/master/crt/shaders/guest/advanced/crt-guest-advanced.slang |
| **crt-royale** (TroggleMonkey) | Best-in-class masks, bloom, halation, convergence | 10+ | Desktop only (GTX-class) | `Copyright (C) 2014 TroggleMonkey ... either version 2 of the License, or any later version.` | https://github.com/libretro/glsl-shaders/tree/master/crt/shaders/crt-royale |
| **crt-mattias / newpixie** (Mattias Gustavsson, Shadertoy) | Curvature, vignette, scanline crawl, noise | 1 | Low | No header; Shadertoy default is **CC-BY-NC-SA 3.0**: NOT compatible. Do not copy | https://www.shadertoy.com/view/lsB3DV |

Notes
- Everything under `libretro/*-shaders` is GLSL 1.20/ES with `#ifdef GL_ES` wrappers; the code still
  needs a port to our uniforms (no `TextureSize`/`InputSize`: we feed `virtual_size` ourselves).
- None of the cheap single-pass shaders does real bloom/halation. Real bloom needs a blurred copy
  (second pass).

## 2. What looks good (best practices)

Our input is not a low-res buffer: the frame is rendered at screen resolution (or the virtual
800x600 scaled up). Most libretro shaders assume the texture IS the low-res source and use
`TextureSize` for scanline position. We must use a uniform `virtual_lines` instead.

1. **Tie scanlines to the virtual source, not to physical pixels.**
   `line = uv.y * virtual_lines;  dy = fract(line) - 0.5;` Then weight = `max(1 - dy*dy*W, gap)`
   (crt-pi: `W = 6.0`, `gap = 0.12`) or a Gaussian `exp2(hard * dy*dy)` (Lottes `hardScan = -8`).
2. **Pick the line count for the screen.** 600 lines on a 1080-px-tall phone is 1.8 physical px per
   line: it will alias. Rule: each scanline period must be at least 3 physical px (better 4+).
   `lines = virtual_h / k` with the smallest integer `k` (1, 2, 3) such that `screen_h / lines >= 3`.
   At 600-line virtual and 1080p: `k = 2` (300 lines, 3.6 px per period). On 1440p+/desktop: `k = 1`
   (600 lines, 2.4 px) is borderline; prefer `k = 2` too unless the screen is 1800+ px tall.
   Quantize the period to a whole number of physical pixels when the scale is near-integer
   (`period = round(screen_h/lines)`) to avoid beat patterns; accept a slight uneven top/bottom.
3. **Moiré at non-integer scales.** Causes: scanline/mask frequency near the pixel grid frequency,
   and sampling the source with nearest at a non-integer ratio. Fixes: (a) keep the period >= 3 px,
   (b) soften the scanline profile (wider beam, `gap` not below ~0.25 on phones), (c) sample the
   source with bilinear (never nearest) in the CRT pass, (d) fade the mask out when
   `phys_px_per_mask_triad < 3` (zfast's `MASK_FADE` and crt-pi's single `MASK_BRIGHTNESS` are the
   cheap forms), (e) a single beam-width cosine `0.5 + 0.5*cos(2*pi*line)` instead of a hard
   `fract` step has no high harmonics (no aliasing at all, at the cost of a plainer look).
4. **Linear light.** Decode ~2.4 (crt-pi `INPUT_GAMMA 2.4`), re-encode 2.2 (`OUTPUT_GAMMA 2.2`):
   the 2.4/2.2 mismatch is a built-in mild contrast/black-crush that also brightens mids after
   scanline darkening. On mobile use `FAKE_GAMMA` form: `c = c*c` ... `sqrt(c)` (gamma 2.0, one
   multiply, one `sqrt`) as crt-pi offers; the visual difference to 2.4 is small.
5. **Brightness compensation.** Scanlines + mask lose 30-50% of light. Crt-pi multiplies by
   `BLOOM_FACTOR 1.5`; zfast uses `BRIGHTBOOST 1.25` and reduces the scanline strength for bright
   pixels (`HILUMSCAN`) so highlights bloom into the gap. Do the same: weight =
   `mix(dark_weight, 1.0, luma*k)` so white text stays white and readable. Measured target: average
   frame luminance with CRT on within ~90% of off.
6. **Mask scale on high-DPI.** Triad width in physical px = `round(dpi_scale * 1.0)` at least; the
   mask is only visible if triad >= 3 px. Phones at 400+ ppi: mask is invisible or moiré, so use
   scanlines + light vignette only, mask off or very light (0.9 floor). Desktop 1080p/1440p: the
   2px RG/B pair (crt-pi `MASK_TYPE 1`, `0.7` floor) or 3px RGB (`MASK_TYPE 2`) works. Use
   `gl_FragCoord.x` with `floor`, as crt-pi/zfast do, so it is stable.
7. **Curvature: subtle.** Barrel factor `~0.03-0.06` per axis (crt-pi default `0.10/0.25` is strong),
   with rounded corners (smoothstep mask, radius ~3-5% of height) and a black-outside region.
   Curvature bends the scanlines and causes moiré; keep it off by default (an option). UI clicks need
   the inverse mapping: **input coordinates must be un-distorted** or the mouse/touch is off at the
   edges. With curvature below ~0.04 it is a few pixels; otherwise map touches through the inverse.
8. **Cheap bloom.** Options, cheapest first:
   a. Fake bloom inside the single pass: scanline gap brightness grows with pixel luma (zfast, crt-pi
      `BLOOM_FACTOR`). Free.
   b. 3-tap horizontal softening (Lottes `Horz3`) in the same pass: 2 extra taps.
   c. Two-pass: downsample RT to 1/4 size, blur 5-tap (box/Gaussian, separable not needed at 1/4),
      add `0.15 * blurred` in the final pass. One extra low-res RT. Use only if (a) looks dull.
9. **Text legibility.** Small UI text suffers most. Keep scanline darkness at the pixel-centre
   floor >= 0.35 for text-sized features; use brightness boost; keep mask >= 0.75. A UI-region
   bypass (draw the HUD after the CRT pass) is the strongest fix, but the owner wants the whole
   frame, so offer the strength presets instead.
10. **Vignette and mild chromatic offset.** Vignette `1.0 - 0.15*dot(p,p)` costs nothing. RGB
    convergence offset of ~0.5 virtual px needs 3 taps; skip on mobile or keep at 0.

## 3. Pitfalls

- **mediump (fp16 on Mali/Adreno).** Range +-65504, ~3 decimal digits in [1,2). `gl_FragCoord.x` up to
  2400 with `fract(x * 0.3333)` is imprecise at the right edge (mask shifts). Compute mask index in
  `highp` (`#ifdef GL_FRAGMENT_PRECISION_HIGH`) or on integers: `mod(floor(fragCoord.x), 3.0)`. The
  scanline phase `uv.y * 600.0` has the same problem: do `fract` of the phase in highp, or pass the
  phase base from the vertex shader as a varying (interpolated per-vertex, cheaper and exact enough).
  zfast's header says the shader was tuned for Mali-400 fp precision.
  Practical rule: declare `precision highp float` where available (all GLES3-class GPUs, Mali-T,
  Adreno), fall back to mediump only for colour math.
- **`pow()` is expensive on mobile**: 3 `pow` per pixel at 1080p is fine on mid GPUs, noticeable on
  Mali-400/low-end; use the square/sqrt fake gamma there.
- **Render-target cost**: the RT at native screen size at 60 fps costs bandwidth on phones; the CRT
  pass itself (1 tap, ~30 ALU) is cheaper than the extra RT write. Bilinear filter on the RT is needed
  (nearest + non-integer scale = banding/moiré).
- **Pixel-perfect virtual lines**: the sampled colour must come from the scanline centre
  (`floor(line)+0.5`) like crt-pi, not the pixel's own y, otherwise text edges get sliced. Our RT is
  at screen resolution, so this means a vertical resample from the same texel row; with a screen-res
  RT it is optional but still avoids "double edge" on 1-px strokes.
- **Moiré gotcha with desktop scaling**: fractional UI scale (125%, 150%) makes mask/scanline period
  fractional; round the period or fade the effect per section 2.3.
- **Performance numbers**: no solid public per-device numbers were found. Known anchors: crt-pi ran
  full-screen at 60 fps on a Raspberry Pi 1 (VideoCore IV) at 1080p; zfast ran on SNES Classic
  (Mali-400). Anything at that complexity is safe on every phone Razdor supports.
  crt-royale needs a desktop GPU. Treat our shader as "crt-pi class" and verify with the project's
  offscreen profiler on the Samsung lab phone.
- **License traps**: Shadertoy shaders default to CC-BY-NC-SA (Mattias, many "CRT" toys); EasyMode
  headers say only `GPL`; fakelottes has no header. Avoid copying those.

## 4. Recommendation

**Write our own single-pass shader (GLSL ES 1.00), structurally based on crt-pi (GPL v2+) with
Lottes's public-domain Gaussian beam and mask ideas, plus zfast's luma-dependent scanline gap.**
Why:
- crt-pi/zfast are the only candidates built for weak mobile GPUs: 1 texture tap, no loops,
  `mediump`-aware, trivially parameterised. GPL v2+ combines fine into a GPL-3.0-only project
  (credit the authors in the file header and in the wiki license page).
- Lottes is public domain: formulas from it need no attribution constraint, safest to copy
  verbatim if we need the Gaussian, the Horz3 filter or the mask code.
- crt-pi uses `TextureSize` for scanline position, which we replace by a `virtual_lines` uniform
  (the key adaptation); also fix its fragile `gl_FragCoord * 0.5` mask for highp.
- Rejected: royale/guest-advanced (multi-pass, desktop only), geom (heavy, curvature-centric),
  EasyMode/aperture (ambiguous license), Mattias/newpixie (CC-BY-NC-SA).

Fallback if "own shader" is too much: port `zfast_crt` nearly 1:1 (shortest, safest on mobile) and
add a curvature/vignette block from crt-pi.

### Recommended defaults (preset "Normal")

| Parameter | Value | Notes |
|---|---|---|
| virtual lines | `600 / k`, k from screen height rule (section 2.2) | uniform `virtual_lines` |
| scanline strength (dark depth) | 0.35 | centre 1.0, gap `1 - 0.35` ~ 0.65 min on phones; 0.5 on desktop |
| beam shape | `max(1 - dy*dy*6.0, gap)` | crt-pi weight; gap 0.12 is too harsh: use `gap = 0.45` default |
| luma bloom | gap += `0.35 * luma` | zfast-style |
| brightness comp | x1.25 (scanlines only), x1.1 (mask on) | |
| gamma | decode 2.4 / encode 2.2; fake `c*c`/`sqrt` on low-end | |
| mask | off on phones; desktop: type RG-B 2 px, floor 0.80 | auto-fade when triad < 3 px |
| curvature | 0.0 (option: 0.035) with 4% rounded corners | off keeps touch exact |
| vignette | 0.12 | |
| source sampling | bilinear, vertical sample at line centre | |
| bloom pass | none in v1 | add 1/4-res blur later only if needed |

### Strength presets

- **Off**: pass skipped (draw RT straight).
- **Light**: scanline 0.20, mask off, vignette 0.08, no curvature.
- **Normal**: table above.
- **Strong**: scanline 0.50, mask 0.70 (desktop), curvature 0.05, vignette 0.20, bloom up.

### Exposable settings (keep to 4-5 in the UI)

1. Preset: Off / Light / Normal / Strong (one cycle button; the rest is derived).
2. Scanlines strength (slider, 0-0.6).
3. Mask (off / on; auto-disabled on phones).
4. Curvature (off / light) with touch-coordinate correction.
5. Brightness (compensation, 0.9-1.4), for people whose panel is dark.
Internal only: virtual line divisor k, gamma pair, vignette, bloom (`ExperimentalRules`-style
switch if the owner wants it as a deviation: the CRT look is a Razdor feature the original lacks).

### Checks before shipping (project DoD)

- UI change: owner approves off-screen screenshots (desktop and phones); UI regression goldens will
  all change, so CRT must be off by default and the regress run done with it off.
- Phone check with the mobile-ui skill: no readability regression of 12 sp text under Normal.
- Test 720p/1080p/1440p/2400x1080 and 125/150% scales for moiré (sweep render scale 0.9-2.0 and look
  for beats; a script can FFT the rendered image column average to detect periodic components).
- Profile on the Samsung lab phone; record in `docs/perf/ideas-ledger.md`.
- Wiki page ru+en; changelog line (Russian).

## 5. Source URLs

- libretro glsl-shaders CRT directory: https://github.com/libretro/glsl-shaders/tree/master/crt/shaders
- crt-pi: https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-pi.glsl
- zfast_crt: https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/zfast_crt.glsl
- crt-lottes: https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-lottes.glsl
- crt-lottes-fast (CRTS, Unlicense): https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-lottes-fast.glsl
- crt-geom: https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-geom.glsl
- crt-easymode: https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-easymode.glsl
- crt-aperture: https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-aperture.glsl
- crt-hyllian: https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/hyllian/crt-hyllian.glsl
- crt-nobody: https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-nobody.glsl
- crt-caligari: https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-caligari.glsl
- fakelottes: https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/fakelottes.glsl
- crt-mattias (Shadertoy lsB3DV): https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-mattias.glsl
- crt-guest-advanced (slang): https://github.com/libretro/slang-shaders/blob/master/crt/shaders/guest/advanced/crt-guest-advanced.slang
- crt-royale: https://github.com/libretro/glsl-shaders/tree/master/crt/shaders/crt-royale
- Lottes talk "CRT Shader" (original gist/Shadertoy forms are public domain): https://www.shadertoy.com/view/MsjXzh
- GPL v2-or-later to v3 compatibility: https://www.gnu.org/licenses/gpl-faq.html#AllCompatibility

Caveat: no live web search was used; sources are the shader files themselves (fetched), and
performance claims beyond the header comments are from general knowledge, not measured here. The
Lottes Shadertoy URL and the FAQ URL were not re-fetched.
