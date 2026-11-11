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
    draw_quad(tex, dest, src, sheets(), parallax);
}

/// Draws the fog texture `tex` into `dest` as the tilted views' ground: sheet 0 alone, opaque where
/// unexplored, with no parallax (the raised sheets carry the rest, [`raised`]). Without the shader
/// it draws the plain texture (black).
pub fn draw_ground(tex: &Texture2D, dest: Rect, src: Option<Rect>) {
    draw_quad(tex, dest, src, 1.0, Vec2::ZERO);
}

/// Runs `draw` with the material of raised sheet `sheet` (1..=3, lifted `lift` cells) and the fog
/// texture `tex` set: the caller draws the quad or the mesh of the sheet inside it (in the space the
/// sheet stands in). Translucent, wispier than the ground's. Without the shader nothing is drawn: a
/// raised sheet is never the black.
pub fn raised(tex: &Texture2D, sheet: usize, lift: f32, draw: impl FnOnce()) {
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
}
