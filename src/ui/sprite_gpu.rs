//! The map sprites' placement as the GPU computes it (backlog 1605, step 5): the billboard's terms, which a vertex
//! shader takes as uniforms, and a sprite's corners in world units, which a vertex carries.
//!
//! The CPU places a standing sprite with `standing_model` (a billboard about its foot, `view25::billboard_model_about`)
//! and projects its picture's corners through the view (`picture_quad`). Both are linear in the picture's own pixels once
//! the scale is divided out: a corner is `foot + (corner − foot)` with `corner − foot` in world units, so the scale and
//! the origin cancel, and the sprite size is one uniform. A map sprite's base is its foot (`standing_map`), so there is no
//! base term. [`Billboard::point`] is the shader's arithmetic; the harness in `world_view`'s tests compares it with the
//! CPU's quads for every sprite class.
//!
//! No macroquad here: this module is pure. `world_view`'s `gpu_object` packs the vertices and `sharp`'s
//! `SHARP_VERTEX_BILLBOARD` is the shader that runs [`Billboard::point`] (backlog 1605, step 5a).

use macroquad::prelude::{vec3, Vec3};
#[cfg(test)]
use macroquad::prelude::{vec2, Mat4, Rect, Vec2};

use super::view25;

/// The billboard's terms of a camera: the ground's right and the camera's up, and the sprite size. The shader's uniforms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Billboard {
    /// The ground's right along the view (`(r.x, 0, r.y)`).
    pub right: Vec3,
    /// The camera's up on the sprite's plane (`(f.x·sin, cos, f.y·sin)`).
    pub up: Vec3,
    /// The sprite size factor (`sprite_size`).
    pub size: f32,
}

impl Billboard {
    /// The billboard of a camera at `pitch_deg` and `yaw_deg` with sprite size `size` (as `billboard_model_at`).
    pub fn new(pitch_deg: f32, yaw_deg: f32, size: f32) -> Billboard {
        let (s, c) = super::memo::sin_cos_deg(pitch_deg);
        let (f, r) = (view25::forward(yaw_deg), view25::right(yaw_deg));
        Billboard { right: vec3(r.x, 0.0, r.y), up: vec3(f.x * s, c, f.y * s), size }
    }

    /// The world point of a sprite vertex: `foot` is the ground point the sprite stands on (world units, y 0 on the
    /// flat ground) and `rel` the vertex's offset from the foot in world units (the picture's corner, `quad_rel`).
    /// The billboard scales `rel` by the size: `foot + size·rel.x·right − size·rel.y·up`.
    /// (The shader's arithmetic, `sharp::SHARP_VERTEX_BILLBOARD`: the live path has no CPU copy of it; the test does.)
    #[cfg(test)]
    pub fn point(&self, foot: Vec3, rel: Vec2) -> Vec3 {
        foot + self.right * (self.size * rel.x) - self.up * (self.size * rel.y)
    }
}

/// The corners of a picture relative to its foot, in world units: the picture's top-left corner `tl` (relative to the foot)
/// and its size in pixels `size_px`, at `px_per_unit` pixels per world unit. The order is `picture_quad`'s: top left, top
/// right, bottom right, bottom left.
#[cfg(test)]
pub fn quad_rel(tl: Vec2, size_px: Vec2, px_per_unit: f32) -> [Vec2; 4] {
    let d = size_px / px_per_unit;
    [tl, tl + vec2(d.x, 0.0), tl + d, tl + vec2(0.0, d.y)]
}

/// The projection of a world point through a view-projection matrix into the window's pixels of `frame`, as
/// `view25::project_with` (the GPU's `Projection`, then the window's mapping). None behind the eye.
#[cfg(test)]
pub fn project(view_proj: Mat4, p: Vec3, frame: Rect) -> Option<Vec2> {
    view25::project_with(view_proj, p, frame)
}
