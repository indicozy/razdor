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

use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation, PipelineParams, TextureWrap};
use razdor::dt::gfx::{water_frame, Image, WATER_FRAMES};
use razdor::rules::map::{is_water, Surface};
use super::{daylight, sharp, water};

/// The period of the `Clock` uniform (the water clock, seconds): 6400 = 64 × 100. Every drift in the
/// shaders moves its noise by whole lattice periods (64 cells) over it, and every sine has a whole
/// number of cycles in it, so the wrap is invisible. Keep the shader constants in step with the tests.
pub const CLOCK_PERIOD: f32 = 6400.0;

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

// The lattice repeats every 64 cells, so a drift that moves the noise by a whole number of periods
// over CLOCK_PERIOD comes back to itself: the Clock's wrap is seamless (see the tests).
float vnoise(vec2 q) {
    vec2 i = floor(q);
    vec2 f = fract(q);
    vec2 u = f * f * (3.0 - 2.0 * f);
    vec2 a = mod(i, 64.0);
    vec2 b = mod(i + 1.0, 64.0);
    return mix(mix(hash2(a), hash2(vec2(b.x, a.y)), u.x), mix(hash2(vec2(a.x, b.y)), hash2(b), u.x), u.y);
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
// Live water («Живая вода», `water`): 1.0 when on. Flow, one texel per cell (linear, so the shore is a soft
// edge): rg = the flow as (128 + 127 · flow) / 255, so the still water (128) is exactly zero; b = the
// river's drift speed; a = the water mask (255 on the water cells, 0 on land). See `flow_texels`.
// Sky: the sky's colour, which the water takes by its share of the mask.
uniform float LiveWater;
// The water's detail by the map's zoom (`water::lod_detail`): 1 whole, 0 zoomed out. It scales the ripple and the
// foam's lapping; zoomed out the water keeps its texture, colour and foam.
uniform float Detail;
uniform sampler2D Flow;
uniform vec3 Sky;
// The water's noise texture (`water::noise_texels`): a wrapped 64×64 table of the lattice's hashes.
uniform sampler2D Noise;
// @SHARP_UV@

float hash2(vec2 q) {
    return fract(sin(dot(q, vec2(127.1, 311.7))) * 43758.5453);
}

// The lattice repeats every 64 cells, so a drift that moves the noise by a whole number of periods
// over CLOCK_PERIOD comes back to itself: the Clock's wrap is seamless (see the tests).
float vnoise(vec2 q) {
    vec2 i = floor(q);
    vec2 f = fract(q);
    vec2 u = f * f * (3.0 - 2.0 * f);
    vec2 a = mod(i, 64.0);
    vec2 b = mod(i + 1.0, 64.0);
    return mix(mix(hash2(a), hash2(vec2(b.x, a.y)), u.x), mix(hash2(vec2(a.x, b.y)), hash2(b), u.x), u.y);
}

// The water's value noise: the same lattice as `vnoise`, one texture fetch. The lookup sits on the lattice
// point's centre plus the smoothed fraction, so the hardware's bilinear step is the smooth interpolation.
float wnoise(vec2 q) {
    vec2 i = floor(q);
    vec2 f = fract(q);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return texture2D(Noise, (mod(i, 64.0) + u + 0.5) / 64.0).r;
}

// The ripple's offset of the water at `q` (the water's uv distortion, in frame units).
vec2 ripple(vec2 q) {
    return vec2(sin(q.y * 2.1 + Clock * 1.29983) + 0.5 * wnoise(q * 0.7 + Clock * 0.2), cos(q.x * 1.7 + Clock * 1.09956));
}


@CP@ vec3 ground(float code, vec2 px, @CP@ vec3 water, @CP@ float sea) {
    vec2 slot = vec2(mod(code, 4.0), floor(code / 4.0));
    vec2 t = mod(px, Tile);
    vec2 atlas_uv = (slot * Slot + (Slot - Tile) * 0.5 + t) / AtlasSize;
    @CP@ vec3 c = texture2D(Atlas, sharp_uv(atlas_uv, AtlasSize, AtlasScale)).rgb;
    if (code < 2.5 && WaterOn > 0.5) {
        c = min(c * water * 2.0, 1.0);
        // Live water: the sky's colour lies on the water by its share of the mask (the foam is laid on the colour below).
        if (LiveWater > 0.5) {
            c = mix(c, Sky, 0.18 * sea);
        }
    }
    return c;
}

// The shore foam at a water pixel: `cov` is the water's share of the pixel's four cells (0.5 on the waterline, 1
// deep), `p` the cell coordinate and `fl` the flow there; s = cov - 0.5 is the distance into the water, in cells.
// The main line is near-white at the waterline, full for a few hundredths of a cell and fading by a third of a cell into the water; its inner
// edge laps between the waterline and a tenth of a cell out (a wave of about 6.4 s, its phase varying along the
// shore), and a noise breaks it into patches. A fainter second line lies further out, on a wave of its own. A still
// shore (a lake, the sea) foams at full strength, a river at 0.55. The periods fit CLOCK_PERIOD (6400 s): the wave is
// 1000 cycles of it, and the drifts move the noise by whole lattice periods. Returns the foam's colour and alpha.
vec4 shore_foam(float cov, vec2 p, vec2 fl) {
    float s = cov - 0.5;
    if (s <= 0.0) {
        return vec4(0.0);
    }
    float t0 = fract(Clock * 0.05);
    float t1 = fract(Clock * 0.05 + 0.5);
    vec2 q0 = p - fl * t0 * 7.0;
    vec2 q1 = p - fl * t1 * 7.0;
    // Zoomed out (Detail 0) the line stands at its middle phase: the lapping is the moving detail.
    float lap = mix(0.5, 0.5 + 0.5 * sin(Clock * 0.98174770 + 6.0 * wnoise(q0 * 0.8)), Detail);
    float lap2 = mix(0.5, 0.5 + 0.5 * sin(Clock * 0.98174770 + 2.5 + 6.0 * wnoise(q1 * 0.6)), Detail);
    float patch = smoothstep(0.38, 0.62, 0.6 * wnoise(q0 * 2.4 + Clock * vec2(0.01, 0.0)) + 0.4 * wnoise(q1 * 5.1 + Clock * vec2(0.0, -0.02)));
    float inner = 0.1 * lap;
    float line = smoothstep(inner - 0.015, inner + 0.015, s) * (1.0 - smoothstep(inner + 0.08, inner + 0.33, s));
    float inner2 = 0.36 + 0.05 * lap2;
    float far = smoothstep(inner2 - 0.01, inner2 + 0.01, s) * (1.0 - smoothstep(inner2 + 0.03, inner2 + 0.07, s));
    float still = 0.55 + 0.45 * (1.0 - smoothstep(0.2, 0.8, length(fl)));
    float a1 = 0.85 * line * (0.3 + 0.7 * patch) * still;
    float a2 = 0.3 * far * (0.2 + 0.8 * patch) * still;
    return vec4(0.97, 0.98, 1.0, a1 + a2 - a1 * a2);
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
    // Live water: this pixel's mask (1 deep, 0.5 on the shore line, 0 on land) and its ripple, the same for each
    // of the four codes. The ripple reads the noise along the flow (a river drifts; zero flow stands still).
    @CP@ float sea = 0.0;
    vec2 fl = vec2(0.0);
    if (wet) {
        vec2 uvw = (p + 1.5) / 3.0;
        if (LiveWater > 0.5) {
            vec2 cell_uv = (p + 0.5) / MapSize;
            vec4 fd = texture2D(Flow, cell_uv);
            sea = fd.a;
            // The flow scroll: two phases half a cycle apart (a 20 s cycle, 7 cells of travel a cycle),
            // each faded out at its own wrap, so the pattern never jumps when Clock wraps.
            fl = (fd.rg * 255.0 - 128.0) / 127.0;
            float t0 = fract(Clock * 0.05);
            float t1 = fract(Clock * 0.05 + 0.5);
            float wt = abs(2.0 * t0 - 1.0);
            vec2 q0 = p - fl * t0 * 7.0;
            vec2 q1 = p - fl * t1 * 7.0;
            uvw += 0.015 * Detail * mix(ripple(q0), ripple(q1), wt);
        }
        w = texture2D(Water, sharp_uv(uvw, WaterSize, WaterScale)).rgb;
    }
    @CP@ vec3 col;
    if (k.x == k.y && k.y == k.z && k.z == k.w) {
        col = ground(k.x, px, w, sea);
    } else {
        @CP@ vec3 top = mix(ground(k.x, px, w, sea), ground(k.y, px, w, sea), fx);
        @CP@ vec3 bottom = mix(ground(k.z, px, w, sea), ground(k.w, px, w, sea), fx);
        col = mix(top, bottom, fy);
    }
    // The shore foam (live water, the water's pixels only): laid on the coast by the water's share of the four
    // cells, with the weights the colour blend uses, so it sits on the visible shore and not on the mask's edge.
    if (wet && LiveWater > 0.5) {
        float cov = (k.x < 2.5 ? 1.0 : 0.0) * (1.0 - fx) * (1.0 - fy) + (k.y < 2.5 ? 1.0 : 0.0) * fx * (1.0 - fy)
            + (k.z < 2.5 ? 1.0 : 0.0) * (1.0 - fx) * fy + (k.w < 2.5 ? 1.0 : 0.0) * fx * fy;
        vec4 fo = shore_foam(cov, p, fl);
        col = mix(col, fo.rgb, fo.a);
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

/// The unlit shader: byte for byte the text the lighting-off picture was made with (play-build), kept so that
/// the picture with the lighting and the live water off is identical to before. Used only then (`legacy_material`);
/// the relief code in it never runs, as its `Strength` is always 0 there. Do not edit it.
const LEGACY_FRAGMENT: &str = r#"#version 100
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
// The relief («Освещение»): the light (in the ground's axes), its raking strength and the cavities'
// darkening. Strength 0 is the lighting off: the land is then drawn as it always was.
uniform vec3 Light;
uniform float Strength;
uniform float Cavity;
// @SHARP_UV@
// @RELIEF@

float hash2(vec2 q) {
    return fract(sin(dot(q, vec2(127.1, 311.7))) * 43758.5453);
}

// The lattice repeats every 64 cells, so a drift that moves the noise by a whole number of periods
// over CLOCK_PERIOD comes back to itself: the Clock's wrap is seamless (see the tests).
float vnoise(vec2 q) {
    vec2 i = floor(q);
    vec2 f = fract(q);
    vec2 u = f * f * (3.0 - 2.0 * f);
    vec2 a = mod(i, 64.0);
    vec2 b = mod(i + 1.0, 64.0);
    return mix(mix(hash2(a), hash2(vec2(b.x, a.y)), u.x), mix(hash2(vec2(a.x, b.y)), hash2(b), u.x), u.y);
}

// The ripple's offset of the water at `q` (the water's uv distortion, in frame units).
vec2 ripple(vec2 q) {
    return vec2(sin(q.y * 2.1 + Clock * 1.29983) + 0.5 * vnoise(q * 0.7 + Clock * 0.2), cos(q.x * 1.7 + Clock * 1.09956));
}

// The atlas colour of a slot at `t` texels from the tile's corner. The tile repeats, so the offset is
// wrapped inside the tile (the padded border is only for the filter): any offset of the relief's
// samples reads the right neighbour texel.
@CP@ vec3 atlas_at(vec2 slot, vec2 t) {
    vec2 tt = mod(t, Tile);
    return texture2D(Atlas, sharp_uv((slot * Slot + (Slot - Tile) * 0.5 + tt) / AtlasSize, AtlasSize, AtlasScale)).rgb;
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
    // The relief of the land (not the water: it has its own look). Each code is lit by its own texture.
    if (Strength > 0.0 && code >= 2.5) {
        @CP@ float h = luma(c);
        @CP@ float xp = luma(atlas_at(slot, px + vec2(RELIEF_R, 0.0)));
        @CP@ float xm = luma(atlas_at(slot, px - vec2(RELIEF_R, 0.0)));
        @CP@ float yp = luma(atlas_at(slot, px + vec2(0.0, RELIEF_R)));
        @CP@ float ym = luma(atlas_at(slot, px - vec2(0.0, RELIEF_R)));
        @CP@ float deep = (luma(atlas_at(slot, px + vec2(3.0 * RELIEF_R, 0.0))) + luma(atlas_at(slot, px - vec2(3.0 * RELIEF_R, 0.0))) + luma(atlas_at(slot, px + vec2(0.0, 3.0 * RELIEF_R))) + luma(atlas_at(slot, px - vec2(0.0, 3.0 * RELIEF_R)))) * 0.25;
        c = relief_shade(c, h, xp, xm, yp, ym, deep);
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
            // The flow scroll: two phases half a cycle apart (a 20 s cycle, 7 cells of travel a cycle),
            // each faded out at its own wrap, so the pattern never jumps when Clock wraps.
            vec2 fl = (texture2D(Flow, cell_uv).rg * 255.0 - 128.0) / 127.0;
            float t0 = fract(Clock * 0.05);
            float t1 = fract(Clock * 0.05 + 0.5);
            float wt = abs(2.0 * t0 - 1.0);
            vec2 q0 = p - fl * t0 * 7.0;
            vec2 q1 = p - fl * t1 * 7.0;
            uvw += 0.015 * mix(ripple(q0), ripple(q1), wt);
            float band = smoothstep(0.45, 0.6, sea) * (1.0 - smoothstep(0.6, 0.85, sea));
            band *= 0.6 + 0.4 * sin(Clock * 1.60025 + mix(vnoise(q0 * 1.3), vnoise(q1 * 1.3), wt) * 6.0);
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

/// The relief of the unlit shader (play-build's `sharp::RELIEF_GLSL`), for `LEGACY_FRAGMENT` only. Do not edit it.
const LEGACY_RELIEF: &str = r#"
const float RELIEF_R = 1.5;
const float RELIEF_K = 2.0;
float luma(vec3 c) {
    return dot(c, vec3(0.299, 0.587, 0.114));
}
// `rgb` lit by the slope from the heights `xp`, `xm` (at +x, -x), `yp`, `ym` (at +y, -y) around
// the centre height `h`, and the cavity from `deep`, the mean height at ±3 R.
vec3 relief_shade(vec3 rgb, float h, float xp, float xm, float yp, float ym, float deep) {
    vec3 n = normalize(vec3(-(xp - xm) * RELIEF_K, -(yp - ym) * RELIEF_K, 1.0));
    float f = dot(n, Light) / Light.z;
    float cav = clamp(deep - h, 0.0, 0.25) * 4.0;
    return clamp(rgb * mix(1.0, f, Strength) * (1.0 - Cavity * cav), 0.0, 1.0);
}
"#;

/// The uniforms of `LEGACY_FRAGMENT` (play-build's list, with `Light`, `Strength` and `Cavity`).
const LEGACY_UNIFORMS: [(&str, UniformType); 19] = [
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
    ("Light", UniformType::Float3),
    ("Strength", UniformType::Float1),
    ("Cavity", UniformType::Float1),
];

/// The uniforms every terrain shader declares (the old one leaves `Overlay` out of its code).
const UNIFORMS: [(&str, UniformType); 17] = [
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
    ("Detail", UniformType::Float1),
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
    /// The live water's per-cell texture (`flow_texels`): flow, drift speed and the water mask.
    flow: Texture2D,
    /// Razdor's weather masks (`weather::mask_texels`): snow, swamp rain, heat and ash; the lowland rain.
    weather: Texture2D,
    lowland: Texture2D,
    /// The map's weather seed (`weather::map_seed`).
    weather_seed: u64,
}

/// The weather's masks of a map, from the cache (see [`TerrainLayer::weather_masks`]).
pub struct WeatherMasks {
    /// One texel per cell: red snow, green swamp rain, blue heat, alpha ash (nearest, clamped).
    pub biome: Texture2D,
    /// One texel per cell: red lowland rain (nearest, clamped).
    pub lowland: Texture2D,
    pub seed: u64,
}

/// A map's surface codes, their flow, their textures and their size, kept while they stay the same.
struct CellTextures {
    codes: Vec<u8>,
    flow: Vec<(f32, f32)>,
    size: (u32, u32),
    tex: MapTextures,
}

/// One cell's flow component as a `Flow` texel byte: 128 + 127 · v (so zero is exactly 128).
fn flow_byte(v: f32) -> u8 {
    (128.0 + 127.0 * v.clamp(-1.0, 1.0)).round() as u8
}

/// The live water's per-cell texture as RGBA8 bytes, one texel per cell (row by row), baked once per map
/// (`TerrainLayer::cells`): red and green are the rivers' flow as [`flow_byte`] (still water and land are
/// 128, zero); blue is the river's drift speed (`water::river_speed`, 0 to 255); alpha is the water mask
/// (255 on the water cells, `map::is_water`, 0 on land), linear so the shore is a soft edge. The flow and
/// speed are zero where `flow` is empty (no field: still water everywhere).
pub fn flow_texels(codes: &[u8], flow: &[(f32, f32)], speed: &[f32]) -> Vec<u8> {
    codes
        .iter()
        .enumerate()
        .flat_map(|(i, &c)| {
            let (x, y) = flow.get(i).copied().unwrap_or((0.0, 0.0));
            let s = speed.get(i).copied().unwrap_or(0.0);
            let wet = Surface::from_code(c).is_some_and(is_water);
            [flow_byte(x), flow_byte(y), (s.clamp(0.0, 1.0) * 255.0).round() as u8, if wet { 255 } else { 0 }]
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

/// «Живая вода» as a picture (`draw_water_picture`): two drawings of the water, the newer being `cur`, with
/// their water times and the age of the newer one in frames, and the key of the view they were drawn for.
#[derive(Default)]
struct WaterPictures {
    targets: [Option<RenderTarget>; 2],
    cur: usize,
    size: (u32, u32),
    key: u64,
    valid: bool,
    has_prev: bool,
    age: u32,
    t_prev: f64,
    t_cur: f64,
}

/// The water picture's composite: the two drawings blended by `Mix` (0 the older, 1 the newer), read at each
/// pixel's window position as the ground's composite is. The water's alpha is 1 where it is drawn, else 0.
const WATER_COMPOSITE_FRAGMENT: &str = r#"#version 100
precision highp float;
varying highp vec2 uv;
uniform sampler2D Prev;
uniform sampler2D Cur;
uniform vec2 ScreenSize;
uniform float Mix;
void main() {
    vec2 at = gl_FragCoord.xy / ScreenSize;
    gl_FragColor = mix(texture2D(Prev, at), texture2D(Cur, at), Mix);
}
"#;

/// The blending shader, the terrain atlas and the current map's surface codes.
pub struct TerrainLayer {
    /// The old shader, when option 1 is off.
    old: Option<Material>,
    /// The neighbours shader in highp, and in mediump (option 4), both made at start.
    new_hp: Option<Material>,
    new_mp: Option<Material>,
    /// The unlit shader (`LEGACY_FRAGMENT`) in highp and in mediump, made on the first unlit draw.
    legacy: [std::cell::OnceCell<Option<Material>>; 2],
    /// The cache's copy, when option 2 is on.
    composite: Option<Material>,
    /// The options in effect for this layer (a shader that fails to compile turns its options off).
    opts: Options,
    atlas: Texture2D,
    tile: Vec2,
    slot: Vec2,
    /// The water frames ([`WATER_FRAMES`] or none, then water is still).
    water: Vec<Texture2D>,
    /// The live water's noise (`water::noise_texels`), read by the neighbours shader's `wnoise`.
    noise: Texture2D,
    /// Surface codes and size of the map in `cells`, to rebuild it when the map changes.
    cells: RefCell<Option<CellTextures>>,
    ground: RefCell<GroundCache>,
    /// The live water as a picture (`draw_water_picture`), and its composite (the neighbours shader's option only).
    pictures: RefCell<WaterPictures>,
    water_composite: Option<Material>,
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

/// The water's noise texture (`water::noise_texels`): wrapped and linear, so the shader's lookup is a
/// smooth value noise with the lattice's period.
fn noise_texture() -> Texture2D {
    let n = water::NOISE_SIZE as u16;
    let tex = Texture2D::from_rgba8(n, n, &water::noise_texels().iter().flat_map(|&v| [v, v, v, 255]).collect::<Vec<u8>>());
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

/// A material of `fragment` drawn alpha-blended over what is under it. The default pipeline blends nothing, so the
/// water picture's composite (its transparent pixels show the ground) needs this.
fn make_blended_material(fragment: &str, uniforms: &[(&str, UniformType)], textures: &[&str]) -> Result<Material, String> {
    let pipeline_params = PipelineParams {
        color_blend: Some(BlendState::new(Equation::Add, BlendFactor::Value(BlendValue::SourceAlpha), BlendFactor::OneMinusValue(BlendValue::SourceAlpha))),
        alpha_blend: Some(BlendState::new(Equation::Add, BlendFactor::One, BlendFactor::OneMinusValue(BlendValue::SourceAlpha))),
        ..Default::default()
    };
    let params = MaterialParams {
        uniforms: uniforms.iter().map(|(n, t)| UniformDesc::new(n, *t)).collect(),
        textures: textures.iter().map(|t| t.to_string()).collect(),
        pipeline_params,
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
            let hp_ok = make_material(&new_fragment("highp"), &UNIFORMS, &["Neighbours", "Atlas", "Water", "Flow", "Noise"]);
            match hp_ok {
                Ok(m) => {
                    new_hp = Some(m);
                    // Both precisions are made at start; option 4 picks one.
                    new_mp = make_material(&new_fragment("mediump"), &UNIFORMS, &["Neighbours", "Atlas", "Water", "Flow", "Noise"]).map_err(|e| razdor::diag!("terrain shader (mediump): {e}")).ok();
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
        let water_composite = if opts.new_shader {
            make_blended_material(WATER_COMPOSITE_FRAGMENT, &[("ScreenSize", UniformType::Float2), ("Mix", UniformType::Float1)], &["Prev", "Cur"])
                .map_err(|e| razdor::diag!("terrain water picture: {e}"))
                .ok()
        } else {
            None
        };
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
            legacy: Default::default(),
            composite,
            opts,
            atlas,
            tile: vec2(tw as f32, th as f32),
            slot: vec2((tw + 2 * PAD) as f32, (th + 2 * PAD) as f32),
            water,
            noise: noise_texture(),
            cells: RefCell::new(None),
            ground: RefCell::new(GroundCache::default()),
            pictures: RefCell::new(WaterPictures::default()),
            water_composite,
        })
    }

    /// Whether the ground cache is in use for this layer (option 2).
    pub fn cache_on(&self) -> bool {
        self.opts.cache
    }

    /// The textures of a `w`×`h` map (the codes, the neighbours, the water mask and the flow),
    /// rebuilt only when the codes or the flow change.
    fn cells(&self, codes: &[u8], w: u32, h: u32, flow: &[(f32, f32)]) -> MapTextures {
        let mut cache = self.cells.borrow_mut();
        if let Some(c) = cache.as_ref() {
            if c.size == (w, h) && c.codes == codes && c.flow.as_slice() == flow {
                return c.tex.clone();
            }
        }
        let rgba: Vec<u8> = codes.iter().flat_map(|&c| code_texel(c)).collect();
        let cells = Texture2D::from_rgba8(w as u16, h as u16, &rgba);
        cells.set_filter(FilterMode::Nearest);
        let neighbours = Texture2D::from_rgba8((w + 2) as u16, (h + 2) as u16, &neighbour_texels(codes, w, h));
        neighbours.set_filter(FilterMode::Nearest);
        // The live water's per-cell texture: the flow (zero without the field: still water), the river's
        // drift speed and the water mask, baked once per map (the shader reads one texel per cell).
        let field = if flow.len() == codes.len() { flow } else { &[] };
        let speed = if field.is_empty() { Vec::new() } else { super::water::river_speed(codes, (w, h), field) };
        let flow_tex = Texture2D::from_rgba8(w as u16, h as u16, &flow_texels(codes, field, &speed));
        flow_tex.set_filter(FilterMode::Linear);
        let (biome_bytes, lowland_bytes) = super::weather::mask_texels(codes);
        // Nearest: the shader blends the four cells itself (`weather::mask4`).
        let weather = Texture2D::from_rgba8(w as u16, h as u16, &biome_bytes);
        weather.set_filter(FilterMode::Nearest);
        let lowland = Texture2D::from_rgba8(w as u16, h as u16, &lowland_bytes);
        lowland.set_filter(FilterMode::Nearest);
        let weather_seed = super::weather::map_seed(w, h, codes);
        let tex = MapTextures { cells, neighbours, flow: flow_tex, weather, lowland, weather_seed };
        *cache = Some(CellTextures { codes: codes.to_vec(), flow: flow.to_vec(), size: (w, h), tex: tex.clone() });
        tex
    }

    /// The weather's masks of a `w`×`h` map (kept with the cells: rebuilt only when the codes or the
    /// flow change). `flow` as in [`TerrainLayer::draw`].
    pub fn weather_masks(&self, codes: &[u8], (w, h): (u32, u32), flow: &[(f32, f32)]) -> WeatherMasks {
        let maps = self.cells(codes, w, h, flow);
        WeatherMasks { biome: maps.weather, lowland: maps.lowland, seed: maps.weather_seed }
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
    /// reach past the map then. The fog's clock is `water_ms`. `mode`: see [`Mode`]. `flow`: the
    /// rivers' flow per cell (`rules::flow::flow_field`), empty for still water.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, codes: &[u8], (w, h): (u32, u32), dest: Rect, view: Vec4, cell_px: Vec2, water_ms: Option<i64>, edge: bool, mode: Mode, flow: &[(f32, f32)]) {
        if w == 0 || h == 0 || w + 2 > u16::MAX as u32 || h + 2 > u16::MAX as u32 || codes.len() != (w * h) as usize {
            return;
        }
        if mode == Mode::Cached {
            self.draw_cached(dest);
            return;
        }
        // «Живая вода» at a lower rate or size, on the flat map: the water is drawn as a picture (`draw_water_picture`).
        // The tilted views draw it at once: their camera belongs to world_view, which a picture would reset.
        if mode == Mode::Overlay && water::live() && (water::every() > 1 || water::scale() < 1.0) && !super::world_view::tilt_view() && self.draw_water_picture(codes, (w, h), dest, view, cell_px, water_ms, edge, flow) {
            return;
        }
        self.draw_layer(codes, (w, h), dest, view, cell_px, water_ms, edge, mode, flow, 1.0);
    }

    /// One draw of the layer: the material of the mode, its uniforms and the quad. `res` is the target's size
    /// relative to the screen (1 on the screen, the water picture's scale in its target), so the sampling's
    /// screen pixels per texel are the target's.
    #[allow(clippy::too_many_arguments)]
    fn draw_layer(&self, codes: &[u8], (w, h): (u32, u32), dest: Rect, view: Vec4, cell_px: Vec2, water_ms: Option<i64>, edge: bool, mode: Mode, flow: &[(f32, f32)], res: f32) {
        // The unlit shader while the lighting and the live water are both off (the picture as it was before).
        let unlit = daylight::shown_hour().is_none() && !water::live();
        let Some((m, new, legacy)) = self.ground_material(unlit) else { return };
        if legacy {
            // The unlit shader's relief, off: `Strength` 0 leaves the colour as it is.
            m.set_uniform("Light", vec3(0.0, 0.0, 1.0));
            m.set_uniform("Strength", 0.0f32);
            m.set_uniform("Cavity", 0.0f32);
        }
        // The trailer's stills (dev-only): the atlas is sampled nearest, so the grass is not streaked.
        if super::world_view::trailer_active() {
            self.atlas.set_filter(FilterMode::Nearest);
        }
        let maps = self.cells(codes, w, h, flow);
        gl_use_material(m);
        if new {
            m.set_texture("Neighbours", maps.neighbours.clone());
            m.set_texture("Flow", maps.flow.clone());
            if legacy {
                // The unlit shader still declares its live water's mask (never sampled with the water off):
                // it gets the per-cell texture, whose alpha is the same mask.
                m.set_texture("WaterMask", maps.flow.clone());
            } else {
                m.set_texture("Noise", self.noise.clone());
            }
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
        m.set_uniform("AtlasScale", vec2(res * dest.w / (cells_w * cell_px.x), res * dest.h / (cells_h * cell_px.y)));
        m.set_uniform("WaterScale", vec2(3.0 * res * dest.w / (cells_w * water.width()), 3.0 * res * dest.h / (cells_h * water.height())));
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
        m.set_uniform("Clock", water_ms.map_or(0.0, |ms| (ms as f32 / 1000.0) % CLOCK_PERIOD));
        m.set_uniform("Overlay", if mode == Mode::Overlay { 1.0f32 } else { 0.0 });
        // Live water («Живая вода»): the water's draws only (the full pass and the overlay). The
        // sky's colour is the lighting's (the day's with the lighting off).
        m.set_uniform("LiveWater", if water::live() && water_on { 1.0f32 } else { 0.0 });
        // The water's detail by the zoom: the on-screen cell over the zoom-1 cell (`cell_px`, the map's PX).
        if !legacy {
            m.set_uniform("Detail", water::lod_detail((dest.w / (view.z - view.x)) / cell_px.x));
        }
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

    /// «Живая вода» as a picture (the flat map, a lower redraw rate or size): the overlay's water is drawn into a
    /// target of `scale` × the screen when it is due (`water::redraw_due`); every frame the two last drawings are
    /// blended in time (`water::picture_mix`) over the cached ground. Returns false when the picture cannot be
    /// made (no composite material); the caller then draws the layer at once.
    #[allow(clippy::too_many_arguments)]
    fn draw_water_picture(&self, codes: &[u8], (w, h): (u32, u32), dest: Rect, view: Vec4, cell_px: Vec2, water_ms: Option<i64>, edge: bool, flow: &[(f32, f32)]) -> bool {
        let Some(composite) = self.water_composite.as_ref() else { return false };
        let maps = self.cells(codes, w, h, flow);
        let (sw, sh) = super::render_cache::base_pixels().unwrap_or_else(macroquad::miniquad::window::screen_size);
        let scale = water::scale();
        let size = ((sw * scale).round().max(1.0) as u32, (sh * scale).round().max(1.0) as u32);
        // The key: what the picture was drawn for (the view, the quad, the fog, the target's size, the map).
        let fog = edge_fog_colour();
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            let f = |v: f32| v.to_bits();
            [dest.x, dest.y, dest.w, dest.h, view.x, view.y, view.z, view.w, cell_px.x, cell_px.y, scale, fog[0], fog[1], fog[2]].map(f).hash(&mut h);
            (edge, size, maps.weather_seed, flow.len()).hash(&mut h);
            h.finish()
        };
        {
            let mut st = self.pictures.borrow_mut();
            if st.size != size {
                st.targets = [None, None];
                st.size = size;
                st.valid = false;
            }
            if st.key != key {
                st.key = key;
                st.valid = false;
            }
        }
        let now = water_ms.map_or(0.0, |ms| ms as f64 / 1000.0);
        let (due, idx, was_valid) = {
            let st = self.pictures.borrow();
            (!st.valid || water::redraw_due(st.age, water::every()), if st.valid { 1 - st.cur } else { st.cur }, st.valid)
        };
        if due {
            // A redraw goes into the target the older picture does not use (the same one when the view changed).
            let target = {
                let mut st = self.pictures.borrow_mut();
                st.targets[idx]
                    .get_or_insert_with(|| {
                        let rt = macroquad::texture::render_target(size.0, size.1);
                        rt.texture.set_filter(FilterMode::Linear);
                        rt
                    })
                    .clone()
            };
            super::render_cache::fill_space(&target, Rect::new(0.0, 0.0, screen_width(), screen_height()), || {
                clear_background(Color::new(0.0, 0.0, 0.0, 0.0));
                self.draw_layer(codes, (w, h), dest, view, cell_px, water_ms, edge, Mode::Overlay, flow, scale);
            });
            let mut st = self.pictures.borrow_mut();
            if was_valid {
                st.t_prev = st.t_cur;
                st.has_prev = true;
            } else {
                st.has_prev = false;
            }
            st.cur = idx;
            st.t_cur = now;
            st.valid = true;
            st.age = 0;
        } else {
            self.pictures.borrow_mut().age += 1;
        }
        let st = self.pictures.borrow();
        let prev_idx = if st.has_prev { 1 - st.cur } else { st.cur };
        let mix = water::picture_mix(now, st.has_prev.then_some(st.t_prev), st.t_cur);
        let (Some(prev), Some(cur)) = (st.targets[prev_idx].clone(), st.targets[st.cur].clone()) else { return false };
        drop(st);
        // The composite reads each picture at its fragment's place, as the ground's composite does.
        gl_use_material(composite);
        composite.set_texture("Prev", prev.texture.clone());
        composite.set_texture("Cur", cur.texture.clone());
        composite.set_uniform("ScreenSize", vec2(sw, sh));
        composite.set_uniform("Mix", mix);
        draw_texture_ex(&cur.texture, dest.x, dest.y, WHITE, DrawTextureParams { dest_size: Some(dest.size()), ..Default::default() });
        gl_use_default_material();
        true
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

    /// The unlit shader at the current precision (None when it does not compile).
    fn legacy_material(&self) -> Option<&Material> {
        let mediump = self.opts.mediump;
        self.legacy[usize::from(mediump)]
            .get_or_init(|| {
                let precision = if mediump { "mediump" } else { "highp" };
                let text = LEGACY_FRAGMENT.replace("@CP@", precision).replace("@SHARP_UV@", sharp::SHARP_UV_GLSL).replace("@RELIEF@", LEGACY_RELIEF);
                make_material(&text, &LEGACY_UNIFORMS, &["Neighbours", "Atlas", "Water", "WaterMask", "Flow"])
                    .map_err(|e| razdor::diag!("terrain unlit shader ({precision}): {e}"))
                    .ok()
            })
            .as_ref()
    }

    /// The material of the ground draws: the unlit shader when `unlit` (and it compiles), else the neighbours
    /// shader. The flag says whether it is the neighbours shader (its textures) and whether it is the unlit one.
    fn ground_material(&self, unlit: bool) -> Option<(&Material, bool, bool)> {
        if self.opts.new_shader {
            if unlit {
                if let Some(m) = self.legacy_material() {
                    return Some((m, true, true));
                }
            }
            let m = if self.opts.mediump { self.new_mp.as_ref().or(self.new_hp.as_ref()) } else { self.new_hp.as_ref() };
            m.map(|m| (m, true, false))
        } else {
            self.old.as_ref().map(|m| (m, false, false))
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
    fn the_flow_texels_keep_still_water_at_128_and_the_ends_at_the_bytes_0_and_255() {
        // Codes 0 (water), 5 (land): the flow in rg (still 128), the speed in b, the water mask in a.
        let codes = [0u8, 5];
        // Water at still: [128, 128, 0 speed, 255 mask]; land with the flow (1, -1) at full speed: [255, 1, 255, 0].
        assert_eq!(flow_texels(&codes, &[(0.0, 0.0), (1.0, -1.0)], &[0.0, 1.0]), vec![128, 128, 0, 255, 255, 1, 255, 0]);
        // The shader's decode of a byte b is (b · 255 - 128) / 127 for b / 255: zero is exact.
        let back = |b: u8| (b as f32 / 255.0 * 255.0 - 128.0) / 127.0;
        assert_eq!(back(128), 0.0);
        assert!((back(255) - 1.0).abs() < 1e-6 && (back(1) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_flow_texel_mask_is_255_on_the_water_codes_only() {
        // Codes 0-2 are water (shallows and fords, coastal water, deep sea); 3 (lava) and 5 are land.
        let texels = flow_texels(&[0, 3, 2, 5], &[], &[]);
        let alpha: Vec<u8> = texels.chunks(4).map(|t| t[3]).collect();
        assert_eq!(alpha, vec![255, 0, 255, 0]);
        // Still water without the field: flow 128, speed 0.
        assert_eq!(&texels[0..3], &[128, 128, 0]);
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

#[cfg(test)]
mod clock_wrap_tests {
    use super::CLOCK_PERIOD;

    /// The shaders' sine frequencies (rad/s): each a whole number of cycles in `CLOCK_PERIOD`.
    #[test]
    fn every_sine_has_a_whole_number_of_cycles_in_the_clock_period() {
        for w in [1.29983f32, 1.09956, 1.60025] {
            let cycles = w * CLOCK_PERIOD / std::f32::consts::TAU;
            assert!((cycles - cycles.round()).abs() < 0.01, "{w} rad/s: {cycles} cycles");
        }
    }

    /// The shaders' drifts (noise units per second, on the 64-cell lattice): whole periods per wrap.
    #[test]
    fn every_drift_moves_the_noise_a_whole_number_of_lattice_periods_per_wrap() {
        for v in [0.05f32, 0.02, -0.08, 0.2, -0.03, 0.06, 0.07, -0.04, -0.09, -0.05] {
            let periods = v * CLOCK_PERIOD / 64.0;
            assert!((periods - periods.round()).abs() < 0.01, "drift {v}: {periods} periods");
        }
    }
}
