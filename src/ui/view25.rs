//! The tilted camera of the 2.5D world map (`world_view`): pure maths, no drawing.
//!
//! The 3D world: X is the world x (cells), Z the world y (world units, south is +Z) and Y up.
//! A ground point `(x, y)` is `(x, 0, y)`. The camera looks at its target from above and behind
//! it; its pitch is the angle of the view line below the horizon (90 is straight down) and its
//! vertical field of view [`FOV_DEG`]. Its yaw turns it around the target: at 0 the eye is south
//! of the target and north is up the screen; at 90 the eye is east of it (positive yaw turns the
//! eye from the south toward the east, the map as Don't Starve Together turns it).
//!
//! Standing sprites are billboards: a plane through the sprite's foot that faces the eye, its
//! up the camera's up, so a sprite is drawn at its own size where the target is (see
//! [`billboard_model`]).

use macroquad::math::{vec2, vec3, vec4, Mat4, Rect, Vec2, Vec3, Vec4};

/// The pitch range of the view, in degrees, and where it starts.
pub const PITCH_MIN: f32 = 35.0;
pub const PITCH_MAX: f32 = 75.0;
pub const PITCH_DEFAULT: f32 = 55.0;
/// The vertical field of view, in degrees.
pub const FOV_DEG: f32 = 35.0;
/// How far from the target the ground is looked for, in world units (the far limit of the
/// footprint, and of the ground the view draws).
pub const FAR: f32 = 240.0;
/// The pitch range of a [`TiltCamera::free`] camera (the trailer's, dev-only): from a low chase
/// view to the plain top-down one (at exactly 90 `look_at` with up = Y is degenerate).
pub const FREE_PITCH_MIN: f32 = 5.0;
pub const FREE_PITCH_MAX: f32 = 89.9;

/// The horizontal direction the camera looks in at `yaw_deg`, as a ground vector (world x, z):
/// north `(0, -1)` at yaw 0, east `(-1, 0)` at 90.
pub fn forward(yaw_deg: f32) -> Vec2 {
    let (s, c) = yaw_deg.to_radians().sin_cos();
    vec2(-s, -c)
}

/// The ground's right on the screen at `yaw_deg`, as a ground vector: east `(1, 0)` at yaw 0.
pub fn right(yaw_deg: f32) -> Vec2 {
    let (s, c) = yaw_deg.to_radians().sin_cos();
    vec2(c, -s)
}

/// Where north is on the screen of `frame` as the camera projects it: the unit direction, in
/// screen pixels (y down), from the target to a point a step north of it (up at yaw 0, clockwise
/// as the map turns). The compass needle's direction; None when the target is behind the eye.
pub fn north_on_screen(cam: &TiltCamera, frame: Rect) -> Option<Vec2> {
    let t = vec3(cam.target.x, 0.0, cam.target.y);
    let here = cam.project(t, frame)?;
    let north = cam.project(t + vec3(0.0, 0.0, -1.0), frame)?;
    Some((north - here).normalize_or_zero())
}

/// The camera: where it looks, how far it stands from that point, how far it is tilted and how
/// far it is turned around it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TiltCamera {
    /// The ground point it looks at (world units).
    pub target: Vec2,
    /// The distance from the eye to the target (world units).
    pub distance: f32,
    /// The pitch in degrees, within [`PITCH_MIN`]..=[`PITCH_MAX`].
    pub pitch_deg: f32,
    /// The yaw in degrees: the turn of the eye around the target from the south (0), toward the
    /// east (positive). Any value; the maths is periodic.
    pub yaw_deg: f32,
    /// The vertical field of view in degrees ([`FOV_DEG`] unless the camera was made with another).
    pub fov_deg: f32,
    /// The bent ground of the trailer's camera (dev-only, `RAZDOR_TRAILER_CAM`); None for the
    /// player's flat ground.
    pub bend: Option<Bend>,
}

impl TiltCamera {
    /// The camera at `pitch_deg` and `yaw_deg` looking at `target`, at the distance where the
    /// ground at the target is drawn `scale` pixels per world unit in x: the same size as the
    /// flat view's cells at that scale. `frame` is the view's rectangle in pixels.
    pub fn new(target: Vec2, scale: f32, frame: Rect, pitch_deg: f32, yaw_deg: f32) -> TiltCamera {
        TiltCamera::free(target, scale, frame, pitch_deg.clamp(PITCH_MIN, PITCH_MAX), yaw_deg, FOV_DEG)
    }

    /// The camera at any pitch in [`FREE_PITCH_MIN`]..=[`FREE_PITCH_MAX`] (no player limit) and
    /// the field of view `fov_deg`, fitted to `scale` as [`TiltCamera::new`] is. The trailer's
    /// camera (`RAZDOR_TRAILER_CAM`, dev-only) is the only user.
    pub fn free(target: Vec2, scale: f32, frame: Rect, pitch_deg: f32, yaw_deg: f32, fov_deg: f32) -> TiltCamera {
        let fov_deg = fov_deg.clamp(5.0, 120.0);
        let half_fov = (fov_deg / 2.0).to_radians();
        TiltCamera {
            target,
            distance: frame.h / 2.0 / (half_fov.tan() * scale),
            pitch_deg: pitch_deg.clamp(FREE_PITCH_MIN, FREE_PITCH_MAX),
            yaw_deg,
            fov_deg,
            bend: None,
        }
    }

    /// Screen pixels per world unit at the target: the scale the target's ground is drawn at for
    /// this camera's distance (the flat view's `scale` at the fitted distance).
    pub fn pivot_scale(&self, frame: Rect) -> f32 {
        frame.h / 2.0 / ((self.fov_deg / 2.0).to_radians().tan() * self.distance)
    }

    /// The eye: behind the target as seen along [`forward`] (south of it at yaw 0), and above it.
    pub fn eye(&self) -> Vec3 {
        let (s, c) = self.pitch_deg.to_radians().sin_cos();
        let f = forward(self.yaw_deg);
        vec3(self.target.x - f.x * self.distance * c, self.distance * s, self.target.y - f.y * self.distance * c)
    }

    /// A ground vector of the screen's plane, turned into the ground: `c.x` along the screen's
    /// right and `c.y` toward the viewer (down the screen at yaw 0, where it is the identity).
    pub fn ground_of_screen(&self, c: Vec2) -> Vec2 {
        right(self.yaw_deg) * c.x - forward(self.yaw_deg) * c.y
    }

    /// The inverse of [`TiltCamera::ground_of_screen`]: a ground vector in the screen's plane.
    pub fn screen_of_ground(&self, g: Vec2) -> Vec2 {
        vec2(right(self.yaw_deg).dot(g), -forward(self.yaw_deg).dot(g))
    }

    /// The matrix from world space to the normalised device coordinates of `frame` (-1..1
    /// across it, y up).
    pub fn view_proj(&self, frame: Rect) -> Mat4 {
        let aspect = frame.w / frame.h;
        let (near, far) = (self.distance * 0.01, 4.0 * (self.distance + FAR));
        let eye = self.eye();
        let view = Mat4::look_at_rh(eye, vec3(self.target.x, 0.0, self.target.y), Vec3::Y);
        Mat4::perspective_rh_gl(self.fov_deg.to_radians(), aspect, near, far) * view
    }

    /// The pixel of `p` in `frame`, or None behind the eye.
    pub fn project(&self, p: Vec3, frame: Rect) -> Option<Vec2> {
        let c = self.view_proj(frame) * p.extend(1.0);
        if c.w <= 0.0 {
            return None;
        }
        let (x, y) = (c.x / c.w, c.y / c.w);
        Some(vec2(frame.x + (x + 1.0) * 0.5 * frame.w, frame.y + (1.0 - y) * 0.5 * frame.h))
    }

    /// The ray through the pixel `at` in `frame`: its start (on the near plane) and its unit
    /// direction.
    fn ray(&self, at: Vec2, frame: Rect) -> (Vec3, Vec3) {
        let ndc = vec2((at.x - frame.x) / frame.w * 2.0 - 1.0, 1.0 - (at.y - frame.y) / frame.h * 2.0);
        let inverse = self.view_proj(frame).inverse();
        let unproject = |z: f32| {
            let v = inverse * vec4(ndc.x, ndc.y, z, 1.0);
            v.truncate() / v.w
        };
        let (near, far) = (unproject(-1.0), unproject(1.0));
        (near, (far - near).normalize())
    }

    /// The ground point (world units) under the pixel `at` of `frame`: the ray's hit with
    /// Y = 0. None when the ray does not reach the ground (above the horizon).
    pub fn pick(&self, at: Vec2, frame: Rect) -> Option<Vec2> {
        let (origin, dir) = self.ray(at, frame);
        if dir.y >= -1e-6 {
            return None;
        }
        let t = -origin.y / dir.y;
        if t < 0.0 {
            return None;
        }
        let p = origin + dir * t;
        Some(vec2(p.x, p.z))
    }

    /// The ground the pixels of `area` see, in world units, as the bounding box of the ground
    /// under its corners. A corner that sees no ground takes the ground at [`FAR`] in its ray's
    /// direction, and every corner is kept within [`FAR`] of the target. `frame` is the view the
    /// camera maps (`area` may reach past it, under the bar).
    pub fn ground_footprint(&self, frame: Rect, area: Rect) -> Rect {
        let corners = [vec2(area.x, area.y), vec2(area.x + area.w, area.y), vec2(area.x + area.w, area.y + area.h), vec2(area.x, area.y + area.h)];
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for c in corners {
            let p = self.pick(c, frame).unwrap_or_else(|| {
                let (_, dir) = self.ray(c, frame);
                self.target + vec2(dir.x, dir.z).normalize_or_zero() * FAR
            });
            let p = self.target + (p - self.target).clamp_length_max(FAR);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        Rect::new(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y)
    }
}

/// Maps the normalised device coordinates of `frame` to those of a window `w` × `h` (in the
/// units of `frame`). A 3D pass draws with this matrix and no viewport, as a viewport is not
/// on every backend: the view covers its frame, and what lies past the frame (under the bar)
/// still lands on the window.
pub fn window_matrix(frame: Rect, w: f32, h: f32) -> Mat4 {
    let (sx, sy) = (frame.w / w, frame.h / h);
    let (tx, ty) = ((2.0 * frame.x + frame.w) / w - 1.0, 1.0 - (2.0 * frame.y + frame.h) / h);
    Mat4::from_cols(vec4(sx, 0.0, 0.0, 0.0), vec4(0.0, sy, 0.0, 0.0), vec4(0.0, 0.0, 1.0, 0.0), vec4(tx, ty, 0.0, 1.0))
}

/// The model matrix of the ground pass: a 2D point `p` of a camera whose origin (world pixels
/// of its top-left) is `origin` and whose scale is `scale` pixels per world unit lies on the
/// ground at `(p + origin) / scale`.
pub fn ground_model(origin: Vec2, scale: f32) -> Mat4 {
    let k = 1.0 / scale;
    Mat4::from_cols(vec4(k, 0.0, 0.0, 0.0), vec4(0.0, 0.0, k, 0.0), Vec4::ZERO, vec4(origin.x * k, 0.0, origin.y * k, 1.0))
}

/// The model matrix of a standing sprite whose foot is at the 2D point `foot` (its row is
/// the foot's): the 2D point `p` lies on the plane through the foot that faces the eye at
/// `pitch_deg` and `yaw_deg`, `size` times its own size: `(p.x - foot.x)` along the ground's
/// right and `(foot.y - p.y)` up along the camera's up `(f·sin, cos, …)` (at yaw 0 `(0, cos,
/// -sin)`), each over `scale` pixels per world unit. A sprite is then drawn where its foot stands,
/// upright, at its own size (`size` 1) or larger or smaller about the foot.
pub fn billboard_model(origin: Vec2, foot: Vec2, scale: f32, pitch_deg: f32, yaw_deg: f32, size: f32) -> Mat4 {
    let ground = vec3((foot.x + origin.x) / scale, 0.0, (foot.y + origin.y) / scale);
    billboard_model_at(foot, scale, pitch_deg, yaw_deg, size, ground)
}

/// [`billboard_model`] with the foot's point given in space: `ground` is where the foot stands
/// (the flat ground's point, or a point of a [`Bend`] sheet). The sprite keeps the camera's
/// orientation, so it faces the eye wherever its foot is.
pub fn billboard_model_at(foot: Vec2, scale: f32, pitch_deg: f32, yaw_deg: f32, size: f32, ground: Vec3) -> Mat4 {
    let (s, c) = pitch_deg.to_radians().sin_cos();
    let (f, r) = (forward(yaw_deg), right(yaw_deg));
    let up = vec3(f.x * s, c, f.y * s);
    let right3 = vec3(r.x, 0.0, r.y);
    let k = size / scale;
    let at = ground - right3 * foot.x * k + up * foot.y * k;
    Mat4::from_cols(right3.extend(0.0) * k, (-up).extend(0.0) * k, Vec4::ZERO, at.extend(1.0))
}

/// [`billboard_model`] grown about `base` instead of `foot`: the sprite turns about `foot` (its
/// point on the ground at every yaw), but a grown sprite keeps `base` on its row, the point at
/// `size` 1 stays where it is. The drawn point `p` is first moved by `d = (base − foot)(1/size − 1)`,
/// so `billboard_model(foot, size)` of `p + d` is the grown picture about `base`. With
/// `base == foot` this is [`billboard_model`].
pub fn billboard_model_about(origin: Vec2, foot: Vec2, base: Vec2, scale: f32, pitch_deg: f32, yaw_deg: f32, size: f32) -> Mat4 {
    let d = (base - foot) * (1.0 / size - 1.0);
    billboard_model(origin, foot, scale, pitch_deg, yaw_deg, size) * Mat4::from_translation(vec3(d.x, d.y, 0.0))
}

/// [`billboard_model_about`] with the foot's point given in space (see [`billboard_model_at`]).
pub fn billboard_model_about_at(foot: Vec2, base: Vec2, scale: f32, pitch_deg: f32, yaw_deg: f32, size: f32, ground: Vec3) -> Mat4 {
    let d = (base - foot) * (1.0 / size - 1.0);
    billboard_model_at(foot, scale, pitch_deg, yaw_deg, size, ground) * Mat4::from_translation(vec3(d.x, d.y, 0.0))
}

/// The ground of the trailer's bent view (`RAZDOR_TRAILER_CAM`, dev-only): the flat ground up to
/// `flat` ahead of the pivot, then a sheet that rolls up along an arc of `radius` until its slope
/// is `max_deg`, and goes straight on at that slope. Arc length is kept: a step of ground is a
/// step along the sheet. The pivot is the camera's target; `yaw_deg` turns the sheet with the
/// camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bend {
    pub pivot: Vec2,
    pub yaw_deg: f32,
    pub flat: f32,
    pub radius: f32,
    pub max_deg: f32,
}

impl Bend {
    /// The bend with `radius` at least 0.01 world units and `max_deg` within 0..=89.
    pub fn new(pivot: Vec2, yaw_deg: f32, flat: f32, radius: f32, max_deg: f32) -> Bend {
        Bend { pivot, yaw_deg, flat: flat.max(0.0), radius: radius.max(0.01), max_deg: max_deg.clamp(0.0, 89.0) }
    }

    /// The place of ground point `g` on the sheet (height 0 on the flat part).
    pub fn point(&self, g: Vec2) -> Vec3 {
        let d = g - self.pivot;
        let (f, r) = (forward(self.yaw_deg), right(self.yaw_deg));
        let (u, h) = self.profile(d.dot(f));
        let l = d.dot(r);
        vec3(self.pivot.x, 0.0, self.pivot.y) + vec3(f.x, 0.0, f.y) * u + vec3(r.x, 0.0, r.y) * l + Vec3::Y * h
    }

    /// The profile of the sheet: the distance along the forward direction and the height of the
    /// point at ground distance `s` ahead of the pivot.
    pub fn profile(&self, s: f32) -> (f32, f32) {
        if s <= self.flat {
            return (s, 0.0);
        }
        let (r, amax) = (self.radius, self.max_deg.to_radians());
        let a = (s - self.flat) / r;
        if a <= amax {
            (self.flat + r * a.sin(), r * (1.0 - a.cos()))
        } else {
            let e = (a - amax) * r;
            (self.flat + r * amax.sin() + e * amax.cos(), r * (1.0 - amax.cos()) + e * amax.sin())
        }
    }

    /// The ground the sheet covers: the bounding box of the rectangle `width` either side of the
    /// pivot, `ahead` in front of it and `behind` behind it (along the view's forward and right).
    pub fn region(&self, width: f32, ahead: f32, behind: f32) -> Rect {
        let (f, r) = (forward(self.yaw_deg), right(self.yaw_deg));
        let mut lo = Vec2::splat(f32::MAX);
        let mut hi = Vec2::splat(f32::MIN);
        for s in [-behind, ahead] {
            for l in [-width, width] {
                let p = self.pivot + f * s + r * l;
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        Rect::new(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y)
    }
}

/// The trailer's camera, parsed from `RAZDOR_TRAILER_CAM` (dev-only, for the trailer's stills):
/// `key=value` pairs, comma-separated, every key optional. `pitch`, `fov` and `dist` replace the
/// player's camera; `bend` (degrees, 0 is no bend), `flat`, `radius`, `width`, `ahead` and
/// `behind` shape the ground ([`Bend`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trailer {
    /// The view line below the horizontal, in degrees (5 to 89.9); None: the player's pitch.
    pub pitch: Option<f32>,
    /// The vertical field of view in degrees; None: [`FOV_DEG`].
    pub fov: Option<f32>,
    /// The eye's distance from the target in world units; None: fitted to the zoom.
    pub dist: Option<f32>,
    /// The arc's full angle in degrees; 0 is no bend.
    pub bend: f32,
    /// The ground ahead of the pivot that stays flat, in world units.
    pub flat: f32,
    /// The arc's radius in world units.
    pub radius: f32,
    /// The half-width of the ground drawn across the view, in world units.
    pub width: f32,
    /// The ground drawn ahead of the pivot, and behind it, in world units.
    pub ahead: f32,
    pub behind: f32,
    /// Standing sprites whose foot is nearer the eye than this fade out (world units; 0 is off):
    /// they are faded fully by the distance and back in over the half of it beyond.
    pub near: f32,
}

impl Default for Trailer {
    fn default() -> Trailer {
        Trailer { pitch: None, fov: None, dist: None, bend: 0.0, flat: 3.0, radius: 8.0, width: 40.0, ahead: 90.0, behind: 8.0, near: 8.0 }
    }
}

/// Parses `RAZDOR_TRAILER_CAM` (see [`Trailer`]). An unknown key or a value out of its range is
/// an error naming the item.
pub fn parse_trailer(text: &str) -> Result<Trailer, String> {
    let mut t = Trailer::default();
    for item in text.split(',').map(str::trim).filter(|i| !i.is_empty()) {
        let (key, value) = item.split_once('=').ok_or_else(|| format!("`{item}`: not key=value"))?;
        let key = key.trim();
        let v: f32 = value.trim().parse().map_err(|_| format!("`{item}`: not a number"))?;
        let ok = match key {
            "pitch" => (FREE_PITCH_MIN..=FREE_PITCH_MAX).contains(&v),
            "fov" => (5.0..=120.0).contains(&v),
            "dist" => v > 0.0,
            "bend" => (0.0..=89.0).contains(&v),
            "flat" | "ahead" | "behind" | "near" => v >= 0.0,
            "radius" | "width" => v > 0.0,
            _ => return Err(format!("`{key}`: no such key")),
        };
        if !ok {
            return Err(format!("`{item}`: out of range"));
        }
        match key {
            "pitch" => t.pitch = Some(v),
            "fov" => t.fov = Some(v),
            "dist" => t.dist = Some(v),
            "bend" => t.bend = v,
            "flat" => t.flat = v,
            "radius" => t.radius = v,
            "width" => t.width = v,
            "ahead" => t.ahead = v,
            "near" => t.near = v,
            _ => t.behind = v,
        }
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Rect = Rect { x: 0.0, y: 0.0, w: 1280.0, h: 760.0 };
    const SCALE: f32 = 32.0;

    fn cam(pitch: f32) -> TiltCamera {
        TiltCamera::new(vec2(20.5, 13.25), SCALE, FRAME, pitch, 0.0)
    }

    fn turned(pitch: f32, yaw: f32) -> TiltCamera {
        TiltCamera::new(vec2(20.5, 13.25), SCALE, FRAME, pitch, yaw)
    }

    /// The billboard matrix of the pitch-era formula (before the yaw and the size).
    fn old_billboard(origin: Vec2, foot: Vec2, scale: f32, pitch_deg: f32) -> Mat4 {
        let k = 1.0 / scale;
        let (s, c) = pitch_deg.to_radians().sin_cos();
        Mat4::from_cols(
            vec4(k, 0.0, 0.0, 0.0),
            vec4(0.0, -c * k, s * k, 0.0),
            Vec4::ZERO,
            vec4(origin.x * k, foot.y * c * k, (foot.y + origin.y) * k - foot.y * s * k, 1.0),
        )
    }

    const YAWS: [f32; 6] = [0.0, 45.0, 90.0, 135.0, 180.0, -45.0];

    fn close(a: Vec2, b: Vec2, eps: f32) -> bool {
        (a - b).length() < eps
    }

    #[test]
    fn pick_undoes_project_at_several_points_and_pitches() {
        let points = [vec2(20.5, 13.25), vec2(23.0, 9.5), vec2(15.0, 18.0), vec2(31.5, 14.0), vec2(8.0, 4.0)];
        for pitch in [35.0, 55.0, 75.0] {
            let c = cam(pitch);
            for p in points {
                let px = c.project(vec3(p.x, 0.0, p.y), FRAME).expect("in front of the eye");
                let back = c.pick(px, FRAME).expect("the ground under the pixel");
                assert!(close(back, p, 1e-2), "pitch {pitch}: {p:?} came back as {back:?}");
            }
        }
    }

    #[test]
    fn the_target_is_at_the_centre_of_the_frame() {
        for pitch in [35.0, 55.0, 75.0] {
            let c = cam(pitch);
            let px = c.project(vec3(c.target.x, 0.0, c.target.y), FRAME).unwrap();
            assert!(close(px, vec2(640.0, 380.0), 1e-2), "pitch {pitch}: {px:?}");
        }
    }

    #[test]
    fn the_cell_under_the_target_is_as_wide_as_the_flat_cell() {
        for pitch in [35.0, 55.0, 75.0] {
            let c = cam(pitch);
            let t = vec3(c.target.x, 0.0, c.target.y);
            let a = c.project(t, FRAME).unwrap();
            let b = c.project(t + vec3(1.0, 0.0, 0.0), FRAME).unwrap();
            assert!(((b - a).length() - SCALE).abs() < 0.05, "pitch {pitch}: {}", (b - a).length());
        }
    }

    #[test]
    fn north_is_up_east_is_right_and_up_is_up() {
        let c = cam(55.0);
        let t = vec3(c.target.x, 0.0, c.target.y);
        let here = c.project(t, FRAME).unwrap();
        assert!(c.project(t + vec3(0.0, 0.0, -2.0), FRAME).unwrap().y < here.y, "north is up the screen");
        assert!(c.project(t + vec3(2.0, 0.0, 0.0), FRAME).unwrap().x > here.x, "east is right of the screen");
        assert!(c.project(t + vec3(0.0, 1.0, 0.0), FRAME).unwrap().y < here.y, "Y is up");
    }

    #[test]
    fn the_footprint_contains_the_target_and_is_wider_at_the_far_edge() {
        for pitch in [35.0, 55.0, 75.0] {
            let c = cam(pitch);
            let f = c.ground_footprint(FRAME, FRAME);
            assert!(f.contains(c.target), "pitch {pitch}: {f:?}");
            let far = c.pick(vec2(0.0, 0.0), FRAME).unwrap().distance(c.pick(vec2(FRAME.w, 0.0), FRAME).unwrap());
            let near = c.pick(vec2(0.0, FRAME.h), FRAME).unwrap().distance(c.pick(vec2(FRAME.w, FRAME.h), FRAME).unwrap());
            assert!(far > near, "pitch {pitch}: far edge {far} should be wider than near edge {near}");
        }
    }

    #[test]
    fn the_footprint_reaches_past_the_frame_under_the_bar() {
        let c = cam(55.0);
        let under = Rect::new(0.0, 0.0, FRAME.w, FRAME.h + 42.0);
        let inside = c.ground_footprint(FRAME, FRAME);
        let all = c.ground_footprint(FRAME, under);
        assert!(all.y + all.h > inside.y + inside.h, "the strip under the bar sees more ground to the south");
    }

    #[test]
    fn the_pitch_is_kept_in_range() {
        assert_eq!(cam(10.0).pitch_deg, PITCH_MIN);
        assert_eq!(cam(90.0).pitch_deg, PITCH_MAX);
        assert_eq!(cam(55.0).pitch_deg, 55.0);
    }

    #[test]
    fn the_window_matrix_maps_the_frame_into_the_window() {
        // The frame's centre is the window's (1280 x 800) centre horizontally, and 380 of 800
        // pixels down.
        let m = window_matrix(FRAME, 1280.0, 800.0);
        let centre = m * vec4(0.0, 0.0, 0.0, 1.0);
        assert!(centre.x.abs() < 1e-6, "{centre:?}");
        assert!((centre.y - (1.0 - 760.0 / 800.0)).abs() < 1e-6, "{centre:?}");
        // The frame's top-left corner is the window's top-left corner.
        let corner = m * vec4(-1.0, 1.0, 0.0, 1.0);
        assert!((corner.x + 1.0).abs() < 1e-6 && (corner.y - 1.0).abs() < 1e-6, "{corner:?}");
    }

    #[test]
    fn the_ground_model_puts_a_pixel_on_its_cell() {
        let m = ground_model(vec2(100.0, 64.0), SCALE);
        let p = m * vec4(32.0, 32.0, 0.0, 1.0);
        assert!(close(vec2(p.x, p.z), vec2(4.125, 3.0), 1e-5), "{p:?}");
        assert!(p.y.abs() < 1e-6);
    }

    #[test]
    fn at_yaw_zero_the_billboard_is_the_pitch_era_formula() {
        for pitch in [35.0, 55.0, 75.0] {
            for (origin, foot) in [(vec2(0.0, 0.0), vec2(0.0, 400.0)), (vec2(100.0, 64.0), vec2(230.0, 175.5))] {
                let (new, old) = (billboard_model(origin, foot, SCALE, pitch, 0.0, 1.0), old_billboard(origin, foot, SCALE, pitch));
                for p in [vec4(0.0, 0.0, 0.0, 1.0), vec4(12.0, 300.0, 0.0, 1.0), vec4(-40.0, 9.0, 0.0, 1.0)] {
                    let (a, b) = (new * p, old * p);
                    assert!((a - b).length() < 1e-4, "pitch {pitch}: {a:?} vs {b:?}");
                }
            }
        }
    }

    #[test]
    fn the_eye_turns_from_the_south_toward_the_east() {
        let c = turned(55.0, 90.0);
        let eye = c.eye() - vec3(c.target.x, 0.0, c.target.y);
        assert!(eye.x > 0.0 && eye.z.abs() < 1e-3, "yaw 90: the eye is east of the target: {eye:?}");
        let c = turned(55.0, 45.0);
        let eye = c.eye() - vec3(c.target.x, 0.0, c.target.y);
        assert!(eye.x > 0.0 && eye.z > 0.0 && (eye.x - eye.z).abs() < 1e-3, "yaw 45: south-east: {eye:?}");
    }

    #[test]
    fn the_target_is_at_the_centre_of_the_frame_at_every_yaw() {
        for yaw in YAWS {
            for pitch in [35.0, 55.0, 75.0] {
                let c = turned(pitch, yaw);
                let px = c.project(vec3(c.target.x, 0.0, c.target.y), FRAME).unwrap();
                assert!(close(px, vec2(640.0, 380.0), 1e-2), "yaw {yaw} pitch {pitch}: {px:?}");
            }
        }
    }

    #[test]
    fn the_screen_turns_with_the_yaw_and_forward_is_up_the_screen() {
        for yaw in YAWS {
            let c = turned(55.0, yaw);
            let t = vec3(c.target.x, 0.0, c.target.y);
            let here = c.project(t, FRAME).unwrap();
            let f = forward(yaw);
            let far = c.project(t + vec3(f.x, 0.0, f.y) * 2.0, FRAME).unwrap();
            assert!(far.y < here.y, "yaw {yaw}: forward is up the screen");
            let r = right(yaw);
            let east = c.project(t + vec3(r.x, 0.0, r.y) * 2.0, FRAME).unwrap();
            assert!(east.x > here.x, "yaw {yaw}: the screen's right is right");
        }
        // At 90 north is on the right of the screen and east is toward the viewer.
        let c = turned(55.0, 90.0);
        let t = vec3(c.target.x, 0.0, c.target.y);
        let here = c.project(t, FRAME).unwrap();
        assert!(c.project(t + vec3(0.0, 0.0, -2.0), FRAME).unwrap().x > here.x, "north is right at yaw 90");
    }

    #[test]
    fn a_positive_yaw_turns_the_ground_clockwise_on_the_screen() {
        // Clockwise on the screen: north (up at yaw 0) moves right and east (right) moves down.
        let c = turned(55.0, 45.0);
        let t = vec3(c.target.x, 0.0, c.target.y);
        let here = c.project(t, FRAME).unwrap();
        let north = c.project(t + vec3(0.0, 0.0, -2.0), FRAME).unwrap();
        let east = c.project(t + vec3(2.0, 0.0, 0.0), FRAME).unwrap();
        assert!(north.x > here.x && north.y < here.y, "north moved up and right: {north:?} from {here:?}");
        assert!(east.y > here.y && east.x > here.x, "east moved down and right: {east:?} from {here:?}");
    }

    #[test]
    fn pick_undoes_project_at_every_yaw() {
        let points = [vec2(20.5, 13.25), vec2(23.0, 9.5), vec2(15.0, 18.0), vec2(31.5, 14.0), vec2(8.0, 4.0)];
        for yaw in [45.0, 90.0] {
            for pitch in [35.0, 55.0, 75.0] {
                let c = turned(pitch, yaw);
                for p in points {
                    let px = c.project(vec3(p.x, 0.0, p.y), FRAME).expect("in front of the eye");
                    let back = c.pick(px, FRAME).expect("the ground under the pixel");
                    assert!(close(back, p, 1e-2), "yaw {yaw} pitch {pitch}: {p:?} came back as {back:?}");
                }
            }
        }
    }

    #[test]
    fn the_footprint_contains_the_target_at_every_yaw() {
        for yaw in YAWS {
            let c = turned(55.0, yaw);
            assert!(c.ground_footprint(FRAME, FRAME).contains(c.target), "yaw {yaw}");
        }
    }

    #[test]
    fn the_compass_needle_points_where_north_is_on_the_map_at_every_yaw() {
        // Up at yaw 0, right at 90, and clockwise as the map turns (north moves up and right at 45).
        let up = north_on_screen(&turned(55.0, 0.0), FRAME).unwrap();
        assert!(up.x.abs() < 1e-3 && up.y < -0.99, "up at yaw 0: {up:?}");
        let right = north_on_screen(&turned(55.0, 90.0), FRAME).unwrap();
        assert!(right.x > 0.99 && right.y.abs() < 1e-3, "right at yaw 90: {right:?}");
        let diagonal = north_on_screen(&turned(55.0, 45.0), FRAME).unwrap();
        assert!(diagonal.x > 0.0 && diagonal.y < 0.0, "up and right at 45: {diagonal:?}");
        // Whatever the yaw, the needle is the screen's direction of north from the target.
        for yaw in [0.0, 45.0, 90.0, 135.0, 180.0, -45.0, -90.0] {
            let c = turned(55.0, yaw);
            let t = vec3(c.target.x, 0.0, c.target.y);
            let here = c.project(t, FRAME).unwrap();
            let north = c.project(t + vec3(0.0, 0.0, -2.0), FRAME).unwrap();
            let got = north_on_screen(&c, FRAME).unwrap();
            assert!((got - (north - here).normalize()).length() < 1e-3, "yaw {yaw}: {got:?}");
        }
    }

    #[test]
    fn a_billboard_stands_upright_at_every_yaw_and_its_foot_is_on_the_ground() {
        for yaw in YAWS {
            let pitch = 55.0;
            let c = turned(pitch, yaw);
            // A foot at the target (the 2D point whose ground is the target, with no origin), and
            // the sprite's top 300 px above it.
            let foot_2d = c.target * SCALE;
            let m = billboard_model(vec2(0.0, 0.0), foot_2d, SCALE, pitch, yaw, 1.0);
            let foot = m * vec4(foot_2d.x, foot_2d.y, 0.0, 1.0);
            assert!(foot.y.abs() < 1e-5, "yaw {yaw}: the foot is on the ground: {foot:?}");
            let top = m * vec4(foot_2d.x, foot_2d.y - 300.0, 0.0, 1.0);
            let (base, head) = (c.project(foot.truncate(), FRAME).unwrap(), c.project(top.truncate(), FRAME).unwrap());
            assert!((head.x - base.x).abs() < 1e-2, "yaw {yaw}: the top is over the foot: {head:?} vs {base:?}");
            assert!(head.y < base.y, "yaw {yaw}: up is up the screen");
            // Not mirrored: one pixel to the right of the sprite is to the right on the screen.
            let right_px = m * vec4(foot_2d.x + 1.0, foot_2d.y, 0.0, 1.0);
            assert!(c.project(right_px.truncate(), FRAME).unwrap().x > base.x, "yaw {yaw}: right is right");
        }
    }

    #[test]
    fn a_billboard_at_size_two_is_twice_as_tall_about_its_foot() {
        for yaw in YAWS {
            let m1 = billboard_model(vec2(0.0, 0.0), vec2(0.0, 400.0), SCALE, 55.0, yaw, 1.0);
            let m2 = billboard_model(vec2(0.0, 0.0), vec2(0.0, 400.0), SCALE, 55.0, yaw, 2.0);
            let foot = m2 * vec4(0.0, 400.0, 0.0, 1.0);
            assert!((foot - m1 * vec4(0.0, 400.0, 0.0, 1.0)).length() < 1e-5, "yaw {yaw}: the foot stays put");
            let (up1, up2) = (m1 * vec4(0.0, 300.0, 0.0, 1.0) - foot, m2 * vec4(0.0, 300.0, 0.0, 1.0) - foot);
            assert!((up2 - up1 * 2.0).length() < 1e-4, "yaw {yaw}: twice the height");
        }
    }

    #[test]
    fn a_ground_drag_follows_the_pointer_at_every_yaw() {
        // A screen move of a few pixels near the target moves the ground under the pointer by
        // the screen delta turned into the ground (the drag's rule).
        let pitch = 55.0;
        for yaw in YAWS {
            let c = turned(pitch, yaw);
            let at = vec2(640.0, 380.0);
            let d = vec2(3.0, 2.0);
            let (a, b) = (c.pick(at, FRAME).unwrap(), c.pick(at + d, FRAME).unwrap());
            let (sx, sy) = (SCALE, SCALE * pitch.to_radians().sin());
            let want = c.ground_of_screen(vec2(d.x / sx, d.y / sy));
            assert!(((b - a) - want).length() < 0.02 * d.length() / SCALE, "yaw {yaw}: {:?} vs {want:?}", b - a);
            // And the screen's coordinates are the inverse of the ground's.
            assert!((c.screen_of_ground(want) - vec2(d.x / sx, d.y / sy)).length() < 1e-5, "yaw {yaw}");
        }
    }

    #[test]
    fn a_sprite_stands_on_its_foot_at_its_own_size_facing_the_eye_and_not_mirrored() {
        let pitch = 55.0;
        // The target is the foot's cell: the foot pixel (0, 400) is the ground at (0, 12.5).
        let c = TiltCamera::new(vec2(0.0, 12.5), SCALE, FRAME, pitch, 0.0);
        let m = billboard_model(vec2(0.0, 0.0), vec2(0.0, 400.0), SCALE, pitch, 0.0, 1.0);
        let foot = m * vec4(0.0, 400.0, 0.0, 1.0);
        assert!(foot.y.abs() < 1e-6, "the foot is on the ground: {foot:?}");
        // One pixel up the sprite is one pixel on the screen at the target.
        let up = m * vec4(0.0, 399.0, 0.0, 1.0);
        let (base, top) = (c.project(foot.truncate(), FRAME).unwrap(), c.project(up.truncate(), FRAME).unwrap());
        assert!(((top - base).length() - 1.0).abs() < 0.05, "one pixel up is {} px", (top - base).length());
        assert!(top.y < base.y, "up is up the screen");
        // The sprite's plane faces the eye: its normal is the view direction from the target.
        let (x, up_axis) = (m * vec4(1.0, 0.0, 0.0, 0.0), m * vec4(0.0, -1.0, 0.0, 0.0));
        let normal = x.truncate().cross(up_axis.truncate()).normalize();
        let to_eye = (c.eye() - vec3(c.target.x, 0.0, c.target.y)).normalize();
        assert!(normal.dot(to_eye).abs() > 0.999, "normal {normal:?} vs eye {to_eye:?}");
        // Not mirrored: one pixel to the right is to the right on the screen.
        let right = m * vec4(1.0, 400.0, 0.0, 1.0);
        assert!(c.project(right.truncate(), FRAME).unwrap().x > base.x, "right is right");
    }

    /// A grown sprite (`billboard_model_about`) keeps its base point where it is at size 1, at
    /// every yaw; the point that stays on the foot's ground is the one shifted by `d`; and with
    /// `base == foot` it is [`billboard_model`].
    #[test]
    fn a_grown_sprite_keeps_its_base_row_and_turns_about_its_foot() {
        let (origin, foot, base) = (vec2(10.0, -4.0), vec2(40.0, 300.0), vec2(40.0, 316.0));
        let size = 1.5;
        let d = (base - foot) * (1.0 / size - 1.0);
        for yaw in [0.0, 45.0, 90.0, 180.0, -135.0] {
            let at_one = billboard_model(origin, foot, SCALE, 55.0, yaw, 1.0);
            let grown = billboard_model_about(origin, foot, base, SCALE, 55.0, yaw, size);
            // The base point is where it stands at size 1.
            let b1 = at_one.transform_point3(vec3(base.x, base.y, 0.0));
            let bg = grown.transform_point3(vec3(base.x, base.y, 0.0));
            assert!((b1 - bg).length() < 1e-4, "yaw {yaw}: base {bg:?} vs size 1 {b1:?}");
            // The sprite point `foot − d` stands on the foot's ground point at every yaw.
            let fp = grown.transform_point3(vec3((foot - d).x, (foot - d).y, 0.0));
            let ground = vec3((foot.x + origin.x) / SCALE, 0.0, (foot.y + origin.y) / SCALE);
            assert!((fp - ground).length() < 1e-4, "yaw {yaw}: pivot {fp:?} vs ground {ground:?}");
            // With base == foot it is the plain billboard, at any size.
            let same = billboard_model_about(origin, foot, foot, SCALE, 55.0, yaw, size);
            let plain = billboard_model(origin, foot, SCALE, 55.0, yaw, size);
            assert!(same.abs_diff_eq(plain, 1e-5), "yaw {yaw}: base == foot is the plain billboard");
        }
    }

    fn bend(max_deg: f32) -> Bend {
        Bend::new(vec2(20.5, 13.25), 0.0, 3.0, 8.0, max_deg)
    }

    #[test]
    fn the_bend_is_the_identity_on_the_flat_part() {
        let b = bend(78.0);
        for s in [-30.0, -5.0, 0.0, 2.9, 3.0] {
            let g = b.pivot + forward(0.0) * s + right(0.0) * 4.0;
            let p = b.point(g);
            assert!((p - vec3(g.x, 0.0, g.y)).length() < 1e-4, "s {s}: {p:?} vs {g:?}");
        }
    }

    #[test]
    fn the_bend_is_continuous_at_both_joins() {
        let b = bend(78.0);
        let eps = 1e-3;
        let join_flat = b.flat;
        let join_arc = b.flat + b.radius * 78f32.to_radians();
        for join in [join_flat, join_arc] {
            let (lo, hi) = (b.profile(join - eps), b.profile(join + eps));
            assert!((lo.0 - hi.0).abs() < 0.01 && (lo.1 - hi.1).abs() < 0.01, "join at {join}: {lo:?} {hi:?}");
        }
    }

    #[test]
    fn the_slope_beyond_the_arc_is_the_maximum_angle() {
        for max_deg in [30.0, 78.0] {
            let b = bend(max_deg);
            let (a, c) = (b.profile(60.0), b.profile(80.0));
            let slope = (c.1 - a.1) / (c.0 - a.0);
            assert!((slope - max_deg.to_radians().tan()).abs() < 1e-3, "max {max_deg}: slope {slope}");
        }
    }

    #[test]
    fn arc_length_is_kept_along_the_sheet() {
        // A small ground step gives a sheet step of the same length, flat and on the arc.
        let b = bend(78.0);
        for s in [0.0, 5.0, 8.0, 12.0, 30.0] {
            let step = 0.01;
            let g = b.pivot + forward(0.0) * s;
            let next = g + forward(0.0) * step;
            let d = (b.point(next) - b.point(g)).length();
            assert!((d - step).abs() < 1e-3, "at {s}: a step of {step} became {d}");
        }
    }

    #[test]
    fn at_zero_degrees_the_bend_is_the_flat_ground_everywhere() {
        let b = bend(0.0);
        for s in [-20.0, 0.0, 5.0, 40.0, 150.0] {
            let g = b.pivot + forward(0.0) * s + right(0.0) * -7.0;
            let p = b.point(g);
            assert!((p - vec3(g.x, 0.0, g.y)).length() < 1e-3, "s {s}: {p:?} vs {g:?}");
        }
    }

    #[test]
    fn the_bend_turns_with_the_yaw_and_its_region_contains_the_pivot() {
        let b = Bend::new(vec2(20.5, 13.25), 90.0, 3.0, 8.0, 78.0);
        // Within the flat part ahead at yaw 90 (west) the sheet is the ground itself.
        let g = b.pivot + forward(90.0) * 2.0 + right(90.0) * 3.0;
        assert!((b.point(g) - vec3(g.x, 0.0, g.y)).length() < 1e-4);
        // Past the flat part it rises: the point ahead on the arc is above the ground.
        let far = b.pivot + forward(90.0) * 12.0;
        assert!(b.point(far).y > 0.5, "{:?}", b.point(far));
        let region = bend(78.0).region(40.0, 90.0, 8.0);
        assert!(region.contains(vec2(20.5, 13.25)), "{region:?}");
        assert!(region.contains(vec2(20.5, 13.25) + forward(0.0) * 89.0), "ahead is inside: {region:?}");
        assert!(!region.contains(vec2(20.5, 13.25) + forward(0.0) * 95.0), "beyond ahead is outside");
        // Turned by 90, the region is the same ground rotated: its width is the ahead length.
        let turned = Bend::new(vec2(20.5, 13.25), 90.0, 3.0, 8.0, 78.0).region(40.0, 90.0, 8.0);
        assert!((turned.w - 98.0).abs() < 1e-3 && (turned.h - 80.0).abs() < 1e-3, "{turned:?}");
    }

    #[test]
    fn a_free_camera_keeps_its_pitch_within_the_free_range_and_its_fov() {
        let wide = TiltCamera::free(vec2(20.5, 13.25), SCALE, FRAME, 12.0, 0.0, 55.0);
        assert_eq!(wide.pitch_deg, 12.0, "below the player's limit");
        assert_eq!(wide.fov_deg, 55.0);
        assert_eq!(TiltCamera::free(vec2(0.0, 0.0), SCALE, FRAME, 2.0, 0.0, 35.0).pitch_deg, FREE_PITCH_MIN);
        assert_eq!(TiltCamera::free(vec2(0.0, 0.0), SCALE, FRAME, 90.0, 0.0, 35.0).pitch_deg, FREE_PITCH_MAX);
        // A wider lens is fitted from a nearer eye, and the player's camera is the 35° free one.
        assert!(wide.distance < cam(55.0).distance);
        assert_eq!(cam(55.0).fov_deg, FOV_DEG);
    }

    #[test]
    fn the_free_camera_still_centres_its_target_and_pick_undoes_project() {
        for (pitch, fov) in [(12.0, 55.0), (20.0, 70.0), (89.0, 35.0)] {
            let c = TiltCamera::free(vec2(20.5, 13.25), SCALE, FRAME, pitch, 0.0, fov);
            let px = c.project(vec3(c.target.x, 0.0, c.target.y), FRAME).unwrap();
            assert!(close(px, vec2(640.0, 380.0), 1e-2), "pitch {pitch} fov {fov}: {px:?}");
            let p = vec2(21.0, 12.0);
            if let Some(px) = c.project(vec3(p.x, 0.0, p.y), FRAME) {
                assert!(close(c.pick(px, FRAME).unwrap(), p, 1e-2), "pitch {pitch} fov {fov}");
            }
        }
    }

    #[test]
    fn the_trailer_keys_parse_and_the_bad_ones_are_named() {
        let t = parse_trailer("pitch=12,fov=55,dist=14,flat=3,radius=8,bend=78,width=40,ahead=90,behind=8").unwrap();
        assert_eq!((t.pitch, t.fov, t.dist, t.bend, t.flat, t.radius, t.width, t.ahead, t.behind), (Some(12.0), Some(55.0), Some(14.0), 78.0, 3.0, 8.0, 40.0, 90.0, 8.0));
        let none = parse_trailer("").unwrap();
        assert_eq!(none, Trailer::default());
        assert_eq!(none.bend, 0.0);
        assert!(parse_trailer("pitch=95").unwrap_err().contains("pitch=95"));
        assert!(parse_trailer("colour=1").unwrap_err().contains("colour"));
        assert!(parse_trailer("bend").unwrap_err().contains("key=value"));
        assert!(parse_trailer("radius=x").unwrap_err().contains("number"));
        assert!(parse_trailer("radius=0").is_err());
        assert_eq!(parse_trailer("near=6").unwrap().near, 6.0);
        assert_eq!(Trailer::default().near, 8.0);
    }

    #[test]
    fn a_sprite_on_the_sheet_stands_on_its_point_and_faces_the_eye() {
        // The foot of a sprite standing on the arc is the sheet's point, and its plane still
        // faces the eye (the same orientation as on the flat ground).
        let b = bend(78.0);
        let pitch = 12.0;
        let g = b.pivot + forward(0.0) * 13.0;
        let at = b.point(g);
        let foot = vec2(0.0, 400.0);
        let m = billboard_model_at(foot, SCALE, pitch, 0.0, 1.0, at);
        let stood = m * vec4(foot.x, foot.y, 0.0, 1.0);
        assert!((stood.truncate() - at).length() < 1e-4, "{stood:?} vs {at:?}");
        let x = m * vec4(1.0, 0.0, 0.0, 0.0);
        let up = m * vec4(0.0, -1.0, 0.0, 0.0);
        let normal = x.truncate().cross(up.truncate()).normalize();
        let eye = TiltCamera::free(b.pivot, SCALE, FRAME, pitch, 0.0, FOV_DEG).eye();
        let to_eye = (eye - vec3(b.pivot.x, 0.0, b.pivot.y)).normalize();
        assert!(normal.dot(to_eye).abs() > 0.999, "normal {normal:?}");
        // On the flat ground, the same call is the plain billboard.
        let flat_foot = vec3(foot.x / SCALE, 0.0, foot.y / SCALE);
        let flat = billboard_model_at(foot, SCALE, pitch, 0.0, 1.0, flat_foot);
        assert!(flat.abs_diff_eq(billboard_model(vec2(0.0, 0.0), foot, SCALE, pitch, 0.0, 1.0), 1e-5));
    }

    #[test]
    fn the_about_at_billboard_is_the_about_billboard_on_the_flat_ground() {
        let (origin, foot, base) = (vec2(10.0, -4.0), vec2(40.0, 300.0), vec2(40.0, 316.0));
        for yaw in [0.0, 45.0, 180.0] {
            let ground = vec3((foot.x + origin.x) / SCALE, 0.0, (foot.y + origin.y) / SCALE);
            let a = billboard_model_about_at(foot, base, SCALE, 55.0, yaw, 1.5, ground);
            let b = billboard_model_about(origin, foot, base, SCALE, 55.0, yaw, 1.5);
            assert!(a.abs_diff_eq(b, 1e-5), "yaw {yaw}");
        }
    }
}
