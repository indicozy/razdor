//! Blended terrain: the original draws its ground with soft transitions between surfaces
//! (the gameplay video: grass fades into fields, roads and sand over about one cell), not
//! square cells. Here one shader draws the visible map in a single quad: every pixel mixes
//! the textures of the four nearest cell centres by bilinear weights. Texture space is world
//! space, so each texture tiles seamlessly across the map.
//!
//! The 16 terrain textures sit in one atlas (4×4 slots, each padded with a wrapped border so
//! linear filtering never bleeds between slots); the map's surface codes are a small texture
//! with one texel per cell.
//!
//! Water (codes 0–2) is animated as the original's (0x4c8864): a second texture stage holds
//! water frame `(t div 100) mod 32` (0x4c8db2) and is multiplied into the ground twice over
//! (D3D MODULATE2X), with its own coordinates of ⅓ texture per cell, so a 64×64 frame spans
//! 3×3 cells. The original applies it on the water cells' own quads only, not on the soft
//! edges a water cell lays over its neighbours; here it goes with each water code's share of
//! the blend.

use std::cell::RefCell;

use macroquad::prelude::*;

use macroquad::miniquad::TextureWrap;
use razdor::dt::gfx::{water_frame, Image, WATER_FRAMES};
use super::sharp;

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

// `// @SHARP_UV@` is replaced by `sharp::SHARP_UV_GLSL` when the material is made.
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
// else 0.0; yzw: the fog's haze colour there. Ramp: the cells where the ground stops (x) and the
// fog is opaque (y), counted from the border. Clock: seconds, animates the fog.
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
        // Past the border (p in cell units, the border at -0.5 and MapSize - 0.5). The fog's
        // opacity rises from the strip's end to fully opaque at Ramp.y, with the two noise layers
        // (different scales, drifting in different directions) breaking it up; past Ramp.y it is
        // opaque. No line anywhere: the fog starts where the ground's colour ends.
        vec2 gap = max(max(vec2(-0.5) - p, p - (MapSize - 0.5)), vec2(0.0));
        float d = length(gap);
        // Inside the strip the ground stays as it is: the noise is only computed past it.
        if (d > Ramp.x) {
            float n = 0.6 * vnoise(p * 0.11 + Clock * vec2(0.05, 0.02)) + 0.4 * vnoise(p * 0.27 + Clock * vec2(-0.08, 0.05));
            float o = smoothstep(Ramp.x, Ramp.y, d) * (0.75 + 0.5 * n);
            o = mix(o, 1.0, step(Ramp.y, d));
            vec3 haze = Edge.yzw * (0.85 + 0.3 * n);
            col = mix(col, haze, clamp(o, 0.0, 1.0));
        }
    }
    gl_FragColor = vec4(col, 1.0);
}
"#;

/// Wrapped border around each atlas slot, in pixels.
const PAD: u32 = 2;

/// The ground past a map's edge goes on this many cells (its border's own row or column, continued
/// outward), then the fog begins (`fog_opacity`).
pub const EDGE_STRIP: f32 = 3.0;
/// The fog is fully opaque this many cells past the border (the fog's ramp ends here).
pub const EDGE_FULL: f32 = 7.0;
/// The fog's haze: grey-blue, between the map's ground and black (the fog of war is drawn over it).
pub const EDGE_HAZE: [f32; 3] = [0.36, 0.42, 0.49];

/// The cell the ground shows at cell `c` of a map `n` cells long: the border's own cell, clamped
/// (the shader's `clamp`). Past the border it continues the edge's row or column.
#[allow(dead_code)] // The shader's formula in Rust, pinned by the tests (`the_extension_...`, `the_fog_...`).
pub fn edge_cell(c: i32, n: i32) -> i32 {
    if n <= 0 {
        0
    } else {
        c.clamp(0, n - 1)
    }
}

/// The fog's opacity at `d` cells past the border: 0 through the strip, a smooth rise (the
/// shader's `smoothstep`) to 1 at `EDGE_FULL`, and 1 beyond.
#[allow(dead_code)] // The shader's formula in Rust, pinned by the tests (`the_extension_...`, `the_fog_...`).
pub fn fog_opacity(d: f32) -> f32 {
    let t = ((d - EDGE_STRIP) / (EDGE_FULL - EDGE_STRIP)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A map's surface codes, its size and their texture.
type CellCache = Option<(Vec<u8>, (u32, u32), Texture2D)>;

/// The blending shader, the terrain atlas and the current map's surface codes.
pub struct TerrainLayer {
    material: Material,
    atlas: Texture2D,
    tile: Vec2,
    slot: Vec2,
    /// The water frames ([`WATER_FRAMES`] or none, then water is still).
    water: Vec<Texture2D>,
    /// Surface codes and size of the map in `cells`, to rebuild it when the map changes.
    cells: RefCell<CellCache>,
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

/// The cell texture's texel for a surface code (red = code / 15).
fn code_texel(code: u8) -> [u8; 4] {
    [(code & 15) * 17, 0, 0, 255]
}

impl TerrainLayer {
    /// A layer over the given textures, or `None` without any texture or if the shader
    /// does not compile (callers then draw square cells). `water` are the animated water
    /// frames, uploaded once; without all [`WATER_FRAMES`] of them water stays still.
    pub fn new(textures: &[Option<Image>; 16], fill: &[[u8; 4]; 16], water: &[Image]) -> Option<TerrainLayer> {
        let (image, (tw, th)) = pack_terrain_atlas(textures, fill)?;
        let material = load_material(
            ShaderSource::Glsl { vertex: VERTEX, fragment: &FRAGMENT.replace("// @SHARP_UV@", sharp::SHARP_UV_GLSL) },
            MaterialParams {
                uniforms: ["View", "MapSize", "CellPx", "Tile", "Slot", "AtlasSize", "WaterOn", "AtlasScale", "WaterSize", "WaterScale", "Edge", "Ramp", "Clock"]
                    .iter()
                    .zip([
                        UniformType::Float4,
                        UniformType::Float2,
                        UniformType::Float2,
                        UniformType::Float2,
                        UniformType::Float2,
                        UniformType::Float2,
                        UniformType::Float1,
                        UniformType::Float2,
                        UniformType::Float2,
                        UniformType::Float2,
                        UniformType::Float4,
                        UniformType::Float2,
                        UniformType::Float1,
                    ])
                    .map(|(n, t)| UniformDesc::new(n, t))
                    .collect(),
                textures: vec!["Cells".into(), "Atlas".into(), "Water".into()],
                ..Default::default()
            },
        )
        .map_err(|e| razdor::diag!("terrain shader: {e}"))
        .ok()?;
        let atlas = Texture2D::from_rgba8(image.width as u16, image.height as u16, &image.rgba);
        atlas.set_filter(FilterMode::Linear);
        let water = if water.len() == WATER_FRAMES && water.iter().all(|f| f.width > 0 && f.height > 0) {
            water.iter().map(water_texture).collect()
        } else {
            Vec::new()
        };
        Some(TerrainLayer {
            material,
            atlas,
            tile: vec2(tw as f32, th as f32),
            slot: vec2((tw + 2 * PAD) as f32, (th + 2 * PAD) as f32),
            water,
            cells: RefCell::new(None),
        })
    }

    /// The surface-code texture of a `w`×`h` map, rebuilt only when the codes change.
    fn cells(&self, codes: &[u8], w: u32, h: u32) -> Texture2D {
        let mut cache = self.cells.borrow_mut();
        if let Some((c, size, tex)) = cache.as_ref() {
            if *size == (w, h) && c == codes {
                return tex.clone();
            }
        }
        let rgba: Vec<u8> = codes.iter().flat_map(|&c| code_texel(c)).collect();
        let tex = Texture2D::from_rgba8(w as u16, h as u16, &rgba);
        tex.set_filter(FilterMode::Nearest);
        *cache = Some((codes.to_vec(), (w, h), tex.clone()));
        tex
    }

    /// Draws the terrain of a `w`×`h` map (`codes` row by row) into screen rectangle `dest`,
    /// which shows the map area `view` in cells (x0, y0, x1, y1; cell centres at integers).
    /// `cell_px` is the texture pixels per cell (the textures' scale). `water_ms` is the clock
    /// (ms) that animates the water, `None` for still water. `edge`: the ground goes on past the
    /// map's edges for [`EDGE_STRIP`] cells, then the animated fog ([`fog_opacity`]); `view` may
    /// reach past the map then. The fog's clock is `water_ms`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, codes: &[u8], (w, h): (u32, u32), dest: Rect, view: Vec4, cell_px: Vec2, water_ms: Option<i64>, edge: bool) {
        if w == 0 || h == 0 || w > u16::MAX as u32 || h > u16::MAX as u32 || codes.len() != (w * h) as usize {
            return;
        }
        let cells = self.cells(codes, w, h);
        let m = &self.material;
        gl_use_material(m);
        m.set_texture("Cells", cells.clone());
        m.set_texture("Atlas", self.atlas.clone());
        // Without frames the stage samples the atlas and is switched off.
        let water = water_ms.filter(|_| !self.water.is_empty()).map(|ms| self.water[water_frame(ms)].clone());
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
        let haze = EDGE_HAZE;
        m.set_uniform("Edge", vec4(if edge { 1.0 } else { 0.0 }, haze[0], haze[1], haze[2]));
        m.set_uniform("Ramp", vec2(EDGE_STRIP, EDGE_FULL));
        m.set_uniform("Clock", water_ms.map_or(0.0, |ms| (ms as f32 / 1000.0) % 1000.0));
        draw_texture_ex(&cells, dest.x, dest.y, WHITE, DrawTextureParams { dest_size: Some(dest.size()), ..Default::default() });
        gl_use_default_material();
    }
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
    fn the_fog_is_clear_through_the_strip_and_opaque_from_the_full_distance() {
        assert!(EDGE_STRIP < EDGE_FULL && EDGE_STRIP >= 1.0);
        assert_eq!(fog_opacity(0.0), 0.0, "on the border");
        assert_eq!(fog_opacity(EDGE_STRIP), 0.0, "the strip's end");
        assert_eq!(fog_opacity(EDGE_FULL), 1.0, "fully opaque at the full distance");
        assert_eq!(fog_opacity(EDGE_FULL + 5.0), 1.0, "and beyond");
        // Non-decreasing in between, no step: the ramp has no hard line.
        let mut last = 0.0;
        for i in 0..=400 {
            let d = i as f32 / 40.0;
            let o = fog_opacity(d);
            assert!((0.0..=1.0).contains(&o) && o >= last - 1e-6, "{d}: {o}");
            last = o;
        }
        // Halfway up the ramp is half opaque (the smoothstep's midpoint).
        assert!((fog_opacity((EDGE_STRIP + EDGE_FULL) / 2.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_haze_is_grey_blue_and_not_black() {
        let [r, g, b] = EDGE_HAZE;
        assert!(r.min(g).min(b) > 0.2, "not black: {EDGE_HAZE:?}");
        assert!(b > r && g > r, "blue-grey, not warm: {EDGE_HAZE:?}");
        assert!(r.max(g).max(b) < 0.6, "a haze, not a glare: {EDGE_HAZE:?}");
    }

    #[test]
    fn code_texels_round_trip_through_the_shader_formula() {
        for code in 0..16u8 {
            let r = code_texel(code)[0] as f32 / 255.0;
            assert_eq!((r * 15.0 + 0.5).floor() as u8, code);
        }
    }
}
