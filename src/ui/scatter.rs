//! Razdor's «Трава и кусты» (`Settings::map_scatter`, off by default) and «Тени у подножия»
//! (`Settings::map_contact_shadows`, off by default). Style only: no rule, path, fog or save reads them.
//!
//! Scatter: small procedural vegetation on the ground of the 2.5D views (grass tufts and low shrubs on
//! grass, pebbles on dirt and sand, reeds on swamp). Nothing is drawn from the install's art: every shape is
//! made in the fragment shader from a few constants. The placement is a seeded hash per cell, so one map
//! always gets the same layout. The quads are baked once per map into one mesh per 8×8-cell chunk (GLES2 has
//! no instancing); a frame only picks the visible chunks and draws each in one call. The vertex shader does
//! the camera facing, the sway from the shared wind (`living::wind`) and the fade when the map is zoomed out.
//! The sway follows the map animation's level (`map_anim::level`): still under «Анимация карты» Выкл, and still
//! when the animations are off at this zoom. Wheat is not drawn: the install has no reliable field marker
//! (the yellow ground is an ordinary plain, the section-0 yellow pictures are hills).
//!
//! Contact shadows: a soft dark ellipse on the ground under each drawn tree, rock, hill, building and army,
//! as one mesh per frame over the ground (2.5D only). Subtle, and weaker where the sun's shadows already lie.
//!
//! Env overrides (snapshots): `RAZDOR_SCATTER=off|few|many`, `RAZDOR_CONTACT_SHADOWS=1`.

use macroquad::miniquad::PipelineParams;
use macroquad::prelude::*;
use razdor::rules::map::{Surface, TileMap};
use razdor::local;
use std::cell::RefCell;

use super::living;

/// The chunk's side in cells.
pub const CHUNK: i32 = 8;

/// The setting «Трава и кусты».
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Density {
    #[default]
    Off,
    Few,
    Many,
}

impl Density {
    /// The next setting in the row: Нет, Мало, Много, Нет.
    pub fn next(self) -> Density {
        match self {
            Density::Off => Density::Few,
            Density::Few => Density::Many,
            Density::Many => Density::Off,
        }
    }

    /// The setting from its env name (`RAZDOR_SCATTER`); anything else is `None`.
    pub fn parse(s: &str) -> Option<Density> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Density::Off),
            "few" => Some(Density::Few),
            "many" => Some(Density::Many),
            _ => None,
        }
    }

    /// Candidate tufts per grass cell and the chance each one stands (Few 2×0.3, Many 3×0.85).
    fn tufts(self) -> (u64, f32) {
        match self {
            Density::Off => (0, 0.0),
            Density::Few => (2, 0.3),
            Density::Many => (3, 0.85),
        }
    }

    /// The chance of a shrub on a grass cell.
    fn shrub(self) -> f32 {
        match self {
            Density::Off => 0.0,
            Density::Few => 0.03,
            Density::Many => 0.12,
        }
    }

    /// The chance of a pebble on a dirt or sand cell.
    fn pebble(self) -> f32 {
        match self {
            Density::Off => 0.0,
            Density::Few => 0.05,
            Density::Many => 0.15,
        }
    }

    /// The chance of a reed on a swamp cell (two candidates).
    fn reed(self) -> f32 {
        match self {
            Density::Off => 0.0,
            Density::Few => 0.2,
            Density::Many => 0.6,
        }
    }
}

local! {
    /// The setting, taken in every frame (`set_prefs`): 0 off, 1 few, 2 many.
    static DENSITY: std::cell::Cell<Density> = const { std::cell::Cell::new(Density::Off) };
    /// `RAZDOR_SCATTER` overrides the setting (read once).
    static ENV_DENSITY: Option<Density> = std::env::var("RAZDOR_SCATTER").ok().and_then(|v| Density::parse(&v));
    /// `RAZDOR_CONTACT_SHADOWS=1` turns the contact shadows on whatever the setting says (read once).
    static ENV_CONTACT: bool = std::env::var("RAZDOR_CONTACT_SHADOWS").is_ok_and(|v| v.trim() == "1");
    static CONTACT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The baked chunks of the map in view, with the key they were baked for.
    static CACHE: RefCell<Option<Baked>> = const { RefCell::new(None) };
    /// The scatter's material, made once (`None` when the shader does not compile).
    static MATERIAL: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
}

/// Takes the settings in (every frame, from `display::follow_settings`).
pub fn set_prefs(density: Density, contact: bool) {
    DENSITY.set(ENV_DENSITY.with(|e| (*e).unwrap_or(density)));
    CONTACT.set(contact || ENV_CONTACT.with(|e| *e));
}

/// The scatter setting in effect.
pub fn density() -> Density {
    DENSITY.get()
}

/// Whether the contact shadows are on.
pub fn contact_on() -> bool {
    CONTACT.get()
}

// ----- Placement (pure) -----

/// What a cell of the map gets: the kinds of its ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ground {
    None,
    Grass,
    Dirt,
    Swamp,
}

/// The ground a surface code takes its vegetation from. Water, roads, lava, snow and ice take nothing;
/// the yellow plain is ordinary ground and gets grass like the rest.
pub fn ground_of(code: u8) -> Ground {
    match Surface::from_code(code) {
        Some(Surface::GrassLowland | Surface::GrassPlain | Surface::DryPlain) => Ground::Grass,
        Some(Surface::SandDunes | Surface::ClaySoil | Surface::StonySoil) => Ground::Dirt,
        Some(Surface::Marsh | Surface::ImpassableSwamp) => Ground::Swamp,
        _ => Ground::None,
    }
}

/// The kinds of quads a cell gets. The kind is the shader's switch.
pub const KIND_TUFT: f32 = 0.0;
pub const KIND_SHRUB: f32 = 1.0;
pub const KIND_PEBBLE: f32 = 2.0;
pub const KIND_REED: f32 = 3.0;

/// One quad of the scatter: its ground point (world units), its kind, its size in world units (width and
/// height), its phase and its tint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    pub at: Vec2,
    pub kind: f32,
    pub size: Vec2,
    pub phase: f32,
    pub tint: f32,
}

/// Smooth value noise in [0, 1) at the cell `(x, y)` on lattice `scale` cells: the patch map that groups the tufts
/// into clumps with empty ground between (`living::unit` at the lattice corners, smoothstep-blended).
pub fn patch_noise(seed: u64, x: f32, y: f32, scale: f32) -> f32 {
    let (fx, fy) = (x / scale, y / scale);
    let (ix, iy) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - ix, fy - iy);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let corner = |dx: f32, dy: f32| {
        let k = ((ix + dx) as i64 as u64).wrapping_mul(0x9E37_79B9) ^ ((iy + dy) as i64 as u64).wrapping_mul(0x85EB_CA6B);
        living::unit(seed ^ 0xA5A5, k)
    };
    let top = corner(0.0, 0.0) + (corner(1.0, 0.0) - corner(0.0, 0.0)) * sx;
    let bottom = corner(0.0, 1.0) + (corner(1.0, 1.0) - corner(0.0, 1.0)) * sx;
    top + (bottom - top) * sy
}

/// The quads of the cell `(x, y)` (its centre at `centre`, world units) on ground `ground`, at `density`, from
/// `seed`. `edge` is whether an object stands next to the cell (a forest's edge: more shrubs there). Every value
/// is a hash of the seed and the cell, so the same map always gets the same quads. A tuft is a cluster of blades
/// (the fragment shader draws 5 to 9 of them from the quad's phase), its size ±40%, in clumps: the patch map
/// decides where the ground is covered and where it is bare.
pub fn cell_quads(seed: u64, (x, y): (i32, i32), centre: Vec2, ground: Ground, density: Density, edge: bool) -> Vec<Quad> {
    let mut out = Vec::new();
    if density == Density::Off {
        return out;
    }
    let cell = seed ^ living::mix(((x as u64) << 32) ^ (y as u64 & 0xffff_ffff));
    let u = |n: u64| living::unit(cell, n);
    let at = |k: u64| centre + vec2((u(k) - 0.5) * 0.7, (u(k + 1) - 0.5) * 0.7);
    // ±40% on the size, from one draw.
    let grow = |k: u64| 0.6 + 0.8 * u(k);
    match ground {
        Ground::None => {}
        Ground::Grass => {
            // The clump map: 0 in the bare gaps, 1 in the thick of a patch.
            let clump = patch_noise(seed, centre.x, centre.y, 3.5);
            let cover = ((clump - 0.35) / 0.3).clamp(0.0, 1.0);
            let cover = cover * cover * (3.0 - 2.0 * cover);
            let (n, p) = density.tufts();
            for k in 0..n {
                let b = 10 + 8 * k;
                if u(b + 2) < p * cover {
                    let g = grow(b + 6);
                    out.push(Quad { at: at(b), kind: KIND_TUFT, size: vec2(0.6 * g, 0.42 * g), phase: u(b + 4), tint: u(b + 5) });
                }
            }
            let shrub = density.shrub() * if edge { 4.0 } else { 1.0 };
            if u(40) < shrub {
                let g = grow(42);
                out.push(Quad { at: at(41), kind: KIND_SHRUB, size: vec2(0.7 * g, 0.6 * g), phase: u(44), tint: u(45) });
            }
        }
        Ground::Dirt => {
            if u(50) < density.pebble() {
                let g = grow(56);
                out.push(Quad { at: at(51), kind: KIND_PEBBLE, size: vec2(0.24 * g, 0.16 * g), phase: u(53), tint: u(54) });
            }
        }
        Ground::Swamp => {
            for k in 0..2u64 {
                let b = 60 + 8 * k;
                if u(b + 2) < density.reed() {
                    let g = grow(b + 6);
                    out.push(Quad { at: at(b), kind: KIND_REED, size: vec2(0.12, 0.55 * g), phase: u(b + 4), tint: u(b + 5) });
                }
            }
        }
    }
    out
}

/// The bytes of one vertex (for the report).
pub const VERTEX_BYTES: usize = std::mem::size_of::<Vertex>();

/// A chunk's mesh, built once: its quads' vertices and indices.
pub struct Chunk {
    pub mesh: Mesh,
}

/// The baked map: its key (surface generation, size, density, seed) and its chunks, `None` for a chunk
/// with no quads.
struct Baked {
    key: (u64, i32, i32, Density),
    cols: i32,
    rows: i32,
    chunks: Vec<Option<Chunk>>,
    /// The cells with an object or a building on them: no scatter there.
    blocked: Vec<bool>,
    seed: u64,
    /// The surface codes and the map's size, read when a chunk is baked.
    codes: Vec<u8>,
    w: i32,
    h: i32,
    row_height: f32,
    /// The total number of quads and of vertex bytes, for the report.
    quads: usize,
    bytes: usize,
}

/// Bakes a quad into the vertices and indices of a mesh: the four corners, with the kind, phase, size and tint
/// in the vertex's extra data (`normal`): corner x in [-1, 1] and corner y in [0, 1] in the uv.
fn push_quad(vertices: &mut Vec<Vertex>, indices: &mut Vec<u16>, q: &Quad, base: Vec3, ground: [f32; 3]) {
    let start = vertices.len() as u16;
    let corners = [(-1.0, 0.0), (1.0, 0.0), (1.0, 1.0), (-1.0, 1.0)];
    // The colour: the ground's own (its palette colour) for grass and shrubs, a grey-brown for pebbles, a reed green
    // for reeds; each varied a little per quad (`tint`), so the scatter blends in.
    let kind_base = match q.kind {
        k if k == KIND_PEBBLE => [0.5, 0.47, 0.44],
        k if k == KIND_REED => [0.42, 0.46, 0.22],
        _ => ground,
    };
    let v = 0.8 + 0.4 * q.tint;
    let to_byte = |c: f32| (c * v * 255.0).clamp(0.0, 255.0) as u8;
    let colour = [to_byte(kind_base[0]), to_byte(kind_base[1]), to_byte(kind_base[2]), 255];
    for (cx, cy) in corners {
        let p = base + vec3(q.at.x, 0.0, q.at.y);
        vertices.push(Vertex { position: p, uv: vec2(cx, cy), color: colour, normal: vec4(q.kind, q.phase, q.size.x, q.size.y) });
    }
    indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
}

// ----- The shader -----

const VERTEX: &str = r#"#version 100
precision highp float;
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;
varying lowp vec4 vColor;
varying highp vec2 vUv;
varying lowp float vKind;
varying lowp float vFade;
varying highp float vPhase;
uniform mat4 Projection;
uniform vec3 CamRight;
uniform vec3 CamUp;
uniform float Clock;
uniform float Wind;
uniform float Sway;
uniform float Fade;
void main() {
    float kind = normal.x;
    float phase = normal.y;
    float w = normal.z;
    float h = normal.w;
    float top = texcoord.y;
    float gust = sin(Clock * 1.9 + phase * 6.2831853);
    float push = Sway * (0.6 * Wind + 0.4 * gust) * top * top;
    float amp = kind < 0.5 ? 0.08 : (kind < 1.5 ? 0.02 : (kind < 2.5 ? 0.0 : 0.1));
    vec3 p = position + CamRight * (texcoord.x * w * 0.5 + push * amp) + CamUp * (top * h);
    gl_Position = Projection * vec4(p, 1.0);
    vColor = color0 / 255.0;
    vUv = vec2(texcoord.x, top);
    vKind = kind;
    vFade = Fade;
    vPhase = phase;
}
"#;

const FRAGMENT: &str = r#"#version 100
precision mediump float;
varying lowp vec4 vColor;
varying highp vec2 vUv;
varying lowp float vKind;
varying lowp float vFade;
varying highp float vPhase;
void main() {
    float x = vUv.x;
    float y = vUv.y;
    float a = 0.0;
    float shade = 1.0;
    vec3 col = vColor.rgb;
    if (vKind < 0.5) {
        // A cluster of 5 to 9 blades from the base, each curved, dark at the root and lighter at the tip.
        float n = 5.0 + floor(fract(vPhase * 7.3) * 5.0);
        for (int k = 0; k < 9; k++) {
            float fk = float(k);
            if (fk < n) {
                float r = fract(sin(vPhase * 91.7 + fk * 12.9898) * 43758.5453);
                float bx = (fk / (n - 1.0) - 0.5) * 1.5 + (r - 0.5) * 0.25;
                float hk = 0.75 + 0.25 * fract(r * 13.7);
                float lean = (fract(r * 29.1) - 0.5) * 0.9;
                float t = y / hk;
                if (t < 1.0) {
                    float cx = bx + lean * t * t * 0.5;
                    float hw = 0.2 * (1.0 - t) + 0.06;
                    if (abs(x - cx) < hw) {
                        a = 1.0;
                        shade = mix(0.55, 1.15, t);
                    }
                }
            }
        }
        col = vec3(vColor.r * 0.96, vColor.g * 1.04, vColor.b * 0.9);
    } else if (vKind < 1.5) {
        // A leafy shrub: a cluster of rounded crowns, darker than the grass, speckled with leaves.
        float d = 1e3;
        for (int k = 0; k < 5; k++) {
            float fk = float(k);
            float cx = (fract(vPhase * (3.1 + fk) + fk * 0.37) - 0.5) * 1.1;
            float cy = 0.3 + 0.25 * fract(vPhase * (5.3 + fk) + fk * 0.61);
            float e = length(vec2((x - cx) / 0.42, (y - cy) / 0.42));
            d = min(d, e);
        }
        if (d < 1.0 && y < 0.95) {
            a = 1.0;
        }
        // Leaves: a speckle of small cells, each a little lighter or darker by a hash of its place.
        float leaf = fract(sin(dot(floor(vec2(x * 9.0 + vPhase * 7.0, y * 7.0)), vec2(12.9898, 78.233))) * 43758.5453);
        shade = (0.5 + 0.4 * y) * (0.82 + 0.3 * leaf);
        col = vColor.rgb * vec3(0.7, 0.9, 0.65);
    } else if (vKind < 2.5) {
        // A pebble: a grey-brown lump with its lower edge in shadow.
        float e = (x * x) / 0.8 + ((y - 0.42) * (y - 0.42)) / 0.2;
        if (e < 1.0) {
            a = 1.0;
        }
        shade = 0.5 + 0.6 * y;
        col = vec3(0.52, 0.48, 0.43);
    } else {
        // A reed: a thin stalk with a brown head near its top.
        float stalk = abs(x) < 0.12 * (1.0 - 0.4 * y) ? 1.0 : 0.0;
        float head = (abs(x) < 0.3 && y > 0.8 && y < 0.95) ? 1.0 : 0.0;
        a = max(stalk, head);
        shade = head > 0.5 ? 0.9 : 0.8 + 0.3 * y;
    }
    if (a <= 0.0 || vFade <= 0.0) {
        discard;
    }
    gl_FragColor = vec4(col * shade, vColor.a * vFade);
}
"#;

/// The scatter's material: the blend is the default alpha blend, no depth (the draw order is the ground's).
fn material() -> Option<Material> {
    MATERIAL.with(|m| {
        m.get_or_init(|| {
            // The default pipeline is the alpha blend the other map layers use.
            let params = PipelineParams::default();
            load_material(
                ShaderSource::Glsl { vertex: VERTEX, fragment: FRAGMENT },
                MaterialParams {
                    uniforms: vec![
                        UniformDesc::new("CamRight", UniformType::Float3),
                        UniformDesc::new("CamUp", UniformType::Float3),
                        UniformDesc::new("Clock", UniformType::Float1),
                        UniformDesc::new("Wind", UniformType::Float1),
                        UniformDesc::new("Sway", UniformType::Float1),
                        UniformDesc::new("Fade", UniformType::Float1),
                    ],
                    pipeline_params: params,
                    ..Default::default()
                },
            )
            .map_err(|e| razdor::diag!("scatter material: {e}"))
            .ok()
        })
        .clone()
    })
}

// ----- Fade and sway (pure) -----

/// The share of the scatter that shows at a map zoom `zoom` (`cam.scale / PX`): 1 from 0.9 up, 0 at 0.45 and
/// under. Zoomed out the tufts are a few pixels, so they fade out rather than pop off.
pub fn zoom_fade(zoom: f32) -> f32 {
    let t = ((zoom - 0.45) / 0.45).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// ----- Baking and drawing -----

/// Makes sure the baked map is for `key`, rebuilding the chunk table when it is not. Chunks bake on first use.
fn ensure(key: (u64, i32, i32, Density), map: &TileMap, blocked: impl FnOnce() -> Vec<bool>, row_height: f32) {
    CACHE.with_borrow_mut(|c| {
        if c.as_ref().is_some_and(|b| b.key == key) {
            return;
        }
        let (w, h) = (map.w, map.h);
        let cols = (w + CHUNK - 1) / CHUNK;
        let rows = (h + CHUNK - 1) / CHUNK;
        let codes: Vec<u8> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|t| map.surface_code(t)).collect();
        *c = Some(Baked {
            key,
            cols,
            rows,
            chunks: (0..cols * rows).map(|_| None).collect(),
            blocked: blocked(),
            seed: living::mix(((w as u64) << 32) | h as u64),
            codes,
            w,
            h,
            row_height,
            quads: 0,
            bytes: 0,
        });
    });
}

/// Whether an object stands on a cell next to `(x, y)`: the forest's edge.
fn neighbour_blocked(blocked: &[bool], w: i32, h: i32, x: i32, y: i32) -> bool {
    (-1..=1).any(|dy| (-1..=1).any(|dx| {
        let (nx, ny) = (x + dx, y + dy);
        (dx, dy) != (0, 0) && nx >= 0 && ny >= 0 && nx < w && ny < h && blocked[(ny * w + nx) as usize]
    }))
}

/// Bakes chunk `(cx, cy)` if it is not yet baked; the chunk's quads are placed by the cell hash.
fn bake_chunk(b: &mut Baked, cx: i32, cy: i32, density: Density) {
    let idx = (cy * b.cols + cx) as usize;
    if b.chunks[idx].is_some() {
        return;
    }
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut quads = 0usize;
    for y in cy * CHUNK..((cy + 1) * CHUNK).min(b.h) {
        for x in cx * CHUNK..((cx + 1) * CHUNK).min(b.w) {
            let i = (y * b.w + x) as usize;
            if b.blocked[i] {
                continue;
            }
            let ground = ground_of(b.codes[i]);
            let centre = vec2(x as f32, y as f32 * b.row_height);
            let c = super::world_view::surface_color(b.codes[i]);
            let edge = neighbour_blocked(&b.blocked, b.w, b.h, x, y);
            for q in cell_quads(b.seed, (x, y), centre, ground, density, edge) {
                push_quad(&mut vertices, &mut indices, &q, Vec3::ZERO, [c.r, c.g, c.b]);
                quads += 1;
            }
        }
    }
    let bytes = vertices.len() * VERTEX_BYTES + indices.len() * 2;
    b.quads += quads;
    b.bytes += bytes;
    let chunk = (quads > 0).then(|| Chunk { mesh: Mesh { vertices, indices, texture: None } });
    b.chunks[idx] = chunk;
}

/// Draws the visible scatter: the chunks of the cells `(c0..c1, r0..r1)` in view, each in one call. `camera`
/// is the 2.5D view's right and up on the ground (world units, the ground's own) and the wind and clock;
/// `zoom` is the map's zoom (`cam.scale / PX`). Nothing is drawn with the scatter off or with no quads in view.
#[allow(clippy::too_many_arguments)]
pub fn draw(key: (u64, i32, i32, Density), map: &TileMap, blocked: impl FnOnce() -> Vec<bool>, row_height: f32, cells: ((i32, i32), (i32, i32)), right: Vec2, sway: f32, zoom: f32, now: f64) {
    let density = density();
    if density == Density::Off {
        return;
    }
    let fade = zoom_fade(zoom);
    if fade <= 0.0 {
        return;
    }
    let Some(mat) = material() else { return };
    ensure(key, map, blocked, row_height);
    let ((c0, c1), (r0, r1)) = cells;
    CACHE.with_borrow_mut(|c| {
        let Some(b) = c.as_mut() else { return };
        let (cx0, cx1) = (c0.max(0) / CHUNK, ((c1 + CHUNK - 1) / CHUNK).min(b.cols));
        let (cy0, cy1) = (r0.max(0) / CHUNK, ((r1 + CHUNK - 1) / CHUNK).min(b.rows));
        gl_use_material(&mat);
        let time = (now % 1000.0) as f32;
        mat.set_uniform("CamRight", vec3(right.x, 0.0, right.y));
        mat.set_uniform("CamUp", vec3(0.0, 1.0, 0.0));
        mat.set_uniform("Clock", time);
        mat.set_uniform("Wind", living::wind(now));
        mat.set_uniform("Sway", sway);
        mat.set_uniform("Fade", fade);
        for cy in cy0..cy1 {
            for cx in cx0..cx1 {
                bake_chunk(b, cx, cy, density);
                if let Some(chunk) = &b.chunks[(cy * b.cols + cx) as usize] {
                    draw_mesh(&chunk.mesh);
                }
            }
        }
        gl_use_default_material();
    });
}

// ----- Contact shadows -----

/// A contact shadow's footprint: its centre and half-size on the ground (world units).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Footprint {
    pub centre: Vec2,
    pub half: Vec2,
}

/// The contact shadows of the footprints as one mesh over the ground (world units, y 0), `alpha` the strength
/// of each. `None` for none. The texture is the soft round one (`living::puff_texture`).
pub fn contact_mesh(footprints: &[Footprint], alpha: f32) -> Option<Mesh> {
    let (vertices, indices) = contact_geometry(footprints, alpha)?;
    Some(Mesh { vertices, indices, texture: Some(living::puff_texture()) })
}

/// The vertices and indices of [`contact_mesh`] (no texture, so no window is needed to make them).
pub fn contact_geometry(footprints: &[Footprint], alpha: f32) -> Option<(Vec<Vertex>, Vec<u16>)> {
    if footprints.is_empty() || alpha <= 0.0 {
        return None;
    }
    let mut vertices = Vec::with_capacity(footprints.len() * 4);
    let mut indices = Vec::with_capacity(footprints.len() * 6);
    let colour = Color::new(0.0, 0.0, 0.0, alpha);
    for f in footprints {
        let start = vertices.len() as u16;
        let corners = [(-1.0, -1.0, 0.0, 0.0), (1.0, -1.0, 1.0, 0.0), (1.0, 1.0, 1.0, 1.0), (-1.0, 1.0, 0.0, 1.0)];
        for (sx, sz, u, v) in corners {
            let p = f.centre + vec2(sx * f.half.x, sz * f.half.y);
            vertices.push(Vertex::new(p.x, 0.0, p.y, u, v, colour));
        }
        indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
    }
    Some((vertices, indices))
}

/// Draws a contact mesh over the ground in the current 3D view (the caller sets the camera and the model stack).
pub fn draw_contact(mesh: &Mesh) {
    draw_mesh(mesh);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_off_until_it_is_set() {
        set_prefs(Density::Off, false);
        assert_eq!(density(), Density::Off);
        assert!(!contact_on());
        set_prefs(Density::Many, true);
        assert_eq!(density(), Density::Many);
        assert!(contact_on());
        set_prefs(Density::Off, false);
    }

    #[test]
    fn the_settings_cycle_and_parse() {
        assert_eq!(Density::Off.next(), Density::Few);
        assert_eq!(Density::Few.next(), Density::Many);
        assert_eq!(Density::Many.next(), Density::Off);
        assert_eq!(Density::parse(" MANY "), Some(Density::Many));
        assert_eq!(Density::parse("most"), None);
    }

    #[test]
    fn water_roads_snow_and_lava_get_nothing_and_grass_and_dirt_get_their_own() {
        for c in [0u8, 1, 2, 3, 4, 13, 14, 15] {
            assert_eq!(ground_of(c), Ground::None, "code {c}");
        }
        for c in [5u8, 6, 7] {
            assert_eq!(ground_of(c), Ground::Grass, "code {c}: the yellow plain is ground, not a field");
        }
        for c in [10u8, 11, 12] {
            assert_eq!(ground_of(c), Ground::Dirt, "code {c}");
        }
        for c in [8u8, 9] {
            assert_eq!(ground_of(c), Ground::Swamp, "code {c}");
        }
    }

    #[test]
    fn the_same_cell_gets_the_same_quads_and_off_gets_none() {
        let c = vec2(3.0, 4.0);
        let a = cell_quads(7, (3, 4), c, Ground::Grass, Density::Many, false);
        let b = cell_quads(7, (3, 4), c, Ground::Grass, Density::Many, false);
        assert_eq!(a, b, "a seeded hash: the same map gets the same layout");
        assert!(cell_quads(7, (3, 4), c, Ground::Grass, Density::Off, false).is_empty());
        // Another seed gives another layout over a block of cells (one cell may be bare under both).
        let block = |seed: u64| -> Vec<Quad> {
            (0..20).flat_map(|x| (0..20).map(move |y| (x, y))).flat_map(|(x, y)| cell_quads(seed, (x, y), vec2(x as f32, y as f32), Ground::Grass, Density::Many, false)).collect()
        };
        assert_ne!(block(7), block(8), "another map, another layout");
    }

    #[test]
    fn many_has_more_quads_than_few_and_all_stay_near_their_cell() {
        let mut few = 0;
        let mut many = 0;
        for x in 0..40 {
            for y in 0..40 {
                let c = vec2(x as f32, y as f32);
                few += cell_quads(11, (x, y), c, Ground::Grass, Density::Few, false).len();
                let m = cell_quads(11, (x, y), c, Ground::Grass, Density::Many, false);
                many += m.len();
                for q in m {
                    assert!((q.at - c).abs().max_element() <= 0.41, "a quad stays in its cell");
                }
            }
        }
        assert!(few > 0 && many > few * 2, "few {few}, many {many}");
        // Few is sparse: well under one tuft per grass cell.
        assert!((few as f32) / 1600.0 < 1.0);
    }

    #[test]
    fn a_zoomed_out_map_fades_the_scatter_and_a_close_one_keeps_it() {
        assert_eq!(zoom_fade(1.0), 1.0);
        assert_eq!(zoom_fade(0.3), 0.0);
        let mid = zoom_fade(0.6);
        assert!(mid > 0.0 && mid < 1.0);
    }

    #[test]
    fn a_quad_bakes_four_corners_and_two_triangles() {
        let q = Quad { at: vec2(1.0, 2.0), kind: KIND_TUFT, size: vec2(0.2, 0.2), phase: 0.5, tint: 0.5 };
        let mut v = Vec::new();
        let mut i = Vec::new();
        push_quad(&mut v, &mut i, &q, Vec3::ZERO, [0.3, 0.5, 0.2]);
        assert_eq!(v.len(), 4);
        assert_eq!(i, vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(v[0].normal, vec4(KIND_TUFT, 0.5, 0.2, 0.2));
        assert_eq!(v[0].position, vec3(1.0, 0.0, 2.0));
    }

    /// The real map's scatter: quads, vertices and the bytes of the baked chunks, for the report. Needs the install
    /// (`RAZDOR_DT_DIR`); run with `--ignored --nocapture`.
    #[test]
    #[ignore = "needs RAZDOR_DT_DIR: counts the scatter of the real РК3-Столица map"]
    fn real_map_scatter_counts() {
        use razdor::dt::dtm::Scenario;
        use razdor::dt::install::DtInstall;
        use razdor::rules::map::{Decoration, Grid};
        let dir = std::env::var("RAZDOR_DT_DIR").expect("RAZDOR_DT_DIR");
        let install = DtInstall::load(std::path::Path::new(&dir)).expect("the install");
        let path = install.file("Maps_Rus/РК3-Столица.DTm").expect("the map");
        let s = Scenario::load(&path).expect("the scenario");
        let objects = s.objects.iter().map(|o| Decoration::new((o.x as i32, o.y as i32), o.class, o.sprite)).collect();
        let map = TileMap::from_codes(Grid::Square8, s.width() as i32, s.height() as i32, &s.terrain, objects);
        let blocked = || {
            let mut b = vec![false; (map.w * map.h) as usize];
            for o in map.objects_in_rect(0, map.w, 0, map.h) {
                for (x, y) in razdor::rules::map::object_cells(&o) {
                    if x >= 0 && y >= 0 && x < map.w && y < map.h {
                        b[(y * map.w + x) as usize] = true;
                    }
                }
            }
            b
        };
        for density in [Density::Few, Density::Many] {
            let key = (map.surface_generation(), map.w, map.h, density);
            ensure(key, &map, blocked, Grid::Square8.row_height());
            let (mut quads, mut bytes, mut verts) = (0usize, 0usize, 0usize);
            CACHE.with_borrow_mut(|c| {
                let b = c.as_mut().expect("baked");
                for cy in 0..b.rows {
                    for cx in 0..b.cols {
                        bake_chunk(b, cx, cy, density);
                    }
                }
                quads = b.quads;
                bytes = b.bytes;
                verts = b.chunks.iter().flatten().map(|c| c.mesh.vertices.len()).sum();
            });
            println!("{density:?}: {}x{} cells, {quads} quads, {verts} vertices, {bytes} bytes of chunk buffers", map.w, map.h);
        }
    }

    #[test]
    fn a_contact_shadow_is_one_soft_quad_per_footprint() {
        assert!(contact_geometry(&[], 0.3).is_none());
        let (v, i) = contact_geometry(&[Footprint { centre: vec2(2.0, 3.0), half: vec2(0.3, 0.2) }], 0.3).expect("a mesh");
        assert_eq!(v.len(), 4);
        assert_eq!(i.len(), 6);
        assert!(v.iter().all(|v| v.position.y == 0.0), "lies on the ground");
    }
}
