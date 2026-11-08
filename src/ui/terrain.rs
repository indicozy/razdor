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
// else 0.0; yzw: the sky colour the ground sinks into there. Fade: the cells over which it sinks.
uniform vec4 Edge;
uniform float Fade;
// @SHARP_UV@

// The cell index c folded back about the map's border (mirror tiling: the cell past the edge
// is the edge's neighbour's twin), period 2·n. Cells inside the map are their own.
float fold(float c, float n) {
    float m = mod(c, 2.0 * n);
    return m < n ? m : 2.0 * n - 1.0 - m;
}

float code_at(vec2 c) {
    c = Edge.x > 0.5 ? vec2(fold(c.x, MapSize.x), fold(c.y, MapSize.y)) : clamp(c, vec2(0.0), MapSize - 1.0);
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
        // Past the border (p in cell units, the border at -0.5 and MapSize - 0.5): the ground
        // greys a little from the edge on, and sinks into the sky over Fade cells, so the
        // extension reads as not playable.
        vec2 gap = max(max(vec2(-0.5) - p, p - (MapSize - 0.5)), vec2(0.0));
        float d = length(gap);
        float grey = dot(col, vec3(0.299, 0.587, 0.114));
        col = mix(col, vec3(grey), 0.35 * clamp(d * 2.0, 0.0, 1.0));
        col = mix(col, Edge.yzw, smoothstep(0.0, Fade, d));
    }
    gl_FragColor = vec4(col, 1.0);
}
"#;

/// Wrapped border around each atlas slot, in pixels.
const PAD: u32 = 2;

/// Cells over which the ground past a map's edge sinks into the sky (with «Продолжать карту за
/// краем», `world_view`).
pub const EDGE_FADE: f32 = 14.0;
/// The sky the ground sinks into, as the map's shader gets it (the sky of `world_view`).
pub const EDGE_SKY: [f32; 3] = [0.06, 0.08, 0.11];

/// The cell the mirrored ground shows at cell `c` of a map `n` cells long (the shader's
/// `fold`): cells inside are their own, and past the border `c` is reflected back about it, so
/// `-1` shows cell 0 and `n` shows cell `n - 1`. Period `2n`.
pub fn mirror_cell(c: i32, n: i32) -> i32 {
    if n <= 0 {
        return 0;
    }
    let m = c.rem_euclid(2 * n);
    if m < n {
        m
    } else {
        2 * n - 1 - m
    }
}

/// How far cell `c` lies past a map `n` cells long, in cells from its border (the cell centres
/// are half a cell from it): 0 inside, 0.5 for `-1` and `n`.
pub fn edge_gap(c: i32, n: i32) -> f32 {
    if c < 0 {
        -0.5 - c as f32
    } else if c >= n {
        c as f32 - n as f32 + 0.5
    } else {
        0.0
    }
}

/// The cells past a map `n` cells long that show cell `c` (inside it) in the mirrored ground,
/// and lie within `fade` cells of its border: the reflections of `c` about either border, and
/// those of the reflections, as far as `fade` reaches.
pub fn mirror_images(c: i32, n: i32, fade: f32) -> Vec<i32> {
    let mut out = Vec::new();
    if n <= 0 {
        return out;
    }
    let reach = (fade / (2 * n) as f32).ceil() as i32 + 2;
    for k in -reach..=reach {
        for cand in [c + 2 * n * k, -1 - c + 2 * n * k] {
            if (cand < 0 || cand >= n) && edge_gap(cand, n) < fade && mirror_cell(cand, n) == c {
                out.push(cand);
            }
        }
    }
    out
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
                uniforms: ["View", "MapSize", "CellPx", "Tile", "Slot", "AtlasSize", "WaterOn", "AtlasScale", "WaterSize", "WaterScale", "Edge", "Fade"]
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
    /// (ms) that animates the water, `None` for still water. `mirror`: the ground goes on past
    /// the map's edges, mirrored and sinking into the sky ([`mirror_cell`]); `view` may reach
    /// past the map then.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, codes: &[u8], (w, h): (u32, u32), dest: Rect, view: Vec4, cell_px: Vec2, water_ms: Option<i64>, mirror: bool) {
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
        let sky = EDGE_SKY;
        m.set_uniform("Edge", vec4(if mirror { 1.0 } else { 0.0 }, sky[0], sky[1], sky[2]));
        m.set_uniform("Fade", EDGE_FADE);
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
    fn the_mirror_reflects_about_the_border_and_repeats() {
        // A 5-cell map: 0..4 are their own, -1 shows 0, -2 shows 1, 5 shows 4, 6 shows 3.
        let inside: Vec<i32> = (0..5).map(|c| mirror_cell(c, 5)).collect();
        assert_eq!(inside, [0, 1, 2, 3, 4]);
        assert_eq!([-1, -2, -5, -6].map(|c| mirror_cell(c, 5)), [0, 1, 4, 4]);
        assert_eq!([5, 6, 9, 10].map(|c| mirror_cell(c, 5)), [4, 3, 0, 0]);
        // Cells past both borders at once (a corner) fold the same way on each axis.
        assert_eq!(mirror_cell(-3, 5), 2);
        assert_eq!(mirror_cell(12, 5), 2);
        // A one-cell map is one cell everywhere.
        assert!((-7..9).all(|c| mirror_cell(c, 1) == 0));
        assert_eq!(mirror_cell(3, 0), 0, "an empty map shows nothing");
    }

    #[test]
    fn the_mirror_gap_is_half_a_cell_at_the_border() {
        assert_eq!(edge_gap(0, 5), 0.0);
        assert_eq!(edge_gap(4, 5), 0.0);
        assert_eq!(edge_gap(-1, 5), 0.5);
        assert_eq!(edge_gap(5, 5), 0.5);
        assert_eq!(edge_gap(-3, 5), 2.5);
        assert_eq!(edge_gap(7, 5), 2.5);
    }

    #[test]
    fn the_mirror_folds_both_axes_and_every_corner_alike() {
        // A 5 × 4 map: the shader folds x with the width and y with the height, so each corner's
        // outside cell shows the inside cell at its own corner, on both axes.
        let (w, h) = (5, 4);
        let fold = |x: i32, y: i32| (mirror_cell(x, w), mirror_cell(y, h));
        // The north-west, north-east, south-west and south-east corners' outside cells.
        assert_eq!(fold(-1, -1), (0, 0));
        assert_eq!(fold(w, -1), (w - 1, 0));
        assert_eq!(fold(-1, h), (0, h - 1));
        assert_eq!(fold(w, h), (w - 1, h - 1));
        // The middle of each side folds across its own border only.
        assert_eq!(fold(2, -1), (2, 0));
        assert_eq!(fold(2, h), (2, h - 1));
        assert_eq!(fold(-1, 1), (0, 1));
        assert_eq!(fold(w, 1), (w - 1, 1));
        // The same on both axes: a cell's east copy equals its west copy's mirror, row by row.
        for y in -3..h + 3 {
            for x in -3..w + 3 {
                let (fx, fy) = fold(x, y);
                assert!((0..w).contains(&fx) && (0..h).contains(&fy), "({x}, {y}) shows ({fx}, {fy})");
            }
        }
        // Reflection about the west border: the cell past it at distance d shows cell d (inside).
        for d in 0..w {
            assert_eq!(mirror_cell(-1 - d, w), d, "west reflection at {d}");
            assert_eq!(mirror_cell(w + d, w), w - 1 - d, "east reflection at {d}");
        }
        // The fade is symmetric on both axes: the same distance past either border.
        for d in 0..6 {
            assert_eq!(edge_gap(-1 - d, w), edge_gap(w + d, w), "x at {d}");
            assert_eq!(edge_gap(-1 - d, h), edge_gap(h + d, h), "y at {d}");
        }
    }

    #[test]
    fn the_mirror_images_are_the_outside_cells_showing_a_cell() {
        // Cell 0 of a 5-cell map is shown by -1 (its reflection about the west border), by 9 (about
        // the east one), and by -10 and 10 (the period 2n), and -11 (the reflection of -10). Within
        // 14 cells of the map only: -21 and 19 are too far.
        let mut near = mirror_images(0, 5, 14.0);
        near.sort();
        assert_eq!(near, [-11, -10, -1, 9, 10]);
        for c in 0..5 {
            for cand in mirror_images(c, 5, 14.0) {
                assert_eq!(mirror_cell(cand, 5), c, "{cand} shows {c}");
                assert!(cand < 0 || cand >= 5, "{cand} is outside");
                assert!(edge_gap(cand, 5) < 14.0);
            }
        }
        // A fade that reaches no cell past the border has no images; one that reaches the
        // half cell has the border's reflection only.
        assert!(mirror_images(2, 5, 1.0).is_empty());
        assert_eq!(mirror_images(4, 5, 1.0), [5]);
    }

    #[test]
    fn code_texels_round_trip_through_the_shader_formula() {
        for code in 0..16u8 {
            let r = code_texel(code)[0] as f32 / 255.0;
            assert_eq!((r * 15.0 + 0.5).floor() as u8, code);
        }
    }
}
