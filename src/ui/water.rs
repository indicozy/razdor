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
