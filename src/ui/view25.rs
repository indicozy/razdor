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
}

impl TiltCamera {
    /// The camera at `pitch_deg` and `yaw_deg` looking at `target`, at the distance where the
    /// ground at the target is drawn `scale` pixels per world unit in x: the same size as the
    /// flat view's cells at that scale. `frame` is the view's rectangle in pixels.
    pub fn new(target: Vec2, scale: f32, frame: Rect, pitch_deg: f32, yaw_deg: f32) -> TiltCamera {
        let half_fov = (FOV_DEG / 2.0).to_radians();
        TiltCamera { target, distance: frame.h / 2.0 / (half_fov.tan() * scale), pitch_deg: pitch_deg.clamp(PITCH_MIN, PITCH_MAX), yaw_deg }
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
        Mat4::perspective_rh_gl(FOV_DEG.to_radians(), aspect, near, far) * view
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
    let (s, c) = pitch_deg.to_radians().sin_cos();
    let (f, r) = (forward(yaw_deg), right(yaw_deg));
    let up = vec3(f.x * s, c, f.y * s);
    let right3 = vec3(r.x, 0.0, r.y);
    let k = size / scale;
    let ground = vec3((foot.x + origin.x) / scale, 0.0, (foot.y + origin.y) / scale);
    let at = ground - right3 * foot.x * k + up * foot.y * k;
    Mat4::from_cols(right3.extend(0.0) * k, (-up).extend(0.0) * k, Vec4::ZERO, at.extend(1.0))
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
}
