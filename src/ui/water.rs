//! Moving water (Razdor's, «Живая вода», `Settings::map_live_water`, off by default). Style only:
//! the water ripples, foam runs along the shores, the sky's colour lies on the water, and the
//! sailing armies and the hero's galley leave a fading wake. No rule, path, fog or save reads it.
//!
//! The shader's part is in `terrain` (the `LiveWater` branch of the neighbours shader: the water
//! mask, the ripple, the foam and the sky colour); the wakes are drawn by `world_view`. This module
//! keeps the switch and the pure maths both sides share, so they are tested without a window.

use razdor::local;
use std::collections::{HashMap, VecDeque};

local! {
    /// The setting, taken in every frame (`set_prefs`).
    static LIVE: std::cell::Cell<bool> = std::cell::Cell::new(false);
    /// `RAZDOR_LIVE_WATER=1` turns the water on whatever the setting says (read once).
    static ENV_ON: bool = std::env::var("RAZDOR_LIVE_WATER").is_ok_and(|v| v.trim() == "1");
}

/// Takes the setting «Живая вода» in (every frame, from `display::follow_settings`).
pub fn set_prefs(on: bool) {
    LIVE.set(on || ENV_ON.with(|e| *e));
}

/// Whether the water is alive this frame (the setting, or `RAZDOR_LIVE_WATER=1`).
pub fn live() -> bool {
    LIVE.get()
}

/// The side of the water's noise texture (`noise_texels`): the lattice repeats every this many cells, as
/// the shaders' hash lattice did (`vnoise`), and a wrapped texture of this size keeps the same period.
pub const NOISE_SIZE: u32 = 64;

/// The hash the shaders' value noise uses at lattice point (x, y): `fract(sin(dot(q, (127.1, 311.7))) · 43758.5453)`,
/// in f32 as GLSL computes it. Only the noise texture's bytes come from it (the shaders no longer hash).
pub fn hash2_ref(x: f32, y: f32) -> f32 {
    let d = x * 127.1 + y * 311.7;
    // The shader's constant: the nearest f32 to 43758.5453 (clippy's shorter spelling of it).
    let s = d.sin() * 43_758.547;
    // GLSL's fract is x - floor(x) (Rust's `fract` keeps the sign).
    s - s.floor()
}

/// The water's noise texture: `NOISE_SIZE`² bytes, row by row, the lattice's hash at each texel (0..255).
/// Uploaded once with linear filtering and wrapping (`terrain::noise_texture`), so a lookup at the lattice
/// point's centre plus the smoothed fraction gives the same smooth value noise as the hash did.
pub fn noise_texels() -> Vec<u8> {
    let n = NOISE_SIZE;
    (0..n * n)
        .map(|i| {
            let (x, y) = ((i % n) as f32, (i / n) as f32);
            (hash2_ref(x, y) * 255.0).round().clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// The shaders' smooth value noise at `q`, as the hash computes it (`vnoise` in the mist and the unlit shader).
#[cfg(test)]
pub fn vnoise_ref(q: (f32, f32)) -> f32 {
    let n = NOISE_SIZE as f32;
    let (ix, iy) = (q.0.floor(), q.1.floor());
    let (fx, fy) = (q.0 - ix, q.1 - iy);
    let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ax, ay) = (ix.rem_euclid(n), iy.rem_euclid(n));
    let (bx, by) = ((ix + 1.0).rem_euclid(n), (iy + 1.0).rem_euclid(n));
    let h = |x: f32, y: f32| hash2_ref(x, y);
    let lo = h(ax, ay) + (h(bx, ay) - h(ax, ay)) * ux;
    let hi = h(ax, by) + (h(bx, by) - h(ax, by)) * ux;
    lo + (hi - lo) * uy
}

/// The water's noise as the shader reads it from the texture: the texel at the lattice point's centre
/// plus the smoothed fraction, interpolated bilinearly with the texture's bytes (`noise_texels`) and wrapped.
/// The shader's `wnoise` is this lookup; the tests pin it to `vnoise_ref` within the byte's rounding.
#[cfg(test)]
pub fn wnoise_ref(table: &[u8], q: (f32, f32)) -> f32 {
    let n = NOISE_SIZE as i64;
    let texel = |x: i64, y: i64| table[(y.rem_euclid(n) * n + x.rem_euclid(n)) as usize] as f32 / 255.0;
    let (ix, iy) = (q.0.floor(), q.1.floor());
    let (fx, fy) = (q.0 - ix, q.1 - iy);
    let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ax, ay) = (ix as i64, iy as i64);
    let lo = texel(ax, ay) + (texel(ax + 1, ay) - texel(ax, ay)) * ux;
    let hi = texel(ax, ay + 1) + (texel(ax + 1, ay + 1) - texel(ax, ay + 1)) * ux;
    lo + (hi - lo) * uy
}

#[allow(dead_code)] // Used by `foam_edge` only, which the shader's formula mirrors and the tests pin.
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The main foam line's alpha at `s` (the water's distance from the waterline, in cells) for the wave's phase `lap`
/// (0 low, 1 high): near-white on the waterline, fading over about a third of a cell, its inner edge laps between the
/// waterline and a tenth of a cell out. The shader's formula, mirrored without the noise and the still-shore weight.
#[allow(dead_code)] // The shader's formula in Rust, pinned by the tests.
pub fn foam_line(s: f32, lap: f32) -> f32 {
    if s <= 0.0 {
        return 0.0;
    }
    let inner = 0.1 * lap;
    smoothstep(inner - 0.015, inner + 0.015, s) * (1.0 - smoothstep(inner + 0.08, inner + 0.33, s))
}

/// The fainter second line's alpha at `s` for the wave's phase `lap`, a little further out (about 0.36 to 0.41 cell).
#[allow(dead_code)] // The shader's formula in Rust, pinned by the tests.
pub fn foam_far(s: f32, lap: f32) -> f32 {
    let inner = 0.36 + 0.05 * lap;
    smoothstep(inner - 0.01, inner + 0.01, s) * (1.0 - smoothstep(inner + 0.03, inner + 0.07, s))
}

/// Seconds between two wake points.
pub const WAKE_STEP: f64 = 0.12;
/// Seconds a wake point stays visible: it fades from the first frame to nothing at this age.
pub const WAKE_LIFE: f64 = 1.5;
/// The points a wake keeps (the last `WAKE_POINTS` samples, about the fade's length).
pub const WAKE_POINTS: usize = 12;

/// One recorded display position of a sailing figure, with the time it was taken.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub at: (f32, f32),
    pub t: f64,
}

/// The alpha (0.35 down to 0) and the size in cells (0.3 up to 0.7) of a wake point of `age`
/// seconds, `None` once it has faded.
pub fn wake_fade(age: f64) -> Option<(f32, f32)> {
    if !(0.0..WAKE_LIFE).contains(&age) {
        return None;
    }
    let k = (age / WAKE_LIFE) as f32;
    Some((0.35 * (1.0 - k), 0.3 + 0.4 * k))
}

/// The recent positions of one sailing figure, oldest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wake {
    points: VecDeque<Point>,
}

impl Wake {
    /// Records `at` at time `now` if the last point is `WAKE_STEP` or more old (a clock that went
    /// back starts the wake again). The oldest point goes when `WAKE_POINTS` are kept.
    pub fn record(&mut self, at: (f32, f32), now: f64) {
        if self.points.back().is_some_and(|p| now < p.t) {
            self.points.clear();
        }
        if self.points.back().is_some_and(|p| now - p.t < WAKE_STEP) {
            return;
        }
        if self.points.len() == WAKE_POINTS {
            self.points.pop_front();
        }
        self.points.push_back(Point { at, t: now });
    }

    /// The points still visible at `now`, each with its alpha and size (`wake_fade`).
    pub fn visible(&self, now: f64) -> impl Iterator<Item = (Point, f32, f32)> + '_ {
        self.points.iter().filter_map(move |p| {
            let (alpha, size) = wake_fade(now - p.t)?;
            Some((*p, alpha, size))
        })
    }

    /// Whether any point is still visible at `now`.
    pub fn alive(&self, now: f64) -> bool {
        self.points.iter().any(|p| wake_fade(now - p.t).is_some())
    }
}

/// The wakes of the map, keyed by army index (the hero's is `HERO`).
#[derive(Clone, Debug, Default)]
pub struct Wakes {
    trails: HashMap<usize, Wake>,
}

/// The key of the hero's galley's wake.
pub const HERO: usize = usize::MAX;

impl Wakes {
    /// Records a point of figure `key` at `now`.
    pub fn record(&mut self, key: usize, at: (f32, f32), now: f64) {
        self.trails.entry(key).or_default().record(at, now);
    }

    /// Every wake, with the key of its figure.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &Wake)> + '_ {
        self.trails.iter().map(|(k, w)| (*k, w))
    }

    /// Drops the wakes with no point left at `now`.
    pub fn prune(&mut self, now: f64) {
        self.trails.retain(|_, w| w.alive(now));
    }

    /// Forgets every wake (a game was loaded or started).
    pub fn clear(&mut self) {
        self.trails.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_foam_line_starts_on_the_waterline_and_fades_into_the_water_by_a_third_of_a_cell() {
        assert_eq!(foam_line(0.0, 0.0), 0.0, "no foam on the land side");
        assert!(foam_line(0.02, 0.0) > 0.99, "full at the waterline: {}", foam_line(0.02, 0.0));
        let mid = foam_line(0.2, 0.0);
        assert!(mid > 0.0 && mid < 1.0, "fading: {mid}");
        assert!(foam_line(0.34, 0.0) < 1e-3, "gone by a third of a cell");
    }

    #[test]
    fn the_wave_moves_the_line_between_the_waterline_and_a_tenth_of_a_cell_out() {
        // The high point of the wave clears the waterline: the line starts a tenth of a cell out.
        assert!(foam_line(0.05, 1.0) < 1e-3, "the waterline is clear at the high point: {}", foam_line(0.05, 1.0));
        assert!(foam_line(0.12, 1.0) > 0.99, "the line is out there at the high point: {}", foam_line(0.12, 1.0));
        assert!(foam_line(0.02, 0.0) > foam_line(0.02, 1.0), "the low point reaches the waterline");
    }

    #[test]
    fn the_second_line_is_further_out_and_lies_within_the_water() {
        assert!(foam_far(0.1, 0.5) < 1e-3, "not near the waterline");
        assert!(foam_far(0.4, 0.5) > 0.9, "the second line: {}", foam_far(0.4, 0.5));
        assert!(foam_far(0.5, 0.5) < 1e-3, "not in the deep water");
    }
    #[test]
    fn a_wake_point_fades_from_035_to_nothing_and_grows() {
        let (a0, s0) = wake_fade(0.0).expect("fresh point");
        assert!((a0 - 0.35).abs() < 1e-6 && (s0 - 0.3).abs() < 1e-6);
        let (a1, s1) = wake_fade(0.75).expect("half way");
        assert!((a1 - 0.175).abs() < 1e-5 && (s1 - 0.5).abs() < 1e-5);
        assert!(wake_fade(WAKE_LIFE).is_none(), "gone at the full life");
        assert!(wake_fade(-0.1).is_none(), "a point from the future is not drawn");
    }

    #[test]
    fn a_wake_keeps_the_last_points_spaced_by_the_step() {
        let mut w = Wake::default();
        for i in 0..100 {
            // Frames every 16 ms: a point every WAKE_STEP only.
            w.record((i as f32, 0.0), i as f64 * 0.016);
        }
        assert!(w.points.len() <= WAKE_POINTS);
        let times: Vec<f64> = w.points.iter().map(|p| p.t).collect();
        for pair in times.windows(2) {
            assert!(pair[1] - pair[0] >= WAKE_STEP - 1e-9, "spaced: {pair:?}");
        }
    }

    #[test]
    fn a_wake_starts_again_when_the_clock_goes_back() {
        let mut w = Wake::default();
        w.record((1.0, 1.0), 10.0);
        w.record((2.0, 2.0), 10.5);
        w.record((3.0, 3.0), 4.0);
        assert_eq!(w.points.len(), 1);
        assert_eq!(w.points[0].at, (3.0, 3.0));
    }

    #[test]
    fn wakes_drop_the_figures_with_no_point_left() {
        let mut wakes = Wakes::default();
        wakes.record(0, (0.0, 0.0), 0.0);
        wakes.record(HERO, (5.0, 5.0), 1.5);
        wakes.prune(1.6);
        let keys: Vec<usize> = wakes.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec![HERO], "the army's point is 1.6 s old, gone at 1.5 s; the hero's is 0.1 s old");
        wakes.clear();
        assert_eq!(wakes.iter().count(), 0);
    }

    #[test]
    fn the_noise_texture_is_the_lattice_hash_and_repeats_every_64_cells() {
        let table = noise_texels();
        assert_eq!(table.len(), (NOISE_SIZE * NOISE_SIZE) as usize);
        let n = NOISE_SIZE as usize;
        assert_eq!(table[3 * n + 5], (hash2_ref(5.0, 3.0) * 255.0).round() as u8, "texel (5, 3) holds the hash at (5, 3)");
        // The shaders' noise has a period of 64 cells: a point and the one 64 cells on hold the same value.
        for q in [(0.3, 1.7), (12.25, -4.5), (63.9, 40.1)] {
            let a = vnoise_ref(q);
            let b = vnoise_ref((q.0 + 64.0, q.1 - 64.0));
            assert!((a - b).abs() < 1e-3, "period at {q:?}: {a} vs {b}");
        }
    }

    #[test]
    fn the_texture_lookup_gives_the_hash_value_noise_within_a_byte() {
        // The GPU's bilinear step rounds each texel to a byte: the lookup stays within half a byte per
        // texel of the hash's smooth noise, everywhere (across the wrap at 64 too).
        let table = noise_texels();
        let mut worst = 0.0f32;
        for k in 0..400 {
            let q = (k as f32 * 0.7311 - 40.0, (k * k) as f32 * 0.0137 - 30.0);
            let d = (wnoise_ref(&table, q) - vnoise_ref(q)).abs();
            worst = worst.max(d);
        }
        assert!(worst <= 1.0 / 255.0 + 1e-3, "the lookup differs by {worst}");
    }

    #[test]
    fn the_switch_is_off_until_it_is_set() {
        set_prefs(false);
        if !ENV_ON.with(|e| *e) {
            assert!(!live());
        }
        set_prefs(true);
        assert!(live());
        set_prefs(false);
    }
}
