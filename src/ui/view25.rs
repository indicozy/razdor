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

/// The shape of the first-person view's far ground: rolled up to a top-down map («Вверх»), curved down
/// along the view like the earth's horizon, with the sky above it («Горизонт»), or curved down in every
/// direction from the hero like a small planet, its horizon a circle («Шар»).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FarGround {
    Up,
    Horizon,
    Globe,
}

/// The globe's radius («Шар», in cells) for the first-person view and the eyes view: `default` (the
/// constants below), unless `RAZDOR_VIEW_GLOBE_RADIUS` gives another (a developer override for the
/// snapshots, read once).
pub fn globe_radius(default: f32) -> f32 {
    static ENV: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    ENV.get_or_init(|| std::env::var("RAZDOR_VIEW_GLOBE_RADIUS").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|r| *r > 0.0)).unwrap_or(default)
}

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
/// The first-person view («От первого лица», the player's): its camera's pitch and field of view,
/// the ground's flat part ahead of the view's centre and the arc's radius, and the arc's angle
/// for the far ground rolling up («Вверх», the trailer's look: the sheet square-on to the eye),
/// curving down like the earth's horizon («Горизонт», the sky above it), or curving away on every
/// side like a small planet («Шар», [`FarGround::Globe`]).
pub const FP_PITCH: f32 = 12.0;
pub const FP_FOV: f32 = 55.0;
pub const FP_FLAT: f32 = 3.0;
/// The arc's radius of the roll up, and of the curve down. The roll up is wide (24 cells: a tight roll
/// turned the far ground to square-on within about ten cells, too fast to follow; the trailer's 8 was
/// that tight roll). A tight curve down (the trailer's) gives a horizon near the middle of the frame,
/// the wide curve of a planet would put it near the top and the footprint would need the far ground to
/// the horizon (about 170 units).
pub const FP_RADIUS: f32 = 24.0;
pub const FP_RADIUS_DOWN: f32 = 60.0;
/// The globe's («Шар») arc: a small planet, so its horizon is near the middle of the frame. The
/// developer override `RAZDOR_VIEW_GLOBE_RADIUS` replaces it (`view25::globe_radius`).
pub const FP_RADIUS_GLOBE: f32 = 40.0;
pub const FP_BEND_UP: f32 = 78.0;
pub const FP_BEND_DOWN: f32 = -60.0;
/// The eyes view («Глазами героя»): the eye's height above the ground (cells, the world units), its
/// pitch (degrees below the horizontal, 0 to 5 by the design) and the distance inside which a
/// standing sprite fades out (cells; `world_view::near_fade_limit`). Its ground's flat part ahead of
/// the eye and the arc of the roll up («Вверх») or the curve down («Горизонт»).
pub const EYE_HEIGHT: f32 = 0.7;
pub const EYE_PITCH: f32 = 2.0;
pub const EYE_NEAR: f32 = 2.5;
/// The width of the eyes view's fade-out band beyond [`EYE_NEAR`] (cells): a sprite is translucent over it.
pub const EYE_FADE_BAND: f32 = 0.5;
pub const EYE_FLAT: f32 = 2.5;
pub const EYE_RADIUS_UP: f32 = 24.0;
pub const EYE_RADIUS_DOWN: f32 = 60.0;
/// The globe's arc of the eyes view, as [`FP_RADIUS_GLOBE`].
pub const EYE_RADIUS_GLOBE: f32 = 150.0;
pub const EYE_BEND_UP: f32 = 78.0;
pub const EYE_BEND_DOWN: f32 = -60.0;
/// The near plane of the eyes camera (cells): the eye sees the ground under the eye's own sprites.
const EYE_CLIP: f32 = 0.05;
/// How far ahead of the eye the compass looks for north on the screen (cells): the point under
/// the eye projects far below the frame.
const EYE_COMPASS_LOOK: f32 = 3.0;

/// The horizontal direction the camera looks in at `yaw_deg`, as a ground vector (world x, z):
/// north `(0, -1)` at yaw 0, east `(-1, 0)` at 90.
pub fn forward(yaw_deg: f32) -> Vec2 {
    let (s, c) = super::memo::sin_cos_deg(yaw_deg);
    vec2(-s, -c)
}

/// The ground's right on the screen at `yaw_deg`, as a ground vector: east `(1, 0)` at yaw 0.
pub fn right(yaw_deg: f32) -> Vec2 {
    let (s, c) = super::memo::sin_cos_deg(yaw_deg);
    vec2(c, -s)
}

/// Where north is on the screen of `frame` as the camera projects it: the unit direction, in
/// screen pixels (y down), from the target to a point a step north of it (up at yaw 0, clockwise
/// as the map turns). The compass needle's direction; None when the target is behind the eye.
pub fn north_on_screen(cam: &TiltCamera, frame: Rect) -> Option<Vec2> {
    // The eyes camera's target is the ground under its eye: a point ahead of it stands in the frame.
    let anchor = if cam.eye_at.is_some() { cam.target + forward(cam.yaw_deg) * EYE_COMPASS_LOOK } else { cam.target };
    let t = vec3(anchor.x, 0.0, anchor.y);
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
    /// The eyes camera's eye (world units, y up): it stands here and looks along its pitch and yaw,
    /// not at `target` (the ground under it). None for the cameras that look at the target.
    pub eye_at: Option<Vec3>,
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
            eye_at: None,
        }
    }

    /// Screen pixels per world unit at the target: the scale the target's ground is drawn at for
    /// this camera's distance (the flat view's `scale` at the fitted distance).
    pub fn pivot_scale(&self, frame: Rect) -> f32 {
        frame.h / 2.0 / ((self.fov_deg / 2.0).to_radians().tan() * self.distance)
    }

    /// The eye: behind the target as seen along [`forward`] (south of it at yaw 0), and above it.
    pub fn eye(&self) -> Vec3 {
        if let Some(e) = self.eye_at {
            return e;
        }
        let (s, c) = super::memo::sin_cos_deg(self.pitch_deg);
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
        if let Some(eye) = self.eye_at {
            let view = Mat4::look_at_rh(eye, eye + self.look_dir(), Vec3::Y);
            return Mat4::perspective_rh_gl(self.fov_deg.to_radians(), aspect, EYE_CLIP, 4.0 * (self.distance + FAR)) * view;
        }
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
    /// Y = 0, or with the bent sheet when the camera has one ([`TiltCamera::pick_bent`]). None
    /// when the ray does not reach the ground (above the horizon).
    pub fn pick(&self, at: Vec2, frame: Rect) -> Option<Vec2> {
        let (origin, dir) = self.ray(at, frame);
        if let Some(bend) = self.bend {
            return if bend.globe { pick_globe(&bend, origin, dir, self.distance) } else { pick_bent(&bend, origin, dir, self.distance) };
        }
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

    /// The first-person camera (`RAZDOR_TRAILER_CAM` aside, the player's own): its pitch
    /// [`FP_PITCH`], field of view [`FP_FOV`], and the far ground `far` with its pivot where the
    /// camera looks. It looks at `look` (the map view's centre, the hero's place) from beyond it,
    /// so the hero stands in the lower third of `frame`: the eye is placed where the hero's ray
    /// hits the ground at the depression that puts him at two thirds of the frame's height, and
    /// the distance is fitted so the hero's ground is drawn `scale` pixels per world unit, as
    /// the flat view draws it (the cells of the rest of the frame are smaller, as they are far).
    pub fn first_person(look: Vec2, scale: f32, frame: Rect, yaw_deg: f32, far: FarGround) -> TiltCamera {
        let (p, phi) = (FP_PITCH.to_radians(), (FP_FOV / 2.0).to_radians());
        // The hero's ray below the horizontal: the view line plus the angle of a point a third of
        // the half height below the centre (NDC y -1/3).
        let hero_ray = p + (phi.tan() / 3.0).atan();
        let (sp, cp, th) = (p.sin(), p.cos(), hero_ray.tan());
        // The hero's depth along the view line: `distance · (sin p cos p / tan θ + sin² p)`.
        let d0 = frame.h / 2.0 / (phi.tan() * scale);
        let distance = d0 / (sp * cp / th + sp * sp);
        // The hero stands `off` short of the target along the view.
        let off = distance * cp - distance * sp / th;
        let target = look + forward(yaw_deg) * off;
        let mut cam = TiltCamera { target, distance, pitch_deg: FP_PITCH, yaw_deg, fov_deg: FP_FOV, bend: None, eye_at: None };
        // The sheet's pivot is the view's centre; its horizon (the down bend, the globe) is where the
        // eye's sight line grazes it.
        cam.bend = Some(match far {
            FarGround::Up => Bend::new(target, yaw_deg, FP_FLAT, FP_RADIUS, FP_BEND_UP),
            FarGround::Horizon => Bend::new(target, yaw_deg, FP_FLAT, FP_RADIUS_DOWN, FP_BEND_DOWN).with_horizon(cam.eye()),
            FarGround::Globe => cam.globe_bend(look, FP_FLAT, globe_radius(FP_RADIUS_GLOBE)),
        });
        cam
    }

    /// The eyes camera («Глазами героя», the trailer's `eye`): the eye stands on the ground at `ground`,
    /// `height` cells up, and looks along `pitch_deg` below the horizontal (0 to 89.9) and `yaw_deg`,
    /// with the field of view `fov_deg`. It has no bend: the caller adds one with [`TiltCamera::eye_bend`].
    /// Its distance is the flat view's at `scale`, a reference only (the bent picture's density and the
    /// sprites' magnification, which do not depend on it).
    pub fn eyes(ground: Vec2, height: f32, scale: f32, frame: Rect, pitch_deg: f32, yaw_deg: f32, fov_deg: f32) -> TiltCamera {
        let fov_deg = fov_deg.clamp(5.0, 120.0);
        let half_fov = (fov_deg / 2.0).to_radians();
        TiltCamera {
            target: ground,
            distance: frame.h / 2.0 / (half_fov.tan() * scale),
            pitch_deg: pitch_deg.clamp(0.0, FREE_PITCH_MAX),
            yaw_deg,
            fov_deg,
            bend: None,
            eye_at: Some(vec3(ground.x, height.max(0.0), ground.y)),
        }
    }

    /// The bend of an eyes camera: the sheet starts `flat` cells ahead of the eye and rolls along an
    /// arc of `radius` to the slope `max_deg` (negative: it curves down to the horizon, which
    /// [`Bend::with_horizon`] finds from the eye).
    pub fn eye_bend(&self, flat: f32, radius: f32, max_deg: f32) -> Bend {
        Bend::new(self.target, self.yaw_deg, flat, radius, max_deg).with_horizon(self.eye())
    }

    /// The globe of this camera (`FarGround::Globe`): the ground curves down in every direction from the
    /// target, its horizon found from the eye ([`Bend::globe`]). Both the first-person and the eyes cameras.
    pub fn globe_bend(&self, pivot: Vec2, flat: f32, radius: f32) -> Bend {
        Bend::globe(pivot, self.yaw_deg, flat, radius).with_horizon(self.eye())
    }

    /// The direction the eyes camera looks in (unit): along its pitch below the horizontal, at its yaw.
    fn look_dir(&self) -> Vec3 {
        let (s, c) = super::memo::sin_cos_deg(self.pitch_deg);
        let f = forward(self.yaw_deg);
        vec3(f.x * c, -s, f.y * c)
    }

    /// The ground the pixels of `area` see, in world units, as the bounding box of the ground
    /// under its corners. A corner that sees no ground takes the ground at [`FAR`] in its ray's
    /// direction, and every corner is kept within [`FAR`] of the target. `frame` is the view the
    /// camera maps (`area` may reach past it, under the bar).
    pub fn ground_footprint(&self, frame: Rect, area: Rect) -> Rect {
        if let Some(bend) = self.bend {
            return if bend.globe { self.globe_footprint(&bend) } else { self.bent_footprint(frame, area) };
        }
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

impl TiltCamera {
    /// The footprint of a globe: the square about its pivot that holds the whole of its sheet, out to the
    /// farthest horizon at any bearing (every ground point the frame sees lies within it). The bounding box
    /// of the frame's edges would miss the far rim beside the view's middle, and the ground picture would
    /// leave that rim as sky with the sprites on it floating.
    fn globe_footprint(&self, bend: &Bend) -> Rect {
        const BEARINGS: usize = 64;
        let far = (0..BEARINGS)
            .map(|i| {
                let a = i as f32 * std::f32::consts::TAU / BEARINGS as f32;
                bend.horizon_at(bend.pivot + vec2(a.cos(), a.sin()))
            })
            .fold(0.0f32, f32::max)
            .min(FAR);
        Rect::new(bend.pivot.x - far, bend.pivot.y - far, 2.0 * far, 2.0 * far)
    }

    /// [`TiltCamera::ground_footprint`] of a bent camera: the bounding box of the sheet the rays
    /// through the edges of `area` meet (a ray that meets none adds nothing), each point kept
    /// within [`FAR`] of the target. The sheet is not a rectangle of the screen, so the edges are
    /// sampled, not only the corners.
    fn bent_footprint(&self, frame: Rect, area: Rect) -> Rect {
        let (x0, x1, y0, y1) = (area.x, area.x + area.w, area.y, area.y + area.h);
        let edges = [(vec2(x0, y0), vec2(x1, y0)), (vec2(x1, y0), vec2(x1, y1)), (vec2(x1, y1), vec2(x0, y1)), (vec2(x0, y1), vec2(x0, y0))];
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        let mut hit = false;
        for (a, b) in edges {
            for i in 0..=8 {
                if let Some(p) = self.pick(a.lerp(b, i as f32 / 8.0), frame) {
                    let p = self.target + (p - self.target).clamp_length_max(FAR);
                    lo = lo.min(p);
                    hi = hi.max(p);
                    hit = true;
                }
            }
        }
        if !hit {
            return Rect::new(self.target.x, self.target.y, 0.0, 0.0);
        }
        Rect::new(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y)
    }
}

/// The first `t` in `0..=t_end` where `above` turns from positive to non-positive, found by marching
/// in steps of 0.1 world units and bisecting the step. None when `above` is not positive at 0 or never
/// turns, so the march stops at `t_end`.
fn first_meet(above: impl Fn(f32) -> f32, t_end: f32) -> Option<f32> {
    const MARCH_STEP: f32 = 0.1;
    if t_end <= 0.0 || above(0.0) <= 0.0 {
        return None;
    }
    let mut t0 = 0.0;
    loop {
        let t1 = (t0 + MARCH_STEP).min(t_end);
        if above(t1) <= 0.0 {
            let (mut lo, mut hi) = (t0, t1);
            for _ in 0..40 {
                let mid = 0.5 * (lo + hi);
                if above(mid) > 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            return Some(0.5 * (lo + hi));
        }
        if t1 >= t_end {
            return None;
        }
        t0 = t1;
    }
}

/// The ground point (world units) where the ray `origin + t·dir` first meets the bent sheet `b`
/// (a camera `distance` from its target). The sheet is the profile extruded along the view's
/// right, so a ray meets it where its height is the profile's height at its forward distance
/// ([`first_meet`]). None where the sheet is not seen: past the horizon, or no sheet ahead of the ray.
fn pick_bent(b: &Bend, origin: Vec3, dir: Vec3, distance: f32) -> Option<Vec2> {
    let (f, r) = (forward(b.yaw_deg), right(b.yaw_deg));
    let o = vec2(origin.x, origin.z) - b.pivot;
    let d = vec2(dir.x, dir.z);
    let (w0, dw) = (o.dot(f), d.dot(f));
    if dw <= 1e-6 {
        return None;
    }
    // The farthest forward distance the sheet can be seen at: the horizon, else the footprint's far limit.
    let w_end = if b.limit.is_finite() { b.profile(b.limit).0 } else { FAR + distance };
    // A ray that barely goes forward would march for ever: the march stops at four far limits (a
    // real hit is within a few dozen units).
    let t_end = ((w_end - w0) / dw).min(4.0 * (FAR + distance));
    let above = |t: f32| origin.y + dir.y * t - b.height_at(w0 + dw * t);
    let t = first_meet(above, t_end)?;
    let s = b.s_of_u(w0 + dw * t);
    if s > b.limit {
        return None;
    }
    let l = (o + d * t).dot(r);
    Some(b.pivot + f * s + r * l)
}

/// The sheet's height at the distance `rho` from a globe's pivot: 0 on its flat part, then the drop of
/// its arc (the arc's `sin a` is `(rho − flat) / radius`, its height `−radius (1 − cos a)`).
fn globe_drop(b: &Bend, rho: f32) -> f32 {
    if rho <= b.flat {
        return 0.0;
    }
    let x = ((rho - b.flat) / b.radius).min(1.0);
    -b.radius * (1.0 - (1.0 - x * x).sqrt())
}

/// The ground point (world units) where the ray first meets the globe `b` (a camera `distance` from its
/// target). The ray is marched only over the sheet's horizontal reach, the disc of `flat + radius`, where
/// the sheet is `globe_drop` high; outside it there is nothing to meet. The first meeting is the one seen
/// from the eye, so a point beyond the horizon, behind the globe, is never found. None where the sheet is
/// not seen.
fn pick_globe(b: &Bend, origin: Vec3, dir: Vec3, distance: f32) -> Option<Vec2> {
    let o = vec2(origin.x, origin.z) - b.pivot;
    let d = vec2(dir.x, dir.z);
    let reach = b.flat + b.radius;
    // The march ends at the disc's exit of the ray, and at four far limits (as `pick_bent`'s).
    let far = 4.0 * (FAR + distance);
    let (qa, qb, qc) = (d.dot(d), 2.0 * o.dot(d), o.dot(o) - reach * reach);
    let t_end = if qa <= 1e-12 {
        if qc > 0.0 {
            return None;
        }
        far
    } else {
        let disc = qb * qb - 4.0 * qa * qc;
        if disc < 0.0 {
            return None;
        }
        ((-qb + disc.sqrt()) / (2.0 * qa)).min(far)
    };
    let above = |t: f32| {
        let q = o + d * t;
        let rho = q.length();
        if rho > reach {
            return f32::INFINITY;
        }
        origin.y + dir.y * t - globe_drop(b, rho)
    };
    let t = first_meet(above, t_end)?;
    let q = o + d * t;
    let rho = q.length();
    // The ground distance of the sheet point at the projected distance `rho` (the inverse of the profile).
    let s = if rho <= b.flat { rho } else { b.flat + b.radius * ((rho - b.flat) / b.radius).min(1.0).asin() };
    if s > b.horizon_at(b.pivot + q) {
        return None;
    }
    let g = if rho > 1e-9 { q * (s / rho) } else { Vec2::ZERO };
    Some(b.pivot + g)
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
    let (s, c) = super::memo::sin_cos_deg(pitch_deg);
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

/// The ground of the bent views: the trailer's (`RAZDOR_TRAILER_CAM`, dev-only) and the first-person
/// view's far ground. Each is flat up to `flat` ahead of the pivot, then:
/// - an up bend rolls the sheet up along an arc of `radius` until its slope is `max_deg`, and goes
///   straight on at that slope; a down bend curves it down the same way, but only along the view's
///   forward (a cylinder: the sides stay flat);
/// - a globe ([`Bend::globe`]) curves it down in every direction from the pivot, like a small planet:
///   the same arc, but its distance is radial, so its horizon is a circle.
///
/// Arc length is kept: a step of ground is a step along the sheet. The pivot is the camera's target;
/// `yaw_deg` turns the sheet with the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bend {
    pub pivot: Vec2,
    pub yaw_deg: f32,
    pub flat: f32,
    pub radius: f32,
    /// The arc's angle and its slope beyond: positive rolls the far ground up (toward the eye's
    /// top-down), negative curves it down like the earth's horizon.
    pub max_deg: f32,
    /// The ground distance past which the sheet is hidden: the horizon of a down bend or a globe
    /// ([`Bend::with_horizon`]); for a globe it is the horizon ahead, and the sides' differ
    /// ([`Bend::horizon_at`]). Infinite for an up bend, or until the eye is given.
    pub limit: f32,
    /// Whether the sheet is a globe ([`Bend::globe`]): it curves down in every direction, and its
    /// distances are radial.
    pub globe: bool,
    /// The eye the globe's horizon is found from (world units); None until [`Bend::with_horizon`].
    pub eye: Option<Vec3>,
}

impl Bend {
    /// The bend with `radius` at least 0.01 world units and `max_deg` within -89..=89.
    pub fn new(pivot: Vec2, yaw_deg: f32, flat: f32, radius: f32, max_deg: f32) -> Bend {
        Bend { pivot, yaw_deg, flat: flat.max(0.0), radius: radius.max(0.01), max_deg: max_deg.clamp(-89.0, 89.0), limit: f32::INFINITY, globe: false, eye: None }
    }

    /// A globe («Шар»): the ground curves down in every direction from `pivot`, as on a small planet.
    /// Its flat part is the disk of `flat` and its arc `radius` long, down to a slope of 89 degrees.
    /// Its horizon comes from the eye ([`Bend::with_horizon`]).
    pub fn globe(pivot: Vec2, yaw_deg: f32, flat: f32, radius: f32) -> Bend {
        Bend { globe: true, ..Bend::new(pivot, yaw_deg, flat, radius, -89.0) }
    }

    /// The bend with its horizon for the eye at `eye`: a down bend's sheet is hidden past the
    /// point where the eye's sight line grazes it (the planet's horizon), so past it the sheet
    /// is not drawn, not picked, and its points stand on the horizon ([`Bend::point`]). A globe's
    /// horizon is found the same way in each direction ([`Bend::horizon_at`]), and its `limit` is
    /// the one ahead. An up bend has none.
    pub fn with_horizon(self, eye: Vec3) -> Bend {
        if self.globe {
            let mut bend = Bend { eye: Some(eye), ..self };
            bend.limit = bend.horizon_at(bend.pivot + forward(bend.yaw_deg));
            return bend;
        }
        if self.max_deg >= 0.0 {
            return self;
        }
        let f = forward(self.yaw_deg);
        let (eu, eh) = ((vec2(eye.x, eye.z) - self.pivot).dot(f), eye.y);
        let amax = self.max_deg.abs().to_radians();
        // Visible while the sight line is above the sheet's normal (upward): the sign of
        // `(eye − point) · normal`, with the tangent `(cos a, −sin a)` of the down bend.
        let visible = |s: f32| {
            let a = if s <= self.flat { 0.0 } else { ((s - self.flat) / self.radius).min(amax) };
            let (tu, th) = (a.cos(), -a.sin());
            let (u, h) = self.profile(s);
            (eu - u) * (-th) + (eh - h) * tu > 0.0
        };
        let step = self.radius / 16.0;
        let (mut lo, mut s) = (self.flat, self.flat);
        while s < self.flat + 4000.0 {
            if !visible(s) {
                let (mut lo_s, mut hi_s) = (lo, s);
                for _ in 0..40 {
                    let mid = 0.5 * (lo_s + hi_s);
                    if visible(mid) {
                        lo_s = mid;
                    } else {
                        hi_s = mid;
                    }
                }
                return Bend { limit: 0.5 * (lo_s + hi_s), ..self };
            }
            lo = s;
            s += step;
        }
        self
    }

    /// The distance past which the sheet is hidden in the direction of ground point `g`: [`Bend::limit`]
    /// for a down bend. For a globe, the horizon in that direction: the sight line from the eye grazes
    /// the arc at the angle `a` from its top where `A sin a + B cos a = R`, with `A` the eye's offset
    /// along the direction less the flat part and `B` the eye's height over the arc's centre (`R` below
    /// the pivot's ground). The eye is not over the pivot, so the horizon is nearest ahead of the view
    /// and farther out at the sides. Infinite until the eye is given.
    pub fn horizon_at(&self, g: Vec2) -> f32 {
        if !self.globe {
            return self.limit;
        }
        let Some(eye) = self.eye else { return f32::INFINITY };
        let d = g - self.pivot;
        let dir = if d.length_squared() > 1e-12 { d.normalize() } else { forward(self.yaw_deg) };
        let (a, b) = ((vec2(eye.x, eye.z) - self.pivot).dot(dir) - self.flat, eye.y + self.radius);
        let (r, cap) = (self.radius, self.max_deg.abs().to_radians());
        let m = (a * a + b * b).sqrt();
        let angle = if m <= r { cap } else { a.atan2(b) + (r / m).acos() };
        self.flat + r * angle.clamp(0.0, cap)
    }

    /// The distance of ground point `g` that the horizon is compared with: along the forward for a
    /// down bend (see [`Bend::hidden`]), radially for a globe.
    pub fn reach(&self, g: Vec2) -> f32 {
        let d = g - self.pivot;
        if self.globe {
            d.length()
        } else {
            d.dot(forward(self.yaw_deg))
        }
    }

    /// Whether ground point `g` is past the horizon (a down bend or a globe hides it).
    pub fn hidden(&self, g: Vec2) -> bool {
        self.reach(g) > self.horizon_at(g)
    }

    /// The place of ground point `g` on the sheet (height 0 on the flat part). Past the horizon
    /// it stands on the horizon's line.
    pub fn point(&self, g: Vec2) -> Vec3 {
        let d = g - self.pivot;
        let (f, r) = (forward(self.yaw_deg), right(self.yaw_deg));
        if self.globe {
            // The arc is radial: the point stands at its own bearing from the pivot, at the profile's distance.
            let rho = d.length();
            let dir = if rho > 1e-9 { d / rho } else { f };
            let (u, h) = self.profile(rho.min(self.horizon_at(g)));
            return vec3(self.pivot.x, 0.0, self.pivot.y) + vec3(dir.x, 0.0, dir.y) * u + Vec3::Y * h;
        }
        let (u, h) = self.profile(d.dot(f).min(self.limit));
        let l = d.dot(r);
        vec3(self.pivot.x, 0.0, self.pivot.y) + vec3(f.x, 0.0, f.y) * u + vec3(r.x, 0.0, r.y) * l + Vec3::Y * h
    }

    /// The profile of the sheet: the distance along the forward direction and the height of the
    /// point at ground distance `s` ahead of the pivot. (For a globe, the distance from the pivot.)
    pub fn profile(&self, s: f32) -> (f32, f32) {
        if s <= self.flat {
            return (s, 0.0);
        }
        let (r, amax) = (self.radius, self.max_deg.abs().to_radians());
        let sign = if self.max_deg < 0.0 { -1.0 } else { 1.0 };
        let a = (s - self.flat) / r;
        if a <= amax {
            (self.flat + r * a.sin(), sign * r * (1.0 - a.cos()))
        } else {
            let e = (a - amax) * r;
            (self.flat + r * amax.sin() + e * amax.cos(), sign * (r * (1.0 - amax.cos()) + e * amax.sin()))
        }
    }

    /// The ground distance `s` whose profile has forward distance `u` (the inverse of the
    /// profile's first coordinate, which only grows with `s`).
    fn s_of_u(&self, u: f32) -> f32 {
        if u <= self.flat {
            return u;
        }
        let (r, amax) = (self.radius, self.max_deg.abs().to_radians());
        let ua = self.flat + r * amax.sin();
        if u <= ua {
            self.flat + r * ((u - self.flat) / r).clamp(-1.0, 1.0).asin()
        } else {
            self.flat + r * amax + (u - ua) / amax.cos()
        }
    }

    /// The sheet's height above the pivot's ground at forward distance `u`.
    fn height_at(&self, u: f32) -> f32 {
        if u <= self.flat {
            return 0.0;
        }
        self.profile(self.s_of_u(u)).1
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
    /// The view azimuth in degrees; None: the player's yaw. Dev-only (the trailer's shots set it per frame).
    pub yaw: Option<f32>,
    /// The eye's distance as a multiple of the distance the view fits to the zoom (dev-only, the
    /// shots' `fit`: the same framing at any window size); `dist` wins when both are set.
    pub fit: Option<f32>,
    /// The eyes view (dev-only, the shots' `eye`): the camera stands on the target at `height` cells
    /// and looks along its pitch and yaw; `horizon` curves the ground down (`bend` is its angle) instead
    /// of rolling it up. See [`TiltCamera::eyes`].
    pub eye: bool,
    pub height: f32,
    pub horizon: bool,
}

impl Default for Trailer {
    fn default() -> Trailer {
        Trailer { pitch: None, fov: None, dist: None, bend: 0.0, flat: 3.0, radius: 8.0, width: 40.0, ahead: 90.0, behind: 8.0, near: 8.0, yaw: None, fit: None, eye: false, height: EYE_HEIGHT, horizon: false }
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
            "yaw" => v.is_finite(),
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
            "yaw" => t.yaw = Some(v),
            _ => t.behind = v,
        }
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every shape of the far ground, for the tests that run at each.
    const FAR_GROUNDS: [FarGround; 3] = [FarGround::Up, FarGround::Horizon, FarGround::Globe];

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

    /// The down bend (the horizon's) is continuous at both joins, and keeps arc length like the up
    /// one: a step along the ground is a step along the sheet, and it sinks below the flat ground.
    #[test]
    fn the_down_bend_is_continuous_at_both_joins_and_keeps_arc_length() {
        let b = Bend::new(vec2(0.0, 0.0), 0.0, 3.0, 8.0, -60.0);
        let amax = 60f32.to_radians();
        let join = 3.0 + 8.0 * amax;
        for s in [3.0, join] {
            let (a, c) = (b.profile(s - 1e-3), b.profile(s + 1e-3));
            assert!((a.0 - c.0).abs() < 0.01 && (a.1 - c.1).abs() < 0.01, "a join at {s}: {a:?} {c:?}");
        }
        assert!(b.profile(10.0).1 < 0.0, "down: the ground past the flat part sinks");
        let mut len = 0.0;
        let (mut prev, mut s) = (b.profile(0.0), 0.0);
        while s < 40.0 {
            s += 0.01;
            let p = b.profile(s);
            len += ((p.0 - prev.0).powi(2) + (p.1 - prev.1).powi(2)).sqrt();
            prev = p;
        }
        assert!((len - 40.0).abs() < 0.05, "arc length {len} for 40 units of ground");
        // The slope beyond the arc is the bend's angle, downward.
        let (p, q) = (b.profile(join + 2.0), b.profile(join + 3.0));
        assert!((((q.1 - p.1) / (q.0 - p.0)).atan().to_degrees() + 60.0).abs() < 1e-2);
    }

    /// The globe («Шар») is flat inside its flat part, and sinks alike at the same distance in every
    /// direction from the pivot, each point on its own bearing: unlike the down bend, its sides fall away.
    #[test]
    fn a_globe_is_flat_inside_its_flat_part_and_sinks_alike_in_every_direction() {
        let b = Bend::globe(vec2(1.0, -2.0), 33.0, 3.0, 8.0);
        let dir = |deg: f32| vec2(deg.to_radians().cos(), deg.to_radians().sin());
        for rho in [0.0, 1.5, 3.0] {
            for deg in [0.0, 60.0, 180.0] {
                let g = b.pivot + dir(deg) * rho;
                let at = b.point(g);
                assert!(at.y.abs() < 1e-6 && (vec2(at.x, at.z) - g).length() < 1e-4, "flat at {rho} bearing {deg}: {at:?}");
            }
        }
        for rho in [5.0, 9.0, 14.0] {
            let heights: Vec<f32> = [0.0, 90.0, 180.0, 270.0, 33.0].iter().map(|&deg| b.point(b.pivot + dir(deg) * rho).y).collect();
            assert!(heights[0] < -0.01, "the globe sinks at {rho}: {heights:?}");
            for h in &heights {
                assert!((h - heights[0]).abs() < 1e-4, "rho {rho}: the same height in every direction: {heights:?}");
            }
            // The point stays on its own bearing from the pivot (a radial sheet).
            let at = b.point(b.pivot + dir(90.0) * rho);
            let bearing = (vec2(at.x, at.z) - b.pivot).normalize();
            assert!((bearing - dir(90.0)).length() < 1e-4, "rho {rho}: bearing {bearing:?}");
        }
        // The arc's own length: a step of ground is a step along the sheet, as the bend's.
        let mut len = 0.0;
        let (mut prev, mut rho) = (b.point(b.pivot + dir(0.0) * 0.0), 0.0);
        while rho < 30.0 {
            rho += 0.01;
            let p = b.point(b.pivot + dir(0.0) * rho);
            len += ((p.x - prev.x).powi(2) + (p.y - prev.y).powi(2) + (p.z - prev.z).powi(2)).sqrt();
            prev = p;
        }
        assert!((len - 30.0).abs() < 0.05, "arc length {len} for 30 units of ground");
    }

    /// A globe's horizon is found from the eye behind the pivot: it is nearest straight ahead, farther at
    /// the sides, and a point is hidden past the horizon of its own bearing, by its radial distance.
    #[test]
    fn a_globe_has_a_horizon_per_bearing_and_hides_by_radius() {
        // The eye stands 41 cells behind the pivot and 8.7 up (yaw 0: the view looks north, the eye is south).
        let eye = vec3(0.0, 8.7, 41.0);
        let b = Bend::globe(vec2(0.0, 0.0), 0.0, 3.0, 40.0).with_horizon(eye);
        assert!(b.globe && b.limit.is_finite() && b.limit > 3.0, "the horizon ahead at {}", b.limit);
        assert!((b.limit - b.horizon_at(vec2(0.0, -1.0))).abs() < 1e-4, "the limit is the horizon ahead");
        let side = b.horizon_at(vec2(1.0, 0.0));
        assert!(side > b.limit + 1.0, "the horizon at the side {side} is farther than ahead {}", b.limit);
        let dir = |deg: f32| vec2(deg.to_radians().cos(), deg.to_radians().sin());
        for deg in [0.0, 45.0, 90.0, 180.0, 270.0] {
            let h = b.horizon_at(b.pivot + dir(deg));
            assert!(!b.hidden(b.pivot + dir(deg) * (h - 0.5)), "bearing {deg}: the ground short of its horizon is seen");
            assert!(b.hidden(b.pivot + dir(deg) * (h + 0.5)), "bearing {deg}: the ground past its horizon is hidden");
            // The sheet stands on the horizon at its line's end (the point at the limit is no lower).
            let at = b.point(b.pivot + dir(deg) * (h + 3.0));
            assert!((vec2(at.x, at.z) - b.pivot).length() <= h + 1e-3, "bearing {deg}: the point past the horizon stands on it");
        }
        // Without the eye the globe has no horizon: nothing is hidden.
        let open = Bend::globe(vec2(0.0, 0.0), 0.0, 3.0, 40.0);
        assert!(!open.hidden(vec2(0.0, -500.0)) && open.limit.is_infinite());
    }

    /// The first-person camera's globe: its horizon ahead is finite and the sky is above it.
    #[test]
    fn the_first_person_globe_has_a_finite_horizon_and_its_sky_above() {
        let look = vec2(20.5, 13.25);
        let cam = TiltCamera::first_person(look, SCALE, FRAME, 0.0, FarGround::Globe);
        let bend = cam.bend.expect("the globe");
        assert!(bend.globe && bend.limit.is_finite() && bend.limit > FP_FLAT, "the horizon at {}", bend.limit);
        let line = cam.project(bend.point(bend.pivot + forward(0.0) * bend.limit), FRAME).expect("the horizon");
        let x = FRAME.x + FRAME.w / 2.0;
        assert!(cam.pick(vec2(x, line.y - 4.0), FRAME).is_none(), "the sky above the horizon");
        assert!(cam.pick(vec2(x, line.y + 4.0), FRAME).is_some(), "the sheet below the horizon");
    }

    /// The eyes camera's globe picks back its own sheet, as the bends do (a point round trip within 0.1 cell).
    #[test]
    fn the_eyes_globe_picks_back_its_sheet_at_every_yaw() {
        let look = vec2(20.5, 13.25);
        let mut checked = 0;
        for yaw in YAWS {
            let mut cam = TiltCamera::eyes(look, EYE_HEIGHT, SCALE, FRAME, EYE_PITCH, yaw, FP_FOV);
            cam.bend = Some(cam.globe_bend(cam.target, EYE_FLAT, EYE_RADIUS_GLOBE));
            let bend = cam.bend.expect("the globe");
            // The eye stands on the sheet's top, so the horizon is near (about 8 cells): the samples are dense in it.
            for s in [0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0] {
                for l in [-4.0, -2.0, 0.0, 2.0, 4.0] {
                    let g = bend.pivot + forward(yaw) * s + right(yaw) * l;
                    if bend.hidden(g) {
                        continue;
                    }
                    let Some(px) = cam.project(bend.point(g), FRAME) else { continue };
                    if !FRAME.contains(px) {
                        continue;
                    }
                    let back = cam.pick(px, FRAME).unwrap_or_else(|| panic!("yaw {yaw}: no hit for {g:?}"));
                    assert!(close(back, g, 0.1), "yaw {yaw}: {g:?} came back as {back:?}");
                    checked += 1;
                }
            }
        }
        assert!(checked > 20, "only {checked} points checked");
    }

    /// The first-person camera puts the hero (the look point) at two thirds of the frame's height,
    /// at the centre across, and fits his ground to the flat scale: his depth is the fitted one.
    #[test]
    fn the_first_person_camera_puts_the_hero_in_the_lower_third_at_the_flat_scale() {
        for yaw in YAWS {
            for far in FAR_GROUNDS {
                let look = vec2(20.5, 13.25);
                let cam = TiltCamera::first_person(look, SCALE, FRAME, yaw, far);
                let px = cam.project(vec3(look.x, 0.0, look.y), FRAME).expect("in front of the eye");
                assert!((px.x - (FRAME.x + FRAME.w / 2.0)).abs() < 1.0, "yaw {yaw}: x {px:?}");
                assert!((px.y - (FRAME.y + FRAME.h * 2.0 / 3.0)).abs() < 1.0, "yaw {yaw}: y {px:?}");
                // The hero's depth along the view line is the flat fit's distance.
                let dir = (vec3(cam.target.x, 0.0, cam.target.y) - cam.eye()).normalize();
                let depth = (vec3(look.x, 0.0, look.y) - cam.eye()).dot(dir);
                let d0 = FRAME.h / 2.0 / ((FP_FOV / 2.0).to_radians().tan() * SCALE);
                assert!((depth / d0 - 1.0).abs() < 1e-3, "yaw {yaw} far {far:?}: depth {depth} against {d0}");
            }
        }
    }

    /// A pick at the hero's pixel is the hero's ground, in both bends and at every yaw.
    #[test]
    fn a_pick_at_the_heros_pixel_is_the_hero_at_both_bends_and_every_yaw() {
        for yaw in YAWS {
            for far in FAR_GROUNDS {
                let look = vec2(20.5, 13.25);
                let cam = TiltCamera::first_person(look, SCALE, FRAME, yaw, far);
                // The hero stands on the sheet: behind the pivot a globe's sheet drops below the flat part,
                // so his pixel is the sheet's point (the flat bends are flat there, as before).
                let on_sheet = cam.bend.map_or(vec3(look.x, 0.0, look.y), |b| b.point(look));
                let px = cam.project(on_sheet, FRAME).expect("the hero is in front of the eye");
                let back = cam.pick(px, FRAME).expect("the hero's ground");
                assert!(close(back, look, 0.1), "yaw {yaw} far {far:?}: {back:?}");
            }
        }
    }

    /// Ground points on the bent sheet project through the camera and pick back within a tenth of
    /// a cell, at both bends and several yaws (the sky and the far side of the horizon excluded).
    /// The eyes camera stands `EYE_HEIGHT` above the ground under its target and looks along its yaw
    /// and pitch. Its bent sheet is picked back at both bends and several yaws (a point round trip
    /// within 0.1 cell), and the down bend has a horizon while the up bend has none.

    #[test]
    fn the_eyes_camera_stands_at_its_height_and_picks_its_sheet_at_both_bends() {
        let look = vec2(20.5, 13.25);
        let cam = TiltCamera::eyes(look, EYE_HEIGHT, SCALE, FRAME, EYE_PITCH, 0.0, FP_FOV);
        assert_eq!(cam.eye(), vec3(look.x, EYE_HEIGHT, look.y));
        assert_eq!(cam.target, look);
        let down = cam.eye_bend(EYE_FLAT, EYE_RADIUS_DOWN, EYE_BEND_DOWN);
        assert!(down.limit.is_finite() && down.limit > EYE_FLAT, "the horizon at {}", down.limit);
        assert!(!cam.eye_bend(EYE_FLAT, EYE_RADIUS_UP, EYE_BEND_UP).limit.is_finite());
        let mut checked = 0;
        for yaw in YAWS {
            for (radius, max_deg) in [(EYE_RADIUS_UP, EYE_BEND_UP), (EYE_RADIUS_DOWN, EYE_BEND_DOWN)] {
                let mut cam = TiltCamera::eyes(look, EYE_HEIGHT, SCALE, FRAME, EYE_PITCH, yaw, FP_FOV);
                let bend = cam.eye_bend(EYE_FLAT, radius, max_deg);
                cam.bend = Some(bend);
                for s in [0.5, 1.5, 3.0, 5.0, 9.0, 14.0, 22.0, 30.0] {
                    for l in [-6.0, -2.0, 0.0, 2.0, 6.0] {
                        let g = bend.pivot + forward(yaw) * s + right(yaw) * l;
                        if bend.hidden(g) {
                            continue;
                        }
                        let Some(px) = cam.project(bend.point(g), FRAME) else { continue };
                        if !FRAME.contains(px) {
                            continue;
                        }
                        let back = cam.pick(px, FRAME).unwrap_or_else(|| panic!("yaw {yaw} max {max_deg}: no hit for {g:?}"));
                        assert!(close(back, g, 0.1), "yaw {yaw} max {max_deg}: {g:?} came back as {back:?}");
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 100, "only {checked} points checked");
    }

    #[test]
    fn pick_undoes_the_bent_sheet_at_both_bends_and_every_yaw() {
        let mut checked = 0;
        for yaw in YAWS {
            for far in FAR_GROUNDS {
                let look = vec2(20.5, 13.25);
                let cam = TiltCamera::first_person(look, SCALE, FRAME, yaw, far);
                let bend = cam.bend.expect("the first-person bend");
                for s in [-24.0, -12.0, -6.0, 0.0, 2.0, 5.0, 9.0, 14.0, 22.0, 30.0] {
                    for l in [-9.0, -3.0, 0.0, 4.0, 9.0] {
                        let g = bend.pivot + forward(yaw) * s + right(yaw) * l;
                        if bend.hidden(g) {
                            continue;
                        }
                        let p3 = bend.point(g);
                        let Some(px) = cam.project(p3, FRAME) else { continue };
                        if !FRAME.contains(px) {
                            continue;
                        }
                        let back = cam.pick(px, FRAME).unwrap_or_else(|| panic!("yaw {yaw} far {far:?}: no hit for {g:?}"));
                        assert!(close(back, g, 0.1), "yaw {yaw} far {far:?}: {g:?} came back as {back:?}");
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 200, "only {checked} points checked");
    }

    /// The down bend's horizon: the pixels above it see no ground (NaN in the map's pointer), those
    /// just below it see the sheet; the up bend has no horizon, so its top pixels see ground.
    #[test]
    fn the_sky_above_the_horizon_of_a_down_bend_sees_no_ground() {
        let look = vec2(20.5, 13.25);
        let cam = TiltCamera::first_person(look, SCALE, FRAME, 0.0, FarGround::Horizon);
        let bend = cam.bend.expect("the bend");
        assert!(bend.limit.is_finite(), "a down bend has a horizon");
        let line = cam.project(bend.point(bend.pivot + forward(0.0) * bend.limit), FRAME).expect("the horizon");
        let x = FRAME.x + FRAME.w / 2.0;
        assert!(cam.pick(vec2(x, line.y - 4.0), FRAME).is_none(), "the sky above the horizon");
        assert!(cam.pick(vec2(x, line.y + 4.0), FRAME).is_some(), "the sheet below the horizon");
        let up = TiltCamera::first_person(look, SCALE, FRAME, 0.0, FarGround::Up);
        assert!(up.bend.expect("the bend").limit.is_infinite());
        assert!(up.pick(vec2(x, FRAME.y + 2.0), FRAME).is_some(), "the up bend's top sees the sheet");
    }

    /// A pick far outside the frame, where the ray runs almost sideways, comes back at once (None or a
    /// point), never a march without end.
    #[test]
    fn a_pick_far_outside_the_frame_ends() {
        for far in FAR_GROUNDS {
            let cam = TiltCamera::first_person(vec2(20.5, 13.25), SCALE, FRAME, 0.0, far);
            for p in [vec2(-1.0e5, 380.0), vec2(1.0e5, 380.0), vec2(640.0, -1.0e5), vec2(640.0, 1.0e5)] {
                let _ = cam.pick(p, FRAME);
            }
        }
    }

    /// The globe's footprint covers every ground point the frame sees: the ground picture is drawn over
    /// it, so a point it misses shows as sky with the standing sprite on it floating above.
    #[test]
    fn the_globe_footprint_covers_every_point_the_frame_sees() {
        for yaw in YAWS {
            let look = vec2(20.5, 13.25);
            let cam = TiltCamera::first_person(look, SCALE, FRAME, yaw, FarGround::Globe);
            let fp = cam.ground_footprint(FRAME, FRAME);
            let mut missed = Vec::new();
            for iy in 0..=40 {
                for ix in 0..=60 {
                    let px = vec2(FRAME.x + FRAME.w * ix as f32 / 60.0, FRAME.y + FRAME.h * iy as f32 / 40.0);
                    if let Some(g) = cam.pick(px, FRAME) {
                        if !fp.contains(g) {
                            missed.push(g);
                        }
                    }
                }
            }
            assert!(missed.is_empty(), "yaw {yaw}: {} points outside the footprint {fp:?}, e.g. {:?}", missed.len(), missed.first());
        }
    }

    /// The bent footprint holds the target and the hero's ground, and is the frame's ground.
    #[test]
    fn the_bent_footprint_holds_the_hero_at_both_bends() {
        for far in FAR_GROUNDS {
            let look = vec2(20.5, 13.25);
            let cam = TiltCamera::first_person(look, SCALE, FRAME, 45.0, far);
            let fp = cam.ground_footprint(FRAME, FRAME);
            assert!(fp.contains(look), "far {far:?}: {fp:?}");
            assert!(fp.w > 1.0 && fp.h > 1.0, "a footprint: {fp:?}");
        }
    }
}

/// The projective map from a window pixel to the ground (world units, x and z), fitted to the four
/// corners of `frame` by `TiltCamera::pick`: the ground is a plane, so one 3 × 3 matrix (row-major,
/// in homogeneous pixel coordinates) gives every pixel's ground point. None when a corner sees no
/// ground (above the horizon) or the fit is singular.
pub fn ground_homography(cam: &TiltCamera, frame: Rect) -> Option<[[f32; 3]; 3]> {
    let src = [
        vec2(frame.x, frame.y),
        vec2(frame.x + frame.w, frame.y),
        vec2(frame.x + frame.w, frame.y + frame.h),
        vec2(frame.x, frame.y + frame.h),
    ];
    let mut dst = [Vec2::ZERO; 4];
    for (d, s) in dst.iter_mut().zip(src) {
        *d = cam.pick(s, frame)?;
    }
    // Solve for h (h8 = 1): X (h6 x + h7 y + 1) = h0 x + h1 y + h2, Y likewise. Eight equations.
    let mut a = [[0.0f64; 9]; 8];
    for i in 0..4 {
        let (x, y) = (src[i].x as f64, src[i].y as f64);
        let (bx, by) = (dst[i].x as f64, dst[i].y as f64);
        a[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -bx * x, -bx * y, bx];
        a[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -by * x, -by * y, by];
    }
    for col in 0..8 {
        let piv = (col..8).max_by(|&r, &q| a[r][col].abs().total_cmp(&a[q][col].abs()))?;
        if a[piv][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, piv);
        let pivot = a[col];
        for (r, row) in a.iter_mut().enumerate() {
            if r != col {
                let f = row[col] / pivot[col];
                for (x, p) in row.iter_mut().zip(pivot.iter()).skip(col) {
                    *x -= f * p;
                }
            }
        }
    }
    let h: Vec<f64> = (0..8).map(|i| a[i][8] / a[i][i]).collect();
    Some([[h[0] as f32, h[1] as f32, h[2] as f32], [h[3] as f32, h[4] as f32, h[5] as f32], [h[6] as f32, h[7] as f32, 1.0]])
}

#[cfg(test)]
mod homography_tests {
    use super::*;

    #[test]
    fn the_homography_matches_pick_at_every_pixel_of_a_tilted_frame() {
        let frame = Rect::new(0.0, 0.0, 1280.0, 711.0);
        let cam = TiltCamera::new(vec2(100.0, 103.0), 33.0, frame, 50.0, 30.0);
        let h = ground_homography(&cam, frame).expect("the corners see the ground");
        for (x, y) in [(640.0, 355.0), (100.0, 600.0), (1200.0, 80.0), (300.0, 300.0)] {
            let g = cam.pick(vec2(x, y), frame).expect("ground");
            let w = h[2][0] * x + h[2][1] * y + h[2][2];
            let px = (h[0][0] * x + h[0][1] * y + h[0][2]) / w;
            let py = (h[1][0] * x + h[1][1] * y + h[1][2]) / w;
            assert!((px - g.x).abs() < 0.01 && (py - g.y).abs() < 0.01, "({x},{y}): {px},{py} vs {g:?}");
        }
    }

    #[test]
    fn the_first_person_roll_up_rises_gently_from_the_flat_part() {
        // Eight cells past the flat part the far ground is under 1.5 cells high: the tight roll of
        // radius 8 was 3.7 there, so the horizon went from flat to square-on too fast.
        let bend = Bend::new(Vec2::ZERO, 0.0, FP_FLAT, FP_RADIUS, FP_BEND_UP);
        let (_, h) = bend.profile(FP_FLAT + 8.0);
        assert!(h < 1.5, "the roll is {h} cells high 8 cells past its flat part");
        assert!(bend.profile(FP_FLAT + 8.0).0 > FP_FLAT + 7.0, "the roll keeps the ground's distance");
    }
}
