//! Razdor's «Живая карта» (`Settings::map_living`, off by default; `RAZDOR_LIVING=1` for the
//! snapshots). Style only: smoke rises from the chimneys, the mill sails turn, the flags wave and
//! small flocks of birds cross the explored map now and then. No rule, path, fog or save reads it.
//!
//! The points are the offline detector's boxes (`dt::lights`). This module holds the switch, the
//! pure maths (tested without a window) and the mesh builders; `world_view` places them. The
//! sprites with flags or sails are drawn from a copy with those areas cleared (`cleared`), and the
//! moving parts are drawn over it, so no pixel shows both at rest and moved.

use macroquad::prelude::*;
use razdor::dt::gfx::Image;
use razdor::dt::lights::{Kind, Point};
use razdor::local;

local! {
    /// The setting, taken in every frame (`set_prefs`).
    static LIVING: std::cell::Cell<bool> = std::cell::Cell::new(false);
    /// `RAZDOR_LIVING=1` turns the map alive whatever the setting says (read once).
    static ENV_ON: bool = std::env::var("RAZDOR_LIVING").is_ok_and(|v| v.trim() == "1");
}

/// Takes the setting «Живая карта» in (every frame, from `display::follow_settings`).
pub fn set_prefs(on: bool) {
    LIVING.set(on || ENV_ON.with(|e| *e));
}

/// Whether the map is alive this frame (the setting, or `RAZDOR_LIVING=1`).
pub fn on() -> bool {
    LIVING.get()
}

// ----- Smoke -----

/// Seconds one puff lives: it rises, grows and fades in that time.
pub const PUFF_LIFE: f64 = 6.0;
/// Puffs per chimney at once (phones draw fewer, see `puffs_now`).
pub const PUFFS: usize = 7;
/// Puffs per chimney on the phones.
pub const PUFFS_PHONE: usize = 4;
/// The smoke's colour: grey-white. It is a parameter so the map's daylight tint can be set on it
/// later; today the tint of the whole map (`daylight`) already lies over it, as over every sprite.
pub const SMOKE_RGB: [f32; 3] = [0.86, 0.86, 0.88];
/// The alpha of a puff at its strongest.
const PUFF_ALPHA: f32 = 0.85;

/// The puffs per chimney on this device: half on the phones.
pub fn puffs_now() -> usize {
    if crate::ui::display::dp_px().is_some() {
        PUFFS_PHONE
    } else {
        PUFFS
    }
}

/// The smoothstep from `a` to `b` (either order: `a > b` falls from 1 to 0).
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A puff at life fraction `f` (0 at its birth, 1 at its end): its rise above the chimney top, its
/// radius and its alpha, in cells. It rises 1.5 cells, grows from 0.2 to 0.7 cell, fades in
/// quickly and out slowly.
pub fn puff_shape(f: f32) -> (f32, f32, f32) {
    let f = f.clamp(0.0, 1.0);
    let rise = 1.5 * f;
    let radius = 0.2 + 0.5 * f;
    let alpha = PUFF_ALPHA * smoothstep(0.0, 0.15, f) * (1.0 - f).powf(1.5);
    (rise, radius, alpha)
}

/// The life fraction (0..1) of puff `k` of `puffs` of an emitter with phase `offset` (0..1) at
/// `now` seconds: the puffs of one chimney are spaced evenly through their lives.
pub fn puff_fraction(now: f64, offset: f32, k: usize, puffs: usize) -> f32 {
    let x = now / PUFF_LIFE;
    let p = (x - x.floor()) as f32 + offset + k as f32 / puffs.max(1) as f32;
    p - p.floor()
}

/// The shared wind at `now`, from -1 (blowing left) to 1 (right): a slow sway over about 50 s.
pub fn wind(now: f64) -> f32 {
    (now * std::f64::consts::TAU / 50.0).sin() as f32
}

/// splitmix64: a hash of `x` for the seeds of the living map (the same on every run).
pub fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A value in [0, 1) from `seed` and `n`.
pub fn unit(seed: u64, n: u64) -> f32 {
    (mix(seed ^ mix(n)) >> 40) as f32 / (1u64 << 24) as f32
}

/// Where a 2D point of a standing sprite lies in the world: flat, the 2D point is the screen's;
/// tilted, the billboard's model (`view25::billboard_model_about`), the same one the sprite uses.
#[derive(Clone, Copy, Debug)]
pub struct Space {
    model: Option<Mat4>,
}

impl Space {
    /// The flat view: the 2D point is the screen point.
    pub fn flat() -> Space {
        Space { model: None }
    }

    /// A standing sprite's billboard model.
    pub fn standing(model: Mat4) -> Space {
        Space { model: Some(model) }
    }

    /// The vertex position of the 2D point `p`.
    pub fn point(&self, p: Vec2) -> Vec3 {
        match self.model {
            Some(m) => m.transform_point3(vec3(p.x, p.y, 0.0)),
            None => vec3(p.x, p.y, 0.0),
        }
    }
}

/// A batch of quads drawn in one mesh per frame (the smoke of the whole map).
#[derive(Default)]
pub struct Batch {
    quads: Vec<[Vertex; 4]>,
}

/// Quads per draw call: well under macroquad's per-draw vertex limit.
const QUADS_PER_DRAW: usize = 2000;

impl Batch {
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
    }

    /// A soft quad centred on the 2D point `c`, `r` wide each way, in `space`.
    pub fn puff(&mut self, space: &Space, c: Vec2, r: f32, color: Color) {
        let v = |p: Vec2, u: f32, w: f32| {
            let q = space.point(p);
            Vertex::new(q.x, q.y, q.z, u, w, color)
        };
        self.quads.push([v(c + vec2(-r, -r), 0.0, 0.0), v(c + vec2(r, -r), 1.0, 0.0), v(c + vec2(r, r), 1.0, 1.0), v(c + vec2(-r, r), 0.0, 1.0)]);
    }

    /// Draws the batch with `tex`, in chunks of `QUADS_PER_DRAW` quads.
    pub fn draw(&self, tex: &Texture2D) {
        for chunk in self.quads.chunks(QUADS_PER_DRAW) {
            let vertices: Vec<Vertex> = chunk.iter().flatten().copied().collect();
            let indices: Vec<u16> = (0..chunk.len() as u16).flat_map(|q| {
                let b = q * 4;
                [b, b + 1, b + 2, b, b + 2, b + 3]
            }).collect();
            draw_mesh(&Mesh { vertices, indices, texture: Some(tex.clone()) });
        }
    }
}

/// The soft round texture of a puff (white, alpha falling to 0 at the edge), made once.
pub fn puff_texture() -> Texture2D {
    local! {
        static TEX: Texture2D = {
            const N: usize = 32;
            let mut rgba = Vec::with_capacity(N * N * 4);
            for y in 0..N {
                for x in 0..N {
                    let d = ((x as f32 + 0.5 - N as f32 / 2.0).powi(2) + (y as f32 + 0.5 - N as f32 / 2.0).powi(2)).sqrt() / (N as f32 / 2.0);
                    // Full in the middle, fading out at the edge: a puff is a soft blob, not a dot.
                    let a = smoothstep(1.0, 0.35, d);
                    rgba.extend_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
                }
            }
            let tex = Texture2D::from_rgba8(N as u16, N as u16, &rgba);
            tex.set_filter(FilterMode::Linear);
            tex
        };
    }
    TEX.with(Clone::clone)
}

/// The smoke's timing and size for one frame: the time, the puffs per chimney and the size of a cell
/// in the 2D space.
#[derive(Clone, Copy, Debug)]
pub struct Smoke {
    pub now: f64,
    pub puffs: usize,
    pub cell: f32,
}

/// Adds the puffs of one chimney to `batch`. `top` is the chimney's top centre in the 2D space; `seed`
/// makes its phase and its wobble its own; `alpha` scales the whole (the fog's visibility of the
/// building).
pub fn push_chimney(batch: &mut Batch, space: &Space, top: Vec2, seed: u64, alpha: f32, smoke: &Smoke) {
    let Smoke { now, puffs, cell } = *smoke;
    let offset = unit(seed, 0);
    for k in 0..puffs {
        let f = puff_fraction(now, offset, k, puffs);
        let (rise, radius, a) = puff_shape(f);
        let a = a * alpha;
        if a <= 0.004 {
            continue;
        }
        let lateral = (unit(seed, 1 + k as u64) - 0.5) * 0.15;
        let drift = wind(now) * 0.8 * f + lateral;
        let c = top + vec2(drift * cell, -rise * cell);
        let [r, g, b] = SMOKE_RGB;
        batch.puff(space, c, radius * cell, Color::new(r, g, b, a));
    }
}

// ----- Moving parts: flags and sails -----

/// The seconds a flag's cloth takes for one wave.
pub const FLAG_WAVE_SECS: f64 = 2.4;
/// The seconds a sail takes for one turn (slow: a windmill in a light wind).
pub const SAIL_TURN_SECS: f64 = 9.0;

/// A disc of a sail: its hub and its radius in sprite pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Disc {
    pub hub: [f32; 2],
    pub radius: f32,
}

/// The disc a sails box turns in: centred on the box, as wide as half its longer side, kept inside
/// the sprite (`w` × `h` pixels).
pub fn sail_disc(rect: [f32; 4], w: f32, h: f32) -> Disc {
    let hub = [(rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0];
    let radius = ((rect[2] - rect[0]).max(rect[3] - rect[1]) / 2.0).min(hub[0]).min(hub[1]).min(w - hub[0]).min(h - hub[1]).max(1.0);
    Disc { hub, radius }
}

/// A sail's turn at `now`, in radians; `phase` offsets the sails of one sprite from each other.
pub fn sail_angle(now: f64, phase: f32) -> f32 {
    let turn = (now * std::f64::consts::TAU / SAIL_TURN_SECS) % std::f64::consts::TAU;
    turn as f32 + phase
}

/// A flag's sideways displacement in sprite pixels at `s` (0 at the pole, 1 at the free end) for a
/// flag `width` pixels wide: the cloth's wave, its free end moving most.
pub fn flag_sway(s: f32, now: f64, phase: f32, width: f32) -> f32 {
    let amp = (0.06 * width).clamp(0.6, 2.0);
    let t = ((now * std::f64::consts::TAU / FLAG_WAVE_SECS) % std::f64::consts::TAU) as f32;
    amp * s * (t - 3.0 * s + phase).sin()
}

/// A moving part of a sprite: a flag's box (its pole on the edge nearer the sprite's centre) or a
/// sails disc. `phase` is the part's own, from its box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Part {
    Flag { rect: [f32; 4], pole_right: bool, phase: f32 },
    Sails { disc: Disc, phase: f32 },
}

impl Part {
    /// The part's centre in sprite pixels (a flag's box centre, a sails hub).
    pub fn anchor(&self) -> Vec2 {
        match *self {
            Part::Flag { rect, .. } => vec2((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0),
            Part::Sails { disc, .. } => vec2(disc.hub[0], disc.hub[1]),
        }
    }

    /// The part a point of a `w` × `h` sprite makes, or `None` for the kinds that do not move.
    /// `index` (the point's place in its sprite) sets the phase.
    pub fn of(point: &Point, w: f32, h: f32, index: usize) -> Option<Part> {
        let phase = unit(0x5EED, index as u64) * std::f32::consts::TAU;
        let r = point.rect;
        match point.kind {
            Kind::Flag => {
                let (c, x0, x1) = (w / 2.0, r[0], r[2]);
                let pole_right = (x1 - c).abs() < (x0 - c).abs();
                Some(Part::Flag { rect: r, pole_right, phase })
            }
            Kind::Sails => Some(Part::Sails { disc: sail_disc(r, w, h), phase }),
            _ => None,
        }
    }
}

/// The part's mesh at `now`, drawn from a texture whose sprite lies at `src` (in texels) in a
/// texture of `tex` texels; the sprite's top-left is at `origin` in the 2D space, `zoom` 2D units
/// to a sprite pixel. Its vertices are 2D points (the caller's standing transform places them).
pub fn part_mesh(part: &Part, now: f64, src: Rect, tex: Vec2, origin: Vec2, zoom: f32, color: Color) -> (Vec<Vertex>, Vec<u16>) {
    let uv = |x: f32, y: f32| vec2((src.x + x) / tex.x, (src.y + y) / tex.y);
    let at = |x: f32, y: f32| origin + vec2(x, y) * zoom;
    match *part {
        Part::Flag { rect, pole_right, phase } => {
            let [x0, y0, x1, y1] = rect;
            let (w, h) = (x1 - x0, y1 - y0);
            let cols = (w.round() as usize).max(2);
            let mut vertices = Vec::with_capacity((cols + 1) * 2);
            for i in 0..=cols {
                let u = i as f32 / cols as f32;
                let s = if pole_right { 1.0 - u } else { u };
                let dx = flag_sway(s, now, phase, w);
                for j in 0..=1 {
                    let v = j as f32;
                    let p = at(x0 + u * w + dx, y0 + v * h);
                    let t = uv(x0 + u * w, y0 + v * h);
                    vertices.push(Vertex::new(p.x, p.y, 0.0, t.x, t.y, color));
                }
            }
            let mut indices = Vec::with_capacity(cols * 6);
            for i in 0..cols as u16 {
                let a = i * 2;
                indices.extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
            }
            (vertices, indices)
        }
        Part::Sails { disc, phase } => {
            const RIM: usize = 32;
            let angle = sail_angle(now, phase);
            let (sin, cos) = angle.sin_cos();
            let [hx, hy] = disc.hub;
            let r = disc.radius;
            let mut vertices = Vec::with_capacity(RIM + 1);
            let c = at(hx, hy);
            let ct = uv(hx, hy);
            vertices.push(Vertex::new(c.x, c.y, 0.0, ct.x, ct.y, color));
            for i in 0..RIM {
                let phi = std::f32::consts::TAU * i as f32 / RIM as f32;
                let (ox, oy) = (phi.cos() * r, phi.sin() * r);
                // The rim's place turns with the sails; its texels stay where they are.
                let (px, py) = (ox * cos - oy * sin, ox * sin + oy * cos);
                let p = at(hx + px, hy + py);
                let t = uv(hx + ox, hy + oy);
                vertices.push(Vertex::new(p.x, p.y, 0.0, t.x, t.y, color));
            }
            let mut indices = Vec::with_capacity(RIM * 3);
            for i in 0..RIM as u16 {
                indices.extend_from_slice(&[0, 1 + i, 1 + (i + 1) % RIM as u16]);
            }
            (vertices, indices)
        }
    }
}

/// The sprite's pixels with the moving parts' areas cleared (alpha 0): a flag's box and a sails
/// disc (kept inside the sprite). The parts are drawn over the cleared copy, so the still picture
/// never shows under a moved one.
pub fn cleared(img: &Image, parts: &[Part]) -> Image {
    let mut out = img.clone();
    let (w, h) = (img.width as usize, img.height as usize);
    let mut clear = |hit: &dyn Fn(f32, f32) -> bool| {
        for y in 0..h {
            for x in 0..w {
                if hit(x as f32 + 0.5, y as f32 + 0.5) {
                    out.rgba[(y * w + x) * 4 + 3] = 0;
                }
            }
        }
    };
    for part in parts {
        match *part {
            Part::Flag { rect, .. } => clear(&|x, y| x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]),
            Part::Sails { disc, .. } => clear(&|x, y| (x - disc.hub[0]).powi(2) + (y - disc.hub[1]).powi(2) <= disc.radius * disc.radius),
        }
    }
    out
}

// ----- Telling the parts apart -----

/// What a box of a sprite holds: its size, its opaque pixels (alpha 128 or more), those per quadrant
/// about the box centre, and the centroid of the opaque pixels, in sprite pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxLook {
    pub area: usize,
    pub opaque: usize,
    pub quadrants: [usize; 4],
    pub centroid: (f32, f32),
    pub centre: (f32, f32),
    pub half: (f32, f32),
}

/// The look of the box `rect` of `img`, clamped to the sprite.
pub fn box_look(img: &Image, rect: [f32; 4]) -> BoxLook {
    let (w, h) = (img.width as usize, img.height as usize);
    let x0 = rect[0].max(0.0).floor() as usize;
    let y0 = rect[1].max(0.0).floor() as usize;
    let x1 = (rect[2].ceil().max(0.0) as usize).min(w);
    let y1 = (rect[3].ceil().max(0.0) as usize).min(h);
    let centre = ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
    let half = ((rect[2] - rect[0]) / 2.0, (rect[3] - rect[1]) / 2.0);
    let mut look = BoxLook { area: 0, opaque: 0, quadrants: [0; 4], centroid: (0.0, 0.0), centre, half };
    let (mut sx, mut sy) = (0.0f64, 0.0f64);
    for y in y0..y1 {
        for x in x0..x1 {
            look.area += 1;
            if img.rgba[(y * w + x) * 4 + 3] < 128 {
                continue;
            }
            look.opaque += 1;
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let q = usize::from(px >= centre.0) + 2 * usize::from(py >= centre.1);
            look.quadrants[q] += 1;
            sx += f64::from(px);
            sy += f64::from(py);
        }
    }
    if look.opaque > 0 {
        look.centroid = ((sx / look.opaque as f64) as f32, (sy / look.opaque as f64) as f32);
    }
    look
}

/// Whether a box's pixels look like sails: thin spokes or blades on the sprite's background, so
/// under 55 % of the box is opaque, the opaque pixels are about evenly spread over the four quadrants
/// (the weakest at least half the strongest), and their centroid lies near the box centre (within a
/// quarter of its half-size: the hub). A windmill's roof or a wall fails the coverage test.
pub fn sails_look(look: &BoxLook) -> bool {
    if look.opaque == 0 || look.area == 0 {
        return false;
    }
    let coverage = look.opaque as f32 / look.area as f32;
    let (lo, hi) = (look.quadrants.iter().min().copied().unwrap_or(0), look.quadrants.iter().max().copied().unwrap_or(0));
    let balanced = hi > 0 && lo * 2 >= hi;
    let near_hub = (look.centroid.0 - look.centre.0).abs() <= 0.25 * look.half.0 && (look.centroid.1 - look.centre.1).abs() <= 0.25 * look.half.1;
    coverage < 0.55 && balanced && near_hub
}

/// Whether a flag box's pixels look like cloth: not a figure-sized opaque blob, so at most 70 % of
/// the box is opaque.
pub fn cloth_look(look: &BoxLook) -> bool {
    look.opaque > 0 && (look.opaque as f32 / look.area.max(1) as f32) <= 0.70
}

/// Whether a point's moving part is real: a sails box that looks like sails, a flag box that looks
/// like cloth. Other kinds never move.
pub fn moves(img: &Image, point: &Point) -> bool {
    let look = box_look(img, point.rect);
    match point.kind {
        Kind::Sails => sails_look(&look),
        Kind::Flag => cloth_look(&look),
        _ => false,
    }
}

// ----- Birds -----

/// One chance of a flock per slot of this many seconds: a flock starts at a seeded time in the
/// first half of its slot, so flocks come 20 to 60 seconds apart.
pub const BIRD_SLOT: f64 = 40.0;
/// Seconds a flock is in the air.
pub const BIRD_TRIP: f64 = 14.0;

/// A flock: its start time, its straight course in world units (from `from` to `to`, bowed by
/// `arc`), its birds and its seed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flock {
    pub start: f64,
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub arc: f32,
    pub birds: usize,
    pub seed: u64,
}

/// The flock in the air at `now`, if any: the slot's flock when its trip covers `now`. `bounds` is
/// the map's `[x0, y0, x1, y1]` in world units. The same `seed` and `now` always give the same flock.
pub fn flock_at(now: f64, seed: u64, bounds: [f32; 4]) -> Option<Flock> {
    if now < 0.0 {
        return None;
    }
    let slot = (now / BIRD_SLOT).floor() as u64;
    let slot_seed = seed ^ mix(slot);
    let start = slot as f64 * BIRD_SLOT + BIRD_SLOT / 2.0 * f64::from(unit(slot_seed, 1));
    if now < start || now >= start + BIRD_TRIP {
        return None;
    }
    let [x0, y0, x1, y1] = bounds;
    let (bw, bh) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    let from = (x0 + unit(slot_seed, 2) * bw, y0 + unit(slot_seed, 3) * bh);
    let heading = unit(slot_seed, 4) * std::f32::consts::TAU;
    let len = (0.35 + 0.25 * unit(slot_seed, 5)) * bw.min(bh);
    let to = (from.0 + heading.cos() * len, from.1 + heading.sin() * len);
    let to = (to.0.clamp(x0, x1), to.1.clamp(y0, y1));
    let arc = (unit(slot_seed, 6) - 0.5) * 0.2 * len;
    let birds = 3 + (unit(slot_seed, 7) * 5.0) as usize;
    Some(Flock { start, from, to, arc, birds: birds.min(7), seed: slot_seed })
}

/// Each bird of `f` at `now`: its world position (in a V behind the leader, along the flock's
/// course) and its wing's flap, from -1 (down) to 1 (up).
pub fn bird_points(f: &Flock, now: f64) -> Vec<((f32, f32), f32)> {
    let u = (((now - f.start) / BIRD_TRIP) as f32).clamp(0.0, 1.0);
    let (dx, dy) = (f.to.0 - f.from.0, f.to.1 - f.from.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
    let (dir, perp) = ((dx / len, dy / len), (-dy / len, dx / len));
    let bow = f.arc * (std::f32::consts::PI * u).sin();
    let lead = (f.from.0 + dx * u + perp.0 * bow, f.from.1 + dy * u + perp.1 * bow);
    (0..f.birds)
        .map(|j| {
            let rank = j.div_ceil(2) as f32;
            let side = if j % 2 == 0 { 1.0 } else { -1.0 };
            let back = -rank * 0.5;
            let out = side * rank * 0.6;
            let p = (lead.0 + dir.0 * back + perp.0 * out, lead.1 + dir.1 * back + perp.1 * out);
            let flap = (now * 8.0 + j as f64 * 1.3).sin() as f32;
            (p, flap)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: u32, h: u32) -> Image {
        Image { width: w, height: h, rgba: vec![200; (w * h * 4) as usize] }
    }

    #[test]
    fn a_puff_is_born_small_and_clear_grows_and_fades_out() {
        let (rise0, r0, a0) = puff_shape(0.0);
        assert_eq!((rise0, a0), (0.0, 0.0), "a puff starts at the chimney, clear");
        assert!((r0 - 0.2).abs() < 1e-6);
        let (rise1, r1, a1) = puff_shape(1.0);
        assert!((rise1 - 1.5).abs() < 1e-6, "it rises 1.5 cells");
        assert!((r1 - 0.7).abs() < 1e-6, "it grows to 0.7 cell");
        assert_eq!(a1, 0.0, "and is gone at its end");
        let mut prev = -1.0;
        let mut peak = 0.0f32;
        for i in 0..=100 {
            let (_, _, a) = puff_shape(i as f32 / 100.0);
            peak = peak.max(a);
            assert!(a >= 0.0 && a <= PUFF_ALPHA + 1e-6);
            if i > 15 {
                assert!(a <= prev + 1e-6, "after the fade-in the alpha only falls");
            }
            prev = a;
        }
        assert!(peak > 0.35, "it is visible at its strongest: {peak}");
    }

    #[test]
    fn the_puffs_of_one_chimney_are_spaced_through_their_lives() {
        let f: Vec<f32> = (0..5).map(|k| puff_fraction(10.0, 0.25, k, 5)).collect();
        for w in f.windows(2) {
            let gap = (w[1] - w[0]).rem_euclid(1.0);
            assert!((gap - 0.2).abs() < 1e-4, "one fifth of a life apart: {f:?}");
        }
        for t in [0.0, 1.7, 99.9, 12345.6] {
            for k in 0..5 {
                let x = puff_fraction(t, 0.9, k, 5);
                assert!((0.0..1.0).contains(&x), "a fraction in [0, 1): {x}");
            }
        }
    }

    #[test]
    fn a_chimney_draws_its_puffs_only_while_they_are_visible_and_batches_them() {
        let mut batch = Batch::default();
        let space = Space::flat();
        // At a puff's birth fraction it is clear and draws nothing; the others draw.
        push_chimney(&mut batch, &space, vec2(100.0, 50.0), 7, 1.0, &Smoke { now: 0.0, puffs: 5, cell: 32.0 });
        assert!(batch.quads.len() <= 5);
        assert!(!batch.is_empty());
        let mut none = Batch::default();
        push_chimney(&mut none, &space, vec2(0.0, 0.0), 7, 0.0, &Smoke { now: 0.0, puffs: 5, cell: 32.0 });
        assert!(none.is_empty(), "a building in the fog (visibility 0) draws no smoke");
    }

    #[test]
    fn smoke_rises_from_the_chimney_top_and_drifts_with_the_wind() {
        let mut batch = Batch::default();
        let space = Space::flat();
        let top = vec2(100.0, 50.0);
        push_chimney(&mut batch, &space, top, 3, 1.0, &Smoke { now: 2.0, puffs: 5, cell: 32.0 });
        let centres: Vec<Vec2> = batch.quads.iter().map(|q| vec2((q[0].position.x + q[2].position.x) / 2.0, (q[0].position.y + q[2].position.y) / 2.0)).collect();
        assert!(centres.iter().all(|c| c.y < top.y + 1e-3), "every puff is above the top: {centres:?}");
        assert!(centres.iter().any(|c| c.y < top.y - 20.0), "some have risen");
    }

    #[test]
    fn the_flag_pole_is_on_the_edge_nearer_the_sprites_centre() {
        let left = Point { kind: Kind::Flag, rect: [2.0, 0.0, 12.0, 6.0] };
        let right = Point { kind: Kind::Flag, rect: [28.0, 0.0, 38.0, 6.0] };
        match Part::of(&left, 40.0, 30.0, 0) {
            Some(Part::Flag { pole_right, .. }) => assert!(pole_right, "a flag on the left: its pole is its right edge"),
            other => panic!("{other:?}"),
        }
        match Part::of(&right, 40.0, 30.0, 0) {
            Some(Part::Flag { pole_right, .. }) => assert!(!pole_right, "a flag on the right: its pole is its left edge"),
            other => panic!("{other:?}"),
        }
        assert!(Part::of(&Point { kind: Kind::Chimney, rect: [0.0, 0.0, 2.0, 2.0] }, 40.0, 30.0, 0).is_none());
    }

    #[test]
    fn the_pole_side_stays_still_and_the_free_end_waves() {
        for t in [0.0, 0.7, 1.9] {
            assert_eq!(flag_sway(0.0, t, 1.0, 10.0), 0.0, "the pole does not move");
        }
        let moved = (0..20).map(|i| flag_sway(1.0, i as f64 * 0.13, 0.0, 10.0).abs()).fold(0.0f32, f32::max);
        assert!(moved > 0.5 && moved <= 2.0, "the free end swings within its amplitude: {moved}");
    }

    #[test]
    fn a_sails_disc_is_inside_the_sprite_and_turns_about_its_hub() {
        let d = sail_disc([60.0, 10.0, 95.0, 60.0], 200.0, 164.0);
        assert_eq!(d.hub, [77.5, 35.0]);
        assert!((d.radius - 25.0).abs() < 1e-4, "half the longer side: {}", d.radius);
        let edge = sail_disc([0.0, 5.0, 14.0, 35.0], 70.0, 57.0);
        assert!(edge.hub[0] - edge.radius >= 0.0, "a disc on the sprite's edge is kept inside it: {edge:?}");
        assert!(sail_angle(0.0, 0.0).abs() < 1e-6);
        let quarter = sail_angle(SAIL_TURN_SECS / 4.0, 0.0);
        assert!((quarter - std::f32::consts::FRAC_PI_2).abs() < 1e-4, "a quarter turn after a quarter of the turn: {quarter}");
        assert!((sail_angle(SAIL_TURN_SECS * 3.0 + 0.5, 0.2) - sail_angle(0.5, 0.2)).abs() < 1e-3, "it repeats");
    }

    #[test]
    fn the_cleared_copy_lacks_the_parts_and_keeps_the_rest() {
        let img = image(20, 10);
        let parts = [Part::Flag { rect: [2.0, 2.0, 5.0, 4.0], pole_right: false, phase: 0.0 }];
        let out = cleared(&img, &parts);
        let alpha = |x: u32, y: u32| out.rgba[((y * 20 + x) * 4 + 3) as usize];
        assert_eq!(alpha(3, 3), 0, "inside the flag's box");
        assert_eq!(alpha(6, 3), 200, "outside it");
        assert_eq!(alpha(2, 4), 200, "the box's bottom edge is open");
        let disc = [Part::Sails { disc: Disc { hub: [10.0, 5.0], radius: 3.0 }, phase: 0.0 }];
        let out = cleared(&img, &disc);
        let alpha = |x: u32, y: u32| out.rgba[((y * 20 + x) * 4 + 3) as usize];
        assert_eq!(alpha(10, 5), 0);
        assert_eq!(alpha(16, 5), 200, "outside the disc");
        assert_eq!(out.rgba.len(), img.rgba.len());
    }

    #[test]
    fn the_flock_schedule_is_seeded_and_spaced() {
        let bounds = [0.0, 0.0, 60.0, 40.0];
        let seed = 0xB1D5;
        let a: Vec<Option<Flock>> = (0..2000).map(|i| flock_at(i as f64 * 0.5, seed, bounds)).collect();
        let b: Vec<Option<Flock>> = (0..2000).map(|i| flock_at(i as f64 * 0.5, seed, bounds)).collect();
        assert_eq!(a, b, "the same seed and time give the same flocks");
        let mut starts: Vec<f64> = Vec::new();
        for f in a.iter().flatten() {
            if starts.last() != Some(&f.start) {
                starts.push(f.start);
            }
        }
        assert!(starts.len() >= 20, "flocks come through a long run: {}", starts.len());
        for w in starts.windows(2) {
            let gap = w[1] - w[0];
            assert!((20.0..=60.0).contains(&gap), "20 to 60 seconds apart: {gap}");
        }
        for f in a.iter().flatten() {
            assert!((3..=7).contains(&f.birds), "3 to 7 birds: {}", f.birds);
            assert!(f.to.0 >= 0.0 && f.to.0 <= 60.0 && f.to.1 >= 0.0 && f.to.1 <= 40.0, "the course stays on the map");
        }
        assert!(flock_at(-1.0, seed, bounds).is_none());
        let first = |seed: u64| (0..1000).find_map(|i| flock_at(i as f64 * 0.1, seed, bounds)).map(|f| f.from);
        assert!(first(1).is_some(), "a flock in the first 100 seconds");
        assert_ne!(first(1), first(2), "another seed, another flock");
    }

    #[test]
    fn birds_fly_a_v_along_the_course_and_flap() {
        let f = Flock { start: 100.0, from: (5.0, 5.0), to: (45.0, 5.0), arc: 0.0, birds: 5, seed: 1 };
        let at_start = bird_points(&f, 100.0);
        assert_eq!(at_start.len(), 5);
        assert!((at_start[0].0 .0 - 5.0).abs() < 1e-4, "the leader starts at the course's start");
        let at_mid = bird_points(&f, 107.0);
        assert!((at_mid[0].0 .0 - 25.0).abs() < 0.5, "halfway along at the middle of the trip: {}", at_mid[0].0 .0);
        // The followers sit behind the leader and to either side.
        assert!(at_mid[1].0 .1 != at_mid[0].0 .1 && at_mid[2].0 .1 != at_mid[0].0 .1, "the V opens to both sides");
        assert!(at_mid.iter().skip(1).all(|(p, _)| p.0 < at_mid[0].0 .0 + 1e-3), "the rest trail behind");
        assert!(at_mid.iter().all(|(_, flap)| (-1.0..=1.0).contains(flap)));
    }

    #[test]
    fn a_flag_keeps_its_pole_still_and_its_free_end_in_its_sway() {
        let flag = Part::Flag { rect: [4.0, 2.0, 12.0, 8.0], pole_right: false, phase: 0.0 };
        let (verts, indices) = part_mesh(&flag, 1.3, Rect::new(0.0, 0.0, 20.0, 20.0), vec2(20.0, 20.0), vec2(0.0, 0.0), 1.0, WHITE);
        assert!(indices.iter().all(|&i| usize::from(i) < verts.len()), "every index is a vertex");
        assert_eq!(verts[0].position.x, 4.0, "the pole's column does not move");
        assert_eq!(verts[1].position.x, 4.0);
        let amp = (0.06 * 8.0f32).clamp(0.6, 2.0);
        let free = verts.iter().map(|v| v.position.x).fold(f32::MIN, f32::max);
        assert!(free > 12.0 - amp && free <= 12.0 + amp + 1e-4, "the free end swings within its amplitude: {free}");
        // The texture coordinates are the box's own texels (the atlas rectangle at the origin here).
        assert!(verts.iter().all(|v| (0.0..=20.0).contains(&(v.uv.x * 20.0))));
    }

    /// A sprite of `w` × `h` with the given opaque pixels (`true`), all of one colour.
    fn painted(w: u32, h: u32, on: impl Fn(u32, u32) -> bool) -> Image {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                if on(x, y) {
                    let i = ((y * w + x) * 4) as usize;
                    rgba[i..i + 4].copy_from_slice(&[200, 150, 50, 255]);
                }
            }
        }
        Image { width: w, height: h, rgba }
    }

    #[test]
    fn thin_spokes_about_the_hub_are_sails_and_a_blob_is_not() {
        // A 20 x 20 box with a 1 px cross through its centre (the hub): 39 of 400 pixels.
        let spokes = painted(40, 40, |x, y| (10..30).contains(&x) && (10..30).contains(&y) && (x == 19 || x == 20 || y == 19 || y == 20));
        let look = box_look(&spokes, [10.0, 10.0, 30.0, 30.0]);
        assert!(sails_look(&look), "spokes look like sails: {look:?}");
        assert!(moves(&spokes, &Point { kind: Kind::Sails, rect: [10.0, 10.0, 30.0, 30.0] }));
        // A solid block of the box: a roof or a wall, not sails.
        let block = painted(40, 40, |x, y| (10..30).contains(&x) && (10..30).contains(&y));
        assert!(!sails_look(&box_look(&block, [10.0, 10.0, 30.0, 30.0])), "a solid box is not sails");
        // Sails crowded into one corner are not turning sails about a hub.
        let corner = painted(40, 40, |x, y| (10..16).contains(&x) && (10..16).contains(&y));
        assert!(!sails_look(&box_look(&corner, [10.0, 10.0, 30.0, 30.0])), "one quadrant is not symmetric");
        // Nothing opaque at all: nothing to move.
        assert!(!sails_look(&box_look(&painted(40, 40, |_, _| false), [10.0, 10.0, 30.0, 30.0])));
    }

    #[test]
    fn a_flag_is_cloth_unless_it_is_a_figure_sized_blob() {
        // Cloth: about half the box opaque.
        let cloth = painted(30, 30, |x, y| (4..16).contains(&x) && (4..24).contains(&y) && (x + y) % 2 == 0);
        assert!(cloth_look(&box_look(&cloth, [4.0, 4.0, 16.0, 24.0])));
        assert!(moves(&cloth, &Point { kind: Kind::Flag, rect: [4.0, 4.0, 16.0, 24.0] }));
        // A figure-sized opaque blob (over 70 % of the box): not a flag.
        let figure = painted(30, 30, |x, y| (4..16).contains(&x) && (4..24).contains(&y));
        assert!(!cloth_look(&box_look(&figure, [4.0, 4.0, 16.0, 24.0])), "a solid figure is no flag");
        assert!(!moves(&figure, &Point { kind: Kind::Flag, rect: [4.0, 4.0, 16.0, 24.0] }));
        // A box past the sprite's edge is clamped, not read out of the image.
        let edge = box_look(&figure, [20.0, 20.0, 60.0, 60.0]);
        assert_eq!(edge.area, 100, "clamped to the 30 x 30 sprite's corner");
    }

    #[test]
    fn the_living_map_is_off_by_default() {
        // With no setting and no environment variable, nothing is alive.
        if std::env::var("RAZDOR_LIVING").is_err() {
            set_prefs(false);
            assert!(!on());
            set_prefs(true);
            assert!(on(), "the setting turns it on");
            set_prefs(false);
        }
    }
}
