//! Razdor's cloud shadows (a prototype, `RAZDOR_CLOUD_SHADOWS=1`): shade drifts slowly over the explored map,
//! darkening the ground and what stands on it. Two shapes: round blotches (`RAZDOR_CLOUD_BLOCK=0`), or blocks
//! (Minecraft-like): the cloud density is sampled at each block's centre, thresholded per block, and the four
//! blocks round a pixel are blended, so the blocks join into soft-edged squares. One multiply pass over the
//! map's drawn rectangle, after the world and before the fog and the light: a cell's colour is multiplied by
//! `1 − cover × dark × tint`, where `cover` is the shade's density (0..1) and the fog's shown darkness masks it
//! (no shade on the war fog). The shade is neutral black by default (`tint` 0: all channels darken alike). Its
//! strength follows the daylight (`Pass::daylight`, `daylight::shade_at`): full by day, none at night, eased over
//! dusk and dawn. Style only: no rule, fog or save reads it.
//!
//! The pattern reuses the mist's baked noise texture (`mist::noise_texture`: one periodic field in red and one in
//! green, 64 lattice cells, mipmapped). Its two octaves are a fetch at the cloud size (red) and a fetch at twice the
//! frequency (green); the density is their weighted sum, cut at a threshold that covers the setting's share of the
//! texture (calibrated once from its own histogram).
//!
//! The drift is integrated on the CPU each frame (`advance`): `offset += dir × speed × dt`. The speed is one speed
//! while the game clock stands still (30 cells a minute), and about three times that while game time runs (the
//! clock moved within the last 0.45 s: a hero's walk or a wait). The speed eases toward its target (0.12 s), so a
//! change never jumps. A frame where the clock moves more than 12 game hours (a loaded save) counts for nothing.
//! `dt` is the map's animation time
//! (`map_anim::now`), so the drift is frozen with «Анимация карты» off and deterministic in snapshots.
//! Phones: GLES 2 (`#version 100`), no `sin` hash; texture fetches only (the block mode takes 9 a pixel, the round
//! mode 3).

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation, PipelineParams};
use macroquad::prelude::*;
use razdor::local;

use super::mist;
use super::weather::Projection;

/// The switch (`RAZDOR_CLOUD_SHADOWS=1`), pushed every frame by `display::follow_settings`.
static ON: AtomicBool = AtomicBool::new(false);

/// Takes the prototype's switch in (every frame).
pub fn set_pref(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Whether the shadows are drawn.
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// The tuning (`RAZDOR_CLOUD_*`, read once; the defaults are the prototype's current look).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// The darkening at the middle of a shadow, as a share of the colour (`RAZDOR_CLOUD_DARK`).
    pub dark: f32,
    /// The shade's cool tint, 0 neutral black … 1 cool blue (`RAZDOR_CLOUD_TINT`).
    pub tint: f32,
    /// The cloud's size in cells: one noise lattice cell (`RAZDOR_CLOUD_SCALE`).
    pub scale: f32,
    /// The share of the map under shadow (`RAZDOR_CLOUD_COVER`).
    pub cover: f32,
    /// The soft edge (`RAZDOR_CLOUD_SOFT`): the shade ramps over a fifth of it either side of the threshold.
    pub soft: f32,
    /// The block size in cells (`RAZDOR_CLOUD_BLOCK`): 0 for the round blotches, else the blocks' side.
    pub block: f32,
    /// The drift in cells a minute while the game clock stands still (`RAZDOR_CLOUD_SPEED`; 30 by default).
    pub speed: f32,
    /// The drift's multiple while game time runs (`RAZDOR_CLOUD_RUN_FACTOR`; 3 by default).
    pub run_factor: f32,
    /// How fast the speed follows its target, the exponential's time constant in seconds (`RAZDOR_CLOUD_EASE`;
    /// 0.12 by default).
    pub ease: f32,
    /// How long game time counts as running after its clock last moved, in real seconds (`RAZDOR_CLOUD_RUN_WINDOW`;
    /// 0.45 by default). The slowest walk (`walk_speed` 0, `walk_pace` 0.375) steps every 0.4 s: 0.4 + 0.05.
    pub run_window: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            dark: 0.25,
            tint: 0.0,
            scale: 5.0,
            cover: 0.28,
            soft: 0.15,
            block: 1.0,
            speed: 30.0,
            run_factor: 3.0,
            ease: 0.12,
            run_window: 0.45,
        }
    }
}

/// The tuning read through `get` (an environment variable's value, if set): each value is clamped to its range,
/// and a value that does not parse keeps the default.
pub fn tuning_from(get: impl Fn(&str) -> Option<String>) -> Tuning {
    let d = Tuning::default();
    let num = |key: &str, default: f32, lo: f32, hi: f32| {
        get(key).and_then(|s| s.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).map_or(default, |v| v.clamp(lo, hi))
    };
    Tuning {
        dark: num("RAZDOR_CLOUD_DARK", d.dark, 0.0, 0.5),
        tint: num("RAZDOR_CLOUD_TINT", d.tint, 0.0, 1.0),
        scale: num("RAZDOR_CLOUD_SCALE", d.scale, 2.0, 64.0),
        cover: num("RAZDOR_CLOUD_COVER", d.cover, 0.05, 0.95),
        soft: num("RAZDOR_CLOUD_SOFT", d.soft, 0.02, 0.5),
        block: num("RAZDOR_CLOUD_BLOCK", d.block, 0.0, 4.0),
        speed: num("RAZDOR_CLOUD_SPEED", d.speed, 0.0, 600.0),
        run_factor: num("RAZDOR_CLOUD_RUN_FACTOR", d.run_factor, 0.0, 10.0),
        ease: num("RAZDOR_CLOUD_EASE", d.ease, 0.01, 5.0),
        run_window: num("RAZDOR_CLOUD_RUN_WINDOW", d.run_window, 0.05, 5.0),
    }
}

/// The direction the shadows drift in (unit length, right and down): fixed, as the wind's sway (`living::wind`)
/// only swings left and right.
const DIR: [f64; 2] = [0.8, 0.6];

/// The longest step the drift integrates in one frame (seconds): a long gap (a paused frame) moves no more.
const MAX_STEP: f64 = 0.1;

/// A frame where the clock moves more than this many game hours is a jump (a loaded save, a clock set): it counts
/// for nothing, neither as running nor as drift.
const JUMP_HOURS: f64 = 12.0;

/// The drift's state: the last clocks it ran at (real seconds and game hours), the real time the game clock last
/// moved (None: not yet), its eased speed (cells a minute) and its offset (cells).
#[derive(Clone, Copy, Debug)]
struct Motion {
    last: Option<f64>,
    hours: Option<f64>,
    moved_at: Option<f64>,
    speed: f64,
    offset: [f64; 2],
}

impl Motion {
    fn fresh() -> Self {
        Motion { last: None, hours: None, moved_at: None, speed: 0.0, offset: [0.0; 2] }
    }
}

local! {
    static MOTION: RefCell<Motion> = RefCell::new(Motion::fresh());
}

/// The drift's target speed (cells a minute): the still speed, times the run factor while game time runs.
pub fn target_speed(running: bool, tu: &Tuning) -> f64 {
    let base = f64::from(tu.speed).max(0.0);
    if running { base * f64::from(tu.run_factor).max(0.0) } else { base }
}

/// Moves the drift on to real clock `now` (seconds) and game clock `hours` (total game hours, fractional). Game
/// time runs when its clock moved within `run_window`; the speed eases toward the target and the offset integrates
/// it. A frame whose clock moves more than `JUMP_HOURS` (or goes back) drifts nothing. The first call starts at
/// the still speed.
pub fn advance(now: f64, hours: f64, tu: &Tuning) {
    MOTION.with(|m| {
        let mut m = m.borrow_mut();
        let (Some(last), Some(last_hours)) = (m.last, m.hours) else {
            m.last = Some(now);
            m.hours = Some(hours);
            m.speed = target_speed(false, tu);
            return;
        };
        let real = now - last;
        let passed = hours - last_hours;
        m.last = Some(now);
        m.hours = Some(hours);
        let jump = real.is_nan() || real < 0.0 || !passed.is_finite() || !(0.0..=JUMP_HOURS).contains(&passed);
        let dt = if jump {
            0.0
        } else {
            if passed > 0.0 {
                m.moved_at = Some(now);
            }
            real.min(MAX_STEP)
        };
        let running = m.moved_at.is_some_and(|t| now - t <= f64::from(tu.run_window));
        let target = target_speed(running, tu);
        m.speed += (target - m.speed) * (1.0 - (-dt / f64::from(tu.ease).max(1e-3)).exp());
        // cells a minute → cells a second
        let step = m.speed / 60.0 * dt;
        m.offset[0] += DIR[0] * step;
        m.offset[1] += DIR[1] * step;
    });
}

/// The pattern's offset in noise lattice units: the drift's offset in cells over the cloud size, taken modulo the
/// texture's period (64 lattice cells) in f64. The shade moves with `+DIR` when the offset grows.
pub fn lattice_drift(offset: [f64; 2], scale: f32) -> [f32; 2] {
    let period = mist::NOISE_LATTICE as f64;
    let x = (-offset[0] / f64::from(scale)).rem_euclid(period);
    let y = (-offset[1] / f64::from(scale)).rem_euclid(period);
    [x as f32, y as f32]
}

/// The drift's current offset in lattice units (see [`lattice_drift`]).
fn drift_now(tu: &Tuning) -> [f32; 2] {
    lattice_drift(MOTION.with(|m| m.borrow().offset), tu.scale)
}

/// The density's threshold that leaves out `1 − cover` of the samples (the share at or above it is `cover`).
pub fn threshold_for_cover(mut density: Vec<f32>, cover: f32) -> f32 {
    if density.is_empty() {
        return 0.5;
    }
    density.sort_unstable_by(|a, b| a.total_cmp(b));
    let keep = ((1.0 - cover.clamp(0.0, 1.0)) * density.len() as f32) as usize;
    density[keep.min(density.len() - 1)]
}

/// The density of the noise texture's texels (`rgba` bytes, four a texel): the mix the shader makes of its two
/// fetches, `0.65 × red + 0.35 × green`, sampled every fourth texel.
fn density_of(rgba: &[u8]) -> Vec<f32> {
    rgba.chunks_exact(4).step_by(4).map(|px| 0.65 * f32::from(px[0]) / 255.0 + 0.35 * f32::from(px[1]) / 255.0).collect()
}

/// The threshold that covers the setting's share of the texture, computed once.
struct Calibrated {
    threshold: f32,
}

/// The tuning from the environment, read once.
fn tuning() -> Tuning {
    static TUNING: OnceLock<Tuning> = OnceLock::new();
    *TUNING.get_or_init(|| tuning_from(|k| std::env::var(k).ok()))
}

fn calibrated() -> &'static Calibrated {
    static CALIBRATED: OnceLock<Calibrated> = OnceLock::new();
    CALIBRATED.get_or_init(|| Calibrated { threshold: threshold_for_cover(density_of(&mist::bake_noise()), tuning().cover) })
}

// The vertex stage: the quad's pixel position is the fragment's (the window's pixels).
const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
varying highp vec2 pix;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1.0);
    pix = position.xy;
}
"#;

// The fragment: the multiply factor per pixel (1 where no shade). The ground under the pixel comes from the
// projection (flat or the tilted plane); the pixel's cell is p (cells, centred on whole numbers). The density is
// the two octaves of the mist's noise: red at the cloud size, green at twice the frequency. The drift is in lattice
// units: round mode (Block 0) adds it to the density's position, one density per pixel. Block mode moves the
// pixel into cloud space first (p + Drift × Scale), so the block grid drifts with the clouds. The density at each block's centre,
// thresholded per block, the four blocks round the pixel blended bilinearly. Tint 0 (the default) is neutral
// black; Tint 1 is the old cool shade (red takes the most, blue the least).
const FRAGMENT: &str = r#"#version 100
precision highp float;

varying highp vec2 pix;
uniform sampler2D Noise;
uniform sampler2D Fog;
uniform vec2 MapSize;
uniform vec3 HomRow0;
uniform vec3 HomRow1;
uniform vec3 HomRow2;
uniform float RowHeight;
uniform vec2 Drift;
uniform float Scale;
uniform float Dark;
uniform float Thresh;
uniform float Soft;
uniform float Tint;
uniform float Block;

const float PERIOD = 64.0;
const vec3 COOL = vec3(1.08, 1.0, 0.9);

vec2 ground(vec2 px, out bool ok) {
    vec3 w = vec3(px, 1.0);
    vec3 r = vec3(dot(HomRow0, w), dot(HomRow1, w), dot(HomRow2, w));
    ok = abs(r.z) > 0.000000001;
    return ok ? r.xy / r.z : vec2(0.0);
}

// The cloud's density at the lattice point q: 0..1.
float density(vec2 q) {
    float a = texture2D(Noise, q / PERIOD).r;
    float b = texture2D(Noise, (2.0 * q + vec2(0.37, 0.61)) / PERIOD).g;
    return 0.65 * a + 0.35 * b;
}

// One block's shade at its index i: its centre is (i + ½) blocks in cloud space (the drift is already in the
// position, see main); 1 where the density clears the threshold.
float block_shade(vec2 i, float hw) {
    vec2 c = (i + 0.5) * Block;
    return smoothstep(Thresh - hw, Thresh + hw, density(c / Scale));
}

void main() {
    bool ok;
    vec2 p = ground(pix, ok);
    p.y /= RowHeight;
    if (!ok || p.x < -0.5 || p.y < -0.5 || p.x >= MapSize.x - 0.5 || p.y >= MapSize.y - 0.5) {
        gl_FragColor = vec4(1.0);
        return;
    }
    vec2 uv = (p + 0.5) / MapSize;
    float vis = 1.0 - texture2D(Fog, uv).a;
    if (vis < 0.01) {
        gl_FragColor = vec4(1.0);
        return;
    }
    // The edge's half width: a fifth of the setting (about a tenth of the field's spread at the default).
    float hw = 0.2 * Soft;
    float cover;
    if (Block > 0.0) {
        // The block grid moves with the clouds: the drift is added to the position first (cloud space), so the
        // blocks slide with the shade instead of blinking on and off in place.
        vec2 q = p + Drift * Scale;
        vec2 fc = q / Block - 0.5;
        vec2 i0 = floor(fc);
        vec2 f = fc - i0;
        float v00 = block_shade(i0, hw);
        float v10 = block_shade(i0 + vec2(1.0, 0.0), hw);
        float v01 = block_shade(i0 + vec2(0.0, 1.0), hw);
        float v11 = block_shade(i0 + vec2(1.0, 1.0), hw);
        cover = mix(mix(v00, v10, f.x), mix(v01, v11, f.x), f.y);
    } else {
        cover = smoothstep(Thresh - hw, Thresh + hw, density(p / Scale + Drift));
    }
    cover *= vis;
    gl_FragColor = vec4(vec3(1.0) - cover * Dark * mix(vec3(1.0), COOL, Tint), 1.0);
}
"#;

/// The multiply blend: the destination times the fragment's colour (the shade), alpha untouched.
fn multiply() -> PipelineParams {
    PipelineParams {
        color_blend: Some(BlendState::new(Equation::Add, BlendFactor::Value(BlendValue::DestinationColor), BlendFactor::Zero)),
        ..Default::default()
    }
}

fn load() -> Option<Material> {
    let uniforms = [
        ("MapSize", UniformType::Float2),
        ("HomRow0", UniformType::Float3),
        ("HomRow1", UniformType::Float3),
        ("HomRow2", UniformType::Float3),
        ("RowHeight", UniformType::Float1),
        ("Drift", UniformType::Float2),
        ("Scale", UniformType::Float1),
        ("Dark", UniformType::Float1),
        ("Thresh", UniformType::Float1),
        ("Soft", UniformType::Float1),
        ("Tint", UniformType::Float1),
        ("Block", UniformType::Float1),
    ];
    let params = MaterialParams {
        uniforms: uniforms.iter().map(|(n, t)| UniformDesc::new(n, *t)).collect(),
        textures: vec!["Noise".to_string(), "Fog".to_string()],
        pipeline_params: multiply(),
    };
    match load_material(ShaderSource::Glsl { vertex: VERTEX, fragment: FRAGMENT }, params) {
        Ok(m) => Some(m),
        Err(e) => {
            razdor::diag::log(&format!("cloud shadows: the shader does not compile: {e}"));
            None
        }
    }
}

local! {
    /// The shader's material, loaded at the first draw (None when it does not compile).
    static MATERIAL: RefCell<Option<Option<Material>>> = RefCell::new(None);
    /// A white texel: the quad's texture.
    static ONE: RefCell<Option<Texture2D>> = RefCell::new(None);
    /// A clear texel: the fog when there is none (no shade is masked out).
    static NO_FOG: RefCell<Option<Texture2D>> = RefCell::new(None);
}

/// A one-texel texture, made once on the main thread.
fn texel(cell: &'static local::Local<RefCell<Option<Texture2D>>>, rgba: [u8; 4]) -> Texture2D {
    cell.with(|c| c.borrow_mut().get_or_insert_with(|| Texture2D::from_rgba8(1, 1, &rgba)).clone())
}

/// What the shadow pass draws this frame.
pub struct Pass<'a> {
    /// The fog's shown darkness (alpha), one texel per cell, the map's size; None when there is no fog.
    pub fog: Option<&'a Texture2D>,
    /// The map's size in cells.
    pub map: [f32; 2],
    /// The rectangle drawn over (window pixels).
    pub draw: [f32; 4],
    /// The drawing's projection: the flat view's pixel-to-cell map or the tilted plane's (`weather::Projection`).
    pub projection: Projection,
    /// The grid's row height in world units (the world's y is rows of it).
    pub row_height: f32,
    /// The map's animation time in seconds (`map_anim::now`): the drift integrates its steps.
    pub now: f64,
    /// The game clock in hours (total, fractional: `Clock::total_minutes / 60`): the drift's speed follows how fast
    /// it passes (`advance`).
    pub hours: f64,
    /// The share of the daylight the shade keeps this frame (`daylight::shade_at`, 0..1): 1 by day, 0 at night.
    pub daylight: f32,
}

/// Draws the cloud shadows of `pass` over its rectangle (window pixels), multiplied onto what is drawn. Without the
/// shader, or when the switch is off, it draws nothing.
pub fn draw(pass: &Pass) {
    if !on() {
        return;
    }
    let cal = calibrated();
    let tu = tuning();
    advance(pass.now, pass.hours, &tu);
    // At night the shade is gone: nothing to multiply (the drift still moves on).
    if pass.daylight <= 0.0 {
        return;
    }
    let d = drift_now(&tu);
    let fog = pass.fog.cloned().unwrap_or_else(|| texel(&NO_FOG, [0, 0, 0, 0]));
    let noise = mist::noise_texture();
    MATERIAL.with(|cell| {
        let mut cell = cell.borrow_mut();
        let Some(m) = cell.get_or_insert_with(load).as_ref() else { return };
        gl_use_material(m);
        m.set_texture("Noise", noise);
        m.set_texture("Fog", fog);
        m.set_uniform("MapSize", vec2(pass.map[0], pass.map[1]));
        let rows = pass.projection.matrix();
        for (name, row) in [("HomRow0", rows[0]), ("HomRow1", rows[1]), ("HomRow2", rows[2])] {
            m.set_uniform(name, vec3(row[0], row[1], row[2]));
        }
        m.set_uniform("RowHeight", pass.row_height);
        m.set_uniform("Drift", vec2(d[0], d[1]));
        m.set_uniform("Scale", tu.scale);
        // The daylight scales the shade's strength: 0 at night (no multiply at all), full by day.
        m.set_uniform("Dark", tu.dark * pass.daylight.clamp(0.0, 1.0));
        m.set_uniform("Thresh", cal.threshold);
        m.set_uniform("Soft", tu.soft);
        m.set_uniform("Tint", tu.tint);
        m.set_uniform("Block", tu.block);
        let one = texel(&ONE, [255, 255, 255, 255]);
        let r = pass.draw;
        draw_texture_ex(&one, r[0], r[1], WHITE, DrawTextureParams { dest_size: Some(vec2(r[2], r[3])), ..Default::default() });
        gl_use_default_material();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shader's density at the lattice point `q`, from the baked texture's nearest texels (the CPU mirror).
    fn density_at(rgba: &[u8], q: [f32; 2]) -> f32 {
        let n = mist::NOISE_TEXELS as i64;
        let fetch = |u: f32, v: f32, ch: usize| -> f32 {
            let x = ((u / mist::NOISE_LATTICE as f32) * n as f32).floor() as i64;
            let y = ((v / mist::NOISE_LATTICE as f32) * n as f32).floor() as i64;
            let t = (y.rem_euclid(n) * n + x.rem_euclid(n)) as usize * 4 + ch;
            f32::from(rgba[t]) / 255.0
        };
        0.65 * fetch(q[0], q[1], 0) + 0.35 * fetch(2.0 * q[0] + 0.37, 2.0 * q[1] + 0.61, 1)
    }

    #[test]
    fn the_defaults_are_the_current_look_and_the_env_clamps() {
        let d = tuning_from(|_| None);
        assert_eq!(d, Tuning::default());
        assert_eq!((d.dark, d.scale, d.soft, d.tint, d.block), (0.25, 5.0, 0.15, 0.0, 1.0));
        assert_eq!((d.speed, d.run_factor, d.ease, d.run_window), (30.0, 3.0, 0.12, 0.45));
        let t = tuning_from(|k| match k {
            "RAZDOR_CLOUD_DARK" => Some("9".into()),
            "RAZDOR_CLOUD_COVER" => Some("nope".into()),
            "RAZDOR_CLOUD_SCALE" => Some(" 12 ".into()),
            "RAZDOR_CLOUD_BLOCK" => Some("0".into()),
            _ => None,
        });
        assert_eq!(t.dark, 0.5, "clamped to the top of its range");
        assert_eq!(t.cover, Tuning::default().cover, "a value that does not parse keeps the default");
        assert_eq!((t.scale, t.block), (12.0, 0.0));
        assert!(!on(), "off until the switch is set");
    }

    #[test]
    fn the_lattice_drift_loops_with_the_texture_period() {
        // Shifting the offset by a whole texture period (64 lattice cells) is the same drift, on both axes.
        let scale = 5.0f32;
        for off in [[0.0, 0.0], [13.7, 2.2], [1000.3, -77.5]] {
            let a = lattice_drift(off, scale);
            let b = lattice_drift([off[0] + 64.0 * f64::from(scale), off[1] + 64.0 * f64::from(scale)], scale);
            for k in 0..2 {
                let diff = (a[k] - b[k]).rem_euclid(64.0);
                assert!(diff.min(64.0 - diff) < 1e-2, "offset {off:?}, axis {k}: {} vs {}", a[k], b[k]);
            }
        }
    }

    #[test]
    fn the_shade_follows_the_daylight_none_at_night_full_by_day_and_continuous() {
        use crate::ui::daylight::shade_at;
        assert_eq!(shade_at(0.0), 0.0, "midnight: no shade");
        assert_eq!(shade_at(12.0), 1.0, "noon: full shade");
        assert!(shade_at(18.0) > 0.0 && shade_at(18.0) < 1.0, "dusk eases");
        assert!(shade_at(5.0) > 0.0 && shade_at(5.0) < 1.0, "dawn eases in");
        // Minute steps over a whole day (and back to midnight): the largest step is well under 0.012
        // (the factor's slope is at most 0.66 an hour).
        let mut prev = shade_at(0.0);
        for k in 1..=(24 * 60) {
            let now = shade_at(k as f32 / 60.0);
            assert!((now - prev).abs() < 0.012, "a jump of {} at minute {k}", now - prev);
            prev = now;
        }
        assert_eq!(prev, 0.0, "back at midnight");
    }

    #[test]
    fn a_frozen_clock_moves_nothing() {
        let tu = Tuning::default();
        local_reset();
        advance(5.0, 100.0, &tu);
        let before = MOTION.with(|m| m.borrow().offset);
        // Game time runs on, but the map animation is frozen (`now` stands still): no drift.
        for k in 0..100 {
            advance(5.0, 100.0 + f64::from(k) * 0.01, &tu);
        }
        assert_eq!(MOTION.with(|m| m.borrow().offset), before, "no real time, no drift");
    }

    /// Clears the drift's state for a test (the tests share the thread's state).
    fn local_reset() {
        MOTION.with(|m| *m.borrow_mut() = Motion::fresh());
    }

    /// Runs `frames` frames at 60 a second from real time `t0`. The game clock moves `step` hours every `every`
    /// frames (0 = never) from `hours0`.
    fn run(t0: f64, frames: u32, hours0: f64, step: f64, every: u32, tu: &Tuning) -> f64 {
        let mut hours = hours0;
        for k in 1..=frames {
            if every > 0 && k % every == 0 {
                hours += step;
            }
            advance(t0 + f64::from(k) / 60.0, hours, tu);
        }
        hours
    }

    fn speed() -> f64 {
        MOTION.with(|m| m.borrow().speed)
    }

    fn offset() -> [f64; 2] {
        MOTION.with(|m| m.borrow().offset)
    }

    #[test]
    fn a_still_clock_drifts_at_the_still_speed() {
        let tu = Tuning::default();
        local_reset();
        run(0.0, 600, 100.0, 0.0, 0, &tu);
        assert!((speed() - 30.0).abs() < 1e-6, "still speed: {}", speed());
        let o = offset();
        let moved = (o[0] * o[0] + o[1] * o[1]).sqrt();
        assert!((moved - 30.0 / 60.0 * 10.0).abs() < 0.05, "still: {moved} cells in 10 s");
    }

    #[test]
    fn a_stepwise_clock_counts_as_running() {
        let tu = Tuning::default();
        local_reset();
        // The hero's walk: 25 game minutes (25/60 h) in one frame every 9 frames (0.15 s), so the clock never
        // stands still for the 0.45 s window. The target is 3 × 30 = 90 cells a minute.
        run(0.0, 600, 100.0, 25.0 / 60.0, 9, &tu);
        assert!((speed() - 90.0).abs() < 0.05, "running speed after 10 s: {}", speed());
    }

    #[test]
    fn the_speed_returns_to_still_after_the_clock_stops() {
        let tu = Tuning::default();
        local_reset();
        let hours = run(0.0, 180, 100.0, 25.0 / 60.0, 9, &tu);
        assert!(speed() > 80.0, "running before the stop: {}", speed());
        // The clock stands still: 0.45 s of running (the window), then the speed eases down (0.12 s constant).
        let t0 = 3.0;
        let mut running_at_third_second = 0.0;
        let mut eased_at_one_second = 0.0;
        for k in 1..=600 {
            let t = t0 + f64::from(k) / 60.0;
            advance(t, hours, &tu);
            if k == 20 {
                running_at_third_second = speed();
            }
            if k == 60 {
                eased_at_one_second = speed();
            }
        }
        assert!(running_at_third_second > 80.0, "still running 0.33 s after the stop: {running_at_third_second}");
        assert!(eased_at_one_second < 35.0, "eased down 1 s after the stop: {eased_at_one_second}");
        assert!((speed() - 30.0).abs() < 0.05, "stopped: the speed eases to {}", speed());
    }

    #[test]
    fn a_jump_drifts_nothing_and_does_not_count_as_running() {
        let tu = Tuning::default();
        local_reset();
        // Standing still, then a loaded save leaps 13 hours in one frame.
        run(0.0, 60, 100.0, 0.0, 0, &tu);
        let (before, speed_before) = (offset(), speed());
        advance(61.0 / 60.0, 113.0, &tu);
        assert_eq!(offset(), before, "no drift in the jump frame");
        assert_eq!(speed(), speed_before, "no speed change in the jump frame");
        // The next frames stand still: the jump did not start a running spell.
        run(62.0 / 60.0, 120, 113.0, 0.0, 0, &tu);
        assert!((speed() - 30.0).abs() < 1e-6, "still after a jump: {}", speed());
        // A clock set back is a jump too.
        let before = offset();
        advance(10.0, 50.0, &tu);
        assert_eq!(offset(), before, "no drift when the clock goes back");
        assert!((speed() - 30.0).abs() < 1e-6, "a clock set back leaves the speed: {}", speed());
    }

    #[test]
    fn a_change_of_speed_moves_the_offset_without_a_jump() {
        let tu = Tuning::default();
        local_reset();
        let hours = run(0.0, 180, 100.0, 25.0 / 60.0, 9, &tu);
        let mut prev = offset();
        let mut max_step = 0.0f64;
        for k in 1..=600 {
            advance(3.0 + f64::from(k) / 60.0, hours, &tu);
            let now = offset();
            max_step = max_step.max(((now[0] - prev[0]).powi(2) + (now[1] - prev[1]).powi(2)).sqrt());
            prev = now;
        }
        // No frame moves faster than the running speed (90 cells a minute).
        let frame = 90.0 / 60.0 * (1.0 / 60.0);
        assert!(max_step <= frame * 1.001, "a frame moved {max_step} cells, more than {frame}");
    }

    #[test]
    fn the_threshold_covers_the_asked_share_of_the_texture() {
        let bytes = mist::bake_noise();
        for cover in [0.2f32, 0.28, 0.4, 0.6] {
            let thr = threshold_for_cover(density_of(&bytes), cover);
            let mut hit = 0usize;
            let steps = 128usize;
            for i in 0..steps {
                for j in 0..steps {
                    let q = [(i as f32 + 0.5) * 64.0 / steps as f32, (j as f32 + 0.5) * 64.0 / steps as f32];
                    if density_at(&bytes, q) >= thr {
                        hit += 1;
                    }
                }
            }
            let share = hit as f32 / (steps * steps) as f32;
            assert!((share - cover).abs() < 0.03, "cover {cover}: the shader's share is {share}");
        }
    }
}
