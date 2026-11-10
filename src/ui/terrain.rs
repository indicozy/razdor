//! Blended terrain: the original draws its ground with soft transitions between surfaces
//! (the gameplay video: grass fades into fields, roads and sand over about one cell), not
//! square cells. Here one shader draws the visible map in a single quad: every pixel mixes
//! the textures of the four nearest cell centres by bilinear weights. Texture space is world
//! space, so each texture tiles seamlessly across the map.
//!
//! The 16 terrain textures sit in one atlas (4×4 slots, each padded with a wrapped border so
//! linear filtering never bleeds between slots); the map's surface codes are a small texture
//! with one texel per cell, and a second one (the neighbours, [`neighbour_texels`]) holds each
//! cell's four codes, so the shader reads one texel instead of four.
//!
//! Water (codes 0–2) is animated as the original's (0x4c8864): a second texture stage holds
//! water frame `(t div 100) mod 32` (0x4c8db2) and is multiplied into the ground twice over
//! (D3D MODULATE2X), with its own coordinates of ⅓ texture per cell, so a 64×64 frame spans
//! 3×3 cells. The original applies it on the water cells' own quads only, not on the soft
//! edges a water cell lays over its neighbours; here it goes with each water code's share of
//! the blend.
//!
//! Profiling switches (read once, [`options`]; Android: a file in the app folder, desktop: an
//! environment variable). Options 1 (the neighbours shader) and 2 (the ground cache) are on by
//! default, 4 too on Android; 3 is off: 1 `profile-terrain-old` / `RAZDOR_TERRAIN_OLD=1` turns option 1 OFF
//! (the old four-fetch shader); 2 `profile-terrain-nocache` / `RAZDOR_TERRAIN_NOCACHE=1` turns
//! the ground cache OFF; 3 `profile-terrain-vrs` / `RAZDOR_TERRAIN_VRS=1` Adreno's 2×2 shading
//! rate (desktop: a no-op); 4 mediump colour maths: on by default on Android (phone GPUs run
//! mediump at up to twice the rate; `profile-terrain-highp` turns it OFF there), off on the desktop
//! (`RAZDOR_TERRAIN_MEDIUMP=1` turns it on). The map lines of the frame probe name the active set (`opt=1+2`).

use std::cell::RefCell;
use std::sync::OnceLock;

use macroquad::prelude::*;

use macroquad::miniquad::TextureWrap;
use razdor::dt::gfx::{water_frame, Image, WATER_FRAMES};
use razdor::rules::map::{is_water, Surface};
use super::{daylight, sharp, water};

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

// The old shader (option 1 off): four code fetches per pixel. `// @SHARP_UV@` is replaced by
// `sharp::SHARP_UV_GLSL` when the material is made.
const FRAGMENT: &str = r#"#version 100
precision highp float;
varying highp vec2 uv;
uniform sampler2D Cells;
uniform sampler2D Atlas;
uniform sampler2D Water;
// 1.0 when Water holds the current water frame.
uniform float WaterOn;
// The part of the map under the quad, in cells (cell centres at integers): x0, y0, x1, y1.
uniform vec4 View;
uniform vec2 MapSize;
// Texture pixels per cell.
uniform vec2 CellPx;
// Size of one terrain texture, and of its padded atlas slot.
uniform vec2 Tile;
uniform vec2 Slot;
uniform vec2 AtlasSize;
// Screen pixels per texel of the atlas and of the water frame (sharp sampling).
uniform vec2 AtlasScale;
uniform vec2 WaterSize;
uniform vec2 WaterScale;
// x: 1.0 when the ground goes on past the map's edges (Razdor's «Продолжать карту за краем»),
// else 0.0; yzw: the fog's colour there (the war fog's black). Ramp: the cells where the fog
// thickens to solid (x) and where it is solid (y), counted from the border. Clock: seconds.
uniform vec4 Edge;
uniform vec2 Ramp;
uniform float Clock;
// @SHARP_UV@

// Soft value noise (cheap, drifts with Clock): a hashed lattice, smoothly interpolated.
float hash2(vec2 q) {
    return fract(sin(dot(q, vec2(127.1, 311.7))) * 43758.5453);
}

float vnoise(vec2 q) {
    vec2 i = floor(q);
    vec2 f = fract(q);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2(1.0, 0.0)), u.x), mix(hash2(i + vec2(0.0, 1.0)), hash2(i + vec2(1.0, 1.0)), u.x), u.y);
}

float code_at(vec2 c) {
    // Past the border the cell index is clamped: the edge's own row or column continues outward.
    c = clamp(c, vec2(0.0), MapSize - 1.0);
    return floor(texture2D(Cells, (c + 0.5) / MapSize).r * 15.0 + 0.5);
}

vec3 ground(float code, vec2 px, vec3 water) {
    vec2 slot = vec2(mod(code, 4.0), floor(code / 4.0));
    vec2 t = mod(px, Tile);
    vec2 atlas_uv = (slot * Slot + (Slot - Tile) * 0.5 + t) / AtlasSize;
    vec3 c = texture2D(Atlas, sharp_uv(atlas_uv, AtlasSize, AtlasScale)).rgb;
    return code < 2.5 && WaterOn > 0.5 ? min(c * water * 2.0, 1.0) : c;
}

void main() {
    vec2 p = mix(View.xy, View.zw, uv);
    vec2 i = floor(p);
    vec2 f = p - i;
    vec2 px = p * CellPx;
    // The water stage's coordinates: 1/3 per cell from the corner of the cell before map cell 0
    // (the original's terrain has a 1-cell border); the frame texture repeats.
    vec3 w = texture2D(Water, sharp_uv((p + 1.5) / 3.0, WaterSize, WaterScale)).rgb;
    vec3 top = mix(ground(code_at(i), px, w), ground(code_at(i + vec2(1.0, 0.0)), px, w), f.x);
    vec3 bottom = mix(ground(code_at(i + vec2(0.0, 1.0)), px, w), ground(code_at(i + vec2(1.0, 1.0)), px, w), f.x);
    vec3 col = mix(top, bottom, f.y);
    if (Edge.x > 0.5) {
        // Past the border (p in cell units, the border at -0.5 and MapSize - 0.5): the war fog's
        // own colour, its opacity rising from the border (thin wisps, darker and lighter with two
        // noise layers at different scales drifting in different directions), thickening to solid
        // from Ramp.x and solid at Ramp.y. Solid is the same black as the unexplored war fog, so
        // the two meet with no seam.
        vec2 gap = max(max(vec2(-0.5) - p, p - (MapSize - 0.5)), vec2(0.0));
        float d = length(gap);
        if (d > 0.0) {
            float n = 0.6 * vnoise(p * 0.11 + Clock * vec2(0.05, 0.02)) + 0.4 * vnoise(p * 0.27 + Clock * vec2(-0.08, 0.05));
            float o = smoothstep(0.0, Ramp.y, d) * (0.35 + 0.65 * n);
            o = mix(o, 1.0, smoothstep(Ramp.x, Ramp.y, d));
            col = mix(col, Edge.yzw, clamp(o, 0.0, 1.0));
        }
    }
    gl_FragColor = vec4(col, 1.0);
}
"#;

// The neighbours shader (option 1). `@CP@` is the precision of the colour and blend-weight
// values: `highp` (the default) or `mediump` (option 4). Coordinates stay highp.
// `@SHARP_UV@` as above. Its result is the old shader's, pixel for pixel, with the same float
// maths except where a pixel's four codes are equal (one ground call, no blend).
const NEW_FRAGMENT: &str = r#"#version 100
precision highp float;
varying highp vec2 uv;
uniform sampler2D Neighbours;
uniform sampler2D Atlas;
uniform sampler2D Water;
uniform float WaterOn;
uniform vec4 View;
uniform vec2 MapSize;
uniform vec2 CellPx;
uniform vec2 Tile;
uniform vec2 Slot;
uniform vec2 AtlasSize;
uniform vec2 AtlasScale;
uniform vec2 WaterSize;
uniform vec2 WaterScale;
uniform vec4 Edge;
uniform vec2 Ramp;
uniform float Clock;
// 1.0 in the cache's overlay pass: the pixels with no water and no border fog are discarded
// (they keep the cached ground), the others are drawn as the full shader draws them.
uniform float Overlay;
// Live water («Живая вода», `water`): 1.0 when on. WaterMask: 255 on the water cells, 0 elsewhere
// (linear, so the shore is a soft edge). Flow: each cell's flow as rg = (128 + 127 · flow) / 255, so
// the still water (128) is exactly zero (see `flow_texels`).
// Sky: the sky's colour, which the water takes by its share of the mask.
uniform float LiveWater;
uniform sampler2D WaterMask;
uniform sampler2D Flow;
uniform vec3 Sky;
// @SHARP_UV@

float hash2(vec2 q) {
    return fract(sin(dot(q, vec2(127.1, 311.7))) * 43758.5453);
}

float vnoise(vec2 q) {
    vec2 i = floor(q);
    vec2 f = fract(q);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2(1.0, 0.0)), u.x), mix(hash2(i + vec2(0.0, 1.0)), hash2(i + vec2(1.0, 1.0)), u.x), u.y);
}

@CP@ vec3 ground(float code, vec2 px, @CP@ vec3 water, @CP@ float sea, @CP@ vec3 foam) {
    vec2 slot = vec2(mod(code, 4.0), floor(code / 4.0));
    vec2 t = mod(px, Tile);
    vec2 atlas_uv = (slot * Slot + (Slot - Tile) * 0.5 + t) / AtlasSize;
    @CP@ vec3 c = texture2D(Atlas, sharp_uv(atlas_uv, AtlasSize, AtlasScale)).rgb;
    if (code < 2.5 && WaterOn > 0.5) {
        c = min(c * water * 2.0, 1.0);
        // Live water: the sky's colour lies on the water by its share of the mask, and the foam on the shore.
        if (LiveWater > 0.5) {
            c = mix(c, Sky, 0.18 * sea) + foam;
        }
    }
    return c;
}

void main() {
    vec2 p = mix(View.xy, View.zw, uv);
    vec2 i = floor(p);
    vec2 px = p * CellPx;
    // Past the border (p in cell units, the border at -0.5 and MapSize - 0.5): the war fog.
    vec2 gap = max(max(vec2(-0.5) - p, p - (MapSize - 0.5)), vec2(0.0));
    float d = length(gap);
    bool mist = Edge.x > 0.5 && d > 0.0;
    // The four codes of the cell: code(i), code(i + (1, 0)), code(i + (0, 1)), code(i + (1, 1)),
    // each clamped to the map as `code_at` does. The texel of cell i (clamped to one cell past
    // the border, see `neighbour_texels`) holds them; its channels are code · 17 / 255.
    vec4 k = floor(texture2D(Neighbours, (clamp(i, vec2(-1.0), MapSize) + 1.5) / (MapSize + 2.0)) * 15.0 + 0.5);
    bool wet = WaterOn > 0.5 && any(lessThan(k, vec4(2.5)));
    if (Overlay > 0.5 && !wet && !mist) {
        discard;
    }
    vec2 f = p - i;
    @CP@ float fx = f.x;
    @CP@ float fy = f.y;
    // The water stage's coordinates (as the old shader): read only where a code is water.
    @CP@ vec3 w = vec3(1.0);
    // Live water: this pixel's mask (1 deep, 0.5 on the shore line, 0 on land), its foam and its
    // ripple, the same for each of the four codes. The ripple and the foam's pulse read the noise
    // along the flow (a river drifts; zero flow stands still).
    @CP@ float sea = 0.0;
    @CP@ vec3 foam = vec3(0.0);
    if (wet) {
        vec2 uvw = (p + 1.5) / 3.0;
        if (LiveWater > 0.5) {
            vec2 cell_uv = (p + 0.5) / MapSize;
            sea = texture2D(WaterMask, cell_uv).r;
            vec2 q = p - ((texture2D(Flow, cell_uv).rg * 255.0 - 128.0) / 127.0) * Clock * 0.35;
            uvw += 0.015 * vec2(sin(q.y * 2.1 + Clock * 1.3) + 0.5 * vnoise(q * 0.7 + Clock * 0.2), cos(q.x * 1.7 + Clock * 1.1));
            float band = smoothstep(0.45, 0.6, sea) * (1.0 - smoothstep(0.6, 0.85, sea));
            band *= 0.6 + 0.4 * sin(Clock * 1.6 + vnoise(q * 1.3) * 6.0);
            foam = vec3(0.85, 0.90, 0.92) * band * 0.35;
        }
        w = texture2D(Water, sharp_uv(uvw, WaterSize, WaterScale)).rgb;
    }
    @CP@ vec3 col;
    if (k.x == k.y && k.y == k.z && k.z == k.w) {
        col = ground(k.x, px, w, sea, foam);
    } else {
        @CP@ vec3 top = mix(ground(k.x, px, w, sea, foam), ground(k.y, px, w, sea, foam), fx);
        @CP@ vec3 bottom = mix(ground(k.z, px, w, sea, foam), ground(k.w, px, w, sea, foam), fx);
        col = mix(top, bottom, fy);
    }
    if (mist) {
        float n = 0.6 * vnoise(p * 0.11 + Clock * vec2(0.05, 0.02)) + 0.4 * vnoise(p * 0.27 + Clock * vec2(-0.08, 0.05));
        float o = smoothstep(0.0, Ramp.y, d) * (0.35 + 0.65 * n);
        o = mix(o, 1.0, smoothstep(Ramp.x, Ramp.y, d));
        col = mix(col, Edge.yzw, clamp(o, 0.0, 1.0));
    }
    gl_FragColor = vec4(col, 1.0);
}
"#;

/// The cache's composite: the cached ground at each pixel of the quad, read by the pixel's
/// window position (`gl_FragCoord`), so the quad's own geometry decides the coverage, as the
/// terrain's own draw does.
const COMPOSITE_FRAGMENT: &str = r#"#version 100
precision highp float;
varying highp vec2 uv;
uniform sampler2D Ground;
uniform vec2 ScreenSize;
void main() {
    gl_FragColor = texture2D(Ground, gl_FragCoord.xy / ScreenSize);
}
"#;

/// The uniforms every terrain shader declares (the old one leaves `Overlay` out of its code).
const UNIFORMS: [(&str, UniformType); 16] = [
    ("View", UniformType::Float4),
    ("MapSize", UniformType::Float2),
    ("CellPx", UniformType::Float2),
    ("Tile", UniformType::Float2),
    ("Slot", UniformType::Float2),
    ("AtlasSize", UniformType::Float2),
    ("WaterOn", UniformType::Float1),
    ("AtlasScale", UniformType::Float2),
    ("WaterSize", UniformType::Float2),
    ("WaterScale", UniformType::Float2),
    ("Edge", UniformType::Float4),
    ("Ramp", UniformType::Float2),
    ("Clock", UniformType::Float1),
    ("Overlay", UniformType::Float1),
    ("LiveWater", UniformType::Float1),
    ("Sky", UniformType::Float3),
];

/// Wrapped border around each atlas slot, in pixels.
const PAD: u32 = 2;

/// The fog thickens into solid from this many cells past the border.
pub const EDGE_SOLID: f32 = 5.0;
/// The fog is solid (the war fog's black) this many cells past the border, and beyond.
pub const EDGE_FULL: f32 = 7.0;

/// The edge fog's colour: the war fog's own (`minimap::FOG_RGB`), not a guess; with the mist on
/// («Туман вместо черноты»), the mist's, so the two meet with no seam.
pub fn edge_fog_colour() -> [f32; 3] {
    edge_fog_colour_for(crate::ui::mist::on())
}

/// [`edge_fog_colour`] for the mist setting `mist`.
pub fn edge_fog_colour_for(mist: bool) -> [f32; 3] {
    if mist {
        return crate::ui::mist::MIST_RGB;
    }
    let c = crate::ui::minimap::FOG_RGB;
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

/// The cell the ground shows at cell `c` of a map `n` cells long: the border's own cell, clamped
/// (the shader's `clamp`). Past the border it continues the edge's row or column.
pub fn edge_cell(c: i32, n: i32) -> i32 {
    if n <= 0 {
        0
    } else {
        c.clamp(0, n - 1)
    }
}

/// The fog's opacity at `d` cells past the border, without the wisps (the noise at its brightest):
/// the shader's `smoothstep` from the border to `EDGE_FULL`, solid (1) from `EDGE_SOLID` on, so it
/// is continuous at the thickening and at the full distance.
#[allow(dead_code)] // The shader's formula in Rust, pinned by the tests.
pub fn fog_opacity(d: f32) -> f32 {
    let base = smoothstep01(0.0, EDGE_FULL, d);
    let solid = smoothstep01(EDGE_SOLID, EDGE_FULL, d);
    base + (1.0 - base) * solid
}

/// GLSL's smoothstep (clamped, Hermite).
#[allow(dead_code)] // Used by `fog_opacity` only, which the tests pin.
fn smoothstep01(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The terrain profiling switches (see the module doc), as they are in effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Option 1: the neighbours shader (off: the old four-fetch shader).
    pub new_shader: bool,
    /// Option 2: the ground cache (only with the new shader).
    pub cache: bool,
    /// Option 3: Adreno's 2×2 shading rate on the terrain's ground draws.
    pub vrs: bool,
    /// Option 4: mediump colour maths (only with the new shader).
    pub mediump: bool,
}

impl Options {
    /// The active set for the frame probe's lines: `opt=1+2`, or `opt=none`.
    pub fn label(&self) -> String {
        let mut on = Vec::new();
        if self.new_shader {
            on.push("1");
        }
        if self.cache {
            on.push("2");
        }
        if self.vrs {
            on.push("3");
        }
        if self.mediump {
            on.push("4");
        }
        if on.is_empty() {
            "opt=none".into()
        } else {
            format!("opt={}", on.join("+"))
        }
    }
}

/// A switch: on Android the file in the app folder, on desktop the environment variable.
fn switch(file: &str, env: &str) -> bool {
    if cfg!(target_os = "android") {
        razdor::platform::app_dir().is_some_and(|d| d.join(file).is_file())
    } else {
        std::env::var(env).is_ok_and(|v| v == "1")
    }
}

/// The terrain options, read once. Sets the frame probe's label and logs the set.
pub fn options() -> Options {
    static OPTS: OnceLock<Options> = OnceLock::new();
    *OPTS.get_or_init(|| {
        let new_shader = !switch("profile-terrain-old", "RAZDOR_TERRAIN_OLD");
        let o = Options {
            new_shader,
            cache: new_shader && !switch("profile-terrain-nocache", "RAZDOR_TERRAIN_NOCACHE"),
            vrs: switch("profile-terrain-vrs", "RAZDOR_TERRAIN_VRS"),
            // Android: on unless `profile-terrain-highp`; the desktop: off unless `RAZDOR_TERRAIN_MEDIUMP=1`.
            mediump: new_shader
                && if cfg!(target_os = "android") { !switch("profile-terrain-highp", "") } else { switch("", "RAZDOR_TERRAIN_MEDIUMP") },
        };
        let label = o.label();
        razdor::frame_probe::set_map_options(&label);
        let vrs_note = if o.vrs && !cfg!(target_os = "android") { " (VRS: Android only, no-op here)" } else { "" };
        razdor::diag::log(&format!("terrain options: {label}{vrs_note}"));
        o
    })
}

/// The cell texture's texel for a surface code (red = code / 15).
fn code_texel(code: u8) -> [u8; 4] {
    [(code & 15) * 17, 0, 0, 255]
}

/// The neighbours texture of a `w`×`h` map, as RGBA8 bytes: one texel per cell, plus a ring of one
/// cell around the map, so the shader's clamp past the border is a plain lookup. Texel (x, y)
/// holds the cell (x − 1, y − 1), clamped to the ring: red = code(c), green = code(c + (1, 0)),
/// blue = code(c + (0, 1)), alpha = code(c + (1, 1)), each code clamped to the map as
/// [`edge_cell`] does, in the cell texel's encoding ([`code_texel`]).
pub fn neighbour_texels(codes: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (wi, hi) = (w as i32, h as i32);
    let code = |x: i32, y: i32| codes[(edge_cell(y, hi) * wi + edge_cell(x, wi)) as usize];
    let mut out = Vec::with_capacity(((wi + 2) * (hi + 2) * 4) as usize);
    for ty in 0..hi + 2 {
        for tx in 0..wi + 2 {
            let (cx, cy) = (tx - 1, ty - 1);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                out.push(code_texel(code(cx + dx, cy + dy))[0]);
            }
        }
    }
    out
}

/// The textures one map's draws bind (clones share the texture).
#[derive(Clone)]
struct MapTextures {
    cells: Texture2D,
    neighbours: Texture2D,
    /// The live water's mask (`water_mask_texels`).
    mask: Texture2D,
    /// The rivers' flow (zero until the flow is built: a still river).
    flow: Texture2D,
}

/// A map's surface codes, their textures and their size, kept while the codes stay the same.
struct CellTextures {
    codes: Vec<u8>,
    size: (u32, u32),
    tex: MapTextures,
}

/// The live water's mask of a map's surface codes, as RGBA8 bytes, one texel per cell (row by row):
/// red 255 where the cell is water (`map::is_water`), 0 otherwise.
pub fn water_mask_texels(codes: &[u8]) -> Vec<u8> {
    codes
        .iter()
        .flat_map(|&c| {
            let wet = Surface::from_code(c).is_some_and(is_water);
            [if wet { 255 } else { 0 }, 0, 0, 255]
        })
        .collect()
}

/// How a terrain draw is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The whole terrain in one pass: ground, water and the border's fog.
    Full,
    /// The ground only (water and the fog left out), drawn into the cache.
    Ground,
    /// The cache's ground, copied over the quad.
    Cached,
    /// Water and the border's fog over the cached ground: the pixels with neither are discarded.
    Overlay,
}

/// The ground cache: the target the ground is drawn into and the key it was drawn for.
#[derive(Default)]
struct GroundCache {
    target: Option<RenderTarget>,
    key: Option<u64>,
}

/// The blending shader, the terrain atlas and the current map's surface codes.
pub struct TerrainLayer {
    /// The old shader, when option 1 is off.
    old: Option<Material>,
    /// The neighbours shader in highp, and in mediump (option 4), both made at start.
    new_hp: Option<Material>,
    new_mp: Option<Material>,
    /// The cache's copy, when option 2 is on.
    composite: Option<Material>,
    /// The options in effect for this layer (a shader that fails to compile turns its options off).
    opts: Options,
    atlas: Texture2D,
    tile: Vec2,
    slot: Vec2,
    /// The water frames ([`WATER_FRAMES`] or none, then water is still).
    water: Vec<Texture2D>,
    /// Surface codes and size of the map in `cells`, to rebuild it when the map changes.
    cells: RefCell<Option<CellTextures>>,
    ground: RefCell<GroundCache>,
}

/// Packs the terrain textures (by surface code) into a 4×4 atlas of padded slots. Every
/// texture is repeated into a slot of the first one's size; a missing texture is `fill[code]`.
pub fn pack_terrain_atlas(textures: &[Option<Image>; 16], fill: &[[u8; 4]; 16]) -> Option<(Image, (u32, u32))> {
    let first = textures.iter().flatten().next()?;
    let (tw, th) = (first.width, first.height);
    let (sw, sh) = (tw + 2 * PAD, th + 2 * PAD);
    let (aw, ah) = (sw * 4, sh * 4);
    let mut rgba = vec![0u8; (aw * ah * 4) as usize];
    for (code, tex) in textures.iter().enumerate() {
        let (ox, oy) = ((code as u32 % 4) * sw, (code as u32 / 4) * sh);
        for y in 0..sh {
            for x in 0..sw {
                // Slot pixel (x, y) shows texture pixel (x − PAD, y − PAD), wrapped.
                let px = match tex {
                    Some(t) if t.width > 0 && t.height > 0 => {
                        let u = (x + tw - PAD) % tw % t.width;
                        let v = (y + th - PAD) % th % t.height;
                        let [r, g, b, _] = t.pixel(u, v);
                        [r, g, b, 255]
                    }
                    _ => fill[code],
                };
                let i = (((oy + y) * aw + ox + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&px);
            }
        }
    }
    Some((Image { width: aw, height: ah, rgba }, (tw, th)))
}

/// A water frame's texture: linear and repeating, as the frame tiles every 3 cells.
fn water_texture(img: &Image) -> Texture2D {
    let tex = Texture2D::from_rgba8(img.width as u16, img.height as u16, &img.rgba);
    tex.set_filter(FilterMode::Linear);
    // SAFETY: called on the main thread while macroquad runs, as every texture upload.
    let gl = unsafe { get_internal_gl() };
    gl.quad_context.texture_set_wrap(tex.raw_miniquad_id(), TextureWrap::Repeat, TextureWrap::Repeat);
    tex
}

/// A material of `fragment` with the uniforms and textures it declares.
fn make_material(fragment: &str, uniforms: &[(&str, UniformType)], textures: &[&str]) -> Result<Material, String> {
    let params = MaterialParams {
        uniforms: uniforms.iter().map(|(n, t)| UniformDesc::new(n, *t)).collect(),
        textures: textures.iter().map(|t| t.to_string()).collect(),
        ..Default::default()
    };
    load_material(ShaderSource::Glsl { vertex: VERTEX, fragment }, params).map_err(|e| e.to_string())
}

/// The neighbours shader with the given colour precision (`highp` or `mediump`).
fn new_fragment(precision: &str) -> String {
    NEW_FRAGMENT.replace("@CP@", precision).replace("@SHARP_UV@", sharp::SHARP_UV_GLSL)
}

impl TerrainLayer {
    /// A layer over the given textures, or `None` without any texture or if no shader compiles
    /// (callers then draw square cells). `water` are the animated water frames, uploaded once;
    /// without all [`WATER_FRAMES`] of them water stays still.
    pub fn new(textures: &[Option<Image>; 16], fill: &[[u8; 4]; 16], water: &[Image]) -> Option<TerrainLayer> {
        let (image, (tw, th)) = pack_terrain_atlas(textures, fill)?;
        let mut opts = options();
        let (mut old, mut new_hp, mut new_mp, mut composite) = (None, None, None, None);
        if opts.new_shader {
            let hp_ok = make_material(&new_fragment("highp"), &UNIFORMS, &["Neighbours", "Atlas", "Water", "WaterMask", "Flow"]);
            match hp_ok {
                Ok(m) => {
                    new_hp = Some(m);
                    // Both precisions are made at start; option 4 picks one.
                    new_mp = make_material(&new_fragment("mediump"), &UNIFORMS, &["Neighbours", "Atlas", "Water", "WaterMask", "Flow"]).map_err(|e| razdor::diag!("terrain shader (mediump): {e}")).ok();
                }
                Err(e) => razdor::diag!("terrain shader: {e}"),
            }
            if new_hp.is_none() {
                razdor::diag!("terrain: the neighbours shader does not compile; the old shader is used");
                opts = Options { new_shader: false, cache: false, mediump: false, ..opts };
            }
        }
        if !opts.new_shader {
            old = Some(make_material(&FRAGMENT.replace("// @SHARP_UV@", sharp::SHARP_UV_GLSL), &UNIFORMS, &["Cells", "Atlas", "Water"]).map_err(|e| razdor::diag!("terrain shader: {e}")).ok()?);
        }
        if opts.mediump && new_mp.is_none() {
            opts.mediump = false;
        }
        if opts.cache {
            composite = make_material(COMPOSITE_FRAGMENT, &[("ScreenSize", UniformType::Float2)], &["Ground"]).map_err(|e| razdor::diag!("terrain cache copy: {e}")).ok();
            opts.cache = composite.is_some();
        }
        let atlas = Texture2D::from_rgba8(image.width as u16, image.height as u16, &image.rgba);
        atlas.set_filter(FilterMode::Linear);
        let water = if water.len() == WATER_FRAMES && water.iter().all(|f| f.width > 0 && f.height > 0) {
            water.iter().map(water_texture).collect()
        } else {
            Vec::new()
        };
        Some(TerrainLayer {
            old,
            new_hp,
            new_mp,
            composite,
            opts,
            atlas,
            tile: vec2(tw as f32, th as f32),
            slot: vec2((tw + 2 * PAD) as f32, (th + 2 * PAD) as f32),
            water,
            cells: RefCell::new(None),
            ground: RefCell::new(GroundCache::default()),
        })
    }

    /// Whether the ground cache is in use for this layer (option 2).
    pub fn cache_on(&self) -> bool {
        self.opts.cache
    }

    /// The textures of a `w`×`h` map (the codes, the neighbours, the water mask and the flow),
    /// rebuilt only when the codes change.
    fn cells(&self, codes: &[u8], w: u32, h: u32) -> MapTextures {
        let mut cache = self.cells.borrow_mut();
        if let Some(c) = cache.as_ref() {
            if c.size == (w, h) && c.codes == codes {
                return c.tex.clone();
            }
        }
        let rgba: Vec<u8> = codes.iter().flat_map(|&c| code_texel(c)).collect();
        let cells = Texture2D::from_rgba8(w as u16, h as u16, &rgba);
        cells.set_filter(FilterMode::Nearest);
        let neighbours = Texture2D::from_rgba8((w + 2) as u16, (h + 2) as u16, &neighbour_texels(codes, w, h));
        neighbours.set_filter(FilterMode::Nearest);
        let mask = Texture2D::from_rgba8(w as u16, h as u16, &water_mask_texels(codes));
        mask.set_filter(FilterMode::Linear);
        // Zero flow: rg = ½ (the flow is built from the map's objects in the next step).
        let flow = Texture2D::from_rgba8(1, 1, &[128, 128, 128, 255]);
        flow.set_filter(FilterMode::Linear);
        let tex = MapTextures { cells, neighbours, mask, flow };
        *cache = Some(CellTextures { codes: codes.to_vec(), size: (w, h), tex: tex.clone() });
        tex
    }

    /// The cache's target for `key` (option 2): the screen's size in physical pixels, made or
    /// resized as needed. Returns it with whether it must be filled now (the key changed).
    /// `None` when the screen has no usable size (then the terrain is drawn live).
    pub fn ground_target(&self, key: u64) -> Option<(RenderTarget, bool)> {
        let (w, h) = macroquad::miniquad::window::screen_size();
        if !(w >= 1.0 && h >= 1.0 && w <= 8192.0 && h <= 8192.0) {
            return None;
        }
        let (pw, ph) = (w as u32, h as u32);
        let mut g = self.ground.borrow_mut();
        let sized = g.target.as_ref().is_some_and(|t| t.texture.width() as u32 == pw && t.texture.height() as u32 == ph);
        if !sized {
            let rt = render_target(pw, ph);
            rt.texture.set_filter(FilterMode::Nearest);
            g.target = Some(rt);
            g.key = None;
        }
        let fill = g.key != Some(key);
        if fill {
            g.key = Some(key);
        }
        g.target.clone().map(|rt| (rt, fill))
    }

    /// Draws the terrain of a `w`×`h` map (`codes` row by row) into screen rectangle `dest`,
    /// which shows the map area `view` in cells (x0, y0, x1, y1; cell centres at integers).
    /// `cell_px` is the texture pixels per cell (the textures' scale). `water_ms` is the clock
    /// (ms) that animates the water, `None` for still water. `edge`: the ground goes on past the
    /// map's edges: the ground continues, then the animated fog ([`fog_opacity`]); `view` may
    /// reach past the map then. The fog's clock is `water_ms`. `mode`: see [`Mode`].
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, codes: &[u8], (w, h): (u32, u32), dest: Rect, view: Vec4, cell_px: Vec2, water_ms: Option<i64>, edge: bool, mode: Mode) {
        if w == 0 || h == 0 || w + 2 > u16::MAX as u32 || h + 2 > u16::MAX as u32 || codes.len() != (w * h) as usize {
            return;
        }
        if mode == Mode::Cached {
            self.draw_cached(dest);
            return;
        }
        let Some((m, new)) = self.ground_material() else { return };
        // The trailer's stills (dev-only): the atlas is sampled nearest, so the grass is not streaked.
        if super::world_view::trailer_active() {
            self.atlas.set_filter(FilterMode::Nearest);
        }
        let maps = self.cells(codes, w, h);
        gl_use_material(m);
        if new {
            m.set_texture("Neighbours", maps.neighbours.clone());
            m.set_texture("WaterMask", maps.mask.clone());
            m.set_texture("Flow", maps.flow.clone());
        } else {
            m.set_texture("Cells", maps.cells.clone());
        }
        m.set_texture("Atlas", self.atlas.clone());
        // Water and the fog belong to the full pass and the overlay; the ground pass has neither.
        let water_on = matches!(mode, Mode::Full | Mode::Overlay);
        let water = water_ms.filter(|_| water_on && !self.water.is_empty()).map(|ms| self.water[water_frame(ms)].clone());
        m.set_uniform("WaterOn", if water.is_some() { 1.0f32 } else { 0.0 });
        let water = water.unwrap_or_else(|| self.atlas.clone());
        m.set_uniform("WaterSize", vec2(water.width(), water.height()));
        // Screen pixels per texel: the quad spans `view` cells over `dest`; the atlas has
        // `cell_px` texels per cell, and the water frame 1/3 of its texture per cell.
        let (cells_w, cells_h) = (view.z - view.x, view.w - view.y);
        m.set_uniform("AtlasScale", vec2(dest.w / (cells_w * cell_px.x), dest.h / (cells_h * cell_px.y)));
        m.set_uniform("WaterScale", vec2(3.0 * dest.w / (cells_w * water.width()), 3.0 * dest.h / (cells_h * water.height())));
        m.set_texture("Water", water);
        m.set_uniform("View", view);
        m.set_uniform("MapSize", vec2(w as f32, h as f32));
        m.set_uniform("CellPx", cell_px);
        m.set_uniform("Tile", self.tile);
        m.set_uniform("Slot", self.slot);
        m.set_uniform("AtlasSize", vec2(self.atlas.width(), self.atlas.height()));
        let edge_on = matches!(mode, Mode::Full | Mode::Overlay) && edge;
        let fog = edge_fog_colour();
        m.set_uniform("Edge", vec4(if edge_on { 1.0 } else { 0.0 }, fog[0], fog[1], fog[2]));
        m.set_uniform("Ramp", vec2(EDGE_SOLID, EDGE_FULL));
        m.set_uniform("Clock", water_ms.map_or(0.0, |ms| (ms as f32 / 1000.0) % 1000.0));
        m.set_uniform("Overlay", if mode == Mode::Overlay { 1.0f32 } else { 0.0 });
        // Live water («Живая вода»): the water's draws only (the full pass and the overlay). The
        // sky's colour is the lighting's (the day's with the lighting off).
        m.set_uniform("LiveWater", if water::live() && water_on { 1.0f32 } else { 0.0 });
        let sky = daylight::sky_rgb();
        m.set_uniform("Sky", vec3(sky[0], sky[1], sky[2]));
        // The ground draws (the whole terrain, the cache's ground) get the shading rate (option 3).
        let vrs = self.opts.vrs && matches!(mode, Mode::Full | Mode::Ground);
        if vrs {
            shading_rate::begin();
        }
        let shown = if new { &maps.neighbours } else { &maps.cells };
        draw_texture_ex(shown, dest.x, dest.y, WHITE, DrawTextureParams { dest_size: Some(dest.size()), ..Default::default() });
        if vrs {
            shading_rate::end();
        }
        gl_use_default_material();
    }

    /// The cache's copy: the filled ground, read at each pixel of `dest` by its window position.
    fn draw_cached(&self, dest: Rect) {
        let (Some(m), Some(rt)) = (self.composite.as_ref(), self.ground.borrow().target.clone()) else { return };
        // The composite reads the ground at its fragment's place on the target it draws into: the
        // map's scaled target when one is set (`render_cache::with_base`), else the screen.
        let (sw, sh) = super::render_cache::base_pixels().unwrap_or_else(macroquad::miniquad::window::screen_size);
        gl_use_material(m);
        m.set_texture("Ground", rt.texture.clone());
        m.set_uniform("ScreenSize", vec2(sw, sh));
        draw_texture_ex(&rt.texture, dest.x, dest.y, WHITE, DrawTextureParams { dest_size: Some(dest.size()), ..Default::default() });
        gl_use_default_material();
    }

    /// The material of the ground draws, and whether it is the neighbours shader.
    fn ground_material(&self) -> Option<(&Material, bool)> {
        if self.opts.new_shader {
            let m = if self.opts.mediump { self.new_mp.as_ref().or(self.new_hp.as_ref()) } else { self.new_hp.as_ref() };
            m.map(|m| (m, true))
        } else {
            self.old.as_ref().map(|m| (m, false))
        }
    }
}

/// Adreno's variable-rate shading for the terrain (option 3). `GL_QCOM_shading_rate` gives
/// `glShadingRateQCOM`, which applies to the draws after it: the terrain's ground draws are
/// shaded at 2×2 pixels per fragment, and the rate goes back to 1×1 after them. The batch is
/// flushed on each side so only the terrain is affected. Off Android, and without the
/// extension, both do nothing.
#[cfg(target_os = "android")]
mod shading_rate {
    use std::ffi::{c_char, c_void, CStr};
    use std::sync::OnceLock;

    /// The tokens of the extension spec (GL_QCOM_shading_rate: SHADING_RATE_1X1_PIXELS_QCOM and
    /// SHADING_RATE_2X2_PIXELS_QCOM).
    const RATE_1X1: u32 = 0x96A6;
    const RATE_2X2: u32 = 0x96A9;

    type RateFn = unsafe extern "C" fn(u32);
    type GetProcFn = unsafe extern "C" fn(*const c_char) -> *const c_void;

    /// `glShadingRateQCOM`, loaded once: from libEGL's `eglGetProcAddress` (miniquad's GL loader
    /// does not list it). `None` without the extension or the function.
    fn rate_fn() -> Option<RateFn> {
        static F: OnceLock<Option<RateFn>> = OnceLock::new();
        *F.get_or_init(load)
    }

    fn load() -> Option<RateFn> {
        // SAFETY: called on the GL thread after the context exists; the symbols are looked up
        // by name in the library the process already has loaded, and the pointers are called
        // with the spec's signatures.
        unsafe {
            let exts = macroquad::miniquad::gl::glGetString(macroquad::miniquad::gl::GL_EXTENSIONS);
            let present = !exts.is_null() && CStr::from_ptr(exts as *const c_char).to_string_lossy().split(' ').any(|e| e == "GL_QCOM_shading_rate");
            if !present {
                razdor::diag::log("terrain VRS: GL_QCOM_shading_rate is absent; option 3 does nothing");
                return None;
            }
            let lib = libc::dlopen(c"libEGL.so".as_ptr(), libc::RTLD_NOLOAD | libc::RTLD_LAZY);
            let get: Option<GetProcFn> = if lib.is_null() {
                None
            } else {
                let sym = libc::dlsym(lib, c"eglGetProcAddress".as_ptr());
                (!sym.is_null()).then(|| std::mem::transmute::<*mut c_void, GetProcFn>(sym))
            };
            let p = get.map_or(std::ptr::null(), |get| get(c"glShadingRateQCOM".as_ptr()));
            razdor::diag::log(&format!("terrain VRS: GL_QCOM_shading_rate present, glShadingRateQCOM {}", if p.is_null() { "not found" } else { "loaded" }));
            (!p.is_null()).then(|| std::mem::transmute::<*const c_void, RateFn>(p))
        }
    }

    fn set(rate: u32) {
        if let Some(f) = rate_fn() {
            // SAFETY: the function was loaded with the extension's signature (`ShadingRateQCOM(enum)`).
            unsafe {
                macroquad::window::get_internal_gl().flush();
                f(rate);
            }
        }
    }

    /// The draws after this are shaded at 2×2.
    pub fn begin() {
        set(RATE_2X2);
    }

    /// The draws after this are shaded at 1×1 again.
    pub fn end() {
        set(RATE_1X1);
    }
}

#[cfg(not(target_os = "android"))]
mod shading_rate {
    /// Desktop: no variable-rate shading (option 3 does nothing).
    pub fn begin() {}
    /// Desktop: as [`begin`].
    pub fn end() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_slots_hold_wrapped_textures_and_fills() {
        // A 3×2 texture whose pixel (x, y) has red = 10x + y.
        let mut rgba = Vec::new();
        for y in 0..2u8 {
            for x in 0..3u8 {
                rgba.extend_from_slice(&[10 * x + y, 0, 0, 255]);
            }
        }
        let mut textures: [Option<Image>; 16] = Default::default();
        textures[5] = Some(Image { width: 3, height: 2, rgba });
        let fill = [[1, 2, 3, 255]; 16];
        let (atlas, tile) = pack_terrain_atlas(&textures, &fill).unwrap();
        assert_eq!(tile, (3, 2));
        let (sw, sh) = (3 + 2 * PAD, 2 + 2 * PAD);
        assert_eq!((atlas.width, atlas.height), (4 * sw, 4 * sh));
        // Code 5 is slot (1, 1). Its inner pixel (0, 0) is texture (0, 0); the border wraps.
        let (ox, oy) = (sw, sh);
        assert_eq!(atlas.pixel(ox + PAD, oy + PAD)[0], 0);
        assert_eq!(atlas.pixel(ox + PAD + 2, oy + PAD + 1)[0], 21);
        assert_eq!(atlas.pixel(ox + PAD - 1, oy + PAD)[0], 20);
        assert_eq!(atlas.pixel(ox + PAD, oy + PAD - 1)[0], 1);
        assert_eq!(atlas.pixel(ox + PAD + 3, oy + PAD + 2)[0], 0);
        // Other codes are the fill colour.
        assert_eq!(atlas.pixel(0, 0), [1, 2, 3, 255]);
    }

    #[test]
    fn the_extension_continues_the_border_cell() {
        // A 5-cell map: the cells inside are their own; past either border the ground shows the
        // border's own cell, however far out (the edge row or column goes on).
        assert_eq!((0..5).map(|c| edge_cell(c, 5)).collect::<Vec<_>>(), [0, 1, 2, 3, 4]);
        assert_eq!([-1, -2, -9].map(|c| edge_cell(c, 5)), [0, 0, 0]);
        assert_eq!([5, 6, 40].map(|c| edge_cell(c, 5)), [4, 4, 4]);
        assert_eq!(edge_cell(3, 0), 0, "an empty map shows nothing");
    }

    #[test]
    fn the_fog_starts_at_the_border_thickens_and_is_solid_at_the_full_distance() {
        const { assert!(0.0 < EDGE_SOLID && EDGE_SOLID < EDGE_FULL) };
        assert_eq!(fog_opacity(0.0), 0.0, "clear on the border");
        assert_eq!(fog_opacity(EDGE_FULL), 1.0, "solid at the full distance");
        assert_eq!(fog_opacity(EDGE_FULL + 5.0), 1.0, "and beyond");
        assert!(fog_opacity(EDGE_SOLID) >= 0.8 && fog_opacity(EDGE_SOLID) < 1.0, "thick, not yet solid: {}", fog_opacity(EDGE_SOLID));
        // Monotone and within [0, 1] across the whole range: no step, no line.
        let mut last = 0.0;
        for i in 0..=400 {
            let d = i as f32 / 40.0;
            let o = fog_opacity(d);
            assert!((0.0..=1.0).contains(&o) && o >= last - 1e-6, "{d}: {o}");
            last = o;
        }
    }

    #[test]
    fn the_edge_fog_is_the_war_fogs_own_black() {
        // The war fog is drawn in `minimap::FOG_RGB`; the edge fog takes the same colour.
        let c = edge_fog_colour_for(false);
        let w = crate::ui::minimap::FOG_RGB;
        assert_eq!(c, [w[0] as f32 / 255.0, w[1] as f32 / 255.0, w[2] as f32 / 255.0]);
        assert_eq!(c, [0.0, 0.0, 0.0], "the war fog is black");
    }

    #[test]
    fn with_the_mist_the_edge_fog_is_the_mists_colour() {
        // «Туман вместо черноты»: the edge fog takes the mist's colour, so it meets the mist with no seam.
        assert_eq!(edge_fog_colour_for(true), crate::ui::mist::MIST_RGB);
    }

    #[test]
    fn the_water_mask_is_255_on_the_water_codes_only() {
        // Codes 0-2 are water (shallows and fords, coastal water, deep sea); 3 (lava) and 5 are land.
        let texels = water_mask_texels(&[0, 3, 2, 5]);
        assert_eq!(texels, vec![255, 0, 0, 255, 0, 0, 0, 255, 255, 0, 0, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn code_texels_round_trip_through_the_shader_formula() {
        for code in 0..16u8 {
            let r = code_texel(code)[0] as f32 / 255.0;
            assert_eq!((r * 15.0 + 0.5).floor() as u8, code);
        }
    }

    /// The shader's lookup of the neighbours texture for cell (i, j), as its GLSL reads it: the
    /// cell clamped to the ring, its texel, and the four channels decoded as `floor(v·15 + ½)`.
    fn shader_codes(tex: &[u8], w: i32, h: i32, i: i32, j: i32) -> [u8; 4] {
        let (tx, ty) = (i.clamp(-1, w) + 1, j.clamp(-1, h) + 1);
        let at = ((ty * (w + 2) + tx) * 4) as usize;
        [0, 1, 2, 3].map(|c| ((tex[at + c] as f32 / 255.0) * 15.0 + 0.5).floor() as u8)
    }

    #[test]
    fn neighbour_texels_hold_the_four_codes_with_the_border_clamp() {
        // A 3×2 map, row by row: the codes are 1..=6.
        let codes = [1u8, 2, 3, 4, 5, 6];
        let tex = neighbour_texels(&codes, 3, 2);
        assert_eq!(tex.len(), (3 + 2) * (2 + 2) * 4, "one texel per cell plus the ring");
        // The code the old shader reads at (i, j), clamped as `code_at` clamps it.
        let code_at = |i: i32, j: i32| codes[(edge_cell(j, 2) * 3 + edge_cell(i, 3)) as usize];
        // Every cell from one ring cell out past the map: the four codes equal the old shader's
        // four `code_at` lookups, pixel for pixel.
        for j in -6..=6 {
            for i in -6..=6 {
                let want = [code_at(i, j), code_at(i + 1, j), code_at(i, j + 1), code_at(i + 1, j + 1)];
                assert_eq!(shader_codes(&tex, 3, 2, i, j), want, "cell ({i}, {j})");
            }
        }
        // The corners, spelled out: the first cell has its right and lower neighbours; the last
        // one's neighbours past the edge are its own codes.
        assert_eq!(shader_codes(&tex, 3, 2, 0, 0), [1, 2, 4, 5]);
        assert_eq!(shader_codes(&tex, 3, 2, 2, 1), [6, 6, 6, 6]);
        assert_eq!(shader_codes(&tex, 3, 2, -1, -1), [1, 1, 1, 1], "the ring continues the corner");
    }
}
