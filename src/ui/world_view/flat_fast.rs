//! The flat view's sprite fast path (option f, `RAZDOR_FLAT_FAST`): the standing objects with a picture are drawn from
//! static records (the corner's cell, the corner offset in sprite pixels, the picture of the whole sprite and of its two
//! copies, the sway phase of a tree), one loop per pass. Each quad is computed with the same f32 operations, in the same
//! order, as `draw_object` → `draw_picture_form` → `sharp::draw` (and `draw_leaning` for a tree), so its vertices are the
//! same bytes; it is submitted by `sharp::draw_quad`, as `sharp::draw` submits it.
//!
//! The records follow the map's objects and the atlas; a camera move or a zoom changes no record (the scale enters in the
//! loop; the copy is chosen there). Not on this path, and drawn as before: the hills, an object without a picture, the
//! sun's form shading (`FrameCtx::sun`: the corner colours), the tilted views, and every other drawable.
//!
//! Modes (`RAZDOR_FLAT_FAST`): the fast path by default; `0` the old path only; `check` the old path runs and its pieces
//! are compared with the fast path's vertices, sprite by sprite, every frame (the output is the old path's).

use std::rc::Rc;

use macroquad::miniquad::TextureId;
use macroquad::prelude::*;
use razdor::frame_probe;
use razdor::local;
use razdor::rules::map::{object_class, Decoration, Grid, TileMap};

use super::super::dt_art::{Atlas, DtArt, Slot};
use super::super::sharp;
use super::{detail_visibility, lit_whole_if, plant_shift, sprite_lod_divisor, Camera, FrameCtx, PX, SPRITE_VIS};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Off,
    On,
    Check,
}

fn mode() -> Mode {
    static MODE: std::sync::OnceLock<Mode> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("RAZDOR_FLAT_FAST").ok().as_deref().map(str::trim) {
        Some("0") => Mode::Off,
        Some("check") => Mode::Check,
        _ => Mode::On,
    })
}

/// One object's static record: what its quad depends on besides the camera (see the module's note).
#[derive(Clone, Copy)]
struct Rec {
    /// The sprite's corner in the cell's frame: `(x + 0.5, y + row_height / 2)` (`object_sprite_origin`).
    corner: (f32, f32),
    /// The corner's offset in sprite pixels: `(dx − w − 0.5, dy − h − 0.5)` for the whole sprite's `w`, `h`.
    offset: Vec2,
    /// The picture of the whole sprite, then of its copies at 1/2 and 1/4 (the whole one where a copy is missing).
    slots: [Slot; 3],
    /// A tree's sway phase (`TREES` only: the others are not swaying).
    tree: Option<i32>,
}

fn rec_of(o: &Decoration, atlas: &Atlas, cam: &Camera) -> Option<Rec> {
    let r = atlas.decoration(o.class, o.sprite)?;
    let (x, y) = cam.grid.center(o.tile);
    let (dx, dy) = match o.class {
        1..=8 => (16.0, 11.0),
        _ => plant_shift(o),
    };
    let copy = |d: u32| atlas.decoration_lod(o.class, o.sprite, d).unwrap_or(r);
    Some(Rec {
        corner: (x + 0.5, y + cam.grid.row_height() / 2.0),
        offset: vec2(dx - r.full.x - 0.5, dy - r.full.y - 0.5),
        slots: [r, copy(2), copy(4)],
        tree: (o.class == object_class::TREES).then_some(o.jitter.2),
    })
}

/// What the records were made from: the map (its identity, its objects' count and a hash of their pictures and cells),
/// the grid (the cell's frame) and the atlas (its address, and its texture's id: a texture id is never reused, so an
/// atlas made again at the same address is not taken for the old one).
#[derive(Clone, Copy, PartialEq)]
struct Key {
    map: usize,
    objects: usize,
    hash: u64,
    grid: Grid,
    atlas: usize,
    texture: TextureId,
}

type Records = Rc<Vec<Option<Rec>>>;

local! {
    /// The records of the last map, kept while their key holds.
    static RECS: std::cell::RefCell<Option<(Key, Records)>> = const { std::cell::RefCell::new(None) };
}

/// The records of the map's objects (index = the object's index), made again when the key changes.
fn records(map: &TileMap, atlas: &Atlas, cam: &Camera) -> Records {
    // The objects' hash is computed each frame: a map swapped for another at the same address is then not taken for the old one.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for o in &map.objects {
        for v in [o.tile.0 as u64, o.tile.1 as u64, o.class as u64, o.sprite as u64, o.jitter.0 as u64, o.jitter.1 as u64, o.jitter.2 as u64] {
            hash = (hash ^ v).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    let key = Key { map: std::ptr::from_ref(map) as usize, objects: map.objects.len(), hash, grid: cam.grid, atlas: std::ptr::from_ref(atlas) as usize, texture: atlas.texture.raw_miniquad_id() };
    RECS.with_borrow_mut(|c| {
        if let Some((k, recs)) = c.as_ref() {
            if *k == key {
                return recs.clone();
            }
        }
        let recs: Records = Rc::new(map.objects.iter().map(|o| rec_of(o, atlas, cam)).collect());
        *c = Some((key, recs.clone()));
        recs
    })
}

/// What one object does in the pass: nothing (too small to see), a quad in the pass's mesh, or the old path.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Built {
    Old,
    Skip,
    Quad,
}

/// The flat pass's sprite state, read once for the pass (`FrameCtx`, the scale factors, the session's material).
pub struct Pass<'a> {
    cam: &'a Camera,
    recs: Records,
    mode: Mode,
    mat: Material,
    tex_size: Vec2,
    /// `cam.scale / PX`: the sprites' screen pixels per sprite pixel.
    zoom: f32,
    dpi: f32,
    night: bool,
    clock: i64,
    anim: f32,
    divisor: Option<u32>,
    /// The standing sprites' visibility outside a fade (`SPRITE_VIS` at the pass's start).
    vis: f32,
    /// `draw_scale`'s perspective and raster factors.
    factors: (f32, f32),
    /// The quad of the current object; its indices and texture are set once.
    mesh: Mesh,
    checked: u32,
    differ: u32,
}

impl<'a> Pass<'a> {
    /// The pass for the flat view's objects, or None where the old path draws them all: the mode is off, the view is tilted,
    /// the sun's shading takes corner colours, a fog cell is set, or the session has no material (no sharp material).
    pub fn begin(map: &TileMap, art: Option<&DtArt>, cam: &'a Camera, ctx: &FrameCtx) -> Option<Pass<'a>> {
        let mode = mode();
        if mode == Mode::Off || cam.tilt.is_some() || ctx.sun.is_some() || !sharp::fog_cell_is_zero() {
            return None;
        }
        let atlas = art?.map_atlas()?;
        let mat = sharp::pass_quad_material()?;
        let tex = atlas.texture.clone();
        let tex_size = sharp::texels(&tex);
        Some(Pass {
            cam,
            recs: records(map, atlas, cam),
            mode,
            mat,
            tex_size,
            zoom: cam.scale / PX,
            dpi: ctx.dpi,
            night: ctx.night,
            clock: ctx.clock,
            anim: ctx.anim,
            divisor: sprite_lod_divisor(cam.scale),
            vis: SPRITE_VIS.with(std::cell::Cell::get),
            factors: sharp::pass_factors(),
            mesh: Mesh { vertices: Vec::with_capacity(4), indices: vec![0, 1, 2, 0, 2, 3], texture: Some(tex) },
            checked: 0,
            differ: 0,
        })
    }

    /// Whether the pass runs the check mode (the caller draws the object the old way and passes its pieces to [`Pass::check`]).
    pub fn checking(&self) -> bool {
        self.mode == Mode::Check
    }

    /// Draws object `i` on the fast path. False when the old path must draw it.
    pub fn draw(&mut self, i: u32) -> bool {
        match self.build(i) {
            Built::Old => false,
            Built::Skip => true,
            Built::Quad => {
                sharp::draw_quad(&self.mat, &self.mesh, self.tex_size);
                true
            }
        }
    }

    /// The check mode: compares object `i`'s fast quad with the `old` pieces the old path made for it (one piece, or none
    /// when it is not drawn), byte for byte.
    pub fn check(&mut self, i: u32, old: &[sharp::Piece]) {
        let same = match self.build(i) {
            Built::Old => return,
            Built::Skip => old.is_empty(),
            Built::Quad => old.len() == 1 && sharp::same_piece(&old[0], &self.mesh),
        };
        self.checked += 1;
        if !same {
            self.differ += 1;
            if self.differ <= 3 {
                razdor::diag!("flat fast check: object {i} differs (old pieces {}, fast {})", old.len(), self.mesh.vertices.len());
            }
        }
    }

    /// Ends the pass; the check mode logs its counts.
    pub fn finish(self) {
        if self.mode == Mode::Check {
            razdor::diag!("flat fast check: {} sprites, {} differ (zoom {:.4}, copy divisor {:?})", self.checked, self.differ, self.zoom, self.divisor);
        }
    }

    /// The quad of object `i` (the arithmetic of `draw_object`, `draw_picture_form` and `sharp::draw`, in their order).
    fn build(&mut self, i: u32) -> Built {
        let Some(rec) = self.recs.get(i as usize).copied().flatten() else { return Built::Old };
        let zoom = self.zoom;
        // `object_picture` and `draw_object`: the corner, the origin of the whole sprite, its picture and fade.
        let origin = self.cam.to_screen(rec.corner) + rec.offset * zoom;
        let full = rec.slots[0].full;
        let picture = Rect::new(origin.x, origin.y, full.x * zoom, full.y * zoom);
        let detail = detail_visibility(picture, self.dpi);
        // `!(detail > 0.0)` of the old path: a NaN is not drawn either.
        if detail.is_nan() || detail <= 0.0 {
            return Built::Skip;
        }
        let vis = if detail < 1.0 { detail } else { self.vis };
        let slot = rec.slots[match self.divisor {
            Some(2) => 1,
            Some(4) => 2,
            _ => 0,
        }];
        let r = lit_whole_if(slot, self.night);
        // `faded(WHITE)`: the colour kept, the alpha times the visibility.
        let color = Color::new(WHITE.r, WHITE.g, WHITE.b, WHITE.a * vis);
        let c: [u8; 4] = color.into();
        if let Some(phase) = rec.tree {
            // `draw_leaning`: the top corners lean by the sway, the bottom stays; drawn by `draw_mesh_sharp`.
            let lean = razdor::rules::rng::tree_sway(self.clock, phase) * zoom * self.anim;
            let whole = Slot { rect: Rect::new(r.base.x, r.base.y, r.tex.x, r.tex.y), off: Vec2::ZERO, size: r.full, ..r };
            let src = whole.rect;
            let (tw, th) = (self.tex_size.x, self.tex_size.y);
            let (u0, v0, u1, v1) = (src.x / tw, src.y / th, (src.x + src.w) / tw, (src.y + src.h) / th);
            let at = origin + whole.off * zoom;
            let size = whole.size * zoom;
            let share = |y: f32| lean * (1.0 - y / whole.full.y);
            let (top, bottom) = (share(whole.off.y), share(whole.off.y + whole.size.y));
            // The scale in the vertices' normals (`draw_scaled_mesh`), the fog cell there is none.
            let scale = vec2(size.x / src.w, size.y / src.h).abs();
            let normal = vec4(scale.x, scale.y, 0.0, 0.0);
            let v = |x: f32, y: f32, u: f32, v: f32| Vertex { position: vec3(x, y, 0.0), uv: vec2(u, v), color: c, normal };
            self.mesh.vertices.clear();
            self.mesh.vertices.extend([
                v(at.x + top, at.y, u0, v0),
                v(at.x + size.x + top, at.y, u1, v0),
                v(at.x + size.x + bottom, at.y + size.y, u1, v1),
                v(at.x + bottom, at.y + size.y, u0, v1),
            ]);
            return Built::Quad;
        }
        // `draw_picture_form` (no sun: `sharp::draw` of the picture), then `sharp::draw`'s quad (`fill_sprite_mesh`).
        frame_probe::sprite(r.size.x * zoom, r.size.y * zoom);
        let (src, at, size) = (r.rect, origin + r.off * zoom, r.size * zoom);
        let (x, y, w, h) = (at.x, at.y, size.x, size.y);
        let pivot = vec2(x + w / 2., y + h / 2.);
        // The rotation is 0: its sine and cosine are exactly 0 and 1.
        let (sin_r, cos_r) = (0.0_f32, 1.0_f32);
        let corner = |px: f32, py: f32| {
            let p = vec2(px, py) - pivot;
            vec2(p.x * cos_r - p.y * sin_r, p.x * sin_r + p.y * cos_r) + pivot
        };
        // `draw_scale`: the destination over the source, times the perspective and the raster factors, in this order.
        let scale = size.abs() / src.size() * self.factors.0 * self.factors.1;
        let normal = vec4(scale.x, scale.y, 0.0, 0.0);
        let (width, height) = (self.tex_size.x, self.tex_size.y);
        let (u0, v0) = (src.x / width, src.y / height);
        let (u1, v1) = ((src.x + src.w) / width, (src.y + src.h) / height);
        let v = |p: Vec2, u: f32, v: f32| Vertex { position: vec3(p.x, p.y, 0.0), uv: vec2(u, v), color: c, normal };
        self.mesh.vertices.clear();
        self.mesh.vertices.extend([v(corner(x, y), u0, v0), v(corner(x + w, y), u1, v0), v(corner(x + w, y + h), u1, v1), v(corner(x, y + h), u0, v1)]);
        Built::Quad
    }
}
