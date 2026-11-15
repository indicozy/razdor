//! The 2.5D view's standing objects on the fast path (option f, stage 2, `RAZDOR_DEPTH_FAST`): the objects with a picture are
//! drawn from the static records of `object_records`, one loop per pass. Each object does what `old_object` does: the same
//! visibility (`detail_visibility`), the same fog cell (`map_fog`, read per frame from the table), the same billboard
//! (`standing_model`, computed per frame: it depends on the camera), the same off-window cull (`picture_quad`), the same
//! CPU model matrix (`push_cpu_model`, which macroquad applies to the quad's vertices), and the quad of `draw_object`
//! (`draw_picture_form` → `sharp::draw`, `draw_leaning` for a tree), with the same f32 operations in the same order.
//!
//! Not on this path, and drawn as before: an object without a picture, the hills, the trailer's and the first-person
//! views (their near fades and magnification), the eyes view (`eye_at`), the sun's form shading (`FrameCtx::sun`), and
//! every other drawable.
//!
//! Modes (`RAZDOR_DEPTH_FAST`): the fast path by default; `0` the old path only; `check` the old path runs, and its
//! textured pieces and CPU model matrices are compared with the fast path's, object by object, every frame (the output is
//! the old path's).

use std::sync::atomic::{AtomicBool, Ordering};

use macroquad::prelude::*;
use razdor::frame_probe;
use razdor::local;
use razdor::rules::map::TileMap;

use super::super::dt_art::DtArt;
use super::super::sharp;
use super::super::sprite_fog::Cell as FogCell;
use super::object_records::{self, copy_index, plain_quad, tree_quad, Records};
use super::{detail_visibility, sprite_lod_divisor, Camera, FrameCtx, Pass as DrawPass, PX};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Off,
    On,
    Check,
}

fn mode() -> Mode {
    static MODE: std::sync::OnceLock<Mode> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("RAZDOR_DEPTH_FAST").ok().as_deref().map(str::trim) {
        Some("0") => Mode::Off,
        Some("check") => Mode::Check,
        _ => Mode::On,
    })
}

/// What the pass does with object `i`: nothing (not drawn: too small, faded out, hidden, or off the window), a quad with its
/// model matrix, or the old path.
#[derive(Clone, Copy)]
enum Built {
    Old,
    Skip,
    Quad(Mat4),
}

/// The matrices the old path pushes while the check mode runs (`push_cpu_model` records them, `note_push`).
static NOTING: AtomicBool = AtomicBool::new(false);

local! {
    static PUSHED: std::cell::RefCell<Vec<Mat4>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Starts keeping the CPU model matrices pushed from now on (the check mode). Ends with [`note_end`].
pub fn note_start() {
    PUSHED.with_borrow_mut(Vec::clear);
    NOTING.store(true, Ordering::Relaxed);
}

/// Records a CPU model matrix pushed by `world_view::push_cpu_model` while the check mode notes them.
pub fn note_push(m: Mat4) {
    if NOTING.load(Ordering::Relaxed) {
        PUSHED.with_borrow_mut(|p| p.push(m));
    }
}

/// Stops keeping the matrices and returns those pushed since [`note_start`].
pub fn note_end() -> Vec<Mat4> {
    NOTING.store(false, Ordering::Relaxed);
    PUSHED.with_borrow_mut(std::mem::take)
}

/// Whether two matrices are the same bits.
fn same_bits(a: Mat4, b: Mat4) -> bool {
    a.to_cols_array().map(f32::to_bits) == b.to_cols_array().map(f32::to_bits)
}

/// The 2.5D pass for the standing objects, or None where the old path draws them all: the mode is off, the view is flat or
/// is a trailer, first-person or eyes view, the sun's shading takes corner colours, the frame's pass is the ground's, or the
/// session has no material.
pub struct Pass<'a> {
    cam: &'a Camera,
    ctx: &'a FrameCtx,
    recs: Records,
    mode: Mode,
    mat: Material,
    tex_size: Vec2,
    /// `cam.scale / PX`: the sprites' screen pixels per sprite pixel.
    zoom: f32,
    /// The copy the sprites are drawn from (`sprite_lod_divisor`).
    divisor: Option<u32>,
    /// `draw_scale`'s perspective (1: `standing_core`'s `with_perspective(1.0)`) and the raster factor of the pass.
    factors: (f32, f32),
    /// The quad of the current object; its indices and texture are set once.
    mesh: Mesh,
    checked: u32,
    differ: u32,
    fogged: u32,
    /// The sprites the fog hides on their foot cell (`FogCell::Hidden`), counted in every frame.
    hidden: u32,
}

impl<'a> Pass<'a> {
    pub fn begin(map: &TileMap, art: Option<&DtArt>, cam: &'a Camera, ctx: &'a FrameCtx) -> Option<Pass<'a>> {
        let mode = mode();
        let tilt = cam.tilt?;
        if mode == Mode::Off || tilt.cam.eye_at.is_some() || super::trailer().is_some() || super::first_person() || ctx.sun.is_some() || ctx.pass == DrawPass::Ground {
            return None;
        }
        let atlas = art?.map_atlas()?;
        let mat = sharp::pass_quad_material()?;
        let tex = atlas.texture.clone();
        let tex_size = sharp::texels(&tex);
        Some(Pass {
            cam,
            ctx,
            recs: object_records::records(map, atlas, cam, tex_size),
            mode,
            mat,
            tex_size,
            zoom: cam.scale / PX,
            divisor: sprite_lod_divisor(cam.scale),
            factors: (1.0, sharp::pass_factors().1),
            mesh: Mesh { vertices: Vec::with_capacity(4), indices: vec![0, 1, 2, 0, 2, 3], texture: Some(tex) },
            checked: 0,
            differ: 0,
            fogged: 0,
            hidden: 0,
        })
    }

    /// Whether the pass runs the check mode (the caller draws the object the old way and passes what it made to [`Pass::check`]).
    pub fn checking(&self) -> bool {
        self.mode == Mode::Check
    }

    /// Draws object `i` (its foot `foot`) on the fast path. False when the old path must draw it.
    pub fn draw(&mut self, i: u32, foot: Vec2) -> bool {
        match self.build(i, foot) {
            Built::Old => false,
            Built::Skip => true,
            Built::Quad(place) => {
                super::push_cpu_model(place);
                sharp::draw_quad(&self.mat, &self.mesh, self.tex_size);
                super::pop_cpu_model();
                true
            }
        }
    }

    /// The check mode: compares object `i`'s fast result with what the old path made for it: its textured pieces (`old`) and
    /// its CPU model matrices (`mats`), byte for byte.
    pub fn check(&mut self, i: u32, foot: Vec2, old: &[sharp::Piece], mats: &[Mat4]) {
        let same = match self.build(i, foot) {
            Built::Old => return,
            Built::Skip => old.is_empty() && mats.is_empty(),
            Built::Quad(place) => {
                // The sprites whose foot cell is in the fog's table (their normals carry a fog cell): the walks' fog is checked here.
                if self.mesh.vertices.first().is_some_and(|v| v.normal.z != 0.0 || v.normal.w != 0.0) {
                    self.fogged += 1;
                }
                old.len() == 1 && sharp::same_piece(&old[0], &self.mesh) && mats.len() == 1 && same_bits(mats[0], place)
            }
        };
        self.checked += 1;
        if !same {
            self.differ += 1;
            if self.differ <= 3 {
                razdor::diag!("depth fast check: object {i} differs (old pieces {}, matrices {}, fast {})", old.len(), mats.len(), self.mesh.vertices.len());
            }
        }
    }

    /// Ends the pass; the check mode logs its counts.
    pub fn finish(self) {
        if self.mode == Mode::Check {
            razdor::diag!("depth fast check: {} sprites, {} differ, {} fogged, {} hidden by fog, fog table uploads {} (zoom {:.4}, copy divisor {:?})", self.checked, self.differ, self.fogged, self.hidden, super::super::sprite_fog::upload_count(), self.zoom, self.divisor);
        }
    }

    /// What `old_object` → `standing_map` → `standing_core` → `draw_object` does with object `i`, on its records. The
    /// arithmetic and its order are theirs; the static parts come from the record.
    fn build(&mut self, i: u32, foot: Vec2) -> Built {
        // Borrowed fields only (the record is large): `recs` and `mesh` are disjoint fields.
        let Some(Some(rec)) = self.recs.get(i as usize) else { return Built::Old };
        let (cam, ctx) = (self.cam, self.ctx);
        let Some(tilt) = cam.tilt else { return Built::Old };
        let zoom = self.zoom;
        // `object_picture`: the sprite's origin and its whole picture; the fade is the picture's detail (`old_object`).
        let origin = cam.to_screen(rec.corner) + rec.offset * zoom;
        let full = rec.pics[0].plain.slot.full;
        let picture = Rect::new(origin.x, origin.y, full.x * zoom, full.y * zoom);
        let detail = detail_visibility(picture, ctx.dpi);
        // `if detail > 0.0` of `old_object`: a NaN is not drawn either.
        if detail.is_nan() || detail <= 0.0 {
            return Built::Skip;
        }
        // `standing_core`: the fog cell of the foot (a cell that hides the sprite draws nothing), then the bend's horizon.
        let fog = match super::map_fog(rec.fog) {
            FogCell::Hidden => {
                self.hidden += 1;
                return Built::Skip;
            }
            FogCell::Clear => None,
            FogCell::At(uv) => Some(uv),
        };
        let vis = detail;
        if vis <= 0.0 {
            return Built::Skip;
        }
        if tilt.cam.bend.is_some_and(|b| b.hidden((foot + cam.origin) / cam.scale)) {
            return Built::Skip;
        }
        // The billboard about the foot, and the off-window cull (`standing_core`: a corner behind the eye keeps the sprite).
        // `bent` is the sprites' run of a bent view (`standing_core`: `pass == Pass::Sprites`).
        let (place, _) = super::standing_model(cam, &tilt, foot, foot, ctx.pass == DrawPass::Sprites);
        if let Some(q) = super::picture_quad(ctx.view_proj, ctx.view, place, picture) {
            if !super::overlaps(super::quad_bounds(q), ctx.window, super::OFF_WINDOW_MARGIN) {
                return Built::Skip;
            }
        }
        // `draw_object` (its colour is `faded(WHITE)` with the visibility `standing_core` set).
        let pic = &rec.pics[copy_index(self.divisor)];
        let color = Color::new(WHITE.r, WHITE.g, WHITE.b, WHITE.a * vis);
        let fog = fog.unwrap_or(Vec2::ZERO);
        if let Some(phase) = rec.tree {
            // `draw_leaning`: the top corners lean by the sway (`ctx.anim` scales it), the bottom stays.
            let lean = razdor::rules::rng::tree_sway(ctx.clock, phase) * zoom * ctx.anim;
            tree_quad(&mut self.mesh, &pic.tree, origin, zoom, lean, color, fog);
            return Built::Quad(place);
        }
        let shape = if ctx.night { &pic.lit } else { &pic.plain };
        frame_probe::sprite(shape.slot.size.x * zoom, shape.slot.size.y * zoom);
        plain_quad(&mut self.mesh, shape, origin, zoom, color, fog, self.factors);
        Built::Quad(place)
    }
}
