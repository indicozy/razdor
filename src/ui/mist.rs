//! Razdor's «Туман вместо черноты» (`Settings::map_mist`, off by default): the war fog's unexplored
//! land is drifting mist instead of black. Style only: no rule, fog, path or save reads it.
//!
//! The fog texture (`minimap::fog_layer`: one texel per cell, its alpha the shown darkness) is drawn
//! through a material, on the same quad and texels as the black fog. Where the darkness is 1 the
//! mist is opaque, where it is 0 nothing shows, and only at the opening's edge the alpha wisps
//! ([`mist_alpha`]). Stacked billow sheets (domain-warped noise) drift at different speeds.
//!
//! The flat view draws the sheets on the ground, as they always were. The tilted views (2.5D and
//! first-person) draw the lowest sheet on the ground, opaque where unexplored, and the others as
//! raised sheets ([`raised`]): translucent copies of the same quad lifted off the ground, so the
//! sheet has a thickness and parallax as the camera turns. The mist's colour is the
//! lighting's (the daylight tint is drawn over it after the fog). The minimap keeps its black.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use macroquad::miniquad::{FilterMode as MqFilter, MipmapFilterMode, TextureWrap};
use macroquad::prelude::*;
use razdor::local;

/// The mist's neutral grey-white (the colour with the lighting off, and the edge fog beyond the map).
pub const MIST_RGB: [f32; 3] = [0.74, 0.77, 0.82];

/// The setting (`Settings::map_mist`), pushed every frame by `display::follow_settings`.
static ON: AtomicBool = AtomicBool::new(false);

/// Takes the setting «Туман вместо черноты» in (every frame).
pub fn set_pref(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Whether the unexplored land is drawn as mist (instead of black).
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// The mist's alpha on a cell of shown darkness `d` (0 lit … 1 unexplored), with the wisp value `w`
/// (0..1, the noise). Unexplored (`d` ≥ 0.995) is opaque; lit is clear; in between the alpha leans
/// on `w` (a patchy edge, never past 0 or 1). The shader's formula, pinned by the tests.
#[allow(dead_code)] // The shader's formula in Rust, pinned by the tests.
pub fn mist_alpha(d: f32, w: f32) -> f32 {
    if d >= 0.995 {
        1.0
    } else {
        (d + 0.5 * (w - 0.5) * d * (1.0 - d) * 4.0).clamp(0.0, 1.0)
    }
}

/// The 2.5D tilt's offset of the sheets per unit of height, in cells: a view turned and flatter
/// shifts the upper sheets further. The flat view has none (the caller passes zero there). The
/// tilted views now lift their raised sheets for real ([`raised_height`]); this is the flat-ground
/// parallax of the flat mist's sheets, kept for the flat path and for `RAZDOR_MIST_RAISED=0`.
pub fn parallax(yaw_deg: f32, pitch_deg: f32) -> Vec2 {
    let (s, c) = yaw_deg.to_radians().sin_cos();
    vec2(s, c) * (1.0 - pitch_deg / 90.0) * 0.6
}

/// `RAZDOR_MIST_SHEETS=1..4` (read once): the sheets drawn, whatever the platform (measurements).
fn env_sheets() -> Option<f32> {
    static ENV: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *ENV.get_or_init(|| std::env::var("RAZDOR_MIST_SHEETS").ok().and_then(|v| v.trim().parse::<u32>().ok()).map(|n| n.clamp(1, 4) as f32))
}

/// The sheets drawn: 3 on the desktop, 2 on a phone (the fill rate), or `RAZDOR_MIST_SHEETS`.
fn sheets() -> f32 {
    if let Some(n) = env_sheets() {
        return n;
    }
    if crate::ui::display::dp_px().is_some() {
        2.0
    } else {
        3.0
    }
}

/// The heights of the raised sheets in cells, bottom up: sheet `i` (1..=3) stands at `RAISED[i - 1]`.
/// Sheet 0 is the ground's.
const RAISED: [f32; 3] = [0.35, 0.75, 1.2];

/// `RAZDOR_MIST_RAISED=0..3` (read once): the raised sheets of the tilted views.
fn env_raised() -> Option<usize> {
    static ENV: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    *ENV.get_or_init(|| std::env::var("RAZDOR_MIST_RAISED").ok().and_then(|v| v.trim().parse::<usize>().ok()).map(|n| n.min(3)))
}

/// The raised sheets drawn in the tilted views: 3 on the desktop, 2 on a phone (the fill rate), or
/// `RAZDOR_MIST_RAISED`. 0 keeps the tilted views on the flat mist's ground sheets (the old look).
pub fn raised_count() -> usize {
    if cloud() {
        return 0;
    }
    if let Some(n) = env_raised() {
        return n;
    }
    if crate::ui::display::dp_px().is_some() {
        2
    } else {
        3
    }
}

/// The height of raised sheet `sheet` (1..=3) in cells.
pub fn raised_height(sheet: usize) -> f32 {
    RAISED[sheet.clamp(1, 3) - 1]
}

/// The clock the mist drifts on: the terrain's own (its water clock, in seconds), so the edge fog
/// and the mist move together.
fn clock() -> f32 {
    ((crate::ui::clock::now() * 1000.0) as i64 as f32 / 1000.0) % crate::ui::terrain::CLOCK_PERIOD
}

// The shader's vertex stage: the quad's uv, as the terrain's, and its colour's alpha (a raised
// sheet's vertices past a horizon are clear; a draw in white is opaque).
const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying highp vec2 uv;
varying lowp vec4 vcolor;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1.0);
    uv = texcoord;
    // The fork's colour attribute is raw bytes (0..255), as the default shader divides it.
    vcolor = color0 / 255.0;
}
"#;

// The fragment: the fog texture's alpha is the darkness. Sheets of domain-warped fractal noise
// ("billows", as rolling smoke): each sheet's coordinates are warped by two fractal fields that
// evolve with the Clock, and the warped field is the height. Each billow is shaded by its slope
// toward a light from the upper left (lighter tops, darker undersides) and multiplies the mist's
// colour; the first sheet's height is the wisp at the opening's edge, so the edge curls too.
// Everything drifts on the Clock and loops exactly with it (the lattice repeats every 64 cells,
// and every speed is a whole number of periods per CLOCK_PERIOD). `Sheets` (1..4) says how many.
//
// `Sheet` < 0 is the flat mist and the tilted views' ground: sheets 0 .. Sheets-1, opaque where
// unexplored (`mist_alpha`). `Sheet` ≥ 0 is one raised sheet of the tilted views: translucent,
// thinner the higher it stands (`Lift`), and wispier (only its high billows show).
const FRAGMENT: &str = r#"#version 100
precision highp float;

varying highp vec2 uv;
varying lowp vec4 vcolor;
uniform sampler2D Texture;
uniform vec2 FogSize;
uniform float Clock;
uniform float Sheets;
uniform float Sheet;
uniform float Lift;
uniform vec2 Parallax;
uniform vec3 MistRgb;

float hash2(vec2 q) {
    return fract(sin(dot(q, vec2(127.1, 311.7))) * 43758.5453);
}

// Value noise on a lattice that repeats every 64 cells (see terrain.rs CLOCK_PERIOD).
float vnoise(vec2 q) {
    vec2 i = floor(q);
    vec2 f = fract(q);
    vec2 u = f * f * (3.0 - 2.0 * f);
    vec2 a = mod(i, 64.0);
    vec2 b = mod(i + 1.0, 64.0);
    return mix(mix(hash2(a), hash2(vec2(b.x, a.y)), u.x), mix(hash2(vec2(a.x, b.y)), hash2(b), u.x), u.y);
}

// Fractal noise, 4 octaves in [0, 1]: each octave at exactly twice the last one's frequency, so a
// drift of whole periods in the lattice stays whole in every octave. The fine octaves are faint
// (gain 0.4): the billows read as soft smoke, not as marble.
float fbm(vec2 q) {
    float s = vnoise(q) * 0.5;
    q = q * 2.0 + vec2(1.7, 9.2);
    s += vnoise(q) * 0.2;
    q = q * 2.0 + vec2(1.7, 9.2);
    s += vnoise(q) * 0.08;
    q = q * 2.0 + vec2(1.7, 9.2);
    s += vnoise(q) * 0.032;
    return s / 0.812;
}

float sheet_scale(float i) { return i < 0.5 ? 0.18 : (i < 1.5 ? 0.33 : (i < 2.5 ? 0.6 : 1.1)); }
float sheet_height(float i) { return i < 0.5 ? 0.3 : (i < 1.5 ? 0.6 : (i < 2.5 ? 0.9 : 1.2)); }
float sheet_weight(float i) { return i < 0.5 ? 1.0 : (i < 1.5 ? 0.8 : (i < 2.5 ? 0.6 : 0.4)); }
// The drift of the sheet (its coordinates) and the two warp fields' evolution, per second.
vec2 sheet_drift(float i) {
    if (i < 0.5) return vec2(0.10, 0.04);
    if (i < 1.5) return vec2(-0.08, 0.12);
    if (i < 2.5) return vec2(0.14, -0.10);
    return vec2(-0.18, -0.12);
}
vec2 sheet_evolve1(float i) {
    if (i < 0.5) return vec2(0.07, -0.05);
    if (i < 1.5) return vec2(-0.05, 0.09);
    if (i < 2.5) return vec2(0.09, 0.06);
    return vec2(-0.11, -0.07);
}
vec2 sheet_evolve2(float i) {
    if (i < 0.5) return vec2(-0.06, 0.08);
    if (i < 1.5) return vec2(0.08, 0.04);
    if (i < 2.5) return vec2(-0.07, -0.08);
    return vec2(0.06, -0.10);
}

// Two octaves of the same fractal noise (the warp's swirls stay large; the cheap part of the shader).
float fbm2(vec2 q) {
    return (vnoise(q) * 0.5 + vnoise(q * 2.0 + vec2(1.7, 9.2)) * 0.2) / 0.7;
}

// The domain warp: the offset of the billows at the sheet's coordinates x (cells · scale, drifting).
vec2 warp(vec2 x, float i) {
    return 1.2 * vec2(fbm2(x + Clock * sheet_evolve1(i)), fbm2(x + vec2(5.2, 1.3) + Clock * sheet_evolve2(i)));
}

void main() {
    float d = texture2D(Texture, uv).a;
    // The cell coordinates: past the map's own texels (the extended draw) they run on.
    vec2 p = uv * FogSize;
    // The light from the upper left, and the flat ground's reading of it (the shade's zero).
    const vec3 LIGHT = vec3(-0.55, -0.65, 0.9);
    vec3 light = normalize(LIGHT);
    const float STEP = 0.05;
    const float SLOPE = 0.6;
    bool raised = Sheet >= 0.0;
    float first = raised ? Sheet : 0.0;
    float last = raised ? Sheet + 1.0 : Sheets;
    float acc = 0.0;
    float wsum = 0.0;
    float wisp = 0.5;
    float warm = 0.5;
    float field = 0.5;
    for (int i = 0; i < 4; i++) {
        float fi = float(i);
        if (fi >= last) break;
        if (fi < first) continue;
        vec2 x = (p + Parallax * sheet_height(fi)) * sheet_scale(fi) + Clock * sheet_drift(fi);
        vec2 wv = warp(x, fi);
        vec2 q = x + wv;
        float h = fbm(q);
        // The slope by two more samples of the field (forward differences), as a normal.
        float gx = (fbm(q + vec2(STEP, 0.0)) - h) / STEP;
        float gy = (fbm(q + vec2(0.0, STEP)) - h) / STEP;
        vec3 n = normalize(vec3(-gx * SLOPE, -gy * SLOPE, 1.0));
        float shade = clamp(0.9 + 1.2 * (dot(n, light) - light.z) + 0.25 * (h - 0.5), 0.65, 1.15);
        if (fi == first) {
            wisp = clamp(0.5 + (h - 0.5) * 2.0, 0.0, 1.0);
            warm = clamp((wv.x / 1.2 - 0.5) * 2.0, 0.0, 1.0);
            field = h;
        }
        acc += sheet_weight(fi) * shade;
        wsum += sheet_weight(fi);
    }
    float shade = acc / max(wsum, 0.001);
    // A faint cool-to-warm drift of the colour across the mist.
    vec3 tint = mix(vec3(0.96, 1.0, 1.06), vec3(1.05, 1.0, 0.94), warm);
    vec3 base = MistRgb * shade * tint;
    float a;
    if (raised) {
        // Translucent, a little less the higher it stands; the wisps are the billows above the mean.
        float rise = clamp(Lift / 1.2, 0.0, 1.0);
        // Its rim fades over the last two cells of the fog's texture: past the map the ground is black, and a
        // lifted sheet would end there in a hard edge above it.
        vec2 inside = min(uv, 1.0 - uv) * FogSize;
        float rim = smoothstep(0.0, 2.0, min(inside.x, inside.y));
        a = d * 0.42 * (1.0 - 0.5 * rise) * smoothstep(0.44, 0.72, field) * rim;
    } else {
        a = d >= 0.995 ? 1.0 : clamp(d + 0.5 * (wisp - 0.5) * d * (1.0 - d) * 4.0, 0.0, 1.0);
    }
    gl_FragColor = vec4(base, a * vcolor.a);
}
"#;

local! {
    /// The material, loaded at the first draw (None when the shader does not compile: black then).
    static MATERIAL: RefCell<Option<Option<Material>>> = RefCell::new(None);
}

fn load() -> Option<Material> {
    let uniforms = [
        ("FogSize", UniformType::Float2),
        ("Clock", UniformType::Float1),
        ("Sheets", UniformType::Float1),
        ("Sheet", UniformType::Float1),
        ("Lift", UniformType::Float1),
        ("Parallax", UniformType::Float2),
        ("MistRgb", UniformType::Float3),
    ];
    // The mist is blended over the ground by its alpha (the default pipeline ignores it).
    let params = MaterialParams {
        uniforms: uniforms.iter().map(|(n, t)| UniformDesc::new(n, *t)).collect(),
        pipeline_params: crate::ui::sharp::alpha_pipeline(),
        ..Default::default()
    };
    load_material(ShaderSource::Glsl { vertex: VERTEX, fragment: FRAGMENT }, params).ok()
}

fn set_common(m: &Material, tex: &Texture2D, sheets: f32, sheet: f32, lift: f32, parallax: Vec2) {
    m.set_uniform("FogSize", vec2(tex.width(), tex.height()));
    m.set_uniform("Clock", clock());
    m.set_uniform("Sheets", sheets);
    m.set_uniform("Sheet", sheet);
    m.set_uniform("Lift", lift);
    m.set_uniform("Parallax", parallax);
    m.set_uniform("MistRgb", vec3(MIST_RGB[0], MIST_RGB[1], MIST_RGB[2]));
}

/// Draws the fog texture `tex` as the mist over `dest` with the sheets `sheets` and `parallax`
/// (`sheet` -1: the flat mist's ground sheets), showing its texels `src` (`None`: all of it, as
/// `minimap::draw_fog`). Without the shader it draws the plain texture (black).
fn draw_quad(tex: &Texture2D, dest: Rect, src: Option<Rect>, sheets: f32, parallax: Vec2) {
    let params = DrawTextureParams { dest_size: Some(dest.size()), source: src, ..Default::default() };
    MATERIAL.with(|cell| {
        let mut cell = cell.borrow_mut();
        match cell.get_or_insert_with(load).as_ref() {
            Some(m) => {
                gl_use_material(m);
                set_common(m, tex, sheets, -1.0, 0.0, parallax);
                draw_texture_ex(tex, dest.x, dest.y, WHITE, params);
                gl_use_default_material();
            }
            None => draw_texture_ex(tex, dest.x, dest.y, WHITE, params),
        }
    });
}

/// Draws the fog texture `tex` into `dest` as the flat mist (every sheet, with `parallax`). The
/// flat view's call: `parallax` is zero there.
pub fn draw(tex: &Texture2D, dest: Rect, src: Option<Rect>, parallax: Vec2) {
    if cloud() {
        return draw_cloud(tex, dest, src);
    }
    draw_quad(tex, dest, src, sheets(), parallax);
}

/// Draws the fog texture `tex` into `dest` as the tilted views' ground: sheet 0 alone, opaque where
/// unexplored, with no parallax (the raised sheets carry the rest, [`raised`]). Without the shader
/// it draws the plain texture (black).
pub fn draw_ground(tex: &Texture2D, dest: Rect, src: Option<Rect>) {
    if cloud() {
        return draw_cloud(tex, dest, src);
    }
    draw_quad(tex, dest, src, 1.0, Vec2::ZERO);
}

/// Runs `draw` with the material of raised sheet `sheet` (1..=3, lifted `lift` cells) and the fog
/// texture `tex` set: the caller draws the quad or the mesh of the sheet inside it (in the space the
/// sheet stands in). Translucent, wispier than the ground's. Without the shader nothing is drawn: a
/// raised sheet is never the black.
pub fn raised(tex: &Texture2D, sheet: usize, lift: f32, draw: impl FnOnce()) {
    if cloud() {
        // The soft look has no raised sheets (`raised_count` is 0): nothing to draw.
        drop((tex, lift, draw));
        return;
    }
    MATERIAL.with(|cell| {
        let mut cell = cell.borrow_mut();
        if let Some(m) = cell.get_or_insert_with(load).as_ref() {
            gl_use_material(m);
            set_common(m, tex, sheets(), sheet.clamp(1, 3) as f32, lift, Vec2::ZERO);
            draw();
            gl_use_default_material();
        }
    });
}

// ---------------------------------------------------------------------------------------------
// The soft cloud look (the default; `RAZDOR_MIST_STYLE=old` draws the billows above, for comparison):
// the mist as soft cloud masses. One pre-baked periodic noise texture (its red and green channels, two
// scales) and three slow sine waves shape the masses; there is no domain warp and one layer, drawn in
// one pass over the fog quad (the flat view's, and the tilted views' ground: no raised sheets).
// ---------------------------------------------------------------------------------------------

/// The look of the mist (`RAZDOR_MIST_STYLE`, read once).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// The domain-warped billows (the first look, kept for comparison).
    Billow,
    /// The soft cloud masses (the default).
    Soft,
}

/// The look drawn when `RAZDOR_MIST_STYLE` is not set.
const DEFAULT_STYLE: Style = Style::Soft;

/// The look named by `RAZDOR_MIST_STYLE` (`old` or `a`, or the look's name); None for anything else.
pub fn parse_style(v: &str) -> Option<Style> {
    match v.trim().to_ascii_lowercase().as_str() {
        "old" | "billow" | "billows" => Some(Style::Billow),
        "a" | "soft" => Some(Style::Soft),
        _ => None,
    }
}

/// The mist's look: `RAZDOR_MIST_STYLE` (a dev switch), else [`DEFAULT_STYLE`].
pub fn style() -> Style {
    static ENV: std::sync::OnceLock<Style> = std::sync::OnceLock::new();
    *ENV.get_or_init(|| std::env::var("RAZDOR_MIST_STYLE").ok().and_then(|v| parse_style(&v)).unwrap_or(DEFAULT_STYLE))
}

/// Whether the mist is drawn in the soft cloud look (not the billows).
pub fn cloud() -> bool {
    style() != Style::Billow
}

/// The noise texture's side in texels: `NOISE_LATTICE` lattice cells wide, 16 texels a cell. Finer
/// than the cloud needs, so the texture's bilinear creases stay below what a pixel shows.
const NOISE_TEXELS: usize = 1024;
/// The noise's period in lattice cells (the texture repeats every this many).
const NOISE_LATTICE: usize = 64;

/// A lattice cell's hash in [0, 1) (the bake's noise; deterministic, so the look is the same every run).
fn lattice_hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h >> 8) as f32 / 16_777_216.0
}

/// Smooth value noise at the point `(x, y)` (in lattice cells), periodic in `NOISE_LATTICE` cells.
fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let period = NOISE_LATTICE as i32;
    let h = |dx: i32, dy: i32| lattice_hash((x0 as i32 + dx).rem_euclid(period), (y0 as i32 + dy).rem_euclid(period), seed);
    let a = h(0, 0) + (h(1, 0) - h(0, 0)) * ux;
    let b = h(0, 1) + (h(1, 1) - h(0, 1)) * ux;
    a + (b - a) * uy
}

/// The noise texture's RGBA bytes: red is one periodic value noise, green another (independent seeds),
/// each stretched to the full range over the texture; blue and alpha are unused (255).
fn bake_noise() -> Vec<u8> {
    let (n, per) = (NOISE_TEXELS, NOISE_TEXELS / NOISE_LATTICE);
    let mut out = vec![255u8; n * n * 4];
    for c in 0..2 {
        let field: Vec<f32> = (0..n * n)
            .map(|t| value_noise((t % n) as f32 / per as f32, (t / n) as f32 / per as f32, c as u32 + 1))
            .collect();
        let lo = field.iter().cloned().fold(f32::MAX, f32::min);
        let hi = field.iter().cloned().fold(f32::MIN, f32::max);
        for (t, v) in field.iter().enumerate() {
            out[t * 4 + c] = (((v - lo) / (hi - lo).max(1e-6)) * 255.0).round() as u8;
        }
    }
    out
}

local! {
    /// The noise texture, baked at the first draw of the soft look.
    static NOISE: RefCell<Option<Texture2D>> = RefCell::new(None);
    /// The soft look's material, loaded at the first draw (None when the shader does not compile).
    static CLOUD: RefCell<Option<Option<Material>>> = RefCell::new(None);
}

/// The noise texture (baked once, repeating: the field is periodic in `NOISE_LATTICE` cells, and mipmapped
/// for the distant ground, where a pixel spans several cells).
fn noise_texture() -> Texture2D {
    NOISE.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| {
                let tex = Texture2D::from_rgba8(NOISE_TEXELS as u16, NOISE_TEXELS as u16, &bake_noise());
                tex.set_filter(FilterMode::Linear);
                // SAFETY: called on the main thread while macroquad runs, as every texture call.
                let gl = unsafe { get_internal_gl() };
                gl.quad_context.texture_set_wrap(tex.raw_miniquad_id(), TextureWrap::Repeat, TextureWrap::Repeat);
                gl.quad_context.texture_generate_mipmaps(tex.raw_miniquad_id());
                gl.quad_context.texture_set_min_filter(tex.raw_miniquad_id(), MqFilter::Linear, MipmapFilterMode::Linear);
                tex
            })
            .clone()
    })
}

// The soft fragment. The fog texture's alpha is the darkness, as the billows'. The density is the
// noise texture's two octaves (red at the cloud's size, green at a third of it) with the bank's three
// slow sine waves added; its top (0 a gap … 1 a dense top) and its sun bump (the density rising toward
// the upper left: lit) shade it, so the masses have bright tops and darker gaps. The drift is a whole
// number of lattice units per CLOCK_PERIOD (6400 s) and the waves' phases turn whole times in it, so
// the Clock's wrap is seamless.
const FRAGMENT_CLOUD: &str = r#"#version 100
precision highp float;

varying highp vec2 uv;
varying lowp vec4 vcolor;
uniform sampler2D Texture;
uniform sampler2D NoiseTex;
uniform vec2 FogSize;
uniform float Clock;
uniform vec3 MistRgb;

const vec2 SUN = vec2(-0.5547, -0.8321);
const float NOISE_PERIOD = 64.0;
// Cells per noise lattice cell (the clouds' size), and the drift in lattice units a second (multiples of 0.01).
const float UNIT = 32.0;
const vec2 DRIFT = vec2(0.01, 0.0);

// The bank: three slow waves over the cells (Clock in seconds; the phases are 2π/6400 and its multiples), in -1..1.
float bank(vec2 p) {
    float w = sin(0.045 * dot(p, vec2(0.8, 0.6)) + Clock * 0.00098175)
        + 0.8 * cos(0.031 * dot(p, vec2(-0.5, 0.87)) - Clock * 0.0019635)
        + 0.6 * sin(0.017 * dot(p, vec2(0.9, -0.4)) + Clock * 0.0029452);
    return w / 2.4;
}

// The cloud's mass at the cell p: two octaves of the noise texture, 0..1 with mean ½.
float mass_at(vec2 p) {
    vec2 n = p / UNIT + DRIFT * Clock;
    float a = texture2D(NoiseTex, n / NOISE_PERIOD).r;
    float b = texture2D(NoiseTex, (3.0 * n + vec2(0.37, 0.61)) / NOISE_PERIOD).g;
    return 0.55 * a + 0.45 * b;
}

void main() {
    float d = texture2D(Texture, uv).a;
    vec2 p = uv * FogSize;
    float env = bank(p);
    float dn = mass_at(p) + 0.25 * env;
    float ds = mass_at(p + SUN * 2.0) + 0.25 * env;
    float top = smoothstep(0.42, 0.68, dn);
    float bump = clamp((dn - ds) * 10.0, -0.25, 0.25);
    float shade = clamp(0.62 + 0.42 * top + bump, 0.5, 1.15);
    vec3 tint = vec3(1.0 + 0.05 * env, 1.0, 1.0 - 0.05 * env);
    // The opening's edge: the cloud's top is the wisp (as the billows' first sheet), so the edge curls.
    float a = d >= 0.995 ? 1.0 : clamp(d + 0.5 * (top - 0.5) * d * (1.0 - d) * 4.0, 0.0, 1.0);
    gl_FragColor = vec4(MistRgb * shade * tint, a * vcolor.a);
}
"#;

fn load_cloud() -> Option<Material> {
    let uniforms = [("FogSize", UniformType::Float2), ("Clock", UniformType::Float1), ("MistRgb", UniformType::Float3)];
    let params = MaterialParams {
        uniforms: uniforms.iter().map(|(n, t)| UniformDesc::new(n, *t)).collect(),
        textures: vec!["NoiseTex".to_string()],
        pipeline_params: crate::ui::sharp::alpha_pipeline(),
    };
    let m = load_material(ShaderSource::Glsl { vertex: VERTEX, fragment: FRAGMENT_CLOUD }, params).ok()?;
    m.set_texture("NoiseTex", noise_texture());
    Some(m)
}

/// Draws the fog texture `tex` into `dest` (showing its texels `src`) as the soft look. Without the
/// shader it draws the plain texture (black), as the billows do.
fn draw_cloud(tex: &Texture2D, dest: Rect, src: Option<Rect>) {
    let params = DrawTextureParams { dest_size: Some(dest.size()), source: src, ..Default::default() };
    let drawn = CLOUD.with(|cell| {
        let mut cell = cell.borrow_mut();
        match cell.get_or_insert_with(load_cloud).as_ref() {
            Some(m) => {
                gl_use_material(m);
                m.set_uniform("FogSize", vec2(tex.width(), tex.height()));
                m.set_uniform("Clock", clock());
                m.set_uniform("MistRgb", vec3(MIST_RGB[0], MIST_RGB[1], MIST_RGB[2]));
                draw_texture_ex(tex, dest.x, dest.y, WHITE, params.clone());
                gl_use_default_material();
                true
            }
            None => false,
        }
    });
    if !drawn {
        draw_texture_ex(tex, dest.x, dest.y, WHITE, params);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unexplored_is_opaque_and_lit_is_clear_whatever_the_wisp() {
        for k in 0..=10 {
            let w = k as f32 / 10.0;
            assert_eq!(mist_alpha(1.0, w), 1.0, "unexplored, wisp {w}");
            assert_eq!(mist_alpha(0.0, w), 0.0, "lit, wisp {w}");
        }
    }

    #[test]
    fn the_alpha_stays_in_range_and_rises_with_the_darkness() {
        for k in 0..=10 {
            let w = k as f32 / 10.0;
            let mut last = 0.0f32;
            for step in 0..=1000 {
                let d = step as f32 / 1000.0;
                let a = mist_alpha(d, w);
                assert!((0.0..=1.0).contains(&a), "d {d}, w {w}: {a}");
                assert!(a >= last - 1e-5, "d {d}, w {w}: {a} after {last}");
                last = a;
            }
        }
    }

    #[test]
    fn the_wisp_only_moves_the_opening_s_edge() {
        // Deep inside the band the wisp changes the alpha; at the ends it cannot.
        assert!(mist_alpha(0.5, 1.0) > mist_alpha(0.5, 0.0));
        assert_eq!(mist_alpha(0.0, 1.0), mist_alpha(0.0, 0.0));
    }

    #[test]
    fn every_sheet_speed_loops_exactly_with_the_clock() {
        // The shader's speeds (drift and the two warp fields' evolution, per sheet): each a whole
        // number of lattice periods (64 cells) per CLOCK_PERIOD, so the wrap is seamless.
        let speeds = [
            [0.10f32, 0.04], [0.07, -0.05], [-0.06, 0.08],
            [-0.08, 0.12], [-0.05, 0.09], [0.08, 0.04],
            [0.14, -0.10], [0.09, 0.06], [-0.07, -0.08],
            [-0.18, -0.12], [-0.11, -0.07], [0.06, -0.10],
        ];
        for v in speeds.iter().flatten() {
            let periods = v * crate::ui::terrain::CLOCK_PERIOD / 64.0;
            assert!((periods - periods.round()).abs() < 0.01, "speed {v}: {periods} periods");
        }
    }

    #[test]
    fn the_flat_view_has_no_parallax_and_the_tilt_shifts_the_sheets_away_from_the_camera() {
        // Yaw 0 looks north: the shift runs along +y, and it grows as the view flattens.
        let steep = parallax(0.0, 75.0);
        let flat = parallax(0.0, 35.0);
        assert!(steep.x.abs() < 1e-6 && steep.y > 0.0);
        assert!(flat.y > steep.y, "{flat:?} vs {steep:?}");
        let turned = parallax(90.0, 55.0);
        assert!(turned.x > 0.0 && turned.y.abs() < 1e-6, "{turned:?}");
    }

    #[test]
    fn the_raised_sheets_rise_bottom_up() {
        // Sheet 1 stands above the ground, each one above the last.
        let heights: Vec<f32> = (1..=3).map(raised_height).collect();
        assert!(heights[0] > 0.0);
        assert!(heights.windows(2).all(|w| w[0] < w[1]), "{heights:?}");
        assert_eq!(raised_height(0), raised_height(1), "sheet 0 is clamped to the first raised one");
    }

    #[test]
    fn the_style_names_parse_and_the_default_is_a_cloud_look() {
        assert_eq!(parse_style("old"), Some(Style::Billow));
        assert_eq!(parse_style(" A "), Some(Style::Soft));
        assert_eq!(parse_style("soft"), Some(Style::Soft));
        assert_eq!(parse_style("b"), None);
        assert_eq!(parse_style("painted"), None);
        assert_eq!(DEFAULT_STYLE, Style::Soft);
    }

    #[test]
    fn the_noise_texture_wraps_seamlessly_and_is_centred() {
        let bytes = bake_noise();
        let n = NOISE_TEXELS;
        assert_eq!(bytes.len(), n * n * 4);
        for c in 0..2 {
            let px = |x: usize, y: usize| bytes[(y * n + x) * 4 + c] as i32;
            // The last texel column and the first are neighbours on the repeating texture: their step is no
            // bigger than the largest step between ordinary neighbours (the field is continuous over the wrap).
            let mut inner = 0;
            for y in 0..n {
                for x in 0..n - 1 {
                    inner = inner.max((px(x + 1, y) - px(x, y)).abs());
                }
            }
            for y in 0..n {
                let seam = (px(n - 1, y) - px(0, y)).abs();
                assert!(seam <= inner, "channel {c}, row {y}: seam {seam} > {inner}");
            }
            let mean = (0..n * n).map(|t| bytes[t * 4 + c] as f32).sum::<f32>() / (n * n) as f32;
            assert!((mean - 127.5).abs() < 25.0, "channel {c}: mean {mean}");
        }
        // The two octaves are different fields.
        assert_ne!(bytes[0], bytes[1]);
    }

    #[test]
    fn the_cloud_drifts_and_waves_loop_exactly_with_the_clock() {
        // The shader's drifts, in lattice units a second (the texture repeats every NOISE_LATTICE): each a
        // whole number of periods per CLOCK_PERIOD, the octave's 3× included.
        let period = crate::ui::terrain::CLOCK_PERIOD / NOISE_LATTICE as f32;
        for v in [[0.01f32, 0.0]] {
            for c in v {
                let turns = c * period;
                assert!((turns - turns.round()).abs() < 1e-3, "drift {c}: {turns} periods");
                let octave = 3.0 * turns;
                assert!((octave - octave.round()).abs() < 1e-3, "octave of {c}: {octave}");
            }
        }
        // The bank's waves: 2π k / CLOCK_PERIOD for k = 1, 2, 3 (the shader's literals), whole turns in the clock.
        for k in [1.0f32, 2.0, 3.0] {
            let omega = std::f32::consts::TAU * k / crate::ui::terrain::CLOCK_PERIOD;
            let turns = omega * crate::ui::terrain::CLOCK_PERIOD / std::f32::consts::TAU;
            assert!((turns - turns.round()).abs() < 1e-3, "wave {k}: {turns} turns");
        }
    }
}
