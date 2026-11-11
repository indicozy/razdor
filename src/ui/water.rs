//! Moving water (Razdor's, «Живая вода», `Settings::map_live_water`, off by default). Style only:
//! the water ripples, foam runs along the shores, the sky's colour lies on the water, and the
//! sailing armies and the hero's galley leave a fading wake. No rule, path, fog or save reads it.
//!
//! The shader's part is in `terrain` (the `LiveWater` branch of the neighbours shader: the water
//! mask, the ripple, the foam and the sky colour); the wakes are drawn by `world_view`. This module
//! keeps the switch and the pure maths both sides share, so they are tested without a window.

use razdor::local;
use serde::{Deserialize, Serialize};
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

/// The drift speed of a river at its narrowest (a bank next to the water): the flow map's full speed.
pub const SPEED_NARROW: f32 = 1.0;
/// The slowest a wide river drifts (its banks far away), a share of the full speed.
pub const SPEED_WIDE: f32 = 0.35;

/// Chessboard distance, in cells, from each cell to the nearest land (a cell off the map counts as land, so
/// water on the border has a bank). Land is 0; water reached by no land (none on the map) is `u32::MAX`.
/// Built once per map, with the surface codes (`terrain::cells`).
pub fn bank_distance(codes: &[u8], (w, h): (u32, u32)) -> Vec<u32> {
    let (w, h) = (w as usize, h as usize);
    let mut dist = vec![u32::MAX; codes.len()];
    let mut queue = std::collections::VecDeque::new();
    for (i, &c) in codes.iter().enumerate() {
        let wet = razdor::rules::map::Surface::from_code(c).is_some_and(razdor::rules::map::is_water);
        if !wet {
            dist[i] = 0;
            queue.push_back(i);
        }
    }
    // Water on the border touches the off-map land: its own cells at the edge are one step from it.
    for (i, d) in dist.iter_mut().enumerate() {
        let (x, y) = (i % w, i / w);
        if *d == u32::MAX && (x == 0 || y == 0 || x + 1 == w || y + 1 == h) {
            *d = 1;
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        let (x, y) = ((i % w) as i64, (i / w) as i64);
        let d = dist[i];
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                if dist[j] > d + 1 {
                    dist[j] = d + 1;
                    queue.push_back(j);
                }
            }
        }
    }
    dist
}

/// The bank distance (in steps) from which water is wide: a lake of about 13 cells across, or the sea. A river is
/// narrower (the original's rivers are a few cells across), so it is not wide and it flows.
pub const WIDE_BANK: u32 = 6;

/// How far (in steps) from the wide interior the water still counts as wide: its shore, which lies that far from
/// the middle of a lake of `WIDE_BANK` cells' reach. A river within it (a mouth) is wide as well.
pub const WIDE_REACH: u32 = 6;

/// Whether each water cell is in a wide body of water (a lake, the sea), not a river: within `WIDE_REACH` steps of
/// water `WIDE_BANK` steps from the bank (the interior). Built once per map, with `bank_distance`; the waves and the
/// sparkle go on the wide water only, and the wide water does not drift.
pub fn wide_water(bank: &[u32], (w, h): (u32, u32)) -> Vec<bool> {
    let (wu, hu) = (w as usize, h as usize);
    let mut dist = vec![u32::MAX; bank.len()];
    let mut queue = std::collections::VecDeque::new();
    for (i, &b) in bank.iter().enumerate() {
        if b >= WIDE_BANK {
            dist[i] = 0;
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        let d = dist[i];
        if d >= WIDE_REACH {
            continue;
        }
        let (x, y) = ((i % wu) as i64, (i / wu) as i64);
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= wu as i64 || ny >= hu as i64 {
                    continue;
                }
                let j = ny as usize * wu + nx as usize;
                if dist[j] > d + 1 && bank[j] > 0 {
                    dist[j] = d + 1;
                    queue.push_back(j);
                }
            }
        }
    }
    (0..bank.len()).map(|i| bank[i] > 0 && dist[i] <= WIDE_REACH).collect()
}

/// The drift speed of each cell's water (0..1, the `Flow` texture's blue channel): `SPEED_NARROW` next to a bank,
/// easing toward `SPEED_WIDE` as a river widens (a cell `d` from the bank runs at 1.6 − 0.25 d, clamped). Cells with
/// no flow (still water, land) and the wide water (a lake's or the sea's rim flows inward, but it does not drift)
/// are 0.
pub fn river_speed(codes: &[u8], size: (u32, u32), flow: &[(f32, f32)]) -> Vec<f32> {
    let bank = bank_distance(codes, size);
    let wide = wide_water(&bank, size);
    (0..codes.len())
        .map(|i| {
            let moving = flow.get(i).is_some_and(|&(x, y)| x != 0.0 || y != 0.0);
            if !moving || wide[i] {
                return 0.0;
            }
            let d = bank[i].min(1 << 16) as f32;
            (1.6 - 0.25 * d).clamp(SPEED_WIDE, SPEED_NARROW)
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

/// «Частота анимации воды» (`Settings::map_water_rate`): the moving water is redrawn every 1, 2 or 3 frames;
/// the frames between show the last two drawings blended in time (`picture_mix`). Flat map only.
// The variants name the row's choices («Каждый кадр», «Через кадр», «Каждый третий»).
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameRate {
    #[default]
    EveryFrame,
    EveryOther,
    EveryThird,
}

impl FrameRate {
    /// The frames per redraw of the water picture.
    pub fn frames(self) -> u32 {
        match self {
            FrameRate::EveryFrame => 1,
            FrameRate::EveryOther => 2,
            FrameRate::EveryThird => 3,
        }
    }

    /// The next setting in the row: every frame, every other, every third, every frame.
    pub fn next(self) -> FrameRate {
        match self {
            FrameRate::EveryFrame => FrameRate::EveryOther,
            FrameRate::EveryOther => FrameRate::EveryThird,
            FrameRate::EveryThird => FrameRate::EveryFrame,
        }
    }
}

/// «Качество воды» (`Settings::map_water_quality`): the water picture's size as a share of the screen's, in each
/// direction (1, 0.75, 0.5), upscaled linearly. Flat map only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    #[default]
    High,
    Medium,
    Low,
}

impl Quality {
    /// The picture's share of the screen in each direction.
    pub fn scale(self) -> f32 {
        match self {
            Quality::High => 1.0,
            Quality::Medium => 0.75,
            Quality::Low => 0.5,
        }
    }

    /// The next setting in the row: High, Medium, Low, High.
    pub fn next(self) -> Quality {
        match self {
            Quality::High => Quality::Medium,
            Quality::Medium => Quality::Low,
            Quality::Low => Quality::High,
        }
    }
}

local! {
    /// The frame rate and quality of the settings, taken in by `set_levels` (every frame, from `display`).
    static RATE: std::cell::Cell<FrameRate> = std::cell::Cell::new(FrameRate::EveryFrame);
    static QUALITY: std::cell::Cell<Quality> = std::cell::Cell::new(Quality::High);
    /// `RAZDOR_WATER_EVERY=1|2|3` and `RAZDOR_WATER_SCALE=<0.25..1>` override them (the snapshots and profiling).
    static ENV_EVERY: Option<u32> = std::env::var("RAZDOR_WATER_EVERY").ok().and_then(|v| v.trim().parse::<u32>().ok()).filter(|n| (1..=3).contains(n));
    static ENV_SCALE: Option<f32> = std::env::var("RAZDOR_WATER_SCALE").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|s| (0.25..=1.0).contains(s));
}

/// Takes the frame rate and the quality in (from the settings; the snapshots keep the defaults).
pub fn set_levels(rate: FrameRate, quality: Quality) {
    RATE.set(rate);
    QUALITY.set(quality);
}

/// The redraw interval of the water picture, in frames (1 is every frame; the env overrides the setting).
pub fn every() -> u32 {
    ENV_EVERY.with(|e| *e).unwrap_or_else(|| RATE.get().frames())
}

/// The water picture's share of the screen in each direction (1 is the screen's size; the env overrides it).
pub fn scale() -> f32 {
    ENV_SCALE.with(|e| *e).unwrap_or_else(|| QUALITY.get().scale())
}

/// The zoom band of the water's detail (`lod_detail`): the map's zoom (1 at the default, 0.4 the furthest out).
/// The ripple, the foam's lapping and the sparkle and shore waves fade out between these.
pub const LOD_OUT: f32 = 0.45;
/// See [`LOD_OUT`]: the detail is whole from this zoom up.
pub const LOD_IN: f32 = 0.7;

/// The water's detail at the map's `zoom`: 1 from `LOD_IN` up, 0 at `LOD_OUT` and below, and smooth between (no
/// pop as the map zooms). Zoomed out, the water keeps its texture, colour and foam; the moving details go.
pub fn lod_detail(zoom: f32) -> f32 {
    if !zoom.is_finite() {
        return 1.0;
    }
    smoothstep(LOD_OUT, LOD_IN, zoom)
}

/// Whether the water picture is redrawn this frame: `age` frames have passed since the picture it shows was
/// drawn (0 on the frame it was drawn), and it is redrawn every `every` frames.
pub fn redraw_due(age: u32, every: u32) -> bool {
    every <= 1 || age + 1 >= every
}

/// The blend of the two water pictures on this frame, from the older (`prev`, drawn at that water time) to the
/// newer (`cur`, drawn at `cur`), at water time `now`. The frame shows the water as it was one interval ago
/// (the newer picture's own time is reached when the next one is due), so the motion is smooth between redraws:
/// 0 at the redraw, 1 one interval on. Without an older picture, or when the clock did not go on, the newer.
pub fn picture_mix(now: f64, prev: Option<f64>, cur: f64) -> f32 {
    match prev {
        Some(p) if cur > p => ((now - cur) / (cur - p)).clamp(0.0, 1.0) as f32,
        _ => 1.0,
    }
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
    fn the_bank_distance_counts_the_steps_to_the_nearest_land() {
        // A 5 × 5 map of land (code 5) with a 3 × 3 block of water (code 0) in the middle.
        let mut codes = vec![5u8; 25];
        for y in 1..4 {
            for x in 1..4 {
                codes[y * 5 + x] = 0;
            }
        }
        let d = bank_distance(&codes, (5, 5));
        assert_eq!(d[0], 0, "land is 0");
        assert_eq!(d[6], 1, "the block's corner is a step from the land");
        assert_eq!(d[12], 2, "the block's centre is two steps from the land (chessboard)");
    }

    #[test]
    fn a_narrow_river_runs_at_full_speed_a_wide_one_slower_and_still_water_not_at_all() {
        // 9 × 9: land at x = 0 and x = 8, water between; the flow runs down the water (+y).
        let mut codes = vec![5u8; 81];
        for y in 0..9 {
            for x in 1..8 {
                codes[y * 9 + x] = 0;
            }
        }
        let mut flow = vec![(0.0f32, 0.0f32); 81];
        for y in 0..9 {
            for x in 1..8 {
                flow[y * 9 + x] = (0.0, 1.0);
            }
        }
        let speed = river_speed(&codes, (9, 9), &flow);
        assert_eq!(speed[4 * 9 + 1], SPEED_NARROW, "next to the bank: full speed");
        // The centre column is 4 cells from either bank: 1.6 − 0.25 · 4 = 0.6.
        assert!((speed[4 * 9 + 4] - 0.6).abs() < 1e-6, "wide water: {}", speed[4 * 9 + 4]);
        assert_eq!(speed[4 * 9], 0.0, "land is not water");
        // The same water with no flow (a lake's interior) stands still.
        let still = river_speed(&codes, (9, 9), &vec![(0.0, 0.0); 81]);
        assert!(still.iter().all(|&s| s == 0.0));
        // Speeds stay within the band, however wide the water gets.
        assert!(speed.iter().all(|&s| (0.0..=SPEED_NARROW).contains(&s)));
    }

    /// The redraw flags of `frames` frames at `every`, as the picture's age runs (0 on a redraw, then up).
    fn schedule(every: u32, frames: usize) -> Vec<bool> {
        let mut age = 0u32;
        let mut out = Vec::new();
        for frame in 0..frames {
            let due = frame == 0 || redraw_due(age, every);
            if due {
                age = 0;
            } else {
                age += 1;
            }
            out.push(due);
        }
        out
    }

    #[test]
    fn the_water_is_redrawn_every_frame_every_other_or_every_third_frame() {
        assert_eq!(schedule(1, 4), vec![true, true, true, true]);
        assert_eq!(schedule(2, 6), vec![true, false, true, false, true, false]);
        assert_eq!(schedule(3, 7), vec![true, false, false, true, false, false, true]);
        // The rates in the settings row, and their cycle.
        assert_eq!([FrameRate::EveryFrame, FrameRate::EveryOther, FrameRate::EveryThird].map(FrameRate::frames), [1, 2, 3]);
        assert_eq!(FrameRate::EveryThird.next(), FrameRate::EveryFrame);
        assert_eq!(Quality::High.next(), Quality::Medium);
        assert_eq!(Quality::Low.next(), Quality::High);
        assert_eq!([Quality::High, Quality::Medium, Quality::Low].map(Quality::scale), [1.0, 0.75, 0.5]);
    }

    #[test]
    fn the_blend_moves_from_the_older_picture_to_the_newer_one_over_an_interval() {
        // Pictures drawn at water times 10.0 (older) and 10.5 (newer): the frame at the newer's time shows the
        // older, one interval on it shows the newer, halfway between them the middle.
        assert_eq!(picture_mix(10.5, Some(10.0), 10.5), 0.0);
        assert!((picture_mix(10.75, Some(10.0), 10.5) - 0.5).abs() < 1e-6);
        assert_eq!(picture_mix(11.0, Some(10.0), 10.5), 1.0);
        assert_eq!(picture_mix(12.0, Some(10.0), 10.5), 1.0, "held past the interval, until the next redraw");
        // No older picture (the first, or after a change of view), or a clock that did not go on: the newer.
        assert_eq!(picture_mix(10.2, None, 10.5), 1.0);
        assert_eq!(picture_mix(10.2, Some(10.5), 10.5), 1.0);
    }

    #[test]
    fn a_lake_is_wide_water_and_a_narrow_river_is_not_and_only_the_river_drifts() {
        // 25 × 15 of land, with a 15 × 13 lake (wide: its middle is 7 steps from the land) and a one-cell river in
        // column 21 (narrow, out of the lake's reach: the window is 3 cells).
        let (w, h) = (25u32, 15u32);
        let mut codes = vec![5u8; 375];
        for y in 1..14 {
            for x in 1..16 {
                codes[y * 25 + x] = 0;
            }
            codes[y * 25 + 21] = 0;
        }
        let bank = bank_distance(&codes, (w, h));
        let wide = wide_water(&bank, (w, h));
        assert!(wide[7 * 25 + 8], "the lake's middle is wide");
        assert!(wide[26], "the lake's rim is wide water too");
        assert!(!wide[7 * 25 + 21], "the one-cell river is not wide");
        assert!(!wide[0], "land is not wide water");
        // Flow on all the water (the lake's rim flows inward, the river runs down); the speed is zero on the
        // lake only, and the narrow river runs at full speed.
        let flow: Vec<(f32, f32)> = codes.iter().map(|&c| if c == 0 { (0.0, 1.0) } else { (0.0, 0.0) }).collect();
        let speed = river_speed(&codes, (w, h), &flow);
        assert_eq!(speed[7 * 25 + 8], 0.0, "the lake does not drift");
        assert_eq!(speed[26], 0.0, "nor its rim");
        assert_eq!(speed[7 * 25 + 21], SPEED_NARROW, "the narrow river runs at full speed");
    }

    #[test]
    fn the_water_detail_fades_out_over_the_zoom_band_without_a_pop() {
        assert_eq!(lod_detail(1.0), 1.0, "the default zoom and in: the whole detail");
        assert_eq!(lod_detail(LOD_IN), 1.0);
        assert_eq!(lod_detail(0.4), 0.0, "the furthest zoom out: none of the moving detail");
        assert_eq!(lod_detail(LOD_OUT), 0.0);
        // Between the ends it only ever falls as the map zooms out, in small steps (no jump).
        let mut last = 1.0f32;
        let mut z = 1.0f32;
        while z >= 0.4 {
            let d = lod_detail(z);
            assert!(d <= last + 1e-6, "rises at zoom {z}: {last} -> {d}");
            assert!(last - d < 0.05, "a step of {} at zoom {z}", last - d);
            last = d;
            z -= 0.005;
        }
        assert!((lod_detail((LOD_OUT + LOD_IN) / 2.0) - 0.5).abs() < 1e-6, "halfway at the band's middle");
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
