//! Razdor's «Погода» (`Settings::map_weather`, off by default): the weather follows the land. Snow
//! over snow and ice, rain over swamps and lowlands, heat haze over deserts, ash over fire lands and
//! cloud shadows everywhere, each biome's weather drawn for its day. Style only: no rule, fog, path,
//! AI or save reads it, and nothing is kept (the weather is recomputed from the map and the clock).
//!
//! The pure part is tested here: the biomes of the terrain codes ([`weights`]), each day's spell per
//! biome from a hash of the map and the day ([`Schedule`]), the blend over each midnight, and the
//! easing of the shown levels. The pass ([`draw`]: one shader over the drawn map) is here too;
//! `world_view::draw_weather` feeds it the frame's masks, fog, projection and light.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

use macroquad::prelude::*;
use razdor::local;

/// The setting (`Settings::map_weather`), pushed every frame by `display::follow_settings`.
static ON: AtomicBool = AtomicBool::new(false);

/// Takes the setting «Погода» in (every frame).
pub fn set_pref(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Whether the weather is drawn (the setting, or `RAZDOR_WEATHER=1`).
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// The weather kinds. Each has its own spell per day; the land decides where each one falls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Biome {
    Snow,
    /// Swamps and deep swamps.
    SwampRain,
    /// Lowlands, plains and grass.
    LowlandRain,
    /// Deserts and dust: heat haze, and sand on a heavy day.
    Heat,
    /// Fire lands and badground: ash (and haze, see [`weights`]).
    Ash,
    /// Everywhere: cloud shadows.
    Clouds,
}

impl Biome {
    #[cfg(test)]
    pub const ALL: [Biome; 6] = [Biome::Snow, Biome::SwampRain, Biome::LowlandRain, Biome::Heat, Biome::Ash, Biome::Clouds];

    /// The chance that the biome has weather on a day (the owner's brief, 2026-11-10).
    pub fn chance(self) -> f32 {
        match self {
            Biome::Snow => 0.60,
            Biome::SwampRain => 0.45,
            Biome::LowlandRain => 0.20,
            Biome::Heat => 0.80,
            Biome::Ash => 0.70,
            Biome::Clouds => 0.70,
        }
    }
}

/// A day's weather in one biome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spell {
    None,
    Light,
    Heavy,
}

impl Spell {
    /// The level the spell draws at: 0, half, or all.
    pub fn level(self) -> f32 {
        match self {
            Spell::None => 0.0,
            Spell::Light => 0.5,
            Spell::Heavy => 1.0,
        }
    }
}

/// The share of a biome's weather days that are heavy.
const HEAVY_SHARE: f32 = 0.4;

/// splitmix64: a well-mixed 64-bit hash of `z` (the schedule's only randomness, a pure function).
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A hash as a number in [0, 1).
fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// The map's seed: FNV-1a over its size and its terrain codes, so each map has its own weather.
pub fn map_seed(w: u32, h: u32, codes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for b in w.to_le_bytes().into_iter().chain(h.to_le_bytes()).chain(codes.iter().copied()) {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// The spell of `biome` on game day `day` of the map `seed`: none, or light, or heavy. Pure and
/// deterministic; the chances are [`Biome::chance`] and [`HEAVY_SHARE`].
pub fn spell(seed: u64, day: i64, biome: Biome) -> Spell {
    let h = mix(seed ^ mix(day as u64) ^ mix(0x5EED_0000 + biome as u64));
    if unit(h) >= biome.chance() {
        return Spell::None;
    }
    if unit(mix(h ^ 0xBEEF)) < HEAVY_SHARE {
        Spell::Heavy
    } else {
        Spell::Light
    }
}

/// The schedule of one map: its seed, and the day that stands for every day (`RAZDOR_WEATHER_DAY`,
/// for the snapshots); None for the game's own days.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule {
    pub seed: u64,
    pub day: Option<i64>,
}

/// The game's time at which the spells change: each midnight. The blend runs over one game hour
/// centred on it, half of it in days.
const HALF_HOUR: f64 = 1.0 / 48.0;

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

impl Schedule {
    /// The spell's level of `biome` on game day `k`.
    fn day_level(&self, k: i64, biome: Biome) -> f32 {
        spell(self.seed, self.day.unwrap_or(k), biome).level()
    }

    /// The level of `biome` at game time `t` (days, continuous, from the game's start): the day's
    /// spell, and across each midnight a smooth blend from the day before's level to the next's over
    /// one game hour, so no level jumps. Within [0, 1].
    pub fn level(&self, t: f64, biome: Biome) -> f32 {
        let k = t.floor();
        let f = t - k;
        let k = k as i64;
        if f < HALF_HOUR {
            // The window centred on midnight `k`: its second half, from the day before's level.
            let x = ((f + HALF_HOUR) / (2.0 * HALF_HOUR)) as f32;
            let (a, b) = (self.day_level(k - 1, biome), self.day_level(k, biome));
            a + (b - a) * smoothstep(x)
        } else if f > 1.0 - HALF_HOUR {
            // The window centred on midnight `k + 1`: its first half, toward the next day's level.
            let x = ((f - 1.0 + HALF_HOUR) / (2.0 * HALF_HOUR)) as f32;
            let (a, b) = (self.day_level(k, biome), self.day_level(k + 1, biome));
            a + (b - a) * smoothstep(x)
        } else {
            self.day_level(k, biome)
        }
    }
}

/// The weights of a terrain code in the weather masks: `[snow, swamp rain, heat, ash]` (the
/// `weather` texture's red, green, blue, alpha) and the lowland rain (the `lowland` texture's red).
/// The codes are the `.DTm` terrain codes, the indices of `gfx::TERRAIN_TEXTURES`: 14 Snow and 15 Ice;
/// 8 Swamp and 9 DeepSwamp; 5 LowLand, 6 land and 7 plain; 10 Desert and 13 Dust (haze); 3 FlameLand
/// and 11 Badground (ash, with half the haze). Water, roads and rock have none.
pub fn weights(code: u8) -> ([f32; 4], f32) {
    match code {
        14 | 15 => ([1.0, 0.0, 0.0, 0.0], 0.0),
        8 | 9 => ([0.0, 1.0, 0.0, 0.0], 0.0),
        5..=7 => ([0.0, 0.0, 0.0, 0.0], 1.0),
        10 | 13 => ([0.0, 0.0, 1.0, 0.0], 0.0),
        3 | 11 => ([0.0, 0.0, 0.5, 1.0], 0.0),
        _ => ([0.0; 4], 0.0),
    }
}

/// The two mask textures' bytes for a map's terrain codes, one texel per cell, row by row: the
/// `weather` texture (RGBA) and the `lowland` texture (red; green, blue 0; alpha 255).
pub fn mask_texels(codes: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut weather = Vec::with_capacity(codes.len() * 4);
    let mut lowland = Vec::with_capacity(codes.len() * 4);
    for &code in codes {
        let (w, l) = weights(code);
        weather.extend(w.map(byte));
        lowland.extend([byte(l), 0, 0, 255]);
    }
    (weather, lowland)
}

/// The levels of every biome shown this frame (0 none … 1 heavy), eased toward the schedule's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Levels {
    pub snow: f32,
    pub swamp: f32,
    pub lowland: f32,
    pub heat: f32,
    pub ash: f32,
    pub clouds: f32,
}

impl Levels {
    /// The schedule's levels at game time `t` (days), unshown and uneased.
    pub fn at(schedule: &Schedule, t: f64) -> Levels {
        Levels {
            snow: schedule.level(t, Biome::Snow),
            swamp: schedule.level(t, Biome::SwampRain),
            lowland: schedule.level(t, Biome::LowlandRain),
            heat: schedule.level(t, Biome::Heat),
            ash: schedule.level(t, Biome::Ash),
            clouds: schedule.level(t, Biome::Clouds),
        }
    }

    /// Whether any weather is drawn at these levels.
    pub fn any(&self) -> bool {
        [self.snow, self.swamp, self.lowland, self.heat, self.ash, self.clouds].iter().any(|&v| v > 0.001)
    }

    /// Each level moved toward `target` by the real time `dt` (seconds): an exponential approach
    /// with [`LEVEL_TAU`], snapped to the target within a thousandth. A wait's jumps of game time
    /// (30-minute ticks) ease in real time this way.
    pub fn eased_toward(self, target: Levels, dt: f32) -> Levels {
        let k = 1.0 - (-dt.max(0.0) / LEVEL_TAU).exp();
        let ease = |a: f32, b: f32| if (b - a).abs() < 0.001 { b } else { a + (b - a) * k };
        Levels {
            snow: ease(self.snow, target.snow),
            swamp: ease(self.swamp, target.swamp),
            lowland: ease(self.lowland, target.lowland),
            heat: ease(self.heat, target.heat),
            ash: ease(self.ash, target.ash),
            clouds: ease(self.clouds, target.clouds),
        }
    }
}

/// The time constant of the shown levels (seconds).
pub const LEVEL_TAU: f32 = 0.8;

thread_local! {
    /// The shown levels and the real time they were last moved to (per thread: the map's frames).
    static SHOWN: Cell<Option<(Levels, f64)>> = const { Cell::new(None) };
}

/// The levels to draw this frame: the schedule at game time `t` (days), eased from the shown
/// levels by the real time since the last frame (`now`, seconds). None when the weather is off or
/// no weather is due. Forgets the shown levels when the weather is off, so it starts from none.
pub fn frame_levels(schedule: &Schedule, t: f64, now: f64) -> Option<Levels> {
    if !on() {
        SHOWN.set(None);
        return None;
    }
    let target = Levels::at(schedule, t);
    let (shown, at) = SHOWN.get().unwrap_or((target, now));
    let shown = shown.eased_toward(target, (now - at).clamp(0.0, 1.0) as f32);
    SHOWN.set(Some((shown, now)));
    shown.any().then_some(shown)
}

/// Forgets the shown levels (a game loaded or started: the weather of the new map starts fresh).
pub fn forget() {
    SHOWN.set(None);
}

/// `RAZDOR_WEATHER_DAY=<n>` (read once): the day that stands for every day, for the snapshots.
pub fn env_day() -> Option<i64> {
    static ENV: std::sync::OnceLock<Option<i64>> = std::sync::OnceLock::new();
    *ENV.get_or_init(|| std::env::var("RAZDOR_WEATHER_DAY").ok().and_then(|v| v.trim().parse().ok()))
}

/// The schedule of a map (its seed), on the game's days or on the fixed day `RAZDOR_WEATHER_DAY`.
pub fn schedule_of(seed: u64) -> Schedule {
    Schedule { seed, day: env_day() }
}

/// The drawing's projection: how a window pixel lands on the ground (in cells, the terrain's world
/// units). Flat: `pixel / scale + offset`. Tilted: the ground plane's fit to the camera's picks
/// (`view25::ground_homography`).
#[derive(Clone, Copy, Debug)]
pub enum Projection {
    Flat { scale: f32, offset: [f32; 2] },
    Tilted { h: [[f32; 3]; 3] },
}

impl Projection {
    /// The pixel-to-ground matrix (row-major, homogeneous): the flat view's `pixel / scale + offset`,
    /// or the tilted camera's fitted plane.
    pub fn matrix(&self) -> [[f32; 3]; 3] {
        match *self {
            Projection::Flat { scale, offset } => [[1.0 / scale, 0.0, offset[0]], [0.0, 1.0 / scale, offset[1]], [0.0, 0.0, 1.0]],
            Projection::Tilted { h } => h,
        }
    }
}

/// What the weather pass draws this frame.
pub struct Pass<'a> {
    /// The biome weights (red snow, green swamp rain, blue heat, alpha ash), one texel per cell.
    pub biome: &'a Texture2D,
    /// The lowland rain (red), one texel per cell.
    pub lowland: &'a Texture2D,
    /// The fog's shown darkness (alpha), one texel per cell; None when the fog is off (no darkness).
    pub fog: Option<&'a Texture2D>,
    /// The map's size in cells.
    pub map: [f32; 2],
    /// The rectangle drawn over (window pixels).
    pub draw: [f32; 4],
    pub projection: Projection,
    pub levels: Levels,
    /// Seconds modulo [`PERIOD`]: every particle loops exactly with it.
    pub phase: f32,
    /// The terrain's clock (modulo its period): the clouds' drift.
    pub clock: f32,
    /// The daylight tint (white with the lighting off).
    pub tint: [f32; 3],
    /// Particle depths: 3 on the desktop, 2 on a phone.
    pub layers: f32,
    /// The map's zoom: the flakes and streaks grow with it (pixels per cell over 32).
    pub zoom: f32,
    /// The grid's row height in world units (`Grid::row_height`): the world's y is rows of it.
    pub row_height: f32,
}

/// The loop of every particle, in seconds: each speed times it is a whole number of cells, and the
/// wobble's frequency is a whole number of cycles in it.
pub const PERIOD: f32 = 32.0;

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

// The fragment: every weather layer in one pass, composed into one straight-alpha colour. Darkness
// (the clouds, the wet ground) and light (specks, streaks, haze) are kept apart and combined as
// (1 - a_total) = (1 - a_light)(1 - a_dark), so one blend gives both. The ground under the pixel
// comes from the projection (flat or the ray's hit with Y = 0). Cells span ±½ around their centres
// (the terrain's convention), so the mask lookup is (p + ½) / MapSize.
const FRAGMENT: &str = r#"#version 100
precision highp float;

varying highp vec2 pix;
uniform sampler2D Weather;
uniform sampler2D LowlandMask;
uniform sampler2D Fog;
uniform vec2 MapSize;
uniform vec3 HomRow0;
uniform vec3 HomRow1;
uniform vec3 HomRow2;
uniform float RowHeight;
uniform float Zoom;
uniform float Snow;
uniform float SwampLevel;
uniform float LowlandLevel;
uniform float Heat;
uniform float Ash;
uniform float Clouds;
uniform float Phase;
uniform float Clock;
uniform vec3 Tint;
uniform float Layers;

// The loop's angular speed: one whole cycle in the period (32 s), and the multiples used below.
const float W = 0.19634954;

float hash12(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

vec2 hash22(vec2 p) {
    return vec2(hash12(p), hash12(p + vec2(41.3, 17.7)));
}

// The ground under a pixel: the projection's homography (`Projection`: the flat view's scale and
// offset, or the ground plane's fit to the tilted camera), applied to the pixel's position.
vec2 ground(vec2 px, out bool ok) {
    vec3 w = vec3(px, 1.0);
    vec3 r = vec3(dot(HomRow0, w), dot(HomRow1, w), dot(HomRow2, w));
    ok = abs(r.z) > 0.000000001;
    return ok ? r.xy / r.z : vec2(0.0);
}

// The mask's bilinear sample at the cell coordinate p (the texel of cell i is centred at i + ½):
// four nearest taps, blended by the fraction, clamped to the map. The masks are nearest-filtered.
vec4 mask4(sampler2D t, vec2 p) {
    vec2 i0 = floor(p);
    vec2 f = p - i0;
    vec2 a = clamp(i0, vec2(0.0), MapSize - 1.0);
    vec2 b = clamp(i0 + 1.0, vec2(0.0), MapSize - 1.0);
    vec4 c00 = texture2D(t, (vec2(a.x, a.y) + 0.5) / MapSize);
    vec4 c10 = texture2D(t, (vec2(b.x, a.y) + 0.5) / MapSize);
    vec4 c01 = texture2D(t, (vec2(a.x, b.y) + 0.5) / MapSize);
    vec4 c11 = texture2D(t, (vec2(b.x, b.y) + 0.5) / MapSize);
    return mix(mix(c00, c10, f.x), mix(c01, c11, f.x), f.y);
}

// Soft round flakes (or specks) in square cells of `size` px. The field drifts by `vel` px/s (vel times
// the 32 s period is a whole number of cells, so the loop is seamless). Each cell holds one flake with
// probability `density`, at a hashed place, swaying by `wob` px. Radius `radius` px (scaled by Zoom).
float flakes(vec2 px, float size, vec2 vel, float radius, float density, float seed, float wob) {
    vec2 q = px - vel * Phase;
    vec2 c0 = floor(q / size);
    float acc = 0.0;
    for (int j = -1; j <= 1; j++) {
        for (int i = -1; i <= 1; i++) {
            vec2 c = c0 + vec2(float(i), float(j));
            float h = hash12(c + seed);
            if (h > density) {
                continue;
            }
            vec2 h2 = hash22(c + seed + 3.7);
            vec2 centre = (c + 0.15 + 0.7 * h2) * size;
            centre.x += sin(16.0 * W * Phase + h * 40.0) * wob;
            float r = radius * Zoom * (0.75 + 0.5 * fract(h * 7.31));
            acc += 1.0 - smoothstep(r * 0.55, r, length(q - centre));
        }
    }
    return min(acc, 1.0);
}

// Rain streaks, slanted about 15 degrees down to the left: u = x + 0.27 y is constant along a streak,
// and v = y − speed·t falls. Cells are `across` px wide (u) and `down` px tall (v); speed · 32 is a
// whole number of `down` cells. A streak is 'len' (base) + 'range' px long, 0.6 px wide.
float streaks(vec2 px, float speed, float seed, float across, float down, float len, float range, float density) {
    float u = px.x + 0.27 * px.y;
    float v = px.y - speed * Phase;
    vec2 c0 = floor(vec2(u / across, v / down));
    float acc = 0.0;
    for (int j = -1; j <= 1; j++) {
        for (int i = -1; i <= 1; i++) {
            vec2 c = c0 + vec2(float(i), float(j));
            float h = hash12(c + seed);
            if (h > density) {
                continue;
            }
            float cu = (c.x + 0.1 + 0.8 * fract(h * 5.13)) * across;
            float cv = (c.y + 0.1 + 0.8 * fract(h * 9.71)) * down;
            float l = (len + range * fract(h * 3.37)) * Zoom;
            float w = 0.6 * Zoom;
            float ac = 1.0 - smoothstep(w * 0.5, w + 0.5, abs(u - cu));
            float along = smoothstep(cv - l, cv - l + 4.0, v) * (1.0 - smoothstep(cv - 1.0, cv + 1.0, v));
            acc += ac * along;
        }
    }
    return min(acc, 1.0);
}

// Sand blown sideways in gusts: streaks along x (u = y, v = x + speed·t, the wind to the left). Cells
// are 14 px across (y) and 150 px along (x); speed · 32 is a whole number of 150 px cells.
float gusts(vec2 px, float speed, float seed) {
    float u = px.y;
    float v = px.x + speed * Phase;
    vec2 c0 = floor(vec2(u / 14.0, v / 150.0));
    float acc = 0.0;
    for (int j = -1; j <= 1; j++) {
        for (int i = -1; i <= 1; i++) {
            vec2 c = c0 + vec2(float(i), float(j));
            float h = hash12(c + seed);
            if (h > 0.2) {
                continue;
            }
            float cu = (c.x + 0.2 + 0.6 * fract(h * 5.13)) * 14.0;
            float cv = (c.y + 0.1 + 0.8 * fract(h * 9.71)) * 150.0;
            float l = (30.0 + 30.0 * fract(h * 3.37)) * Zoom;
            float ac = 1.0 - smoothstep(0.6, 1.5, abs(u - cu));
            float along = smoothstep(cv, cv + 6.0, v) * (1.0 - smoothstep(cv + l - 6.0, cv + l, v));
            acc += ac * along;
        }
    }
    return min(acc, 1.0);
}

// Value noise on a lattice that repeats every 64 cells (the terrain's clock loops on that lattice).
float vnoise(vec2 q) {
    vec2 i = floor(q);
    vec2 f = fract(q);
    vec2 u = f * f * (3.0 - 2.0 * f);
    vec2 a = mod(i, 64.0);
    vec2 b = mod(i + 1.0, 64.0);
    return mix(mix(hash12(a), hash12(vec2(b.x, a.y)), u.x), mix(hash12(vec2(a.x, b.y)), hash12(b), u.x), u.y);
}

float fbm2(vec2 q) {
    return (vnoise(q) * 0.5 + vnoise(q * 2.0 + vec2(1.7, 9.2)) * 0.25) / 0.75;
}

void main() {
    bool ok;
    vec2 p = ground(pix, ok);
    // The world's y is in rows of RowHeight (the square grid's 22 / 32): the cells are whole rows.
    p.y /= RowHeight;
    if (!ok || p.x < -0.5 || p.y < -0.5 || p.x >= MapSize.x - 0.5 || p.y >= MapSize.y - 0.5) {
        gl_FragColor = vec4(0.0);
        return;
    }
    vec2 uv = (p + 0.5) / MapSize;
    vec4 bw = mask4(Weather, p);
    float lowl = mask4(LowlandMask, p).r;
    // Only the explored ground: the fog's shown darkness (0 lit, 1 black) masks the weather. The fog
    // texture is filtered linearly, so its edge is a smooth ramp, not a cell step.
    float vis = 1.0 - texture2D(Fog, uv).a;
    if (vis < 0.01) {
        gl_FragColor = vec4(0.0);
        return;
    }
    // Everything is an overlay (premultiplied colour `ov`, alpha `oa`) except the darkening `dark`.
    float dark = 0.0;
    float oa = 0.0;
    vec3 ov = vec3(0.0);

    // Cloud shadows: soft blobs of 2 to 5 cells, drifting slowly over the ground in its own coordinates.
    // They are everywhere (no biome mask), a cool shade, feathered by a wide smoothstep.
    if (Clouds > 0.01) {
        vec2 q = p * 0.25 + Clock * vec2(0.02, 0.01);
        float cover = smoothstep(0.42, 0.60, fbm2(q)) * Clouds * vis;
        float a = cover * 0.34;
        ov += vec3(0.36, 0.46, 0.62) * a;
        oa += a;
    }

    // Snow: soft round flakes down and to the left, three depths (near is faster and bigger), swaying.
    if (Snow > 0.01 && bw.r > 0.01) {
        float s = flakes(pix, 48.0, vec2(-30.0, 45.0), 2.6, 0.50 * Snow, 0.0, 4.0);
        if (Layers > 1.5) {
            s += 0.9 * flakes(pix, 32.0, vec2(-20.0, 30.0), 1.7, 0.60 * Snow, 11.0, 3.0);
        }
        if (Layers > 2.5) {
            s += 0.7 * flakes(pix, 16.0, vec2(-10.0, 15.0), 1.1, 0.60 * Snow, 23.0, 1.5);
        }
        float a = clamp(s * bw.r * vis, 0.0, 1.0);
        ov += vec3(0.98, 0.99, 1.0) * a;
        oa += a;
        // A slight whitening of the snow under the flakes.
        float h = 0.07 * Snow * bw.r * vis;
        ov += vec3(0.95, 0.96, 1.0) * h;
        oa += h;
    }

    // Rain over swamps and lowlands: slanted grey streaks in two depths, the wet ground about 10 %
    // darker and a faint cool tint under a heavy spell.
    float rain = max(SwampLevel * bw.g, LowlandLevel * lowl);
    if (rain > 0.01) {
        float s = streaks(pix, 900.0, 0.0, 26.0, 96.0, 14.0, 12.0, 0.6);
        if (Layers > 2.5) {
            s = max(s, 0.8 * streaks(pix, 1200.0, 5.0, 20.0, 80.0, 10.0, 10.0, 0.5));
        }
        float a = clamp(s * rain * vis * 0.45, 0.0, 1.0);
        ov += vec3(0.80, 0.82, 0.85) * a;
        oa += a;
        dark += rain * 0.10 * vis;
        float c = 0.06 * rain * vis;
        ov += vec3(0.50, 0.60, 0.75) * c;
        oa += c;
    }

    // Heat haze over deserts and dust (and half over fire lands): a faint pale shimmer of bands that
    // waver, since reading the frame back to bend it would cost a readback each frame. A heavy spell
    // blows pale sand in gusts.
    if (Heat > 0.01 && bw.b > 0.01) {
        float wv = pix.y * 0.09 + 2.0 * sin(pix.x * 0.02 + 2.0 * W * Phase) + 8.0 * W * Phase;
        float band = 0.5 + 0.5 * sin(wv);
        float a = band * bw.b * Heat * 0.06 * vis;
        ov += vec3(1.0, 0.94, 0.8) * a;
        oa += a;
        float gust = clamp((Heat - 0.9) * 10.0, 0.0, 1.0);
        if (gust > 0.0) {
            float sand = gusts(pix, 600.0, 3.0);
            float b = clamp(sand * bw.b * vis * 0.5 * gust, 0.0, 1.0);
            ov += vec3(0.86, 0.74, 0.46) * b;
            oa += b;
        }
    }

    // Ash over fire lands: few soft grey specks, floating and drifting up a little.
    if (Ash > 0.01 && bw.a > 0.01) {
        float s = flakes(pix, 32.0, vec2(4.0, -8.0), 2.0, 0.24 * Ash, 23.0, 2.0);
        float a = clamp(s * bw.a * vis * 0.8, 0.0, 1.0);
        ov += vec3(0.78, 0.74, 0.68) * a;
        oa += a;
    }

    float A = clamp(oa, 0.0, 1.0);
    float D = clamp(dark, 0.0, 0.6);
    float srcA = 1.0 - (1.0 - A) * (1.0 - D);
    vec3 rgb = srcA > 0.0001 ? ov / srcA : vec3(0.0);
    gl_FragColor = vec4(clamp(rgb * Tint, 0.0, 1.0), srcA);
}
"#;

local! {
    /// The material, loaded at the first draw (None when the shader does not compile: no weather).
    static MATERIAL: std::cell::RefCell<Option<Option<Material>>> = std::cell::RefCell::new(None);
    /// 1×1 textures: the quad's, and the fog's when it is off (no darkness).
    static ONE: Texture2D = Texture2D::from_rgba8(1, 1, &[255, 255, 255, 255]);
    static NO_FOG: Texture2D = Texture2D::from_rgba8(1, 1, &[0, 0, 0, 0]);
}

fn load() -> Option<Material> {
    let uniforms = [
        ("MapSize", UniformType::Float2),
        ("HomRow0", UniformType::Float3),
        ("HomRow1", UniformType::Float3),
        ("HomRow2", UniformType::Float3),
        ("Snow", UniformType::Float1),
        ("SwampLevel", UniformType::Float1),
        ("LowlandLevel", UniformType::Float1),
        ("Heat", UniformType::Float1),
        ("Ash", UniformType::Float1),
        ("Clouds", UniformType::Float1),
        ("Phase", UniformType::Float1),
        ("Clock", UniformType::Float1),
        ("Tint", UniformType::Float3),
        ("Layers", UniformType::Float1),
        ("Zoom", UniformType::Float1),
        ("RowHeight", UniformType::Float1),
    ];
    let params = MaterialParams {
        uniforms: uniforms.iter().map(|(n, t)| UniformDesc::new(n, *t)).collect(),
        textures: vec!["Weather".to_string(), "LowlandMask".to_string(), "Fog".to_string()],
        // The weather is laid over the drawn map by its alpha (the default pipeline ignores it).
        pipeline_params: crate::ui::sharp::alpha_pipeline(),
    };
    match load_material(ShaderSource::Glsl { vertex: VERTEX, fragment: FRAGMENT }, params) {
        Ok(m) => Some(m),
        Err(e) => {
            razdor::diag::log(&format!("weather: the shader does not compile: {e}"));
            None
        }
    }
}

/// Draws the weather of `pass` over its rectangle (window pixels), after the map. Without the shader
/// it draws nothing.
pub fn draw(pass: &Pass) {
    let fog = pass.fog.cloned().unwrap_or_else(|| NO_FOG.with(|t| t.clone()));
    MATERIAL.with(|cell| {
        let mut cell = cell.borrow_mut();
        let Some(m) = cell.get_or_insert_with(load).as_ref() else { return };
        gl_use_material(m);
        m.set_texture("Weather", pass.biome.clone());
        m.set_texture("LowlandMask", pass.lowland.clone());
        m.set_texture("Fog", fog);
        m.set_uniform("MapSize", vec2(pass.map[0], pass.map[1]));
        let rows = pass.projection.matrix();
        for (name, row) in [("HomRow0", rows[0]), ("HomRow1", rows[1]), ("HomRow2", rows[2])] {
            m.set_uniform(name, vec3(row[0], row[1], row[2]));
        }
        let l = pass.levels;
        m.set_uniform("Snow", l.snow);
        m.set_uniform("SwampLevel", l.swamp);
        m.set_uniform("LowlandLevel", l.lowland);
        m.set_uniform("Heat", l.heat);
        m.set_uniform("Ash", l.ash);
        m.set_uniform("Clouds", l.clouds);
        m.set_uniform("Phase", pass.phase);
        m.set_uniform("Clock", pass.clock);
        m.set_uniform("Tint", vec3(pass.tint[0], pass.tint[1], pass.tint[2]));
        m.set_uniform("Layers", pass.layers);
        m.set_uniform("Zoom", pass.zoom);
        m.set_uniform("RowHeight", pass.row_height);
        let d = pass.draw;
        let one = ONE.with(|t| t.clone());
        draw_texture_ex(&one, d[0], d[1], WHITE, DrawTextureParams { dest_size: Some(vec2(d[2], d[3])), ..Default::default() });
        gl_use_default_material();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use razdor::dt::gfx::TERRAIN_TEXTURES;

    fn schedule(seed: u64) -> Schedule {
        Schedule { seed, day: None }
    }

    #[test]
    fn the_switch_is_off_until_it_is_set() {
        set_pref(false);
        assert!(!on());
        set_pref(true);
        assert!(on());
        set_pref(false);
    }

    #[test]
    fn the_codes_name_the_textures_the_biomes_are_about() {
        // The biome table's codes, against the install's texture names (`gfx::TERRAIN_TEXTURES`).
        let name = |c: u8| TERRAIN_TEXTURES[c as usize];
        for c in [14, 15] {
            assert!(["Snow.lit", "Ice.lit"].contains(&name(c)), "{c}: {}", name(c));
            assert_eq!(weights(c).0, [1.0, 0.0, 0.0, 0.0]);
        }
        for c in [8, 9] {
            assert!(["Swamp.lit", "DeepSwamp.lit"].contains(&name(c)), "{c}: {}", name(c));
            assert_eq!(weights(c).0, [0.0, 1.0, 0.0, 0.0]);
        }
        for c in [5, 6, 7] {
            assert!(["LowLand.lit", "land.lit", "plain.lit"].contains(&name(c)), "{c}: {}", name(c));
            assert_eq!(weights(c), ([0.0; 4], 1.0));
        }
        for c in [10, 13] {
            assert!(["Desert.lit", "Dust.lit"].contains(&name(c)), "{c}: {}", name(c));
            assert_eq!(weights(c).0, [0.0, 0.0, 1.0, 0.0]);
        }
        for c in [3, 11] {
            assert!(["FlameLand.lit", "Badground.lit"].contains(&name(c)), "{c}: {}", name(c));
            assert_eq!(weights(c).0, [0.0, 0.0, 0.5, 1.0]);
        }
    }

    #[test]
    fn water_roads_and_rock_get_no_weather() {
        for c in [0u8, 1, 2, 4, 12] {
            assert_eq!(weights(c), ([0.0; 4], 0.0), "code {c}");
        }
    }

    #[test]
    fn the_mask_texels_hold_one_texel_per_cell_with_the_weights_as_bytes() {
        let (weather, lowland) = mask_texels(&[14, 6, 3, 0]);
        assert_eq!(weather, vec![255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 255, 0, 0, 0, 0]);
        assert_eq!(lowland, vec![0, 0, 0, 255, 255, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn a_spell_is_deterministic_and_the_map_seed_tells_maps_apart() {
        for day in 0..50 {
            for b in Biome::ALL {
                assert_eq!(spell(7, day, b), spell(7, day, b));
            }
        }
        let a = map_seed(2, 2, &[1, 2, 3, 4]);
        let b = map_seed(2, 2, &[1, 2, 3, 5]);
        let c = map_seed(4, 1, &[1, 2, 3, 4]);
        assert!(a != b && a != c && b != c);
        let same = (0..200).filter(|&d| spell(a, d, Biome::Snow) == spell(b, d, Biome::Snow)).count();
        assert!(same < 200, "two maps do not share their weather every day");
    }

    #[test]
    fn each_biome_has_its_chance_of_weather_over_a_thousand_days() {
        for b in Biome::ALL {
            let mut on = 0;
            let mut heavy = 0;
            for day in 0..1000 {
                match spell(0x1234_5678, day, b) {
                    Spell::None => {}
                    Spell::Light => on += 1,
                    Spell::Heavy => {
                        on += 1;
                        heavy += 1;
                    }
                }
            }
            let share = on as f32 / 1000.0;
            assert!((share - b.chance()).abs() < 0.05, "{b:?}: {share} against {}", b.chance());
            let heavy_share = heavy as f32 / on as f32;
            assert!((heavy_share - HEAVY_SHARE).abs() < 0.06, "{b:?}: heavy {heavy_share}");
        }
    }

    #[test]
    fn the_schedule_is_the_same_on_every_call_and_for_every_day_of_a_fixed_day_map() {
        let s = Schedule { seed: 99, day: Some(12) };
        let t = [0.0, 0.3, 4.99, 12.5, 40.2];
        for &x in &t {
            assert_eq!(Levels::at(&s, x), Levels::at(&s, 12.5), "a fixed day is the same at every time: {x}");
        }
        assert_eq!(schedule(5).level(3.25, Biome::Ash), schedule(5).level(3.25, Biome::Ash));
    }

    #[test]
    fn the_levels_stay_in_range_and_never_jump_across_a_midnight() {
        let s = schedule(0xABCDEF);
        let step = 0.0005; // 43 seconds of game time
        for b in Biome::ALL {
            let mut prev = s.level(0.0, b);
            let mut t = step;
            while t < 200.0 {
                let v = s.level(t, b);
                assert!((0.0..=1.0).contains(&v), "{b:?} at {t}: {v}");
                // The blend's steepest slope is 1.5 over one game hour: at most 0.02 per 43 seconds.
                assert!((v - prev).abs() <= 0.02, "{b:?} at {t}: a jump {prev} to {v}");
                prev = v;
                t += step;
            }
        }
    }

    #[test]
    fn the_midnight_blend_runs_over_one_game_hour_from_the_day_before_to_the_next() {
        let s = schedule(0x51DE);
        for b in Biome::ALL {
            for k in 1..40 {
                let before = s.day_level(k - 1, b);
                let after = s.day_level(k, b);
                let kf = k as f64;
                // The window's edges are the days' own levels; its middle is the midway level.
                assert!((s.level(kf - HALF_HOUR, b) - before).abs() < 1e-6, "day {k}: the window's start");
                assert!((s.level(kf + HALF_HOUR, b) - after).abs() < 1e-6, "day {k}: the window's end");
                let mid = s.level(kf, b);
                assert!((mid - (before + after) / 2.0).abs() < 1e-5, "day {k}: midway {mid}");
            }
        }
    }

    #[test]
    fn the_shown_levels_ease_in_real_time_and_snap_to_the_target() {
        let target = Levels { snow: 1.0, ..Levels::default() };
        let mut shown = Levels::default();
        let mut last = 0.0;
        // Ten seconds of frames: the approach is within a thousandth by then (tau 0.8 s).
        for _ in 0..600 {
            shown = shown.eased_toward(target, 1.0 / 60.0);
            assert!(shown.snow >= last - 1e-6 && shown.snow <= 1.0, "monotonic: {}", shown.snow);
            last = shown.snow;
        }
        assert!((shown.snow - 1.0).abs() < 1e-3, "{}", shown.snow);
        // A long frame moves most of the way, but not past the target.
        let far = Levels::default().eased_toward(target, 10.0);
        assert!(far.snow <= 1.0 && far.snow > 0.99);
        assert_eq!(Levels::default().eased_toward(Levels::default(), 5.0), Levels::default());
    }

    #[test]
    fn a_frame_without_weather_draws_nothing() {
        set_pref(true);
        forget();
        // A fixed day with no weather at all: the frame draws nothing.
        let day = (0..5000).find(|&d| !Levels::at(&Schedule { seed: 1, day: Some(d) }, 0.0).any());
        let day = day.expect("some day has no weather on this seed");
        assert!(frame_levels(&Schedule { seed: 1, day: Some(day) }, day as f64 + 0.5, 0.0).is_none());
        set_pref(false);
        assert!(frame_levels(&schedule(1), 0.5, 0.0).is_none(), "off: nothing");
    }
}
