//! Razdor's «Туман вместо черноты» (`Settings::map_mist`, off by default): the war fog's unexplored
//! land is drifting mist instead of black. Style only: no rule, fog, path or save reads it.
//!
//! The fog texture (`minimap::fog_layer`: one texel per cell, its alpha the shown darkness) is drawn
//! through a material, on the same quad and texels as the black fog. Where the darkness is 1 the
//! mist is opaque, where it is 0 nothing shows, and only at the opening's edge the alpha wisps
//! ([`mist_alpha`]). Stacked value-noise sheets drift at different speeds and heights; the 2.5D view
//! shifts the upper ones by its tilt ([`parallax`]). The mist's colour is the lighting's (the
//! daylight tint is drawn over it after the fog). The minimap keeps its black.

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
/// shifts the upper sheets further. The flat view has none (the caller passes zero there).
pub fn parallax(yaw_deg: f32, pitch_deg: f32) -> Vec2 {
    let (s, c) = yaw_deg.to_radians().sin_cos();
    vec2(s, c) * (1.0 - pitch_deg / 90.0) * 0.6
}

/// The sheets drawn: 3 on the desktop, 2 on a phone (the fill rate).
fn sheets() -> f32 {
    if crate::ui::display::dp_px().is_some() {
        2.0
    } else {
        3.0
    }
}

/// The clock the mist drifts on: the terrain's own (its water clock, in seconds), so the edge fog
/// and the mist move together.
fn clock() -> f32 {
    ((crate::ui::clock::now() * 1000.0) as i64 as f32 / 1000.0) % 1000.0
}

// The shader's vertex stage: the quad's uv, as the terrain's.
const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
varying highp vec2 uv;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1.0);
    uv = texcoord;
}
"#;

// The fragment: the fog texture's alpha is the darkness. Four sheets of value noise at
// increasing scale, each drifting and shifted by the tilt with its height; the first sheet's value
// is the wisp, the average the brightness. `Sheets` (1..4) says how many are drawn.
const FRAGMENT: &str = r#"#version 100
precision highp float;

varying highp vec2 uv;
uniform sampler2D Texture;
uniform vec2 FogSize;
uniform float Clock;
uniform float Sheets;
uniform vec2 Parallax;
uniform vec3 MistRgb;

float hash2(vec2 q) {
    return fract(sin(dot(q, vec2(127.1, 311.7))) * 43758.5453);
}

float vnoise(vec2 q) {
    vec2 i = floor(q);
    vec2 f = fract(q);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2(1.0, 0.0)), u.x), mix(hash2(i + vec2(0.0, 1.0)), hash2(i + vec2(1.0, 1.0)), u.x), u.y);
}

float sheet_scale(float i) { return i < 0.5 ? 0.18 : (i < 1.5 ? 0.33 : (i < 2.5 ? 0.6 : 1.1)); }
float sheet_height(float i) { return i < 0.5 ? 0.3 : (i < 1.5 ? 0.6 : (i < 2.5 ? 0.9 : 1.2)); }
float sheet_weight(float i) { return i < 0.5 ? 1.0 : (i < 1.5 ? 0.8 : (i < 2.5 ? 0.6 : 0.4)); }
vec2 sheet_drift(float i) {
    if (i < 0.5) return vec2(0.05, 0.02);
    if (i < 1.5) return vec2(-0.03, 0.06);
    if (i < 2.5) return vec2(0.07, -0.04);
    return vec2(-0.09, -0.05);
}

void main() {
    float d = texture2D(Texture, uv).a;
    // The cell coordinates: past the map's own texels (the extended draw) they run on.
    vec2 p = uv * FogSize;
    float acc = 0.0;
    float wsum = 0.0;
    float wisp = 0.5;
    for (int i = 0; i < 4; i++) {
        float fi = float(i);
        if (fi >= Sheets) break;
        vec2 q = (p + Parallax * sheet_height(fi)) * sheet_scale(fi) + Clock * sheet_drift(fi);
        float n = vnoise(q);
        if (i == 0) wisp = n;
        acc += sheet_weight(fi) * n;
        wsum += sheet_weight(fi);
    }
    float b = acc / max(wsum, 0.001);
    vec3 base = MistRgb * (0.80 + 0.30 * b);
    float a = d >= 0.995 ? 1.0 : clamp(d + 0.5 * (wisp - 0.5) * d * (1.0 - d) * 4.0, 0.0, 1.0);
    gl_FragColor = vec4(base, a);
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

/// Draws the fog texture `tex` (one texel per cell) into `dest` as mist, showing its texels `src`
/// (`None`: all of it, as `minimap::draw_fog`). `parallax` is [`parallax`] (zero in the flat view).
/// Without the shader it draws the plain texture (black).
pub fn draw(tex: &Texture2D, dest: Rect, src: Option<Rect>, parallax: Vec2) {
    let params = DrawTextureParams { dest_size: Some(dest.size()), source: src, ..Default::default() };
    MATERIAL.with(|cell| {
        let mut cell = cell.borrow_mut();
        match cell.get_or_insert_with(load).as_ref() {
            Some(m) => {
                gl_use_material(m);
                m.set_uniform("FogSize", vec2(tex.width(), tex.height()));
                m.set_uniform("Clock", clock());
                m.set_uniform("Sheets", sheets());
                m.set_uniform("Parallax", parallax);
                m.set_uniform("MistRgb", vec3(MIST_RGB[0], MIST_RGB[1], MIST_RGB[2]));
                draw_texture_ex(tex, dest.x, dest.y, WHITE, params);
                gl_use_default_material();
            }
            None => draw_texture_ex(tex, dest.x, dest.y, WHITE, params),
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
    fn the_flat_view_has_no_parallax_and_the_tilt_shifts_the_sheets_away_from_the_camera() {
        // Yaw 0 looks north: the shift runs along +y, and it grows as the view flattens.
        let steep = parallax(0.0, 75.0);
        let flat = parallax(0.0, 35.0);
        assert!(steep.x.abs() < 1e-6 && steep.y > 0.0);
        assert!(flat.y > steep.y, "{flat:?} vs {steep:?}");
        let turned = parallax(90.0, 55.0);
        assert!(turned.x > 0.0 && turned.y.abs() < 1e-6, "{turned:?}");
    }
}
