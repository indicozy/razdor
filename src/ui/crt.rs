//! The CRT effect («ЭЛТ-эффект», `Settings::crt`): an old monitor's look over the whole screen.
//!
//! While it is on, the frame is drawn into a target the size of the window (physical pixels), which
//! stands in for the screen (`render_cache::set_screen_target`); [`end_frame`] then copies it to the
//! real screen. Everything after [`begin_frame`] (the map, the windows, the text, the pointer) reaches
//! the screen through that copy, so the look covers the whole frame. Look only: no rule changes.
//! Off by default; a snapshot or a photo reads the stand-in (`target_image`), so no effect is on them
//! where the picture is read before the screen (`map_freeze` stays off while it is on).
//!
//! The shader (`CRT_FRAGMENT`) is our own, in GLSL ES 1.00. Its structure follows crt-pi (davej,
//! GPL-2.0-or-later); the beam and the aperture mask follow Timothy Lottes' CRT shader (public
//! domain); the scanlines that are relieved by bright pixels follow zfast_crt (Greg Hogan,
//! GPL-2.0-or-later). The licences and the research are in `docs/specs/2026-11-21-crt-effect.md`.

use macroquad::camera::set_default_camera;
use macroquad::color::{BLACK, WHITE};
use macroquad::material::{gl_use_default_material, gl_use_material, load_material, Material, MaterialParams};
use macroquad::math::vec2;
use macroquad::miniquad::{BlendFactor, BlendState, Equation, PipelineParams, ShaderSource};
use macroquad::prelude::{clear_background, UniformDesc, UniformType};
use macroquad::texture::{draw_texture_ex, render_target, DrawTextureParams, FilterMode, Image, RenderTarget, Texture2D};
use macroquad::window::{get_internal_gl, screen_dpi_scale, screen_height, screen_width};
use serde::{Deserialize, Serialize};

use super::render_cache;
use super::sharp::SHARP_VERTEX;

/// The curve of the screen (`curve_point`): how far the edges come in. The shader and the pointer share it.
const CURVE: f32 = 0.04;

/// The largest side in physical pixels the effect runs at; a larger window is drawn without it.
const MAX_SIDE: f32 = 8192.0;

/// «ЭЛТ-эффект» (`Settings::crt`): how strong the look is. «Выкл» is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Strength {
    #[default]
    Off,
    Light,
    Normal,
    Strong,
}

/// «Качество ЭЛТ» (`Settings::crt_quality`): the cost of the look. The platform default is
/// [`platform_default`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Fast,
    Normal,
    High,
}

/// The quality when the setting is «none»: `Fast` on a phone, `Normal` on a desktop.
pub fn platform_default() -> Quality {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        Quality::Fast
    } else {
        Quality::Normal
    }
}

/// The values of a profiling switch (`RAZDOR_CRT*`), desktop only, read once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Switch {
    strength: Option<Strength>,
    quality: Option<Quality>,
    curve: Option<bool>,
    passthrough: bool,
}

/// `off`, `light`, `normal` or `strong` (any case); anything else is no value.
pub fn parse_strength(s: &str) -> Option<Strength> {
    match s.trim().to_ascii_lowercase().as_str() {
        "off" => Some(Strength::Off),
        "light" => Some(Strength::Light),
        "normal" => Some(Strength::Normal),
        "strong" => Some(Strength::Strong),
        _ => None,
    }
}

/// `fast`, `normal` or `high` (any case); anything else is no value.
pub fn parse_quality(s: &str) -> Option<Quality> {
    match s.trim().to_ascii_lowercase().as_str() {
        "fast" => Some(Quality::Fast),
        "normal" => Some(Quality::Normal),
        "high" => Some(Quality::High),
        _ => None,
    }
}

/// `1` (the curve on) or `0` (off); anything else is no value.
pub fn parse_curve(s: &str) -> Option<bool> {
    match s.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// `RAZDOR_CRT_PASSTHROUGH=1`: the frame goes through the stand-in but is copied as it is (the check
/// that the redirection alone changes no pixel).
pub fn parse_passthrough(s: &str) -> bool {
    s.trim() == "1"
}

/// The profiling switch from the environment, read once. Phones have none.
fn switch() -> Switch {
    static SWITCH: std::sync::OnceLock<Switch> = std::sync::OnceLock::new();
    *SWITCH.get_or_init(|| {
        if cfg!(any(target_os = "android", target_os = "ios")) {
            return Switch::default();
        }
        let get = |key: &str| std::env::var(key).ok();
        Switch {
            strength: get("RAZDOR_CRT").and_then(|v| parse_strength(&v)),
            quality: get("RAZDOR_CRT_QUALITY").and_then(|v| parse_quality(&v)),
            curve: get("RAZDOR_CRT_CURVE").and_then(|v| parse_curve(&v)),
            passthrough: get("RAZDOR_CRT_PASSTHROUGH").is_some_and(|v| parse_passthrough(&v)),
        }
    })
}

/// The settings the window pushes (`main_menu::apply_options`, every frame).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Setting {
    strength: Strength,
    quality: Option<Quality>,
    curve: bool,
}

razdor::local! {
    /// The settings in use, as the window last pushed them.
    static SETTING: std::cell::Cell<Setting> = const { std::cell::Cell::new(Setting { strength: Strength::Off, quality: None, curve: false }) };
    /// Whether this frame is drawn through the stand-in (`begin_frame`).
    static ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The stand-in, kept between frames while the physical size holds.
    static TARGET: std::cell::RefCell<Option<RenderTarget>> = const { std::cell::RefCell::new(None) };
    /// The glow target (High), a quarter of the stand-in's size, kept while the size holds.
    static GLOW: std::cell::RefCell<Option<RenderTarget>> = const { std::cell::RefCell::new(None) };
    /// The CRT pass's material per quality (`Quality::index`): `None` in a slot is not made yet, and
    /// `Some(None)` is a shader that does not compile.
    static MATERIALS: std::cell::RefCell<[Option<Option<macroquad::material::Material>>; 3]> = const { std::cell::RefCell::new([None, None, None]) };
    /// The glow's downsample material, made once.
    static DOWN: std::cell::OnceCell<Option<macroquad::material::Material>> = const { std::cell::OnceCell::new() };
}

/// Pushes the settings: the strength, the quality (`None`: the platform's) and the curve.
pub fn set_settings(strength: Strength, quality: Option<Quality>, curve: bool) {
    SETTING.with(|s| s.set(Setting { strength, quality, curve }));
}

/// The strength in use: the profiling switch when one is set, else the setting.
fn strength_in_use() -> Strength {
    switch().strength.unwrap_or_else(|| SETTING.with(|s| s.get().strength))
}

/// Whether this frame is drawn through the CRT stand-in.
pub fn active() -> bool {
    ACTIVE.with(|a| a.get())
}

/// The stand-in of `w` × `h` physical pixels, kept while the size holds.
fn target_for(w: u32, h: u32) -> RenderTarget {
    TARGET.with(|t| {
        let mut t = t.borrow_mut();
        if let Some(rt) = t.as_ref().filter(|rt| rt.texture.width() as u32 == w && rt.texture.height() as u32 == h) {
            return rt.clone();
        }
        let rt = render_target(w, h);
        razdor::mem_probe::set_texture("crt target", razdor::mem_probe::texture_bytes(w, h, 4));
        rt.texture.set_filter(FilterMode::Linear);
        *t = Some(rt.clone());
        rt
    })
}

/// The frame starts: with the effect on, the 2D space and the cameras draw into the stand-in.
pub fn begin_frame() {
    let (w, h) = macroquad::miniquad::window::screen_size();
    let fits = |v: f32| (1.0..=MAX_SIDE).contains(&v);
    let on = strength_in_use() != Strength::Off && fits(w) && fits(h);
    ACTIVE.with(|a| a.set(on));
    if !on {
        render_cache::set_screen_target(None);
        return;
    }
    render_cache::set_screen_target(Some(target_for(w as u32, h as u32)));
    render_cache::to_screen();
    clear_background(BLACK);
}

/// The frame is drawn: the stand-in goes to the real screen. Nothing when the effect is off.
pub fn end_frame() {
    if !ACTIVE.with(|a| a.replace(false)) {
        return;
    }
    let Some(rt) = render_cache::screen_target() else {
        return;
    };
    render_cache::set_screen_target(None);
    set_default_camera();
    if switch().passthrough {
        return render_cache::blit_screen(&rt.texture);
    }
    let quality = quality_in_use();
    let Some(material) = material(quality) else {
        return render_cache::blit_screen(&rt.texture);
    };
    let (w, h) = (rt.texture.width(), rt.texture.height());
    let p = params(strength_in_use(), quality, desktop(), screen_dpi_scale(), curve_in_use(), h);
    let glow = if quality == Quality::High { glow_pass(&rt.texture) } else { rt.texture.clone() };
    let (sw, sh) = (screen_width(), screen_height());
    material.set_uniform("Screen", vec2(w, h));
    material.set_uniform("GlowTexel", vec2(1.0 / glow.width(), 1.0 / glow.height()));
    material.set_uniform("Period", p.period);
    material.set_uniform("Scan", p.scan);
    material.set_uniform("Relief", p.relief);
    material.set_uniform("Mask", p.mask);
    material.set_uniform("Vignette", p.vignette);
    material.set_uniform("Curve", p.curve);
    material.set_uniform("CornerPx", p.corner_px);
    material.set_uniform("Boost", p.boost);
    material.set_uniform("GlowAmt", p.glow);
    material.set_uniform("ConvPx", p.conv);
    material.set_texture("Glow", glow);
    gl_use_material(&material);
    draw_texture_ex(&rt.texture, 0.0, 0.0, WHITE, DrawTextureParams { dest_size: Some(vec2(sw, sh)), flip_y: true, ..Default::default() });
    gl_use_default_material();
}

/// The frame so far as the stand-in holds it, top row first and opaque, for the photo shutter
/// (`world_view::take_shot`): a photo has no effect on it. `None` when the effect is off.
pub fn target_image() -> Option<Image> {
    if !active() {
        return None;
    }
    let rt = render_cache::screen_target()?;
    unsafe { get_internal_gl() }.flush();
    let mut img = rt.texture.get_texture_data();
    flip_rows_opaque(&mut img.bytes, img.width as usize, img.height as usize);
    Some(img)
}

/// Flips an RGBA picture of `w` × `h` pixels top to bottom (a render target's texture comes out
/// upside down) and makes every pixel opaque. A short buffer is left as it is.
pub fn flip_rows_opaque(bytes: &mut [u8], w: usize, h: usize) {
    let stride = w * 4;
    if bytes.len() < stride * h {
        return;
    }
    for y in 0..h / 2 {
        let (top, bottom) = bytes.split_at_mut((h - 1 - y) * stride);
        top[y * stride..(y + 1) * stride].swap_with_slice(&mut bottom[..stride]);
    }
    for px in bytes[..stride * h].chunks_exact_mut(4) {
        px[3] = 255;
    }
}

/// The CRT pass, GLSL ES 1.00 (our own code). `#define QUALITY 0|1|2` is inserted after the first
/// line by [`fragment`]: 0 Fast (a square gamma, one tap), 1 Normal (the mask, the 2.4 gamma), 2 High
/// (the convergence offset, the glow).
const CRT_FRAGMENT: &str = r#"#version 100
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
}"#;

/// The glow's downsample: the frame averaged over four bilinear taps at ±1 source texel around each
/// glow texel, so every source texel counts (no shimmer while the map scrolls).
const DOWN_FRAGMENT: &str = r#"#version 100
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
varying lowp vec4 color;
varying highp vec2 uv;
uniform sampler2D Texture;
uniform vec2 SrcTexel;
void main() {
    vec4 s = texture2D(Texture, uv + SrcTexel * vec2( 1.0,  1.0))
           + texture2D(Texture, uv + SrcTexel * vec2(-1.0,  1.0))
           + texture2D(Texture, uv + SrcTexel * vec2( 1.0, -1.0))
           + texture2D(Texture, uv + SrcTexel * vec2(-1.0, -1.0));
    gl_FragColor = 0.25 * s * color;
}"#;

/// The CRT pass's source for a quality: [`CRT_FRAGMENT`] with its `QUALITY` define on line 2.
pub fn fragment(q: Quality) -> String {
    CRT_FRAGMENT.replacen("#version 100\n", &format!("#version 100\n#define QUALITY {}\n", q.index()), 1)
}

/// The values the CRT pass takes for one frame (`params`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub scan: f32,
    pub relief: f32,
    pub mask: f32,
    pub vignette: f32,
    pub boost: f32,
    pub glow: f32,
    pub conv: f32,
    pub curve: f32,
    pub corner_px: f32,
    pub period: f32,
}

/// The CRT pass's values for a strength, quality and screen. `desktop` (no phone) and a DPI scale
/// under 2 allow the aperture mask; the glow and the convergence are High only. `screen_h_px` is the
/// screen's height in physical pixels; the scanline period is a whole number of them, at least 3.
/// Off takes any values: the pass is skipped.
pub fn params(strength: Strength, quality: Quality, desktop: bool, dpi: f32, curve_on: bool, screen_h_px: f32) -> Params {
    let i = match strength {
        Strength::Off | Strength::Light => 0,
        Strength::Normal => 1,
        Strength::Strong => 2,
    };
    let mask_on = desktop && quality != Quality::Fast && dpi < 2.0;
    let mask = if mask_on { [0.0, 0.15, 0.25][i] } else { 0.0 };
    Params {
        scan: [0.25, 0.40, 0.55][i],
        relief: 0.5,
        mask,
        vignette: [0.10, 0.18, 0.28][i],
        boost: [1.08, 1.15, 1.22][i] + if mask > 0.0 { 0.05 } else { 0.0 },
        glow: if quality == Quality::High { [0.10, 0.15, 0.22][i] } else { 0.0 },
        conv: if quality == Quality::High { [0.0, 0.75, 1.0][i] } else { 0.0 },
        curve: if curve_on { CURVE } else { 0.0 },
        corner_px: if curve_on { 0.03 * screen_h_px } else { 0.0 },
        period: (screen_h_px / 360.0).round().max(3.0),
    }
}

/// The quality in use: the profiling switch, else the setting, else the platform's.
fn quality_in_use() -> Quality {
    switch().quality.or_else(|| SETTING.with(|s| s.get().quality)).unwrap_or_else(platform_default)
}

/// Whether the curve is on: the profiling switch when one is set, else the setting.
fn curve_in_use() -> bool {
    switch().curve.unwrap_or_else(|| SETTING.with(|s| s.get().curve))
}

/// Whether the screen is a desktop (the mask is for it only).
fn desktop() -> bool {
    !cfg!(any(target_os = "android", target_os = "ios"))
}

/// The copy's pipeline: the source written over the target as it is (no blend).
fn copy_pipeline() -> PipelineParams {
    PipelineParams {
        color_blend: Some(BlendState::new(Equation::Add, BlendFactor::One, BlendFactor::Zero)),
        alpha_blend: Some(BlendState::new(Equation::Add, BlendFactor::One, BlendFactor::Zero)),
        ..Default::default()
    }
}

/// The uniforms of the CRT pass (the names of [`CRT_FRAGMENT`]).
const CRT_UNIFORMS: [(&str, UniformType); 12] = [
    ("Screen", UniformType::Float2),
    ("GlowTexel", UniformType::Float2),
    ("Period", UniformType::Float1),
    ("Scan", UniformType::Float1),
    ("Relief", UniformType::Float1),
    ("Mask", UniformType::Float1),
    ("Vignette", UniformType::Float1),
    ("Curve", UniformType::Float1),
    ("CornerPx", UniformType::Float1),
    ("Boost", UniformType::Float1),
    ("GlowAmt", UniformType::Float1),
    ("ConvPx", UniformType::Float1),
];

impl Quality {
    /// The quality's number in the shader's `QUALITY` (Fast 0, Normal 1, High 2) and the cache's slot.
    fn index(self) -> usize {
        match self {
            Quality::Fast => 0,
            Quality::Normal => 1,
            Quality::High => 2,
        }
    }
}

/// The CRT pass's material for a quality, made on first use. `None` when it does not compile: the
/// frame then goes through the copy (logged once, when it is made).
fn material(q: Quality) -> Option<Material> {
    MATERIALS.with(|m| {
        let mut m = m.borrow_mut();
        let slot = &mut m[q.index()];
        if slot.is_none() {
            let params = MaterialParams {
                uniforms: CRT_UNIFORMS.iter().map(|(n, t)| UniformDesc::new(n, *t)).collect(),
                textures: vec!["Glow".to_string()],
                pipeline_params: copy_pipeline(),
            };
            let made = load_material(ShaderSource::Glsl { vertex: SHARP_VERTEX, fragment: &fragment(q) }, params)
                .map_err(|e| razdor::diag!("crt: the {q:?} shader does not load: {e}"))
                .ok();
            *slot = Some(made);
        }
        slot.clone().flatten()
    })
}

/// The glow's downsample material, made once (`None` when it does not compile).
fn downsample_material() -> Option<Material> {
    DOWN.with(|m| {
        m.get_or_init(|| {
            let params = MaterialParams {
                uniforms: vec![UniformDesc::new("SrcTexel", UniformType::Float2)],
                textures: Vec::new(),
                pipeline_params: copy_pipeline(),
            };
            load_material(ShaderSource::Glsl { vertex: SHARP_VERTEX, fragment: DOWN_FRAGMENT }, params)
                .map_err(|e| razdor::diag!("crt: the glow downsample does not load: {e}"))
                .ok()
        })
        .clone()
    })
}

/// The glow of the frame in `src` (High): a target a quarter of its size, kept while the size holds.
/// Without the downsample material the frame itself is the glow.
fn glow_pass(src: &Texture2D) -> Texture2D {
    let (w, h) = (src.width() as u32, src.height() as u32);
    let Some(down) = downsample_material() else {
        return src.clone();
    };
    let (gw, gh) = (w.div_ceil(4).max(1), h.div_ceil(4).max(1));
    let rt = GLOW.with(|g| {
        let mut g = g.borrow_mut();
        if let Some(rt) = g.as_ref().filter(|rt| rt.texture.width() as u32 == gw && rt.texture.height() as u32 == gh) {
            return rt.clone();
        }
        let rt = render_target(gw, gh);
        razdor::mem_probe::set_texture("crt glow", razdor::mem_probe::texture_bytes(gw, gh, 4));
        rt.texture.set_filter(FilterMode::Linear);
        *g = Some(rt.clone());
        rt
    });
    down.set_uniform("SrcTexel", vec2(1.0 / w as f32, 1.0 / h as f32));
    render_cache::fill(&rt, || {
        gl_use_material(&down);
        draw_texture_ex(src, 0.0, 0.0, WHITE, DrawTextureParams { dest_size: Some(vec2(screen_width(), screen_height())), flip_y: true, ..Default::default() });
        gl_use_default_material();
    });
    rt.texture.clone()
}

/// The screen's point (logical px, `x`, `y`) on the curved screen: the source point the shader
/// shows there is `curve_point(p)`, so input at `p` is the picture's point `curve(p)`. Same math as
/// the shader: `c = p * 2 - 1; c *= 1 + k * c.yx * c.yx`. Clamped to the `w` × `h` picture.
pub fn curve_point(x: f32, y: f32, w: f32, h: f32, k: f32) -> (f32, f32) {
    if w <= 0.0 || h <= 0.0 {
        return (x, y);
    }
    let cx = x / w * 2.0 - 1.0;
    let cy = y / h * 2.0 - 1.0;
    let bx = cx * (1.0 + k * cy * cy);
    let by = cy * (1.0 + k * cx * cx);
    let px = (bx * 0.5 + 0.5) * w;
    let py = (by * 0.5 + 0.5) * h;
    (px.clamp(0.0, w), py.clamp(0.0, h))
}

/// A pointer position (logical px) as the picture sees it: the curve when the effect is on and the
/// curve is on, else the position as it is.
pub fn pointer(p: (f32, f32)) -> (f32, f32) {
    if !active() || !curve_in_use() {
        return p;
    }
    curve_point(p.0, p.1, screen_width(), screen_height(), CURVE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strength_names_parse_in_any_case_and_junk_is_no_value() {
        assert_eq!(parse_strength("off"), Some(Strength::Off));
        assert_eq!(parse_strength(" Light "), Some(Strength::Light));
        assert_eq!(parse_strength("NORMAL"), Some(Strength::Normal));
        assert_eq!(parse_strength("strong"), Some(Strength::Strong));
        assert_eq!(parse_strength("max"), None);
        assert_eq!(parse_strength(""), None);
    }

    #[test]
    fn the_quality_curve_and_passthrough_switches_parse() {
        assert_eq!(parse_quality("fast"), Some(Quality::Fast));
        assert_eq!(parse_quality("High"), Some(Quality::High));
        assert_eq!(parse_quality("ultra"), None);
        assert_eq!(parse_curve("1"), Some(true));
        assert_eq!(parse_curve("0"), Some(false));
        assert_eq!(parse_curve("yes"), None);
        assert!(parse_passthrough("1"));
        assert!(!parse_passthrough("0"));
    }

    #[test]
    fn the_default_is_off_and_the_platform_quality_is_normal_on_desktop() {
        assert_eq!(Strength::default(), Strength::Off);
        if !cfg!(any(target_os = "android", target_os = "ios")) {
            assert_eq!(platform_default(), Quality::Normal);
        }
    }

    #[test]
    fn a_picture_is_flipped_top_down_and_made_opaque() {
        // Two pixels wide, three high: rows 0, 1, 2 hold the values 10, 20, 30 (alpha 0).
        let mut px: Vec<u8> = [10u8, 0, 0, 0, 11, 0, 0, 0, 20, 0, 0, 0, 21, 0, 0, 0, 30, 0, 0, 0, 31, 0, 0, 0].to_vec();
        flip_rows_opaque(&mut px, 2, 3);
        let reds: Vec<u8> = px.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(reds, vec![30, 31, 20, 21, 10, 11]);
        assert!(px.chunks_exact(4).all(|p| p[3] == 255));
    }

    #[test]
    fn the_curve_keeps_the_centre_and_the_edge_midpoints_and_clamps_the_corners() {
        let (w, h, k) = (1280.0, 800.0, CURVE);
        let near = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3;
        assert!(near(curve_point(w / 2.0, h / 2.0, w, h, k), (w / 2.0, h / 2.0)));
        assert!(near(curve_point(w / 2.0, 0.0, w, h, k), (w / 2.0, 0.0)));
        assert!(near(curve_point(w / 2.0, h, w, h, k), (w / 2.0, h)));
        assert!(near(curve_point(0.0, h / 2.0, w, h, k), (0.0, h / 2.0)));
        assert!(near(curve_point(w, h / 2.0, w, h, k), (w, h / 2.0)));
        // Near a corner the point moves outward and clamps to the corner.
        assert_eq!(curve_point(1.0, 1.0, w, h, k), (0.0, 0.0));
        assert_eq!(curve_point(w - 1.0, h - 1.0, w, h, k), (w, h));
    }

    #[test]
    fn the_pointer_is_the_identity_while_the_effect_is_off() {
        assert!(!active());
        assert_eq!(pointer((123.0, 45.0)), (123.0, 45.0));
    }

    #[test]
    fn the_parameter_table_has_the_spec_values() {
        let p = |s, q, desktop, dpi, curve| params(s, q, desktop, dpi, curve, 800.0);
        assert_eq!(p(Strength::Light, Quality::Normal, true, 1.0, false).scan, 0.25);
        assert_eq!(p(Strength::Normal, Quality::Normal, true, 1.0, false).scan, 0.40);
        assert_eq!(p(Strength::Strong, Quality::Normal, true, 1.0, false).scan, 0.55);
        assert_eq!(p(Strength::Normal, Quality::Normal, true, 1.0, false).relief, 0.5);
        // The mask: desktop, not Fast, DPI under 2; each strength's depth.
        assert_eq!(p(Strength::Light, Quality::Normal, true, 1.0, false).mask, 0.0);
        assert_eq!(p(Strength::Normal, Quality::Normal, true, 1.0, false).mask, 0.15);
        assert_eq!(p(Strength::Strong, Quality::High, true, 1.0, false).mask, 0.25);
        assert_eq!(p(Strength::Strong, Quality::Normal, false, 1.0, false).mask, 0.0, "a phone has no mask");
        assert_eq!(p(Strength::Strong, Quality::Fast, true, 1.0, false).mask, 0.0);
        assert_eq!(p(Strength::Strong, Quality::Normal, true, 2.0, false).mask, 0.0);
        assert_eq!(p(Strength::Normal, Quality::Normal, true, 1.0, false).vignette, 0.18);
        // The boost gains 0.05 with the mask.
        assert_eq!(p(Strength::Normal, Quality::Normal, true, 1.0, false).boost, 1.15 + 0.05);
        assert_eq!(p(Strength::Normal, Quality::Fast, true, 1.0, false).boost, 1.15);
        // The glow and the convergence are High only.
        assert_eq!(p(Strength::Strong, Quality::High, true, 1.0, false).glow, 0.22);
        assert_eq!(p(Strength::Strong, Quality::Normal, true, 1.0, false).glow, 0.0);
        assert_eq!(p(Strength::Strong, Quality::High, true, 1.0, false).conv, 1.0);
        assert_eq!(p(Strength::Light, Quality::High, true, 1.0, false).conv, 0.0);
        // The curve: 0 and 0 while it is off.
        let off = p(Strength::Normal, Quality::Normal, true, 1.0, false);
        assert_eq!((off.curve, off.corner_px), (0.0, 0.0));
        let on = p(Strength::Normal, Quality::Normal, true, 1.0, true);
        assert_eq!(on.curve, CURVE);
        assert!((on.corner_px - 0.03 * 800.0).abs() < 1e-3);
    }

    #[test]
    fn the_scanline_period_is_a_whole_number_of_pixels_of_at_least_three() {
        let period = |h: f32| params(Strength::Normal, Quality::Normal, true, 1.0, false, h).period;
        let cases = [(720.0, 3.0), (768.0, 3.0), (900.0, 3.0), (1080.0, 3.0), (1200.0, 3.0), (1440.0, 4.0), (2340.0, 7.0), (2400.0, 7.0)];
        for (h, want) in cases {
            assert_eq!(period(h), want, "height {h}");
            assert_eq!(period(h).fract(), 0.0, "height {h}");
        }
    }

    #[test]
    fn the_fragment_has_its_quality_on_line_two() {
        for (q, n) in [(Quality::Fast, 0), (Quality::Normal, 1), (Quality::High, 2)] {
            let src = fragment(q);
            let mut lines = src.lines();
            assert_eq!(lines.next(), Some("#version 100"));
            assert_eq!(lines.next(), Some(format!("#define QUALITY {n}").as_str()));
        }
    }

    #[test]
    fn a_short_buffer_is_left_alone() {
        let mut px = vec![1u8, 2, 3, 4];
        flip_rows_opaque(&mut px, 2, 2);
        assert_eq!(px, vec![1, 2, 3, 4]);
    }
}
