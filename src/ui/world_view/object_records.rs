//! The static records of the map's standing objects (option f: `flat_fast` and `depth_fast`): what a sprite's quad depends on
//! besides the camera and the frame. Built once per map, grid and atlas (kept while their key holds) and read by the flat
//! view's fast path and the 2.5D view's. Each computes its quad with the same f32 operations as the old path
//! (`draw_object` → `draw_picture_form` → `sharp::draw`, and `draw_leaning` for a tree).
//!
//! A record holds the corner in the cell's frame, the corner's offset in sprite pixels, the fog cell (2.5D), a tree's sway
//! phase and, per copy (the whole sprite, 1/2 and 1/4), the plain and the night's whole shapes with their texture
//! coordinates: the parts of a quad that do not depend on the scale. Keep them in step with `world_view::object_sprite_origin`,
//! `object_picture`, `object_fog`, `draw_object` and `sharp::fill_sprite_mesh`.

use std::rc::Rc;

use macroquad::miniquad::TextureId;
use macroquad::prelude::*;
use razdor::local;
use razdor::rules::map::{object_class, Decoration, Grid, Tile, TileMap};

use super::super::dt_art::{Atlas, Slot};
use super::{lit_whole_if, object_fog, plant_shift, Camera};

/// A slot and its texture coordinates `(u0, v0, u1, v1)` in the atlas (`fill_sprite_mesh`'s and `draw_leaning`'s).
#[derive(Clone, Copy)]
pub(super) struct Shape {
    pub slot: Slot,
    pub uv: [f32; 4],
}

/// The shapes of one copy: `plain` as `draw_object` draws it, `lit` as `lit_whole_if` makes it at night, and `tree`, the whole
/// box a swaying tree is drawn in (`draw_leaning`'s `whole`: the picture's base and texture size, the whole sprite's size).
#[derive(Clone, Copy)]
pub(super) struct Pic {
    pub plain: Shape,
    pub lit: Shape,
    pub tree: Shape,
}

/// One object's static record: what its quad depends on besides the camera (see the module's note).
#[derive(Clone, Copy)]
pub(super) struct Rec {
    /// The sprite's corner in the cell's frame: `(x + 0.5, y + row_height / 2)` (`object_sprite_origin`).
    pub corner: (f32, f32),
    /// The corner's offset in sprite pixels: `(dx − w − 0.5, dy − h − 0.5)` for the whole sprite's `w`, `h`.
    pub offset: Vec2,
    /// The fog cell of the sprite (`object_fog`), read by the 2.5D view (the flat view's sprites take no fog).
    pub fog: Tile,
    /// A tree's sway phase (`TREES` only: the others are not swaying).
    pub tree: Option<i32>,
    /// The whole sprite, then its copies at 1/2 and 1/4 (the whole one where a copy is missing).
    pub pics: [Pic; 3],
}

/// The index of the copy a sprite of `divisor` is drawn from (`sprite_lod_divisor`): 0 whole, 1 half, 2 quarter.
pub(super) fn copy_index(divisor: Option<u32>) -> usize {
    match divisor {
        Some(2) => 1,
        Some(4) => 2,
        _ => 0,
    }
}

/// The texture coordinates of `rect` in a texture of `tex` texels (the arithmetic of `fill_sprite_mesh` and `draw_leaning`).
fn uv_of(rect: Rect, tex: Vec2) -> [f32; 4] {
    let (u0, v0) = (rect.x / tex.x, rect.y / tex.y);
    let (u1, v1) = ((rect.x + rect.w) / tex.x, (rect.y + rect.h) / tex.y);
    [u0, v0, u1, v1]
}

fn shape(slot: Slot, tex: Vec2) -> Shape {
    Shape { slot, uv: uv_of(slot.rect, tex) }
}

fn rec_of(o: &Decoration, atlas: &Atlas, cam: &Camera, tex: Vec2) -> Option<Rec> {
    let r = atlas.decoration(o.class, o.sprite)?;
    let (x, y) = cam.grid.center(o.tile);
    let (dx, dy) = match o.class {
        1..=8 => (16.0, 11.0),
        _ => plant_shift(o),
    };
    let pic = |s: Slot| {
        // `lit_whole_if(s, true)`: the whole sprite's box (its base and texture size), as the night's draw takes it.
        let lit = lit_whole_if(s, true);
        // `draw_leaning`'s whole box: the same rectangle as the lit one, at the whole sprite's size.
        let whole = Slot { rect: Rect::new(s.base.x, s.base.y, s.tex.x, s.tex.y), off: Vec2::ZERO, size: s.full, ..s };
        Pic { plain: shape(s, tex), lit: shape(lit, tex), tree: shape(whole, tex) }
    };
    let copy = |d: u32| atlas.decoration_lod(o.class, o.sprite, d).unwrap_or(r);
    Some(Rec {
        corner: (x + 0.5, y + cam.grid.row_height() / 2.0),
        offset: vec2(dx - r.full.x - 0.5, dy - r.full.y - 0.5),
        fog: object_fog(o, cam),
        tree: (o.class == object_class::TREES).then_some(o.jitter.2),
        pics: [pic(r), pic(copy(2)), pic(copy(4))],
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

pub(super) type Records = Rc<Vec<Option<Rec>>>;

local! {
    /// The records of the last map, kept while their key holds.
    static RECS: std::cell::RefCell<Option<(Key, Records)>> = const { std::cell::RefCell::new(None) };
}

/// The records of the map's objects (index = the object's index), made again when the key changes. `tex` is the atlas
/// texture's size in texels (`sharp::texels`).
pub(super) fn records(map: &TileMap, atlas: &Atlas, cam: &Camera, tex: Vec2) -> Records {
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
        let recs: Records = Rc::new(map.objects.iter().map(|o| rec_of(o, atlas, cam, tex)).collect());
        *c = Some((key, recs.clone()));
        recs
    })
}

/// The quad of a plain sprite as `sharp::draw` makes it (`fill_sprite_mesh`, rotation 0) into `mesh`: its corners at `origin`
/// (the picture's top-left) with the shape's offset and size, scaled by `zoom`. The vertices' normal is the draw's scale times
/// the factors `factors` (perspective, raster), and the fog cell `fog` (zero where none). `color` is the sprite's colour.
pub(super) fn plain_quad(mesh: &mut Mesh, sh: &Shape, origin: Vec2, zoom: f32, color: Color, fog: Vec2, factors: (f32, f32)) {
    let src = sh.slot.rect;
    let at = origin + sh.slot.off * zoom;
    let (x, y) = (at.x, at.y);
    let size = sh.slot.size * zoom;
    let (w, h) = (size.x, size.y);
    let pivot = vec2(x + w / 2., y + h / 2.);
    // The rotation is 0: its sine and cosine are exactly 0 and 1.
    let (sin_r, cos_r) = (0.0_f32, 1.0_f32);
    let corner = |px: f32, py: f32| {
        let p = vec2(px, py) - pivot;
        vec2(p.x * cos_r - p.y * sin_r, p.x * sin_r + p.y * cos_r) + pivot
    };
    // `draw_scale`: the destination over the source, times the perspective and the raster factors, in this order.
    let scale = size.abs() / src.size() * factors.0 * factors.1;
    let normal = vec4(scale.x, scale.y, fog.x, fog.y);
    let c: [u8; 4] = color.into();
    let [u0, v0, u1, v1] = sh.uv;
    let v = |p: Vec2, u: f32, v: f32| Vertex { position: vec3(p.x, p.y, 0.0), uv: vec2(u, v), color: c, normal };
    mesh.vertices.clear();
    mesh.vertices.extend([v(corner(x, y), u0, v0), v(corner(x + w, y), u1, v0), v(corner(x + w, y + h), u1, v1), v(corner(x, y + h), u0, v1)]);
}

/// The quad of a swaying tree as `draw_leaning` makes it (its whole box, the top edge moved by `lean` px, the foot not) into
/// `mesh`, with the sharp scale and the fog cell in the normals (`draw_mesh_sharp`'s session path).
pub(super) fn tree_quad(mesh: &mut Mesh, sh: &Shape, origin: Vec2, zoom: f32, lean: f32, color: Color, fog: Vec2) {
    let src = sh.slot.rect;
    let [u0, v0, u1, v1] = sh.uv;
    let at = origin + sh.slot.off * zoom;
    let size = sh.slot.size * zoom;
    let share = |y: f32| lean * (1.0 - y / sh.slot.full.y);
    let (top, bottom) = (share(sh.slot.off.y), share(sh.slot.off.y + sh.slot.size.y));
    // The scale in the vertices' normals (`draw_leaning`, then `draw_mesh_sharp`'s `abs`).
    let scale = vec2(size.x / src.w, size.y / src.h).abs();
    let normal = vec4(scale.x, scale.y, fog.x, fog.y);
    let c: [u8; 4] = color.into();
    let v = |x: f32, y: f32, u: f32, v: f32| Vertex { position: vec3(x, y, 0.0), uv: vec2(u, v), color: c, normal };
    mesh.vertices.clear();
    mesh.vertices.extend([
        v(at.x + top, at.y, u0, v0),
        v(at.x + size.x + top, at.y, u1, v0),
        v(at.x + size.x + bottom, at.y + size.y, u1, v1),
        v(at.x + bottom, at.y + size.y, u0, v1),
    ]);
}
