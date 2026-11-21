//! Razdor's «Новогодний вид» (`Settings::map_holiday`, off by default): a winter and holiday look for
//! the world map and the main menu. Style only: no rule, path, AI, fog, save or replay reads it.
//!
//! This module holds the setting's per-frame switch and the pure season logic (tested without a
//! window). Each effect reads [`on`] / [`cheap`] in the module that draws it. The main menu asks once
//! per season (from 1 December to 15 January), and the season of a January date is the year before.

// The look's readers come in Tasks 2-8; until they land, nothing here is called outside the tests.
#![allow(dead_code)]

use std::cell::Cell;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use macroquad::prelude::{FilterMode, Image, Texture2D};
use razdor::local;
use razdor::rules::marks::civil;

use super::audio::Settings;
use super::living;
use razdor::dt::lights::Kind;

local! {
    /// The setting, taken in every frame (`set_pref`).
    static ON: Cell<bool> = Cell::new(false);
    /// The cheap path (the light quality is «Низкое» or the battery saver is on), taken in with `set_pref`.
    static CHEAP: Cell<bool> = Cell::new(false);
    /// `RAZDOR_HOLIDAY=1` turns the look on whatever the setting says (read once).
    static ENV_ON: bool = std::env::var("RAZDOR_HOLIDAY").is_ok_and(|v| v.trim() == "1");
}

/// Takes the setting «Новогодний вид» in, and whether the cheap path applies (every frame, from
/// `display::follow_settings`).
pub fn set_pref(on: bool, cheap: bool) {
    ON.set(on || ENV_ON.with(|e| *e));
    CHEAP.set(cheap);
}

/// Whether the look is on this frame (the setting, or `RAZDOR_HOLIDAY=1`).
pub fn on() -> bool {
    ON.get()
}

/// Whether the look runs its cheap path: on, and the light quality is «Низкое» or the battery saver is on.
pub fn cheap() -> bool {
    on() && CHEAP.get()
}

/// The season a date falls in, named by its year: 1 December to 31 December of `y` is `Some(y)`,
/// 1 January to 15 January of `y` is `Some(y - 1)`, every other date is `None` (no season).
pub fn season(y: i64, m: u32, d: u32) -> Option<i64> {
    match (m, d) {
        (12, _) => Some(y),
        (1, 1..=15) => Some(y - 1),
        _ => None,
    }
}

/// Whether the holiday question is asked today: a date in a season that was not answered yet
/// (`asked` holds the season year answered last).
pub fn should_ask(asked: Option<i64>, today: (i64, u32, u32)) -> bool {
    match season(today.0, today.1, today.2) {
        Some(year) => asked != Some(year),
        None => false,
    }
}

/// Whether the main menu asks the holiday question now: today is in a season the settings do not
/// have an answer for yet (`holiday_asked`).
pub fn holiday_question_due(s: &Settings, today: (i64, u32, u32)) -> bool {
    should_ask(s.holiday_asked, today)
}

/// Today's date (UTC) as (year, month, day).
pub fn today() -> (i64, u32, u32) {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    civil((secs / 86_400) as i64)
}

/// The garlands' points on a building's light boxes (`garland_points`): sprite pixels, the colour index (0 red,
/// 1 green, 2 gold, 3 blue) and the dot's index in its box (the chase's order).
pub type Garland = (f32, f32, u8, u8);

/// Spacing of the garland dots along a box's edges, in sprite pixels.
pub const GARLAND_STEP: i32 = 6;

/// The garland dots of a building's light boxes: each window, door and gate box (`[x0, y0, x1, y1]`) gets a frame
/// of dots, one every [`GARLAND_STEP`] px: along the top edge `y0` from `x0` to `x1` (both ends included), down the
/// right side `x1`, then up the left side `x0` (the chase runs clockwise). The dot index restarts in each box and
/// the colour is the index mod 4. Chimneys, flags, sails and the other kinds give none.
pub fn garland_points(boxes: &[(Kind, [i32; 4])]) -> Vec<Garland> {
    let mut out = Vec::new();
    for (kind, [x0, y0, x1, y1]) in boxes {
        if !matches!(kind, Kind::Window | Kind::Door | Kind::Gate) {
            continue;
        }
        let mut frame: Vec<(i32, i32)> = Vec::new();
        let mut x = *x0;
        while x <= *x1 {
            frame.push((x, *y0));
            x += GARLAND_STEP;
        }
        let rows: Vec<i32> = (1..).map(|k| y0 + k * GARLAND_STEP).take_while(|y| y < y1).collect();
        frame.extend(rows.iter().map(|&y| (*x1, y)));
        frame.extend(rows.iter().rev().map(|&y| (*x0, y)));
        for (i, (x, y)) in frame.into_iter().enumerate() {
            out.push((x as f32, y as f32, (i % 4) as u8, i as u8));
        }
    }
    out
}

/// The chase's brightness of dot `index` at time `t` seconds: a wave of light runs along the frame, one dot a
/// quarter period behind the last, from 0.35 (faint) to 1 (full).
pub fn chase(t: f64, index: u8) -> f32 {
    (0.35 + 0.65 * (std::f64::consts::TAU * (t * 0.8 - f64::from(index) / 4.0)).sin().max(0.0)) as f32
}

/// Seconds between two presents dropped from one sleigh.
pub const PRESENT_EVERY: f64 = 1.5;
/// Seconds a present takes to fall.
pub const PRESENT_FALL: f64 = 0.8;
/// Seconds a landed present takes to fade out.
pub const PRESENT_FADE: f64 = 0.3;
/// How far a present falls, in screen pixels.
pub const PRESENT_DROP_PX: f32 = 40.0;
/// The gallop's frames per second (four frames in a cycle).
const GALLOP_FPS: f64 = 8.0;

/// Whether the sleigh flies left (its sprite is flipped): the course runs to the left.
pub fn sleigh_flies_left(from: (f32, f32), to: (f32, f32)) -> bool {
    to.0 < from.0
}

/// The gallop frame (0 to 3) at time `t` seconds: the frames change 8 times a second.
pub fn gallop_frame(t: f64) -> usize {
    ((t * GALLOP_FPS).floor() as i64).rem_euclid(4) as usize
}

/// The times of the presents one trip drops (the trip starts at `start` and lasts `trip` seconds): one every
/// [`PRESENT_EVERY`] seconds, each in the middle of its stretch, none at or after the trip's end.
pub fn present_drop_times(start: f64, trip: f64) -> Vec<f64> {
    (0..).map(|k| start + PRESENT_EVERY * (k as f64 + 0.5)).take_while(|t| *t < start + trip).collect()
}

/// A present's sideways drift in screen pixels (-6 to 6), from its flock's seed and its drop's number.
pub fn present_drift(seed: u64, k: usize) -> f32 {
    (living::unit(seed, 100 + k as u64) - 0.5) * 12.0
}

/// A present `age` seconds after its drop, with its drift: its screen offset (x, y in pixels, y down) and its
/// alpha. None before the drop and once the fade is over. It falls straight down at a steady pace, then fades.
pub fn present_fall(age: f64, drift: f32) -> Option<(f32, f32, f32)> {
    if !(0.0..PRESENT_FALL + PRESENT_FADE).contains(&age) {
        return None;
    }
    let f = (age / PRESENT_FALL).min(1.0) as f32;
    let alpha = if age <= PRESENT_FALL { 1.0 } else { 1.0 - ((age - PRESENT_FALL) / PRESENT_FADE) as f32 };
    Some((drift * f, PRESENT_DROP_PX * f, alpha))
}

/// Santa's sleigh's pictures: four gallop frames (flying right) and a present.
#[derive(Clone)]
pub struct Sleigh {
    pub frames: [Texture2D; 4],
    pub present: Texture2D,
}

/// Where the sleigh's pictures are: `RAZDOR_HOLIDAY_SLEIGH` (a folder), else the hub's `holiday/sleigh`.
fn sleigh_dir() -> Option<PathBuf> {
    std::env::var_os("RAZDOR_HOLIDAY_SLEIGH")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| razdor::hub::local_dir().map(|local| razdor::hub::holiday_sleigh_dir(&local)))
}

/// Loads the sleigh's pictures (nearest filtering), or None when one of them is missing or broken.
fn load_sleigh() -> Option<Sleigh> {
    let dir = sleigh_dir()?;
    let pic = |name: &str| -> Option<Texture2D> {
        let bytes = std::fs::read(dir.join(name)).ok()?;
        let tex = Texture2D::from_image(&Image::from_file_with_format(&bytes, None).ok()?);
        tex.set_filter(FilterMode::Nearest);
        Some(tex)
    };
    let frames = [pic("frame-0.png")?, pic("frame-1.png")?, pic("frame-2.png")?, pic("frame-3.png")?];
    Some(Sleigh { frames, present: pic("present.png")? })
}

/// The sleigh's pictures, loaded on first use. None when they are missing or broken: the birds fly instead.
pub fn sleigh() -> Option<Sleigh> {
    local! {
        static SLEIGH: Option<Sleigh> = load_sleigh();
    }
    SLEIGH.with(Clone::clone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_season_runs_from_1_december_to_15_january() {
        assert_eq!(season(2026, 12, 1), Some(2026));
        assert_eq!(season(2027, 1, 15), Some(2026));
        assert_eq!(season(2027, 1, 16), None);
        assert_eq!(season(2026, 11, 30), None);
    }

    #[test]
    fn the_question_is_asked_once_per_season() {
        assert!(should_ask(None, (2026, 12, 5)), "a new season, never answered");
        assert!(!should_ask(Some(2026), (2027, 1, 3)), "the same season, answered: not again");
        assert!(should_ask(Some(2026), (2027, 12, 1)), "the next December: asked again");
        assert!(!should_ask(None, (2026, 7, 1)), "outside the season: no question");
    }

    #[test]
    fn the_holiday_question_is_due_once_per_season() {
        let mut s = Settings::default();
        assert!(holiday_question_due(&s, (2026, 12, 1)), "the first December visit asks");
        s.holiday_asked = Some(2026);
        assert!(!holiday_question_due(&s, (2027, 1, 3)), "answered for this season: not again");
        assert!(!holiday_question_due(&s, (2026, 7, 1)), "July: no question");
    }

    #[test]
    fn a_frame_runs_clockwise_from_the_top_left_corner() {
        // Top edge left to right, then the right side down, then the left side up: 7 dots for a 24 x 10 box.
        let pts = garland_points(&[(Kind::Window, [10, 20, 34, 30])]);
        assert_eq!(
            pts,
            vec![(10.0, 20.0, 0, 0), (16.0, 20.0, 1, 1), (22.0, 20.0, 2, 2), (28.0, 20.0, 3, 3), (34.0, 20.0, 0, 4), (34.0, 26.0, 1, 5), (10.0, 26.0, 2, 6)],
            "the dot index runs along the frame, the colour cycles by it"
        );
    }

    #[test]
    fn a_short_box_gets_only_its_top_edge() {
        let pts = garland_points(&[(Kind::Window, [0, 0, 12, 4])]);
        assert_eq!(pts.len(), 3, "a 4-px box has no side dots: {pts:?}");
    }

    #[test]
    fn the_chase_wave_travels_along_the_frame() {
        // Dot i lags dot 0 by i/4 of a wave: 1/4 / 0.8 s per dot.
        for i in 0..4u8 {
            for k in 0..20 {
                let t = 3.0 + k as f64 * 0.37;
                let lag = f64::from(i) * 0.25 / 0.8;
                assert!((chase(t, i) - chase(t - lag, 0)).abs() < 1e-5, "dot {i} at {t}");
            }
        }
    }

    #[test]
    fn the_chase_stays_between_faint_and_full() {
        let vals: Vec<f32> = (0..400).map(|k| chase(f64::from(k) * 0.01, 0)).collect();
        assert!(vals.iter().all(|v| (0.35..=1.0).contains(v)), "{vals:?}");
        assert!(vals.iter().any(|v| *v > 0.99), "the wave reaches full brightness");
    }

    #[test]
    fn doors_and_gates_get_points_too() {
        assert!(!garland_points(&[(Kind::Door, [0, 5, 12, 9])]).is_empty());
        assert!(!garland_points(&[(Kind::Gate, [0, 5, 12, 9])]).is_empty());
    }

    #[test]
    fn chimneys_flags_and_sails_give_no_garlands() {
        assert!(garland_points(&[(Kind::Chimney, [0, 0, 24, 4]), (Kind::Flag, [0, 0, 24, 4]), (Kind::Sails, [0, 0, 24, 4])]).is_empty());
    }

    #[test]
    fn no_boxes_give_no_garlands() {
        assert!(garland_points(&[]).is_empty());
    }

    #[test]
    fn the_dot_index_restarts_on_each_box() {
        let pts = garland_points(&[(Kind::Window, [0, 0, 0, 4]), (Kind::Window, [0, 9, 0, 13])]);
        let idx: Vec<u8> = pts.iter().map(|p| p.3).collect();
        assert_eq!(idx, vec![0, 0], "one point per single-px box, each its own chase");
    }

    #[test]
    fn the_cheap_path_needs_the_look_on() {
        set_pref(false, true);
        assert!(!cheap(), "the look off: no cheap path to take");
        set_pref(true, true);
        assert!(cheap());
        set_pref(true, false);
        assert!(!cheap());
        set_pref(false, false);
    }

    #[test]
    fn the_sleigh_faces_the_way_it_flies() {
        assert!(sleigh_flies_left((30.0, 5.0), (10.0, 5.0)), "a course to the left: mirrored");
        assert!(!sleigh_flies_left((10.0, 5.0), (30.0, 5.0)), "a course to the right: as drawn");
        assert!(!sleigh_flies_left((10.0, 5.0), (10.0, 9.0)), "straight down: not mirrored");
    }

    #[test]
    fn the_gallop_runs_four_frames_at_eight_a_second() {
        let frames: Vec<usize> = (0..10).map(|k| gallop_frame(f64::from(k) * 0.125)).collect();
        assert_eq!(frames, vec![0, 1, 2, 3, 0, 1, 2, 3, 0, 1]);
        assert_eq!(gallop_frame(100.0), gallop_frame(100.0 + 0.5), "the cycle is 0.5 s");
        assert!(gallop_frame(-0.01) < 4, "before time 0 it still cycles");
    }

    #[test]
    fn a_trip_drops_a_present_every_1_5_seconds_and_none_after_it_ends() {
        let drops = present_drop_times(100.0, living::BIRD_TRIP);
        assert_eq!(drops.len(), 9, "1.5, 3, ... 13.5 s into a 14 s trip: {drops:?}");
        assert!((drops[0] - 100.75).abs() < 1e-9, "the first one at the middle of its stretch");
        for w in drops.windows(2) {
            assert!((w[1] - w[0] - PRESENT_EVERY).abs() < 1e-9, "one every 1.5 s");
        }
        assert!(drops.iter().all(|t| *t >= 100.0 && *t < 100.0 + living::BIRD_TRIP));
        assert_eq!(drops, present_drop_times(100.0, living::BIRD_TRIP), "the same trip, the same drops");
        assert!(present_drop_times(100.0, 0.5).is_empty(), "a short trip drops none");
    }

    #[test]
    fn a_present_drifts_a_little_from_its_flocks_seed() {
        for k in 0..20 {
            let d = present_drift(0xABCD, k);
            assert!((-6.0..6.0).contains(&d), "a slight drift: {d}");
            assert_eq!(d, present_drift(0xABCD, k), "deterministic");
        }
        assert_ne!(present_drift(1, 0), present_drift(2, 0), "another flock, another drift");
    }

    #[test]
    fn a_present_falls_forty_pixels_then_fades() {
        assert_eq!(present_fall(-0.1, 0.0), None, "not dropped yet");
        assert_eq!(present_fall(0.0, 3.0), Some((0.0, 0.0, 1.0)), "at the drop: its place");
        let (dx, dy, a) = present_fall(PRESENT_FALL, 3.0).unwrap();
        assert!((dy - 40.0).abs() < 1e-4 && (dx - 3.0).abs() < 1e-4 && a == 1.0, "landed: 40 px down, full");
        let (_, _, a) = present_fall(PRESENT_FALL + PRESENT_FADE / 2.0, 0.0).unwrap();
        assert!((a - 0.5).abs() < 1e-4, "half faded halfway through the fade");
        assert_eq!(present_fall(PRESENT_FALL + PRESENT_FADE, 0.0), None, "gone after the fade");
    }
}
