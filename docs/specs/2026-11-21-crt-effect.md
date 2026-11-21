# CRT effect («ЭЛТ-эффект»): spec and plan

Date: 2026-11-21. The owner's request and decisions (this session): a CRT post-process over the
**whole screen** (map, windows, text, pointer), a strength row, a separate quality/performance row,
and a separate curved screen with rounded corners. Look only: no rules change, so no RE and no
experimental switch; off by default. A UI change: the owner approves screenshots before `main`.
Research and licence notes: `crt-shader-research.md` (same folder).

Branch `crt-effect` (worktree `.claude/worktrees/crt-effect`), local until the owner's yes.

## 1. Settings (`audio::Settings`, group «Экран» after «Крупные окна»)

| Field | Type | Default | Row (ru / en catalog key) | Values |
|---|---|---|---|---|
| `crt` | `crt::Strength` | `Off` | «ЭЛТ-эффект» / `CRT effect` | Выкл / Лёгкий / Обычный / Сильный (`Off`/`Light`/`Normal`/`Strong`) |
| `crt_quality` | `Option<crt::Quality>` | `None` = platform default | «Качество ЭЛТ» / `CRT quality` | Быстрое / Обычное / Высокое (`Fast`/`Normal`/`High`) |
| `crt_curve` | `bool` | `false` | «Скруглённый экран» / `Curved screen` | Выкл / Вкл |

Platform default quality: `Normal` on desktop, `Fast` on Android and iOS. All three are serde
defaults (older settings files load with the CRT off). «Качество графики» presets do not touch them.
The quality and curve rows do nothing while the strength is «Выкл» (they still cycle).

Help text (the CRT row's «?»): en `An old monitor's look over the whole screen: scanlines, a soft
glow and darker edges. Quality: Fast for weak phones, High adds a glow around bright areas.`
ru `Вид старого монитора на весь экран: строки развёртки, мягкое свечение и затемнённые края.
Качество: «Быстрое» для слабых телефонов, «Высокое» добавляет свечение вокруг ярких мест.`

Profiling/snapshot overrides (desktop only, read once, like `RAZDOR_MAP_SCALE`):
`RAZDOR_CRT=off|light|normal|strong`, `RAZDOR_CRT_QUALITY=fast|normal|high`, `RAZDOR_CRT_CURVE=0|1`,
`RAZDOR_CRT_PASSTHROUGH=1` (the frame goes through the target but is copied as it is: the check that
the redirection changes no pixel).

## 2. Frame flow

```
frame_state::refresh
crt::begin_frame()      if active: target (physical px, Linear) = the "screen"; camera into it
app.frame() … overlays … cursor::draw()     all into the target, as before
crt::end_frame()        screen target cleared, default camera; [High: glow pass]; CRT pass to the screen
bench/snapshot/trailer captures (get_screen_data reads the CRT result)
next_frame
```

Active = strength ≠ Off and the window's physical size is 1..=8192 on each side.

### The "screen" redirection (`render_cache`)

Today "back to the screen" is `set_default_camera()` in `render_cache` (`back_to_2d`, `with_base`'s
drop, the fill's end) and `map_freeze`, and the pan's camera (`pan::sync`) draws to the screen.
Add `SCREEN: RefCell<Option<RenderTarget>>` with `set_screen_target(Option<RenderTarget>)` and
`to_screen()` (the screen target's 2D camera over `Rect(0,0,screen_width,screen_height)` when set,
else `set_default_camera()`); every former `set_default_camera()` there calls `to_screen()`.
`draw_target()` falls back to the screen target (the 3D views render into it, as they already do into
`map_scale`'s base target). `pan::sync`: with a screen target, the camera gets
`render_target: Some(rt)` and keeps `from_display_rect`'s zoom.y (no negation).

Scissors (`widgets.rs:777`, `pan::scissor`, `weather.rs:835`) are in framebuffer pixels; check
whether macroquad flips them inside a render target. If the passthrough picture of a clipped list
differs, fix it in one helper (flip y to `target_h - y - h` while a target is bound) used by all three.

Reads of the screen mid-frame: `map_freeze` (grab_screen) is off while the CRT is active (its gate
returns "no freeze"; the map is drawn live under windows); the photo shutter
(`world_view::take_shot`) reads the CRT target's texture instead of `get_screen_data`: first
`get_internal_gl().flush()` (as `map_freeze` does), then `get_texture_data`, rows flipped, alpha forced
opaque (`opaque_rgba`), so a photo has no CRT on it.

Scissor: macroquad applies `height - (y + h)` with the bound target's height, and the target's 2D camera
keeps the screen's orientation, so a clip needs no correction in the stand-in. (A pre-flip was tried first
and mirrored the clips: the scenario list's panes came out cut; measured, 2026-11-21.) The clips go through
one helper, `render_cache::scissor`, used by `widgets::Clip::new`, `pan::scissor` and `weather.rs`.

Sprite retain (`sharp/retain.rs`) already turns itself off while a render pass is active, so it never
escapes the target (its `begin_default_pass` is only in the replay path); the cost is that the retain's
saving is lost while the CRT is on.

### Curved screen and input

The shader maps a screen point `p` (uv 0..1) to the source point `curve(p)` it shows. So the pointer at
`p` points at source `curve(p)`: input needs the same forward function, no inverse. `crt::pointer(x, y)`
(logical px) returns `curve` of it, clamped to the screen; identity when inactive or the curve is off.
Apply it in `input.rs` at the raw sources only: `mq::mouse_position()` (lines ~130, ~204, ~414) and
each touch's position from `mq::touches()` (~206). The pointer is drawn into the source, so it shows
where the finger or mouse is.

```
c = p*2 - 1;  c *= 1 + K * c.yx*c.yx;  return c*0.5 + 0.5      (K = CURVE = 0.04)
```
Edge midpoints stay exact (nothing of the picture is cut off); the source's corners come inwards, and the
screen's corners are black, rounded with radius `CORNER = 0.03 * screen height` (physical px).

## 3. The shader (`src/ui/crt.rs`, GLSL ES 1.00, our own)

Structure after crt-pi (GPL-2.0-or-later, davej), beam/mask ideas after Lottes (public domain),
luma-relieved scanlines after zfast_crt (GPL-2.0-or-later, Greg Hogan). Credit all three in the
module header. One fragment source; `#define QUALITY 0|1|2` is inserted after `#version 100`, one
material per quality, made lazily. Vertex shader: `sharp::SHARP_VERTEX`. Pipeline: copy blend (as
`render_cache::copy_material`). Drawn like `render_cache::blit_screen` (`flip_y: true`, full screen).

```glsl
#version 100
// #define QUALITY n  (inserted)
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
varying lowp vec4 color;
varying highp vec2 uv;
uniform sampler2D Texture;
uniform sampler2D Glow;
uniform vec2 Screen;      // target size, physical px
uniform vec2 GlowTexel;   // 1 / glow target size
uniform float Period;     // scanline period, whole physical px (>= 3)
uniform float Scan;       // scanline depth
uniform float Relief;     // how much bright pixels fill the gap
uniform float Mask;       // aperture mask depth, 0 = off
uniform float Vignette;
uniform float Curve;      // 0 = flat
uniform float CornerPx;   // 0 = square corners
uniform float Boost;      // brightness compensation
uniform float GlowAmt;
uniform float ConvPx;     // RGB convergence offset, physical px

vec3 dec(vec3 c) {
#if QUALITY == 0
    return c * c;
#else
    return pow(c, vec3(2.4));
#endif
}
vec3 enc(vec3 c) {
#if QUALITY == 0
    return sqrt(c);
#else
    return pow(c, vec3(1.0 / 2.2));
#endif
}

void main() {
    vec2 cc = uv * 2.0 - 1.0;
    cc *= 1.0 + Curve * (cc.yx * cc.yx);
    vec2 src = cc * 0.5 + 0.5;
    vec2 r = 2.0 * CornerPx / Screen;
    vec2 d = max(abs(cc) - (1.0 - r), 0.0) * Screen * 0.5;
    float inside = clamp(CornerPx - length(d) + 1.0, 0.0, 1.0);
    if (inside <= 0.0) { gl_FragColor = vec4(0.0, 0.0, 0.0, 1.0); return; }

#if QUALITY == 2
    float o = ConvPx / Screen.x;
    vec3 col = vec3(texture2D(Texture, src + vec2(o, 0.0)).r,
                    texture2D(Texture, src).g,
                    texture2D(Texture, src - vec2(o, 0.0)).b);
#else
    vec3 col = texture2D(Texture, src).rgb;
#endif
    col = dec(col);
    float luma = dot(col, vec3(0.299, 0.587, 0.114));

    float phase = fract(src.y * Screen.y / Period);
    float gap = 0.5 + 0.5 * cos(6.2831853 * phase);      // 1 between lines, 0 at a line's centre
    col *= 1.0 - Scan * gap * (1.0 - Relief * luma);

#if QUALITY >= 1
    if (Mask > 0.0) {
        float m = mod(floor(gl_FragCoord.x), 2.0);
        col *= m < 0.5 ? vec3(1.0, 1.0 - Mask, 1.0) : vec3(1.0 - Mask, 1.0, 1.0 - Mask);
    }
#endif
#if QUALITY == 2
    vec3 g = texture2D(Glow, src).rgb * 0.4
           + texture2D(Glow, src + GlowTexel * vec2( 1.5,  1.5)).rgb * 0.15
           + texture2D(Glow, src + GlowTexel * vec2(-1.5,  1.5)).rgb * 0.15
           + texture2D(Glow, src + GlowTexel * vec2( 1.5, -1.5)).rgb * 0.15
           + texture2D(Glow, src + GlowTexel * vec2(-1.5, -1.5)).rgb * 0.15;
    col += GlowAmt * dec(g);
#endif
    col *= 1.0 - Vignette * 0.5 * dot(cc, cc);
    col = enc(clamp(col * Boost, 0.0, 1.0)) * inside;
    gl_FragColor = vec4(col, 1.0) * color;
}
```

Scanlines: the cosine profile has no sharp edges, so no aliasing; the source is sampled bilinear at
`src` itself (never snapped to the line centre: that would drop rows of 1-px text).

Period: a whole number of physical px, `Period = max(3, round(screen_h_px / 360))` (720 → 3, 1080 → 3,
1440 → 4, 2160 → 6, 2400 → 7). A fractional period beats into bands (advisor review); a whole one doesn't.

High's glow pass: a target of `ceil(w/4) × ceil(h/4)`, Linear, the frame drawn into it with a 4-tap
downsample material (bilinear taps at ±1 source texel around the quad's centre, averaged: every source texel
counts, so the glow doesn't shimmer when the map scrolls); kept between frames like `map_scale`'s target. Fast and Normal bind the frame texture as
`Glow` (unused) and skip the pass.

### Parameter table (`crt::params(strength, quality, phone, curve, screen_px) -> Params`, unit-tested)

| | Light | Normal | Strong |
|---|---|---|---|
| Scan | 0.25 | 0.40 | 0.55 |
| Relief | 0.5 | 0.5 | 0.5 |
| Mask (desktop, quality ≥ Normal, dpi scale < 2; else 0) | 0.0 | 0.15 | 0.25 |
| Vignette | 0.10 | 0.18 | 0.28 |
| Boost (+0.05 when Mask > 0) | 1.08 | 1.15 | 1.22 |
| GlowAmt (High only, else 0) | 0.10 | 0.15 | 0.22 |
| ConvPx (High only, else 0) | 0.0 | 0.75 | 1.0 |

Curve = `0.04` and CornerPx = `0.03 × screen_h_px` when the curve is on, else 0 and 0.

## 4. Implementation tasks (sequential, one subagent each, all in the `crt-effect` worktree)

1. **Plumbing + settings.** `crt.rs` with the enums, settings fields, env overrides, `set_settings`
   (called where `frame_cap::set_setting` is, `main_menu.rs:~1602`), the target, `begin_frame` /
   `end_frame` with the passthrough copy only (no shader yet), the render_cache screen target,
   `pan::sync`, the scissor helper, `map_freeze` off, photo read, and `ui_regress.py run --env K=V`
   (repeatable, added to every case's env). Verify: `ui_regress.py run --env RAZDOR_CRT=light --env
   RAZDOR_CRT_PASSTHROUGH=1` matches the goldens on every scene (a diff is an escaped draw or a
   flipped scissor), and the plain run (CRT off) too.
2. **Shader + curve.** The shader above, the three qualities, the parameter table, the glow pass,
   `crt::pointer` wired into `input.rs`, unit tests (`curve` keeps centre and edge midpoints, corners
   leave the screen; the period is whole and ≥ 3 for heights 720, 768, 900, 1080, 1200, 1440, 2340, 2400; the parameter table; env parsing; settings serde defaults).
3. **Rows + text.** The three rows in group Screen, the catalog lines (ru, en), help text, changelog
   line in `changelog/unreleased.md`, `docs/flows/assets-and-drawing.md` section. Then the screenshot
   matrix for the owner.

After the owner's yes: the UI regression check (all goldens unchanged with the CRT off), the mobile UI
check, the wiki page (ru, en; `wiki` skill), rebase, push to `main`.

## 5. Risks

- Scissor inside a render target (task 1 checks it).
- Extra full-screen target on phones: memory `w*h*4` bytes (+1/16 for High); report it to
  `mem_probe` as "crt target". Fast stays crt-pi class (1 tap, ~25 ALU).
- `map_freeze` off: a window over the map redraws the live map every frame while the CRT is on; sprite
  retain is off too. Profile it and give it a row in `docs/perf/ideas-ledger.md`.
- The text and touch probes measure the source, not the curved picture (fine: the curve is optional).
