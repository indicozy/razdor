//! Razdor's «Туман вместо черноты» (`Settings::map_mist`): the war fog's unexplored land is a mist
//! instead of black. Three looks: «Нет» (the original's black, no mist is drawn), «Простой» (a plain
//! mist colour with a soft edge: one cheap pass) and «Облака» (soft clouds drifting over it, from a
//! pre-baked noise texture and slow sine waves). Style only: no rule, fog, path or save reads it.
//!
//! The fog texture (`minimap::fog_layer_box`: one texel per cell, its alpha the shown darkness) is
//! drawn through a material on the same quad and texels as the black fog: where the darkness is 1 the
//! mist is opaque, where it is 0 nothing shows, and the opening's edge is soft ([`mist_alpha`]). The
//! mist's colour is the lighting's: the daylight tint is drawn over it after the world (see
//! `world_view`), and a bent view's sprite run takes the frame's tint here ([`set_frame_tint`]).
//! The minimap keeps its black.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Deserializer, Serialize};

use macroquad::miniquad::{FilterMode as MqFilter, MipmapFilterMode, TextureWrap};
use macroquad::prelude::*;
use razdor::local;

/// The mist's neutral grey-white (the colour with the lighting off, and the edge fog beyond the map).
pub const MIST_RGB: [f32; 3] = [0.74, 0.77, 0.82];

/// The soft cloud look's base colour: a bright, near-white cloud (the lighting's palette is drawn over
/// it after the fog, so dawn, dusk and night tint it).
pub const CLOUD_RGB: [f32; 3] = [0.93, 0.94, 0.96];

/// The look of the unexplored land (`Settings::map_mist`, «Туман вместо черноты»).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MistMode {
    /// «Нет»: the original's black (no mist is drawn).
    #[default]
    Off,
    /// «Простой»: a plain mist colour with a soft edge (one cheap pass).
    Flat,
    /// «Облака»: soft clouds drifting over the unexplored land.
    Clouds,
}

/// The mode named by `RAZDOR_MIST` (a dev switch): `1` or `clouds`, `flat` or `simple`; else off.
pub fn parse(v: &str) -> MistMode {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "clouds" => MistMode::Clouds,
        "flat" | "simple" => MistMode::Flat,
        _ => MistMode::Off,
    }
}

/// The setting as a file holds it: the old bool (true is «Облака») or the mode's name.
pub fn de_mode<'de, D: Deserializer<'de>>(d: D) -> Result<MistMode, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored {
        Flag(bool),
        Mode(MistMode),
    }
    Ok(match Stored::deserialize(d)? {
        Stored::Flag(true) => MistMode::Clouds,
        Stored::Flag(false) => MistMode::Off,
        Stored::Mode(m) => m,
    })
}

/// The setting, pushed every frame by `display::follow_settings`.
static MODE: AtomicU8 = AtomicU8::new(0);

/// Takes the setting «Туман вместо черноты» in (every frame).
pub fn set_pref(mode: MistMode) {
    MODE.store(mode as u8, Ordering::Relaxed);
}

/// The look of the unexplored land.
pub fn mode() -> MistMode {
    match MODE.load(Ordering::Relaxed) {
        1 => MistMode::Flat,
        2 => MistMode::Clouds,
        _ => MistMode::Off,
    }
}

/// Whether the unexplored land is drawn as mist (instead of black).
pub fn on() -> bool {
    mode() != MistMode::Off
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

// ---------------------------------------------------------------------------------------------
// The soft cloud look (`MistMode::Clouds`): the mist as soft cloud masses. One pre-baked periodic
// noise texture (its red and green channels, two scales) and three slow sine waves shape the masses;
// there is no domain warp and one layer, drawn in one pass over the fog quad.
// ---------------------------------------------------------------------------------------------

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
    let quintic = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (ux, uy) = (quintic(fx), quintic(fy));
    let period = NOISE_LATTICE as i32;
    let h = |dx: i32, dy: i32| lattice_hash((x0 as i32 + dx).rem_euclid(period), (y0 as i32 + dy).rem_euclid(period), seed);
    let a = h(0, 0) + (h(1, 0) - h(0, 0)) * ux;
    let b = h(0, 1) + (h(1, 1) - h(0, 1)) * ux;
    a + (b - a) * uy
}

/// A periodic [1 2 1] / 4 blur of an `n` × `n` field, along each axis: the texel grid's creases go.
fn blur(f: &[f32], n: usize) -> Vec<f32> {
    let mut h = vec![0.0f32; n * n];
    for j in 0..n {
        for i in 0..n {
            h[j * n + i] = 0.25 * f[j * n + (i + n - 1) % n] + 0.5 * f[j * n + i] + 0.25 * f[j * n + (i + 1) % n];
        }
    }
    let mut out = vec![0.0f32; n * n];
    for j in 0..n {
        for i in 0..n {
            out[j * n + i] = 0.25 * h[((j + n - 1) % n) * n + i] + 0.5 * h[j * n + i] + 0.25 * h[((j + 1) % n) * n + i];
        }
    }
    out
}

/// The noise texture's RGBA bytes: red is one periodic value noise, green another (independent seeds),
/// blurred once and stretched to the full range over the texture; blue and alpha are unused (255).
fn bake_noise() -> Vec<u8> {
    let (n, per) = (NOISE_TEXELS, NOISE_TEXELS / NOISE_LATTICE);
    let mut out = vec![255u8; n * n * 4];
    for c in 0..2 {
        let raw: Vec<f32> = (0..n * n)
            .map(|t| value_noise((t % n) as f32 / per as f32, (t / n) as f32 / per as f32, c as u32 + 1))
            .collect();
        let field = blur(&raw, n);
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

// The soft fragment. The fog texture's alpha is the darkness, as the billows'. Two octaves of the noise
// texture shape the cloud: the masses (the red channel, at the cloud's size) and the puffs (the green
// channel, at a smaller size, rotated so the two grids never line up). The bank's three slow sine waves
// add to the density. The density's top (0 a gap … 1 a dense top) and its sun bump (the density rising
// toward the upper left: lit) shade it: near-white tops, a slightly darker underside, never grey. The
// drifts are whole lattice units per CLOCK_PERIOD (6400 s) and the waves turn whole times in it, so the
// Clock's wrap is seamless. A tiny dither breaks the banding of the smooth gradients.
const FRAGMENT_CLOUD: &str = r#"#version 100
precision highp float;

varying highp vec2 uv;
varying lowp vec4 vcolor;
uniform sampler2D Texture;
uniform sampler2D NoiseTex;
uniform vec2 FogSize;
uniform float Clock;
uniform vec3 MistRgb;
uniform vec3 Light;

const vec2 SUN = vec2(-0.5547, -0.8321);
const float NOISE_PERIOD = 64.0;
// The masses: cells per noise lattice cell, and their drift in lattice units a second (multiples of 0.01).
const float UNIT = 32.0;
const vec2 DRIFT = vec2(0.01, 0.0);
// The puffs: a smaller scale, rotated 37 degrees (cos, sin), drifting the other way.
const float PUFF = 13.0;
const mat2 TURN = mat2(0.7986, 0.6018, -0.6018, 0.7986);
const vec2 DRIFT2 = vec2(-0.02, 0.01);

// The bank: three slow waves over the cells (Clock in seconds; the phases are 2π/6400 and its multiples), in -1..1.
float bank(vec2 p) {
    float w = sin(0.045 * dot(p, vec2(0.8, 0.6)) + Clock * 0.00098175)
        + 0.8 * cos(0.031 * dot(p, vec2(-0.5, 0.87)) - Clock * 0.0019635)
        + 0.6 * sin(0.017 * dot(p, vec2(0.9, -0.4)) + Clock * 0.0029452);
    return w / 2.4;
}

// The cloud's density at the cell p: the masses and the puffs (two fetches each), 0..1 with mean ½.
float mass_at(vec2 p) {
    float a = texture2D(NoiseTex, (p / UNIT + DRIFT * Clock) / NOISE_PERIOD).r;
    float b = texture2D(NoiseTex, (TURN * (p / PUFF) + DRIFT2 * Clock + vec2(0.37, 0.61)) / NOISE_PERIOD).g;
    return 0.6 * a + 0.4 * b;
}

void main() {
    float d = texture2D(Texture, uv).a;
    vec2 p = uv * FogSize;
    float env = bank(p);
    float dn = mass_at(p) + 0.25 * env;
    float ds = mass_at(p + SUN * 2.0) + 0.25 * env;
    float top = smoothstep(0.42, 0.68, dn);
    float bump = clamp((dn - ds) * 10.0, -0.12, 0.12);
    float shade = clamp(0.84 + 0.20 * top + bump, 0.7, 1.08);
    vec3 tint = vec3(1.0 + 0.04 * env, 1.0, 1.0 - 0.04 * env);
    // The opening's edge: the cloud's top is the wisp (as the billows' first sheet), so the edge curls.
    float a = d >= 0.995 ? 1.0 : clamp(d + 0.5 * (top - 0.5) * d * (1.0 - d) * 4.0, 0.0, 1.0);
    vec3 col = MistRgb * shade * tint;
    // A sine-free hash (`sin` of a large argument is NaN on some GPUs, and a NaN colour renders black).
    vec3 q = fract(vec3(gl_FragCoord.xyx) * 0.1031);
    q += dot(q, q.yzx + 33.33);
    float dither = fract((q.x + q.y) * q.z);
    gl_FragColor = vec4(col * Light + (dither - 0.5) / 255.0, a * vcolor.a);
}
"#;

fn load_cloud() -> Option<Material> {
    let uniforms = [("FogSize", UniformType::Float2), ("Clock", UniformType::Float1), ("MistRgb", UniformType::Float3), ("Light", UniformType::Float3)];
    let params = MaterialParams {
        uniforms: uniforms.iter().map(|(n, t)| UniformDesc::new(n, *t)).collect(),
        textures: vec!["NoiseTex".to_string()],
        pipeline_params: crate::ui::sharp::alpha_pipeline(),
    };
    let m = load_material(ShaderSource::Glsl { vertex: VERTEX, fragment: FRAGMENT_CLOUD }, params).ok()?;
    m.set_texture("NoiseTex", noise_texture());
    Some(m)
}

// The frame's light tint (`daylight::Light::tint`), set once per frame before the world is drawn.
thread_local! {
    static FRAME_TINT: std::cell::Cell<[f32; 3]> = const { std::cell::Cell::new([1.0; 3]) };
}

/// Sets the frame's light tint (`[1; 3]` with the lighting off).
pub fn set_frame_tint(tint: [f32; 3]) {
    FRAME_TINT.with(|t| t.set(tint));
}

/// The frame's light tint, for a cloud drawn before the daylight (a bent view's sprite run: the
/// picture takes the daylight after the world, the sprites' run does not).
pub fn frame_tint() -> [f32; 3] {
    FRAME_TINT.with(|t| t.get())
}

/// Draws the fog texture `tex` into `dest` (showing its texels `src`) as the soft look, lit by `light`
/// (the palette's tint, `[1; 3]` when the daylight is drawn over it afterwards). Without the shader it
/// draws the plain texture (black).
fn draw_cloud(tex: &Texture2D, dest: Rect, src: Option<Rect>, light: [f32; 3]) {
    let params = DrawTextureParams { dest_size: Some(dest.size()), source: src, ..Default::default() };
    let drawn = CLOUD.with(|cell| {
        let mut cell = cell.borrow_mut();
        match cell.get_or_insert_with(load_cloud).as_ref() {
            Some(m) => {
                gl_use_material(m);
                m.set_uniform("FogSize", vec2(tex.width(), tex.height()));
                m.set_uniform("Clock", clock());
                m.set_uniform("MistRgb", vec3(CLOUD_RGB[0], CLOUD_RGB[1], CLOUD_RGB[2]));
                m.set_uniform("Light", vec3(light[0], light[1], light[2]));
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

// The plain look's fragment: the mist's colour, with the fog texture's alpha as the edge and nothing
// else (one cheap pass; the phones' choice). The alpha is the darkness: opaque where unexplored.
const FRAGMENT_PLAIN: &str = r#"#version 100
precision highp float;

varying highp vec2 uv;
varying lowp vec4 vcolor;
uniform sampler2D Texture;
uniform vec3 MistRgb;
uniform vec3 Light;

void main() {
    float d = texture2D(Texture, uv).a;
    float a = d >= 0.995 ? 1.0 : d;
    gl_FragColor = vec4(MistRgb * Light, a * vcolor.a);
}
"#;

local! {
    /// The plain look's material, loaded at the first draw (None when the shader does not compile).
    static PLAIN: RefCell<Option<Option<Material>>> = RefCell::new(None);
}

fn load_plain() -> Option<Material> {
    let uniforms = [("MistRgb", UniformType::Float3), ("Light", UniformType::Float3)];
    let params = MaterialParams {
        uniforms: uniforms.iter().map(|(n, t)| UniformDesc::new(n, *t)).collect(),
        textures: Vec::new(),
        pipeline_params: crate::ui::sharp::alpha_pipeline(),
    };
    load_material(ShaderSource::Glsl { vertex: VERTEX, fragment: FRAGMENT_PLAIN }, params).ok()
}

/// Draws the fog texture `tex` into `dest` (showing its texels `src`) as the plain look, lit by `light`.
fn draw_plain(tex: &Texture2D, dest: Rect, src: Option<Rect>, light: [f32; 3]) {
    let params = DrawTextureParams { dest_size: Some(dest.size()), source: src, ..Default::default() };
    let drawn = PLAIN.with(|cell| {
        let mut cell = cell.borrow_mut();
        match cell.get_or_insert_with(load_plain).as_ref() {
            Some(m) => {
                gl_use_material(m);
                m.set_uniform("MistRgb", vec3(MIST_RGB[0], MIST_RGB[1], MIST_RGB[2]));
                m.set_uniform("Light", vec3(light[0], light[1], light[2]));
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

/// Draws the fog texture `tex` into `dest` (showing its texels `src`) as the mist of the setting's look,
/// lit by `light` (`[1; 3]` when the daylight is drawn over it afterwards).
pub fn draw(tex: &Texture2D, dest: Rect, src: Option<Rect>, light: [f32; 3]) {
    match mode() {
        MistMode::Clouds => draw_cloud(tex, dest, src, light),
        MistMode::Flat => draw_plain(tex, dest, src, light),
        MistMode::Off => {}
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
    fn the_setting_names_parse_and_the_default_is_the_original_black() {
        assert_eq!(parse("1"), MistMode::Clouds);
        assert_eq!(parse(" Flat "), MistMode::Flat);
        assert_eq!(parse("nope"), MistMode::Off);
        assert_eq!(MistMode::default(), MistMode::Off);
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
        // whole number of periods per CLOCK_PERIOD.
        let period = crate::ui::terrain::CLOCK_PERIOD / NOISE_LATTICE as f32;
        for v in [[0.01f32, 0.0], [-0.02, 0.01]] {
            for c in v {
                let turns = c * period;
                assert!((turns - turns.round()).abs() < 1e-3, "drift {c}: {turns} periods");
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
