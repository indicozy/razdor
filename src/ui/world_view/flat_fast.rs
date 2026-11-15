//! The flat view's sprite fast path (option f, `RAZDOR_FLAT_FAST`): the standing objects with a picture are drawn from
//! static records (`object_records`: the corner's cell, the corner offset in sprite pixels, the picture of the whole sprite and
//! of its two copies, the sway phase of a tree), one loop per pass. Each quad is computed with the same f32 operations, in the
//! same order, as `draw_object` → `draw_picture_form` → `sharp::draw` (and `draw_leaning` for a tree), so its vertices are the
//! same bytes; it is submitted by `sharp::draw_quad`, as `sharp::draw` submits it.
//!
//! The records follow the map's objects and the atlas; a camera move or a zoom changes no record (the scale enters in the
//! loop; the copy is chosen there). Not on this path, and drawn as before: the hills, an object without a picture, the
//! sun's form shading (`FrameCtx::sun`: the corner colours), the tilted views (`depth_fast`), and every other drawable.
//!
//! Modes (`RAZDOR_FLAT_FAST`): the fast path by default; `0` the old path only; `check` the old path runs and its pieces
//! are compared with the fast path's vertices, sprite by sprite, every frame (the output is the old path's).

use macroquad::prelude::*;
use razdor::frame_probe;
use razdor::rules::map::TileMap;

use super::super::dt_art::{Atlas, DtArt};
use super::super::sharp;
use super::object_records::{self, copy_index, plain_quad, tree_quad, Records};
use super::{detail_visibility, sprite_lod_divisor, Camera, FrameCtx, PX, SPRITE_VIS};

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

/// What the pass does with object `i`: nothing (too small to see), a quad in the pass's mesh, or the old path.
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
        let atlas: &Atlas = art?.map_atlas()?;
        let mat = sharp::pass_quad_material()?;
        let tex = atlas.texture.clone();
        let tex_size = sharp::texels(&tex);
        Some(Pass {
            cam,
            recs: object_records::records(map, atlas, cam, tex_size),
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
        // Borrowed fields only (the record is large): `recs` and `mesh` are disjoint fields.
        let Some(Some(rec)) = self.recs.get(i as usize) else { return Built::Old };
        let zoom = self.zoom;
        // `object_picture` and `draw_object`: the corner, the origin of the whole sprite, its picture and fade.
        let origin = self.cam.to_screen(rec.corner) + rec.offset * zoom;
        let full = rec.pics[0].plain.slot.full;
        let picture = Rect::new(origin.x, origin.y, full.x * zoom, full.y * zoom);
        let detail = detail_visibility(picture, self.dpi);
        // `!(detail > 0.0)` of the old path: a NaN is not drawn either.
        if detail.is_nan() || detail <= 0.0 {
            return Built::Skip;
        }
        let vis = if detail < 1.0 { detail } else { self.vis };
        let pic = &rec.pics[copy_index(self.divisor)];
        // `faded(WHITE)`: the colour kept, the alpha times the visibility.
        let color = Color::new(WHITE.r, WHITE.g, WHITE.b, WHITE.a * vis);
        if let Some(phase) = rec.tree {
            // `draw_leaning`: the top corners lean by the sway, the bottom stays; drawn by `draw_mesh_sharp`.
            let lean = razdor::rules::rng::tree_sway(self.clock, phase) * zoom * self.anim;
            // The flat view's sprites take no fog: the normals' fog cell is zero.
            tree_quad(&mut self.mesh, &pic.tree, origin, zoom, lean, color, Vec2::ZERO);
            return Built::Quad;
        }
        // `draw_picture_form` (no sun: `sharp::draw` of the picture), then `sharp::draw`'s quad (`fill_sprite_mesh`).
        let shape = if self.night { &pic.lit } else { &pic.plain };
        frame_probe::sprite(shape.slot.size.x * zoom, shape.slot.size.y * zoom);
        plain_quad(&mut self.mesh, shape, origin, zoom, color, Vec2::ZERO, self.factors);
        Built::Quad
    }
}
